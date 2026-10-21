//! The optional `attention.yml` realm policy.
//!
//! This mirrors `supervision.rs` deliberately: a versioned YAML document in the
//! realm state root, read at start, validated into typed errors that name a
//! stable rule rather than a configured value. An absent file means the Realm
//! emits nothing; there is no default destination, ever.
//!
//! The two secret values — the Workflows URL and the mention list — may instead
//! come from an owner-only `<state root>/.env`, so an operator edits one file
//! and never the policy. The `.env` value beats `attention.yml`; both are read
//! once at start, so a change needs a daemon restart.

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
/// The longest single mention target accepted.
const MAX_MENTION_TARGET: usize = 256;
/// The most mention targets one post may carry.
const MAX_MENTIONS: usize = 10;

/// The optional secret overrides file inside a Realm state root.
pub const ENV_FILE: &str = ".env";
/// Overrides the one `teams_workflow` sink's `url`.
const ENV_TEAMS_URL: &str = "KONTOR_ATTENTION_TEAMS_URL";
/// Overrides `mention.target`; a comma-separated list.
const ENV_MENTIONS: &str = "KONTOR_ATTENTION_MENTIONS";
/// The key prefix this module owns inside `.env`.
const ENV_PREFIX: &str = "KONTOR_ATTENTION_";
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

/// The required mention targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionPolicy {
    /// The Teams identities a destination adapter mentions at the edge, in
    /// configured order, deduplicated, between one and ten.
    pub targets: Vec<String>,
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

/// The `KONTOR_ATTENTION_*` values found in `.env`.
///
/// Deliberately not `Debug`: the values are secret and must never reach a log.
#[derive(Default)]
struct EnvOverrides {
    teams_url: Option<String>,
    mentions: Option<String>,
}

/// Parse and validate an attention YAML document.
///
/// # Errors
/// Returns [`AttentionConfigError`] for malformed, incomplete or unsafe
/// configuration. Rules are stable names and never echo a configured value.
pub fn parse(document: &str) -> Result<AttentionPolicy, AttentionConfigError> {
    parse_with(document, &EnvOverrides::default())
}

/// Parse `attention.yml` with the `.env` values applied on top.
fn parse_with(document: &str, env: &EnvOverrides) -> Result<AttentionPolicy, AttentionConfigError> {
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
    if env.teams_url.is_some()
        && raw_sinks
            .iter()
            .filter(|sink| sink.channel.as_deref() == Some("teams_workflow"))
            .count()
            != 1
    {
        return invalid(
            "KONTOR_ATTENTION_TEAMS_URL needs exactly one teams_workflow sink to override",
        );
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
        let url = env.teams_url.clone().or(raw_sink.url);
        let url = url.ok_or(AttentionConfigError::Invalid {
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
    let targets = match (&env.mentions, raw.mention) {
        (Some(list), _) => list.clone(),
        (None, None) => return invalid("mention is required when attention.yml is present"),
        (None, Some(mention)) => mention.target.ok_or(AttentionConfigError::Invalid {
            rule: "mention.target is required and may not be empty",
        })?,
    };
    let targets = parse_mentions(&targets)?;
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
        mention: MentionPolicy { targets },
        emit: EmitPolicy { kinds },
    })
}

/// Read this Realm's optional attention policy and its `.env` overrides.
///
/// `.env` is read only when `attention.yml` is present: the policy file is
/// still what arms the channel.
///
/// # Errors
/// Returns [`AttentionConfigError`] when a present file cannot be read or
/// validated, or when `.env` is readable by anyone but its owner. An absent
/// `attention.yml` is not an error and emits nothing.
pub fn read(state_root: &Path) -> Result<Option<AttentionPolicy>, AttentionConfigError> {
    let path = state_root.join(ATTENTION_FILE);
    let document = match std::fs::read_to_string(&path) {
        Ok(document) => document,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(AttentionConfigError::Read { path, source }),
    };
    parse_with(&document, &read_env(state_root)?).map(Some)
}

/// Read the optional owner-only `<state root>/.env`.
fn read_env(state_root: &Path) -> Result<EnvOverrides, AttentionConfigError> {
    let path = state_root.join(ENV_FILE);
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(EnvOverrides::default());
        }
        Err(source) => return Err(AttentionConfigError::Read { path, source }),
    };
    // The mode is checked on the open handle, so the file read is the file
    // checked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = file
            .metadata()
            .map_err(|source| AttentionConfigError::Read {
                path: path.clone(),
                source,
            })?;
        if metadata.permissions().mode() & 0o077 != 0 {
            return invalid(".env must be readable only by its owner (chmod 600)");
        }
    }
    let mut document = String::new();
    std::io::Read::read_to_string(&mut file, &mut document)
        .map_err(|source| AttentionConfigError::Read { path, source })?;
    parse_env(&document)
}

/// Parse plain `KEY=VALUE` lines and `#` comments.
///
/// Keys outside `KONTOR_ATTENTION_` belong to someone else and are skipped; an
/// unknown key inside it is a typo that would silently do nothing, so it is
/// refused. No rule echoes a line, because a line may hold a secret.
fn parse_env(document: &str) -> Result<EnvOverrides, AttentionConfigError> {
    let mut overrides = EnvOverrides::default();
    for line in document.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return invalid(".env lines must be KEY=VALUE or # comments");
        };
        let slot = match key.trim() {
            ENV_TEAMS_URL => &mut overrides.teams_url,
            ENV_MENTIONS => &mut overrides.mentions,
            other if other.starts_with(ENV_PREFIX) => {
                return invalid(".env names an unknown KONTOR_ATTENTION_ key");
            }
            _ => continue,
        };
        if slot.replace(unquote(value.trim()).to_owned()).is_some() {
            return invalid(".env names a KONTOR_ATTENTION_ key more than once");
        }
    }
    Ok(overrides)
}

/// Strip one pair of matching surrounding quotes.
fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Split a comma-separated mention list into validated, deduplicated targets.
fn parse_mentions(list: &str) -> Result<Vec<String>, AttentionConfigError> {
    let mut targets: Vec<String> = Vec::new();
    for target in list
        .split(',')
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        if target.chars().count() > MAX_MENTION_TARGET
            || target
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
            || target.contains('<')
            || target.contains('>')
        {
            return invalid("mention.target is a comma-separated list of bounded identity tokens");
        }
        // Teams identities are e-mail-shaped and compare case-insensitively.
        if !targets
            .iter()
            .any(|known| known.eq_ignore_ascii_case(target))
        {
            targets.push(target.to_owned());
        }
    }
    if targets.is_empty() {
        return invalid("mention.target is required and may not be empty");
    }
    if targets.len() > MAX_MENTIONS {
        return invalid("mention.target names at most 10 identities");
    }
    Ok(targets)
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

    const SECRET_URL: &str = "https://secret-host.example/workflows/secret-signature";

    fn env(teams_url: Option<&str>, mentions: Option<&str>) -> EnvOverrides {
        EnvOverrides {
            teams_url: teams_url.map(str::to_owned),
            mentions: mentions.map(str::to_owned),
        }
    }

    #[test]
    fn a_mention_list_is_split_trimmed_deduplicated_and_bounded() {
        assert_eq!(
            parse_mentions(" a@example.com, ,b@example.com,A@EXAMPLE.com,, ")
                .expect("a valid list"),
            ["a@example.com", "b@example.com"]
        );
        assert!(matches!(
            parse_mentions(" , ,"),
            Err(AttentionConfigError::Invalid {
                rule: "mention.target is required and may not be empty"
            })
        ));
        assert!(matches!(
            parse_mentions("a@example.com,b c@example.com"),
            Err(AttentionConfigError::Invalid { .. })
        ));
        assert!(matches!(
            parse_mentions("<at>a</at>"),
            Err(AttentionConfigError::Invalid { .. })
        ));
        let ten: Vec<_> = (0..10).map(|n| format!("u{n}@example.com")).collect();
        assert_eq!(
            parse_mentions(&ten.join(","))
                .expect("ten is the bound")
                .len(),
            10
        );
        let eleven: Vec<_> = (0..11).map(|n| format!("u{n}@example.com")).collect();
        assert!(matches!(
            parse_mentions(&eleven.join(",")),
            Err(AttentionConfigError::Invalid {
                rule: "mention.target names at most 10 identities"
            })
        ));
    }

    #[test]
    fn the_yaml_mention_target_is_a_list_too() {
        let document = EXAMPLE.replace(
            "  target: operator@example.com",
            "  target: operator@example.com, second@example.com",
        );
        let policy = parse(&document).expect("a list is valid");
        assert_eq!(
            policy.mention.targets,
            ["operator@example.com", "second@example.com"]
        );
    }

    #[test]
    fn env_values_beat_the_yaml() {
        let policy = parse_with(
            EXAMPLE,
            &env(Some(SECRET_URL), Some("igor@example.com,ops@example.com")),
        )
        .expect("overrides apply");
        assert_eq!(policy.destinations[0].url, SECRET_URL);
        assert_eq!(
            policy.mention.targets,
            ["igor@example.com", "ops@example.com"]
        );
        let plain = parse_with(EXAMPLE, &env(None, None)).expect("no overrides");
        assert_eq!(plain, parse(EXAMPLE).expect("the example"));
    }

    #[test]
    fn the_yaml_may_omit_what_env_supplies() {
        let document = EXAMPLE
            .replace(
                "    url: https://example.logic.azure.com/workflows/attention\n",
                "",
            )
            .replace(
                "mention:\n  # Required: a channel post without a mention does not reliably raise a phone\n  # notification (OQ-OS-2).\n  target: operator@example.com\n",
                "",
            );
        assert!(!document.contains("url:") && !document.contains("mention:"));
        assert!(
            parse(&document).is_err(),
            "without .env the fields are required"
        );
        let policy = parse_with(&document, &env(Some(SECRET_URL), Some("igor@example.com")))
            .expect(".env supplies both");
        assert_eq!(policy.destinations[0].url, SECRET_URL);
        assert_eq!(policy.mention.targets, ["igor@example.com"]);
    }

    #[test]
    fn an_env_url_needs_exactly_one_teams_sink() {
        let document = EXAMPLE.replace(
            "sinks:\n",
            "sinks:\n  - channel: teams_workflow\n    url: https://other.example/w\n    timeout_seconds: 1\n    max_attempts: 1\n",
        );
        assert!(parse(&document).is_ok());
        assert!(matches!(
            parse_with(&document, &env(Some(SECRET_URL), None)),
            Err(AttentionConfigError::Invalid {
                rule: "KONTOR_ATTENTION_TEAMS_URL needs exactly one teams_workflow sink to override"
            })
        ));
    }

    #[test]
    fn the_env_file_is_plain_key_value_lines() {
        let parsed = parse_env(
            "# attention secrets\n\nOTHER_TOOL=ignored\n  KONTOR_ATTENTION_TEAMS_URL = \"https://a.example/w\"\nKONTOR_ATTENTION_MENTIONS=a@example.com,b@example.com\n",
        )
        .expect("a valid file");
        assert_eq!(parsed.teams_url.as_deref(), Some("https://a.example/w"));
        assert_eq!(
            parsed.mentions.as_deref(),
            Some("a@example.com,b@example.com")
        );
        assert!(matches!(
            parse_env("KONTOR_ATTENTION_MENTION=a@example.com\n"),
            Err(AttentionConfigError::Invalid {
                rule: ".env names an unknown KONTOR_ATTENTION_ key"
            })
        ));
        assert!(matches!(
            parse_env("KONTOR_ATTENTION_MENTIONS=a\nKONTOR_ATTENTION_MENTIONS=b\n"),
            Err(AttentionConfigError::Invalid { .. })
        ));
        assert!(matches!(
            parse_env("just a line\n"),
            Err(AttentionConfigError::Invalid { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn the_env_file_is_read_at_start_only_when_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("a state root");
        std::fs::write(root.path().join(ATTENTION_FILE), EXAMPLE).expect("the policy");
        let env_path = root.path().join(ENV_FILE);
        std::fs::write(
            &env_path,
            format!("{ENV_TEAMS_URL}={SECRET_URL}\n{ENV_MENTIONS}=igor@example.com\n"),
        )
        .expect("the env file");
        for mode in [0o644, 0o640, 0o604, 0o660] {
            std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(mode))
                .expect("chmod");
            let refused = read(root.path()).expect_err("a shared .env is refused");
            assert!(matches!(
                refused,
                AttentionConfigError::Invalid {
                    rule: ".env must be readable only by its owner (chmod 600)"
                }
            ));
            assert!(!refused.to_string().contains("secret"));
        }
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        let policy = read(root.path())
            .expect("an owner-only .env is read")
            .expect("the policy is present");
        assert_eq!(policy.destinations[0].url, SECRET_URL);
        assert_eq!(policy.mention.targets, ["igor@example.com"]);

        std::fs::remove_file(root.path().join(ATTENTION_FILE)).expect("remove the policy");
        assert!(
            read(root.path()).expect("absence is valid").is_none(),
            ".env alone arms nothing"
        );
    }

    #[test]
    fn no_error_carries_a_configured_value() {
        let secret = "s3cr3t-value";
        let refusals = [
            parse_env(&format!("{secret}\n")).err(),
            parse_env(&format!("KONTOR_ATTENTION_{secret}={secret}\n")).err(),
            parse_env(&format!(
                "{ENV_MENTIONS}={secret}\n{ENV_MENTIONS}={secret}\n"
            ))
            .err(),
            parse_mentions(&format!("{secret} {secret}")).err(),
            parse_mentions(&format!("<{secret}>")).err(),
            parse_with(EXAMPLE, &env(Some(&format!("ftp://{secret}")), None)).err(),
            parse_with(
                EXAMPLE,
                &env(Some(&format!("https://{secret}:{secret}@h.example/")), None),
            )
            .err(),
            parse_with(EXAMPLE, &env(Some(secret), None)).err(),
        ];
        for refusal in refusals {
            let refusal = refusal.expect("each case is refused");
            assert!(!refusal.to_string().contains(secret), "{refusal}");
            assert!(!format!("{refusal:?}").contains(secret), "{refusal:?}");
        }
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
