//! Live fleet model routing: read, validate, flatten and reload `fleet.yml`.
//!
//! The model route of a seat used to be frozen into a template version and then
//! copied into every team run that started from it. This module replaces that
//! for the seats a live `fleet.yml` binds: an operator edits one file in the
//! Realm state root and the next placement reads it, with no rebuild, restart or
//! republish.
//!
//! Parsing, validation, flattening, key construction and vendor lookup are
//! [`kontor_fleet`], the one implementation that can also be read without the
//! daemon. This module owns only what touches the Realm state root: the file
//! security checks, the last-valid state, the history copies, the status
//! projection and the decision receipts.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use kontor_core::id::{Timestamp, format_utc_timestamp};
use kontor_core::spec::ModelRung;
use kontor_fleet::MAX_FILE_BYTES;
use kontor_fleet::rule::F04;
use serde::{Deserialize, Serialize};

pub use kontor_fleet::{FleetError, FleetSnapshot};
pub(crate) use kontor_fleet::{FleetRoute, advisor_key, committee_key, independence_key, team_key};

/// The fleet configuration file name inside a Realm state root.
pub(crate) const FLEET_FILE: &str = "fleet.yml";

/// One immutable copy of every accepted fleet configuration.
pub(crate) const FLEET_HISTORY_DIR: &str = "fleet-history";

/// One JSON-lines decision log per team run.
pub(crate) const FLEET_DECISIONS_DIR: &str = "fleet-decisions";

/// Whether the last edit was accepted, and why not when it was not.
pub(crate) const FLEET_STATUS_FILE: &str = "fleet-status.json";

const F01: &str = "fleet.yml must be a regular file, not a symlink";
const F02: &str = "fleet.yml must not be writable by group or others";
const F03: &str = "fleet.yml must be owned by the state root's owner";
const F05: &str = "fleet.yml changed while it was being read";
const F06: &str = "fleet.yml is not UTF-8";

/// The rungs one delivery seat may walk, and the fleet binding they came from.
///
/// `fleet` is `Some` exactly when the rungs came from the live `fleet.yml`
/// snapshot rather than the frozen template chain: it carries that snapshot and
/// the binding key so an admitted placement can be recorded against the exact
/// version that authorised it.
#[derive(Debug)]
pub(crate) struct DeclaredRungs {
    pub(crate) rungs: Vec<ModelRung>,
    pub(crate) fleet: Option<(Arc<FleetSnapshot>, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    dev: u64,
    ino: u64,
}

impl Stamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }
}

#[derive(Debug, Default)]
struct Cache {
    stamp: Option<Stamp>,
    good: Option<Arc<FleetSnapshot>>,
    last_error: Option<String>,
    loaded_at: Option<String>,
}

/// One placement decision, written to the decision log and the `info` log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FleetDecision {
    pub(crate) team_run_id: String,
    pub(crate) agent_run_id: String,
    pub(crate) binding_key: String,
    pub(crate) role_slot: String,
    pub(crate) fleet_hash: String,
    pub(crate) step: u16,
    pub(crate) sub_step: u16,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
    pub(crate) vendor: String,
    pub(crate) account_profile_id: Option<String>,
    pub(crate) decided_at: String,
}

#[derive(Serialize)]
struct StatusFile<'a> {
    active_hash: Option<&'a str>,
    loaded_at: Option<&'a str>,
    last_error: Option<&'a str>,
    checked_at: String,
}

/// The live, stamp-tracked fleet configuration for one Realm.
///
/// One process holds one of these per Realm, on [`crate::applications::Services`].
/// It never watches the file; every placement asks it for the current snapshot,
/// and the file is re-parsed only when its stamp changed.
#[derive(Debug)]
pub struct FleetSource {
    state_root: PathBuf,
    cache: Mutex<Cache>,
}

impl FleetSource {
    /// A source rooted at `state_root`; nothing is read until first use.
    pub fn at(state_root: &Path) -> Self {
        Self {
            state_root: state_root.to_path_buf(),
            cache: Mutex::new(Cache::default()),
        }
    }

    /// The current valid snapshot, or `None` when no valid file is present.
    ///
    /// A missing file clears the cache and means "legacy routing". A changed
    /// file is re-read; an invalid edit keeps the last valid snapshot and is
    /// reported in `fleet-status.json`.
    pub fn current(&self) -> Option<Arc<FleetSnapshot>> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self.state_root.join(FLEET_FILE);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let was_tracking =
                    cache.stamp.is_some() || cache.good.is_some() || cache.last_error.is_some();
                cache.stamp = None;
                cache.good = None;
                cache.last_error = None;
                cache.loaded_at = None;
                if was_tracking {
                    self.write_status_file(&cache);
                }
                return None;
            }
            Err(error) => {
                cache.last_error = Some(format!(
                    "the fleet configuration could not be read: {error}"
                ));
                return cache.good.clone();
            }
        };
        let stamp = Stamp::of(&metadata);
        if cache.stamp == Some(stamp) {
            return cache.good.clone();
        }
        cache.stamp = Some(stamp);
        match self.load(&path) {
            Ok(snapshot) => {
                let snapshot = Arc::new(snapshot);
                let changed = cache
                    .good
                    .as_ref()
                    .is_none_or(|good| good.hash() != snapshot.hash());
                if changed {
                    self.write_history(&snapshot);
                    tracing::info!(fleet_hash = %snapshot.hash(), "fleet.loaded");
                    cache.good = Some(snapshot);
                    cache.loaded_at = Some(format_utc_timestamp(Timestamp::now()));
                }
                cache.last_error = None;
            }
            Err(error) => {
                tracing::warn!(error = %error, "fleet.rejected");
                cache.last_error = Some(error.to_string());
            }
        }
        self.write_status_file(&cache);
        cache.good.clone()
    }

    /// Append one admitted placement to the run's decision log and the `info` log.
    ///
    /// # Errors
    /// Returns the I/O failure so the caller can fail the placement: an
    /// unrecorded route would silently break `independent_of`.
    pub(crate) fn record_decision(&self, decision: &FleetDecision) -> std::io::Result<()> {
        let directory = self.state_root.join(FLEET_DECISIONS_DIR);
        create_private_dir(&directory)?;
        let path = directory.join(format!("{}.jsonl", decision.team_run_id));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let line = serde_json::to_string(decision).map_err(std::io::Error::other)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        tracing::info!(
            team_run_id = %decision.team_run_id,
            agent_run_id = %decision.agent_run_id,
            binding_key = %decision.binding_key,
            role_slot = %decision.role_slot,
            fleet_hash = %decision.fleet_hash,
            step = decision.step,
            sub_step = decision.sub_step,
            provider = %decision.provider,
            model = %decision.model,
            effort = decision.effort.as_deref().unwrap_or(""),
            vendor = %decision.vendor,
            account = decision.account_profile_id.as_deref().unwrap_or(""),
            "fleet.route_decided"
        );
        Ok(())
    }

    /// The vendor of the last recorded route for `binding_key` in this run.
    pub(crate) fn last_vendor(&self, team_run_id: &str, binding_key: &str) -> Option<String> {
        let path = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(format!("{team_run_id}.jsonl"));
        let text = std::fs::read_to_string(path).ok()?;
        let mut vendor = None;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<FleetDecision>(line) {
                Ok(decision) => {
                    if decision.binding_key == binding_key {
                        vendor = Some(decision.vendor);
                    }
                }
                Err(_) => tracing::warn!(team_run_id, "fleet.decision_line_unreadable"),
            }
        }
        vendor
    }

    fn load(&self, path: &Path) -> Result<FleetSnapshot, FleetError> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|source| FleetError::Read { source })?;
        if !metadata.file_type().is_file() {
            return Err(invalid(F01));
        }
        if metadata.mode() & 0o022 != 0 {
            return Err(invalid(F02));
        }
        let root = std::fs::symlink_metadata(&self.state_root)
            .map_err(|source| FleetError::Read { source })?;
        if metadata.uid() != root.uid() {
            return Err(invalid(F03));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let file = std::fs::File::open(path).map_err(|source| FleetError::Read { source })?;
        let opened = file
            .metadata()
            .map_err(|source| FleetError::Read { source })?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(invalid(F05));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| FleetError::Read { source })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let document = std::str::from_utf8(&bytes).map_err(|_| invalid(F06))?;
        FleetSnapshot::parse(document)
    }

    fn write_history(&self, snapshot: &FleetSnapshot) {
        let directory = self.state_root.join(FLEET_HISTORY_DIR);
        if let Err(error) = create_private_dir(&directory) {
            tracing::warn!(error = %error, "fleet.history_write_failed");
            return;
        }
        let target = directory.join(format!("{}.yml", snapshot.hash()));
        if target.exists() {
            return;
        }
        let temporary = directory.join(format!("{}.yml.tmp", snapshot.hash()));
        let result = write_owner_only(&temporary, snapshot.raw().as_bytes())
            .and_then(|()| std::fs::rename(&temporary, &target));
        if let Err(error) = result {
            tracing::warn!(error = %error, "fleet.history_write_failed");
            let _ = std::fs::remove_file(&temporary);
        }
    }

    fn write_status_file(&self, cache: &Cache) {
        let status = StatusFile {
            active_hash: cache.good.as_ref().map(|good| good.hash().as_str()),
            loaded_at: cache.loaded_at.as_deref(),
            last_error: cache.last_error.as_deref(),
            checked_at: format_utc_timestamp(Timestamp::now()),
        };
        if let Err(error) = self.write_status(&status) {
            tracing::warn!(error = %error, "fleet.status_write_failed");
        }
    }

    fn write_status(&self, status: &StatusFile<'_>) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(status).map_err(std::io::Error::other)?;
        let temporary =
            self.state_root
                .join(format!("{}.{}.tmp", FLEET_STATUS_FILE, std::process::id()));
        write_owner_only(&temporary, &bytes)?;
        std::fs::rename(&temporary, self.state_root.join(FLEET_STATUS_FILE))
    }
}

fn invalid(rule: &'static str) -> FleetError {
    FleetError::Invalid { rule }
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::spec::{ModelRef, ProviderRef};
    use kontor_fleet::rule::*;
    use std::os::unix::fs::PermissionsExt;

    const EXAMPLE: &str = include_str!("../../../config/examples/fleet.yml");

    fn write_fleet(root: &Path, contents: &str) -> PathBuf {
        let path = root.join(FLEET_FILE);
        std::fs::write(&path, contents).expect("write fleet.yml");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only mode");
        path
    }

    fn read_status(root: &Path) -> serde_json::Value {
        let bytes = std::fs::read(root.join(FLEET_STATUS_FILE)).expect("status file");
        serde_json::from_slice(&bytes).expect("status is JSON")
    }

    fn decision(binding_key: &str, vendor: &str) -> FleetDecision {
        FleetDecision {
            team_run_id: "run-1".to_owned(),
            agent_run_id: "agent-1".to_owned(),
            binding_key: binding_key.to_owned(),
            role_slot: "slot".to_owned(),
            fleet_hash: "hash".to_owned(),
            step: 1,
            sub_step: 1,
            provider: "provider".to_owned(),
            model: "model".to_owned(),
            effort: None,
            vendor: vendor.to_owned(),
            account_profile_id: None,
            decided_at: "2026-09-24T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn the_shipped_example_parses() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(snapshot.raw(), EXAMPLE);
        assert!(!snapshot.hash().as_str().is_empty());
    }

    #[test]
    fn each_rule_in_appendix_b_is_enforced() {
        let accounts: Vec<String> = std::iter::once("cursor".to_owned())
            .chain((1..=64).map(|index| format!("cursor-a{index}")))
            .collect();
        let cases: Vec<(&str, String)> = vec![
            (V01, EXAMPLE.replacen("schema_version: 1", "schema_version: 2", 1)),
            (
                V02,
                EXAMPLE.replacen("  claude:     { provider:", "  Claude:     { provider:", 1),
            ),
            (
                V03,
                EXAMPLE.replacen("provider: claude,   accounts", "provider: bogus,    accounts", 1),
            ),
            (
                V04,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: []",
                    1,
                ),
            ),
            (
                V05,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: [personal]",
                    1,
                ),
            ),
            (
                V06,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: [claude-personal, claude-personal]",
                    1,
                ),
            ),
            (
                V07,
                EXAMPLE.replacen(", model_prefix: \"deepseek/\"", "", 1),
            ),
            (
                V08,
                EXAMPLE.replacen("  opus-5.5:       { domain:", "  Opus-5.5:       { domain:", 1),
            ),
            (
                V09,
                EXAMPLE.replacen("opus-5.5:       { domain: claude,", "opus-5.5:       { domain: nope,", 1),
            ),
            (
                V10,
                EXAMPLE.replacen(
                    "id: openrouter/z-ai/glm-5.3-flash,",
                    "id: z-ai/glm-5.3-flash,",
                    1,
                ),
            ),
            (V11, EXAMPLE.replacen("vendor: deepseek,", "vendor: DeepSeek,", 1)),
            (V12, EXAMPLE.replacen("max, ultra]", "max, bogus]", 1)),
            (
                V13,
                EXAMPLE.replacen("efforts: [low, high, max]", "efforts: [low, high, high]", 1),
            ),
            (
                V14,
                EXAMPLE.replacen(
                    "id: deepseek/deepseek-flash,",
                    "id: deepseek/deepseek-v4-pro,",
                    1,
                ),
            ),
            (V15, EXAMPLE.replacen("  review-a:", "  Review-a:", 1)),
            (
                V16,
                EXAMPLE.replacen("    - [sol@xhigh]", "    - []", 1),
            ),
            (V17, EXAMPLE.replace("- [sol@xhigh]", "- [soll@xhigh]")),
            (V18, EXAMPLE.replace("grok-4.7@xhigh", "grok-4.7@ultra")),
            (
                V19,
                EXAMPLE.replacen(
                    "    - [sol@xhigh]",
                    "    - [sol@xhigh, grok-4.7@xhigh]",
                    1,
                ),
            ),
            (
                V20,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [sol@xhigh]",
                    1,
                ),
            ),
            (
                V21,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [grok-4.7@xhigh, grok-4.7@xhigh]",
                    1,
                ),
            ),
            (
                V22,
                EXAMPLE.replace(
                    "accounts: [cursor]",
                    &format!("accounts: [{}]", accounts.join(", ")),
                ),
            ),
            (
                V23,
                EXAMPLE.replacen(
                    "  team/01936f5a-0000-7000-8000-000000000101/scope:",
                    "  core/01936f5a-0000-7000-8000-000000000101/scope:",
                    1,
                ),
            ),
            (
                V24,
                EXAMPLE.replacen(
                    "  team/01936f5a-0000-7000-8000-000000000101/scope: codex-first",
                    "  team/01936f5a-0000-7000-8000-000000000101/scope: nope",
                    1,
                ),
            ),
            (
                V25,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [grok-4.7@xhigh, grok-4.6@high, cursor-auto]",
                    1,
                ),
            ),
            (
                V25,
                EXAMPLE.replace(
                    "    - [composer-2.5, grok-4.5@high]",
                    "    - [composer-2.5, grok-4.5@high, cursor-auto]",
                ),
            ),
            (
                V26,
                EXAMPLE.replacen(
                    "domains: [openrouter, deepseek]",
                    "domains: [openrouter, deepseek, nope]",
                    1,
                ),
            ),
            (
                V27,
                EXAMPLE.replacen(
                    "accounts: [claude-work]",
                    "accounts: [claude-work, nope]",
                    1,
                ),
            ),
            (
                V28,
                EXAMPLE.replacen(
                    "  vision_required:",
                    "    - team/01936f5a-0000-7000-8000-000000000101/nope\n  vision_required:",
                    1,
                ),
            ),
            (
                V29,
                EXAMPLE.replacen(
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/implement",
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/verify",
                    1,
                ),
            ),
            (
                V30,
                EXAMPLE.replacen(
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/implement",
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000102/implement",
                    1,
                ),
            ),
        ];
        for (expected, yaml) in cases {
            match FleetSnapshot::parse(&yaml) {
                Err(FleetError::Invalid { rule }) => {
                    assert_eq!(rule, expected, "the first refusal must be the edited rule");
                }
                other => panic!("expected {expected}, got {other:?}"),
            }
        }
    }

    #[test]
    fn malformed_yaml_and_unknown_fields_are_document_errors() {
        assert!(matches!(
            FleetSnapshot::parse("schema_version: [1"),
            Err(FleetError::Document)
        ));
        let unknown = EXAMPLE.replace("schema_version: 1", "schema_version: 1\nunknown_section: 1");
        assert!(matches!(
            FleetSnapshot::parse(&unknown),
            Err(FleetError::Document)
        ));
    }

    #[test]
    fn flattening_is_model_major_then_account() {
        let yaml = "\
schema_version: 1
domains:
  claude: { provider: claude, accounts: [claude-personal, claude-work] }
  codex: { provider: codex, accounts: [codex-work] }
models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  opus-4.8: { domain: claude, id: claude-opus-4-8, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [opus-5@xhigh, opus-4.8@xhigh]
    - [sol@xhigh]
bindings:
  team/t/s: c
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        let seen: Vec<(&str, &str, u16, u16)> = routes
            .iter()
            .map(|route| {
                (
                    route.rung.provider.0.as_str(),
                    route.rung.model.0.as_str(),
                    route.step,
                    route.sub_step,
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("claude-personal", "claude-opus-5", 1, 1),
                ("claude-work", "claude-opus-5", 1, 2),
                ("claude-personal", "claude-opus-4-8", 1, 3),
                ("claude-work", "claude-opus-4-8", 1, 4),
                ("codex-work", "gpt-5.6-sol", 2, 1),
            ]
        );
    }

    #[test]
    fn an_unavailable_domain_or_account_is_skipped() {
        let yaml = "\
schema_version: 1
domains:
  claude: { provider: claude, accounts: [claude-personal, claude-work] }
  codex: { provider: codex, accounts: [codex-work] }
unavailable:
  domains: [codex]
  accounts: [claude-work]
models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [sol@xhigh]
    - [opus-5@xhigh]
bindings:
  team/t/s: c
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].rung.provider.0, "claude-personal");
        assert_eq!(routes[0].step, 2);
        assert_eq!(routes[0].sub_step, 1);
    }

    #[test]
    fn calibration_and_vision_rules_drop_models() {
        let yaml = "\
schema_version: 1
domains:
  cursor: { provider: cursor, accounts: [cursor] }
models:
  grok-4.7: { domain: cursor, id: grok-4.7, vendor: xai, efforts: [xhigh], vision: true, calibrated: false }
  grok-4.6: { domain: cursor, id: grok-4.6, vendor: xai, efforts: [xhigh], vision: true, calibrated: true }
  composer: { domain: cursor, id: composer-2.5, vendor: cursor, vision: false, calibrated: false }
chains:
  c:
    - [grok-4.7@xhigh, grok-4.6@xhigh, composer]
bindings:
  team/t/s: c
  team/t/unlisted: c
rules:
  calibration_required: [team/t/s]
  vision_required: [team/t/s]
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].rung.model.0, "grok-4.6");
        // Both rules are scoped to the keys they list. A seat outside them keeps
        // the uncalibrated and non-vision models, which is what tells a scoped
        // rule from one applied to every key.
        let unlisted = snapshot.routes_for("team/t/unlisted").expect("bound");
        assert_eq!(
            unlisted
                .iter()
                .map(|route| route.rung.model.0.as_str())
                .collect::<Vec<_>>(),
            ["grok-4.7", "grok-4.6", "composer-2.5"]
        );
    }

    #[test]
    fn an_unbound_key_returns_none() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        assert!(snapshot.routes_for("team/does-not-exist/slot").is_none());
    }

    #[test]
    fn an_unknown_vendor_has_no_independence_key() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        let rung = ModelRung {
            provider: ProviderRef("cursor".to_owned()),
            model: ModelRef("auto-smart".to_owned()),
            effort: None,
        };
        assert_eq!(snapshot.vendor_of(&rung), Some("unknown"));
        assert_eq!(independence_key(&rung, Some(&snapshot)), None);
    }

    #[test]
    fn a_listed_route_must_match_the_models_efforts() {
        use kontor_core::spec::EffortLevel;
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        let route = |model: &str, effort: Option<EffortLevel>| ModelRung {
            provider: ProviderRef("cursor".to_owned()),
            model: ModelRef(model.to_owned()),
            effort,
        };
        // `composer-2.5` declares no efforts, so only an effort-less route is it.
        assert!(snapshot.lists(&route("composer-2.5", None)));
        assert!(!snapshot.lists(&route("composer-2.5", Some(EffortLevel::High))));
        // `grok-4.7` declares efforts, so a route must name one of them.
        assert!(snapshot.lists(&route("grok-4.7", Some(EffortLevel::Xhigh))));
        assert!(!snapshot.lists(&route("grok-4.7", Some(EffortLevel::Max))));
        assert!(!snapshot.lists(&route("grok-4.7", None)));
    }

    #[test]
    fn without_a_fleet_claude_and_codex_map_to_their_vendor() {
        let claude = ModelRung {
            provider: ProviderRef("claude-work".to_owned()),
            model: ModelRef("claude-opus-5".to_owned()),
            effort: None,
        };
        let codex = ModelRung {
            provider: ProviderRef("codex-work".to_owned()),
            model: ModelRef("gpt-5.6-sol".to_owned()),
            effort: None,
        };
        assert_eq!(
            independence_key(&claude, None),
            Some("anthropic".to_owned())
        );
        assert_eq!(independence_key(&codex, None), Some("openai".to_owned()));
    }

    #[test]
    fn a_missing_file_means_no_fleet() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        assert!(source.current().is_none());
    }

    #[test]
    fn an_edit_is_picked_up_without_restart() {
        let root = tempfile::tempdir().expect("temporary state root");
        let first_yaml = "\
schema_version: 1
domains:
  codex: { provider: codex, accounts: [codex-work] }
  claude: { provider: claude, accounts: [claude-personal] }
models:
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
  opus: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [sol@xhigh]
    - [opus@xhigh]
bindings:
  team/t/s: c
";
        let second_yaml = "\
schema_version: 1
domains:
  codex: { provider: codex, accounts: [codex-work] }
  claude: { provider: claude, accounts: [claude-personal] }
models:
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
  opus: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [opus@xhigh]
    - [sol@xhigh]
bindings:
  team/t/s: c
";
        write_fleet(root.path(), first_yaml);
        let source = FleetSource::at(root.path());
        let first = source.current().expect("loaded");
        write_fleet(root.path(), second_yaml);
        let second = source.current().expect("reloaded");
        assert_ne!(first.hash(), second.hash());
        assert_eq!(
            second.routes_for("team/t/s").expect("bound")[0]
                .rung
                .provider
                .0,
            "claude-personal"
        );
    }

    #[test]
    fn an_invalid_edit_keeps_the_last_valid_snapshot() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), EXAMPLE);
        let source = FleetSource::at(root.path());
        let good = source.current().expect("loaded");
        write_fleet(root.path(), "schema_version: [");
        let after = source.current().expect("keeps the last valid snapshot");
        assert_eq!(after.hash(), good.hash());
    }

    #[test]
    fn a_group_writable_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = root.path().join(FLEET_FILE);
        std::fs::write(&path, EXAMPLE).expect("write fleet.yml");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("mode");
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F02),
            other => panic!("expected a group-writable refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_symlink_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let real = root.path().join("real.yml");
        std::fs::write(&real, EXAMPLE).expect("write real.yml");
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let link = root.path().join(FLEET_FILE);
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let source = FleetSource::at(root.path());
        match source.load(&link) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F01),
            other => panic!("expected a symlink refusal, got {other:?}"),
        }
    }

    #[test]
    fn an_oversized_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = write_fleet(root.path(), &"x".repeat(MAX_FILE_BYTES as usize + 1));
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F04),
            other => panic!("expected an oversized refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_non_utf8_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = root.path().join(FLEET_FILE);
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).expect("write invalid UTF-8");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F06),
            other => panic!("expected a UTF-8 refusal, got {other:?}"),
        }
    }

    #[test]
    fn each_accepted_hash_is_written_to_history_once() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), EXAMPLE);
        let source = FleetSource::at(root.path());
        let first = source.current().expect("loaded");
        let history = root.path().join(FLEET_HISTORY_DIR);
        assert!(history.join(format!("{}.yml", first.hash())).is_file());
        source.current();
        assert_eq!(
            std::fs::read_dir(&history)
                .expect("history directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yml"))
                .count(),
            1
        );
    }

    #[test]
    fn the_status_file_reports_the_last_error() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(
            root.path(),
            &EXAMPLE.replacen("schema_version: 1", "schema_version: 10", 1),
        );
        let source = FleetSource::at(root.path());
        assert!(source.current().is_none());
        let status = read_status(root.path());
        assert!(status["active_hash"].is_null());
        assert!(
            status["last_error"]
                .as_str()
                .is_some_and(|error| error.contains("schema_version must be 1"))
        );

        write_fleet(root.path(), EXAMPLE);
        let snapshot = source.current().expect("valid edit is accepted");
        let status = read_status(root.path());
        assert_eq!(
            status["active_hash"].as_str(),
            Some(snapshot.hash().as_str())
        );
        assert!(status["last_error"].is_null());
    }

    #[test]
    fn a_recorded_decision_is_the_last_vendor() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        source
            .record_decision(&decision("team/t/s", "anthropic"))
            .expect("record");
        assert_eq!(
            source.last_vendor("run-1", "team/t/s"),
            Some("anthropic".to_owned())
        );
        source
            .record_decision(&decision("team/t/s", "openai"))
            .expect("record");
        assert_eq!(
            source.last_vendor("run-1", "team/t/s"),
            Some("openai".to_owned())
        );
        assert_eq!(source.last_vendor("run-1", "team/other/s"), None);
    }

    #[test]
    fn every_rule_string_is_verbatim_from_appendix_b() {
        assert_eq!(F01, "fleet.yml must be a regular file, not a symlink");
        assert_eq!(F02, "fleet.yml must not be writable by group or others");
        assert_eq!(F03, "fleet.yml must be owned by the state root's owner");
        assert_eq!(F04, "fleet.yml exceeds 256 KiB");
        assert_eq!(F05, "fleet.yml changed while it was being read");
        assert_eq!(F06, "fleet.yml is not UTF-8");
        assert_eq!(V01, "schema_version must be 1");
        assert_eq!(V02, "a domain name must be a lowercase slug");
        assert_eq!(
            V03,
            "a domain provider must be claude, codex, cursor or opencode"
        );
        assert_eq!(V04, "a domain must list at least one account");
        assert_eq!(
            V05,
            "an account alias must be its domain's provider or start with the provider and a hyphen"
        );
        assert_eq!(V06, "a domain lists an account twice");
        assert_eq!(
            V07,
            "domains that share an account must each declare a different model prefix"
        );
        assert_eq!(V08, "a model name must be a lowercase slug");
        assert_eq!(V09, "a model names an unknown domain");
        assert_eq!(
            V10,
            "a model id must be non-empty and start with its domain's model prefix"
        );
        assert_eq!(V11, "a model vendor must be a lowercase slug");
        assert_eq!(
            V12,
            "a model effort is not in the runtime effort vocabulary"
        );
        assert_eq!(V13, "a model lists an effort twice");
        assert_eq!(V14, "a model route failed route validation");
        assert_eq!(V15, "a chain name must be a lowercase slug");
        assert_eq!(V16, "a chain must have 1 to 16 non-empty steps");
        assert_eq!(V17, "a chain entry names an unknown model");
        assert_eq!(
            V18,
            "a chain entry effort does not match the model's efforts"
        );
        assert_eq!(V19, "every entry in one step must use the same domain");
        assert_eq!(V20, "a chain uses the same domain in two steps");
        assert_eq!(V21, "a chain repeats a model");
        assert_eq!(V22, "a chain flattens to more than 64 routes");
        assert_eq!(
            V23,
            "a binding key must be team/<id>/<slot>, committee/<id>/<slot> or advisor/<id>"
        );
        assert_eq!(V24, "a binding names an unknown chain");
        assert_eq!(
            V25,
            "a Committee, Advisor or independent seat chain may not use a model with vendor unknown"
        );
        assert_eq!(V26, "an unavailable domain is not declared");
        assert_eq!(V27, "an unavailable account is not in any domain");
        assert_eq!(V28, "a rule names a seat key that has no binding");
        assert_eq!(V29, "a seat cannot be independent of itself");
        assert_eq!(
            V30,
            "independent_of pairs must be two seats of the same team template"
        );
    }
}
