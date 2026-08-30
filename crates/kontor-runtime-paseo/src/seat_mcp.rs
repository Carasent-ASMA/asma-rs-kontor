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
//! hands the adapter `None`, which withdraws the permission block along with
//! the MCP files. Providers other than Claude and opencode are a no-op: only the
//! Claude harness reads `.mcp.json` from its cwd (codex/opencode are follow-up).
//! Account-qualified Claude provider ids such as `claude-work` and
//! `claude-personal` are the same harness boundary and are composed too.

use crate::posture::SeatPosture;
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
    match seat {
        Some(seat) if crate::client::built_in_provider(provider) == "claude" => {
            seat.compose(cwd, "consultation")
        }
        Some(_) if crate::client::built_in_provider(provider) == "opencode" => {
            compose_opencode(posture, cwd)
        }
        _ => Ok(()),
    }
}

/// Merge-write `<cwd>/.opencode/opencode.json` with this seat's posture.
///
/// Idempotent like [`SeatMcp::compose`], and merging for the same reason: only
/// the `permission` key is ours, so a `model` or an `mcp` entry somebody put in
/// the seat's own `.opencode/` config outlives the composition.
fn compose_opencode(posture: &SeatPosture, cwd: &Path) -> io::Result<()> {
    let Some(permission) = posture.permission.clone() else {
        // A posture that renders no block — `plan` — is already read-only by
        // mode. Writing an allow-list for a seat that may not act would be a
        // second, weaker statement of the same rule.
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
        if path.is_absolute() { path } else { cwd.join(path) }
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

    /// A posture that renders no block writes no file.
    #[test]
    fn a_read_only_seat_has_nothing_composed_for_it() {
        let repo = repo();
        compose_for_seat(
            Some(&seat("/realm/state")),
            "opencode",
            &crate::posture::seat_posture("opencode", SeatAutonomy::Advisory, &[])
                .expect("plan renders no block"),
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
        assert_eq!(permission["bash"]["*"], "allow", "the posture still applies");
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

    /// The kill switch withdraws the permission block, not just the MCP files.
    #[test]
    fn the_kill_switch_withdraws_the_permission_block_too() {
        let repo = repo();
        let posture =
            crate::posture::seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("posture");
        compose_for_seat(None, "opencode", &posture, repo.path()).expect("a no-op");
        assert!(
            !repo.path().join(".opencode/opencode.json").exists(),
            "`KONTOR_SEAT_MCP=off` disables every kind of composition"
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
}
