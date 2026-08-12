//! Bounded retrieval of one holiday document.
//!
//! This is the only part of `kontor-calendar` that touches a network, and it is
//! deliberately separate from everything that decides anything.
//! [`crate::resolve`] never calls it, [`crate::import::preview`] never calls it,
//! and the dispatch path therefore cannot reach it: a calendar that had to be
//! fetched before work could start would make every dispatch depend on somebody
//! else's uptime.
//!
//! Two bounds, both hard: a request that takes longer than [`FETCH_TIMEOUT`] is
//! abandoned, and a body larger than [`crate::import::MAX_DOCUMENT_BYTES`] is
//! abandoned *while it streams* rather than after it has been buffered.

use std::time::Duration;

use crate::import::MAX_DOCUMENT_BYTES;
use crate::{CalendarError, CalendarResult};

/// How long one retrieval may take, connection included.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Retrieve one document over HTTPS.
///
/// The URL is a caller's input and is never echoed into an error: a URL can
/// carry a credential in its query string, and an error message is a place
/// credentials end up in logs.
///
/// # Errors
/// [`CalendarError::Retrieval`] for a refused URL scheme, a transport failure, a
/// non-success status, or a body that exceeds the bounded size.
pub async fn retrieve(url: &str) -> CalendarResult<String> {
    // Plain HTTP is refused rather than upgraded: a holiday feed read over a
    // channel anyone can rewrite is a channel anyone can use to close a
    // workspace's calendar.
    if !url.starts_with("https://") {
        return Err(CalendarError::Retrieval {
            rule: "only https sources are retrieved",
        });
    }
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|_| CalendarError::Retrieval {
            rule: "the http client could not be built",
        })?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|_| CalendarError::Retrieval {
            rule: "the source could not be reached",
        })?;
    if !response.status().is_success() {
        return Err(CalendarError::Retrieval {
            rule: "the source did not return a document",
        });
    }
    // Read incrementally and stop at the bound. Checking `Content-Length` alone
    // would trust a header the source controls.
    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CalendarError::Retrieval {
            rule: "the source stopped mid-document",
        })?
    {
        if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err(CalendarError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| CalendarError::Retrieval {
        rule: "the document is not valid UTF-8",
    })
}
