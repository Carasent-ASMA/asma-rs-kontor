//! The project/subject authority ledger and the one-way cutover it grants.
//!
//! Every native write to a project's memory or backlog passes
//! [`require_subject_authority`] first. The check is by `(project, subject)`, so
//! one project's cutover cannot make another project writable and a memory
//! switch cannot make a backlog writable.
//!
//! Nothing here decides *what* an import contains: memory and backlog keep their
//! own typed import bodies and their own readback computation. What they share is
//! this ledger, the manifest their switch is granted against, and the receipt
//! rules. That is why [`SqliteStore::switch_subject_authority`] takes a readback
//! hash rather than computing one — the hash has to come from whichever subject's
//! stored state is being proved, and a shared function that knew how to read both
//! would be the generic importer the design refuses.

use kontor_core::authority::{
    AuthoritySubject, ProjectSubjectAuthority, SubjectAuthority, SubjectOrigin,
};
use kontor_core::id::{AggregateRevision, ContentHash, ProjectId, Timestamp, parse_utc_timestamp};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SqliteStore;

/// Why an authority read, attestation or switch was refused.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    /// The project has no row for that subject.
    #[error("no authority row exists for this project and subject")]
    NotFound,
    /// The caller's revision is not the stored one.
    #[error("revision conflict: expected {expected}, current {current}")]
    RevisionConflict {
        /// What the caller presented.
        expected: u64,
        /// What is stored.
        current: u64,
    },
    /// The subject is not writable by Kontor yet.
    #[error("{subject} authority for this project is `{current}`; `kontor` is required")]
    Denied {
        /// The subject that refused.
        subject: AuthoritySubject,
        /// Who holds it now.
        current: SubjectAuthority,
    },
    /// A cutover rule refused.
    #[error("subject authority rule refused the operation: {0}")]
    Rule(&'static str),
    /// A domain value did not parse.
    #[error(transparent)]
    Domain(#[from] kontor_core::DomainError),
    /// SQLite refused or failed.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// Stored JSON is not valid.
    #[error("stored authority JSON is invalid")]
    Json(#[from] serde_json::Error),
    /// The existing backlog graph rejected an imported shape.
    #[error(transparent)]
    Repository(#[from] kontor_core::repository::RepositoryError),
}

/// One recorded authority operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectAuthorityReceipt {
    /// The receipt id.
    pub receipt_id: String,
    /// The project.
    pub project_id: ProjectId,
    /// The subject.
    pub subject: AuthoritySubject,
    /// What was recorded: `import`, `attest` or `switch`.
    pub operation: String,
    /// The canonical input the operation was granted against.
    pub input_hash: ContentHash,
    /// The resulting stored facts.
    pub result_hash: ContentHash,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// What one subject's legacy import carried, and what was durably here after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectImportManifest {
    /// The project.
    pub project_id: ProjectId,
    /// The subject.
    pub subject: AuthoritySubject,
    /// The legacy system the export came from.
    pub source: String,
    /// The canonical hash of the submitted export.
    pub import_hash: ContentHash,
    /// How many items it carried.
    pub imported_count: u64,
    /// Recomputed from stored Kontor state after the import committed.
    pub readback_hash: ContentHash,
    /// When it was imported.
    pub imported_at: Timestamp,
}

/// One import to record: what was submitted, and what stored state it left.
#[derive(Debug, Clone, Copy)]
pub struct SubjectImportRecord<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The subject.
    pub subject: AuthoritySubject,
    /// The legacy system the export came from.
    pub source: &'a str,
    /// The canonical hash of the submitted export.
    pub import_hash: &'a ContentHash,
    /// The canonical manifest, as stored.
    pub canonical_manifest: &'a str,
    /// How many items it carried.
    pub imported_count: u64,
    /// Recomputed from stored Kontor state after the import committed.
    pub readback_hash: &'a ContentHash,
}

/// The origins a project is created with, one per subject.
///
/// Both are required: a project that declared only one would leave the other
/// subject with no answer to "who may write this?", and the safe default is not
/// obvious enough to guess. A caller creating a fresh project states
/// `kontor_native` twice; one carrying legacy facts says so per subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectOrigins {
    /// Where the project's memory comes from.
    pub memory: SubjectOrigin,
    /// Where the project's backlog comes from.
    pub backlog: SubjectOrigin,
}

impl SubjectOrigins {
    /// The origins of a project whose facts start in Kontor.
    #[must_use]
    pub const fn native() -> Self {
        Self {
            memory: SubjectOrigin::KontorNative,
            backlog: SubjectOrigin::KontorNative,
        }
    }

    /// The origin declared for one subject.
    #[must_use]
    pub const fn for_subject(self, subject: AuthoritySubject) -> SubjectOrigin {
        match subject {
            AuthoritySubject::Memory => self.memory,
            AuthoritySubject::Backlog => self.backlog,
        }
    }
}

/// Refuse unless Kontor holds this project's named subject.
///
/// Called inside the caller's transaction so the check and the write it guards
/// commit or roll back together: an authority that moved between the two would
/// otherwise let one write land under the authority of the other.
///
/// # Errors
/// Returns [`AuthorityError::NotFound`] when no row exists — a project with no
/// declared origin is never treated as writable — and
/// [`AuthorityError::Denied`] while the legacy system still owns it.
pub fn require_subject_authority(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    subject: AuthoritySubject,
) -> Result<(), AuthorityError> {
    let current: Option<String> = tx
        .query_row(
            "SELECT authority FROM project_subject_authority WHERE project_id=?1 AND subject=?2",
            params![project_id.to_string(), subject.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let current = current.ok_or(AuthorityError::NotFound)?;
    match SubjectAuthority::parse(&current)? {
        SubjectAuthority::Kontor => Ok(()),
        SubjectAuthority::Agentsroom => Err(AuthorityError::Denied {
            subject,
            current: SubjectAuthority::Agentsroom,
        }),
    }
}

/// Insert both of a new project's authority rows.
///
/// Part of the project-creation transaction: a project cannot exist for even one
/// commit without stating who owns its memory and its backlog.
///
/// # Errors
/// Propagates SQLite failures, including the unique violation a second insert for
/// the same `(project, subject)` would cause.
pub(crate) fn create_subject_authorities(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    origins: SubjectOrigins,
) -> Result<(), rusqlite::Error> {
    for subject in AuthoritySubject::ALL.iter().copied() {
        let origin = origins.for_subject(subject);
        tx.execute(
            "INSERT INTO project_subject_authority
                 (project_id, subject, origin, authority, revision)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![
                project_id.to_string(),
                subject.as_str(),
                origin.as_str(),
                origin.initial_authority().as_str(),
            ],
        )?;
    }
    Ok(())
}

/// The canonical digest of one row's stored facts, used as a receipt result.
fn row_hash(row: &ProjectSubjectAuthority) -> Result<ContentHash, AuthorityError> {
    let value = serde_json::json!({
        "project_id": row.project_id,
        "subject": row.subject,
        "origin": row.origin,
        "authority": row.authority,
        "revision": row.revision.get(),
        "source_frozen_at": row.source_frozen_at.map(|at| at.to_string()),
        "final_import_hash": row.final_import_hash.as_ref().map(ContentHash::as_str),
        "readback_hash": row.readback_hash.as_ref().map(ContentHash::as_str),
        "switched_at": row.switched_at.map(|at| at.to_string()),
    });
    Ok(ContentHash::of(serde_json::to_string(&value)?.as_bytes()))
}

/// One authority row exactly as SQLite stores it: origin, authority, revision and
/// the four nullable evidence columns.
type StoredAuthorityRow = (
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn read_row(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    subject: AuthoritySubject,
) -> Result<ProjectSubjectAuthority, AuthorityError> {
    let row: Option<StoredAuthorityRow> = tx
        .query_row(
            "SELECT origin, authority, revision, source_frozen_at, final_import_hash,
                    readback_hash, switched_at
             FROM project_subject_authority WHERE project_id=?1 AND subject=?2",
            params![project_id.to_string(), subject.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    let (origin, authority, revision, frozen, final_import, readback, switched) =
        row.ok_or(AuthorityError::NotFound)?;
    let revision = u64::try_from(revision)
        .map_err(|_| AuthorityError::Rule("stored authority revision is negative"))?;
    Ok(ProjectSubjectAuthority {
        project_id,
        subject,
        origin: SubjectOrigin::parse(&origin)?,
        authority: SubjectAuthority::parse(&authority)?,
        revision: AggregateRevision::parse(revision)?,
        source_frozen_at: frozen.as_deref().map(parse_utc_timestamp).transpose()?,
        final_import_hash: final_import
            .as_deref()
            .map(ContentHash::parse)
            .transpose()?,
        readback_hash: readback.as_deref().map(ContentHash::parse).transpose()?,
        switched_at: switched.as_deref().map(parse_utc_timestamp).transpose()?,
    })
}

fn record_receipt(
    tx: &Transaction<'_>,
    row: &ProjectSubjectAuthority,
    operation: &str,
    input_hash: &ContentHash,
    result_hash: &ContentHash,
) -> Result<SubjectAuthorityReceipt, AuthorityError> {
    let receipt_id = Uuid::now_v7().to_string();
    let recorded_at = Timestamp::now();
    tx.execute(
        "INSERT INTO subject_authority_receipts
             (id, project_id, subject, operation, input_hash, result_hash, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            receipt_id,
            row.project_id.to_string(),
            row.subject.as_str(),
            operation,
            input_hash.as_str(),
            result_hash.as_str(),
            recorded_at.to_string(),
        ],
    )?;
    Ok(SubjectAuthorityReceipt {
        receipt_id,
        project_id: row.project_id,
        subject: row.subject,
        operation: operation.to_owned(),
        input_hash: input_hash.clone(),
        result_hash: result_hash.clone(),
        recorded_at,
    })
}

fn matching_receipt(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    subject: AuthoritySubject,
    operation: &str,
    input_hash: &ContentHash,
) -> Result<Option<SubjectAuthorityReceipt>, AuthorityError> {
    let row = tx
        .query_row(
            "SELECT id, result_hash, recorded_at
             FROM subject_authority_receipts
             WHERE project_id=?1 AND subject=?2 AND operation=?3 AND input_hash=?4",
            params![
                project_id.to_string(),
                subject.as_str(),
                operation,
                input_hash.as_str(),
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(receipt_id, result_hash, recorded_at)| {
        Ok(SubjectAuthorityReceipt {
            receipt_id,
            project_id,
            subject,
            operation: operation.to_owned(),
            input_hash: input_hash.clone(),
            result_hash: ContentHash::parse(&result_hash)?,
            recorded_at: parse_utc_timestamp(&recorded_at)?,
        })
    })
    .transpose()
}

impl SqliteStore {
    /// Read one project/subject authority row.
    ///
    /// # Errors
    /// [`AuthorityError::NotFound`] when the project has no row for that subject.
    pub fn subject_authority(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
    ) -> Result<ProjectSubjectAuthority, AuthorityError> {
        let tx = self.connection.unchecked_transaction()?;
        read_row(&tx, project_id, subject)
    }

    /// Read both of a project's authority rows, in subject order.
    ///
    /// # Errors
    /// [`AuthorityError::NotFound`] when either row is missing.
    pub fn subject_authorities(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectSubjectAuthority>, AuthorityError> {
        let tx = self.connection.unchecked_transaction()?;
        AuthoritySubject::ALL
            .iter()
            .copied()
            .map(|subject| read_row(&tx, project_id, subject))
            .collect()
    }

    /// Whether Kontor may write this project's named subject.
    ///
    /// # Errors
    /// [`AuthorityError::NotFound`] when the project has no row for that subject.
    pub fn subject_writable(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
    ) -> Result<bool, AuthorityError> {
        Ok(self
            .subject_authority(project_id, subject)?
            .writable_by_kontor())
    }

    /// Record that this project's legacy source for one subject is frozen.
    ///
    /// The attestation is an operator statement about a system Kontor cannot
    /// observe, so what is stored is *that it was made*, against a named source
    /// cursor and hash. The switch later refuses without it.
    ///
    /// # Errors
    /// [`AuthorityError::Rule`] for a native subject, one already switched, or a
    /// second attestation; [`AuthorityError::RevisionConflict`] on a stale
    /// revision.
    pub fn attest_subject_source_frozen(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
        expected_revision: AggregateRevision,
        source_cursor: &str,
        source_hash: &ContentHash,
    ) -> Result<(ProjectSubjectAuthority, SubjectAuthorityReceipt), AuthorityError> {
        kontor_core::id::reject_sensitive_text("authority.source_cursor", source_cursor)?;
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let row = read_row(&tx, project_id, subject)?;
        let input_hash = ContentHash::of(
            serde_json::to_string(&serde_json::json!({
                "source_cursor": source_cursor,
                "source_hash": source_hash.as_str(),
            }))?
            .as_bytes(),
        );
        if row.source_frozen_at.is_some()
            && let Some(receipt) =
                matching_receipt(&tx, project_id, subject, "attest", &input_hash)?
        {
            return Ok((row, receipt));
        }
        if !row.origin.permits_cutover() {
            return Err(AuthorityError::Rule(
                "this subject was created in Kontor and has no legacy source to freeze",
            ));
        }
        if row.authority == SubjectAuthority::Kontor {
            return Err(AuthorityError::Rule(
                "this subject has already been switched to Kontor",
            ));
        }
        if row.revision != expected_revision {
            return Err(AuthorityError::RevisionConflict {
                expected: expected_revision.get(),
                current: row.revision.get(),
            });
        }
        if row.source_frozen_at.is_some() {
            return Err(AuthorityError::Rule(
                "this subject's legacy source is already attested frozen",
            ));
        }

        let frozen_at = Timestamp::now();
        tx.execute(
            "UPDATE project_subject_authority
             SET source_frozen_at=?3, revision=revision+1
             WHERE project_id=?1 AND subject=?2",
            params![
                project_id.to_string(),
                subject.as_str(),
                frozen_at.to_string(),
            ],
        )?;
        let updated = read_row(&tx, project_id, subject)?;
        let result_hash = row_hash(&updated)?;
        let receipt = record_receipt(&tx, &updated, "attest", &input_hash, &result_hash)?;
        tx.commit()?;
        Ok((updated, receipt))
    }

    /// Record one subject's import manifest and the state it left behind.
    ///
    /// `readback_hash` is recomputed by the caller from *stored* Kontor state
    /// after the import wrote it, never from the submitted bytes. The switch
    /// compares its own recomputation against this stored value, so an import
    /// that reported more than it persisted cannot later be switched.
    ///
    /// # Errors
    /// [`AuthorityError::Rule`] for a native subject or a duplicate manifest.
    pub fn record_subject_import(
        &self,
        record: &SubjectImportRecord<'_>,
    ) -> Result<(SubjectImportManifest, SubjectAuthorityReceipt), AuthorityError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let recorded = record_subject_import_in(&tx, record)?;
        tx.commit()?;
        Ok(recorded)
    }

    /// The manifest one import produced, if it was recorded.
    ///
    /// # Errors
    /// Propagates SQLite and parse failures.
    pub fn subject_import_manifest(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
        source: &str,
        import_hash: &ContentHash,
    ) -> Result<Option<SubjectImportManifest>, AuthorityError> {
        let tx = self.connection.unchecked_transaction()?;
        read_manifest(&tx, project_id, subject, source, import_hash)
    }

    /// Move one `(project, subject)` from AgentsRoom to Kontor, once.
    ///
    /// Every precondition is checked inside the transaction that performs the
    /// move, and the schema trigger refuses any update that is not exactly this
    /// shape — so a caller that skipped a check here still cannot produce a
    /// switched row.
    ///
    /// # Errors
    /// [`AuthorityError::Rule`] when the subject is native, already switched, not
    /// attested frozen, has no manifest for the named import, or when the
    /// recomputed readback does not equal the stored one;
    /// [`AuthorityError::RevisionConflict`] on a stale revision.
    pub fn switch_subject_authority(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
        source: &str,
        final_import_hash: &ContentHash,
        recomputed_readback: &ContentHash,
        expected_revision: AggregateRevision,
    ) -> Result<(ProjectSubjectAuthority, SubjectAuthorityReceipt), AuthorityError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let row = read_row(&tx, project_id, subject)?;
        let input_hash = ContentHash::of(
            serde_json::to_string(&serde_json::json!({
                "source": source,
                "final_import_hash": final_import_hash.as_str(),
                "readback_hash": recomputed_readback.as_str(),
            }))?
            .as_bytes(),
        );
        if row.authority == SubjectAuthority::Kontor
            && let Some(receipt) =
                matching_receipt(&tx, project_id, subject, "switch", &input_hash)?
        {
            return Ok((row, receipt));
        }
        if !row.origin.permits_cutover() {
            return Err(AuthorityError::Rule(
                "this subject was created in Kontor and is already its own authority",
            ));
        }
        if row.authority == SubjectAuthority::Kontor {
            return Err(AuthorityError::Rule(
                "this subject has already been switched to Kontor",
            ));
        }
        if row.revision != expected_revision {
            return Err(AuthorityError::RevisionConflict {
                expected: expected_revision.get(),
                current: row.revision.get(),
            });
        }
        if row.source_frozen_at.is_none() {
            return Err(AuthorityError::Rule(
                "the legacy source has not been attested frozen for this subject",
            ));
        }
        let manifest = read_manifest(&tx, project_id, subject, source, final_import_hash)?.ok_or(
            AuthorityError::Rule("the final hashed export has not been imported"),
        )?;
        if manifest.readback_hash != *recomputed_readback {
            return Err(AuthorityError::Rule(
                "stored state does not match the readback hash the import recorded",
            ));
        }

        let switched_at = Timestamp::now();
        let changed = tx.execute(
            "UPDATE project_subject_authority
             SET authority='kontor', final_import_hash=?3, readback_hash=?4,
                 switched_at=?5, revision=revision+1
             WHERE project_id=?1 AND subject=?2
               AND authority='agentsroom' AND source_frozen_at IS NOT NULL",
            params![
                project_id.to_string(),
                subject.as_str(),
                final_import_hash.as_str(),
                recomputed_readback.as_str(),
                switched_at.to_string(),
            ],
        )?;
        if changed != 1 {
            return Err(AuthorityError::Rule(
                "the subject was not in a switchable state",
            ));
        }
        let updated = read_row(&tx, project_id, subject)?;
        let result_hash = row_hash(&updated)?;
        let receipt = record_receipt(&tx, &updated, "switch", &input_hash, &result_hash)?;
        tx.commit()?;
        Ok((updated, receipt))
    }

    /// Every recorded authority operation for one project/subject, oldest first.
    ///
    /// # Errors
    /// Propagates SQLite and parse failures.
    pub fn subject_authority_receipts(
        &self,
        project_id: ProjectId,
        subject: AuthoritySubject,
    ) -> Result<Vec<SubjectAuthorityReceipt>, AuthorityError> {
        let mut statement = self.connection.prepare(
            "SELECT id, operation, input_hash, result_hash, recorded_at
             FROM subject_authority_receipts
             WHERE project_id=?1 AND subject=?2
             ORDER BY recorded_at, id",
        )?;
        let rows =
            statement.query_map(params![project_id.to_string(), subject.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?;
        rows.map(|row| {
            let (receipt_id, operation, input_hash, result_hash, recorded_at) = row?;
            Ok(SubjectAuthorityReceipt {
                receipt_id,
                project_id,
                subject,
                operation,
                input_hash: ContentHash::parse(&input_hash)?,
                result_hash: ContentHash::parse(&result_hash)?,
                recorded_at: parse_utc_timestamp(&recorded_at)?,
            })
        })
        .collect()
    }
}

/// Record one subject's manifest and receipt **inside the caller's transaction**.
///
/// Split out from [`SqliteStore::record_subject_import`] because an import has to
/// be one transaction with the items it imported. Two transactions — items, then
/// manifest — can stop between them, and since `already_imported` is derived from
/// the manifest, the retry re-runs the item loop against rows that are already
/// there, fails on their primary key, and leaves a subject that can never be
/// switched. Joining the caller's transaction makes the manifest and the state it
/// describes commit or roll back together.
///
/// # Errors
/// [`AuthorityError::Rule`] for a native subject or a duplicate manifest.
pub(crate) fn record_subject_import_in(
    tx: &Transaction<'_>,
    record: &SubjectImportRecord<'_>,
) -> Result<(SubjectImportManifest, SubjectAuthorityReceipt), AuthorityError> {
    let &SubjectImportRecord {
        project_id,
        subject,
        source,
        import_hash,
        canonical_manifest,
        imported_count,
        readback_hash,
    } = record;
    let row = read_row(tx, project_id, subject)?;
    if !row.origin.permits_cutover() {
        return Err(AuthorityError::Rule(
            "this subject was created in Kontor and has nothing to import",
        ));
    }
    let count = i64::try_from(imported_count)
        .map_err(|_| AuthorityError::Rule("too many imported items"))?;
    let imported_at = Timestamp::now();
    let inserted = tx.execute(
        "INSERT INTO subject_import_manifests
             (project_id, subject, source, import_hash, canonical_manifest,
              imported_count, readback_hash, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT DO NOTHING",
        params![
            project_id.to_string(),
            subject.as_str(),
            source,
            import_hash.as_str(),
            canonical_manifest,
            count,
            readback_hash.as_str(),
            imported_at.to_string(),
        ],
    )?;
    if inserted != 1 {
        return Err(AuthorityError::Rule(
            "this export has already been imported for this project and subject",
        ));
    }
    let receipt = record_receipt(tx, &row, "import", import_hash, readback_hash)?;
    Ok((
        SubjectImportManifest {
            project_id,
            subject,
            source: source.to_owned(),
            import_hash: import_hash.clone(),
            imported_count,
            readback_hash: readback_hash.clone(),
            imported_at,
        },
        receipt,
    ))
}

/// Refuse a backlog write unless Kontor holds this project's backlog.
///
/// The backlog subject is the mini-project/task graph and its lifecycle, so this
/// guards the two seams that write it: applying an epic and transitioning a task.
/// It speaks [`RepositoryError`] because those are repository operations, and it
/// takes the caller's transaction so the check and the write it guards commit or
/// roll back together.
///
/// # Errors
/// [`RepositoryError::AuthorityWithheld`] while a legacy system owns the backlog,
/// and [`RepositoryError::Backend`] if the row cannot be read at all.
pub(crate) fn require_backlog_authority(
    tx: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<(), kontor_core::repository::RepositoryError> {
    match require_subject_authority(tx, project_id, AuthoritySubject::Backlog) {
        Ok(()) => Ok(()),
        Err(AuthorityError::Denied { .. } | AuthorityError::NotFound) => {
            Err(kontor_core::repository::RepositoryError::AuthorityWithheld { subject: "backlog" })
        }
        Err(other) => Err(kontor_core::repository::RepositoryError::Backend {
            detail: other.to_string(),
        }),
    }
}

/// Read one authority row inside the caller's transaction.
///
/// The same reason as above: an import checks origin and authority and then acts
/// on them, and a check taken from another transaction is a check of what was
/// true before this one started.
pub(crate) fn subject_authority_in(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    subject: AuthoritySubject,
) -> Result<ProjectSubjectAuthority, AuthorityError> {
    read_row(tx, project_id, subject)
}

fn read_manifest(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    subject: AuthoritySubject,
    source: &str,
    import_hash: &ContentHash,
) -> Result<Option<SubjectImportManifest>, AuthorityError> {
    let row: Option<(i64, String, String)> = tx
        .query_row(
            "SELECT imported_count, readback_hash, imported_at
             FROM subject_import_manifests
             WHERE project_id=?1 AND subject=?2 AND source=?3 AND import_hash=?4",
            params![
                project_id.to_string(),
                subject.as_str(),
                source,
                import_hash.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((count, readback, imported_at)) = row else {
        return Ok(None);
    };
    Ok(Some(SubjectImportManifest {
        project_id,
        subject,
        source: source.to_owned(),
        import_hash: import_hash.clone(),
        imported_count: u64::try_from(count)
            .map_err(|_| AuthorityError::Rule("stored imported count is negative"))?,
        readback_hash: ContentHash::parse(&readback)?,
        imported_at: parse_utc_timestamp(&imported_at)?,
    }))
}
