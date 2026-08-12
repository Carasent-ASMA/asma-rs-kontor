//! Bounded holiday imports: parse, normalize, diff, plan.
//!
//! Every function here is pure. Bytes come in as a `&str` — from
//! [`crate::fetch::retrieve`] or from a file a caller read — and immutable
//! revisions come out. Nothing in this module opens a socket, reads a clock or
//! touches a store, which is what lets a preview be shown to a reviewer without
//! anything having changed yet, and what lets the same document be re-imported
//! years later to exactly the same result.
//!
//! ## The three shapes, and only three
//!
//! | Kind | Document | Coverage |
//! |---|---|---|
//! | [`HolidayImportKind::NagerV4`] | the Nager holiday API's JSON array | national sets such as `US`, `MD`, `NO` |
//! | [`HolidayImportKind::GovUkJson`] | GOV.UK `bank-holidays.json` | `GB-ENG`, `GB-WLS`, `GB-SCT`, `GB-NIR` |
//! | [`HolidayImportKind::Ical`] | an iCalendar document | any all-day feed, local file or URL |
//!
//! An importer for a fourth shape is a new function beside these, not a new
//! branch inside them.
//!
//! ## What is refused, and how
//!
//! A document that is not the shape its importer expects is refused whole:
//! [`CalendarError::Malformed`]. A single *entry* that cannot be imported —
//! timed rather than all-day, recurring, out of the requested range, of a
//! category nobody selected, for a subdivision nobody asked about, or a repeat
//! of an identity already seen — is dropped with a stable
//! [`ImportWarningCode`] and its position in the document. Positions, never
//! values: an import warning is stored and exported, and a source document is
//! not ours to echo.

pub mod gov_uk;
pub mod ics;
pub mod nager;

use std::collections::{BTreeSet, HashSet};

use jiff::civil;
use kontor_core::calendar::{
    CalendarExceptionRevision, CountryCode, ExceptionKind, ExceptionProvenance, HolidayCategory,
    HolidayImportBatch, HolidayImportKind, HolidayProviderKind, HolidaySourceRevision,
    ImportWarning, ImportWarningCode, MAX_IMPORT_WARNINGS,
};
use kontor_core::id::{
    CalendarExceptionId, CalendarProfileId, ContentHash, ExternalId, ExternalName, HolidaySourceId,
    IdempotencyKey, ProjectId, SpecVersion, Timestamp, WorkCalendarId,
};
use serde::{Deserialize, Serialize};

use crate::{CalendarError, CalendarResult};

/// The largest document any importer will read.
///
/// Two mebibytes is several times the largest real national holiday feed and
/// well under anything that would make parsing a memory decision.
pub const MAX_DOCUMENT_BYTES: usize = 2 * 1024 * 1024;

/// The most entries one document may contain.
pub const MAX_ENTRIES: usize = 4096;

/// What one import was asked to retrieve.
///
/// The range and the categories are part of the *request*, not of the result,
/// and both are recorded with the applied import: without them a later reader
/// cannot tell "the source listed no holidays" from "the request filtered them
/// all out".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRequest {
    /// Which importer reads the document.
    pub kind: HolidayImportKind,
    /// The country the set belongs to.
    pub country: CountryCode,
    /// The subdivision, for a regional set. `None` means nationwide entries
    /// only: a regional day is not silently promoted to a national one.
    pub subdivision: Option<ExternalName>,
    /// First local date wanted, inclusive.
    pub range_start: civil::Date,
    /// Last local date wanted, inclusive.
    pub range_end: civil::Date,
    /// The categories to import. [`HolidayCategory::DEFAULT_SELECTION`] is
    /// public and bank holidays; anything else must be asked for by name.
    pub categories: BTreeSet<HolidayCategory>,
    /// A non-secret reference to where the document came from: a URL without
    /// credentials, or a file label.
    pub reference: ExternalName,
}

impl ImportRequest {
    /// Validate the request itself, before a document is read.
    ///
    /// # Errors
    /// Rejects an inverted or unbounded range and an empty category selection.
    pub fn validate(&self) -> CalendarResult<()> {
        match self.kind {
            HolidayImportKind::NagerV4 if !matches!(self.country.as_str(), "US" | "MD" | "NO") => {
                return Err(kontor_core::DomainError::invalid(
                    "ImportRequest",
                    "Nager v4 is limited to US, MD and NO in this build",
                )
                .into());
            }
            HolidayImportKind::GovUkJson if self.country.as_str() != "GB" => {
                return Err(kontor_core::DomainError::invalid(
                    "ImportRequest",
                    "GOV.UK JSON requires country GB",
                )
                .into());
            }
            _ => {}
        }
        if self.range_start > self.range_end {
            return Err(kontor_core::DomainError::invalid(
                "ImportRequest",
                "covers an inverted range",
            )
            .into());
        }
        let days = self
            .range_start
            .until(self.range_end)
            .map_err(|_| {
                kontor_core::DomainError::invalid("ImportRequest", "covers an unmeasurable span")
            })?
            .get_days();
        if i64::from(days) > kontor_core::calendar::MAX_IMPORT_DAYS {
            return Err(CalendarError::TooLarge);
        }
        if self.categories.is_empty() {
            return Err(kontor_core::DomainError::invalid(
                "ImportRequest",
                "selects no holiday category",
            )
            .into());
        }
        Ok(())
    }

    /// Whether a date lies in the requested range.
    #[must_use]
    pub fn covers(&self, date: civil::Date) -> bool {
        date >= self.range_start && date <= self.range_end
    }
}

/// One entry exactly as a source document stated it, before any filter.
///
/// Crate-internal: the three importers produce these, and one shared pass turns
/// them into normalized holidays. Keeping the filters in that one pass is what
/// stops "public and bank by default" from being implemented three times.
#[derive(Debug, Clone)]
pub(crate) struct SourceEntry {
    /// Zero-based position in the document, for warnings.
    pub position: u32,
    /// First local date, inclusive.
    pub start: civil::Date,
    /// Last local date, inclusive.
    pub end: civil::Date,
    /// The day's name in the source.
    pub label: ExternalName,
    /// What kind of day the source says it is.
    pub category: HolidayCategory,
    /// A stable identity for the entry within its source.
    pub identity: ExternalId,
    /// Subdivision codes the entry is limited to. Empty means nationwide.
    pub subdivisions: Vec<String>,
}

/// What an importer produced from one document, before any filter.
pub(crate) struct ParsedDocument {
    pub entries: Vec<SourceEntry>,
    pub warnings: Vec<ImportWarning>,
}

/// One normalized closure day.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NormalizedHoliday {
    /// First local date, inclusive.
    pub start: civil::Date,
    /// Last local date, inclusive.
    pub end: civil::Date,
    /// The day's name, as the source gave it.
    pub label: ExternalName,
    /// What kind of day it is.
    pub category: HolidayCategory,
    /// The source's stable identity for it.
    pub identity: ExternalId,
}

/// A parsed, normalized, not-yet-applied import.
///
/// Producing one changes nothing. That is the point: a preview is what a
/// reviewer looks at, and a reviewer who says no has cost the calendar nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportPreview {
    /// The request this preview answers.
    pub request: ImportRequest,
    /// The holidays that survived every filter, in a deterministic order.
    pub holidays: Vec<NormalizedHoliday>,
    /// What was refused or dropped, by position in the document.
    pub warnings: Vec<ImportWarning>,
    /// Digest of the document as retrieved.
    pub raw_hash: ContentHash,
    /// Digest of the normalized holidays.
    pub normalized_hash: ContentHash,
}

/// Parse and normalize one retrieved document. Pure.
///
/// # Errors
/// * [`CalendarError::Malformed`] when the document is not the shape the
///   requested importer reads.
/// * [`CalendarError::TooLarge`] when the document or its entry count exceeds
///   the import bounds.
/// * [`CalendarError::TooManyWarnings`] when so many entries were refused that
///   this is the wrong document rather than a document with a few bad rows.
pub fn preview(request: &ImportRequest, raw: &str) -> CalendarResult<ImportPreview> {
    request.validate()?;
    if raw.len() > MAX_DOCUMENT_BYTES {
        return Err(CalendarError::TooLarge);
    }
    let parsed = match request.kind {
        HolidayImportKind::NagerV4 => nager::parse(raw, &request.country)?,
        HolidayImportKind::GovUkJson => gov_uk::parse(raw, request.subdivision.as_ref())?,
        HolidayImportKind::Ical => ics::parse(raw)?,
    };
    normalize(request, parsed, raw)
}

/// Apply the request's filters, drop repeats, order and hash.
fn normalize(
    request: &ImportRequest,
    parsed: ParsedDocument,
    raw: &str,
) -> CalendarResult<ImportPreview> {
    if parsed.entries.len() > MAX_ENTRIES {
        return Err(CalendarError::TooLarge);
    }
    let mut warnings = parsed.warnings;
    let mut holidays = Vec::new();
    let mut seen: HashSet<ExternalId> = HashSet::new();
    for entry in parsed.entries {
        let mut refuse = |code: ImportWarningCode| {
            warnings.push(ImportWarning {
                code,
                entry: entry.position,
            });
        };
        if !request.categories.contains(&entry.category) {
            refuse(ImportWarningCode::FilteredCategory);
            continue;
        }
        // Wholly inside the requested range, not merely overlapping it. A
        // clamped multi-day closure would be a day the source never stated.
        if !request.covers(entry.start) || !request.covers(entry.end) {
            refuse(ImportWarningCode::OutOfRange);
            continue;
        }
        if !subdivision_matches(request.subdivision.as_ref(), &entry.subdivisions) {
            refuse(ImportWarningCode::FilteredSubdivision);
            continue;
        }
        if !seen.insert(entry.identity.clone()) {
            refuse(ImportWarningCode::DuplicateIdentity);
            continue;
        }
        holidays.push(NormalizedHoliday {
            start: entry.start,
            end: entry.end,
            label: entry.label,
            category: entry.category,
            identity: entry.identity,
        });
    }
    if warnings.len() > MAX_IMPORT_WARNINGS {
        return Err(CalendarError::TooManyWarnings);
    }
    // Deterministic order, so the digest below is a property of the holidays
    // and not of the order the document happened to list them in.
    holidays.sort();
    warnings.sort();
    let normalized = serde_json::to_vec(&holidays).map_err(|_| CalendarError::Malformed {
        subject: "normalized holiday set",
        rule: "could not be rendered for hashing",
    })?;
    Ok(ImportPreview {
        request: request.clone(),
        holidays,
        warnings,
        raw_hash: ContentHash::of(raw.as_bytes()),
        normalized_hash: ContentHash::of(&normalized),
    })
}

/// Make source text usable as an [`ExternalId`]: no whitespace, no control
/// characters, bounded length.
///
/// The identity is derived rather than invented so the same document always
/// produces the same one — a re-import of an unchanged year must not look like a
/// year of new holidays.
pub(crate) fn slug(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_whitespace() || character.is_control() {
                '-'
            } else {
                character
            }
        })
        .take(kontor_core::id::MAX_EXTERNAL_ID_LEN)
        .collect()
}

/// Whether an entry's subdivisions satisfy what the request asked for.
///
/// A nationwide entry (no subdivisions) always matches. A regional entry matches
/// only a request that named its region: importing one region's day as a
/// national closure would close the whole workspace for a day most of it works.
fn subdivision_matches(requested: Option<&ExternalName>, entry: &[String]) -> bool {
    if entry.is_empty() {
        return true;
    }
    requested.is_some_and(|code| entry.iter().any(|value| value == code.as_str()))
}

/// One imported day that changed between the applied import and a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HolidayChange {
    /// The exception revision currently applied.
    pub applied: CalendarExceptionRevision,
    /// What the new document says instead.
    pub incoming: NormalizedHoliday,
}

/// What applying a preview would do to a calendar.
///
/// This is the "refresh shows additions, removals and changes before apply"
/// answer, and it deliberately says nothing about manual exceptions: an import
/// does not add, change or remove a decision a human made.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportDiff {
    /// Days the new document lists that the applied import does not.
    pub added: Vec<NormalizedHoliday>,
    /// Days the applied import holds that the new document no longer lists.
    pub removed: Vec<CalendarExceptionRevision>,
    /// Days both hold, differently.
    pub changed: Vec<HolidayChange>,
    /// Days both hold identically.
    pub unchanged: u32,
}

impl ImportDiff {
    /// Whether applying this preview would change anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

/// Compare a preview against the exceptions a calendar has applied.
///
/// Days are matched on their local span, because that is the only key both
/// sides always have: the applied exception is a stored row with dates and a
/// label, and a source identity is not part of it. A same-span day with a
/// different name is therefore a change, and a moved day is a removal plus an
/// addition — which is exactly what a reviewer needs to see before approving.
#[must_use]
pub fn diff(preview: &ImportPreview, applied: &[CalendarExceptionRevision]) -> ImportDiff {
    let imported: Vec<&CalendarExceptionRevision> = applied
        .iter()
        .filter(|exception| {
            matches!(
                exception.provenance,
                ExceptionProvenance::HolidaySource { .. }
            )
        })
        .collect();
    let mut result = ImportDiff::default();
    for holiday in &preview.holidays {
        match imported
            .iter()
            .find(|applied| applied.start_date == holiday.start && applied.end_date == holiday.end)
        {
            None => result.added.push(holiday.clone()),
            Some(existing) if existing.label == holiday.label => result.unchanged += 1,
            Some(existing) => result.changed.push(HolidayChange {
                applied: (*existing).clone(),
                incoming: holiday.clone(),
            }),
        }
    }
    for existing in imported {
        let still_listed = preview.holidays.iter().any(|holiday| {
            holiday.start == existing.start_date && holiday.end == existing.end_date
        });
        if !still_listed {
            result.removed.push(existing.clone());
        }
    }
    result
}

/// The calendar an import is being planned for, and the identity of the apply.
#[derive(Debug, Clone)]
pub struct ImportTarget<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The calendar assignment.
    pub work_calendar_id: WorkCalendarId,
    /// The profile the assignment pins.
    pub profile_id: CalendarProfileId,
    /// The pinned revision.
    pub profile_version: SpecVersion,
    /// The exceptions currently applied, from
    /// [`kontor_core::repository::CalendarRepository::applied_exceptions`].
    pub applied: &'a [CalendarExceptionRevision],
    /// The source revision the applied import produced, if the calendar has one.
    /// The new import supersedes exactly this one.
    pub supersedes: Option<HolidaySourceId>,
    /// The caller's replay key.
    pub idempotency_key: IdempotencyKey,
    /// When the document was retrieved.
    pub retrieved_at: Timestamp,
    /// When the apply is being made.
    pub applied_at: Timestamp,
}

/// Everything one apply writes, ready for the store's single transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportApplication {
    /// The immutable source revision.
    pub revision: HolidaySourceRevision,
    /// Its provenance.
    pub batch: HolidayImportBatch,
    /// One exception revision per normalized holiday.
    pub exceptions: Vec<CalendarExceptionRevision>,
    /// What this apply changes, for the receipt an operator reads.
    pub diff: ImportDiff,
}

/// Turn a preview and the current state into the revisions an apply writes.
///
/// Nothing here is written; the caller hands the result to
/// [`kontor_core::repository::CalendarRepository::apply_holiday_import`], which
/// commits all of it or none of it.
///
/// # Errors
/// [`CalendarError::Domain`] when the revisions this would write do not satisfy
/// their own invariants.
pub fn plan(
    preview: &ImportPreview,
    target: &ImportTarget<'_>,
) -> CalendarResult<ImportApplication> {
    let source_id = HolidaySourceId::generate();
    // The revision records the span actually covered; the batch records the span
    // that was asked for. An import that found nothing still has the second.
    let covered_start = preview
        .holidays
        .first()
        .map_or(preview.request.range_start, |holiday| holiday.start);
    let covered_end = preview
        .holidays
        .iter()
        .map(|holiday| holiday.end)
        .max()
        .unwrap_or(preview.request.range_end);
    let revision = HolidaySourceRevision {
        id: source_id,
        profile_id: target.profile_id,
        profile_version: target.profile_version,
        provider: HolidayProviderKind::Ical,
        country: preview.request.country.clone(),
        subdivision: preview.request.subdivision.clone(),
        reference: preview.request.reference.clone(),
        range_start: covered_start,
        range_end: covered_end,
        retrieved_at: target.retrieved_at,
        raw_hash: preview.raw_hash.clone(),
        normalized_hash: preview.normalized_hash.clone(),
    };
    revision.validate()?;

    let difference = diff(preview, target.applied);
    let exceptions: Vec<CalendarExceptionRevision> = preview
        .holidays
        .iter()
        .map(|holiday| CalendarExceptionRevision {
            id: CalendarExceptionId::generate(),
            project_id: target.project_id,
            work_calendar_id: target.work_calendar_id,
            start_date: holiday.start,
            end_date: holiday.end,
            kind: ExceptionKind::Closed,
            label: holiday.label.clone(),
            provenance: ExceptionProvenance::HolidaySource { source_id },
            // Lineage: the revision this one replaces for the same span, so an
            // audit can follow a single day across refreshes.
            supersedes: superseded_by(holiday, target.applied),
            created_at: target.applied_at,
        })
        .collect();
    for exception in &exceptions {
        exception.validate()?;
    }

    let batch = HolidayImportBatch {
        source_id,
        project_id: target.project_id,
        work_calendar_id: target.work_calendar_id,
        kind: preview.request.kind,
        requested_start: preview.request.range_start,
        requested_end: preview.request.range_end,
        categories: preview.request.categories.clone(),
        warnings: preview.warnings.clone(),
        applied_exceptions: u32::try_from(exceptions.len()).unwrap_or(u32::MAX),
        supersedes: target.supersedes,
        idempotency_key: target.idempotency_key.clone(),
        applied_at: target.applied_at,
    };
    batch.validate()?;

    Ok(ImportApplication {
        revision,
        batch,
        exceptions,
        diff: difference,
    })
}

/// The applied imported exception one incoming holiday replaces, if any.
fn superseded_by(
    holiday: &NormalizedHoliday,
    applied: &[CalendarExceptionRevision],
) -> Option<CalendarExceptionId> {
    applied
        .iter()
        .find(|exception| {
            matches!(
                exception.provenance,
                ExceptionProvenance::HolidaySource { .. }
            ) && exception.start_date == holiday.start
                && exception.end_date == holiday.end
        })
        .map(|exception| exception.id)
}
