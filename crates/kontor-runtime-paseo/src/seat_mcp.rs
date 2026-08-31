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

impl SeatMcp {
    /// The `config.mcpServers` entry a delivery seat is created with.
    ///
    /// Built from this value rather than taken as JSON from a caller: the
    /// create payload decides what a seat can reach, and "some object the
    /// launch path happened to assemble" is not a thing to validate after the
    /// fact. The shape mirrors what [`SeatMcp::compose`] writes into
    /// `.mcp.json` for a Claude seat, so both surfaces name one server at one
    /// tier under one profile.
    #[must_use]
    pub fn server_config(&self, serve_profile: &str) -> serde_json::Value {
        serde_json::json!({
            "kontor": {
                "type": "local",
                "command": [
                    self.command.clone(),
                    "--state-root".to_owned(),
                    self.state_root.display().to_string(),
                    "--credential-tier".to_owned(),
                    "operator".to_owned(),
                    "--serve-profile".to_owned(),
                    serve_profile.to_owned(),
                ],
            }
        })
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
    // **No OpenCode configuration is written here, by any provider.**
    //
    // An OpenCode seat reaches this function and leaves with nothing written.
    // Its posture and its MCP surface both travel in the create's `config`, so
    // there is nothing here to carry — and a file could not carry the posture
    // anyway: the layers that decide it merge after anything Kontor writes and
    // depend on who the seat authenticated as. Writing one would only put two
    // seats sharing a worktree in each other's way, and change operator state
    // for nothing.
    //
    // Nor does any other provider touch it: a Claude or Codex seat has no
    // business rewriting OpenCode configuration, and Kontor holds no marker
    // proving a `permission` block found there is one it wrote rather than one a
    // human did.
    let _ = posture;
    match seat {
        Some(seat) if harness == "claude" => seat.compose(cwd, "consultation"),
        _ => Ok(()),
    }
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

/// Every configuration file OpenCode merges for one seat.
///
/// Named explicitly rather than resolved inline so a test can present an exact
/// layer stack without depending on the machine it runs on, and so the set of
/// travel through the Paseo transport seam.
/// The lines a composed Claude seat must never surface as in `git status`.
const CLAUDE_EXCLUDED: &[&str] = &[".mcp.json", ".claude/"];

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

    /// An OpenCode seat leaves the worktree it shares exactly as it found it.
    ///
    /// Not "no `opencode.json`" — *nothing*: the whole tree is listed before and
    /// after, at every posture, with a seat MCP configured and without one. This
    /// is what lets two OpenCode seats share one worktree, so it is asserted
    /// against the directory rather than against a path the test picked.
    #[test]
    fn an_opencode_seat_leaves_the_shared_worktree_untouched() {
        fn listing(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
            let mut found = Vec::new();
            let mut pending = vec![root.to_path_buf()];
            while let Some(directory) = pending.pop() {
                for entry in std::fs::read_dir(&directory).expect("readable") {
                    let path = entry.expect("an entry").path();
                    if path.is_dir() {
                        pending.push(path);
                    } else {
                        found.push((path.clone(), std::fs::read(&path).expect("readable")));
                    }
                }
            }
            found.sort();
            found
        }

        let repo = repo();
        let cwd = repo.path();
        std::fs::write(
            cwd.join("opencode.json"),
            r#"{"permission": {"bash": "ask"}}"#,
        )
        .expect("an operator file the seat must not touch");
        let before = listing(cwd);
        assert!(!before.is_empty(), "the oracle would pass on an empty tree");

        for posture in [
            SeatPosture::read_only(),
            crate::posture::seat_posture("opencode", kontor_core::spec::SeatAutonomy::Bounded, &[])
                .expect("bounded"),
            crate::posture::seat_posture(
                "opencode",
                kontor_core::spec::SeatAutonomy::Supervised,
                &[],
            )
            .expect("supervised"),
            crate::posture::seat_posture(
                "opencode",
                kontor_core::spec::SeatAutonomy::Advisory,
                &[],
            )
            .expect("advisory"),
        ] {
            for seat_mcp in [None, Some(seat("/realm/state"))] {
                compose_for_seat(seat_mcp.as_ref(), "opencode", &posture, cwd)
                    .expect("an OpenCode seat composes nothing");
                compose_for_seat(seat_mcp.as_ref(), "opencode-work", &posture, cwd)
                    .expect("an account alias is still the OpenCode harness");
            }
        }

        assert_eq!(
            listing(cwd),
            before,
            "an OpenCode seat wrote into the worktree it shares"
        );
    }

    /// A provider id selects an account, while MCP composition belongs to the
    /// harness.""" A Claude account alias must therefore receive the same local
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
}
