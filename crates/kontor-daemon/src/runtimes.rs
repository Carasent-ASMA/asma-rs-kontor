//! The configured runtime fleet: from a file in the state root to live adapters
//! in the registry.
//!
//! This is the path that makes a session route runtime-backed. Without it the
//! daemon starts with an empty registry and every `/v1/sessions/…` route answers
//! "this daemon is not configured with the runtime that owns the session" — which
//! is the honest answer to *no configuration*, and a defect when configuration
//! exists.
//!
//! # What a setting may and may not carry
//!
//! A setting names a runtime family, the non-secret facts a lane or plane is
//! scoped by, and how to reach it. Reaching it is the only part that can be
//! sensitive — Paseo's `--host` argument carries its own credential — so that
//! field is held as a [`SecretString`], is redacted in `Debug`, and never reaches
//! a binding, a checkpoint, a receipt or the HTTP surface. The runtime endpoint
//! itself lives here and in the adapter, and nowhere else.
//!
//! # Why every adapter starts from a fresh checkpoint
//!
//! A checkpoint is a runtime's own view of what it was holding, reassembled from
//! this Realm's tables. Rebuilding one at startup is
//! [`crate::Daemon::reconcile`]'s question, and it is answered by *asking the
//! runtime* rather than by trusting a serialized copy — so a newly composed
//! adapter starts empty and learns what exists from reconciliation. An adapter
//! handed a stale checkpoint would believe in sessions nobody has confirmed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{ExternalId, ExternalName, RuntimeKindKey};
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::workspace::WorkspaceRoot;
use kontor_runtime_ao::adapter::{AoAdapter, AoCheckpoint, AoLane};
use kontor_runtime_ao::client::AoHttpTransport;
use kontor_runtime_ao::wire::{AoHarness, AoSessionKind};
use kontor_runtime_paseo::adapter::{
    PaseoAdapter, PaseoCheckpoint, PaseoConfig, PaseoExecutionScope,
};
use kontor_runtime_paseo::client::PaseoLiveTransport;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// The file a Realm's runtime fleet is configured in.
pub const RUNTIMES_FILE: &str = "runtimes.json";

/// The document generation this build writes and is willing to read.
const RUNTIMES_SCHEMA: u32 = 1;

/// The generation every freshly composed adapter starts in.
///
/// Generations count *restarts of the runtime*, and a repeated native id in a new
/// generation is a different session. Composing an adapter is not a runtime
/// restart, so this is a starting point rather than an increment; what the
/// runtime is actually in is discovered by reconciliation.
const INITIAL_GENERATION: u64 = 1;

/// Why a runtime fleet could not be composed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FleetError {
    /// The settings file could not be read.
    #[error("the runtime settings file could not be read: {source}")]
    Io {
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// The settings file is not a document this build understands.
    ///
    /// The parser's own message is deliberately dropped: a serde error quotes the
    /// offending input, and the offending input may be a Paseo host target.
    #[error("the runtime settings file is not a fleet description this build wrote")]
    Malformed,
    /// A setting names a value the domain refuses.
    #[error("runtime `{family}` is misconfigured: {rule}")]
    Invalid {
        /// The family the setting names, or `?` when even that is unreadable.
        family: String,
        /// What was refused. Never the value that was refused.
        rule: &'static str,
    },
    /// Two settings claim the same runtime family.
    #[error("two runtimes are configured under the family `{family}`")]
    DuplicateFamily {
        /// The family claimed twice.
        family: String,
    },
}

/// The runtime fleet one Realm is configured with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSettings {
    /// The document generation.
    pub schema_version: u32,
    /// The configured runtimes, one per family.
    #[serde(default)]
    pub runtimes: Vec<RuntimeSetting>,
}

impl Default for RuntimeSettings {
    /// No runtimes. A Realm with no fleet still serves its whole control plane.
    fn default() -> Self {
        Self {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: Vec::new(),
        }
    }
}

/// One configured runtime.
///
/// The set is closed, and Codex is deliberately absent — see
/// [`build_registry`] for what wiring it needs that this daemon cannot yet
/// supply. A variant that parsed and then failed at startup would be worse than
/// no variant: it is a configuration an operator can write and never make work.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum RuntimeSetting {
    /// One Paseo execution plane, scoped to a mini-project on one host.
    Paseo(PaseoSetting),
    /// One Agent Orchestrator lane.
    Ao(AoSetting),
}

impl RuntimeSetting {
    /// The family this setting registers under, as written.
    #[must_use]
    pub fn family(&self) -> &str {
        match self {
            Self::Paseo(setting) => &setting.runtime_kind,
            Self::Ao(setting) => &setting.runtime_kind,
        }
    }
}

/// One Paseo execution plane.
#[derive(Clone, Serialize, Deserialize)]
pub struct PaseoSetting {
    /// The Kontor runtime-kind key, e.g. `paseo.agent`.
    pub runtime_kind: String,
    /// The non-secret host key bindings are made against.
    pub host_key: String,
    /// The Kontor mini-project this plane serves.
    pub mini_project_id: String,
    /// The Paseo provider every seat on this plane runs under.
    pub provider: String,
    /// The Jira epic the mini-project is tracked as.
    pub jira_epic_key: String,
    /// The compact epic title.
    pub mini_project_short_title: String,
    /// The Kontor plan item.
    pub plan_item_key: String,
    /// The compact task title.
    pub task_short_title: String,
    /// The repository root registered as the epic's Paseo project.
    pub project_root_cwd: String,
    /// The filesystem-canonical task worktree.
    pub canonical_worktree_cwd: String,
    /// The persisted Orchestrator agent every role launches under.
    pub orchestrator_agent_id: String,
    /// The most sessions Kontor holds open on this plane at once.
    pub max_concurrent_sessions: u32,
    /// The `paseo` executable to dispatch.
    pub executable: String,
    /// The complete `--host` argument, credential and all.
    ///
    /// The one sensitive field in this file. It is read into a [`SecretString`]
    /// the moment it is deserialized and is redacted in `Debug`; it reaches the
    /// transport and nothing else.
    pub host_target: String,
    /// The Paseo 0.3.1 session WebSocket endpoint.
    pub endpoint: String,
    /// The stable client id used to resume this plane's socket session.
    pub client_id: String,
    /// The per-command wall-clock budget, in seconds.
    pub timeout_seconds: u64,
}

impl std::fmt::Debug for PaseoSetting {
    /// Written out rather than derived: a derived rendering prints the host
    /// target, and this value is reachable from a settings dump.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaseoSetting")
            .field("runtime_kind", &self.runtime_kind)
            .field("host_key", &self.host_key)
            .field("mini_project_id", &self.mini_project_id)
            .field("max_concurrent_sessions", &self.max_concurrent_sessions)
            .field("host_target", &"<redacted>")
            .finish_non_exhaustive()
    }
}

/// One Agent Orchestrator lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AoSetting {
    /// The Kontor runtime-kind key, e.g. `ao.claude-code`.
    pub runtime_kind: String,
    /// The host label that owns the generation.
    pub host: String,
    /// The AO project id every session in this lane belongs to.
    pub project_id: String,
    /// The project path AO works in.
    pub project_path: String,
    /// `worker` or `orchestrator`.
    pub kind: String,
    /// The client this lane drives, in AO's own spelling.
    pub harness: String,
    /// The most sessions Kontor holds open in this lane at once.
    pub max_concurrent_sessions: u32,
    /// The AO base URL.
    pub endpoint: String,
    /// The per-request wall-clock budget, in seconds.
    pub timeout_seconds: u64,
}

/// The path the fleet description lives at inside `state_root`.
#[must_use]
pub fn path_in(state_root: &Path) -> PathBuf {
    state_root.join(RUNTIMES_FILE)
}

/// Read the fleet this state root is configured with.
///
/// An absent file is not a failure: it is a Realm with no runtimes, which serves
/// its whole control plane and answers session routes as unconfigured.
///
/// # Errors
/// Returns [`FleetError::Io`] when the file exists and cannot be read, and
/// [`FleetError::Malformed`] when it is not a document this build wrote.
pub fn read(state_root: &Path) -> Result<RuntimeSettings, FleetError> {
    let bytes = match std::fs::read(path_in(state_root)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimeSettings::default());
        }
        Err(source) => return Err(FleetError::Io { source }),
    };
    let settings: RuntimeSettings =
        serde_json::from_slice(&bytes).map_err(|_| FleetError::Malformed)?;
    if settings.schema_version != RUNTIMES_SCHEMA {
        return Err(FleetError::Malformed);
    }
    Ok(settings)
}

/// Compose one live adapter per configured runtime.
///
/// Each adapter is keyed by the family its *own* configuration declares, so the
/// key a session's persisted binding carries and the key the registry answers to
/// are the same value rather than two that have to be kept in agreement.
///
/// # Codex
///
/// [`kontor_runtime_codex`] is not composed here, and the reason is structural
/// rather than an omission. Its production account authority
/// (`CodexPinnedAccounts`) borrows a store, an `AccountResolver` — itself bound to
/// a resolver policy of approved config homes and keychain targets — and the
/// fleet's availability boundary. This daemon has no configuration surface for
/// any of those, and the borrows cannot outlive the single private store
/// connection [`kontor_api::state::ApiState`] owns. Wiring it means giving the
/// credential policy a configuration surface, which belongs with the account and
/// CLI work rather than here. Until then a Codex plane is composed by its own
/// caller and handed in through [`RuntimeRegistry`], which is exactly the seam
/// this function uses.
///
/// # Errors
/// Returns [`FleetError::Invalid`] for a setting the domain refuses and
/// [`FleetError::DuplicateFamily`] when two settings claim one family — which
/// would otherwise silently keep whichever was composed last.
pub fn build_registry(settings: &RuntimeSettings) -> Result<RuntimeRegistry, FleetError> {
    let mut registry = RuntimeRegistry::new();
    let mut claimed: BTreeSet<RuntimeKindKey> = BTreeSet::new();
    for setting in &settings.runtimes {
        let (family, adapter) = match setting {
            RuntimeSetting::Paseo(paseo) => compose_paseo(paseo)?,
            RuntimeSetting::Ao(ao) => compose_ao(ao)?,
        };
        if !claimed.insert(family.clone()) {
            return Err(FleetError::DuplicateFamily {
                family: family.to_string(),
            });
        }
        registry = registry.with(family, adapter);
    }
    Ok(registry)
}

/// Build one Paseo execution plane.
fn compose_paseo(
    setting: &PaseoSetting,
) -> Result<(RuntimeKindKey, Arc<dyn RuntimeAdapter>), FleetError> {
    let refuse = |rule: &'static str| FleetError::Invalid {
        family: setting.runtime_kind.clone(),
        rule,
    };
    let runtime_kind =
        RuntimeKindKey::parse(&setting.runtime_kind).map_err(|_| refuse("runtime_kind"))?;
    let host_key = ExternalName::parse(&setting.host_key).map_err(|_| refuse("host_key"))?;
    let config = PaseoConfig {
        runtime_kind: runtime_kind.clone(),
        host_key: host_key.clone(),
        mini_project_id: ExternalId::parse(&setting.mini_project_id)
            .map_err(|_| refuse("mini_project_id"))?,
        provider: ExternalName::parse(&setting.provider).map_err(|_| refuse("provider"))?,
        scope: PaseoExecutionScope {
            jira_epic_key: ExternalId::parse(&setting.jira_epic_key)
                .map_err(|_| refuse("jira_epic_key"))?,
            mini_project_short_title: ExternalName::parse(&setting.mini_project_short_title)
                .map_err(|_| refuse("mini_project_short_title"))?,
            plan_item_key: ExternalId::parse(&setting.plan_item_key)
                .map_err(|_| refuse("plan_item_key"))?,
            task_short_title: ExternalName::parse(&setting.task_short_title)
                .map_err(|_| refuse("task_short_title"))?,
            project_root_cwd: WorkspaceRoot::parse(&setting.project_root_cwd)
                .map_err(|_| refuse("project_root_cwd"))?,
            canonical_worktree_cwd: WorkspaceRoot::parse(&setting.canonical_worktree_cwd)
                .map_err(|_| refuse("canonical_worktree_cwd"))?,
            orchestrator_agent_id: ExternalId::parse(&setting.orchestrator_agent_id)
                .map_err(|_| refuse("orchestrator_agent_id"))?,
        },
        max_concurrent_sessions: setting.max_concurrent_sessions,
    };
    // The credential leaves the settings document here and goes straight into the
    // transport, which is the only thing that may hold it.
    let transport = PaseoLiveTransport::new(
        &setting.executable,
        SecretString::from(setting.host_target.clone()),
        &setting.endpoint,
        &setting.client_id,
        setting.timeout_seconds,
    )
    .map_err(|_| refuse("executable or host target"))?;
    let adapter = PaseoAdapter::new(
        config,
        Box::new(transport),
        PaseoCheckpoint::fresh(INITIAL_GENERATION, host_key),
    )
    .map_err(|_| refuse("execution plane"))?;
    Ok((runtime_kind, Arc::new(adapter)))
}

/// Build one Agent Orchestrator lane.
fn compose_ao(
    setting: &AoSetting,
) -> Result<(RuntimeKindKey, Arc<dyn RuntimeAdapter>), FleetError> {
    let refuse = |rule: &'static str| FleetError::Invalid {
        family: setting.runtime_kind.clone(),
        rule,
    };
    let runtime_kind =
        RuntimeKindKey::parse(&setting.runtime_kind).map_err(|_| refuse("runtime_kind"))?;
    let lane = AoLane {
        runtime_kind: runtime_kind.clone(),
        host: ExternalName::parse(&setting.host).map_err(|_| refuse("host"))?,
        project_id: setting.project_id.clone(),
        project_path: WorkspaceRoot::parse(&setting.project_path)
            .map_err(|_| refuse("project_path"))?,
        kind: match setting.kind.as_str() {
            "worker" => AoSessionKind::Worker,
            "orchestrator" => AoSessionKind::Orchestrator,
            _ => return Err(refuse("kind")),
        },
        harness: AoHarness::parse(&setting.harness).map_err(|_| refuse("harness"))?,
        max_concurrent_sessions: setting.max_concurrent_sessions,
    };
    let transport = AoHttpTransport::new(&setting.endpoint, setting.timeout_seconds)
        .map_err(|_| refuse("endpoint"))?;
    let adapter = AoAdapter::new(
        lane,
        Box::new(transport),
        AoCheckpoint::fresh(INITIAL_GENERATION),
    );
    Ok((runtime_kind, Arc::new(adapter)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ao(runtime_kind: &str) -> RuntimeSetting {
        RuntimeSetting::Ao(AoSetting {
            runtime_kind: runtime_kind.to_owned(),
            host: "ao-host".to_owned(),
            project_id: "proj-1".to_owned(),
            project_path: "/w/ao-project".to_owned(),
            kind: "worker".to_owned(),
            harness: "claude-code".to_owned(),
            max_concurrent_sessions: 4,
            // Composing a transport builds a client; it connects to nothing.
            endpoint: "http://127.0.0.1:1/".to_owned(),
            timeout_seconds: 5,
        })
    }

    #[test]
    fn an_absent_settings_file_is_a_realm_with_no_fleet() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let settings = read(directory.path()).expect("an absent file is not a failure");
        assert!(settings.runtimes.is_empty());
        assert!(
            build_registry(&settings)
                .expect("an empty fleet composes")
                .families()
                .next()
                .is_none()
        );
    }

    #[test]
    fn a_configured_family_is_composed_into_the_registry() {
        let settings = RuntimeSettings {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: vec![ao("ao.claude-code")],
        };
        let registry = build_registry(&settings).expect("the lane composes");
        let families: Vec<String> = registry.families().map(ToString::to_string).collect();
        assert_eq!(families, vec!["ao.claude-code".to_owned()]);
        assert!(
            registry
                .get(&RuntimeKindKey::parse("ao.claude-code").expect("a valid key"))
                .is_some(),
            "the composed adapter answers under the family its own configuration declares"
        );
    }

    #[test]
    fn two_settings_may_not_claim_one_family() {
        let settings = RuntimeSettings {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: vec![ao("ao.claude-code"), ao("ao.claude-code")],
        };
        assert!(
            matches!(
                build_registry(&settings),
                Err(FleetError::DuplicateFamily { .. })
            ),
            "the second would otherwise silently replace the first"
        );
    }

    #[test]
    fn a_refused_value_names_the_field_and_never_the_value() {
        let settings = RuntimeSettings {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: vec![RuntimeSetting::Ao(AoSetting {
                endpoint: "not-a-url".to_owned(),
                ..match ao("ao.claude-code") {
                    RuntimeSetting::Ao(setting) => setting,
                    RuntimeSetting::Paseo(_) => unreachable!("the fixture is a lane"),
                }
            })],
        };
        let error = build_registry(&settings).expect_err("an endpoint that is not a URL");
        let rendered = error.to_string();
        assert!(rendered.contains("endpoint"), "it names the field");
        assert!(
            !rendered.contains("not-a-url"),
            "and never the value, which may be a credential-bearing target"
        );
    }

    #[test]
    fn a_paseo_host_target_is_redacted_in_debug() {
        let setting = PaseoSetting {
            runtime_kind: "paseo.agent".to_owned(),
            host_key: "paseo-host".to_owned(),
            mini_project_id: "mini-1".to_owned(),
            provider: "codex".to_owned(),
            jira_epic_key: "ASMA-1".to_owned(),
            mini_project_short_title: "Epic".to_owned(),
            plan_item_key: "KON-MVP-15".to_owned(),
            task_short_title: "Seat".to_owned(),
            project_root_cwd: "/w/epic".to_owned(),
            canonical_worktree_cwd: "/w/task".to_owned(),
            orchestrator_agent_id: "agent-1".to_owned(),
            max_concurrent_sessions: 2,
            executable: "paseo".to_owned(),
            host_target: "https://user:hunter2@paseo.example".to_owned(),
            endpoint: "ws://127.0.0.1:6767/ws".to_owned(),
            client_id: "kontor-mini-1".to_owned(),
            timeout_seconds: 30,
        };
        let rendered = format!("{setting:?}");
        assert!(
            !rendered.contains("hunter2"),
            "a settings dump must not print the paseo host target"
        );
        assert!(rendered.contains("paseo.agent"), "the family is not secret");

        let settings = RuntimeSettings {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: vec![RuntimeSetting::Paseo(setting)],
        };
        let registry = build_registry(&settings).expect("the complete Paseo plane composes");
        assert!(
            registry
                .get(&RuntimeKindKey::parse("paseo.agent").expect("a valid key"))
                .is_some()
        );
    }

    #[test]
    fn a_document_from_another_generation_is_refused() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        std::fs::write(
            path_in(directory.path()),
            br#"{"schema_version": 99, "runtimes": []}"#,
        )
        .expect("the fixture is written");
        assert!(matches!(read(directory.path()), Err(FleetError::Malformed)));
    }
}
