//! Configurable, runtime-neutral supervision for persistent role seats.
//!
//! Normal completion is event-driven. This policy exists for the one case a
//! completion callback cannot report: a native turn that never completes. It
//! classifies observations only; adapters own notifications and inspection,
//! while the orchestrator owns any recovery action.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The optional policy file inside a Realm state root.
pub const SUPERVISION_FILE: &str = "supervision.yml";

/// Highest operator-configurable number of simultaneous succession sagas.
///
/// This is a process-safety bound, not an operating default. Schema v2 still
/// requires the operator to choose the effective value explicitly.
pub const MAX_CONCURRENT_SUCCESSIONS: u32 = 64;

/// Why the policy file could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SupervisionError {
    /// The file exists but could not be read.
    #[error("the supervision policy could not be read")]
    Read {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is not valid YAML for this schema.
    #[error("the supervision policy is not a valid supported YAML document")]
    Document,
    /// The document is structurally valid but unsafe or contradictory.
    #[error("the supervision policy is invalid: {rule}")]
    Invalid {
        /// The stable rule, never a configured value.
        rule: &'static str,
    },
}

/// A versioned supervision document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisionPolicy {
    /// Schema generation of this document.
    pub schema_version: u32,
    /// How ordinary completion wakes the orchestrator.
    pub completion: CompletionPolicy,
    /// The fallback that observes turns which never complete.
    pub watchdog: WatchdogPolicy,
    /// Safe recovery behavior after a finding.
    pub recovery: RecoveryPolicy,
    /// Prompt documents used by the orchestration surface.
    pub prompts: PromptRefs,
    /// Skills the orchestration surface must load before acting.
    pub skills: Vec<String>,
}

/// Normal completion behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionPolicy {
    /// Notification-first for callback-capable runtimes, bounded polling for a
    /// runtime that cannot emit callbacks.
    pub mode: CompletionMode,
    /// Whether dispatch returns immediately to the orchestrator.
    pub background: bool,
    /// Whether the runtime must request a completion callback.
    pub notify_on_finish: bool,
}

/// Available completion strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMode {
    /// Yield after dispatch and wake on the native completion event.
    NotificationFirst,
    /// Use an external bounded observer when callbacks are unavailable.
    BoundedPolling,
}

/// Independent hang-detection behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchdogPolicy {
    /// Whether this policy asks the deployment to run a watchdog.
    pub enabled: bool,
    /// When the deployment should keep the watchdog registered.
    pub lifecycle: WatchdogLifecycle,
    /// Seconds between read-only observations.
    pub cadence_seconds: u64,
    /// Seconds after which evidence may become stale.
    pub stale_after_seconds: u64,
    /// Every predicate required before a turn is called a suspected hang.
    pub required_evidence: Vec<HangEvidence>,
    /// Conditions that wake the orchestrator without waiting for stale time.
    pub wake_on: Vec<WakeCondition>,
}

/// Lifetime of a watchdog registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogLifecycle {
    /// Register while at least one team-run envelope is active, then remove it.
    WhileTeamRunsActive,
}

/// Evidence that contributes to a suspected-hang decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HangEvidence {
    /// The current native turn has exceeded the stale threshold.
    ActiveTurnAge,
    /// No meaningful progress has been observed for the stale threshold.
    NoProgress,
}

/// Immediate reasons to wake the orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeCondition {
    /// A native tool or runtime is waiting for permission.
    PendingPermission,
    /// The runtime reported an error.
    RuntimeError,
}

/// Recovery constraints kept in data and validated as safety invariants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    /// The first response to a suspected hang.
    pub first_action: FirstRecoveryAction,
    /// The evidence threshold for replacing a persistent seat.
    pub replace_only_when: ReplacementThreshold,
    /// Whether two non-terminal sessions may occupy one role slot.
    pub allow_duplicate_seat: bool,
    /// Whether capacity pressure may cancel work already running.
    pub cancel_running_work: bool,
    /// Explicit process-wide ceiling for concurrently coordinated successions.
    ///
    /// Absent in schema v1, whose behavior remains classification-only. It is
    /// required by schema v2, which enables the resident succession engine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent_successions: Option<u32>,
}

/// Safe first recovery action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirstRecoveryAction {
    /// Inspect and reuse the existing persistent seat.
    ReconcileSameSeat,
}

/// Evidence required before a seat may be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementThreshold {
    /// The native session is explicitly retired or proved unusable.
    RetiredOrUnusable,
}

/// Versioned prompt references; contents remain deployment data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptRefs {
    /// Read-only observation prompt.
    pub watchdog: String,
    /// Same-seat reconciliation prompt.
    pub recovery: String,
}

/// One normalized observation supplied by a runtime adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatObservation {
    /// Age of the active turn, or `None` when the seat is idle.
    pub active_turn_age_seconds: Option<u64>,
    /// Age of the last meaningful progress evidence.
    pub no_progress_seconds: Option<u64>,
    /// Whether a permission request is waiting.
    pub pending_permission: bool,
    /// Whether the runtime reported an error.
    pub runtime_error: bool,
}

/// What one watchdog observation asks the orchestrator to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisionDecision {
    /// No wake-up is warranted.
    NoAction,
    /// Wake the orchestrator with the observation evidence.
    Wake(WakeReason),
}

/// Why the orchestrator is being woken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    /// A native permission request is pending.
    PendingPermission,
    /// The runtime reported an error.
    RuntimeError,
    /// All configured stale predicates hold.
    SuspectedHang,
}

impl SupervisionPolicy {
    /// Validate this policy's safety and internal consistency.
    ///
    /// # Errors
    /// Returns a stable rule without echoing configured prompt or skill text.
    pub fn validate(&self) -> Result<(), SupervisionError> {
        if !matches!(self.schema_version, 1 | 2) {
            return invalid("schema_version must be 1 or 2");
        }
        if self.completion.mode == CompletionMode::NotificationFirst
            && (!self.completion.background || !self.completion.notify_on_finish)
        {
            return invalid("notification_first requires background and notify_on_finish");
        }
        if self.watchdog.cadence_seconds == 0
            || self.watchdog.stale_after_seconds < self.watchdog.cadence_seconds
            || self.watchdog.stale_after_seconds > i64::MAX as u64
        {
            return invalid(
                "watchdog durations must be positive, representable, and cadence must not exceed stale_after",
            );
        }
        let evidence: BTreeSet<_> = self.watchdog.required_evidence.iter().copied().collect();
        if evidence.len() != self.watchdog.required_evidence.len()
            || !evidence.contains(&HangEvidence::ActiveTurnAge)
            || !evidence.contains(&HangEvidence::NoProgress)
        {
            return invalid(
                "suspected hang requires unique active_turn_age and no_progress evidence",
            );
        }
        let wake_on: BTreeSet<_> = self.watchdog.wake_on.iter().copied().collect();
        if wake_on.len() != self.watchdog.wake_on.len() {
            return invalid("wake_on conditions must be unique");
        }
        if self.recovery.allow_duplicate_seat || self.recovery.cancel_running_work {
            return invalid("recovery may neither duplicate a seat nor cancel running work");
        }
        match (
            self.schema_version,
            self.recovery.max_concurrent_successions,
        ) {
            (1, None) => {}
            (1, Some(_)) => {
                return invalid("schema_version 1 cannot enable automatic succession");
            }
            (2, Some(limit @ 1..=MAX_CONCURRENT_SUCCESSIONS)) => {
                let _ = limit;
            }
            (2, None) => {
                return invalid("schema_version 2 requires max_concurrent_successions");
            }
            (2, Some(_)) => {
                return invalid("max_concurrent_successions is outside the safe bound");
            }
            _ => unreachable!("schema version was validated above"),
        }
        if self.prompts.watchdog.trim().is_empty() || self.prompts.recovery.trim().is_empty() {
            return invalid("watchdog and recovery prompt references are required");
        }
        let skills: BTreeSet<_> = self.skills.iter().map(String::as_str).collect();
        if skills.len() != self.skills.len() || skills.iter().any(|skill| skill.trim().is_empty()) {
            return invalid("skill references must be non-empty and unique");
        }
        Ok(())
    }

    /// Configured concurrency ceiling for the automatic succession engine.
    ///
    /// Schema v1 deliberately returns `None`: accepting the old document does
    /// not silently turn on new process behavior after an upgrade.
    #[must_use]
    pub const fn max_concurrent_successions(&self) -> Option<u32> {
        if self.schema_version == 2 && self.watchdog.enabled {
            self.recovery.max_concurrent_successions
        } else {
            None
        }
    }

    /// Classify one read-only runtime observation.
    #[must_use]
    pub fn assess(&self, observation: SeatObservation) -> SupervisionDecision {
        if !self.watchdog.enabled {
            return SupervisionDecision::NoAction;
        }
        if observation.pending_permission
            && self
                .watchdog
                .wake_on
                .contains(&WakeCondition::PendingPermission)
        {
            return SupervisionDecision::Wake(WakeReason::PendingPermission);
        }
        if observation.runtime_error && self.watchdog.wake_on.contains(&WakeCondition::RuntimeError)
        {
            return SupervisionDecision::Wake(WakeReason::RuntimeError);
        }
        let stale = self.watchdog.stale_after_seconds;
        if observation
            .active_turn_age_seconds
            .is_some_and(|age| age >= stale)
            && observation
                .no_progress_seconds
                .is_some_and(|age| age >= stale)
        {
            return SupervisionDecision::Wake(WakeReason::SuspectedHang);
        }
        SupervisionDecision::NoAction
    }
}

/// Parse and validate a supervision YAML document.
///
/// # Errors
/// Returns [`SupervisionError`] for malformed or unsafe configuration.
pub fn parse(document: &str) -> Result<SupervisionPolicy, SupervisionError> {
    let policy: SupervisionPolicy =
        serde_yaml_ng::from_str(document).map_err(|_| SupervisionError::Document)?;
    policy.validate()?;
    Ok(policy)
}

/// Read this Realm's optional supervision policy.
///
/// # Errors
/// Returns [`SupervisionError`] when a present file cannot be read or validated.
pub fn read(state_root: &Path) -> Result<Option<SupervisionPolicy>, SupervisionError> {
    let path = state_root.join(SUPERVISION_FILE);
    match std::fs::read_to_string(&path) {
        Ok(document) => parse(&document).map(Some),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SupervisionError::Read { path, source }),
    }
}

fn invalid<T>(rule: &'static str) -> Result<T, SupervisionError> {
    Err(SupervisionError::Invalid { rule })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/examples/paseo-supervision.yml");

    #[test]
    fn the_example_is_valid_and_notification_first() {
        let policy = parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(policy.completion.mode, CompletionMode::NotificationFirst);
        assert!(policy.completion.notify_on_finish);
        assert_eq!(policy.max_concurrent_successions(), Some(5));
    }

    #[test]
    fn elapsed_time_without_missing_progress_is_not_a_hang() {
        let policy = parse(EXAMPLE).expect("valid policy");
        let decision = policy.assess(SeatObservation {
            active_turn_age_seconds: Some(1_201),
            no_progress_seconds: Some(10),
            pending_permission: false,
            runtime_error: false,
        });
        assert_eq!(decision, SupervisionDecision::NoAction);
    }

    #[test]
    fn age_and_missing_progress_together_wake_the_orchestrator() {
        let policy = parse(EXAMPLE).expect("valid policy");
        let decision = policy.assess(SeatObservation {
            active_turn_age_seconds: Some(1_200),
            no_progress_seconds: Some(1_200),
            pending_permission: false,
            runtime_error: false,
        });
        assert_eq!(
            decision,
            SupervisionDecision::Wake(WakeReason::SuspectedHang)
        );
    }

    #[test]
    fn an_absent_file_configures_no_implicit_watchdog() {
        let root = tempfile::tempdir().expect("temporary state root");
        assert_eq!(read(root.path()).expect("absence is valid"), None);
    }

    #[test]
    fn schema_one_remains_parse_compatible_and_does_not_enable_succession() {
        let document = EXAMPLE
            .replacen("schema_version: 2", "schema_version: 1", 1)
            .replace("  max_concurrent_successions: 5\n", "");
        let policy = parse(&document).expect("the legacy document remains readable");
        assert_eq!(policy.max_concurrent_successions(), None);
    }

    #[test]
    fn schema_two_refuses_an_absent_or_zero_succession_bound() {
        let absent = EXAMPLE.replace("  max_concurrent_successions: 5\n", "");
        assert!(matches!(
            parse(&absent),
            Err(SupervisionError::Invalid { .. })
        ));

        let zero = EXAMPLE.replacen(
            "  max_concurrent_successions: 5",
            "  max_concurrent_successions: 0",
            1,
        );
        assert!(matches!(
            parse(&zero),
            Err(SupervisionError::Invalid { .. })
        ));

        let excessive = EXAMPLE.replacen(
            "  max_concurrent_successions: 5",
            "  max_concurrent_successions: 65",
            1,
        );
        assert!(matches!(
            parse(&excessive),
            Err(SupervisionError::Invalid { .. })
        ));
    }
}
