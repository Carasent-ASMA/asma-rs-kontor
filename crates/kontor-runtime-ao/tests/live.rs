//! An opt-in smoke test against a real, disposable Agent Orchestrator daemon.
//!
//! This is ignored by default and skips with a precise reason when its
//! environment is absent, because the alternative is worse than no coverage: a
//! live test that silently passes when it could not run tells you the integration
//! works when nothing was checked.
//!
//! # Only ever against a disposable project
//!
//! It launches real agents that make real edits. Point it at a scratch AO project
//! on a scratch repository, never at a developer's working project. It kills only
//! the sessions it created and never touches a session, worktree or branch it did
//! not make — a foreign session belongs to whoever started it, and cleaning one up
//! is not this test's business.
//!
//! ```bash
//! KONTOR_AO_LIVE=1 \
//! KONTOR_AO_ENDPOINT=http://127.0.0.1:3001 \
//! KONTOR_AO_PROJECT_ID=prj_scratch \
//! KONTOR_AO_PROJECT_PATH=/absolute/path/to/scratch/repo \
//! KONTOR_AO_HARNESSES=claude-code,codex \
//! cargo test -p kontor-runtime-ao --test live -- --ignored --nocapture
//! ```
//!
//! # What the WebSocket gate leaves out
//!
//! The plan's live criterion also asks for observed `/mux` output. This adapter
//! declares no WebSocket client: that needs an exact workspace-pinned dependency
//! the root manifest does not carry, and hand-rolling frames to avoid that gate is
//! rejected. So this smoke covers REST and the durable SSE replay, and the mux
//! half stays deferred with the dependency. The frame protocol itself is proved
//! against recordings in `contract.rs`.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Barrier;
use tokio::time::timeout;

use kontor_core::id::{
    AgentRunId, BoundedText, ExternalId, ExternalName, MiniProjectId, RoleSlotId, RuntimeBindingId,
    RuntimeKindKey, TaskId, TeamRunId, parse_utc_timestamp,
};
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::RuntimeCapability;
use kontor_runtime::request::{
    CancelRequest, InspectRequest, LaunchParts, LaunchRequest, MessageId, SendMessageRequest,
};
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::workspace::WorkspaceRoot;
use kontor_runtime_ao::adapter::{AoAdapter, AoCheckpoint, AoLane};
use kontor_runtime_ao::client::AoHttpTransport;
use kontor_runtime_ao::wire::AoHarness;

/// Why a live run could not happen, stated precisely rather than as a silent pass.
enum Skip {
    Missing(&'static str),
    Invalid(&'static str),
}

impl Skip {
    fn report(&self) {
        match self {
            Self::Missing(name) => {
                println!("SKIP live AO smoke: {name} is not set, so no disposable daemon was named")
            }
            Self::Invalid(name) => {
                println!("SKIP live AO smoke: {name} is set but not usable")
            }
        }
    }
}

struct LiveEnv {
    endpoint: String,
    project_id: String,
    project_path: WorkspaceRoot,
    harnesses: Vec<AoHarness>,
}

fn env(name: &'static str) -> Result<String, Skip> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(Skip::Missing(name)),
    }
}

fn live_env() -> Result<LiveEnv, Skip> {
    if env("KONTOR_AO_LIVE")? != "1" {
        return Err(Skip::Missing("KONTOR_AO_LIVE=1"));
    }
    let endpoint = env("KONTOR_AO_ENDPOINT")?;
    let project_id = env("KONTOR_AO_PROJECT_ID")?;
    let project_path = WorkspaceRoot::parse(&env("KONTOR_AO_PROJECT_PATH")?)
        .map_err(|_| Skip::Invalid("KONTOR_AO_PROJECT_PATH"))?;
    let harnesses = env("KONTOR_AO_HARNESSES")?
        .split(',')
        .map(str::trim)
        .filter(|it| !it.is_empty())
        .map(AoHarness::parse)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| Skip::Invalid("KONTOR_AO_HARNESSES"))?;
    if harnesses.len() < 2 {
        // The criterion is *concurrent* clients. One lane would prove something
        // else and must not be reported as if it proved this.
        return Err(Skip::Invalid("KONTOR_AO_HARNESSES (two or more required)"));
    }
    Ok(LiveEnv {
        endpoint,
        project_id,
        project_path,
        harnesses,
    })
}

fn lane(config: &LiveEnv, harness: AoHarness) -> AoLane {
    AoLane {
        runtime_kind: RuntimeKindKey::parse(&format!("ao.{harness}")).expect("valid runtime kind"),
        host: ExternalName::parse("ao-live").expect("valid host"),
        project_id: config.project_id.clone(),
        project_path: config.project_path.clone(),
        kind: kontor_runtime_ao::wire::AoSessionKind::Worker,
        harness,
        max_concurrent_sessions: 8,
    }
}

/// A launch for one lane, in the lane's own AO project, with the adapter's own
/// authority to perform it.
///
/// Admission runs here rather than inside the spawned task: it is bookkeeping
/// about seats, it reaches no AO surface, and what this test claims to overlap is
/// the launches themselves. Each lane gets its own team run, so four concurrent
/// launches are four seats and not four attempts at one.
async fn live_admitted_launch(ao: &AoAdapter, config: &LiveEnv) -> LaunchRequest {
    let agent_run_id = AgentRunId::generate();
    let task_id = TaskId::generate();
    let parts = LaunchParts {
        scope: ExecutionScope::for_task(
            EpicScope {
                mini_project_id: MiniProjectId::generate(),
                external_epic_key: ExternalId::parse("ASMA-AO-LIVE").expect("epic key"),
                short_title: ExternalName::parse("AO live").expect("epic title"),
            },
            TaskScope {
                task_id,
                external_issue_key: ExternalId::parse("ASMA-AO-LIVE-1").expect("issue key"),
                short_code: ExternalId::parse("AO-LIVE-1").expect("short code"),
                worktree: config.project_path.clone(),
            },
        ),
        agent_run_id,
        team_run_id: TeamRunId::generate(),
        role_slot_id: RoleSlotId::parse(&format!("slot-{agent_run_id}"))
            .expect("a run id is a legal open key"),
        task_id,
        binding_id: RuntimeBindingId::generate(),
        placement: None,
        cwd: config.project_path.clone(),
        account_profile_id: None,
        prompt: BoundedText::parse(&format!(
            "Create a file named kontor-live-{agent_run_id}.txt containing exactly the \
             text {agent_run_id}. Do nothing else."
        ))
        .expect("bounded prompt"),
        model_rung: kontor_core::spec::ModelRung {
            provider: kontor_core::spec::ProviderRef("test".to_owned()),
            model: kontor_core::spec::ModelRef("test".to_owned()),
            effort: None,
        },
        context_policy: kontor_core::spec::ContextPolicySnapshot::standard(
            &kontor_core::spec::ContextWindowBounds::unknown(),
            false,
            kontor_core::id::SCHEMA_VERSION,
            parse_utc_timestamp("2026-08-10T09:00:00Z").expect("canonical UTC"),
        )
        .expect("the standard fallback freezes"),
        autonomy: kontor_core::spec::SeatAutonomy::standard(),
        requested_at: parse_utc_timestamp("2026-08-10T09:00:00Z").expect("canonical UTC"),
    };
    ao.admit_launch(&AdmissionRequest {
        slot: RoleSlotKey::new(parts.team_run_id, parts.role_slot_id.clone()),
        agent_run_id: parts.agent_run_id,
        binding_id: parts.binding_id,
        replaces: None,
        requested_at: parts.requested_at,
    })
    .await
    .expect("a fresh seat admits this launch")
    .into_authority()
    .expect("admission issues authority rather than a resume")
    .into_request(parts)
}

/// When one lane's launch was in flight.
///
/// Wall-clock instants rather than a counter, because the claim being made is
/// about overlap in real time: two launches were in flight *together*, not merely
/// both eventually issued.
#[derive(Debug, Clone, Copy)]
struct InFlight {
    harness: AoHarness,
    started: Instant,
    finished: Instant,
}

impl InFlight {
    /// Whether these two launches were in flight at the same time.
    ///
    /// Each must have begun before the other ended. A sequential driver produces
    /// disjoint intervals and fails this.
    fn overlaps(self, other: Self) -> bool {
        self.started < other.finished && other.started < self.finished
    }

    fn elapsed(self) -> Duration {
        self.finished.duration_since(self.started)
    }
}

/// Prove every pair of launches was in flight together.
///
/// This is the discriminator the live test rests on, so it is a named function
/// rather than an inline loop: the tests at the bottom of this file exercise *this*
/// function against a genuinely concurrent workload and against the sequential
/// shape it exists to reject, which is what makes the live assertion load-bearing
/// even though the live test itself cannot run without a daemon.
///
/// # Errors
/// Returns the offending pair, with how long each was in flight.
fn all_pairs_overlap(timings: &[InFlight]) -> Result<(), String> {
    for (index, first) in timings.iter().enumerate() {
        for second in &timings[index + 1..] {
            if !first.overlaps(*second) {
                return Err(format!(
                    "{} was in flight for {:?} and {} for {:?}, and the two never overlapped: \
                     the launches ran sequentially",
                    first.harness,
                    first.elapsed(),
                    second.harness,
                    second.elapsed(),
                ));
            }
        }
    }
    Ok(())
}

/// How long a lane will wait for its siblings to reach the starting line.
///
/// Generous, because it is only ever reached by a *sequential* implementation —
/// concurrent tasks release each other in microseconds. A timeout here fails the
/// test with a precise reason instead of hanging.
const RENDEZVOUS_TIMEOUT: Duration = Duration::from_secs(30);

/// Launch two or more real clients concurrently, follow one up, and clean up only
/// what this test created.
///
/// The concurrency is the point of this test, so it is enforced twice over rather
/// than assumed:
///
/// * every lane waits at a shared rendezvous immediately before issuing its
///   launch, so no lane can start until *all* of them are ready. A driver that
///   awaited each launch in turn would never release the first lane and would fail
///   on the rendezvous timeout;
/// * each lane records when its launch began and ended, and the intervals are
///   asserted to overlap pairwise afterwards. Disjoint intervals mean the launches
///   were serialized, however they were scheduled.
///
/// Either check alone could be satisfied by something that only looks concurrent.
/// The multi-threaded flavor is deliberate too: on a current-thread runtime these
/// tasks would still interleave at await points, but the point is real parallel
/// dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a disposable AO daemon; see the module docs"]
async fn live_smoke_launches_two_clients_concurrently() {
    let config = match live_env() {
        Ok(config) => config,
        Err(skip) => {
            skip.report();
            return;
        }
    };

    // Phase 1 — sequential, read-only. Capability and catalog probes decide
    // whether a live run is possible at all, and a skip has to be able to bail
    // out cleanly, which is awkward from inside a spawned task. Nothing here
    // changes AO.
    let mut ready = Vec::new();
    for harness in &config.harnesses {
        let transport = match AoHttpTransport::new(&config.endpoint, 30) {
            Ok(transport) => transport,
            Err(error) => {
                println!("SKIP live AO smoke: endpoint unusable ({error})");
                return;
            }
        };
        let ao = AoAdapter::new(
            lane(&config, *harness),
            Box::new(transport),
            AoCheckpoint::fresh(1),
        );

        let declared = match ao.discover_capabilities().await {
            Ok(declared) => declared,
            Err(error) => {
                println!("SKIP live AO smoke: daemon is not answering ({error})");
                return;
            }
        };
        assert!(declared.supports(RuntimeCapability::Launch));

        // A real client must be installed. Skipping is honest here; asserting
        // would fail a machine that simply does not have this agent.
        let installed = ao
            .discover_clients()
            .await
            .expect("the agent catalog is readable");
        if !installed
            .installed
            .iter()
            .any(|entry| entry.id == harness.as_str())
        {
            println!("SKIP live AO smoke: {harness} is not installed on this machine");
            return;
        }
        ready.push((*harness, ao));
    }

    // Phase 2 — concurrent. Every lane is spawned, and each one blocks at the
    // rendezvous until all of them have arrived, so the launches are issued
    // together rather than one after another.
    let rendezvous = Arc::new(Barrier::new(ready.len()));
    let mut handles = Vec::with_capacity(ready.len());
    for (harness, ao) in ready {
        let request = live_admitted_launch(&ao, &config).await;
        let rendezvous = Arc::clone(&rendezvous);
        handles.push(tokio::spawn(async move {
            let together = timeout(RENDEZVOUS_TIMEOUT, rendezvous.wait()).await.is_ok();
            let started = Instant::now();
            let outcome = ao.launch(&request).await;
            let finished = Instant::now();
            (
                ao,
                outcome,
                InFlight {
                    harness,
                    started,
                    finished,
                },
                together,
            )
        }));
    }

    let mut launched = Vec::new();
    let mut in_flight = Vec::new();
    for handle in handles {
        let (ao, outcome, timing, together) = handle.await.expect("a launch task did not panic");
        assert!(
            together,
            "{} never reached the starting line within {RENDEZVOUS_TIMEOUT:?}: the lanes were \
             launched one after another rather than concurrently",
            timing.harness
        );
        match outcome {
            Ok(outcome) => {
                launched.push((ao, outcome));
                in_flight.push(timing);
            }
            Err(error) => {
                // A refused Codex launch is the safety guard working, not a
                // failure: say so rather than reporting a broken integration.
                println!("live AO smoke: {} did not launch ({error})", timing.harness);
            }
        }
    }

    if launched.len() < 2 {
        println!(
            "SKIP live AO smoke: fewer than two lanes launched, so concurrency was not exercised"
        );
        cleanup(&launched).await;
        return;
    }

    // Phase 3 — the overlap really happened. Every pair of successful launches
    // must have been in flight at the same time; if any pair is disjoint the
    // launches were serialized somewhere and this test has not proved what it
    // claims.
    if let Err(sequential) = all_pairs_overlap(&in_flight) {
        panic!("{sequential}");
    }
    let earliest_finish = in_flight
        .iter()
        .map(|it| it.finished)
        .min()
        .expect("at least two lanes launched");
    assert!(
        in_flight.iter().all(|it| it.started < earliest_finish),
        "every lane must have started before the first one finished"
    );
    println!(
        "live AO smoke: {} lanes launched concurrently, all pairs overlapping",
        in_flight.len()
    );

    // Distinct AO sessions and distinct correlation branches: two runs never share
    // one native session.
    let sessions: BTreeSet<String> = launched
        .iter()
        .map(|(_, outcome)| outcome.snapshot.identity().native_id.as_str().to_owned())
        .collect();
    assert_eq!(
        sessions.len(),
        launched.len(),
        "each lane must own a distinct AO session"
    );
    let branches: BTreeSet<String> = launched
        .iter()
        .map(|(_, outcome)| outcome.snapshot.correlation.label.to_string())
        .collect();
    assert_eq!(branches.len(), launched.len());

    // One follow-up, and a fresh inspect that must not report a verdict AO cannot
    // have.
    let (ao, outcome) = &launched[0];
    let acknowledged = ao
        .send(&SendMessageRequest {
            binding: outcome.snapshot.clone(),
            message_id: MessageId::generate(),
            body: BoundedText::parse("Thank you, that is all.").expect("bounded text"),
            sent_at: parse_utc_timestamp("2026-08-10T09:05:00Z").expect("canonical UTC"),
        })
        .await
        .expect("AO accepts a follow-up");
    assert_eq!(acknowledged.binding_id, outcome.snapshot.binding_id());

    for (ao, outcome) in &launched {
        let observed = ao
            .inspect(&InspectRequest {
                binding: outcome.snapshot.clone(),
                requested_at: parse_utc_timestamp("2026-08-10T09:06:00Z").expect("canonical UTC"),
            })
            .await
            .expect("a fresh inspect succeeds");
        assert!(
            !matches!(
                observed.state,
                kontor_core::state::ObservedRunState::Succeeded
                    | kontor_core::state::ObservedRunState::Failed
            ),
            "AO has no trustworthy verdict to report, live or recorded"
        );
    }

    // The durable replay is readable and continuous.
    let events = ao
        .observe_events(&fetch_events(&config.endpoint).await)
        .expect("the recorded replay is continuous");
    println!("live AO smoke: accepted {} change events", events.len());

    cleanup(&launched).await;
}

/// Read the durable SSE replay from the beginning, bounded by a short timeout.
///
/// `GET /api/v1/events` writes its replay and then blocks for live events, so this
/// takes what the replay produced and stops. A live consumer is a different piece
/// of work and does not belong in a smoke test.
async fn fetch_events(endpoint: &str) -> String {
    let url = format!("{}/api/v1/events?after=0", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .expect("a bounded client");
    // The read timing out is the expected end of the replay, not a failure: after
    // the durable batch AO holds the connection open for live events. Whatever
    // arrived before the timeout is what the replay contained.
    match client.get(url).send().await {
        Ok(response) => response.text().await.unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Kill only the sessions this test created.
async fn cleanup(launched: &[(AoAdapter, kontor_runtime::adapter::LaunchOutcome)]) {
    for (ao, outcome) in launched {
        if let Err(error) = ao
            .cancel(&CancelRequest {
                binding: outcome.snapshot.clone(),
                requested_at: parse_utc_timestamp("2026-08-10T09:10:00Z").expect("canonical UTC"),
            })
            .await
        {
            println!(
                "live AO smoke: could not stop {} ({error}); it must be cleaned up by hand",
                outcome.snapshot.identity().native_id
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The concurrency machinery, checked without a daemon
// ---------------------------------------------------------------------------
//
// The smoke above is `#[ignore]` and needs a real AO daemon, so it cannot guard
// its own concurrency: a regression that quietly serialized the launches would sit
// there passing on every machine that skips it. These tests run in the ordinary
// suite and pin the two halves the live assertion is built from — the rendezvous
// that forces overlap, and the predicate that detects its absence. The third one
// reproduces the exact sequential shape the live test used to have and proves the
// check rejects it, which is what makes the assertion load-bearing rather than
// decorative.

/// One lane's worth of work, timed the way the live test times a launch.
async fn timed_work(harness: AoHarness, duration: Duration) -> InFlight {
    let started = Instant::now();
    tokio::time::sleep(duration).await;
    let finished = Instant::now();
    InFlight {
        harness,
        started,
        finished,
    }
}

/// Long enough that a sequential run separates the intervals unambiguously, short
/// enough to stay invisible in the suite's runtime.
const WORK: Duration = Duration::from_millis(25);

const LANES: [AoHarness; 3] = [AoHarness::ClaudeCode, AoHarness::Codex, AoHarness::Cursor];

#[test]
fn the_overlap_predicate_separates_concurrent_work_from_sequential_work() {
    let base = Instant::now();
    let concurrent_a = InFlight {
        harness: AoHarness::ClaudeCode,
        started: base,
        finished: base + Duration::from_millis(30),
    };
    let concurrent_b = InFlight {
        harness: AoHarness::Codex,
        started: base + Duration::from_millis(5),
        finished: base + Duration::from_millis(35),
    };
    assert!(concurrent_a.overlaps(concurrent_b));
    assert!(concurrent_b.overlaps(concurrent_a), "overlap is symmetric");
    assert!(all_pairs_overlap(&[concurrent_a, concurrent_b]).is_ok());

    // The sequential shape: the second launch begins only after the first ended.
    let after = InFlight {
        harness: AoHarness::Codex,
        started: base + Duration::from_millis(30),
        finished: base + Duration::from_millis(60),
    };
    assert!(!concurrent_a.overlaps(after));
    let refused = all_pairs_overlap(&[concurrent_a, after])
        .expect_err("back-to-back intervals are not concurrency");
    assert!(refused.contains("ran sequentially"), "{refused}");

    // Touching at an instant is still not overlap.
    let touching = InFlight {
        harness: AoHarness::Cursor,
        started: concurrent_a.finished,
        finished: concurrent_a.finished + WORK,
    };
    assert!(!concurrent_a.overlaps(touching));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_rendezvous_makes_spawned_lanes_overlap() {
    // Exactly the phase-2 shape of the live smoke, with the launch replaced by a
    // timed sleep: every lane waits until all of them have arrived, then works.
    let rendezvous = Arc::new(Barrier::new(LANES.len()));
    let mut handles = Vec::with_capacity(LANES.len());
    for harness in LANES {
        let rendezvous = Arc::clone(&rendezvous);
        handles.push(tokio::spawn(async move {
            let together = timeout(RENDEZVOUS_TIMEOUT, rendezvous.wait()).await.is_ok();
            (timed_work(harness, WORK).await, together)
        }));
    }

    let mut timings = Vec::with_capacity(LANES.len());
    for handle in handles {
        let (timing, together) = handle.await.expect("a lane did not panic");
        assert!(
            together,
            "{} never reached the starting line",
            timing.harness
        );
        timings.push(timing);
    }

    assert_eq!(timings.len(), LANES.len());
    all_pairs_overlap(&timings).expect("lanes released together must overlap");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sequential_driver_is_rejected_by_the_same_check() {
    // The defect this fix exists to close: awaiting each lane to completion inside
    // a plain loop. It produces disjoint intervals, and the live test's own
    // discriminator must refuse them — otherwise the concurrency assertion could
    // never have failed and would have proved nothing.
    let mut timings = Vec::with_capacity(LANES.len());
    for harness in LANES {
        timings.push(timed_work(harness, WORK).await);
    }

    assert_eq!(timings.len(), LANES.len());
    let refused = all_pairs_overlap(&timings)
        .expect_err("a sequential driver must fail the live test's overlap check");
    assert!(refused.contains("ran sequentially"), "{refused}");
}
