//! The Nager holiday API's JSON.
//!
//! One object per holiday, with the shape the public API documents:
//!
//! ```json
//! [{"date":"2026-01-01","localName":"Nyttårsdag","name":"New Year's Day",
//!   "countryCode":"NO","fixed":true,"global":true,"counties":null,
//!   "launchYear":null,"types":["Public"]}]
//! ```
//!
//! The API answers one year per request, and a calendar usually wants more than
//! one. Rather than making a multi-year import several *imports* — only the
//! newest of which would be applied — this parser also accepts an array of those
//! arrays, so a caller can retrieve three years and import them as one document.
//!
//! Regional days (`global: false`) carry their `counties`, and the shared
//! normalization drops them unless the request named the subdivision. A regional
//! day imported as a national one would close a whole workspace for a day most
//! of it works.

use jiff::civil;
use kontor_core::calendar::{CountryCode, HolidayCategory, ImportWarning, ImportWarningCode};
use kontor_core::id::{ExternalId, ExternalName};
use serde::Deserialize;

use super::{ParsedDocument, SourceEntry, slug};
use crate::{CalendarError, CalendarResult};

/// One holiday as the API states it.
#[derive(Debug, Deserialize)]
struct NagerHoliday {
    date: String,
    #[serde(default)]
    #[serde(rename = "localName")]
    local_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    #[serde(default)]
    counties: Option<Vec<String>>,
    #[serde(default)]
    global: Option<bool>,
    #[serde(default)]
    types: Vec<String>,
}

/// One year, or several concatenated.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NagerDocument {
    OneYear(Vec<NagerHoliday>),
    ManyYears(Vec<Vec<NagerHoliday>>),
}

/// Parse a Nager document into unfiltered entries.
///
/// # Errors
/// [`CalendarError::Malformed`] when the document is not an array of holiday
/// objects, or an array of such arrays.
pub(crate) fn parse(raw: &str, expected_country: &CountryCode) -> CalendarResult<ParsedDocument> {
    let document: NagerDocument =
        serde_json::from_str(raw).map_err(|_| CalendarError::Malformed {
            subject: "Nager holiday document",
            rule: "is not an array of holiday objects",
        })?;
    let holidays: Vec<NagerHoliday> = match document {
        NagerDocument::OneYear(holidays) => holidays,
        NagerDocument::ManyYears(years) => years.into_iter().flatten().collect(),
    };

    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    for (index, holiday) in holidays.into_iter().enumerate() {
        let position = u32::try_from(index).unwrap_or(u32::MAX);
        let mut refuse = |code: ImportWarningCode| {
            warnings.push(ImportWarning {
                code,
                entry: position,
            })
        };

        // The API's `date` is a plain civil date. A timestamp here is a document
        // this parser does not read, not a holiday at an hour.
        let Ok(date) = holiday.date.parse::<civil::Date>() else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        // `name` is the English name and `localName` the local one; either is a
        // usable label and neither is guaranteed by the schema.
        let Some(label) = holiday
            .name
            .as_deref()
            .or(holiday.local_name.as_deref())
            .and_then(|text| ExternalName::parse(text.trim()).ok())
        else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        let Some(category) = holiday.types.iter().find_map(|kind| category_of(kind)) else {
            // An entry whose every type is unknown to this build is refused
            // rather than defaulted: guessing `public` would close a day on the
            // strength of a word nobody here has read.
            refuse(ImportWarningCode::UnsupportedEntry);
            continue;
        };
        let country = holiday.country_code.as_deref().unwrap_or("");
        if country != expected_country.as_str() {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        }
        let Ok(identity) =
            ExternalId::parse(&slug(&format!("nager:{country}:{date}:{}", label.as_str())))
        else {
            refuse(ImportWarningCode::MalformedEntry);
            continue;
        };
        // `global: false` means the day belongs to the listed counties only. A
        // missing `global` is read as nationwide, which is what the API means
        // when it omits the pair.
        let subdivisions = match (holiday.global, holiday.counties) {
            (Some(false), Some(counties)) => counties,
            (Some(false), None) => {
                refuse(ImportWarningCode::MalformedEntry);
                continue;
            }
            _ => Vec::new(),
        };
        entries.push(SourceEntry {
            position,
            start: date,
            end: date,
            label,
            category,
            identity,
            subdivisions,
        });
    }
    Ok(ParsedDocument { entries, warnings })
}

/// Map one of the API's type words onto this build's vocabulary.
fn category_of(kind: &str) -> Option<HolidayCategory> {
    match kind.to_ascii_lowercase().as_str() {
        "public" => Some(HolidayCategory::Public),
        "bank" => Some(HolidayCategory::Bank),
        "authorities" => Some(HolidayCategory::Authorities),
        "optional" => Some(HolidayCategory::Optional),
        "school" => Some(HolidayCategory::School),
        "observance" => Some(HolidayCategory::Observance),
        _ => None,
    }
}
