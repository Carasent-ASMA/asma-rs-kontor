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
    // An OpenCode seat's posture now travels in a Kontor-owned configuration
    // root outside the worktree, named through `agent run --env` and proved
    // against the resolving binary before the seat is created. Project
    // configuration is disabled for that seat, so a file written into the
    // worktree would not be read — it would only put two seats sharing a
    // worktree back in each other's way, and change operator state for nothing.
    //
    // Nor does any other provider touch it: a Claude or Codex seat has no
    // business rewriting OpenCode configuration, and Kontor holds no marker
    // proving a `permission` block found there is one it wrote rather than one a
    // human did.
    //
    // [`compose_permission_block`] and [`verify_composed_posture`] remain for
    // reading receipts an earlier build left behind. They are history, not
    // authority, and nothing on the delivery path calls them.
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
/// layers this verification believes in is reviewable in one place.
#[derive(Debug, Clone)]
pub struct ConfigLayers {
    /// Files merged in order, lowest precedence first, ending with the block
    /// Kontor composed. **Read only** apart from the seat-local file.
    pub merged: Vec<PathBuf>,
    /// Files whose precedence relative to the composed block is *not* proven,
    /// and which therefore may not declare a `permission` at all.
    ///
    /// The `.jsonc` siblings. The installed 1.18.15 reads both spellings in a
    /// directory, and which one wins over the other is not something this code
    /// can demonstrate — so rather than guess an order and risk approving a
    /// stack the evaluator resolves differently, a `permission` declared in one
    /// of these refuses the launch outright.
    pub unordered: Vec<PathBuf>,
    /// The file Kontor composed; it must exist and state the block.
    pub seat_local: PathBuf,
}

impl ConfigLayers {
    /// The layers a seat spawned in `cwd` will actually read.
    ///
    /// `OPENCODE_CONFIG` names a single file merged under everything else;
    /// `OPENCODE_CONFIG_DIR`, `XDG_CONFIG_HOME` and `HOME` resolve the
    /// configuration directory, whose `opencode.json` and `opencode.jsonc` are
    /// both read. Then the repository's own root config, then the seat-local
    /// block. Both `.jsonc` siblings inside the worktree are treated as
    /// unordered rather than merged — see [`Self::unordered`].
    #[must_use]
    pub fn for_seat(cwd: &Path) -> Self {
        let mut merged = Vec::new();
        if let Some(file) = std::env::var_os("OPENCODE_CONFIG") {
            merged.push(PathBuf::from(file));
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
            merged.push(directory.join("opencode.json"));
            merged.push(directory.join("opencode.jsonc"));
        }
        let seat_local = cwd.join(".opencode/opencode.json");
        merged.push(cwd.join("opencode.json"));
        merged.push(seat_local.clone());
        Self {
            merged,
            unordered: vec![
                cwd.join("opencode.jsonc"),
                cwd.join(".opencode/opencode.jsonc"),
            ],
            seat_local,
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
/// and survives for every key the block does not name; a rule committed in the
/// repository's root `opencode.json` survives the same way; and a `.jsonc`
/// sibling of either is read too. Each was reproduced against the installed
/// 1.18.15. So the whole stack is merged here in OpenCode's own order and the
/// result must equal the rendered posture exactly — any surviving rule the
/// posture did not state refuses the launch, widening or not.
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
/// A missing, unreadable, unparseable or non-object layer; a `permission`
/// declared in a file whose precedence is unproven; an effective `permission`
/// differing in any way from the rendered posture; or a floor pattern that is
/// not denied and was not exactly relaxed by a declared allowance.
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
    for path in &layers.unordered {
        if permission_of(path)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} declares a permission whose precedence over the composed block is not \
                     proven; refusing to spawn",
                    path.display()
                ),
            ));
        }
    }
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

/// Strip JSONC to JSON: line and block comments, and trailing commas.
///
/// OpenCode accepts `opencode.jsonc`, and `serde_json` does not. Written out
/// rather than taken as a dependency because the job is small and exactly
/// specified, and because a config this refuses to read refuses a launch — the
/// behaviour has to be inspectable here.
fn strip_jsonc(source: &str) -> String {
    /// Remove line and block comments that are not inside a string.
    fn without_comments(source: &str) -> String {
        let bytes = source.as_bytes();
        let mut out = String::with_capacity(source.len());
        let mut index = 0;
        let mut in_string = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if in_string {
                out.push(byte as char);
                if byte == b'\\' && index + 1 < bytes.len() {
                    out.push(bytes[index + 1] as char);
                    index += 2;
                    continue;
                }
                if byte == b'"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }
            match (byte, bytes.get(index + 1)) {
                (b'"', _) => {
                    in_string = true;
                    out.push('"');
                    index += 1;
                }
                (b'/', Some(b'/')) => {
                    while index < bytes.len() && bytes[index] != b'\n' {
                        index += 1;
                    }
                }
                (b'/', Some(b'*')) => {
                    index += 2;
                    while index + 1 < bytes.len()
                        && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                    {
                        index += 1;
                    }
                    index = (index + 2).min(bytes.len());
                }
                _ => {
                    out.push(byte as char);
                    index += 1;
                }
            }
        }
        out
    }

    /// Drop a comma whose next meaningful character closes its container.
    ///
    /// Run *after* comments are gone, so a comment sitting between the comma and
    /// the brace cannot hide the fact that the comma is trailing.
    fn without_trailing_commas(source: &str) -> String {
        let bytes = source.as_bytes();
        let mut out = String::with_capacity(source.len());
        let mut index = 0;
        let mut in_string = false;
        while index < bytes.len() {
            let byte = bytes[index];
            if in_string {
                out.push(byte as char);
                if byte == b'\\' && index + 1 < bytes.len() {
                    out.push(bytes[index + 1] as char);
                    index += 2;
                    continue;
                }
                if byte == b'"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }
            if byte == b'"' {
                in_string = true;
                out.push('"');
                index += 1;
                continue;
            }
            if byte == b',' {
                let mut lookahead = index + 1;
                while lookahead < bytes.len() && bytes[lookahead].is_ascii_whitespace() {
                    lookahead += 1;
                }
                if matches!(bytes.get(lookahead), Some(b'}' | b']')) {
                    index += 1;
                    continue;
                }
            }
            out.push(byte as char);
            index += 1;
        }
        out
    }

    without_trailing_commas(&without_comments(source))
}

/// The `permission` one configuration file declares, if it declares one.
fn permission_of(path: &Path) -> io::Result<Option<serde_json::Value>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8_lossy(&bytes);
    let source = if path
        .extension()
        .is_some_and(|extension| extension == "jsonc")
    {
        strip_jsonc(&text)
    } else {
        text.into_owned()
    };
    match serde_json::from_str::<serde_json::Value>(&source) {
        Ok(serde_json::Value::Object(document)) => Ok(document.get("permission").cloned()),
        Ok(_) | Err(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a JSON object; the effective permission cannot be proved",
                path.display()
            ),
        )),
    }
}

/// The `permission` OpenCode resolves from these layers, merged in its order.
fn effective_permission(layers: &ConfigLayers) -> io::Result<serde_json::Value> {
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
    let mut effective = serde_json::Value::Null;
    for path in &layers.merged {
        if let Some(permission) = permission_of(path)? {
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

    /// The JSONC reader strips what the spelling allows and nothing else.
    #[test]
    fn jsonc_stripping_leaves_strings_alone() {
        let stripped = strip_jsonc(
            "{\n  // line\n  /* block */\n  \"url\": \"https://example.test//not-a-comment\",\n  \"list\": [1, 2,],\n}",
        );
        let document: serde_json::Value = stripped.parse().expect("valid JSON after stripping");
        assert_eq!(document["url"], "https://example.test//not-a-comment");
        assert_eq!(document["list"], serde_json::json!([1, 2]));
    }
}
