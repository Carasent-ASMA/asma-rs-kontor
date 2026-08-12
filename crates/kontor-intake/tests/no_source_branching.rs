//! Matching and deciding name no source system, and no source kind.
//!
//! A source scan rather than a behavioural test, and for the same reason
//! `kontor-scheduler`'s is one: "there is no branch on a pull request" is a
//! claim about code that does not exist, and a behavioural test can only sample
//! the kinds somebody thought to write. The branch that recognizes the *next*
//! deployment's kind would pass every one of them.
//!
//! `adapter.rs` is deliberately not scanned. Normalizing is exactly the job of
//! knowing one source's shape, and that knowledge is allowed to exist — in one
//! file, on one side of the envelope, and nowhere after it.

use std::path::Path;

/// The files where a decision is made, as opposed to where a payload is read.
const DECIDING_SOURCES: &[&str] = &["src/matching.rs", "src/decide.rs"];

/// Vendor and source-system vocabulary. Any of these in a deciding file means
/// the matcher has started recognizing one deployment's world.
const SOURCE_VOCABULARY: &[&str] = &[
    "github",
    "gitlab",
    "jira",
    "slack",
    "sentry",
    "pagerduty",
    "webhook",
    "pull_request",
    "pullrequest",
    "monitoring",
    "bug_report",
    "manual",
    "\"ci\"",
];

/// Read one source file with its comments stripped.
///
/// The prose explains the rules and is allowed to name the things the executable
/// code may not — this module's own doc comment names a pull request twice.
fn executable(path: &str) -> String {
    let text = std::fs::read_to_string(Path::new(path))
        .unwrap_or_else(|error| panic!("{path} is readable: {error}"));
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//") && !trimmed.starts_with("*")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn deciding_names_no_source_system() {
    for path in DECIDING_SOURCES {
        let source = executable(path).to_lowercase();
        for forbidden in SOURCE_VOCABULARY {
            assert!(
                !source.contains(forbidden),
                "{path} names `{forbidden}`. A source kind is data: matching compares \
                 opaque keys and pointers, and the moment it recognizes one system it \
                 stops working for the next one"
            );
        }
    }
}

#[test]
fn deciding_compares_against_no_string_literal() {
    // Two exceptions, both structural rather than vocabulary: the pointers into
    // the envelope's *own* declared schema fields, which are this crate's
    // contract with itself rather than any source system's.
    const ENVELOPE_FIELDS: &[&str] = &["/event_schema", "/event_schema_version"];

    for path in DECIDING_SOURCES {
        let source = executable(path);
        for line in source.lines() {
            if ENVELOPE_FIELDS.iter().any(|field| line.contains(field)) {
                continue;
            }
            for operator in ["== \"", "!= \"", "starts_with(\"", "ends_with(\""] {
                assert!(
                    !line.contains(operator),
                    "{path} compares against a string literal (`{operator}`), which is how \
                     a matcher starts recognizing one deployment's names:\n{line}"
                );
            }
        }
    }
}

#[test]
fn nothing_here_reaches_a_database_or_a_runtime() {
    for path in DECIDING_SOURCES
        .iter()
        .chain(["src/adapter.rs", "src/lib.rs"].iter())
    {
        let source = executable(path);
        for forbidden in [
            "rusqlite",
            "kontor_store",
            "kontor_runtime",
            "reqwest",
            "std::process",
            "tokio",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} names `{forbidden}`. Intake decides; storing the decision and \
                 running the work it creates are other crates' jobs"
            );
        }
    }
}
