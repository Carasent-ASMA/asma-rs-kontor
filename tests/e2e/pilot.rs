//! KON-MVP-18 — the disposable mini-project proof.
//!
//! One driver, one bundle, one verdict. Every section below composes merged
//! seams rather than restating them: the adapter rules come from
//! `kontor_tests_contract`, the profile rules from `kontor_profiles`, the
//! refusals from `kontor_scheduler` and `kontor_policy`. Where a seam has not
//! merged the case is recorded `blocked` against its owning ticket, which
//! rejects the run just as a failure does.
//!
//! Run it with:
//!
//! ```sh
//! cargo test -p kontor-tests-e2e --test pilot -- --nocapture
//! ```

use std::collections::BTreeMap;

use kontor_core::id::{Timestamp, parse_utc_timestamp};
use kontor_tests_e2e::{Bundle, digest, head_commit, repo_root};

mod pilot_sections;

/// The disposable project fixture.
const PROJECT_FIXTURE: &str = include_str!("../fixtures/pilot/project.json");
/// The incident profile pack — fixture data the core has never seen.
const INCIDENT_PACK: &str = include_str!("../fixtures/pilot/incident-response-pack.json");
/// The second external workflow, reused from the connector's own fixtures so
/// the two can never drift apart.
const ALTERNATE_WORKFLOW: &str = include_str!(
    "../../crates/kontor-integrations-asma/tests/fixtures/external-workflow-alternate.json"
);

/// A canonical UTC instant.
///
/// # Panics
/// Panics when `text` is not canonical UTC, which is a fixture bug.
fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

#[tokio::test(flavor = "multi_thread")]
async fn pilot() {
    // The run id is content-derived, so its evidence must be content-stable too.
    // This feature-gated source yields real UUIDv7 ids used unchanged by the
    // database and receipts, but starts from the same sequence on every pilot
    // process instead of reading the wall clock.
    kontor_core::id::install_deterministic_id_source();
    let root = repo_root();
    let commit = head_commit(&root);
    let fixtures = BTreeMap::from([
        (
            "tests/fixtures/pilot/project.json".to_owned(),
            digest(PROJECT_FIXTURE.as_bytes()),
        ),
        (
            "tests/fixtures/pilot/incident-response-pack.json".to_owned(),
            digest(INCIDENT_PACK.as_bytes()),
        ),
        (
            "crates/kontor-integrations-asma/tests/fixtures/external-workflow-alternate.json"
                .to_owned(),
            digest(ALTERNATE_WORKFLOW.as_bytes()),
        ),
    ]);
    let mut bundle = Bundle::open(&root, &commit, &fixtures).expect("the bundle roots open");

    pilot_sections::project::run(&mut bundle).await;
    pilot_sections::scheduling::run(&mut bundle);
    pilot_sections::runtime::run(&mut bundle).await;
    pilot_sections::gates::run(&mut bundle).await;
    pilot_sections::session::run(&mut bundle).await;
    pilot_sections::domain::run(&mut bundle).await;
    pilot_sections::ui::run(&mut bundle);

    let run_id = bundle.run_id().to_owned();
    let verdict = bundle.finish().expect("the bundle is written");
    println!(
        "KON-MVP-18 pilot: {} — pass {} · fail {} · blocked {} · missing {}",
        if verdict.accepted { "ACCEPT" } else { "REJECT" },
        verdict.pass,
        verdict.fail,
        verdict.blocked,
        verdict.missing
    );
    if !verdict.unmet.is_empty() {
        println!("unmet criteria:\n  {}", verdict.unmet.join("\n  "));
    }
    println!("evidence: {}", bundle_note(&root, &run_id));

    // A `blocked` case is a proof this tree cannot yet carry, so it rejects the
    // bundle without failing the gate — leaving the pilot permanently red until
    // three other tickets merge would only teach people to skip it. A `fail` is
    // different: something that is supposed to work does not, and the gate says
    // so out loud.
    assert_eq!(
        verdict.fail,
        0,
        "the pilot found {} defect(s) in the merged tree:\n  {}",
        verdict.fail,
        verdict.unmet.join("\n  ")
    );
}

/// Where to read this run's evidence back.
fn bundle_note(root: &std::path::Path, run_id: &str) -> String {
    format!(
        "{} (retained: {})",
        root.join("target/kontor-pilot").join(run_id).display(),
        root.join("docs/evidence/KON-MVP-18").join(run_id).display()
    )
}
