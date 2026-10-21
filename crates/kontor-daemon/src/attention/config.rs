//! The optional `attention.yml` realm policy.
//!
//! This mirrors `supervision.rs` deliberately: a versioned YAML document in the
//! realm state root, read at start, validated into typed errors that name a
//! stable rule rather than a configured value. An absent file means the Realm
//! emits nothing; there is no default destination, ever.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::attention::payload::AlertKind;

/// The optional policy file inside a Realm state root.
pub const ATTENTION_FILE: &str = "attention.yml";

/// The lowest and highest accepted delivery timeout.
const MIN_TIMEOUT_SECONDS: u64 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 300;
/// The lowest and highest accepted attempt count.
const MIN_ATTEMPTS: u32 = 1;
const MAX_ATTEMPTS: u32 = 10;
/// The longest mention target accepted.
const MAX_MENTION_TARGET: usize = 256;
/// The largest destination list accepted.
const MAX_DESTINATIONS: usize = 8;

/// Raw agent-attention names a config may not select.
///
/// Paseo's native push owns these (REQ-004); Kontor duplicating them would ping
/// the operator twice for one event.
const PASEO_OWNED_KINDS: &[&str] = &[
    "permission_requested",
    "permission_request",
    "turn_finished",
    "turn_completed",
    "agent_attention",
    "agent_finished",
];

/// Why the attention policy could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AttentionConfigError {
    /// The file exists but could not be read.
    #[error("the attention policy could not be read")]
    Read {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is not valid YAML for this schema.
    #[error("the attention policy is not a valid supported YAML document")]
    Document,
    /// The document is structurally valid but unsafe or contradictory.
    #[error("the attention policy is invalid: {rule}")]
    Invalid {
        /// The stable rule, never a configured value.
        rule: &'static str,
    },
}

/// One validated attention policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionPolicy {
    /// Schema generation of this document.
    pub schema_version: u32,
    /// The configured destinations, in delivery order.
    ///
    /// One entry is implemented today (`teams_workflow`), but the list is the
    /// point: a second channel is another entry plus one formatting function,
    /// not a schema and sink-type refactor (REQ-013, TASK-005).
    pub destinations: Vec<DestinationPolicy>,
    /// The mention a channel post must carry to raise a phone notification.
    pub mention: MentionPolicy,
    /// Which Kontor-semantic kinds are enabled.
    pub emit: EmitPolicy,
}

/// Which formatting function a destination uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationChannel {
    /// Microsoft Teams through a Power Automate Workflows webhook.
    TeamsWorkflow,
}

impl DestinationChannel {
    /// The stable wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TeamsWorkflow => "teams_workflow",
        }
    }
}

/// One configured destination. The URL is a consumer of the channel, not a
/// coded-in destination (GUD-002): moving to another mechanism changes this URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationPolicy {
    /// Which destination formatter to use.
    pub channel: DestinationChannel,
    /// The Workflows URL one `POST` goes to.
    pub url: String,
    /// Per-attempt timeout.
    pub timeout_seconds: u64,
    /// Bounded attempts inside the detached delivery task.
    pub max_attempts: u32,
}

/// The required mention target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionPolicy {
    /// The Teams identity a destination adapter mentions at the edge.
    pub target: String,
}

/// The closed emit-kind selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitPolicy {
    /// Enabled kinds. `task_needs_human` is always enabled regardless.
    pub kinds: Vec<AlertKind>,
}

impl EmitPolicy {
    /// Whether one kind may be emitted.
    #[must_use]
    pub fn emits(&self, kind: AlertKind) -> bool {
        kind.is_unsuppressible() || self.kinds.contains(&kind)
    }
}

impl AttentionPolicy {
    /// Whether one kind may be emitted under this policy.
    #[must_use]
    pub fn emits(&self, kind: AlertKind) -> bool {
        self.emit.emits(kind)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    schema_version: u32,
    sinks: Option<Vec<RawSink>>,
    mention: Option<RawMention>,
    emit: Option<RawEmit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSink {
    channel: Option<String>,
    url: Option<String>,
    timeout_seconds: Option<u64>,
    max_attempts: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMention {
    target: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEmit {
    kinds: Option<Vec<String>>,
}

/// Parse and validate an attention YAML document.
///
/// # Errors
/// Returns [`AttentionConfigError`] for malformed, incomplete or unsafe
/// configuration. Rules are stable names and never echo a configured value.
pub fn parse(document: &str) -> Result<AttentionPolicy, AttentionConfigError> {
    let raw: RawPolicy =
        serde_yaml_ng::from_str(document).map_err(|_| AttentionConfigError::Document)?;
    if raw.schema_version != 1 {
        return invalid("schema_version must be 1");
    }
    let raw_sinks = raw.sinks.ok_or(AttentionConfigError::Invalid {
        rule: "sinks is required when attention.yml is present",
    })?;
    if raw_sinks.is_empty() {
        return invalid("sinks must name at least one destination");
    }
    if raw_sinks.len() > MAX_DESTINATIONS {
        return invalid("sinks exceeds the safe destination bound");
    }
    let mut destinations = Vec::with_capacity(raw_sinks.len());
    for raw_sink in raw_sinks {
        let channel = raw_sink.channel.ok_or(AttentionConfigError::Invalid {
            rule: "sinks.channel is required",
        })?;
        let channel = match channel.as_str() {
            "teams_workflow" => DestinationChannel::TeamsWorkflow,
            _ => {
                return invalid(
                    "sinks.channel names only the implemented teams_workflow destination",
                );
            }
        };
        let url = raw_sink.url.ok_or(AttentionConfigError::Invalid {
            rule: "sinks.url is required",
        })?;
        validate_url(&url)?;
        let timeout_seconds = raw_sink
            .timeout_seconds
            .ok_or(AttentionConfigError::Invalid {
                rule: "sinks.timeout_seconds is required",
            })?;
        if !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&timeout_seconds) {
            return invalid("sinks.timeout_seconds is outside the safe bound");
        }
        let max_attempts = raw_sink.max_attempts.ok_or(AttentionConfigError::Invalid {
            rule: "sinks.max_attempts is required",
        })?;
        if !(MIN_ATTEMPTS..=MAX_ATTEMPTS).contains(&max_attempts) {
            return invalid("sinks.max_attempts is outside the safe bound");
        }
        destinations.push(DestinationPolicy {
            channel,
            url,
            timeout_seconds,
            max_attempts,
        });
    }
    let mention = raw.mention.ok_or(AttentionConfigError::Invalid {
        rule: "mention is required when attention.yml is present",
    })?;
    let target = mention.target.ok_or(AttentionConfigError::Invalid {
        rule: "mention.target is required and may not be empty",
    })?;
    if target.trim().is_empty()
        || target.chars().count() > MAX_MENTION_TARGET
        || target
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        || target.contains('<')
        || target.contains('>')
    {
        return invalid("mention.target is one bounded identity token");
    }
    let emit = raw.emit.ok_or(AttentionConfigError::Invalid {
        rule: "emit.kinds is required; absence of the file is how a Realm emits nothing",
    })?;
    let names = emit.kinds.ok_or(AttentionConfigError::Invalid {
        rule: "emit.kinds is required; absence of the file is how a Realm emits nothing",
    })?;
    let mut kinds = Vec::with_capacity(names.len());
    for name in names {
        if PASEO_OWNED_KINDS.contains(&name.as_str()) {
            return invalid(
                "emit.kinds may not name raw agent attention; Paseo's native push already owns permission requests and finished turns (REQ-004)",
            );
        }
        let kind = AlertKind::parse(&name).ok_or(AttentionConfigError::Invalid {
            rule: "emit.kinds names only the closed Kontor attention kinds",
        })?;
        kinds.push(kind);
    }
    let unique: BTreeSet<_> = kinds.iter().copied().collect();
    if unique.len() != kinds.len() {
        return invalid("emit.kinds kinds must be unique");
    }
    Ok(AttentionPolicy {
        schema_version: raw.schema_version,
        destinations,
        mention: MentionPolicy { target },
        emit: EmitPolicy { kinds },
    })
}

/// Read this Realm's optional attention policy.
///
/// # Errors
/// Returns [`AttentionConfigError`] when a present file cannot be read or
/// validated. An absent file is not an error and emits nothing.
pub fn read(state_root: &Path) -> Result<Option<AttentionPolicy>, AttentionConfigError> {
    let path = state_root.join(ATTENTION_FILE);
    match std::fs::read_to_string(&path) {
        Ok(document) => parse(&document).map(Some),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(AttentionConfigError::Read { path, source }),
    }
}

fn validate_url(url: &str) -> Result<(), AttentionConfigError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| AttentionConfigError::Invalid {
        rule: "sinks.url must be an absolute http or https URL",
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return invalid("sinks.url must be an absolute http or https URL");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return invalid("sinks.url may not carry credentials");
    }
    Ok(())
}

fn invalid<T>(rule: &'static str) -> Result<T, AttentionConfigError> {
    Err(AttentionConfigError::Invalid { rule })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../../config/examples/attention.yml");

    #[test]
    fn the_example_is_valid_and_selects_the_kontor_kinds() {
        let policy = parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(policy.destinations.len(), 1);
        assert_eq!(
            policy.destinations[0].channel,
            DestinationChannel::TeamsWorkflow
        );
        assert!(policy.emits(AlertKind::TaskNeedsHuman));
        assert!(policy.emits(AlertKind::TaskBlocked));
        assert!(policy.emits(AlertKind::WaitingSeatStalled));
    }

    #[test]
    fn an_absent_file_emits_nothing_and_has_no_default_destination() {
        let root = tempfile::tempdir().expect("a state root");
        assert!(read(root.path()).expect("absence is valid").is_none());
    }

    #[test]
    fn a_missing_mention_target_is_refused_with_a_rule() {
        let document = EXAMPLE.replace("  target: operator@example.com", "  target:");
        let refused = parse(&document).expect_err("the mention is required");
        assert!(matches!(
            refused,
            AttentionConfigError::Invalid {
                rule: "mention.target is required and may not be empty"
            }
        ));
    }

    #[test]
    fn raw_agent_attention_is_refused_and_names_paseo() {
        let document = EXAMPLE.replace(
            "  kinds:\n    - task_needs_human\n",
            "  kinds:\n    - permission_requested\n",
        );
        let refused = parse(&document).expect_err("raw agent attention is Paseo's");
        assert!(matches!(
            refused,
            AttentionConfigError::Invalid {
                rule: "emit.kinds may not name raw agent attention; Paseo's native push already owns permission requests and finished turns (REQ-004)"
            }
        ));
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let document = EXAMPLE.replace(
            "  kinds:\n    - task_needs_human\n",
            "  kinds:\n    - task_done_sound\n",
        );
        assert!(matches!(
            parse(&document),
            Err(AttentionConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn unsafe_bounds_are_refused() {
        assert!(matches!(
            parse(&EXAMPLE.replace("    timeout_seconds: 10", "    timeout_seconds: 0")),
            Err(AttentionConfigError::Invalid { .. })
        ));
        assert!(matches!(
            parse(&EXAMPLE.replace("    max_attempts: 3", "    max_attempts: 99")),
            Err(AttentionConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn needs_human_is_never_suppressed_by_configuration() {
        let document = r#"schema_version: 1
sinks:
  - channel: teams_workflow
    url: http://127.0.0.1:9999/workflows/attention
    timeout_seconds: 1
    max_attempts: 1
mention:
  target: operator@example.com
emit:
  kinds: []
"#;
        let policy = parse(document).expect("an empty kind list is valid");
        assert!(policy.emits(AlertKind::TaskNeedsHuman));
        assert!(!policy.emits(AlertKind::TaskBlocked));
    }

    #[test]
    fn a_missing_url_is_refused() {
        let document = EXAMPLE.replace(
            "    url: https://example.logic.azure.com/workflows/attention\n",
            "",
        );
        assert!(matches!(
            parse(&document),
            Err(AttentionConfigError::Invalid { .. })
        ));
    }
}
