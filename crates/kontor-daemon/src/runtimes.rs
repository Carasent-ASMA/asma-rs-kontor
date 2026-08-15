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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontor_api::state::RuntimeRegistry;
use kontor_core::id::{ExternalId, ExternalName, RoleSlotId, RuntimeKindKey};
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::workspace::WorkspaceRoot;
use kontor_runtime_paseo::adapter::{
    PaseoAdapter, PaseoCheckpoint, PaseoConfig, PaseoExecutionScope,
};
use kontor_runtime_paseo::client::PaseoLiveTransport;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// The file a Realm's runtime fleet is configured in.
pub const RUNTIMES_FILE: &str = "runtimes.json";

/// The document generation this build writes and is willing to read.
///
/// Bumped to 4 when ticket and seat display names became explicit configuration
/// rather than guesses from internal ids. Generation 3 may compose the right
/// sessions under misleading names and labels, so it is refused rather than
/// silently upgraded.
const RUNTIMES_SCHEMA: u32 = 4;

/// Families this build knows the name of and deliberately cannot compose.
///
/// A closed list, and the only strings [`FleetError::UnsupportedFamily`] will
/// ever quote — so naming the family in a refusal can never echo operator input.
const DEFERRED_FAMILIES: &[&str] = &["ao", "codex"];

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
    /// A setting names a family this build deliberately cannot compose.
    ///
    /// Distinct from [`Self::Malformed`] on purpose. Malformed says "this is not
    /// a document I wrote"; this says "the document is well formed and names a
    /// runtime this build will not run". An operator who cannot tell the two
    /// apart has no way to know whether to fix their JSON or stop asking for
    /// that family. There is no fallback to Paseo: composing a different runtime
    /// than the one asked for would be worse than refusing.
    #[error("runtime family `{family}` is not supported by this build")]
    UnsupportedFamily {
        /// The family named. Always one of [`DEFERRED_FAMILIES`], never free text.
        family: &'static str,
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
/// The set is closed, and both Codex and AO are deliberately absent — see
/// [`build_registry`] for what wiring Codex needs that this daemon cannot yet
/// supply, and [`DEFERRED_FAMILIES`] for the refusal both earn. A variant that
/// parsed and then failed at startup would be worse than no variant: it is a
/// configuration an operator can write and never make work.
///
/// AO was a variant, and its removal is the whole point of this shape. While it
/// existed an operator could write `family: "ao"`, the daemon would compose a
/// live `AoAdapter`, and the Paseo-only boundary held only by convention. A
/// boundary that depends on nobody writing a line of JSON is not a boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum RuntimeSetting {
    /// One Paseo execution plane, scoped to a mini-project on one host.
    Paseo(PaseoSetting),
}

impl RuntimeSetting {
    /// The family this setting registers under, as written.
    #[must_use]
    pub fn family(&self) -> &str {
        match self {
            Self::Paseo(setting) => &setting.runtime_kind,
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
    /// The Jira epic the mini-project is tracked as.
    pub jira_epic_key: String,
    /// The compact epic title.
    pub mini_project_short_title: String,
    /// The Kontor plan item.
    pub plan_item_key: String,
    /// The Jira issue for this ticket.
    pub jira_issue_key: String,
    /// The runtime-neutral short ticket code.
    pub ticket_short_code: String,
    /// Canonical visible role names for the declared role slots.
    pub seat_display_roles: BTreeMap<String, PaseoSeatDisplaySetting>,
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

/// The visible portion of one canonical Paseo seat title.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoSeatDisplaySetting {
    /// Human-readable role name.
    pub role: String,
    /// Stable suffix when several declared slots share the same role.
    #[serde(default)]
    pub suffix: Option<String>,
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
/// Returns [`FleetError::Io`] when the file exists and cannot be read,
/// [`FleetError::UnsupportedFamily`] when it names a family this build
/// deliberately cannot compose, and [`FleetError::Malformed`] when it is not a
/// document this build wrote.
pub fn read(state_root: &Path) -> Result<RuntimeSettings, FleetError> {
    let bytes = match std::fs::read(path_in(state_root)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimeSettings::default());
        }
        Err(source) => return Err(FleetError::Io { source }),
    };
    // Named refusals first, and deliberately before the generation check: an
    // operator who wrote `family: "ao"` needs to be told that this build does not
    // run AO, not that their document is from another generation. Both are true;
    // only one tells them what to do.
    if let Some(family) = deferred_family(&bytes) {
        return Err(FleetError::UnsupportedFamily { family });
    }
    let settings: RuntimeSettings =
        serde_json::from_slice(&bytes).map_err(|_| FleetError::Malformed)?;
    if settings.schema_version != RUNTIMES_SCHEMA {
        return Err(FleetError::Malformed);
    }
    Ok(settings)
}

/// The first deferred family this document names, if it names one.
///
/// Read from the raw bytes rather than from a parsed [`RuntimeSetting`], because
/// the whole point is that a deferred family no longer *has* a variant to parse
/// into — without this the refusal would be the generic "not a document this
/// build wrote", and an operator would have no way to tell a withdrawn family
/// from a typo.
///
/// Only names drawn from [`DEFERRED_FAMILIES`] are returned, so a refusal can
/// quote the family without ever echoing what the operator actually typed.
fn deferred_family(bytes: &[u8]) -> Option<&'static str> {
    let document: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let written: BTreeSet<&str> = document
        .get("runtimes")?
        .as_array()?
        .iter()
        .filter_map(|setting| setting.get("family")?.as_str())
        .collect();
    DEFERRED_FAMILIES
        .iter()
        .find(|family| written.contains(*family))
        .copied()
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
    let seat_display_roles = setting
        .seat_display_roles
        .iter()
        .map(|(slot, display)| {
            Ok((
                RoleSlotId::parse(slot).map_err(|_| refuse("seat_display_roles slot"))?,
                (
                    ExternalName::parse(&display.role)
                        .map_err(|_| refuse("seat_display_roles role"))?,
                    display
                        .suffix
                        .as_deref()
                        .map(ExternalName::parse)
                        .transpose()
                        .map_err(|_| refuse("seat_display_roles suffix"))?,
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, FleetError>>()?;
    let config = PaseoConfig {
        runtime_kind: runtime_kind.clone(),
        host_key: host_key.clone(),
        mini_project_id: ExternalId::parse(&setting.mini_project_id)
            .map_err(|_| refuse("mini_project_id"))?,
        scope: PaseoExecutionScope {
            jira_epic_key: ExternalId::parse(&setting.jira_epic_key)
                .map_err(|_| refuse("jira_epic_key"))?,
            mini_project_short_title: ExternalName::parse(&setting.mini_project_short_title)
                .map_err(|_| refuse("mini_project_short_title"))?,
            plan_item_key: ExternalId::parse(&setting.plan_item_key)
                .map_err(|_| refuse("plan_item_key"))?,
            jira_issue_key: ExternalId::parse(&setting.jira_issue_key)
                .map_err(|_| refuse("jira_issue_key"))?,
            ticket_short_code: ExternalId::parse(&setting.ticket_short_code)
                .map_err(|_| refuse("ticket_short_code"))?,
            seat_display_roles,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn paseo(runtime_kind: &str) -> RuntimeSetting {
        RuntimeSetting::Paseo(PaseoSetting {
            runtime_kind: runtime_kind.to_owned(),
            host_key: "paseo-host".to_owned(),
            mini_project_id: "mini-1".to_owned(),
            jira_epic_key: "ASMA-1".to_owned(),
            mini_project_short_title: "Epic".to_owned(),
            plan_item_key: "KON-MVP-15".to_owned(),
            jira_issue_key: "ASMA-7759".to_owned(),
            ticket_short_code: "KON-15".to_owned(),
            seat_display_roles: BTreeMap::from([(
                "implement".to_owned(),
                PaseoSeatDisplaySetting {
                    role: "Implement".to_owned(),
                    suffix: None,
                },
            )]),
            project_root_cwd: "/w/epic".to_owned(),
            canonical_worktree_cwd: "/w/task".to_owned(),
            orchestrator_agent_id: "agent-1".to_owned(),
            max_concurrent_sessions: 2,
            executable: "paseo".to_owned(),
            host_target: "https://user:hunter2@paseo.example".to_owned(),
            // Composing a transport builds a client; it connects to nothing.
            endpoint: "ws://127.0.0.1:6767/ws".to_owned(),
            client_id: "kontor-mini-1".to_owned(),
            timeout_seconds: 30,
        })
    }

    /// The fleet document an operator would write for one AO lane.
    fn ao_document() -> &'static [u8] {
        br#"{"schema_version": 4, "runtimes": [{"family": "ao",
             "runtime_kind": "ao.claude-code", "host": "ao-host",
             "project_id": "proj-1", "project_path": "/w/ao-project",
             "kind": "worker", "harness": "claude-code",
             "max_concurrent_sessions": 4, "endpoint": "http://127.0.0.1:1/",
             "timeout_seconds": 5}]}"#
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
            runtimes: vec![paseo("paseo.agent")],
        };
        let registry = build_registry(&settings).expect("the lane composes");
        let families: Vec<String> = registry.families().map(ToString::to_string).collect();
        assert_eq!(families, vec!["paseo.agent".to_owned()]);
        assert!(
            registry
                .get(&RuntimeKindKey::parse("paseo.agent").expect("a valid key"))
                .is_some(),
            "the composed adapter answers under the family its own configuration declares"
        );
    }

    #[test]
    fn two_settings_may_not_claim_one_family() {
        let settings = RuntimeSettings {
            schema_version: RUNTIMES_SCHEMA,
            runtimes: vec![paseo("paseo.agent"), paseo("paseo.agent")],
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
            runtimes: vec![RuntimeSetting::Paseo(PaseoSetting {
                // A value that is refused, and that is spelled like something no
                // log line should ever repeat.
                canonical_worktree_cwd: "relative/hunter2".to_owned(),
                ..match paseo("paseo.agent") {
                    RuntimeSetting::Paseo(setting) => setting,
                }
            })],
        };
        let error = build_registry(&settings).expect_err("a worktree root that is not absolute");
        let rendered = error.to_string();
        assert!(
            rendered.contains("canonical_worktree_cwd"),
            "it names the field"
        );
        assert!(
            !rendered.contains("hunter2"),
            "and never the value, which may be a credential-bearing target"
        );
    }

    #[test]
    fn a_paseo_host_target_is_redacted_in_debug() {
        let setting = PaseoSetting {
            runtime_kind: "paseo.agent".to_owned(),
            host_key: "paseo-host".to_owned(),
            mini_project_id: "mini-1".to_owned(),
            jira_epic_key: "ASMA-1".to_owned(),
            mini_project_short_title: "Epic".to_owned(),
            plan_item_key: "KON-MVP-15".to_owned(),
            jira_issue_key: "ASMA-7759".to_owned(),
            ticket_short_code: "KON-15".to_owned(),
            seat_display_roles: BTreeMap::from([(
                "implement".to_owned(),
                PaseoSeatDisplaySetting {
                    role: "Implement".to_owned(),
                    suffix: None,
                },
            )]),
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
    fn an_ao_fleet_is_refused_by_name_and_nothing_is_composed() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        std::fs::write(path_in(directory.path()), ao_document()).expect("the fixture is written");

        let error = read(directory.path()).expect_err("this build does not run AO");
        assert!(
            matches!(error, FleetError::UnsupportedFamily { family: "ao" }),
            "a withdrawn family earns its own refusal, not a generic one: {error:?}"
        );
        // Named, so an operator knows to stop asking rather than to fix their JSON.
        assert!(error.to_string().contains("ao"), "{error}");

        // And fail *closed*: no fleet is returned, so nothing downstream can
        // compose a substitute. A refusal that fell back to Paseo would run a
        // different runtime than the one the operator described.
        assert!(read(directory.path()).is_err());
    }

    #[test]
    fn every_deferred_family_is_refused_the_same_way() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        for family in DEFERRED_FAMILIES {
            std::fs::write(
                path_in(directory.path()),
                format!(r#"{{"schema_version": 4, "runtimes": [{{"family": "{family}"}}]}}"#),
            )
            .expect("the fixture is written");
            let error = read(directory.path()).expect_err("a deferred family is refused");
            assert!(
                matches!(error, FleetError::UnsupportedFamily { .. }),
                "`{family}` must be refused by name: {error:?}"
            );
        }
    }

    #[test]
    fn an_unknown_family_is_malformed_and_is_never_quoted_back() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        std::fs::write(
            path_in(directory.path()),
            br#"{"schema_version": 4, "runtimes": [{"family": "https://user:hunter2@host"}]}"#,
        )
        .expect("the fixture is written");
        let error = read(directory.path()).expect_err("an unknown family is not a fleet");
        assert!(matches!(error, FleetError::Malformed), "{error:?}");
        assert!(
            !error.to_string().contains("hunter2"),
            "only the closed deferred list is ever quoted back: {error}"
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
