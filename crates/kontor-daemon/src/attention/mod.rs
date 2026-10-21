//! The Kontor attention channel: one channel-neutral payload, the configured
//! outbound destinations, and the triggers that settle it.
//!
//! The split is deliberate:
//!
//! * [`payload`] owns the allowlist and the plain-text boundary;
//! * [`config`] owns `attention.yml`, including the channel split that refuses
//!   raw agent attention because Paseo's own push already owns it;
//! * [`sink`] owns each destination's `POST` and the detached, bounded,
//!   isolated delivery;
//! * this module owns the triggers and the two observation classes that have no
//!   writer to push them: a waiting seat and an expired hold.
//!
//! Delivery never happens in a store transaction and never in the store at all
//! (CON-003). The store hands settled facts to an observer after commit; the
//! observer formats and spawns one detached task per alert, which fans out to
//! the configured destinations (CON-006).

pub mod config;
pub mod payload;
pub mod sink;

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use kontor_api::state::ApiState;
use kontor_core::id::{
    AgentRunId, CommandReceiptId, ExternalName, MiniProjectId, ProjectId, RealmId, Timestamp,
};
use kontor_core::receipt::CommandKind;
use kontor_core::repository::{ProjectRepository, RepositoryError, RunRepository};
use kontor_core::state::{ObservedRunState, TaskState};
use kontor_store::{AttentionSignal, AttentionSignalKind, StoredHold};
use tracing::warn;

use crate::attention::config::{AttentionConfigError, AttentionPolicy};
use crate::attention::payload::{
    AlertKind, AttentionPayload, ResultingState, Severity, plain_task_title,
};
use crate::attention::sink::AttentionSink;

/// The configured attention channel.
#[derive(Debug)]
pub struct AttentionChannel {
    policy: AttentionPolicy,
    sink: Arc<AttentionSink>,
    bind: SocketAddr,
    realm_id: RealmId,
    /// The last evidence signature emitted for one seat and rule.
    ///
    /// A cadence must not become a repeating alarm: re-observing the same
    /// unchanged waiting seat raises nothing. The map is pruned to the seats
    /// observed waiting in the current scan, so a seat that stops waiting and
    /// later resumes is a new fact.
    waiting: Mutex<BTreeMap<(AgentRunId, AlertKind), String>>,
}

impl AttentionChannel {
    /// Compose the channel from a validated policy.
    ///
    /// # Errors
    /// Returns [`AttentionConfigError`] when the HTTP client cannot be built.
    pub fn from_policy(
        policy: AttentionPolicy,
        bind: SocketAddr,
        realm_id: RealmId,
    ) -> Result<Self, AttentionConfigError> {
        let sink =
            AttentionSink::from_policy(&policy).map_err(|_| AttentionConfigError::Invalid {
                rule: "the attention HTTP client could not be constructed",
            })?;
        Ok(Self {
            policy,
            sink: Arc::new(sink),
            bind,
            realm_id,
            waiting: Mutex::new(BTreeMap::new()),
        })
    }

    /// The policy in force.
    #[must_use]
    pub const fn policy(&self) -> &AttentionPolicy {
        &self.policy
    }

    /// Turn one settled store fact into a payload and a detached delivery.
    ///
    /// Returns whether the kind was enabled. The triggering command never sees
    /// a delivery outcome and never waits for one.
    pub fn notify_signal(&self, signal: AttentionSignal) -> bool {
        let kind = match signal.kind {
            AttentionSignalKind::TaskBlocked => AlertKind::TaskBlocked,
            AttentionSignalKind::TaskParked => AlertKind::TaskParked,
            AttentionSignalKind::TaskNeedsHuman => AlertKind::TaskNeedsHuman,
            AttentionSignalKind::TaskFailed => AlertKind::TaskFailed,
            AttentionSignalKind::CommandConfirmed => AlertKind::CommandConfirmed,
        };
        if !self.policy.emits(kind) {
            return false;
        }
        let severity = match kind {
            AlertKind::TaskNeedsHuman | AlertKind::TaskFailed => Severity::Critical,
            AlertKind::TaskBlocked | AlertKind::TaskParked => Severity::Warning,
            _ => Severity::Info,
        };
        let title = match kind {
            AlertKind::TaskBlocked => format!(
                "Task blocked: {}",
                signal.task_title.as_ref().map_or_else(
                    || signal
                        .task_id
                        .map_or("unknown task".to_owned(), |id| id.to_string()),
                    plain_task_title
                )
            ),
            AlertKind::TaskParked => format!("Task parked: {}", title_or_id(&signal)),
            AlertKind::TaskNeedsHuman => format!("Task needs a human: {}", title_or_id(&signal)),
            AlertKind::TaskFailed => format!("Task failed: {}", title_or_id(&signal)),
            AlertKind::CommandConfirmed => {
                format!(
                    "Command confirmed: {}",
                    signal.command_kind.map_or("command", CommandKind::as_str)
                )
            }
            _ => return false,
        };
        let body = match kind {
            AlertKind::CommandConfirmed => format!(
                "Receipt {} is confirmed for project {}.",
                signal
                    .receipt_id
                    .map_or("unknown".to_owned(), |id| id.to_string()),
                signal.project_id
            ),
            _ => format!(
                "Task {} in project {} entered {}.{}{}",
                signal
                    .task_id
                    .map_or("unknown".to_owned(), |id| id.to_string()),
                signal.project_id,
                signal.kind.as_str().trim_start_matches("task_"),
                signal
                    .mini_project_id
                    .map(|epic| format!(" Epic {epic}."))
                    .unwrap_or_default(),
                signal
                    .task_title
                    .as_ref()
                    .map(|title| format!(" Title: {}.", plain_task_title(title)))
                    .unwrap_or_default()
            ),
        };
        let deep_link = match (signal.task_id, kind) {
            (Some(task), _) => self.task_link(signal.project_id, task),
            (None, AlertKind::CommandConfirmed) => {
                format!("http://{}/v1/projects/{}", self.bind, signal.project_id)
            }
            (None, _) => format!("http://{}/v1/projects/{}", self.bind, signal.project_id),
        };
        let resulting_state = match signal.kind {
            AttentionSignalKind::TaskBlocked => ResultingState::Blocked,
            AttentionSignalKind::TaskParked => ResultingState::Parked,
            AttentionSignalKind::TaskNeedsHuman => ResultingState::NeedsHuman,
            AttentionSignalKind::TaskFailed => ResultingState::Failed,
            AttentionSignalKind::CommandConfirmed => ResultingState::Confirmed,
        };
        self.deliver(
            kind,
            severity,
            signal.realm_id,
            signal.project_id,
            signal.mini_project_id,
            signal.task_id,
            signal.seat,
            signal.command_kind,
            signal.receipt_id,
            resulting_state,
            signal.occurred_at,
            deep_link,
            title,
            body,
        )
    }

    /// Emit one expired-hold alert for a newly raised attention row.
    pub fn notify_hold(&self, hold: &StoredHold, raised_at: Timestamp) -> bool {
        let kind = AlertKind::HoldReviewOverdue;
        if !self.policy.emits(kind) {
            return false;
        }
        let title = "Hold review overdue".to_owned();
        let body = format!(
            "Hold {} on epic {} passed its review date {}; owner {}.{}",
            hold.authorization_id,
            hold.epic_id,
            hold.terms
                .review_by
                .map_or("unknown".to_owned(), |date| date.to_string()),
            hold.owner,
            hold.terms
                .reason_code
                .map(|code| format!(" Reason: {}.", code.as_str()))
                .unwrap_or_default()
        );
        self.deliver(
            kind,
            Severity::Warning,
            self.realm_id,
            hold.project_id,
            Some(hold.epic_id),
            None,
            None,
            None,
            None,
            ResultingState::HoldReviewOverdue,
            raised_at,
            self.epic_link(hold.project_id, hold.epic_id),
            title,
            body,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &self,
        kind: AlertKind,
        severity: Severity,
        realm_id: RealmId,
        project_id: ProjectId,
        mini_project_id: Option<MiniProjectId>,
        task_id: Option<kontor_core::id::TaskId>,
        seat: Option<kontor_core::id::RoleKey>,
        command_kind: Option<CommandKind>,
        receipt_id: Option<CommandReceiptId>,
        resulting_state: ResultingState,
        occurred_at: Timestamp,
        deep_link: String,
        title: String,
        body: String,
    ) -> bool {
        let Ok(payload) = AttentionPayload::new(
            kind,
            severity,
            realm_id,
            project_id,
            mini_project_id,
            task_id,
            seat,
            command_kind,
            receipt_id,
            resulting_state,
            occurred_at,
            deep_link,
            title,
            body,
        ) else {
            warn!(
                kind = kind.as_str(),
                "an attention payload was refused by its own allowlist and was not delivered"
            );
            return false;
        };
        if payload.ensure_no_session_content().is_err() {
            warn!(
                kind = kind.as_str(),
                "an attention payload left the closed shape and was not delivered"
            );
            return false;
        }
        self.sink.dispatch(payload);
        true
    }

    fn task_link(&self, project: ProjectId, task: kontor_core::id::TaskId) -> String {
        format!("http://{}/v1/projects/{project}/tasks/{task}", self.bind)
    }

    fn epic_link(&self, project: ProjectId, epic: MiniProjectId) -> String {
        format!("http://{}/v1/projects/{project}/epics/{epic}", self.bind)
    }
}

fn title_or_id(signal: &AttentionSignal) -> String {
    signal.task_title.as_ref().map_or_else(
        || {
            signal
                .task_id
                .map_or("unknown task".to_owned(), |id| id.to_string())
        },
        plain_task_title,
    )
}

/// One seat observation that warrants an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingSeat {
    /// The run.
    pub agent_run_id: AgentRunId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning epic, when the task names one.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task.
    pub task_id: Option<kontor_core::id::TaskId>,
    /// The task's normalized title.
    pub task_title: Option<ExternalName>,
    /// The role the seat fills.
    pub role: Option<kontor_core::id::RoleKey>,
    /// Which of the three evidence rules fired.
    pub kind: AlertKind,
    /// The exact evidence signature, for re-alert suppression.
    pub signature: String,
    /// When the evidence was observed.
    pub observed_at: Timestamp,
    /// The native session, for a Paseo deep link.
    pub native_id: Option<String>,
}

/// Observe every open seat once and emit each new waiting-seat fact.
///
/// This is the class Paseo misses (REQ-010): a pending permission under presence
/// suppression, an idle seat whose task is still open, or a seat that calls
/// itself running with no progress inside the staleness bound. It is called from
/// the deployment's existing supervision cadence, not from a new resident task
/// (CON-006).
///
/// # Errors
/// Returns the store refusal when the durable inventory cannot be read. Delivery
/// outcomes are never errors.
pub async fn observe_waiting_seats(
    state: &ApiState,
    stale_after_seconds: u64,
    channel: &AttentionChannel,
) -> Result<u32, RepositoryError> {
    let now = Timestamp::now();
    let projects = state.with_store(kontor_store::SqliteStore::list_projects)?;
    let mut emitted = 0_u32;
    let mut seen: BTreeSet<(AgentRunId, AlertKind)> = BTreeSet::new();
    for project in projects {
        let runs = state.with_store(|store| store.list_agent_runs(project.project_id, None))?;
        for summary in runs {
            if summary.closed_at.is_some() || summary.lifecycle.is_terminal() {
                continue;
            }
            let Some(run) = state.with_store(|store| {
                store.get_agent_run(project.project_id, summary.agent_run_id)
            })?
            else {
                continue;
            };
            if run.terminal.is_some() || run.projection.lifecycle.is_terminal() {
                continue;
            }
            let task = match state
                .with_store(|store| store.get_team_run(project.project_id, run.team_run_id))?
            {
                Some(team) => {
                    state.with_store(|store| store.get_task(project.project_id, team.task_id))?
                }
                None => None,
            };
            let task_open = task.as_ref().is_some_and(|task| {
                !task.state.is_terminal() && task.state != TaskState::Withdrawn
            });
            let pending = pending_permissions(state, &run, now).await;
            let last_progress = state
                .with_store(|store| store.read_runtime_events(run.project_id, run.id, None))?;
            let last_progress = last_progress.last().map(|event| event.observed_at);
            let decision = if !pending.is_empty() {
                Some((
                    AlertKind::WaitingSeatPendingPermission,
                    format!(
                        "pending_permission:{}",
                        pending
                            .iter()
                            .cloned()
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                ))
            } else if run.projection.observed == ObservedRunState::WaitingInput && task_open {
                Some((
                    AlertKind::WaitingSeatWaitingInput,
                    format!(
                        "waiting_input:{}",
                        task.as_ref().map_or("none", |task| task.state.as_str())
                    ),
                ))
            } else if run.projection.observed == ObservedRunState::Running
                && stale_progress(last_progress, now, stale_after_seconds)
            {
                Some((
                    AlertKind::WaitingSeatStalled,
                    format!(
                        "stalled:{}",
                        last_progress.map_or("never".to_owned(), |at| at.to_string())
                    ),
                ))
            } else {
                None
            };
            let Some((kind, signature)) = decision else {
                continue;
            };
            let key = (run.id, kind);
            seen.insert(key);
            {
                let Ok(mut waiting) = channel.waiting.lock() else {
                    continue;
                };
                if waiting.get(&key) == Some(&signature) {
                    continue;
                }
                waiting.insert(key, signature.clone());
            }
            let observation = WaitingSeat {
                agent_run_id: run.id,
                project_id: run.project_id,
                mini_project_id: task.as_ref().and_then(|task| task.mini_project_id),
                task_id: task.as_ref().map(|task| task.id),
                task_title: task.as_ref().map(|task| task.title.clone()),
                role: Some(run.role.clone()),
                kind,
                signature,
                observed_at: now,
                native_id: run
                    .binding
                    .as_ref()
                    .map(|binding| binding.identity.native_id.to_string()),
            };
            if channel.notify_waiting_seat(&observation) {
                emitted = emitted.saturating_add(1);
            }
        }
    }
    if let Ok(mut waiting) = channel.waiting.lock() {
        waiting.retain(|key, _| seen.contains(key));
    }
    Ok(emitted)
}

impl AttentionChannel {
    /// Emit one waiting-seat alert.
    pub fn notify_waiting_seat(&self, seat: &WaitingSeat) -> bool {
        if !self.policy.emits(seat.kind) {
            return false;
        }
        let (resulting_state, title, body) = match seat.kind {
            AlertKind::WaitingSeatPendingPermission => (
                ResultingState::PendingPermission,
                format!(
                    "Seat waiting on a permission: {}",
                    seat.role.as_ref().map_or("seat", |role| role.as_str())
                ),
                format!(
                    "Seat {} in task {} holds a pending permission that Paseo may not surface.",
                    seat.role.as_ref().map_or("unknown", |role| role.as_str()),
                    seat.task_id
                        .map_or("unknown".to_owned(), |task| task.to_string())
                ),
            ),
            AlertKind::WaitingSeatWaitingInput => (
                ResultingState::WaitingInput,
                format!(
                    "Seat waiting for input: {}",
                    seat.role.as_ref().map_or("seat", |role| role.as_str())
                ),
                format!(
                    "Seat {} is idle while task {} is still open; the turn may be waiting on an unanswered question.",
                    seat.role.as_ref().map_or("unknown", |role| role.as_str()),
                    seat.task_id
                        .map_or("unknown".to_owned(), |task| task.to_string())
                ),
            ),
            AlertKind::WaitingSeatStalled => (
                ResultingState::Stalled,
                format!(
                    "Seat stalled: {}",
                    seat.role.as_ref().map_or("seat", |role| role.as_str())
                ),
                format!(
                    "Seat {} reports running but has shown no progress inside the staleness bound.",
                    seat.role.as_ref().map_or("unknown", |role| role.as_str())
                ),
            ),
            _ => return false,
        };
        let deep_link = match (&seat.native_id, seat.task_id) {
            (Some(native), _) => format!("paseo://agents/{native}"),
            (None, Some(task)) => self.task_link(seat.project_id, task),
            (None, None) => format!("http://{}/v1/projects/{}", self.bind, seat.project_id),
        };
        self.deliver(
            seat.kind,
            Severity::Warning,
            self.realm_id,
            seat.project_id,
            seat.mini_project_id,
            seat.task_id,
            seat.role.clone(),
            None,
            None,
            resulting_state,
            seat.observed_at,
            deep_link,
            title,
            body,
        )
    }
}

/// Read the seat's pending permissions from the adapter's own inspection.
///
/// The durable control-plane payload is a flat closed vocabulary and therefore
/// does not carry a pending-permission list; the list lives in the inspection
/// evidence the adapter already produces. This is the evidence rule (a), and a
/// failed inspection is an empty list rather than an alert invented from
/// silence.
async fn pending_permissions(
    state: &ApiState,
    run: &kontor_core::repository::AgentRun,
    now: Timestamp,
) -> Vec<String> {
    let Some(binding) = run.binding.as_ref() else {
        return Vec::new();
    };
    let snapshot = state.sessions().get(binding.id).or_else(|| {
        state
            .with_store(|store| store.get_binding_snapshot(binding.id))
            .ok()
            .flatten()
            .and_then(|stored| {
                serde_json::from_str::<kontor_runtime::capability::RuntimeBindingSnapshot>(
                    &stored.document,
                )
                .ok()
            })
    });
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    let Some(adapter) = state.runtimes().get(&binding.identity.runtime_kind) else {
        return Vec::new();
    };
    let Ok(observation) = adapter
        .inspect(&kontor_runtime::request::InspectRequest {
            binding: snapshot,
            requested_at: now,
        })
        .await
    else {
        return Vec::new();
    };
    pending_permissions_from_document(&observation.evidence)
}

fn pending_permissions_from_document(document: &kontor_core::id::CanonicalDocument) -> Vec<String> {
    let Ok(value) = document.deserialize::<serde_json::Value>() else {
        return Vec::new();
    };
    value
        .get("read")
        .and_then(|read| read.get("pending_permissions"))
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn stale_progress(
    last_confirmed_at: Option<Timestamp>,
    now: Timestamp,
    stale_after_seconds: u64,
) -> bool {
    let Some(last) = last_confirmed_at else {
        return false;
    };
    now.as_second().saturating_sub(last.as_second())
        > i64::try_from(stale_after_seconds).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_progress_is_a_boundary_rule_not_a_runtime_claim() {
        let now: Timestamp = "2026-10-21T10:20:00Z".parse().expect("instant");
        let fresh: Timestamp = "2026-10-21T10:00:00Z".parse().expect("instant");
        assert!(!stale_progress(Some(fresh), now, 1200));
        let stale: Timestamp = "2026-10-21T09:59:59Z".parse().expect("instant");
        assert!(stale_progress(Some(stale), now, 1200));
        assert!(!stale_progress(None, now, 1200));
    }

    #[test]
    fn pending_permission_evidence_is_read_from_the_normalized_payload() {
        let document = kontor_core::id::CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "read": {"pending_permissions": ["permission-1", "permission-2"]}
        }))
        .expect("canonical");
        assert_eq!(
            pending_permissions_from_document(&document),
            vec!["permission-1".to_owned(), "permission-2".to_owned()]
        );
    }
}
