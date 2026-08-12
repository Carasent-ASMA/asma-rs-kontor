//! Bounded iCalendar import: all-day `VEVENT`s and nothing else.
//!
//! A holiday feed is a small, dull subset of RFC 5545, and this importer reads
//! exactly that subset:
//!
//! * `DTSTART` as a `VALUE=DATE` — an all-day event;
//! * `DTEND` optionally, exclusive as the specification says, recorded here as
//!   an inclusive last date;
//! * `SUMMARY` as the label, `UID` as the identity;
//! * `CATEGORIES` when it names a category this build knows.
//!
//! Everything else is refused with a stable warning code: a timed event, a
//! recurring one, an event without a summary or a UID. **Recurrence is
//! deliberately not expanded.** A half-expanded `RRULE` would close days nobody
//! could point at in the source document, and an expansion this build got subtly
//! wrong would be indistinguishable from a correct one until the wrong day
//! arrived.

use icalendar::{Calendar, CalendarComponent, Component, Property};
use jiff::civil;
use kontor_core::calendar::{HolidayCategory, ImportWarning, ImportWarningCode};
use kontor_core::id::{ExternalId, ExternalName};

use super::{ParsedDocument, SourceEntry};
use crate::{CalendarError, CalendarResult};

/// Parse an iCalendar document into unfiltered entries.
///
/// # Errors
/// [`CalendarError::Malformed`] when the text is not an iCalendar document at
/// all. A document whose *events* are unusable parses successfully and reports
/// each one as a warning: that distinction is what lets an operator see "this
/// feed is timed events, not holidays" instead of one opaque failure.
pub(crate) fn parse(raw: &str) -> CalendarResult<ParsedDocument> {
    // The library's parser is forgiving enough to accept a vCard or a bare
    // property list as an empty calendar. An empty calendar and "this is not a
    // calendar" are different answers, and an operator who pasted the wrong file
    // deserves the second one.
    if !raw.to_ascii_uppercase().contains("BEGIN:VCALENDAR") {
        return Err(CalendarError::Malformed {
            subject: "iCalendar document",
            rule: "does not open a VCALENDAR",
        });
    }
    let calendar: Calendar = raw.parse().map_err(|_| CalendarError::Malformed {
        subject: "iCalendar document",
        rule: "is not a parseable iCalendar document",
    })?;

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (index, component) in calendar.components.iter().enumerate() {
        let position = u32::try_from(index).unwrap_or(u32::MAX);
        let mut refuse = |code: ImportWarningCode| {
            warnings.push(ImportWarning {
                code,
                entry: position,
            })
        };

        let CalendarComponent::Event(event) = component else {
            // A VTIMEZONE or a VTODO is not an entry that failed; it is simply
            // not an entry, so it is skipped without a warning.
            continue;
        };
        // Recurrence properties may be filed as a single or multi-valued
        // property by the parser. Any one makes this event outside the bounded
        // all-day subset; exclusions are recurrence semantics too.
        if ["RRULE", "RDATE", "EXDATE", "EXRULE", "RECURRENCE-ID"]
            .iter()
            .any(|key| {
                event.property_value(key).is_some() || event.multi_properties().contains_key(*key)
            })
        {
            refuse(ImportWarningCode::RecurringEvent);
            continue;
        }
        let Some(raw_start) = event.property_value("DTSTART") else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        let Some(start) = all_day(raw_start) else {
            // A `DTSTART` carrying a time is a meeting, not a closure.
            refuse(if raw_start.contains('T') {
                ImportWarningCode::TimedEvent
            } else {
                ImportWarningCode::MalformedEntry
            });
            continue;
        };
        // `DTEND` is exclusive in RFC 5545: a single day is 20260101/20260102.
        // Recorded here as the inclusive last date, because every other date in
        // this crate is inclusive and one exclusive bound in the middle of that
        // is how off-by-one closures happen.
        let end = match event.property_value("DTEND") {
            None => start,
            Some(raw_end) => {
                let Some(exclusive) = all_day(raw_end) else {
                    refuse(if raw_end.contains('T') {
                        ImportWarningCode::TimedEvent
                    } else {
                        ImportWarningCode::MalformedEntry
                    });
                    continue;
                };
                let Ok(inclusive) = exclusive.checked_sub(jiff::Span::new().days(1)) else {
                    refuse(ImportWarningCode::MalformedEntry);
                    continue;
                };
                if inclusive < start {
                    refuse(ImportWarningCode::MalformedEntry);
                    continue;
                }
                inclusive
            }
        };
        let Some(label) = event
            .property_value("SUMMARY")
            .map(str::trim)
            .and_then(|text| ExternalName::parse(text).ok())
        else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        // The UID is the feed's own stable identity, so a refreshed feed matches
        // its previous entries even when a label was edited.
        let Some(identity) = event
            .property_value("UID")
            .map(str::trim)
            .and_then(|text| ExternalId::parse(text).ok())
        else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        let Some(category) = category_of(event.multi_properties().get("CATEGORIES")) else {
            refuse(ImportWarningCode::UnsupportedEntry);
            continue;
        };
        entries.push(SourceEntry {
            position,
            start,
            end,
            label,
            category,
            identity,
            // An ICS feed states no subdivision, so what it states applies to
            // whatever the operator pointed this import at.
            subdivisions: Vec::new(),
        });
    }
    Ok(ParsedDocument { entries, warnings })
}

/// Read an iCalendar `VALUE=DATE` basic-format date: `YYYYMMDD`, nothing else.
fn all_day(value: &str) -> Option<civil::Date> {
    if value.len() != 8 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year: i16 = value.get(0..4)?.parse().ok()?;
    let month: i8 = value.get(4..6)?.parse().ok()?;
    let day: i8 = value.get(6..8)?.parse().ok()?;
    civil::Date::new(year, month, day).ok()
}

/// Map an event's `CATEGORIES` onto this build's vocabulary.
///
/// An absent category is the common public-holiday-feed convention. Once a
/// source explicitly categorizes an event, every value must be in the bounded
/// vocabulary; silently guessing would create an unintended closure.
fn category_of(values: Option<&Vec<Property>>) -> Option<HolidayCategory> {
    let Some(values) = values else {
        return Some(HolidayCategory::Public);
    };
    let mut category = None;
    for word in values
        .iter()
        .flat_map(|property| property.value().split(','))
    {
        let parsed = match word.trim().to_ascii_lowercase().as_str() {
            "public" => HolidayCategory::Public,
            "bank" => HolidayCategory::Bank,
            "optional" => HolidayCategory::Optional,
            "school" => HolidayCategory::School,
            "observance" => HolidayCategory::Observance,
            _ => return None,
        };
        match category {
            None => category = Some(parsed),
            Some(existing) if existing == parsed => {}
            Some(_) => return None,
        }
    }
    category
}
