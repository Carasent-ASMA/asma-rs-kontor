//! Worktree-local harness composition for a seat's own working directory.
//!
//! # Why a seat's MCP config is written into its worktree
//!
//! A seat that inherits the machine's ambient harness configuration inherits
//! whatever authority the operator's own console carries — measured live, that
//! was kontor at *admin* tier plus two unrelated servers, ~18k tokens of tool
//! schemas per turn. Composing the config into the seat's own working directory
//! gives every Claude seat exactly one kontor server at **operator** tier under
//! a narrow serve profile: `consultation` for independent reviewers and
//! `leadership` for persistent LSA/TPM seats. The inherited seat credential,
//! not the profile, remains the authority and supplies the SeatBinding.
//!
//! Three files are touched, all local to the worktree and all kept out of the
//! seat's own diff via `git rev-parse --git-path info/exclude` — a seat whose
//! infrastructure dirties its worktree would ship its own scaffolding:
//!
//! 1. `<cwd>/.mcp.json` — the kontor server entry, merged in: other servers a
//!    project legitimately declares are preserved, only the `kontor` key is
//!    overwritten.
//! 2. `<cwd>/.claude/settings.local.json` — `enableAllProjectMcpServers: true`,
//!    merged in, so the seat does not stall on a project-MCP trust prompt it
//!    has no human to answer.
//! 3. the worktree's `info/exclude` — `.mcp.json` and `.claude/` appended once.
//!
//! # Why an opencode seat is composed too
//!
//! An opencode seat's posture is not in its mode — [`crate::posture`] renders
//! it as a `permission` block, and this is where that block is written so the
//! seat reads it at process start rather than inheriting whatever the machine
//! carries.
//!
//! It is written to `<cwd>/.opencode/opencode.json`, **not** `<cwd>/opencode.json`,
//! and the difference is load-bearing. The root file is one a repository
//! legitimately commits — the superproject these seats run in tracks one
//! carrying its model and instructions — and git applies no ignore rule to a
//! *tracked* file, so merging into it would dirty every seat's own diff and
//! leave Kontor's floor one `git add` from being committed as project config.
//! Opencode reads both and the `.opencode/` copy takes precedence, which is
//! exactly the ordering this needs: the operator's committed configuration
//! survives untouched, and the seat's posture still wins. Nothing outside the
//! worktree is touched — in particular never `~/.config/opencode/opencode.json`,
//! whose machine-local edit is the stopgap this composition exists to replace.
//!
//! # The kill switch and the provider boundary
//!
//! `KONTOR_SEAT_MCP=off` in the daemon's environment disables composition for
//! the whole process — the daemon resolves it once at fleet composition and
//! hands the adapter `None`. That withdraws the MCP files **and nothing else**:
//! a declared permission posture is a safety boundary rather than part of this
//! surface, so it is composed either way. Providers other than Claude and
//! opencode are a no-op for MCP: only the
//! Claude harness reads `.mcp.json` from its cwd (codex/opencode are follow-up).
//! Account-qualified Claude provider ids such as `claude-work` and
//! `claude-personal` are the same harness boundary and are composed too.

use crate::posture::{DESTRUCTIVE_BASH_DENIES, PermissionAllowance, SeatPosture};
use std::io;
use std::path::{Path, PathBuf};

/// The environment variable that disables seat MCP composition daemon-wide.
pub const KILL_SWITCH_ENV: &str = "KONTOR_SEAT_MCP";

/// Whether composition is enabled given the kill-switch value.
///
/// Only the exact spelling `off` disables; absence and anything else leave
/// composition on, so a typo fails toward the configured behavior.
#[must_use]
pub fn enabled(kill_switch: Option<&str>) -> bool {
    kill_switch != Some("off")
}

/// The `kontor-mcp` command a composed seat runs.
///
/// Resolution, documented because it is a choice: the realm's binaries install
/// together, so a `kontor-mcp` sitting next to this daemon's own executable is
/// the same build the realm uses and is named absolutely. When there is no such
/// sibling — a dev run out of `cargo`, say — the bare name is written and the
/// seat resolves it on its own `PATH`, exactly as the shipped seat templates do.
#[must_use]
pub fn kontor_mcp_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("kontor-mcp")))
        .filter(|sibling| sibling.is_file())
        .map_or_else(
            || "kontor-mcp".to_owned(),
            |sibling| sibling.to_string_lossy().into_owned(),
        )
}

/// Everything seat MCP composition needs to know, resolved once by the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatMcp {
    /// The `kontor-mcp` command the seat's `.mcp.json` names.
    pub command: String,
    /// The realm state root that command acts on.
    pub state_root: PathBuf,
}

impl SeatMcp {
    /// Compose the three worktree-local files for one seat cwd.
    ///
    /// Idempotent: repeating it rewrites the same content and appends nothing
    /// twice, so a relaunch or a recovered seat costs nothing.
    ///
    /// # Errors
    /// Any filesystem failure, an existing config file that is not JSON (it is
    /// refused rather than clobbered), or a cwd that is not a git worktree.
    pub fn compose(&self, cwd: &Path, serve_profile: &str) -> io::Result<()> {
        write_mcp_json(cwd, &self.command, &self.state_root, serve_profile)?;
        write_claude_settings(cwd)?;
        exclude_from_git(cwd, CLAUDE_EXCLUDED)
    }
}

/// Compose for one seat: a no-op unless composition is configured **and** the
/// provider is one this composes for.
///
/// Claude gets its MCP files; opencode gets the permission block its declared
/// posture rendered to. The two are disjoint because neither harness reads the
/// other's files, and every other provider writes nothing.
///
/// `posture` arrives already rendered rather than being derived here, so the
/// block a seat reads and the `--mode` it was launched under are two uses of one
/// evaluation. Deriving it twice is how they would come to disagree.
///
/// # Errors
/// As [`SeatMcp::compose`].
pub fn compose_for_seat(
    seat: Option<&SeatMcp>,
    provider: &str,
    posture: &SeatPosture,
    cwd: &Path,
) -> io::Result<()> {
    let harness = crate::client::built_in_provider(provider);
    // The posture is composed for an OpenCode seat regardless of `seat`. It is
    // this seat's permission floor, not part of the MCP surface: gating it on
    // `Option<SeatMcp>` meant `KONTOR_SEAT_MCP=off` — a kill switch for a
    // convenience feature — silently launched autonomous seats with no floor at
    // all, while `permission_posture` still parsed, the seat still spawned
    // `--mode build`, and readback still passed on the mode alone.
    //
    // It is equally deliberate that **no other provider touches this file**. A
    // Claude or Codex seat has no business rewriting OpenCode configuration, and
    // Kontor holds no marker proving a `permission` block found there is one it
    // wrote rather than one a human did. An OpenCode seat overwrites the block
    // because deciding that seat's posture is precisely this code's job; every
    // other seat leaves the file exactly as it found it.
    if harness == "opencode" {
        compose_permission_block(posture, cwd)?;
    }
    match seat {
        Some(seat) if harness == "claude" => seat.compose(cwd, "consultation"),
        _ => Ok(()),
    }
}

/// Merge-write `<cwd>/.opencode/opencode.json` with this seat's posture.
///
/// Idempotent like [`SeatMcp::compose`], and merging for the same reason: only
/// the `permission` key is ours, so a `model` or an `mcp` entry somebody put in
/// the seat's own `.opencode/` config outlives the composition.
fn compose_permission_block(posture: &SeatPosture, cwd: &Path) -> io::Result<()> {
    let Some(permission) = posture.permission.clone() else {
        // Nothing to state, so nothing is touched. A stale block cannot outlive
        // its usefulness here: every OpenCode posture renders a block, so the
        // next OpenCode seat in this worktree overwrites it wholesale before it
        // starts. Deleting somebody's `permission` key on the strength of "no
        // OpenCode seat is launching right now" would be destroying state this
        // code cannot prove it owns.
        return Ok(());
    };
    let directory = cwd.join(".opencode");
    std::fs::create_dir_all(&directory)?;
    merge_json(&directory.join("opencode.json"), |document| {
        document.insert("permission".to_owned(), permission);
        Ok(())
    })?;
    exclude_from_git(cwd, OPENCODE_EXCLUDED)
}

/// Compose the identity-bound leadership surface for a hosted LSA/TPM seat.
/// It shares the same credential transport and provider boundary as
/// consultation composition but presents only completion read/remediate tools.
pub fn compose_for_hosted_seat(
    seat: Option<&SeatMcp>,
    provider: &str,
    cwd: &Path,
) -> io::Result<()> {
    match seat {
        Some(seat) if crate::client::built_in_provider(provider) == "claude" => {
            seat.compose(cwd, "leadership")
        }
        _ => Ok(()),
    }
}

/// Merge-write `<cwd>/.mcp.json`: overwrite only the `kontor` server entry.
fn write_mcp_json(
    cwd: &Path,
    command: &str,
    state_root: &Path,
    serve_profile: &str,
) -> io::Result<()> {
    let kontor = serde_json::json!({
        "command": command,
        "args": [
            "--state-root", state_root.to_string_lossy(),
            "--credential-tier", "operator",
            "--serve-profile", serve_profile,
        ],
    });
    merge_json(&cwd.join(".mcp.json"), |document| {
        let servers = document
            .entry("mcpServers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        match servers.as_object_mut() {
            Some(servers) => {
                servers.insert("kontor".to_owned(), kontor);
                Ok(())
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "existing .mcp.json has a non-object `mcpServers`; refusing to clobber it",
            )),
        }
    })
}

/// Merge-write `<cwd>/.claude/settings.local.json`, preserving unknown keys.
fn write_claude_settings(cwd: &Path) -> io::Result<()> {
    let directory = cwd.join(".claude");
    std::fs::create_dir_all(&directory)?;
    merge_json(&directory.join("settings.local.json"), |document| {
        document.insert(
            "enableAllProjectMcpServers".to_owned(),
            serde_json::Value::Bool(true),
        );
        Ok(())
    })
}

/// Read a JSON object file (absent = empty object), mutate it, write it back.
///
/// An existing file that is not a JSON object is an error, never overwritten:
/// clobbering a config a human wrote is worse than a refused launch.
fn merge_json(
    path: &Path,
    mutate: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> io::Result<()>,
) -> io::Result<()> {
    let mut document = match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(serde_json::Value::Object(document)) => document,
            Ok(_) | Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} exists and is not a JSON object; refusing to clobber it",
                        path.display()
                    ),
                ));
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(error) => return Err(error),
    };
    mutate(&mut document)?;
    let mut rendered = serde_json::to_string_pretty(&serde_json::Value::Object(document))
        .map_err(io::Error::other)?;
    rendered.push('\n');
    std::fs::write(path, rendered)
}

/// Every configuration layer OpenCode merges for one seat, lowest precedence
/// first within `global`, then the repository's root config, then the block
/// Kontor composed.
///
/// Named explicitly rather than resolved inline so a test can present an exact
/// three-layer shape without depending on the machine it runs on — and so the
/// set of layers this verification believes in is reviewable in one place.
#[derive(Debug, Clone)]
pub struct ConfigLayers {
    /// Machine and provider-home configuration. **Read only, never written.**
    pub global: Vec<PathBuf>,
    /// The repository's own committed `opencode.json` at the seat's cwd.
    pub root: PathBuf,
    /// The block this composition wrote.
    pub seat_local: PathBuf,
}

impl ConfigLayers {
    /// The layers a seat spawned in `cwd` will actually read.
    ///
    /// `OPENCODE_CONFIG` names a single file and is merged under everything
    /// else; `OPENCODE_CONFIG_DIR`, `XDG_CONFIG_HOME` and `HOME` each resolve a
    /// configuration directory, whose `opencode.json` and `opencode.jsonc` are
    /// both read — the installed 1.18.15 accepts either spelling.
    #[must_use]
    pub fn for_seat(cwd: &Path) -> Self {
        let mut global = Vec::new();
        if let Some(file) = std::env::var_os("OPENCODE_CONFIG") {
            global.push(PathBuf::from(file));
        }
        let directory = std::env::var_os("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME").map(|xdg| PathBuf::from(xdg).join("opencode"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".config").join("opencode"))
            });
        if let Some(directory) = directory {
            global.push(directory.join("opencode.json"));
            global.push(directory.join("opencode.jsonc"));
        }
        Self {
            global,
            root: cwd.join("opencode.json"),
            seat_local: cwd.join(".opencode/opencode.json"),
        }
    }
}

/// Read the seat's **effective** permission back and refuse the launch unless it
/// states exactly the posture that was rendered.
///
/// # Why every layer is read
///
/// OpenCode merges configuration rather than replacing it, and resolves a call
/// by last match. Comparing only the file Kontor wrote therefore proves nothing:
/// an operator's machine-global config carrying `edit: allow`, `task: allow` or
/// `external_directory: {"*": "allow"}` merges *underneath* the composed block
/// and survives for every key the block does not name, and a rule committed in
/// the repository's own root `opencode.json` survives the same way. Both were
/// reproduced against the installed 1.18.15. So the whole stack is merged here
/// in the same order OpenCode merges it, and the result must equal the rendered
/// posture exactly — any surviving rule that the posture did not state, widening
/// or not, refuses the launch.
///
/// # What it does and does not prove
///
/// It proves that the configuration OpenCode will resolve at process start says
/// what Kontor decided it should say. It does **not** observe provider-internal
/// evaluator state after start, which no surface exposes; and it cannot prevent
/// a file changing between this check and the process reading it, which is why
/// the launch path runs it again after placement.
///
/// # Errors
/// A missing, unreadable, unparseable or non-object layer; an effective
/// `permission` differing in any way from the rendered posture; or a floor
/// pattern that is not denied and was not exactly relaxed by a declared
/// allowance. Each refuses before the seat is spawned.
pub fn verify_composed_posture(
    posture: &SeatPosture,
    allowances: &[PermissionAllowance],
    cwd: &Path,
) -> io::Result<()> {
    verify_composed_posture_in(&ConfigLayers::for_seat(cwd), posture, allowances)
}

/// [`verify_composed_posture`] against an explicit set of layers.
///
/// # Errors
/// As [`verify_composed_posture`].
pub fn verify_composed_posture_in(
    layers: &ConfigLayers,
    posture: &SeatPosture,
    allowances: &[PermissionAllowance],
) -> io::Result<()> {
    let Some(rendered) = posture.permission.as_ref() else {
        return Ok(());
    };
    let effective = effective_permission(layers)?;
    if effective != *rendered {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{}: the effective permission OpenCode would resolve does not match the \
                 posture this seat was rendered for; refusing to spawn",
                layers.seat_local.display()
            ),
        ));
    }
    // Checked again against the floor itself rather than against the value just
    // compared to: an equality test is only as good as what it was handed, and
    // this is the one property that must hold however the block was produced.
    let relaxed: Vec<&str> = allowances
        .iter()
        .map(PermissionAllowance::pattern)
        .collect();
    for pattern in DESTRUCTIVE_BASH_DENIES {
        let expected = if relaxed.contains(pattern) {
            "allow"
        } else {
            "deny"
        };
        if effective["bash"][*pattern] != *expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the destructive floor pattern `{pattern}` is not `{expected}`"),
            ));
        }
    }
    Ok(())
}

/// The `permission` OpenCode resolves from these layers, merged in its order.
fn effective_permission(layers: &ConfigLayers) -> io::Result<serde_json::Value> {
    fn permission_of(path: &Path) -> io::Result<Option<serde_json::Value>> {
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(serde_json::Value::Object(document)) => Ok(document.get("permission").cloned()),
                Ok(_) | Err(_) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} is not a JSON object; the effective permission cannot be proved",
                        path.display()
                    ),
                )),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// OpenCode deep-merges nested objects, the later document winning per key.
    fn merge(under: &serde_json::Value, over: &serde_json::Value) -> serde_json::Value {
        match (under, over) {
            (serde_json::Value::Object(under), serde_json::Value::Object(over)) => {
                let mut merged = under.clone();
                for (key, value) in over {
                    let combined = merged
                        .get(key)
                        .map_or_else(|| value.clone(), |existing| merge(existing, value));
                    merged.insert(key.clone(), combined);
                }
                serde_json::Value::Object(merged)
            }
            _ => over.clone(),
        }
    }

    if permission_of(&layers.seat_local)?.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "{} states no permission block; refusing to spawn",
                layers.seat_local.display()
            ),
        ));
    }
    let ordered = layers
        .global
        .iter()
        .cloned()
        .chain([layers.root.clone(), layers.seat_local.clone()]);
    let mut effective = serde_json::Value::Null;
    for path in ordered {
        if let Some(permission) = permission_of(&path)? {
            effective = if effective.is_null() {
                permission
            } else {
                merge(&effective, &permission)
            };
        }
    }
    Ok(effective)
}

/// The lines a composed Claude seat must never surface as in `git status`.
const CLAUDE_EXCLUDED: &[&str] = &[".mcp.json", ".claude/"];

/// The same for an opencode seat.
///
/// The exact file, never the `.opencode/` directory: a repository may
/// legitimately track commands, skills or agents under it — this one does — and
/// excluding the whole directory would be a claim over somebody else's files.
const OPENCODE_EXCLUDED: &[&str] = &[".opencode/opencode.json"];

/// Append the composed paths to the worktree's `info/exclude`, once each.
///
/// `git rev-parse --git-path info/exclude` rather than a hand-rolled `.git`
/// walk: for a linked worktree the exclude file lives under the *common* git
/// dir, and re-deriving git's own path rules here would be a second copy that
/// drifts. This is a local plumbing query, not a runtime effect, so it does not
/// travel through the Paseo transport seam.
fn exclude_from_git(cwd: &Path, lines: &[&str]) -> io::Result<()> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{} is not a git worktree; seat MCP files would dirty it",
            cwd.display()
        )));
    }
    let printed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let path = PathBuf::from(&printed);
    // `--git-path` answers relative to the cwd it was asked from when it can.
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = match std::fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let missing: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|line| !existing.lines().any(|present| present.trim() == *line))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for line in missing {
        content.push_str(line);
        content.push('\n');
    }
    std::fs::write(&path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::spec::SeatAutonomy;

    /// A fresh git repository to compose into.
    fn repo() -> tempfile::TempDir {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(directory.path())
            .arg("init")
            .arg("--quiet")
            .status()
            .expect("git runs");
        assert!(status.success(), "git init succeeds");
        directory
    }

    fn seat(state_root: &str) -> SeatMcp {
        SeatMcp {
            command: "kontor-mcp".to_owned(),
            state_root: PathBuf::from(state_root),
        }
    }

    /// The layers a test presents: no global unless one is named explicitly, so
    /// a suite never depends on the machine it happens to run on.
    fn layers(cwd: &Path) -> ConfigLayers {
        ConfigLayers {
            global: Vec::new(),
            root: cwd.join("opencode.json"),
            seat_local: cwd.join(".opencode/opencode.json"),
        }
    }

    /// The `permission` block currently composed into a worktree.
    fn read_permission(cwd: &Path) -> serde_json::Value {
        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("a composed config")
                .parse()
                .expect("JSON");
        document["permission"].clone()
    }

    fn porcelain(cwd: &Path) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["status", "--porcelain"])
            .output()
            .expect("git runs");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// TEST-006: the three files are written, carry the right JSON, and none of
    /// them surfaces in `git status`.
    #[test]
    fn composition_writes_the_files_and_none_of_them_dirty_the_worktree() {
        let repo = repo();
        let cwd = repo.path();
        compose_for_seat(
            Some(&seat("/realm/state")),
            "claude",
            &SeatPosture::read_only(),
            cwd,
        )
        .expect("composition");

        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(cwd.join(".mcp.json")).expect("written"))
                .expect(".mcp.json is JSON");
        assert_eq!(mcp["mcpServers"]["kontor"]["command"], "kontor-mcp");
        assert_eq!(
            mcp["mcpServers"]["kontor"]["args"],
            serde_json::json!([
                "--state-root",
                "/realm/state",
                "--credential-tier",
                "operator",
                "--serve-profile",
                "consultation"
            ]),
            "the seat gets operator tier under the consultation profile, nothing wider"
        );

        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(cwd.join(".claude/settings.local.json")).expect("written"),
        )
        .expect("settings are JSON");
        assert_eq!(settings["enableAllProjectMcpServers"], true);

        let status = porcelain(cwd);
        assert!(
            !status.contains(".mcp.json") && !status.contains(".claude"),
            "the composed files must not dirty the seat's worktree:\n{status}"
        );
    }

    /// Persistent LSA/TPM seats receive the completion-only MCP surface while
    /// using the same inherited scoped credential channel.
    #[test]
    fn hosted_leadership_composition_selects_the_completion_profile() {
        let repo = repo();
        compose_for_hosted_seat(Some(&seat("/realm/state")), "claude-personal", repo.path())
            .expect("leadership composition");

        let mcp: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo.path().join(".mcp.json")).expect("written"),
        )
        .expect(".mcp.json is JSON");
        assert_eq!(
            mcp["mcpServers"]["kontor"]["args"],
            serde_json::json!([
                "--state-root",
                "/realm/state",
                "--credential-tier",
                "operator",
                "--serve-profile",
                "leadership"
            ])
        );
        assert!(porcelain(repo.path()).is_empty());
    }

    /// TEST-007: the kill switch and the provider boundary each write nothing.
    #[test]
    fn a_disabled_or_foreign_seat_writes_nothing() {
        assert!(!enabled(Some("off")), "`off` is the kill switch");
        assert!(enabled(None), "absence leaves composition on");
        assert!(enabled(Some("on")), "anything but `off` leaves it on");

        let repo = repo();
        // The daemon resolves the kill switch to `None` before the adapter sees it.
        compose_for_seat(None, "claude", &SeatPosture::read_only(), repo.path()).expect("a no-op");
        compose_for_seat(
            Some(&seat("/realm/state")),
            "codex",
            &SeatPosture::read_only(),
            repo.path(),
        )
        .expect("a no-op");
        assert!(
            !repo.path().join(".mcp.json").exists() && !repo.path().join(".claude").exists(),
            "nothing was written"
        );
    }

    /// A provider id selects an account, while MCP composition belongs to the
    /// harness. A Claude account alias must therefore receive the same local
    /// MCP boundary as the built-in `claude` id.
    #[test]
    fn a_claude_account_alias_composes_the_same_seat_mcp_boundary() {
        let repo = repo();
        compose_for_seat(
            Some(&seat("/realm/state")),
            "claude-personal",
            &SeatPosture::read_only(),
            repo.path(),
        )
        .expect("a Claude account alias is still the Claude harness");

        let mcp: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo.path().join(".mcp.json")).expect("written"),
        )
        .expect(".mcp.json is JSON");
        assert_eq!(
            mcp["mcpServers"]["kontor"]["args"],
            serde_json::json!([
                "--state-root",
                "/realm/state",
                "--credential-tier",
                "operator",
                "--serve-profile",
                "consultation"
            ])
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(repo.path().join(".claude/settings.local.json"))
                    .expect("written"),
            )
            .expect("settings are JSON")["enableAllProjectMcpServers"],
            true
        );
    }

    /// Merging preserves what a project already declared; only `kontor` is ours.
    #[test]
    fn merging_preserves_foreign_servers_and_unknown_settings() {
        let repo = repo();
        let cwd = repo.path();
        std::fs::write(
            cwd.join(".mcp.json"),
            r#"{"mcpServers": {"figma": {"command": "figma-mcp"}, "kontor": {"command": "stale"}}}"#,
        )
        .expect("seeded");
        std::fs::create_dir_all(cwd.join(".claude")).expect("directory");
        std::fs::write(
            cwd.join(".claude/settings.local.json"),
            r#"{"permissions": {"allow": ["Bash"]}}"#,
        )
        .expect("seeded");

        compose_for_seat(
            Some(&seat("/realm/state")),
            "claude",
            &SeatPosture::read_only(),
            cwd,
        )
        .expect("composition");

        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(cwd.join(".mcp.json")).expect("read"))
                .expect("JSON");
        assert_eq!(
            mcp["mcpServers"]["figma"]["command"], "figma-mcp",
            "a foreign server survives"
        );
        assert_eq!(
            mcp["mcpServers"]["kontor"]["command"], "kontor-mcp",
            "the kontor entry is overwritten, not merged field-by-field"
        );

        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(cwd.join(".claude/settings.local.json")).expect("read"),
        )
        .expect("JSON");
        assert_eq!(
            settings["permissions"]["allow"][0], "Bash",
            "unknown settings keys survive"
        );
        assert_eq!(settings["enableAllProjectMcpServers"], true);
    }

    /// A repeated spawn appends no duplicate exclude lines.
    #[test]
    fn composing_twice_appends_each_exclude_line_once() {
        let repo = repo();
        let cwd = repo.path();
        let seat = seat("/realm/state");
        seat.compose(cwd, "consultation")
            .expect("first composition");
        seat.compose(cwd, "consultation")
            .expect("second composition");

        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--git-path", "info/exclude"])
            .output()
            .expect("git runs");
        let printed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let path = PathBuf::from(&printed);
        let path = if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        };
        let exclude = std::fs::read_to_string(path).expect("the exclude file exists");
        for line in CLAUDE_EXCLUDED {
            assert_eq!(
                exclude.lines().filter(|present| present == line).count(),
                1,
                "`{line}` appears exactly once:\n{exclude}"
            );
        }
    }

    /// A config file that is not JSON is refused, never clobbered.
    #[test]
    fn an_unparseable_existing_config_is_refused_rather_than_clobbered() {
        let repo = repo();
        let cwd = repo.path();
        std::fs::write(cwd.join(".mcp.json"), "not json at all").expect("seeded");
        let error = seat("/realm/state")
            .compose(cwd, "consultation")
            .expect_err("a human's broken file is not ours to overwrite");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            std::fs::read_to_string(cwd.join(".mcp.json")).expect("still there"),
            "not json at all",
            "the file is untouched"
        );
    }

    /// A cwd that is not a git worktree is refused: the files would dirty it.
    #[test]
    fn a_non_git_cwd_is_refused() {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        seat("/realm/state")
            .compose(directory.path(), "consultation")
            .expect_err("no worktree, no composition");
    }

    /// Render a real posture the way the adapter does, then compose it.
    fn compose_opencode_seat(cwd: &Path, autonomy: SeatAutonomy) -> serde_json::Value {
        let posture = crate::posture::seat_posture("opencode", autonomy, &[]).expect("a posture");
        compose_for_seat(Some(&seat("/realm/state")), "opencode", &posture, cwd)
            .expect("composition");
        let raw = std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
            .expect("the seat's own opencode config is written");
        let document: serde_json::Value = raw.parse().expect("it is JSON");
        document["permission"].clone()
    }

    /// Where git keeps this worktree's `info/exclude`.
    fn exclude_path(cwd: &Path) -> PathBuf {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "--git-path", "info/exclude"])
            .output()
            .expect("git runs");
        let printed = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let path = PathBuf::from(&printed);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    }

    /// An autonomous opencode seat starts with its posture already on disk, and
    /// the file it arrives in does not show up as the seat's own change.
    #[test]
    fn an_autonomous_opencode_seat_is_composed_without_dirtying_its_worktree() {
        let repo = repo();
        let cwd = repo.path();
        let permission = compose_opencode_seat(cwd, SeatAutonomy::Bounded);

        assert_eq!(permission["bash"]["*"], "allow");
        assert_eq!(permission["external_directory"]["*"], "allow");
        for pattern in crate::posture::DESTRUCTIVE_BASH_DENIES {
            assert_eq!(permission["bash"][*pattern], "deny");
        }
        let status = porcelain(cwd);
        assert!(
            !status.contains("opencode"),
            "the composed block must not dirty the seat's worktree:\n{status}"
        );
    }

    /// A consultation seat states no block, so none is written.
    #[test]
    fn a_read_only_seat_has_nothing_composed_for_it() {
        let repo = repo();
        compose_for_seat(
            Some(&seat("/realm/state")),
            "opencode",
            &SeatPosture::read_only(),
            repo.path(),
        )
        .expect("a no-op");
        assert!(!repo.path().join(".opencode/opencode.json").exists());
    }

    /// The repository's own committed `opencode.json` is not Kontor's to edit.
    ///
    /// This is why the block lands in `.opencode/`: git applies no ignore rule
    /// to a tracked file, so merging into the root config would dirty the seat's
    /// diff and leave the floor one `git add` from becoming project config.
    #[test]
    fn a_committed_root_config_is_left_exactly_as_it_was() {
        let repo = repo();
        let cwd = repo.path();
        let committed = "{\"model\": \"deepseek/deepseek-v4-flash\"}";
        std::fs::write(cwd.join("opencode.json"), committed).expect("seeded");

        let permission = compose_opencode_seat(cwd, SeatAutonomy::Bounded);
        assert_eq!(
            permission["bash"]["*"], "allow",
            "the posture still applies"
        );
        assert_eq!(
            std::fs::read_to_string(cwd.join("opencode.json")).expect("still there"),
            committed,
            "the operator's committed configuration is untouched"
        );
    }

    /// Only `permission` is ours; the rest of a seat's own config survives.
    #[test]
    fn composing_preserves_unknown_keys_in_the_seat_config() {
        let repo = repo();
        let cwd = repo.path();
        std::fs::create_dir_all(cwd.join(".opencode")).expect("directory");
        std::fs::write(
            cwd.join(".opencode/opencode.json"),
            r#"{"model": "kept", "permission": {"bash": {"*": "stale"}}}"#,
        )
        .expect("seeded");

        let permission = compose_opencode_seat(cwd, SeatAutonomy::Bounded);
        assert_eq!(
            permission["bash"]["*"], "allow",
            "a stale posture is replaced wholesale, not merged pattern by pattern"
        );
        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("read")
                .parse()
                .expect("JSON");
        assert_eq!(document["model"], "kept", "an unknown key survives");
    }

    /// A relaunched opencode seat appends its exclude line once, like Claude's.
    #[test]
    fn composing_an_opencode_seat_twice_appends_its_exclude_line_once() {
        let repo = repo();
        let cwd = repo.path();
        for _ in 0..2 {
            compose_opencode_seat(cwd, SeatAutonomy::Bounded);
        }
        let exclude = std::fs::read_to_string(exclude_path(cwd)).expect("the exclude file exists");
        for line in OPENCODE_EXCLUDED {
            assert_eq!(
                exclude.lines().filter(|present| present == line).count(),
                1,
                "`{line}` appears exactly once:\n{exclude}"
            );
        }
    }

    /// The kill switch withdraws MCP scaffolding and **never** the floor.
    ///
    /// `KONTOR_SEAT_MCP=off` resolves to `None` in the daemon, and that used to
    /// take the permission block with it — so a switch for a convenience feature
    /// silently launched autonomous seats with no floor, while the posture still
    /// parsed and readback still passed on the mode alone. A kill switch for MCP
    /// may not withdraw a declared permission boundary.
    #[test]
    fn the_kill_switch_withdraws_mcp_scaffolding_but_never_the_floor() {
        let repo = repo();
        let cwd = repo.path();
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");

        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");

        assert!(
            !cwd.join(".mcp.json").exists(),
            "MCP scaffolding is withdrawn"
        );
        assert!(!cwd.join(".claude").exists(), "so is the Claude trust file");

        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("the floor is composed regardless")
                .parse()
                .expect("JSON");
        assert_eq!(document["permission"]["bash"]["*"], "allow");
        for pattern in [
            "*submodule update*",
            "*submodule deinit*",
            "*git rm --cached*",
            "*git clean -*",
            "*rm -rf *",
        ] {
            assert_eq!(
                document["permission"]["bash"][pattern], "deny",
                "`{pattern}` is denied even with no seat MCP configured"
            );
        }
        // And it reads back as rendered, before anything could be spawned.
        verify_composed_posture_in(&layers(cwd), &posture, &[]).expect("readback");
    }

    /// The same for the asking posture: no seat MCP, floor still present.
    #[test]
    fn an_ask_floor_is_composed_with_no_seat_mcp_configured() {
        let repo = repo();
        let cwd = repo.path();
        let posture = crate::posture::seat_posture("opencode", SeatAutonomy::Supervised, &[])
            .expect("posture");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");

        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("composed")
                .parse()
                .expect("JSON");
        assert_eq!(document["permission"]["bash"]["*"], "ask");
        for pattern in [
            "*submodule update*",
            "*submodule deinit*",
            "*git rm --cached*",
            "*git clean -*",
            "*rm -rf *",
        ] {
            assert_eq!(document["permission"]["bash"][pattern], "deny");
        }
        verify_composed_posture_in(&layers(cwd), &posture, &[]).expect("readback");
    }

    /// A worktree reused for `plan` must not keep the last seat's authority.
    #[test]
    fn a_worktree_reused_for_plan_does_not_keep_the_previous_allow_block() {
        let repo = repo();
        let cwd = repo.path();
        let autonomous =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");
        compose_for_seat(None, "opencode", &autonomous, cwd).expect("first seat");
        assert_eq!(
            read_permission(cwd)["bash"]["*"],
            "allow",
            "the first seat may act"
        );

        let advisory =
            crate::posture::seat_posture("opencode", SeatAutonomy::Advisory, &[]).expect("posture");
        compose_for_seat(None, "opencode", &advisory, cwd).expect("second seat");

        let permission = read_permission(cwd);
        assert_eq!(
            permission["bash"]["*"], "deny",
            "the reused worktree must not hand the plan seat the last seat's shell"
        );
        assert_eq!(permission["*"], "deny");
        assert_eq!(permission["edit"], "deny");
        verify_composed_posture_in(&layers(cwd), &advisory, &[]).expect("readback");
    }

    /// A `permission` block Kontor cannot prove it wrote is never touched by a
    /// seat that has no business with OpenCode configuration.
    #[test]
    fn a_user_owned_permission_block_survives_a_non_opencode_launch() {
        let repo = repo();
        let cwd = repo.path();
        std::fs::create_dir_all(cwd.join(".opencode")).expect("directory");
        let owned = r#"{"permission": {"bash": {"*": "ask"}}, "model": "mine"}"#;
        std::fs::write(cwd.join(".opencode/opencode.json"), owned).expect("seeded");

        for provider in ["claude", "codex", "cursor"] {
            let posture = crate::posture::seat_posture(provider, SeatAutonomy::Bounded, &[])
                .expect("a posture");
            compose_for_seat(Some(&seat("/realm/state")), provider, &posture, cwd)
                .expect("composition");
            assert_eq!(
                std::fs::read_to_string(cwd.join(".opencode/opencode.json")).expect("still there"),
                owned,
                "a {provider} seat must not rewrite OpenCode configuration it does not own"
            );
        }

        // An OpenCode seat *does* own the posture decision, and replaces it.
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");
        assert_eq!(read_permission(cwd)["bash"]["*"], "allow");
        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("read")
                .parse()
                .expect("JSON");
        assert_eq!(
            document["model"], "mine",
            "and still only the permission key is ours"
        );
    }

    /// The readback refuses everything that would let a seat start unprotected.
    #[test]
    fn the_readback_refuses_a_config_that_is_not_what_was_rendered() {
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");

        {
            // missing
            let missing = repo();
            assert_eq!(
                verify_composed_posture_in(&layers(missing.path()), &posture, &[])
                    .expect_err("a missing config is refused")
                    .kind(),
                io::ErrorKind::NotFound
            );
        }
        {
            // unparseable
            let broken = repo();
            std::fs::create_dir_all(broken.path().join(".opencode")).expect("directory");
            std::fs::write(broken.path().join(".opencode/opencode.json"), "not json")
                .expect("seeded");
            assert_eq!(
                verify_composed_posture_in(&layers(broken.path()), &posture, &[])
                    .expect_err("an unparseable config is refused")
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
        {
            // tampered: the floor quietly relaxed after composition
            let tampered = repo();
            let cwd = tampered.path();
            compose_for_seat(None, "opencode", &posture, cwd).expect("composition");
            let mut document: serde_json::Value =
                std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                    .expect("read")
                    .parse()
                    .expect("JSON");
            document["permission"]["bash"]["*rm -rf *"] = serde_json::Value::from("allow");
            std::fs::write(
                cwd.join(".opencode/opencode.json"),
                serde_json::to_string_pretty(&document).expect("render"),
            )
            .expect("tampered");
            assert!(
                verify_composed_posture_in(&layers(cwd), &posture, &[]).is_err(),
                "a floor pattern flipped after composition must refuse the launch"
            );
        }
    }

    /// A repository's own root config cannot widen the effective permission:
    /// OpenCode merges it underneath ours, so a root-only key would survive.
    #[test]
    fn the_readback_refuses_a_root_config_that_widens_the_effective_permission() {
        let repo = repo();
        let cwd = repo.path();
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");
        verify_composed_posture_in(&layers(cwd), &posture, &[]).expect("clean worktree reads back");

        std::fs::write(
            cwd.join("opencode.json"),
            r#"{"permission": {"bash": {"*git*": "allow"}}}"#,
        )
        .expect("seeded");
        assert!(
            verify_composed_posture_in(&layers(cwd), &posture, &[]).is_err(),
            "a root-committed rule that survives the merge must refuse the launch"
        );
    }

    /// An exactly-declared relaxation reads back as the posture that was built
    /// with it, and only for the pattern it names.
    #[test]
    fn the_readback_accepts_an_exactly_declared_relaxation() {
        let repo = repo();
        let cwd = repo.path();
        let allowance =
            crate::posture::PermissionAllowance::parse("*git rm --cached*").expect("floor member");
        let allowances = std::slice::from_ref(&allowance);
        let posture = crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, allowances)
            .expect("posture");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");

        verify_composed_posture_in(&layers(cwd), &posture, allowances)
            .expect("the declared relaxation");
        assert!(
            verify_composed_posture_in(&layers(cwd), &posture, &[]).is_err(),
            "the same file is refused when no relaxation was declared for it"
        );
    }

    /// The bounded exception reaches the file the seat actually reads.
    #[test]
    fn a_task_scoped_exception_reaches_the_composed_block() {
        let repo = repo();
        let cwd = repo.path();
        let allowance =
            crate::posture::PermissionAllowance::parse("*git rm --cached*").expect("named");
        let posture = crate::posture::seat_posture(
            "opencode",
            SeatAutonomy::Bounded,
            std::slice::from_ref(&allowance),
        )
        .expect("posture");
        compose_for_seat(Some(&seat("/realm/state")), "opencode", &posture, cwd)
            .expect("composition");

        let document: serde_json::Value =
            std::fs::read_to_string(cwd.join(".opencode/opencode.json"))
                .expect("read")
                .parse()
                .expect("JSON");
        assert_eq!(document["permission"]["bash"]["*git rm --cached*"], "allow");
        assert_eq!(
            document["permission"]["bash"]["*rm -rf *"], "deny",
            "the rest of the floor is untouched by one ticket's exception"
        );
    }

    /// The permission block an operator host actually carries, verbatim from
    /// `~/.config/opencode/opencode.json` on the machine this was built on.
    const LIVE_GLOBAL: &str = r#"{
      "permission": {
        "read": "allow", "edit": "allow", "glob": "allow", "grep": "allow",
        "list": "allow", "lsp": "allow", "skill": "allow", "task": "allow",
        "todowrite": "allow", "question": "allow", "webfetch": "allow",
        "websearch": "allow",
        "external_directory": { "*": "allow" },
        "bash": {
          "*": "allow",
          "*submodule update*": "deny", "*submodule deinit*": "deny",
          "*git rm --cached*": "deny", "*git clean -*": "deny", "*rm -rf *": "deny"
        }
      }
    }"#;

    /// A permissive machine-global config cannot widen any posture.
    ///
    /// This is the three-layer merge OpenCode actually performs. The global here
    /// says `edit: allow`, `task: allow`, `webfetch: allow` and
    /// `external_directory: {"*": "allow"}` — every one of which used to survive
    /// into the effective configuration, because the composed block did not name
    /// those keys. An `ask` seat therefore edited without asking and a `plan`
    /// seat was not contained. The block now names them, so the merge resolves
    /// to exactly the rendered posture, and the readback proves it did.
    #[test]
    fn a_permissive_operator_global_cannot_widen_any_posture() {
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            let repo = repo();
            let cwd = repo.path();
            let global = cwd.join("operator-global.json");
            std::fs::write(&global, LIVE_GLOBAL).expect("the operator's config");

            let posture = crate::posture::seat_posture("opencode", autonomy, &[]).expect("posture");
            compose_for_seat(None, "opencode", &posture, cwd).expect("composition");

            let layered = ConfigLayers {
                global: vec![global],
                root: cwd.join("opencode.json"),
                seat_local: cwd.join(".opencode/opencode.json"),
            };
            verify_composed_posture_in(&layered, &posture, &[]).unwrap_or_else(|error| {
                panic!("{autonomy:?} must survive a permissive global unchanged: {error}")
            });
        }
    }

    /// A rule that survives the merge from *any* layer refuses the launch.
    #[test]
    fn a_widening_rule_in_any_layer_refuses_the_launch() {
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Advisory, &[]).expect("posture");

        // (1) the machine-global names a tool the posture does not
        let global_case = repo();
        let cwd = global_case.path();
        let global = cwd.join("operator-global.json");
        std::fs::write(
            &global,
            r#"{"permission": {"browser": "allow", "read": "allow"}}"#,
        )
        .expect("seeded");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");
        assert!(
            verify_composed_posture_in(
                &ConfigLayers {
                    global: vec![global],
                    root: cwd.join("opencode.json"),
                    seat_local: cwd.join(".opencode/opencode.json"),
                },
                &posture,
                &[]
            )
            .is_err(),
            "a global rule the posture never stated must refuse the launch"
        );

        // (2) the repository commits a bash rule that outsorts the floor
        let root_case = repo();
        let cwd = root_case.path();
        std::fs::write(
            cwd.join("opencode.json"),
            r#"{"permission": {"bash": {"*git*": "allow"}}}"#,
        )
        .expect("seeded");
        compose_for_seat(None, "opencode", &posture, cwd).expect("composition");
        assert!(
            verify_composed_posture_in(&layers(cwd), &posture, &[]).is_err(),
            "a repository-committed rule that survives the merge must refuse the launch"
        );
    }
}
