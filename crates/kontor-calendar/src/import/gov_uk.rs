//! The GOV.UK `bank-holidays.json` document.
//!
//! Three divisions, keyed by name, each with its own event list:
//!
//! ```json
//! {"england-and-wales":{"division":"england-and-wales",
//!   "events":[{"title":"New Year's Day","date":"2026-01-01","notes":"","bunting":true}]},
//!  "scotland":{"division":"scotland","events":[]},
//!  "northern-ireland":{"division":"northern-ireland","events":[]}}
//! ```
//!
//! The request must name which division it wants, because the document contains
//! all of them and "the UK's bank holidays" is not a set that exists: England
//! and Northern Ireland do not close on the same days. England and Wales share
//! one list, which is why `GB-ENG` and `GB-WLS` select the same division and
//! still record the subdivision they were asked for.

use std::collections::BTreeMap;

use jiff::civil;
use kontor_core::calendar::{HolidayCategory, ImportWarning, ImportWarningCode};
use kontor_core::id::{ExternalId, ExternalName};
use serde::Deserialize;

use super::{ParsedDocument, SourceEntry, slug};
use crate::{CalendarError, CalendarResult};

/// One division's list.
#[derive(Debug, Deserialize)]
struct Division {
    #[serde(default)]
    events: Vec<Event>,
}

/// One bank holiday.
#[derive(Debug, Deserialize)]
struct Event {
    #[serde(default)]
    title: Option<String>,
    date: String,
}

/// The subdivision codes this importer understands, and the division each one
/// reads.
const DIVISIONS: &[(&str, &str)] = &[
    ("GB-ENG", "england-and-wales"),
    ("GB-WLS", "england-and-wales"),
    ("GB-SCT", "scotland"),
    ("GB-NIR", "northern-ireland"),
];

/// Parse the GOV.UK document into unfiltered entries for one division.
///
/// # Errors
/// [`CalendarError::Malformed`] when the request names no subdivision, names one
/// this importer does not know, when the document is not the documented shape,
/// or when it does not contain the requested division.
pub(crate) fn parse(
    raw: &str,
    subdivision: Option<&ExternalName>,
) -> CalendarResult<ParsedDocument> {
    let requested = subdivision.ok_or(CalendarError::Malformed {
        subject: "GOV.UK bank-holiday request",
        rule: "must name a UK division, because the document holds several",
    })?;
    let division_key = DIVISIONS
        .iter()
        .find(|(code, _)| *code == requested.as_str())
        .map(|(_, division)| *division)
        .ok_or(CalendarError::Malformed {
            subject: "GOV.UK bank-holiday request",
            rule: "names a division this importer does not read",
        })?;

    let document: BTreeMap<String, Division> =
        serde_json::from_str(raw).map_err(|_| CalendarError::Malformed {
            subject: "GOV.UK bank-holiday document",
            rule: "is not an object of divisions",
        })?;
    let division = document.get(division_key).ok_or(CalendarError::Malformed {
        subject: "GOV.UK bank-holiday document",
        rule: "does not contain the requested division",
    })?;

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (index, event) in division.events.iter().enumerate() {
        let position = u32::try_from(index).unwrap_or(u32::MAX);
        let mut refuse = |code: ImportWarningCode| {
            warnings.push(ImportWarning {
                code,
                entry: position,
            })
        };

        let Ok(date) = event.date.parse::<civil::Date>() else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        let Some(label) = event
            .title
            .as_deref()
            .and_then(|text| ExternalName::parse(text.trim()).ok())
        else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        let Ok(identity) = ExternalId::parse(&slug(&format!(
            "govuk:{division_key}:{date}:{}",
            label.as_str()
        ))) else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        entries.push(SourceEntry {
            position,
            start: date,
            end: date,
            label,
            // Every day in this document is a bank holiday; that is what the
            // document is. It is recorded as one rather than as a public
            // holiday so a workspace that imports only `public` gets what it
            // asked for.
            category: HolidayCategory::Bank,
            identity,
            // Stated, not empty: these days apply to the requested division and
            // not to the United Kingdom.
            subdivisions: vec![requested.as_str().to_owned()],
        });
    }
    Ok(ParsedDocument { entries, warnings })
}
