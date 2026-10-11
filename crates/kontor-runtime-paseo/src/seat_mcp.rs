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
//! surface, so it is composed either way. Only Claude reads `.mcp.json` from
//! its cwd. Hosted leadership also receives the scoped MCP and exact tool
//! policy in its creation frame for every provider, including Codex and
//! OpenCode; their MCP surface must not depend on Claude's local files.
//! Account-qualified Claude provider ids such as `claude-work` and
//! `claude-personal` are the same harness boundary and are composed too.
//!
//! A proved Desk-scoped local directory uses a separate publication path:
//! settings then MCP, exact originals in a private daemon-owned journal, and
//! recovery under a canonical-cwd lock before any create/reload. It does not
//! make delivery or consultation callers eligible for non-Git composition.

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

/// The guard arguments a planning pair member's hook runs with.
const MEMBER_GUARD_ARGS: [&str; 3] = [
    "--consultation-tool-guard",
    "--serve-profile",
    kontor_core::planning_pair::MEMBER_SERVE_PROFILE,
];

/// Everything seat MCP composition needs to know, resolved once by the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatMcp {
    /// The `kontor-mcp` command the seat's `.mcp.json` names.
    pub command: String,
    /// The realm state root that command acts on.
    pub state_root: PathBuf,
}

impl SeatMcp {
    /// Refuse a mixed-version installation before composing or launching a
    /// session whose containment depends on the companion MCP binary.
    pub fn verify_consultation_guard(&self) -> io::Result<()> {
        let output = std::process::Command::new(&self.command)
            .arg("--consultation-tool-guard")
            .stdin(std::process::Stdio::null())
            .output()?;
        let reply = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
        if !output.status.success()
            || reply.as_ref().is_none_or(|reply| {
                reply["hookSpecificOutput"]["hookEventName"] != "PreToolUse"
                    || reply["hookSpecificOutput"]["permissionDecision"] != "deny"
            })
        {
            return Err(io::Error::other(
                "the MCP binary does not implement the consultation guard",
            ));
        }
        Ok(())
    }

    /// Refuse a guard binary that does not enforce the planning pair member
    /// profile (ASMA-8282 D-3).
    ///
    /// The probe attests the profile rather than a deny-everything guard: under
    /// `--serve-profile planning_pair_member` every member tool must be allowed
    /// and a consultation tool, a caller tool and a shell must be denied. An
    /// older binary, which does not accept the profile argument, and one that
    /// falls back to or unions with the consultation surface both fail it.
    pub fn verify_planning_pair_member_guard(&self) -> io::Result<()> {
        let decide = |tool: &str| -> io::Result<String> {
            use std::io::Write as _;
            let mut child = std::process::Command::new(&self.command)
                .args(MEMBER_GUARD_ARGS)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("the guard probe has no stdin"))?
                .write_all(
                    serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool})
                        .to_string()
                        .as_bytes(),
                )?;
            let output = child.wait_with_output()?;
            let reply = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
            match reply {
                Some(reply)
                    if output.status.success()
                        && reply["hookSpecificOutput"]["hookEventName"] == "PreToolUse" =>
                {
                    Ok(reply["hookSpecificOutput"]["permissionDecision"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned())
                }
                _ => Err(io::Error::other(
                    "the MCP binary does not implement the consultation guard",
                )),
            }
        };
        let members = kontor_core::planning_pair::MEMBER_MCP_TOOLS
            .iter()
            .map(|tool| (format!("mcp__kontor__{tool}"), "allow"));
        let foreign = [
            "mcp__kontor__kontor_committee_findings_record",
            "mcp__kontor__kontor_planning_pair_disposition_record",
            "Bash",
        ]
        .into_iter()
        .map(|tool| (tool.to_owned(), "deny"));
        for (tool, expected) in members.chain(foreign) {
            if decide(&tool)? != expected {
                return Err(io::Error::other(
                    "the MCP binary does not enforce the planning pair member guard",
                ));
            }
        }
        Ok(())
    }

    /// Compose the consultation-only tool guard before starting Claude.
    pub fn compose_consultation(&self, cwd: &Path) -> io::Result<()> {
        self.compose(cwd, "consultation")?;
        self.compose_guard_hook(cwd, "--consultation-tool-guard")
    }

    /// Compose a planning pair member's surface before starting Claude: the
    /// member serve profile in `.mcp.json` and the member guard, which permits
    /// only that profile's three tools (ASMA-8282 D-3).
    pub fn compose_planning_pair_member(&self, cwd: &Path) -> io::Result<()> {
        self.compose(cwd, kontor_core::planning_pair::MEMBER_SERVE_PROFILE)?;
        self.compose_guard_hook(cwd, &MEMBER_GUARD_ARGS.join(" "))
    }

    /// Merge one Kontor-owned `PreToolUse` guard into the cwd's Claude
    /// settings, preserving unrelated hook events and settings.
    fn compose_guard_hook(&self, cwd: &Path, guard_args: &str) -> io::Result<()> {
        let command = format!(
            "'{}' {guard_args} || exit 2",
            self.command.replace('\'', "'\\''")
        );
        merge_json(&cwd.join(".claude/settings.local.json"), |document| {
            // This worktree is a CSW/ASW. Its consultation hook is owned by
            // Kontor; preserve unrelated hook events and settings.
            let hooks = document
                .entry("hooks")
                .or_insert_with(|| serde_json::json!({}));
            let hooks = hooks.as_object_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Claude hooks must be an object")
            })?;
            let entries = hooks
                .entry("PreToolUse")
                .or_insert_with(|| serde_json::json!([]));
            let entries = entries.as_array_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Claude PreToolUse hooks must be an array",
                )
            })?;
            let guard = serde_json::json!({
                "matcher": ".*",
                "hooks": [{"type": "command", "command": command, "timeout": 10}]
            });
            if !entries.contains(&guard) {
                entries.push(guard);
            }
            Ok(())
        })
    }

    /// Compose the three worktree-local files for one seat cwd.
    ///
    /// Idempotent: repeating it rewrites the same content and appends nothing
    /// twice, so a relaunch or a recovered seat costs nothing.
    ///
    /// # Errors
    /// Any filesystem failure, an existing config file that is not JSON (it is
    /// refused rather than clobbered), or a cwd that is not a git worktree.
    pub fn compose(&self, cwd: &Path, serve_profile: &str) -> io::Result<()> {
        // Resolve the exclude destination before writing either seat file. A
        // logical-only ECP currently has a non-Git cwd; that is a typed refusal,
        // not permission to leave half-composed authority files behind.
        let exclude_path = git_exclude_path(cwd)?;
        write_mcp_json(cwd, &self.command, &self.state_root, serve_profile)?;
        write_claude_settings(cwd)?;
        exclude_from_git(&exclude_path, CLAUDE_EXCLUDED)
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
                "type": "stdio",
                "command": self.command,
                "args": [
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
/// This is only Claude's cwd configuration. Every provider also receives the
/// leadership MCP in its creation frame via `PaseoRpc::with_leadership_mcp`;
/// the scoped credential remains exclusively in that frame's secret channel.
/// Selected only after the adapter proves the exact hosting workspace.
#[derive(Clone, Copy)]
pub(crate) enum HostedCompositionMode {
    Worktree,
    OwnedDirectory,
}

pub(crate) fn compose_for_hosted_seat(
    seat: Option<&SeatMcp>,
    provider: &str,
    cwd: &Path,
    mode: HostedCompositionMode,
) -> io::Result<()> {
    if let (Some(seat), HostedCompositionMode::OwnedDirectory) = (seat, mode) {
        // Recovery is required even when a route correction now selects a
        // different provider. Credentials never enter these shared files.
        return compose_hosted_directory(seat, provider, cwd, &mut |_| Ok(()));
    }
    match seat {
        Some(seat) if crate::client::built_in_provider(provider) == "claude" => {
            seat.compose(cwd, "leadership")
        }
        _ => Ok(()),
    }
}

/// Pending publication/rollback failures must not claim zero effects.
pub(crate) fn hosted_composition_refusal(error: &io::Error) -> &'static str {
    if error
        .get_ref()
        .is_some_and(|error| error.is::<IncompletePublication>())
    {
        "hosted directory MCP cleanup is incomplete; publication recovery must finish before create or reload"
    } else {
        "hosted seat MCP composition failed in the ECP"
    }
}

#[derive(Debug)]
struct IncompletePublication(String);

impl std::fmt::Display for IncompletePublication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "incomplete hosted directory MCP cleanup: {}",
            self.0
        )
    }
}

impl std::error::Error for IncompletePublication {}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryFile {
    original: Option<Vec<u8>>,
    rendered: Vec<u8>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryPublication {
    version: u32,
    cwd: PathBuf,
    attempt: uuid::Uuid,
    created_claude_directory: bool,
    settings: DirectoryFile,
    mcp: DirectoryFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublicationStep {
    StageSettings,
    StageMcp,
    PublishSettings,
    PublishMcp,
    Readback,
    RollbackSettings,
    RollbackMcp,
    Cleanup,
}

// All adapter instances and all subjects sharing a cwd use this same lock.
// The daemon's state-root lock excludes another process using its journal.
fn directory_lock(cwd: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    type Locks = std::collections::BTreeMap<PathBuf, std::sync::Arc<std::sync::Mutex<()>>>;
    static LOCKS: std::sync::OnceLock<std::sync::Mutex<Locks>> = std::sync::OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    locks.entry(cwd.to_owned()).or_default().clone()
}

fn directory_file(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => {
            #[cfg(unix)]
            {
                use rustix::fs::{Mode, OFlags};
                use std::io::Read as _;
                let parent = owned_parent(path)?;
                let fd = rustix::fs::openat(
                    &parent,
                    path.file_name().unwrap(),
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?;
                let mut file = std::fs::File::from(fd);
                if !file.metadata()?.is_file() {
                    return Err(io::Error::other(
                        "hosted configuration is not a regular file",
                    ));
                }
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)?;
                Ok(Some(bytes))
            }
            #[cfg(not(unix))]
            {
                std::fs::read(path).map(Some)
            }
        }
        Ok(_) => Err(io::Error::other(format!(
            "{} is not a regular owned file",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn owned_parent(path: &Path) -> io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags};
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("hosted file has no parent"))?;
    let fd = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    Ok(std::fs::File::from(fd))
}

fn replace_directory_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let source_parent = owned_parent(temporary)?;
        let target_parent = owned_parent(destination)?;
        rustix::fs::renameat(
            &source_parent,
            temporary.file_name().unwrap(),
            &target_parent,
            destination.file_name().unwrap(),
        )?;
        target_parent.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::rename(temporary, destination)
    }
}

fn create_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = owned_parent(path)?;
        rustix::fs::mkdirat(&parent, path.file_name().unwrap(), rustix::fs::Mode::RWXU)?;
        parent.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(path)
    }
}

fn remove_created_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = owned_parent(path)?;
        rustix::fs::unlinkat(
            &parent,
            path.file_name().unwrap(),
            rustix::fs::AtFlags::REMOVEDIR,
        )?;
        parent.sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::remove_dir(path)
    }
}

fn check_directory(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => Ok(true),
        Ok(_) => Err(io::Error::other(format!(
            "{} is not an owned directory",
            path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn directory_paths(cwd: &Path) -> io::Result<(PathBuf, PathBuf)> {
    if std::fs::canonicalize(cwd)? != cwd {
        return Err(io::Error::other(
            "the canonical hosted cwd changed during publication",
        ));
    }
    check_directory(&cwd.join(".claude"))?;
    Ok((
        cwd.join(".claude/settings.local.json"),
        cwd.join(".mcp.json"),
    ))
}

fn render_directory_json(
    original: Option<&[u8]>,
    mutate: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> io::Result<()>,
) -> io::Result<Vec<u8>> {
    let mut object = match original {
        None => serde_json::Map::new(),
        Some(bytes) => match serde_json::from_slice(bytes) {
            Ok(serde_json::Value::Object(object)) => object,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "hosted configuration must be a JSON object",
                ));
            }
        },
    };
    mutate(&mut object)?;
    let mut bytes =
        serde_json::to_vec_pretty(&serde_json::Value::Object(object)).map_err(io::Error::other)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn hosted_git_worktree(cwd: &Path) -> io::Result<bool> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--is-inside-work-tree"])
        .env("LC_ALL", "C")
        .output()?;
    if output.status.success() && output.stdout == b"true\n" {
        return Ok(true);
    }
    // Distinguish a genuine non-repository from permission, ownership, broken
    // repository and executable errors. None of those authorizes a fallback.
    if output.status.code() == Some(128)
        && String::from_utf8_lossy(&output.stderr)
            .starts_with("fatal: not a git repository (or any of the parent directories): .git")
        && !hosted_git_marker(cwd)?
    {
        return Ok(false);
    }
    Err(io::Error::other(format!(
        "hosted Git probe refused: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn hosted_git_marker(cwd: &Path) -> io::Result<bool> {
    let ceilings = std::env::var_os("GIT_CEILING_DIRECTORIES")
        .map(|value| {
            std::env::split_paths(&value)
                .filter_map(|path| std::fs::canonicalize(path).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for parent in cwd.ancestors() {
        if ceilings.iter().any(|ceiling| ceiling == parent) {
            break;
        }
        match parent.join(".git").symlink_metadata() {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn stage_directory_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    #[cfg(unix)]
    let mut file = {
        use rustix::fs::{Mode, OFlags};
        let parent = owned_parent(path)?;
        std::fs::File::from(rustix::fs::openat(
            &parent,
            path.file_name().unwrap(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?)
    };
    #[cfg(not(unix))]
    let mut file = {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        options.open(path)?
    };
    file.write_all(bytes)?;
    file.sync_all()
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags};
        std::fs::File::from(rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?)
        .sync_all()
    }
    #[cfg(not(unix))]
    {
        std::fs::File::open(path)?.sync_all()
    }
}

impl DirectoryPublication {
    fn temporary(&self, settings: bool, restoring: bool) -> PathBuf {
        let directory = if settings {
            self.cwd.join(".claude")
        } else {
            self.cwd.clone()
        };
        directory.join(format!(
            ".kontor-{}-{}.tmp",
            self.attempt,
            if restoring { "restore" } else { "stage" }
        ))
    }

    fn verify_current(&self) -> io::Result<()> {
        let (settings, mcp) = directory_paths(&self.cwd)?;
        for (path, file) in [(settings, &self.settings), (mcp, &self.mcp)] {
            let current = directory_file(&path)?;
            if current != file.original && current.as_deref() != Some(file.rendered.as_slice()) {
                return Err(io::Error::other(format!(
                    "{} changed outside the pending publication",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    fn complete(&self) -> io::Result<bool> {
        let (settings, mcp) = directory_paths(&self.cwd)?;
        Ok(
            directory_file(&settings)?.as_deref() == Some(self.settings.rendered.as_slice())
                && directory_file(&mcp)?.as_deref() == Some(self.mcp.rendered.as_slice()),
        )
    }

    fn rollback(
        &self,
        fault: &mut impl FnMut(PublicationStep) -> io::Result<()>,
    ) -> io::Result<()> {
        self.verify_current()?;
        let (settings, mcp) = directory_paths(&self.cwd)?;
        for (path, file, is_settings, step) in [
            (
                settings,
                &self.settings,
                true,
                PublicationStep::RollbackSettings,
            ),
            (mcp, &self.mcp, false, PublicationStep::RollbackMcp),
        ] {
            if directory_file(&path)? == file.original {
                continue;
            }
            fault(step)?;
            match &file.original {
                Some(bytes) => {
                    let temporary = self.temporary(is_settings, true);
                    remove_attempt_file(&temporary)?;
                    stage_directory_file(&temporary, bytes)?;
                    directory_paths(&self.cwd)?;
                    replace_directory_file(&temporary, &path)?;
                }
                None => remove_attempt_file(&path)?,
            }
            sync_directory(path.parent().expect("a hosted file has a parent"))?;
        }
        Ok(())
    }

    fn cleanup(&self, record: &Path, rolled_back: bool) -> io::Result<()> {
        directory_paths(&self.cwd)?;
        for settings in [true, false] {
            for restoring in [true, false] {
                remove_attempt_file(&self.temporary(settings, restoring))?;
            }
        }
        if rolled_back
            && self.created_claude_directory
            && check_directory(&self.cwd.join(".claude"))?
        {
            // Only this attempt's now-empty directory; foreign contents stay.
            match remove_created_directory(&self.cwd.join(".claude")) {
                Ok(()) => sync_directory(&self.cwd)?,
                Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => {}
                Err(error) => return Err(error),
            }
        }
        sync_directory(&self.cwd)?;
        remove_attempt_file(record)?;
        sync_directory(record.parent().expect("a publication record has a parent"))
    }
}

fn remove_attempt_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    let result = (|| {
        let parent = owned_parent(path)?;
        rustix::fs::unlinkat(
            &parent,
            path.file_name().unwrap(),
            rustix::fs::AtFlags::empty(),
        )
        .map_err(io::Error::from)
    })();
    #[cfg(not(unix))]
    let result = std::fs::remove_file(path);
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn publication_record(seat: &SeatMcp, cwd: &Path) -> io::Result<PathBuf> {
    let root = std::fs::canonicalize(&seat.state_root)?;
    let directory = root.join("hosted-directory-publications");
    check_directory(&directory)?;
    let key = kontor_core::id::ContentHash::of(cwd.as_os_str().as_encoded_bytes());
    Ok(directory.join(format!("{key}.json")))
}

fn recover_directory_publication(
    record: &Path,
    cwd: &Path,
    fault: &mut impl FnMut(PublicationStep) -> io::Result<()>,
) -> io::Result<()> {
    let Some(bytes) = directory_file(record).map_err(|error| {
        io::Error::other(IncompletePublication(format!(
            "{}: {error}",
            record.display()
        )))
    })?
    else {
        return Ok(());
    };
    let mut recover = || -> io::Result<()> {
        let publication: DirectoryPublication =
            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if publication.version != 1 || publication.cwd != cwd {
            return Err(io::Error::other(
                "publication record does not name this canonical cwd/version",
            ));
        }
        for bytes in [&publication.settings.rendered, &publication.mcp.rendered] {
            if !serde_json::from_slice::<serde_json::Value>(bytes)
                .is_ok_and(|value| value.is_object())
            {
                return Err(io::Error::other(
                    "publication record has invalid rendered JSON",
                ));
            }
        }
        publication.verify_current()?;
        let complete = publication.complete()?;
        if !complete {
            publication.rollback(fault)?;
        }
        publication.cleanup(record, !complete)
    };
    recover().map_err(|error| {
        io::Error::other(IncompletePublication(format!(
            "{}: {error}",
            record.display()
        )))
    })
}

fn compose_hosted_directory(
    seat: &SeatMcp,
    provider: &str,
    cwd: &Path,
    fault: &mut impl FnMut(PublicationStep) -> io::Result<()>,
) -> io::Result<()> {
    let cwd = std::fs::canonicalize(cwd)?;
    let lock = directory_lock(&cwd);
    // A caught interruption may poison this process's lock; the persisted
    // record, rather than in-memory state, decides whether publication is safe.
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let record = publication_record(seat, &cwd)?;
    recover_directory_publication(&record, &cwd, fault)?;
    if crate::client::built_in_provider(provider) != "claude" {
        return Ok(());
    }
    if hosted_git_worktree(&cwd)? {
        return seat.compose(&cwd, "leadership");
    }
    let (settings_path, mcp_path) = directory_paths(&cwd)?;
    let settings_original = directory_file(&settings_path)?;
    let mcp_original = directory_file(&mcp_path)?;
    // Both objects are read/validated/rendered before even creating .claude.
    let settings = DirectoryFile {
        rendered: render_directory_json(settings_original.as_deref(), |object| {
            object.insert(
                "enableAllProjectMcpServers".to_owned(),
                serde_json::json!(true),
            );
            Ok(())
        })?,
        original: settings_original,
    };
    let mcp = DirectoryFile {
        rendered: render_directory_json(mcp_original.as_deref(), |object| {
            let servers = object
                .entry("mcpServers")
                .or_insert_with(|| serde_json::json!({}));
            let servers = servers.as_object_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "existing .mcp.json has non-object mcpServers",
                )
            })?;
            servers.insert(
                "kontor".to_owned(),
                mcp_file_entry(&seat.command, &seat.state_root, "leadership"),
            );
            Ok(())
        })?,
        original: mcp_original,
    };
    if settings.original.as_deref() == Some(settings.rendered.as_slice())
        && mcp.original.as_deref() == Some(mcp.rendered.as_slice())
    {
        return Ok(());
    }
    let publication = DirectoryPublication {
        version: 1,
        cwd: cwd.clone(),
        attempt: uuid::Uuid::now_v7(),
        created_claude_directory: !check_directory(&cwd.join(".claude"))?,
        settings,
        mcp,
    };
    if !check_directory(record.parent().unwrap())? {
        match create_directory(record.parent().unwrap()) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                check_directory(record.parent().unwrap())?;
            }
            Err(error) => return Err(error),
        }
        sync_directory(record.parent().unwrap().parent().unwrap())?;
    }
    // Write and fsync originals before any destination changes. A process
    // interruption leaves a record the prepared-intent retry must reconcile.
    let record_temp = record.with_extension(format!("{}.tmp", publication.attempt));
    let bytes = serde_json::to_vec(&publication).map_err(io::Error::other)?;
    let record_result = (|| {
        stage_directory_file(&record_temp, &bytes)?;
        replace_directory_file(&record_temp, &record)?;
        sync_directory(record.parent().unwrap())
    })();
    if let Err(error) = record_result {
        remove_attempt_file(&record_temp)?;
        recover_directory_publication(&record, &cwd, fault)?;
        return Err(error);
    }
    let result = (|| {
        if publication.created_claude_directory {
            create_directory(&cwd.join(".claude"))?;
            sync_directory(&cwd)?;
        }
        for (file, settings, step) in [
            (&publication.settings, true, PublicationStep::StageSettings),
            (&publication.mcp, false, PublicationStep::StageMcp),
        ] {
            if file.original.as_deref() == Some(file.rendered.as_slice()) {
                continue;
            }
            fault(step)?;
            directory_paths(&cwd)?;
            stage_directory_file(&publication.temporary(settings, false), &file.rendered)?;
        }
        for (path, file, settings, step) in [
            (
                settings_path,
                &publication.settings,
                true,
                PublicationStep::PublishSettings,
            ),
            (
                mcp_path,
                &publication.mcp,
                false,
                PublicationStep::PublishMcp,
            ),
        ] {
            if file.original.as_deref() == Some(file.rendered.as_slice()) {
                continue;
            }
            fault(step)?;
            publication.verify_current()?;
            replace_directory_file(&publication.temporary(settings, false), &path)?;
            sync_directory(path.parent().unwrap())?;
        }
        fault(PublicationStep::Readback)?;
        if !publication.complete()? {
            return Err(io::Error::other(
                "hosted MCP pair readback differs from publication",
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup) = publication
            .rollback(fault)
            .and_then(|()| publication.cleanup(&record, true))
        {
            return Err(io::Error::other(IncompletePublication(format!(
                "{error}; {}: {cleanup}",
                record.display()
            ))));
        }
        return Err(error);
    }
    fault(PublicationStep::Cleanup)
        .and_then(|()| publication.cleanup(&record, false))
        .map_err(|error| {
            io::Error::other(IncompletePublication(format!(
                "{}: {error}",
                record.display()
            )))
        })
}

/// Merge-write `<cwd>/.mcp.json`: overwrite only the `kontor` server entry.
fn write_mcp_json(
    cwd: &Path,
    command: &str,
    state_root: &Path,
    serve_profile: &str,
) -> io::Result<()> {
    let kontor = mcp_file_entry(command, state_root, serve_profile);
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

fn mcp_file_entry(command: &str, state_root: &Path, serve_profile: &str) -> serde_json::Value {
    serde_json::json!({
        "command": command,
        "args": [
            "--state-root", state_root.to_string_lossy(),
            "--credential-tier", "operator",
            "--serve-profile", serve_profile,
        ],
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

/// Resolve the worktree-local Git exclude file before composing seat files.
fn git_exclude_path(cwd: &Path) -> io::Result<PathBuf> {
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
    Ok(path)
}

/// The lines a composed Claude seat must never surface as in `git status`.
const CLAUDE_EXCLUDED: &[&str] = &[".mcp.json", ".claude/"];

fn exclude_from_git(path: &Path, lines: &[&str]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let existing = match std::fs::read_to_string(path) {
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
    std::fs::write(path, content)
}

#[cfg(test)]
#[path = "seat_mcp_directory_tests.rs"]
mod directory_tests;

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
        compose_for_hosted_seat(
            Some(&seat("/realm/state")),
            "claude-personal",
            repo.path(),
            HostedCompositionMode::Worktree,
        )
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
        assert!(
            !directory.path().join(".mcp.json").exists()
                && !directory.path().join(".claude").exists(),
            "a refused composition is atomic and leaves no partial seat files"
        );
    }

    #[test]
    fn consultation_guard_is_local_and_does_not_replace_other_hook_events() {
        let repo = repo();
        std::fs::create_dir_all(repo.path().join(".claude")).unwrap();
        std::fs::write(
            repo.path().join(".claude/settings.local.json"),
            r#"{"hooks":{"Stop":[{"hooks":[]}]},"permissions":{"allow":["Bash"]}}"#,
        )
        .unwrap();
        let seat = seat("/realm/state");
        seat.compose_consultation(repo.path()).unwrap();
        seat.compose_consultation(repo.path()).unwrap();
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(repo.path().join(".claude/settings.local.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], ".*");
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "'kontor-mcp' --consultation-tool-guard || exit 2"
        );
        assert!(settings["hooks"]["Stop"].is_array());
        assert!(porcelain(repo.path()).is_empty());
    }
}
