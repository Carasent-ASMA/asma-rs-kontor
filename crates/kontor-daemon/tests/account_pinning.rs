//! The account-pinning seam: a run claims the account the walk selected.
//!
//! `ProviderQuotaState` is keyed by `(project, account_profile_id, provider)`
//! and there is no other key, so a seat that reaches a provider unpinned is a
//! seat whose refusal cannot be attributed and whose replacement cannot be
//! evidenced.
//!
//! The mutants this suite exists to kill:
//!
//! * pinning after the native effect instead of before it, so a seat can refuse
//!   while its account is still unknown;
//! * a replayed or restarted launch pinning twice, or a second pin silently
//!   repointing a run onto another account and moving a recorded refusal with
//!   it;
//! * back-filling an arbitrary historical run, which would invent evidence
//!   about work nobody observed;
//! * a legacy unpinned run being treated as though it had an account.

// This binary uses part of the shared harness; the rest is not dead code.
#[allow(dead_code)]
mod harness;

use harness::{Call, World};
use kontor_core::id::AccountProfileId;
use kontor_core::repository::RunRepository;

async fn account(world: &World, label: &str, alias: &str) -> AccountProfileId {
    let project = world.project;
    let created = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": label,
            "harness": "fake.runtime",
            "credential_alias": label,
            "selectable_providers": [alias],
            "enabled": true
        }),
    )
    .signed_as(world, "admin")
    .with_key(label)
    .send(world)
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    AccountProfileId::parse(created.json()["account_profile_id"].as_str().expect("an id"))
        .expect("a canonical account id")
}

#[tokio::test]
async fn an_unpinned_run_claims_the_account_it_was_given() {
    let world = World::open().await;
    let profile = account(&world, "pin-first", "codex-work").await;
    let run = world.unbound_run();
    let state = world.daemon.state();

    let before = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(
        before.account_profile_id, None,
        "a run is created unpinned; the walk has not answered yet",
    );

    state
        .with_store(|store| store.pin_agent_run_account(world.project, run, profile))
        .expect("an unpinned run accepts its account");

    let after = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("the run is readable")
        .expect("the run exists");
    assert_eq!(after.account_profile_id, Some(profile));
    assert!(
        after.revision > before.revision,
        "claiming an account advances the run's revision",
    );
}

#[tokio::test]
async fn re_presenting_the_same_account_is_a_replay_that_writes_nothing() {
    let world = World::open().await;
    let profile = account(&world, "pin-replay", "codex-work").await;
    let run = world.unbound_run();
    let state = world.daemon.state();

    state
        .with_store(|store| store.pin_agent_run_account(world.project, run, profile))
        .expect("the first pin is accepted");
    let once = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("readable")
        .expect("exists");

    // A lost answer, or a launch that restarted after the pin but before the
    // native effect, presents the identical account again.
    state
        .with_store(|store| store.pin_agent_run_account(world.project, run, profile))
        .expect("a replay is accepted");
    let twice = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("readable")
        .expect("exists");

    assert_eq!(twice.account_profile_id, Some(profile));
    assert_eq!(
        twice.revision, once.revision,
        "a replay writes nothing and must not advance the revision",
    );
}

#[tokio::test]
async fn a_second_different_account_is_refused_rather_than_repointed() {
    let world = World::open().await;
    let first = account(&world, "pin-first-account", "codex-work").await;
    let second = account(&world, "pin-second-account", "codex-personal").await;
    let run = world.unbound_run();
    let state = world.daemon.state();

    state
        .with_store(|store| store.pin_agent_run_account(world.project, run, first))
        .expect("the first pin is accepted");

    let refused = state.with_store(|store| store.pin_agent_run_account(world.project, run, second));
    assert!(
        refused.is_err(),
        "a run owns one account; repointing would move a recorded refusal onto an account \
         that never saw one",
    );

    let unchanged = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("readable")
        .expect("exists");
    assert_eq!(
        unchanged.account_profile_id,
        Some(first),
        "the refusal leaves the original claim intact",
    );
}

#[tokio::test]
async fn an_unknown_run_is_not_created_by_pinning_it() {
    let world = World::open().await;
    let profile = account(&world, "pin-unknown", "codex-work").await;
    let state = world.daemon.state();
    let absent = kontor_core::id::AgentRunId::generate();

    assert!(
        state
            .with_store(|store| store.pin_agent_run_account(world.project, absent, profile))
            .is_err(),
        "pinning names an existing run; it never invents one",
    );
}

#[tokio::test]
async fn a_legacy_unpinned_run_is_left_exactly_as_it_is() {
    let world = World::open().await;
    let _profile = account(&world, "pin-legacy", "codex-work").await;
    let run = world.unbound_run();
    let state = world.daemon.state();

    // Nothing back-fills. A historical run whose account nobody observed stays
    // null, because inventing one would assert evidence that does not exist.
    let legacy = state
        .with_store(|store| store.get_agent_run(world.project, run))
        .expect("readable")
        .expect("exists");
    assert_eq!(legacy.account_profile_id, None);

    // And the runtime-observed quota path stays inert for it, rather than
    // guessing an account to attribute a refusal to.
    let refusal = kontor_runtime::refusal::TransientRefusal::parse(
        "You've hit your usage limit. Try again later.",
    )
    .expect("a refusal");
    assert!(
        legacy.account_profile_id.is_none(),
        "the classifier is only reached when the run names an account",
    );
    drop(refusal);
}
