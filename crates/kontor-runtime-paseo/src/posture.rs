//! The one place a declared posture becomes a provider-native permission state.
//!
//! # Why this is a single function
//!
//! A seat is spawned with a posture and then *read back* to prove it launched
//! under the posture that was declared. When the two derive that answer
//! separately they drift, and the drift is invisible: the launch says one thing,
//! the verification asks a different question, and a seat runs under an
//! authority nobody declared. [`paseo_mode`](crate::client::paseo_mode) already
//! solved that for `--mode` by being shared between `agent_run` and
//! `verify_agent_route`. This module extends the same discipline to the rest of
//! the posture, so mode, permission block and feature intent all come from one
//! evaluation of one input.
//!
//! # Why opencode needs more than a mode
//!
//! An opencode session mode is the shape of the turn, not its permission
//! posture: `--mode build` says nothing about what the seat may do without
//! asking. Posture lives in the `permission` block opencode reads from its
//! project configuration. On 2026-08-22 that gap stalled the ASMA-8001 epic for
//! ~2.5h — twelve of fifteen delivery seats blocked mid-turn on prompts no human
//! was watching, while Kontor recorded them as running.
//!
//! # How the block is evaluated, and why key order matters
//!
//! Verified against the installed OpenCode 1.18.15: its `fromConfig` walks
//! `Object.entries` for the outer map and each nested one, preserving the key
//! order it is given, and `evaluate` resolves a call with `.findLast` — the
//! **last** matching rule wins, and an unmatched tool defaults to `ask`.
//!
//! The order it is given is this workspace's serialization, not insertion order:
//! `serde_json` is pinned at `=1.0.151` and its lock entry pulls in no
//! `indexmap`, so `preserve_order` is off, `serde_json::Map` is a `BTreeMap`,
//! and keys are written **lexicographically**. `*` is a prefix of every floor
//! pattern and so sorts before all of them, which is what lets the specific
//! denials beat the catch-all.
//!
//! This is why [`PermissionAllowance`] may only name a pattern the floor already
//! contains: a merely-overlapping pattern sorts after the deny it overlaps and
//! would be evaluated last.
//!
//! # `deny` is not `ask`
//!
//! [`DESTRUCTIVE_BASH_DENIES`] is denied, never asked, under every posture that
//! writes a block. `ask` blocks and waits for a human, which is precisely what
//! wedged the fleet; `deny` refuses instantly and the seat keeps working.
//! Autonomy and guardrails stop being in tension once the patterns that would
//! earn a refusal are refused rather than escalated.

use crate::client::{built_in_provider, paseo_mode};
use kontor_core::id::ContentHash;
use kontor_core::spec::SeatAutonomy;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use std::io;
use std::path::{Path, PathBuf};

/// The destructive bash patterns a composed opencode seat always refuses.
///
/// A fixed floor rather than a per-seat knob: a posture an operator can widen
/// until it permits `rm -rf` is not a floor. The one ticket that legitimately
/// needs `git rm --cached` relaxes exactly that pattern through a
/// [`PermissionAllowance`], and relaxes nothing else.
pub const DESTRUCTIVE_BASH_DENIES: &[&str] = &[
    "*submodule update*",
    "*submodule deinit*",
    "*git rm --cached*",
    "*git clean -*",
    "*rm -rf *",
];

/// One bounded, operator-declared relaxation of the floor, for one task.
///
/// Allow-only, and **only ever an exact member of [`DESTRUCTIVE_BASH_DENIES`]**.
/// That second rule is not tidiness; without it the type does not do its job.
///
/// OpenCode evaluates permissions by last match, and the block reaches it in
/// lexicographic key order (see the module note on serialization). A pattern
/// that merely *overlaps* a floor entry therefore sorts after it and wins: an
/// allowance of `*git*` renders between `*git rm --cached*` and `*rm -rf *`,
/// matches everything both git denies match, and is evaluated last — so one
/// config line that never spells `*` silently deletes the git half of the floor.
/// `*rm*` and `*submodule*` defeat their families the same way.
///
/// Restricting an allowance to an exact floor key makes the allowance set a
/// subset of the floor by construction. An override can then only ever flip a
/// deny that already exists, in the position it already occupies; it can never
/// introduce a new, later-sorting rule. This subsumes the blank and wildcard
/// refusals rather than enumerating them, and it costs nothing real — an
/// allowance is only meaningful against a rule the floor otherwise denies.
///
/// Allowing a non-floor pattern later would require inserting it so that the
/// floor still evaluates last, which lexicographic serialization cannot promise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PermissionAllowance(String);

impl PermissionAllowance {
    /// Read one declared pattern, or refuse it.
    ///
    /// Accepted only when the trimmed pattern is character-for-character one of
    /// [`DESTRUCTIVE_BASH_DENIES`]. Blank, wildcard, broader (`*git*`), narrower,
    /// prefixed, suffixed, case-variant and unknown patterns are all refused —
    /// each of them would either widen the seat or land in the wrong place in the
    /// evaluated order.
    #[must_use]
    pub fn parse(pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        DESTRUCTIVE_BASH_DENIES
            .contains(&pattern)
            .then(|| Self(pattern.to_owned()))
    }

    /// The pattern as opencode reads it.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.0
    }
}

/// Everything one provider needs in order to spawn a seat at a declared posture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatPosture {
    /// The provider-native `--mode`, when the provider spells one.
    pub mode: Option<&'static str>,
    /// The `permission` block to compose into the seat's worktree, when the
    /// provider reads one. Only opencode does today.
    pub permission: Option<serde_json::Value>,
    /// Whether the harness should accept its own tool calls without asking.
    ///
    /// Opencode's per-agent `auto_accept` feature, stated as intent. **Nothing
    /// consumes it yet**: verified against Paseo 0.6.1, neither `paseo agent
    /// run` nor `paseo agent update` exposes a flag for it, and the Kontor
    /// runtime drives the CLI rather than the MCP surface where it is settable.
    /// The permission block is the mechanism that actually holds; this is here
    /// so the day a spawn-time surface appears, the value it needs is already
    /// derived in the same place as everything else. See OQ-OP20-2.
    pub auto_accept: Option<bool>,
}

impl SeatPosture {
    /// The posture of a seat that may not act on the tree at all.
    ///
    /// Consultation seats are read-only by construction: their mode comes from
    /// [`consultation_permission_mode`](crate::client::consultation_permission_mode),
    /// which offers no writing spelling, and they receive no permission block —
    /// a consultation that could mutate is not a consultation. This is the value
    /// their composition is handed, so "writes nothing" is stated rather than
    /// arrived at by rendering a posture they were never given.
    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            mode: None,
            permission: None,
            auto_accept: None,
        }
    }
}

/// The posture a delivery seat launches under, or a refusal.
///
/// # Why OpenCode is refused here
///
/// OpenCode carries its posture in a written permission block rather than in a
/// mode, and Kontor cannot prove what block the spawned process will actually
/// resolve. The deciding inputs include environment variables read by that
/// process — `OPENCODE_CONFIG_CONTENT` and `OPENCODE_PERMISSION` both inject
/// permissions, and `OPENCODE_DISABLE_PROJECT_CONFIG` makes it ignore the
/// composed file entirely — and the process is created by Paseo, whose
/// `agent run` exposes no way to set or read its environment (verified against
/// Paseo 0.6.1). Reading configuration files from the daemon therefore cannot
/// establish the seat's effective policy, and resolving them in the daemon's own
/// environment answers a different question than the one that matters.
///
/// So an OpenCode delivery launch is refused before any native effect rather
/// than reported as running under a posture nobody verified. This is an interim
/// safe state, not the shape of the feature: it lifts as soon as Paseo exposes
/// an attested resolved configuration, or a seat environment Kontor can verify.
/// The composition, the floor and the readback below are kept and kept tested
/// because they are what that surface switches back on.
///
/// Consultation is unaffected — it runs through
/// [`consultation_route_permission_mode`](crate::client::consultation_route_permission_mode),
/// which already refuses OpenCode except for one operator-accepted recovery
/// route — and so is readback of seats that are already running, which resolves
/// its mode through [`paseo_mode`] rather than through this gate.
///
/// `allowances` flips floor entries inside the permission block — it can only
/// ever change an existing deny's action, never add a key — and **cannot reach
/// `mode` or `auto_accept`**. That is what keeps launch and readback
/// honest: readback compares the mode, a task-scoped exception never moves it,
/// and so an override can never make a seat verify as something it is not.
///
/// # Errors
/// [`RuntimeError::PermissionModeUnsupported`](kontor_runtime::adapter::RuntimeError::PermissionModeUnsupported)
/// when the provider cannot express the declared posture — the same fail-closed
/// refusal `paseo_mode` already makes, unchanged.
pub fn seat_posture(
    provider: &str,
    autonomy: SeatAutonomy,
    allowances: &[PermissionAllowance],
) -> RuntimeResult<SeatPosture> {
    if built_in_provider(provider) == "opencode" {
        // Still closed. The owned per-seat configuration root below is the
        // verified way to make an OpenCode posture deterministic, and it is
        // built and tested — but nothing launches through it yet: the capability
        // gate on `agent run --env` and the installed-binary preflight are not
        // wired. Until both are, a delivery launch is refused rather than run
        // under a posture nothing has checked.
        return Err(RuntimeError::PermissionModeUnsupported {
            provider: provider.to_owned(),
        });
    }
    render_posture(provider, autonomy, allowances)
}

/// Render one declared posture for one provider, without the delivery gate.
///
/// This is the translation itself: it stays complete, and stays tested, because
/// it is what an attested OpenCode surface would switch back on. Nothing on the
/// delivery path calls it directly — [`seat_posture`] does, after deciding
/// whether the provider may be delivered to at all.
pub fn render_posture(
    provider: &str,
    autonomy: SeatAutonomy,
    allowances: &[PermissionAllowance],
) -> RuntimeResult<SeatPosture> {
    let mode = paseo_mode(provider, autonomy)?;
    let harness = built_in_provider(provider);
    let permission = (harness == "opencode")
        .then(|| opencode_permission(autonomy, allowances))
        .flatten();
    // Only opencode reads a written permission block. Cursor and Claude spell
    // every posture they have as a mode, and Codex the same; giving them a block
    // would be a second statement of a rule their mode already makes.
    Ok(SeatPosture {
        mode,
        permission,
        // Reported only where the provider actually exposes the toggle —
        // verified live against Paseo 0.6.1, that is opencode and cursor;
        // claude and codex expose no features at all.
        auto_accept: matches!(harness, "opencode" | "cursor")
            .then(|| autonomy == SeatAutonomy::Bounded),
    })
}

/// Tools that cannot change the tree, the machine or the network.
///
/// Allowed under every posture: an advisory seat that cannot read cannot advise.
const READ_ONLY_TOOLS: &[&str] = &[
    "glob",
    "grep",
    "list",
    "lsp",
    "question",
    "read",
    "skill",
    "todowrite",
];

/// Tools that can act — on files, on other agents, or on the network.
///
/// `bash` and `external_directory` are effectful too and are rendered
/// separately because OpenCode spells them as pattern maps rather than actions.
const EFFECTFUL_TOOLS: &[&str] = &["edit", "patch", "task", "webfetch", "websearch", "write"];

/// The `permission` block one posture renders to, or `None` for a provider that
/// reads none.
///
/// **Every** known tool is named, and a `"*"` catch-all covers the ones this
/// build has never heard of. That completeness is not tidiness: OpenCode
/// *merges* configuration layers rather than replacing them, so any tool this
/// block leaves unnamed keeps whatever a machine-global or repository-committed
/// config said about it. A block that named only `bash` left `edit: allow` from
/// an operator's global config standing, and an `ask` seat edited files without
/// asking. Naming a tool is how this posture actually reaches it.
fn opencode_permission(
    autonomy: SeatAutonomy,
    allowances: &[PermissionAllowance],
) -> Option<serde_json::Value> {
    /// Every guarded bash command takes `default`, except the floor's denials,
    /// except in turn the patterns an operator relaxed for this one task.
    fn bash(default: &str, allowances: &[PermissionAllowance]) -> serde_json::Value {
        let mut patterns = serde_json::Map::new();
        patterns.insert("*".to_owned(), serde_json::Value::from(default));
        for pattern in DESTRUCTIVE_BASH_DENIES {
            patterns.insert((*pattern).to_owned(), serde_json::Value::from("deny"));
        }
        for allowance in allowances {
            patterns.insert(
                allowance.pattern().to_owned(),
                serde_json::Value::from("allow"),
            );
        }
        serde_json::Value::Object(patterns)
    }

    /// One complete block: a catch-all, the read-only set, the effectful set,
    /// and the two pattern-mapped tools.
    fn block(effectful: &str, allowances: &[PermissionAllowance]) -> serde_json::Value {
        let mut permission = serde_json::Map::new();
        // Sorts before every named key, so the named ones are evaluated after it
        // and win for themselves. An unknown tool matches only this.
        permission.insert("*".to_owned(), serde_json::Value::from(effectful));
        for tool in READ_ONLY_TOOLS {
            permission.insert((*tool).to_owned(), serde_json::Value::from("allow"));
        }
        for tool in EFFECTFUL_TOOLS {
            permission.insert((*tool).to_owned(), serde_json::Value::from(effectful));
        }
        permission.insert(
            "external_directory".to_owned(),
            serde_json::json!({ "*": effectful }),
        );
        permission.insert("bash".to_owned(), bash(effectful, allowances));
        serde_json::Value::Object(permission)
    }

    match autonomy {
        // Everything Kontor already authorized, without a second question per
        // tool call — including reading outside the worktree, which is what the
        // wedged seats of 2026-08-22 were actually stopped on.
        SeatAutonomy::Bounded => Some(block("allow", allowances)),
        // The asking posture, stated for every effectful tool rather than left
        // to whatever the host's configuration happens to say.
        SeatAutonomy::Supervised => Some(block("ask", allowances)),
        // `--mode plan` is *behavioral guidance, not containment*: the
        // consultation path records a qualified canary in which shell writes
        // proceeded under it, which is why `consultation_permission_mode`
        // refuses OpenCode outright. A delivery seat declared `plan` must
        // therefore be contained by the block or not claimed at all.
        //
        // Allowances are ignored: a task exception may relax the destructive
        // floor for a seat allowed to act, never for one declared unable to.
        SeatAutonomy::Advisory => Some(block("deny", &[])),
    }
}

/// The Kontor-owned OpenCode configuration root one seat is launched against.
///
/// # Why a whole root, and why one per seat
///
/// OpenCode *merges* configuration rather than replacing it, at every layer and
/// through every environment variable that carries configuration. Measured
/// against the installed 1.18.15: `OPENCODE_PERMISSION` and
/// `OPENCODE_CONFIG_CONTENT` merge over what is already there, so a late-sorting
/// nested rule such as `bash: {"*git*": "allow"}` survives them and — because
/// permissions resolve by last match — beats the destructive floor.
/// `OPENCODE_CONFIG` and `OPENCODE_CONFIG_DIR` merge the ambient global too.
///
/// What does hold is redirecting the configuration root itself, so there is no
/// ambient layer left to merge: a directory Kontor owns, named by
/// `XDG_CONFIG_HOME` *and* by both `OPENCODE_CONFIG*` path variables, with the
/// content also passed inline, and project configuration switched off. Verified
/// end to end against a host whose real global allowed `edit`, `task` and
/// `bash *`, and a worktree carrying three hostile project layers: the resolved
/// permission came back byte-identical to the rendered block.
///
/// **One root per seat.** Two seats in one worktree get two roots, so neither
/// can rewrite the other's posture — which is also why no posture is written
/// into the worktree at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatConfigRoot {
    base: PathBuf,
}

impl SeatConfigRoot {
    /// A root at `base`, which the caller must make unique to one seat.
    #[must_use]
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// The directory `XDG_CONFIG_HOME` and `OPENCODE_CONFIG_DIR` name.
    #[must_use]
    pub fn directory(&self) -> PathBuf {
        self.base.join("opencode")
    }

    /// The file `OPENCODE_CONFIG` names.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.directory().join("opencode.json")
    }

    /// The base the caller owns, for materialization and cleanup.
    #[must_use]
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// The root for one seat, under a Kontor-owned subtree of `state_root`.
    ///
    /// Never the worktree — the seat can write there — never `HOME`, and never a
    /// directory shared with a provider. `seat_key` becomes one path component
    /// and is refused if it could be anything else: an empty string, a separator,
    /// a NUL, or a relative marker would each turn this into a write somewhere
    /// nobody chose.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] when `seat_key` is not a single plain
    /// component.
    pub fn for_seat(state_root: &Path, seat_key: &str) -> RuntimeResult<Self> {
        let plain = !seat_key.is_empty()
            && seat_key != "."
            && seat_key != ".."
            && !seat_key.contains(['/', '\\', '\0'])
            && !seat_key.starts_with('.');
        if !plain {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat configuration root is named by one plain path component",
            });
        }
        Ok(Self {
            base: state_root.join("seats").join("opencode").join(seat_key),
        })
    }

    /// Write the owned configuration, then read it back and prove what landed.
    ///
    /// Narrow permissions, because this file decides what a seat may do: the
    /// directories are `0700` and the file `0600`, so nothing but this daemon's
    /// user can read the posture or edit it between here and the spawn.
    ///
    /// Read back rather than trusted: a short write, a full disk or a racing
    /// writer would otherwise leave a seat launching against a file nobody
    /// checked. The digest returned is over the bytes that are actually on disk.
    ///
    /// # Errors
    /// Any filesystem failure; a component of the path that is a symlink, which
    /// could redirect the write outside the owned subtree; or a readback whose
    /// bytes differ from what was written.
    pub fn materialize(&self, config: &serde_json::Value) -> io::Result<ConfigEvidence> {
        let directory = self.directory();
        std::fs::create_dir_all(&directory)?;
        // After creation, not before: a symlink planted earlier would otherwise
        // be followed by the write below.
        refuse_symlinked_path(&self.base, &directory)?;

        let mut rendered = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
        rendered.push('\n');
        let path = self.config_file();
        std::fs::write(&path, &rendered)?;
        narrow_permissions(&self.base, &directory, &path)?;

        let landed = std::fs::read(&path)?;
        if landed != rendered.as_bytes() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} did not read back as written", path.display()),
            ));
        }
        Ok(ConfigEvidence {
            path,
            digest: ContentHash::of(&landed),
        })
    }
}

/// What was written for one seat, and proof of what landed.
///
/// Path and digest only. The configuration itself is not carried here: this
/// value is evidence, and evidence travels into records and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEvidence {
    /// The owned file the seat is launched against.
    pub path: PathBuf,
    /// The hash of the bytes that are on disk.
    pub digest: ContentHash,
}

/// Refuse a path any component of which is a symbolic link.
fn refuse_symlinked_path(base: &Path, directory: &Path) -> io::Result<()> {
    for candidate in [base, directory] {
        let metadata = std::fs::symlink_metadata(candidate)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} is a symbolic link; a seat's configuration root is a real directory",
                    candidate.display()
                ),
            ));
        }
    }
    Ok(())
}

/// `0700` on the directories, `0600` on the file. A no-op off unix.
fn narrow_permissions(base: &Path, directory: &Path, file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for candidate in [base, directory] {
            std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (base, directory, file);
    }
    Ok(())
}

/// The non-sensitive digest of one seat's posture, owned config and environment.
///
/// A hash of the three things that decide what the seat may do, so a launch
/// whose acknowledgement was lost can only be adopted by a census that finds
/// *this* posture. It carries no value: labels are readable by anyone who can
/// list agents, and the configuration is not theirs to read.
#[must_use]
pub fn posture_digest(
    config: &serde_json::Value,
    environment: &[(&'static str, String)],
) -> ContentHash {
    let mut material = config.to_string();
    for (key, value) in environment {
        material.push('\n');
        material.push_str(key);
        material.push('=');
        material.push_str(value);
    }
    ContentHash::of(material.as_bytes())
}

/// The complete configuration document a seat is launched against.
///
/// Complete because the owned root *replaces* the operator's: anything the seat
/// needs and this document omits, the seat does not get. `mcp` is threaded
/// through for that reason rather than composed separately.
#[must_use]
pub fn owned_config(
    permission: &serde_json::Value,
    mcp: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut document = serde_json::Map::new();
    document.insert(
        "$schema".to_owned(),
        serde_json::Value::from("https://opencode.ai/config.json"),
    );
    document.insert("permission".to_owned(), permission.clone());
    if let Some(mcp) = mcp {
        document.insert("mcp".to_owned(), mcp.clone());
    }
    serde_json::Value::Object(document)
}

/// The variables named by [`SEAT_ENVIRONMENT_KEYS`], and nothing else.
///
/// A **closed internal set**: never operator input, and never a credential.
/// Emitted as repeated `paseo agent run --env key=value`, which sets the
/// environment of the *agent* process — not of the CLI invocation, which is a
/// separate thing already carrying `KONTOR_CALLER_AGENT_ID`.
///
/// `XDG_DATA_HOME` and `XDG_STATE_HOME` are deliberately **absent**: provider
/// authentication lives under them, and a seat that cannot authenticate is not a
/// seat. Only configuration is redirected.
#[must_use]
pub fn seat_environment(
    root: &SeatConfigRoot,
    config: &serde_json::Value,
) -> Vec<(&'static str, String)> {
    // Canonical JSON: `serde_json::Map` is a `BTreeMap` in this workspace, so
    // the same posture renders the same bytes on every host — which is what lets
    // the preflight compare them and a test pin them.
    let canonical = config.to_string();
    let permission = config
        .get("permission")
        .map_or_else(|| "{}".to_owned(), ToString::to_string);
    vec![
        ("OPENCODE_CONFIG", root.config_file().display().to_string()),
        ("OPENCODE_CONFIG_CONTENT", canonical),
        (
            "OPENCODE_CONFIG_DIR",
            root.directory().display().to_string(),
        ),
        ("OPENCODE_DISABLE_PROJECT_CONFIG", "true".to_owned()),
        ("OPENCODE_PERMISSION", permission),
        ("XDG_CONFIG_HOME", root.base().display().to_string()),
    ]
}

/// Every variable [`seat_environment`] may set. Nothing else is ever emitted.
pub const SEAT_ENVIRONMENT_KEYS: &[&str] = &[
    "OPENCODE_CONFIG",
    "OPENCODE_CONFIG_CONTENT",
    "OPENCODE_CONFIG_DIR",
    "OPENCODE_DISABLE_PROJECT_CONFIG",
    "OPENCODE_PERMISSION",
    "XDG_CONFIG_HOME",
];

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_runtime::adapter::RuntimeError;

    fn allowance(pattern: &str) -> PermissionAllowance {
        PermissionAllowance::parse(pattern).expect("a named pattern")
    }

    fn opencode(autonomy: SeatAutonomy, allowances: &[PermissionAllowance]) -> serde_json::Value {
        render_posture("opencode", autonomy, allowances)
            .expect("opencode expresses every posture")
            .permission
            .expect("a block")
    }

    /// An autonomous seat acts on what Kontor already authorized — including
    /// reading outside its worktree, the prompt the wedged seats blocked on —
    /// and still cannot do the destructive things.
    #[test]
    fn autonomous_allows_everything_except_the_floor() {
        let permission = opencode(SeatAutonomy::Bounded, &[]);
        assert_eq!(permission["bash"]["*"], "allow");
        assert_eq!(permission["external_directory"]["*"], "allow");
        assert_eq!(permission["edit"], "allow");
        assert_eq!(permission["webfetch"], "allow");
        for pattern in DESTRUCTIVE_BASH_DENIES {
            assert_eq!(
                permission["bash"][*pattern], "deny",
                "`{pattern}` is refused outright, never escalated to a human"
            );
        }
    }

    /// The asking posture is stated for every effectful tool, not left to the
    /// host's ambient configuration.
    ///
    /// Naming only `bash` was a real hole: OpenCode merges layers, so an
    /// operator's machine-global `edit: allow` survived for every key the block
    /// did not name, and an `ask` seat edited files without ever asking.
    #[test]
    fn ask_states_every_effectful_tool_and_keeps_reading_free() {
        let permission = opencode(SeatAutonomy::Supervised, &[]);
        assert_eq!(permission["bash"]["*"], "ask");
        assert_eq!(permission["*"], "ask", "an unknown tool asks too");
        for effectful in ["edit", "write", "patch", "task", "webfetch", "websearch"] {
            assert_eq!(
                permission[effectful], "ask",
                "`{effectful}` must ask rather than inherit an ambient allow"
            );
        }
        assert_eq!(permission["external_directory"]["*"], "ask");
        for reading in ["read", "glob", "grep", "list", "lsp"] {
            assert_eq!(permission[reading], "allow", "`{reading}` is not effectful");
        }
        for pattern in DESTRUCTIVE_BASH_DENIES {
            assert_eq!(permission["bash"][*pattern], "deny");
        }
    }

    /// `plan` is guidance, not containment — the consultation path records a
    /// canary where shell writes proceeded under it — so an advisory OpenCode
    /// seat is contained by the block or it is not contained at all.
    #[test]
    fn plan_is_contained_by_the_block_not_by_the_mode() {
        let posture = render_posture("opencode", SeatAutonomy::Advisory, &[]).expect("plan");
        assert_eq!(posture.mode, Some("plan"), "the mode is still pinned");
        let permission = posture.permission.expect("plan is contained by a block");

        assert_eq!(
            permission["*"], "deny",
            "deny by default, including tools this build of OpenCode has never heard of"
        );
        assert_eq!(permission["bash"]["*"], "deny", "no shell at all");
        for mutating in ["edit", "write", "patch"] {
            assert_eq!(permission[mutating], "deny", "`{mutating}` may not act");
        }
        for reading in ["read", "glob", "grep", "list", "lsp"] {
            assert_eq!(
                permission[reading], "allow",
                "`{reading}` is how it advises"
            );
        }
    }

    /// A task exception may relax the floor for a seat allowed to act. It may
    /// not relax anything for a seat declared unable to.
    #[test]
    fn an_allowance_cannot_weaken_an_advisory_seat() {
        let plain = render_posture("opencode", SeatAutonomy::Advisory, &[]).expect("plan");
        let pressed = render_posture(
            "opencode",
            SeatAutonomy::Advisory,
            &[allowance("*git rm --cached*"), allowance("*rm -rf *")],
        )
        .expect("plan");
        assert_eq!(
            plain.permission, pressed.permission,
            "an advisory block is identical however many exceptions are declared"
        );
        let permission = pressed.permission.expect("a block");
        assert_eq!(permission["bash"]["*git rm --cached*"], "deny");
        assert_eq!(permission["bash"]["*rm -rf *"], "deny");
    }

    /// The floor's membership, written out.
    ///
    /// Deliberately a literal list rather than a loop over the constant: every
    /// other floor assertion in this module iterates `DESTRUCTIVE_BASH_DENIES`
    /// and so uses the floor as its own oracle — delete a pattern and those
    /// loops simply stop checking it. `CONFIGURATION.md` publishes these five as
    /// a contract, and this is the test that fails when one goes missing or is
    /// silently respelled.
    const REQUIRED_FLOOR: [&str; 5] = [
        "*submodule update*",
        "*submodule deinit*",
        "*git rm --cached*",
        "*git clean -*",
        "*rm -rf *",
    ];

    #[test]
    fn the_floor_is_exactly_the_five_published_patterns() {
        assert_eq!(
            DESTRUCTIVE_BASH_DENIES,
            REQUIRED_FLOOR.as_slice(),
            "the floor's membership is a published contract (CONFIGURATION.md); \
             changing it is a decision, not a refactor"
        );
        assert_eq!(
            DESTRUCTIVE_BASH_DENIES.len(),
            5,
            "the floor has exactly five members"
        );
        for required in REQUIRED_FLOOR {
            assert!(
                DESTRUCTIVE_BASH_DENIES.contains(&required),
                "`{required}` is missing from the floor"
            );
        }
    }

    /// The rendered block denies each pattern *by name*, under both postures
    /// that write one — again by literal, so a deletion from the constant cannot
    /// take the assertion with it.
    #[test]
    fn every_published_pattern_is_denied_by_literal_name() {
        for autonomy in [SeatAutonomy::Bounded, SeatAutonomy::Supervised] {
            let permission = opencode(autonomy, &[]);
            assert_eq!(permission["bash"]["*submodule update*"], "deny");
            assert_eq!(permission["bash"]["*submodule deinit*"], "deny");
            assert_eq!(permission["bash"]["*git rm --cached*"], "deny");
            assert_eq!(permission["bash"]["*git clean -*"], "deny");
            assert_eq!(permission["bash"]["*rm -rf *"], "deny");
        }
    }

    /// An allowance may only name a pattern the floor already denies.
    ///
    /// A broader pattern is the dangerous case and the reason for the rule:
    /// `*git*` sorts *after* both git denies under lexicographic serialization,
    /// and OpenCode's `findLast` would evaluate it last — deleting the git half
    /// of the floor from one config line that never spells `*`.
    #[test]
    fn an_allowance_must_name_an_exact_floor_pattern() {
        for exact in REQUIRED_FLOOR {
            assert_eq!(
                PermissionAllowance::parse(exact)
                    .expect("a floor member")
                    .pattern(),
                exact,
                "an exact floor key is the one thing an exception may name"
            );
        }
        for refused in [
            // broader — the F5 hole: these sort after the denies they overlap
            "*git*",
            "*rm*",
            "*submodule*",
            "*",
            "**",
            // near-misses, prefixes, suffixes and case variants
            "*git rm --cached",
            "git rm --cached*",
            "*git rm --cache*",
            "*git rm --cached**",
            "*GIT RM --CACHED*",
            "*RM -RF *",
            "*rm -rf*",
            "*git clean*",
            // blank and unknown
            "",
            "   ",
            "\t",
            "*curl *",
            "*shutdown*",
        ] {
            assert!(
                PermissionAllowance::parse(refused).is_none(),
                "`{refused}` is not an exact floor pattern and must be refused"
            );
        }
        assert_eq!(
            allowance("  *git rm --cached*  ").pattern(),
            "*git rm --cached*",
            "a declared pattern is read without its surrounding whitespace"
        );
    }

    /// The structural reason the fix works: an exception changes an entry's
    /// action, never the set of entries — so it cannot land anywhere new in the
    /// evaluated order, whatever that order turns out to be.
    #[test]
    fn an_allowance_can_only_flip_a_floor_key_never_add_one() {
        fn bash_keys(permission: &serde_json::Value) -> Vec<String> {
            permission["bash"]
                .as_object()
                .expect("a bash map")
                .keys()
                .cloned()
                .collect()
        }
        let plain = opencode(SeatAutonomy::Bounded, &[]);
        let relaxed = opencode(
            SeatAutonomy::Bounded,
            &[allowance("*git rm --cached*"), allowance("*rm -rf *")],
        );
        assert_eq!(
            bash_keys(&plain),
            bash_keys(&relaxed),
            "an exception may change what a key says, never which keys exist"
        );
        assert_eq!(relaxed["bash"]["*git rm --cached*"], "allow");
        assert_eq!(relaxed["bash"]["*rm -rf *"], "allow");
        assert_eq!(
            relaxed["bash"]["*git clean -*"], "deny",
            "the rest of the floor is untouched"
        );
    }

    /// The floor holds under every posture that writes a block at all.
    #[test]
    fn the_destructive_floor_is_denied_under_every_writing_posture() {
        for autonomy in [SeatAutonomy::Bounded, SeatAutonomy::Supervised] {
            let permission = opencode(autonomy, &[]);
            for pattern in DESTRUCTIVE_BASH_DENIES {
                assert_eq!(
                    permission["bash"][*pattern], "deny",
                    "{autonomy:?} must deny `{pattern}`"
                );
            }
        }
    }

    /// The bounded override relaxes exactly the pattern it names, and the rest
    /// of the floor is untouched — CAT-09 gets `git rm --cached`, not a licence.
    #[test]
    fn an_allowance_relaxes_exactly_one_named_pattern() {
        let permission = opencode(SeatAutonomy::Bounded, &[allowance("*git rm --cached*")]);
        assert_eq!(
            permission["bash"]["*git rm --cached*"], "allow",
            "the named exception is granted"
        );
        for pattern in DESTRUCTIVE_BASH_DENIES {
            if *pattern == "*git rm --cached*" {
                continue;
            }
            assert_eq!(
                permission["bash"][*pattern], "deny",
                "`{pattern}` is untouched by an unrelated exception"
            );
        }
    }

    /// The invariant that keeps launch and readback honest: a task-scoped
    /// exception moves the permission block and nothing else, so the mode a
    /// seat is verified against can never depend on one.
    #[test]
    fn allowances_never_move_the_mode_or_the_feature() {
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            let plain = render_posture("opencode", autonomy, &[]).expect("posture");
            let relaxed = render_posture(
                "opencode",
                autonomy,
                &[allowance("*git rm --cached*"), allowance("*git clean -*")],
            )
            .expect("posture");
            assert_eq!(
                plain.mode, relaxed.mode,
                "an exception cannot move the mode"
            );
            assert_eq!(
                plain.auto_accept, relaxed.auto_accept,
                "an exception cannot move the feature intent"
            );
        }
    }

    /// Every provider's native spelling of every posture it can express,
    /// verified against the live Paseo 0.6.1 provider catalogue.
    #[test]
    fn each_provider_spells_each_posture_natively() {
        let expected = [
            (
                "claude",
                Some("bypassPermissions"),
                Some("auto"),
                Some("plan"),
            ),
            ("codex", Some("full-access"), Some("auto-review"), None),
            // Cursor expresses `autonomous` honestly and nothing else: its ACP
            // runtime permits shell writes in `plan` and shell *and* file writes
            // in `ask`, which is why consultation refuses it outright. A mode
            // label is not a permission boundary.
            ("cursor", Some("agent"), None, None),
            // OpenCode's own row is asserted through `render_posture` in
            // `each_opencode_posture_still_renders`; at the delivery gate it is
            // refused, which `opencode_delivery_is_refused_until_it_can_be_proved`
            // pins.
        ];
        for (provider, bounded, supervised, advisory) in expected {
            assert_eq!(
                seat_posture(provider, SeatAutonomy::Bounded, &[])
                    .expect("bounded")
                    .mode,
                bounded,
                "{provider} autonomous"
            );
            match supervised {
                Some(mode) => assert_eq!(
                    seat_posture(provider, SeatAutonomy::Supervised, &[])
                        .expect("supervised")
                        .mode,
                    Some(mode),
                    "{provider} ask"
                ),
                None => assert!(
                    matches!(
                        seat_posture(provider, SeatAutonomy::Supervised, &[]),
                        Err(RuntimeError::PermissionModeUnsupported { .. })
                    ),
                    "{provider} cannot express `ask` and must be refused, not labelled"
                ),
            }
            match advisory {
                Some(mode) => assert_eq!(
                    seat_posture(provider, SeatAutonomy::Advisory, &[])
                        .expect("advisory")
                        .mode,
                    Some(mode),
                    "{provider} plan"
                ),
                // Codex has no read-only mode and cursor's is not a boundary, so
                // an advisory seat on either is refused rather than quietly run
                // under a mode that does not contain it.
                None => assert!(
                    matches!(
                        seat_posture(provider, SeatAutonomy::Advisory, &[]),
                        Err(RuntimeError::PermissionModeUnsupported { .. })
                    ),
                    "{provider} cannot express `plan` and must be refused"
                ),
            }
        }
    }

    /// Only opencode reads a block; the others carry posture in the mode alone.
    #[test]
    fn only_opencode_gets_a_written_block() {
        for provider in ["claude", "codex", "cursor"] {
            assert!(
                seat_posture(provider, SeatAutonomy::Bounded, &[])
                    .expect("a posture")
                    .permission
                    .is_none(),
                "{provider} states its posture as a mode"
            );
        }
        assert!(
            render_posture("opencode", SeatAutonomy::Bounded, &[])
                .expect("a posture")
                .permission
                .is_some()
        );
    }

    /// `auto_accept` is reported only where the provider exposes the toggle.
    #[test]
    fn the_feature_intent_follows_the_providers_that_have_one() {
        for provider in ["opencode", "cursor"] {
            assert_eq!(
                render_posture(provider, SeatAutonomy::Bounded, &[])
                    .expect("a posture")
                    .auto_accept,
                Some(true),
                "{provider} exposes auto_accept and an autonomous seat wants it"
            );
        }
        for provider in ["claude", "codex"] {
            assert_eq!(
                seat_posture(provider, SeatAutonomy::Bounded, &[])
                    .expect("a posture")
                    .auto_accept,
                None,
                "{provider} exposes no features at all"
            );
        }
    }

    /// An account alias is the same harness, so it renders identically.
    #[test]
    fn an_account_alias_renders_like_its_harness() {
        assert_eq!(
            render_posture("opencode-work", SeatAutonomy::Bounded, &[]).expect("alias"),
            render_posture("opencode", SeatAutonomy::Bounded, &[]).expect("harness"),
        );
        assert_eq!(
            seat_posture("cursor-personal", SeatAutonomy::Bounded, &[])
                .expect("alias")
                .mode,
            Some("agent"),
        );
        assert!(
            matches!(
                seat_posture("cursor-personal", SeatAutonomy::Supervised, &[]),
                Err(RuntimeError::PermissionModeUnsupported { .. })
            ),
            "an alias inherits its harness's refusal too"
        );
    }

    /// A provider nobody has taught this table is refused, not guessed at.
    #[test]
    fn an_unknown_provider_is_refused() {
        assert!(matches!(
            seat_posture("new-provider", SeatAutonomy::Bounded, &[]),
            Err(RuntimeError::PermissionModeUnsupported { .. })
        ));
    }

    // ---- OpenCode evaluator model -------------------------------------------
    //
    // A faithful port of the two functions extracted from the installed
    // OpenCode 1.18.15 binary:
    //
    //   fromConfig: walks Object.entries outer and nested, in the order the
    //               document presents them, emitting {permission, pattern,
    //               action} records.
    //   evaluate:   records.findLast(r => match(tool, r.permission)
    //                                  && match(command, r.pattern))
    //               ?? {action: "ask"}
    //
    // `serde_json::Map` here is a `BTreeMap` (this workspace pins
    // serde_json =1.0.151 and its lock entry pulls in no indexmap), so iterating
    // it yields exactly the lexicographic order the file is serialized in —
    // which is the order OpenCode is handed.
    //
    // This models the semantics that were read out of the binary. It is not the
    // live evaluator, and is not claimed to be.

    fn glob_matches(pattern: &str, value: &str) -> bool {
        let (pattern, value) = (pattern.as_bytes(), value.as_bytes());
        let (mut p, mut v) = (0, 0);
        let (mut star, mut mark) = (None, 0);
        while v < value.len() {
            if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
                p += 1;
                v += 1;
            } else if p < pattern.len() && pattern[p] == b'*' {
                star = Some(p);
                mark = v;
                p += 1;
            } else if let Some(star) = star {
                p = star + 1;
                mark += 1;
                v = mark;
            } else {
                return false;
            }
        }
        while p < pattern.len() && pattern[p] == b'*' {
            p += 1;
        }
        p == pattern.len()
    }

    /// `evaluate(fromConfig(block), tool, command)`.
    fn evaluate(block: &serde_json::Value, tool: &str, command: &str) -> String {
        let mut records: Vec<(String, String, String)> = Vec::new();
        for (key, value) in block.as_object().expect("a permission object") {
            match value {
                serde_json::Value::String(action) => {
                    records.push((key.clone(), "*".to_owned(), action.clone()));
                }
                serde_json::Value::Object(nested) => {
                    for (pattern, action) in nested {
                        records.push((
                            key.clone(),
                            pattern.clone(),
                            action.as_str().expect("an action").to_owned(),
                        ));
                    }
                }
                _ => panic!("unexpected permission shape"),
            }
        }
        records
            .iter()
            .rfind(|(permission, pattern, _)| {
                glob_matches(permission, tool) && glob_matches(pattern, command)
            })
            .map_or_else(|| "ask".to_owned(), |(_, _, action)| action.clone())
    }

    #[test]
    fn the_evaluator_model_agrees_with_the_extracted_semantics() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("*rm -rf *", "sudo rm -rf /tmp"));
        assert!(!glob_matches("*rm -rf *", "rm -r /tmp"));
        assert!(glob_matches("*git*", "git clean -fdx"));
        // An unmatched tool falls to the evaluator's own default.
        assert_eq!(
            evaluate(&serde_json::json!({ "read": "allow" }), "bash", "ls"),
            "ask"
        );
    }

    /// An advisory OpenCode seat provably cannot act, under those semantics.
    #[test]
    fn an_advisory_seat_can_neither_run_a_shell_nor_edit() {
        let block = opencode(SeatAutonomy::Advisory, &[]);
        for command in ["ls", "cat file", "rm -rf /", "git status", "echo hi > f"] {
            assert_eq!(
                evaluate(&block, "bash", command),
                "deny",
                "an advisory seat must not run `{command}`"
            );
        }
        for tool in ["edit", "write", "patch"] {
            assert_eq!(evaluate(&block, tool, "*"), "deny", "`{tool}` must not act");
        }
        for tool in ["read", "glob", "grep", "list", "lsp"] {
            assert_eq!(evaluate(&block, tool, "*"), "allow", "`{tool}` is reading");
        }
        assert_eq!(
            evaluate(&block, "some_future_tool", "*"),
            "deny",
            "a tool this build has never heard of is denied, not asked"
        );
    }

    /// The floor actually wins for a seat that *is* allowed to act.
    #[test]
    fn the_floor_wins_over_the_catch_all_under_evaluation() {
        let bounded = opencode(SeatAutonomy::Bounded, &[]);
        assert_eq!(evaluate(&bounded, "bash", "ls -la"), "allow");
        for destructive in [
            "rm -rf /tmp/x",
            "git clean -fdx",
            "git rm --cached thing",
            "git submodule update --init",
            "git submodule deinit -f x",
        ] {
            assert_eq!(
                evaluate(&bounded, "bash", destructive),
                "deny",
                "`{destructive}` must lose to the floor"
            );
        }

        let ask = opencode(SeatAutonomy::Supervised, &[]);
        assert_eq!(evaluate(&ask, "bash", "ls -la"), "ask");
        assert_eq!(evaluate(&ask, "bash", "rm -rf /tmp/x"), "deny");
    }

    /// An exact-key exception flips exactly one command family and no other.
    #[test]
    fn an_exact_exception_flips_only_its_own_family_under_evaluation() {
        let block = opencode(SeatAutonomy::Bounded, &[allowance("*git rm --cached*")]);
        assert_eq!(evaluate(&block, "bash", "git rm --cached gitlink"), "allow");
        assert_eq!(evaluate(&block, "bash", "git clean -fdx"), "deny");
        assert_eq!(evaluate(&block, "bash", "rm -rf /tmp/x"), "deny");
    }

    /// Why `PermissionAllowance` is restricted to exact floor keys: a broader
    /// pattern *would* defeat the floor under these semantics. The type cannot
    /// express this block — this constructs it by hand to show the hazard is
    /// real rather than theoretical.
    #[test]
    fn a_broad_allowance_would_erase_the_floor_which_is_why_it_cannot_be_declared() {
        let mut block = opencode(SeatAutonomy::Bounded, &[]);
        block["bash"]["*git*"] = serde_json::Value::from("allow");
        assert_eq!(
            evaluate(&block, "bash", "git clean -fdx"),
            "allow",
            "a broader pattern sorts after the deny it overlaps and wins"
        );
        assert!(
            PermissionAllowance::parse("*git*").is_none(),
            "which is exactly why the type refuses to build it"
        );
    }

    /// An OpenCode delivery seat is refused until its posture can be proved.
    ///
    /// The deciding inputs — `OPENCODE_CONFIG_CONTENT`, `OPENCODE_PERMISSION`,
    /// `OPENCODE_DISABLE_PROJECT_CONFIG` — are read by the spawned process,
    /// which Paseo creates and whose environment `agent run` neither sets nor
    /// reports. Until that surface exists, claiming a verified posture would be
    /// claiming something nothing checks.
    #[test]
    fn opencode_delivery_is_refused_until_it_can_be_proved() {
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            for provider in ["opencode", "opencode-work"] {
                assert!(
                    matches!(
                        seat_posture(provider, autonomy, &[]),
                        Err(RuntimeError::PermissionModeUnsupported { .. })
                    ),
                    "{provider} {autonomy:?} must be refused at the delivery gate"
                );
            }
        }
    }

    /// The gate is OpenCode's alone: every other provider still resolves.
    #[test]
    fn the_delivery_gate_does_not_touch_other_providers() {
        assert_eq!(
            seat_posture("claude", SeatAutonomy::Bounded, &[])
                .expect("claude is unaffected")
                .mode,
            Some("bypassPermissions")
        );
        assert_eq!(
            seat_posture("codex", SeatAutonomy::Supervised, &[])
                .expect("codex is unaffected")
                .mode,
            Some("auto-review")
        );
        assert_eq!(
            seat_posture("cursor", SeatAutonomy::Bounded, &[])
                .expect("cursor autonomous is unaffected")
                .mode,
            Some("agent")
        );
    }

    /// The translation itself stays complete and stays tested: it is what an
    /// attested Paseo surface switches back on, and it must not rot meanwhile.
    #[test]
    fn each_opencode_posture_still_renders() {
        for (autonomy, mode) in [
            (SeatAutonomy::Bounded, "build"),
            (SeatAutonomy::Supervised, "build"),
            (SeatAutonomy::Advisory, "plan"),
        ] {
            let posture = render_posture("opencode", autonomy, &[]).expect("the renderer");
            assert_eq!(posture.mode, Some(mode));
            assert!(
                posture.permission.is_some(),
                "{autonomy:?} still renders a block"
            );
        }
    }

    fn owned_root() -> SeatConfigRoot {
        SeatConfigRoot::new("/realm/state/seats/agent-1")
    }

    /// The closed set is exactly six variables, named, deduplicated, and
    /// pointing only where Kontor owns.
    #[test]
    fn the_seat_environment_is_a_closed_set_of_six() {
        let root = owned_root();
        let config = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);
        let environment = seat_environment(&root, &config);

        let names: Vec<&str> = environment.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            names, SEAT_ENVIRONMENT_KEYS,
            "exactly the declared set, in a stable order"
        );
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "no key is emitted twice");
    }

    /// Provider authentication lives under the data and state homes. Redirect
    /// those and the seat cannot log in, so they are never named.
    #[test]
    fn the_seat_environment_never_touches_the_data_or_state_home() {
        let config = owned_config(&opencode(SeatAutonomy::Advisory, &[]), None);
        for (key, value) in seat_environment(&owned_root(), &config) {
            assert!(
                !matches!(key, "XDG_DATA_HOME" | "XDG_STATE_HOME" | "HOME"),
                "`{key}` must not be redirected: provider auth lives there"
            );
            assert!(
                !value.contains("hunter2") && !value.to_lowercase().contains("secret"),
                "no value carries anything credential-shaped"
            );
        }
    }

    /// Every path points inside the seat's own root, and the disable flag is the
    /// exact spelling the installed binary treats as true.
    #[test]
    fn the_seat_environment_points_only_at_the_owned_root() {
        let root = owned_root();
        let config = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);
        let environment = seat_environment(&root, &config);
        let value = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
                .expect("the key is present")
        };
        assert_eq!(value("XDG_CONFIG_HOME"), "/realm/state/seats/agent-1");
        assert_eq!(
            value("OPENCODE_CONFIG_DIR"),
            "/realm/state/seats/agent-1/opencode"
        );
        assert_eq!(
            value("OPENCODE_CONFIG"),
            "/realm/state/seats/agent-1/opencode/opencode.json"
        );
        assert_eq!(value("OPENCODE_DISABLE_PROJECT_CONFIG"), "true");
    }

    /// The two carriers state the same posture, canonically, and parse back to
    /// exactly what the renderer produced.
    #[test]
    fn the_carried_config_and_permission_are_canonical_and_agree() {
        let root = owned_root();
        let rendered = opencode(SeatAutonomy::Supervised, &[]);
        let config = owned_config(&rendered, None);
        let environment = seat_environment(&root, &config);
        let value = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
                .expect("present")
        };

        let carried: serde_json::Value = value("OPENCODE_CONFIG_CONTENT")
            .parse()
            .expect("canonical JSON");
        assert_eq!(carried["permission"], rendered);
        let permission: serde_json::Value = value("OPENCODE_PERMISSION")
            .parse()
            .expect("canonical JSON");
        assert_eq!(permission, rendered, "both carriers state one posture");

        // Deterministic bytes: the same posture renders identically every time,
        // which is what the preflight compares against.
        let again = seat_environment(&root, &owned_config(&rendered, None));
        assert_eq!(environment, again);
    }

    /// The owned config replaces the operator's, so it must carry everything the
    /// seat needs — its MCP surface included.
    #[test]
    fn the_owned_config_carries_the_seats_mcp_surface() {
        let mcp = serde_json::json!({ "kontor": { "type": "local" } });
        let config = owned_config(&opencode(SeatAutonomy::Bounded, &[]), Some(&mcp));
        assert_eq!(config["mcp"]["kontor"]["type"], "local");
        assert_eq!(config["$schema"], "https://opencode.ai/config.json");
        assert!(config["permission"]["bash"]["*rm -rf *"] == "deny");
    }

    /// Two seats in one worktree get two roots, so neither can rewrite the
    /// other's posture — the shared-file race cannot arise.
    #[test]
    fn two_seats_get_two_roots() {
        let first = SeatConfigRoot::new("/realm/state/seats/agent-1");
        let second = SeatConfigRoot::new("/realm/state/seats/agent-2");
        assert_ne!(first.config_file(), second.config_file());
        assert_ne!(first.directory(), second.directory());

        let autonomous = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);
        let advisory = owned_config(&opencode(SeatAutonomy::Advisory, &[]), None);
        let one = seat_environment(&first, &autonomous);
        let other = seat_environment(&second, &advisory);
        assert_ne!(one, other);
        for (key, value) in &one {
            if key.starts_with("XDG") || key.ends_with("DIR") || *key == "OPENCODE_CONFIG" {
                assert!(
                    value.contains("agent-1") && !value.contains("agent-2"),
                    "each seat names only its own root"
                );
            }
        }
    }

    /// A seat root is one plain component under a Kontor-owned subtree, and
    /// anything that could escape it is refused.
    #[test]
    fn a_seat_root_cannot_be_named_out_of_its_subtree() {
        let state = Path::new("/realm/state");
        let root = SeatConfigRoot::for_seat(state, "01a0306e-cbce").expect("a plain key");
        assert_eq!(
            root.base(),
            Path::new("/realm/state/seats/opencode/01a0306e-cbce")
        );
        assert!(root.base().starts_with(state), "inside the owned subtree");

        for refused in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            ".hidden",
            "with\0nul",
        ] {
            assert!(
                SeatConfigRoot::for_seat(state, refused).is_err(),
                "`{refused}` must not name a seat root"
            );
        }
    }

    /// The owned file is written, read back, hashed, and narrowly permissioned.
    #[test]
    fn materialization_reads_back_and_narrows_permissions() {
        let scratch = tempfile::TempDir::new().expect("a scratch directory");
        let root = SeatConfigRoot::for_seat(scratch.path(), "agent-1").expect("a root");
        let config = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);

        let evidence = root.materialize(&config).expect("materialized");
        assert_eq!(evidence.path, root.config_file());

        let landed: serde_json::Value = std::fs::read_to_string(&evidence.path)
            .expect("written")
            .parse()
            .expect("JSON");
        assert_eq!(landed, config, "what is on disk is what was rendered");
        assert_eq!(
            evidence.digest,
            ContentHash::of(&std::fs::read(&evidence.path).expect("read")),
            "the digest is over the bytes on disk"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| {
                std::fs::metadata(path)
                    .expect("exists")
                    .permissions()
                    .mode()
                    & 0o777
            };
            assert_eq!(
                mode(&evidence.path),
                0o600,
                "only this user reads a posture"
            );
            assert_eq!(mode(&root.directory()), 0o700);
            assert_eq!(mode(root.base()), 0o700);
        }

        // Idempotent: a relaunch rewrites the same bytes and the same digest.
        let again = root.materialize(&config).expect("materialized again");
        assert_eq!(again, evidence);
    }

    /// A symlinked component could redirect the write out of the owned subtree.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_component_is_refused() {
        let scratch = tempfile::TempDir::new().expect("a scratch directory");
        let elsewhere = scratch.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("target");
        let root = SeatConfigRoot::for_seat(scratch.path(), "agent-1").expect("a root");
        std::fs::create_dir_all(root.base()).expect("base");
        std::os::unix::fs::symlink(&elsewhere, root.directory()).expect("planted");

        let config = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);
        let error = root
            .materialize(&config)
            .expect_err("a symlinked configuration directory is refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            !elsewhere.join("opencode.json").exists(),
            "and nothing was written through it"
        );
    }

    /// The digest covers the posture *and* the environment that carries it, and
    /// carries no value of either.
    #[test]
    fn the_posture_digest_changes_with_posture_and_environment() {
        let root = SeatConfigRoot::new("/realm/state/seats/agent-1");
        let bounded = owned_config(&opencode(SeatAutonomy::Bounded, &[]), None);
        let advisory = owned_config(&opencode(SeatAutonomy::Advisory, &[]), None);

        let one = posture_digest(&bounded, &seat_environment(&root, &bounded));
        let two = posture_digest(&advisory, &seat_environment(&root, &advisory));
        assert_ne!(one, two, "a different posture is a different digest");

        let other_root = SeatConfigRoot::new("/realm/state/seats/agent-2");
        let three = posture_digest(&bounded, &seat_environment(&other_root, &bounded));
        assert_ne!(one, three, "a different root is a different digest");

        assert_eq!(
            one,
            posture_digest(&bounded, &seat_environment(&root, &bounded)),
            "and it is stable for the same inputs"
        );
        let rendered = one.to_string();
        assert!(
            !rendered.contains("opencode.json") && !rendered.contains("deny"),
            "a digest carries no value from what it covers: {rendered}"
        );
    }
}
