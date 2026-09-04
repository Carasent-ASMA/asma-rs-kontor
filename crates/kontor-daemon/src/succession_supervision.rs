//! Policy-driven supervision of durable quota-blocked seat succession.
//!
//! The loop owns observation, ordering and concurrency only. Its coordinator
//! port owns the effectful saga and must re-read every identifier and revision
//! before acting. Durable succession attempts remain the queue and the slot
//! lock, so a daemon restart resumes the same attempt instead of inventing an
//! in-memory job.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kontor_api::state::{ApiState, BarrierState};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ContentHash, EventCursor, ProjectId,
    QuotaObservationProvenanceId, RoleKey, RuntimeBindingId, SuccessionAttemptId, TeamRunId,
    Timestamp,
};
use kontor_core::repository::{
    AgentRun, CapacityRepository, ProviderQuotaState, QuotaObservationProvenance, RepositoryError,
    RepositoryResult, RunRepository, SuccessionRepository,
};
use kontor_core::spec::{ProviderQuotaSource, QuotaDecisionBasis};
use kontor_core::state::{
    DerivedRunState, Freshness, NativeRuntimeIdentity, ObservedRunState, RunLifecycle,
};
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::supervision::{SupervisionPolicy, WakeCondition, WatchdogLifecycle};

/// Exact persisted evidence identifying one seat that may need succession.
///
/// The coordinator must refetch these values before producing an effect. In
/// particular, `provider` is the exact blocking quota route being evaluated;
/// it is not inferred from a model name or from the account alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaBlockedSeatIntent {
    /// Project that owns the seat and quota projection.
    pub project_id: ProjectId,
    /// Team whose immutable role slot the seat occupies.
    pub team_run_id: TeamRunId,
    /// Immutable role key within the team snapshot.
    pub role: RoleKey,
    /// Exact predecessor run.
    pub agent_run_id: AgentRunId,
    /// Exact immutable runtime binding.
    pub runtime_binding_id: RuntimeBindingId,
    /// Exact native session and runtime generation.
    pub native_identity: NativeRuntimeIdentity,
    /// Account held by the predecessor.
    pub account_profile_id: AccountProfileId,
    /// Provider route whose current quota projection blocks this account.
    pub provider: String,
    /// Immutable quota provenance joined to the current blocked cursor.
    pub quota_provenance_id: QuotaObservationProvenanceId,
    /// Quota projection revision observed by this scan.
    pub expected_quota_state_revision: AggregateRevision,
    /// Evidence digest shared by the quota row and its provenance.
    pub quota_evidence_hash: ContentHash,
    /// Predecessor revision observed by this read-only scan.
    pub expected_predecessor_revision: AggregateRevision,
    /// Cursor of the current reduced blocked runtime observation.
    pub runtime_observation_cursor: EventCursor,
}

/// One bounded unit of effectful succession work selected by the supervisor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuccessionSupervisionIntent {
    /// Resume one durable, due, nonterminal saga after a wake or restart.
    Resume {
        /// Project that owns the attempt.
        project_id: ProjectId,
        /// Durable attempt identity; all other state must be refetched.
        attempt_id: SuccessionAttemptId,
    },
    /// Revalidate and, if still authorized, enqueue one blocked predecessor.
    EvaluateQuotaBlockedSeat(Box<QuotaBlockedSeatIntent>),
}

/// Stable result of coordinating one selected intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessionCoordinationOutcome {
    /// The durable saga advanced at least one state.
    Advanced,
    /// Placement recorded the exact future instant at which to retry.
    Deferred,
    /// Readback showed the requested state was already durable.
    Unchanged,
    /// Current evidence produced a typed durable refusal.
    Refused,
}

/// Operational failure returned by the effectful coordinator.
///
/// No variant contains runtime text or persisted content, so scan failures are
/// safe to aggregate and log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SuccessionCoordinationError {
    /// A durable read or write could not complete.
    #[error("succession repository operation failed")]
    Repository,
    /// The native runtime could not be contacted or did not confirm an effect.
    #[error("succession runtime operation failed")]
    Runtime,
    /// The refetched revisions or evidence no longer matched the intent.
    #[error("succession evidence moved before coordination")]
    Conflict,
}

/// Effectful application seam driven by the read-only supervisor.
#[async_trait]
pub trait SuccessionSupervisionCoordinator: Send + Sync {
    /// Refetch and coordinate one exact durable intent.
    ///
    /// # Errors
    /// Returns a content-free operational category. Expected policy refusals
    /// belong in [`SuccessionCoordinationOutcome::Refused`] after they are
    /// durably recorded.
    async fn coordinate(
        &self,
        intent: SuccessionSupervisionIntent,
    ) -> Result<SuccessionCoordinationOutcome, SuccessionCoordinationError>;
}

/// Observable totals from one bounded supervision scan.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SuccessionSupervisionReport {
    /// Active nonterminal team runs found by the lifecycle guard.
    pub active_team_runs: u32,
    /// Due durable attempts selected for resumption.
    pub resumed: u32,
    /// Blocked seats selected for fresh coordinator evaluation.
    pub evaluated: u32,
    /// Intents whose durable saga advanced.
    pub advanced: u32,
    /// Intents that recorded an exact deferred-until instant.
    pub deferred: u32,
    /// Idempotent intents already reflected in durable state.
    pub unchanged: u32,
    /// Intents ending in a typed durable refusal.
    pub refused: u32,
    /// Operational coordinator failures.
    pub failed: u32,
    /// Eligible intents left for a later scan because the configured cap bound.
    pub capped: u32,
}

/// Why one supervision inventory or scan could not complete.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SuccessionSupervisionError {
    /// Durable inventory could not be read.
    #[error("the succession supervision inventory could not be read: {source}")]
    Repository {
        /// Store refusal.
        #[source]
        source: RepositoryError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SlotKey {
    project_id: ProjectId,
    team_run_id: TeamRunId,
    role: RoleKey,
}

impl SlotKey {
    fn new(project_id: ProjectId, team_run_id: TeamRunId, role: RoleKey) -> Self {
        Self {
            project_id,
            team_run_id,
            role,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduledIntent {
    slot: SlotKey,
    intent: SuccessionSupervisionIntent,
}

#[derive(Debug, Default)]
struct SuccessionInventory {
    active_team_runs: u32,
    resumable: Vec<ScheduledIntent>,
    occupied_slots: BTreeSet<SlotKey>,
    candidates: Vec<ScheduledIntent>,
}

fn reachable_blocked_event(run: &AgentRun, state: &ApiState) -> RepositoryResult<bool> {
    let Some(cursor) = run.projection.last_cursor else {
        return Ok(false);
    };
    let events =
        state.with_store(|store| store.read_runtime_events(run.project_id, run.id, None))?;
    let Some(event) = events.iter().find(|event| event.cursor == cursor) else {
        return Ok(false);
    };
    let Some(binding) = run.binding.as_ref() else {
        return Ok(false);
    };
    if event.identity != binding.identity {
        return Ok(false);
    }
    let payload: serde_json::Value = event.payload.deserialize()?;
    Ok(
        payload.get("contact").and_then(serde_json::Value::as_str) == Some("reachable")
            && payload
                .get("observed_state")
                .and_then(serde_json::Value::as_str)
                == Some("blocked"),
    )
}

fn candidate_for(
    run: &AgentRun,
    quota: &ProviderQuotaState,
    provenance: &QuotaObservationProvenance,
    reachable_blocked: bool,
    now: Timestamp,
    stale_after_seconds: i64,
) -> Option<ScheduledIntent> {
    if run.projection.lifecycle == RunLifecycle::Running
        || run.projection.observed == ObservedRunState::Running
        || run.projection.lifecycle != RunLifecycle::Blocked
        || run.projection.observed != ObservedRunState::Blocked
        || run.projection.derived != DerivedRunState::Confirmed
        || run.terminal.is_some()
        || !reachable_blocked
        || Freshness::evaluate(
            run.projection.last_confirmed_at,
            now,
            jiff::SignedDuration::from_secs(stale_after_seconds),
        ) != Freshness::Fresh
    {
        return None;
    }
    let binding = run.binding.as_ref()?;
    let runtime_observation_cursor = run.projection.last_cursor?;
    let account_profile_id = run.account_profile_id?;
    let record = &provenance.record;
    let exact_quota_authority = quota.blocks_at(now)
        && quota.project_id == run.project_id
        && quota.account_profile_id == account_profile_id
        && quota.source == ProviderQuotaSource::RuntimeObservation
        && quota.provenance_id == Some(record.id)
        && record.runtime_observation_cursor == Some(runtime_observation_cursor)
        && record.project_id == run.project_id
        && record.account_profile_id == account_profile_id
        && record.provider == quota.provider
        && record.agent_run_id == run.id
        && record.runtime_binding_id == binding.id
        && record.native_id == binding.identity.native_id
        && record.binding_generation == binding.identity.generation
        && record.decided_state == quota.state
        && record.parsed_resets_at == quota.resets_at
        && record.evidence_digest == quota.evidence_hash
        && record.decision_basis == QuotaDecisionBasis::RuntimeRefusal;
    if !exact_quota_authority {
        return None;
    }
    let slot = SlotKey::new(run.project_id, run.team_run_id, run.role.clone());
    Some(ScheduledIntent {
        slot,
        intent: SuccessionSupervisionIntent::EvaluateQuotaBlockedSeat(Box::new(
            QuotaBlockedSeatIntent {
                project_id: run.project_id,
                team_run_id: run.team_run_id,
                role: run.role.clone(),
                agent_run_id: run.id,
                runtime_binding_id: binding.id,
                native_identity: binding.identity.clone(),
                account_profile_id,
                provider: quota.provider.clone(),
                quota_provenance_id: record.id,
                expected_quota_state_revision: quota.revision,
                quota_evidence_hash: quota.evidence_hash.clone(),
                expected_predecessor_revision: run.revision,
                runtime_observation_cursor,
            },
        )),
    })
}

fn collect_inventory(
    state: &ApiState,
    now: Timestamp,
    limit: u32,
) -> RepositoryResult<SuccessionInventory> {
    let due = state.with_store(|store| store.list_due_succession_attempts(now, limit))?;
    let mut inventory = SuccessionInventory::default();
    let mut due_ids = BTreeSet::new();
    for attempt in due {
        let slot = SlotKey::new(
            attempt.request.project_id,
            attempt.request.team_run_id,
            attempt.request.role.clone(),
        );
        due_ids.insert(attempt.request.id);
        inventory.resumable.push(ScheduledIntent {
            slot,
            intent: SuccessionSupervisionIntent::Resume {
                project_id: attempt.request.project_id,
                attempt_id: attempt.request.id,
            },
        });
    }

    let projects = state.with_store(kontor_store::SqliteStore::list_projects)?;
    for project in projects {
        let team_runs = state.with_store(|store| store.list_team_runs(project.project_id))?;
        let active_teams: BTreeSet<_> = team_runs
            .into_iter()
            .filter(|run| run.closed_at.is_none() && !run.lifecycle.is_terminal())
            .map(|run| run.team_run_id)
            .collect();
        inventory.active_team_runs = inventory
            .active_team_runs
            .saturating_add(u32::try_from(active_teams.len()).unwrap_or(u32::MAX));
        if active_teams.is_empty() {
            continue;
        }

        let mut blocking = Vec::new();
        for row in state.with_store(|store| store.list_provider_quota_states(project.project_id))? {
            if !row.blocks_at(now) || row.source != ProviderQuotaSource::RuntimeObservation {
                continue;
            }
            let Some(provenance_id) = row.provenance_id else {
                continue;
            };
            let Some(provenance) = state.with_store(|store| {
                store.get_quota_observation_provenance(project.project_id, provenance_id)
            })?
            else {
                continue;
            };
            blocking.push((row, provenance));
        }
        if blocking.is_empty() {
            continue;
        }

        for summary in state.with_store(|store| store.list_agent_runs(project.project_id, None))? {
            if !active_teams.contains(&summary.team_run_id)
                || summary.lifecycle == RunLifecycle::Running
                || summary.observed == ObservedRunState::Running
                || summary.lifecycle != RunLifecycle::Blocked
                || summary.observed != ObservedRunState::Blocked
            {
                continue;
            }
            let Some(run) = state.with_store(|store| {
                store.get_agent_run(project.project_id, summary.agent_run_id)
            })?
            else {
                continue;
            };
            let slot = SlotKey::new(run.project_id, run.team_run_id, run.role.clone());
            if let Some(attempt) = state.with_store(|store| {
                store.active_succession_attempt(run.project_id, run.team_run_id, &run.role)
            })? {
                if due_ids.contains(&attempt.request.id) {
                    continue;
                }
                if attempt.is_due(now) {
                    inventory.resumable.push(ScheduledIntent {
                        slot,
                        intent: SuccessionSupervisionIntent::Resume {
                            project_id: attempt.request.project_id,
                            attempt_id: attempt.request.id,
                        },
                    });
                } else {
                    inventory.occupied_slots.insert(slot);
                }
                continue;
            }
            let reachable = reachable_blocked_event(&run, state)?;
            for (row, provenance) in blocking
                .iter()
                .filter(|(row, _)| run.account_profile_id == Some(row.account_profile_id))
            {
                if let Some(candidate) = candidate_for(
                    &run,
                    row,
                    provenance,
                    reachable,
                    now,
                    state.evidence_window_seconds(),
                ) {
                    inventory.candidates.push(candidate);
                }
            }
        }
    }
    Ok(inventory)
}

fn plan(inventory: SuccessionInventory, limit: u32) -> (u32, Vec<ScheduledIntent>, u32) {
    let mut selected = Vec::new();
    let mut slots = inventory.occupied_slots;
    let mut eligible = 0_u32;
    for scheduled in inventory.resumable.into_iter().chain(inventory.candidates) {
        if !slots.insert(scheduled.slot.clone()) {
            continue;
        }
        eligible = eligible.saturating_add(1);
        if selected.len() < limit as usize {
            selected.push(scheduled);
        }
    }
    let capped = eligible.saturating_sub(u32::try_from(selected.len()).unwrap_or(u32::MAX));
    (inventory.active_team_runs, selected, capped)
}

/// Run one configured, bounded supervision pass.
///
/// The durable inventory is rebuilt on every call. No in-memory reservation is
/// carried across scans or process restarts.
///
/// # Errors
/// Returns [`SuccessionSupervisionError`] when durable inventory cannot be read.
pub async fn reconcile_once(
    coordinator: Arc<dyn SuccessionSupervisionCoordinator>,
    state: &ApiState,
    policy: &SupervisionPolicy,
    now: Timestamp,
) -> Result<SuccessionSupervisionReport, SuccessionSupervisionError> {
    let Some(limit) = policy.max_concurrent_successions() else {
        return Ok(SuccessionSupervisionReport::default());
    };
    let inventory = collect_inventory(state, now, limit)
        .map_err(|source| SuccessionSupervisionError::Repository { source })?;
    let (active_team_runs, selected, capped) = plan(inventory, limit);
    let mut report = SuccessionSupervisionReport {
        active_team_runs,
        capped,
        ..SuccessionSupervisionReport::default()
    };
    let mut work = JoinSet::new();
    for scheduled in selected {
        match scheduled.intent {
            SuccessionSupervisionIntent::Resume { .. } => {
                report.resumed = report.resumed.saturating_add(1);
            }
            SuccessionSupervisionIntent::EvaluateQuotaBlockedSeat(_) => {
                report.evaluated = report.evaluated.saturating_add(1);
            }
        }
        let coordinator = Arc::clone(&coordinator);
        work.spawn(async move { coordinator.coordinate(scheduled.intent).await });
    }
    while let Some(result) = work.join_next().await {
        match result {
            Ok(Ok(SuccessionCoordinationOutcome::Advanced)) => {
                report.advanced = report.advanced.saturating_add(1);
            }
            Ok(Ok(SuccessionCoordinationOutcome::Deferred)) => {
                report.deferred = report.deferred.saturating_add(1);
            }
            Ok(Ok(SuccessionCoordinationOutcome::Unchanged)) => {
                report.unchanged = report.unchanged.saturating_add(1);
            }
            Ok(Ok(SuccessionCoordinationOutcome::Refused)) => {
                report.refused = report.refused.saturating_add(1);
            }
            Ok(Err(_)) | Err(_) => report.failed = report.failed.saturating_add(1),
        }
    }
    Ok(report)
}

/// Run the schema-v2 supervisor until the Realm stops.
///
/// A v1 policy returns immediately. A configured v2 loop first resumes durable
/// due work, then wakes on the configured cadence and on committed events when
/// `runtime_error` is a configured wake condition. With no active team runs a
/// scan performs no fresh seat evaluation; hang findings remain read-only and
/// are never translated into succession intents here.
pub async fn poll_until_stopped(
    coordinator: Arc<dyn SuccessionSupervisionCoordinator>,
    state: ApiState,
    policy: SupervisionPolicy,
) {
    if policy.max_concurrent_successions().is_none()
        || policy.watchdog.lifecycle != WatchdogLifecycle::WhileTeamRunsActive
    {
        return;
    }
    let mut stops = state.signals().stops();
    if *stops.borrow_and_update() {
        return;
    }
    let barrier = state.barrier().settled();
    tokio::pin!(barrier);
    let barrier_state = tokio::select! {
        state = &mut barrier => state,
        _ = stops.changed() => return,
    };
    if barrier_state != BarrierState::Open {
        warn!(
            realm_id = %state.realm_id(),
            "automatic succession stayed stopped because startup reconciliation failed"
        );
        return;
    }

    let cadence = Duration::from_secs(policy.watchdog.cadence_seconds);
    let wake_on_runtime_error = policy
        .watchdog
        .wake_on
        .contains(&WakeCondition::RuntimeError);
    let mut appends = state.signals().appends();
    let mut backstop = tokio::time::interval(cadence);
    backstop.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if state.signals().is_stopping() {
            return;
        }
        tokio::select! {
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow_and_update() {
                    return;
                }
                continue;
            }
            changed = appends.changed() => {
                if changed.is_err() {
                    return;
                }
                let _ = *appends.borrow_and_update();
                if !wake_on_runtime_error {
                    continue;
                }
            }
            _ = backstop.tick() => {}
        }

        match reconcile_once(Arc::clone(&coordinator), &state, &policy, Timestamp::now()).await {
            Ok(report) if report.resumed + report.evaluated > 0 => info!(
                realm_id = %state.realm_id(),
                active_team_runs = report.active_team_runs,
                resumed = report.resumed,
                evaluated = report.evaluated,
                advanced = report.advanced,
                deferred = report.deferred,
                unchanged = report.unchanged,
                refused = report.refused,
                failed = report.failed,
                capped = report.capped,
                "automatic succession supervision completed"
            ),
            Ok(_) => {}
            Err(error) => warn!(
                realm_id = %state.realm_id(),
                detail = %error,
                "automatic succession supervision could not read its durable inventory"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kontor_core::id::{
        ContentHash, ExternalId, ExternalName, QuotaObservationProvenanceId, RuntimeKindKey,
        SpecVersion,
    };
    use kontor_core::repository::{NewQuotaObservationProvenance, RuntimeBinding};
    use kontor_core::spec::ProviderQuotaKind;
    use kontor_core::state::{DesiredRunState, RunProjection};

    use super::*;

    fn id<T>(suffix: u8, parse: impl FnOnce(&str) -> Result<T, kontor_core::DomainError>) -> T {
        parse(&format!("018f0000-0000-7000-8000-{suffix:012x}")).expect("deterministic test id")
    }

    fn now() -> Timestamp {
        "2026-09-04T10:00:00Z".parse().expect("timestamp")
    }

    fn seat(suffix: u8, observed: ObservedRunState, confirmed_at: Timestamp) -> AgentRun {
        let project_id = id(1, ProjectId::parse);
        let agent_run_id = id(suffix, AgentRunId::parse);
        AgentRun {
            id: agent_run_id,
            project_id,
            team_run_id: id(suffix.saturating_add(32), TeamRunId::parse),
            parent_agent_run_id: None,
            role: RoleKey::parse(&format!("role-{suffix}")).expect("role"),
            account_profile_id: Some(id(2, AccountProfileId::parse)),
            binding: Some(RuntimeBinding {
                id: id(suffix.saturating_add(64), RuntimeBindingId::parse),
                agent_run_id,
                identity: NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime"),
                    host: ExternalName::parse("local").expect("host"),
                    generation: 3,
                    native_id: ExternalId::parse(&format!("native-{suffix}")).expect("native id"),
                },
                bound_at: confirmed_at,
            }),
            projection: RunProjection {
                lifecycle: if observed == ObservedRunState::Running {
                    RunLifecycle::Running
                } else {
                    RunLifecycle::Blocked
                },
                desired: DesiredRunState::RunRequested,
                observed,
                derived: DerivedRunState::Confirmed,
                last_confirmed_at: Some(confirmed_at),
                last_cursor: Some(EventCursor::parse(i64::from(suffix) + 1).expect("cursor")),
            },
            terminal: None,
            revision: AggregateRevision::parse(7).expect("revision"),
            created_at: confirmed_at,
            closed_at: None,
        }
    }

    fn candidate(suffix: u8) -> ScheduledIntent {
        let run = seat(suffix, ObservedRunState::Blocked, now());
        let (quota, provenance) = quota_authority(&run, "codex", 100);
        candidate_for(&run, &quota, &provenance, true, now(), 1_200).expect("eligible blocked seat")
    }

    fn quota_authority(
        run: &AgentRun,
        provider: &str,
        provenance_suffix: u8,
    ) -> (ProviderQuotaState, QuotaObservationProvenance) {
        let provenance_id = id(provenance_suffix, QuotaObservationProvenanceId::parse);
        let evidence_hash = ContentHash::of(format!("evidence-{provider}").as_bytes());
        let account_profile_id = run.account_profile_id.expect("account");
        let binding = run.binding.as_ref().expect("binding");
        let record = NewQuotaObservationProvenance {
            id: provenance_id,
            project_id: run.project_id,
            account_profile_id,
            provider: provider.to_owned(),
            signal_id: format!("{provider}-quota"),
            signal_version: SpecVersion::parse(1).expect("version"),
            signal_definition_hash: ContentHash::of(provider.as_bytes()),
            agent_run_id: run.id,
            runtime_binding_id: binding.id,
            native_id: binding.identity.native_id.clone(),
            binding_generation: binding.identity.generation,
            runtime_observation_cursor: run.projection.last_cursor,
            item_epoch: 1,
            item_seq_start: 1,
            item_seq_end: 1,
            source_sequences: vec![(1, 1)],
            item_kind: "assistant".to_owned(),
            item_observed_at: now(),
            decision_basis: QuotaDecisionBasis::RuntimeRefusal,
            decided_state: ProviderQuotaKind::Drained,
            parsed_resets_at: None,
            reset_zone: None,
            evidence_digest: evidence_hash.clone(),
            recorded_at: now(),
        };
        (
            ProviderQuotaState {
                project_id: run.project_id,
                account_profile_id,
                provider: provider.to_owned(),
                state: ProviderQuotaKind::Drained,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash,
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at: now(),
                provenance_id: Some(provenance_id),
                revision: AggregateRevision::INITIAL,
                updated_at: now(),
            },
            QuotaObservationProvenance { record },
        )
    }

    #[test]
    fn fresh_reachable_blocked_seats_on_the_exact_quota_pair_are_candidates() {
        let blocked = seat(4, ObservedRunState::Blocked, now());
        let (quota, provenance) = quota_authority(&blocked, "codex", 100);
        let selected =
            candidate_for(&blocked, &quota, &provenance, true, now(), 1_200).expect("candidate");
        let SuccessionSupervisionIntent::EvaluateQuotaBlockedSeat(intent) = selected.intent else {
            panic!("expected a seat evaluation");
        };
        assert_eq!(intent.agent_run_id, blocked.id);
        assert_eq!(intent.account_profile_id, id(2, AccountProfileId::parse));
        assert_eq!(intent.provider, "codex");
        assert_eq!(intent.runtime_observation_cursor.get(), 5);
    }

    #[test]
    fn a_running_seat_on_the_same_account_is_never_a_candidate() {
        let running = seat(4, ObservedRunState::Running, now());
        let (quota, provenance) = quota_authority(&running, "codex", 100);
        assert_eq!(
            candidate_for(&running, &quota, &provenance, true, now(), 1_200,),
            None
        );
    }

    #[test]
    fn stale_unreachable_and_other_account_seats_are_not_candidates() {
        let blocked = seat(4, ObservedRunState::Blocked, now());
        let (quota, provenance) = quota_authority(&blocked, "codex", 100);
        assert!(candidate_for(&blocked, &quota, &provenance, false, now(), 1_200,).is_none());
        let stale = seat(
            4,
            ObservedRunState::Blocked,
            now() - jiff::SignedDuration::from_secs(1_201),
        );
        let (stale_quota, stale_provenance) = quota_authority(&stale, "codex", 102);
        assert!(
            candidate_for(&stale, &stale_quota, &stale_provenance, true, now(), 1_200,).is_none()
        );
        let mut other_account_quota = quota.clone();
        other_account_quota.account_profile_id = id(3, AccountProfileId::parse);
        assert!(
            candidate_for(
                &blocked,
                &other_account_quota,
                &provenance,
                true,
                now(),
                1_200,
            )
            .is_none()
        );
    }

    #[test]
    fn two_blocked_provider_rows_select_only_the_one_bound_to_the_latest_cursor() {
        let blocked = seat(4, ObservedRunState::Blocked, now());
        let (codex, codex_provenance) = quota_authority(&blocked, "codex", 100);
        let (claude, mut claude_provenance) = quota_authority(&blocked, "claude", 101);
        claude_provenance.record.agent_run_id = id(5, AgentRunId::parse);

        assert!(candidate_for(&blocked, &codex, &codex_provenance, true, now(), 1_200,).is_some());
        assert!(
            candidate_for(&blocked, &claude, &claude_provenance, true, now(), 1_200,).is_none()
        );
    }

    #[test]
    fn provenance_from_another_native_generation_is_never_a_candidate() {
        let blocked = seat(4, ObservedRunState::Blocked, now());
        let (quota, mut provenance) = quota_authority(&blocked, "codex", 100);
        provenance.record.binding_generation += 1;

        assert!(candidate_for(&blocked, &quota, &provenance, true, now(), 1_200,).is_none());
    }

    #[test]
    fn an_elapsed_quota_reset_is_not_current_retirement_authority() {
        let blocked = seat(4, ObservedRunState::Blocked, now());
        let (mut quota, mut provenance) = quota_authority(&blocked, "codex", 100);
        quota.state = ProviderQuotaKind::Exhausted;
        quota.resets_at = Some(now());
        provenance.record.decided_state = ProviderQuotaKind::Exhausted;
        provenance.record.parsed_resets_at = Some(now());

        assert!(candidate_for(&blocked, &quota, &provenance, true, now(), 1_200,).is_none());
    }

    #[test]
    fn the_configured_five_seat_cap_leaves_the_remainder_for_another_scan() {
        let inventory = SuccessionInventory {
            active_team_runs: 6,
            candidates: (4..10).map(candidate).collect(),
            ..SuccessionInventory::default()
        };
        let (active, selected, capped) = plan(inventory, 5);
        assert_eq!(active, 6);
        assert_eq!(selected.len(), 5);
        assert_eq!(capped, 1);
    }

    #[test]
    fn a_durable_attempt_wins_its_slot_on_every_restart_scan() {
        let candidate = candidate(4);
        let slot = candidate.slot.clone();
        let attempt_id = id(90, SuccessionAttemptId::parse);
        let inventory = || SuccessionInventory {
            active_team_runs: 1,
            resumable: vec![ScheduledIntent {
                slot: slot.clone(),
                intent: SuccessionSupervisionIntent::Resume {
                    project_id: slot.project_id,
                    attempt_id,
                },
            }],
            candidates: vec![candidate.clone()],
            ..SuccessionInventory::default()
        };

        let (_, first, _) = plan(inventory(), 5);
        let (_, after_restart, _) = plan(inventory(), 5);
        assert_eq!(first, after_restart);
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].intent,
            SuccessionSupervisionIntent::Resume {
                project_id: slot.project_id,
                attempt_id,
            }
        );
    }

    #[derive(Default)]
    struct RecordingCoordinator {
        intents: Mutex<Vec<SuccessionSupervisionIntent>>,
    }

    #[async_trait]
    impl SuccessionSupervisionCoordinator for RecordingCoordinator {
        async fn coordinate(
            &self,
            intent: SuccessionSupervisionIntent,
        ) -> Result<SuccessionCoordinationOutcome, SuccessionCoordinationError> {
            self.intents.lock().expect("recording lock").push(intent);
            Ok(SuccessionCoordinationOutcome::Unchanged)
        }
    }

    #[tokio::test]
    async fn bounded_dispatch_reports_each_idempotent_readback_once() {
        let coordinator = Arc::new(RecordingCoordinator::default());
        let (_, selected, _) = plan(
            SuccessionInventory {
                active_team_runs: 5,
                candidates: (4..9).map(candidate).collect(),
                ..SuccessionInventory::default()
            },
            5,
        );
        let mut work = JoinSet::new();
        for scheduled in selected {
            let coordinator = Arc::clone(&coordinator);
            work.spawn(async move { coordinator.coordinate(scheduled.intent).await });
        }
        let mut unchanged = 0;
        while let Some(result) = work.join_next().await {
            if result.expect("task").expect("coordination")
                == SuccessionCoordinationOutcome::Unchanged
            {
                unchanged += 1;
            }
        }
        assert_eq!(unchanged, 5);
        assert_eq!(coordinator.intents.lock().expect("recording lock").len(), 5);
    }
}
