//! The scheduler branches on no deployment's data, and compiles in no
//! deployment's numbers.
//!
//! This is a source scan rather than a behavioural test, and it is one on
//! purpose. "No profile-specific branch" is a claim about code that does not
//! exist, and a behavioural test can only ever sample the profiles someone
//! thought to write — the branch that recognizes the *next* seed id would pass
//! every one of them.
//!
//! The mutants this suite exists to kill:
//!
//! * a `match` or `==` against a profile id, phase, gate, role, trigger or source
//!   kind, so the pass works for the bundled pack and quietly stops working for a
//!   deployment's own;
//! * a compiled concurrency ceiling — the historical 7/4/2 fan-out — reappearing
//!   as a constant or a default, which would make [`CapacityConfig`] advisory;
//! * a scheduling decision that reads a source envelope or normalizes an event,
//!   both of which belong to intake (KON-MVP-22).
//!
//! [`CapacityConfig`]: kontor_scheduler::CapacityConfig

use std::path::Path;

/// Every source file the crate's decisions live in.
const SOURCES: &[&str] = &["src/lib.rs", "src/model.rs", "src/ready.rs"];

/// Types that name one deployment's own vocabulary.
///
/// A scheduler that mentions any of these is reading deployment data, and the
/// next step after reading it is branching on it. The routing a candidate
/// arrives with is already resolved, so none of them has a reason to appear.
const DEPLOYMENT_VOCABULARY: &[&str] = &[
    "WorkProfileKey",
    "PhaseKey",
    "GateKey",
    "RoleKey",
    "SkillKey",
    "TriggerKey",
    "SourceKindKey",
    "SourceConnectionKey",
    "EventSchemaKey",
    "PersonaKey",
    "source_kind",
    "trigger_key",
    "profile_key",
];

/// Read one source file with its comments stripped.
///
/// The prose explains the rules and is allowed to mention the things the
/// executable code may not: this module's own doc comment names `source_kind`
/// twice. Only what compiles is scanned.
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
fn the_scheduler_names_no_deployment_vocabulary() {
    for path in SOURCES {
        let source = executable(path);
        for forbidden in DEPLOYMENT_VOCABULARY {
            assert!(
                !source.contains(forbidden),
                "{path} names `{forbidden}`, which is one deployment's data — \
                 routing arrives pinned, so the scheduler has no reason to read it"
            );
        }
    }
}

#[test]
fn the_scheduler_compares_against_no_string_literal() {
    // A closed enum's spellings are *declarations* (`Variant => "text"`), and a
    // refusal's rule text is an *argument*. Neither is a branch. What a branch
    // looks like is a comparison, so that is exactly what is scanned for.
    for path in SOURCES {
        let source = executable(path);
        for operator in ["== \"", "!= \"", "starts_with(\"", "contains(\""] {
            assert!(
                !source.contains(operator),
                "{path} compares against a string literal (`{operator}`), which is how a \
                 scheduler starts recognizing one deployment's names"
            );
        }
    }
}

#[test]
fn the_scheduler_compiles_in_no_ceiling_of_its_own() {
    // Every ceiling is configured, so the only constant this crate is entitled to
    // declare is the domain bound it shares with `TriggerLimits::priority` — and
    // the blocker order, which is a list of names rather than a number.
    const ALLOWED: &[&str] = &["MAX_PRIORITY", "BLOCKER_ORDER", "FIRST_FENCING_TOKEN"];

    let mut declared = Vec::new();
    for path in SOURCES {
        for line in executable(path).lines() {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed
                .strip_prefix("pub const ")
                .or_else(|| trimmed.strip_prefix("const "))
            else {
                continue;
            };
            // `const fn` is a function, not a compiled-in value.
            if rest.starts_with("fn ") {
                continue;
            }
            let name = rest.split([':', ' ']).next().unwrap_or_default().to_owned();
            declared.push((*path, name));
        }
    }

    for (path, name) in &declared {
        assert!(
            ALLOWED.contains(&name.as_str()),
            "{path} declares `{name}`. Every scheduling ceiling comes from \
             `CapacityConfig`; a constant here would be a number a deployment \
             cannot change"
        );
    }
    // The bound itself is still expected to exist: a scan that passed because the
    // parser stopped matching would prove nothing.
    assert!(
        declared.iter().any(|(_, name)| name == "MAX_PRIORITY"),
        "the priority bound is still declared, so this scan is still parsing"
    );
    assert_eq!(kontor_scheduler::MAX_PRIORITY, 1_000);
}

#[test]
fn the_scheduler_reads_no_source_envelope() {
    // Intake owns envelopes, normalization and filter matching. The scheduler is
    // handed a receipt's identity and status, and nothing it could use to
    // re-decide intake even if it wanted to.
    for path in SOURCES {
        let source = executable(path);
        for forbidden in [
            "CanonicalSourceEvent",
            "SourceIdentity",
            "envelope",
            "DedupExpression",
            "TriggerFilterClause",
            "JsonPointer",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} names `{forbidden}`, which belongs to intake (KON-MVP-22)"
            );
        }
    }
}

#[test]
fn the_scheduler_resolves_no_calendar() {
    // KON-MVP-21 owns resolution. The scheduler consumes the answer, so nothing
    // that could parse a window, a holiday feed or a zone may appear.
    for path in SOURCES {
        let source = executable(path);
        for forbidden in [
            "WeeklyWindow",
            "CalendarProfileSpec",
            "HolidaySource",
            "CalendarExceptionRevision",
            "icalendar",
            "Weekday",
        ] {
            assert!(
                !source.contains(forbidden),
                "{path} names `{forbidden}`, which belongs to calendar resolution (KON-MVP-21)"
            );
        }
    }
}
