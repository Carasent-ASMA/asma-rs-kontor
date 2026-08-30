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
use kontor_runtime::adapter::RuntimeResult;

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
    /// The `permission` object the seat is created with, when the provider
    /// takes one. Only OpenCode does today.
    ///
    /// It travels in `create_agent_request`'s
    /// `config.providerOptions.permission`, which installed Paseo 0.6.1
    /// validates against OpenCode's own `Config.permission` schema, persists on
    /// the agent record, and replays into `session.promptAsync` on every turn —
    /// where OpenCode installs it on the session before evaluating any tool
    /// call. It is **not** written into the seat's worktree, and nothing about
    /// it is resolved from files or environment.
    pub permission: Option<serde_json::Value>,
    /// Whether the harness should accept its own tool calls without asking.
    ///
    /// OpenCode's per-agent `auto_accept` feature, stated as intent. **Nothing
    /// consumes it, and nothing needs to**: the permission object carried in
    /// `providerOptions` is what decides whether a seat is asked, and it is
    /// applied per turn by the provider itself. This is retained only so a
    /// future feature surface would find the value derived in the same place as
    /// the rest of the posture. See OQ-OP20-2.
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
/// # Why OpenCode's block is rendered here but never written
///
/// OpenCode carries its posture in a permission block rather than in a mode, and
/// no file or environment variable can deliver that block provably. The inputs
/// that decide it are read by the *spawned* process, and several of them sit
/// above anything Kontor could write: `OPENCODE_CONFIG_CONTENT` and
/// `OPENCODE_PERMISSION` inject permissions outright,
/// `OPENCODE_DISABLE_PROJECT_CONFIG` discards the project layer, and the
/// active-org remote config and managed profiles merge later still and depend on
/// who the seat authenticated as.
///
/// So the block this function renders does not go to disk. It travels as
/// `config.providerOptions.permission` on the seat's `create_agent_request`,
/// which the daemon validates, persists on the agent, and replays into
/// `session.promptAsync` on every turn — leaving the merge order above nothing
/// to act on. See
/// [`PaseoRpc::delivery_agent_create`](crate::client::PaseoRpc::delivery_agent_create)
/// for the create and `PaseoAdapter::prove_first_turn` for the acceptance a
/// launch binds on.
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
    // OpenCode is no longer refused here. It is admitted *only* behind the proof
    // the launch path runs before creating anything: the daemon must apply
    // per-agent environment, the binary Paseo resolves must be one this posture
    // was proved against, and its resolved permission must equal this block
    // exactly. A Paseo that cannot carry the environment still fails closed —
    // see `PaseoAdapter::prove_opencode_posture`.
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

/// Everything that makes one launch *this* launch.
///
/// Hashed into a label so a reconciling census can recognise the agent this
/// launch created and refuse anything else.
///
/// The digest is taken over **the create configuration that is actually sent**,
/// not over a hand-listed subset of it. Listing fields is how a digest quietly
/// stops covering the thing it names: an earlier version of this covered only
/// binding, run, place and slot, and so said nothing about provider, model, cwd,
/// mode, thinking option, title or MCP surface — every one of which changes what
/// the create does. Passing the config itself means a field added to the create
/// is covered the day it is added, with nobody having to remember.
///
/// Excluded, necessarily: the correlation id, which differs per attempt and
/// would make a retry's digest disagree with the agent it is looking for, and
/// the intent label itself, which cannot contain its own hash.
#[derive(Debug, Clone, Copy)]
pub struct LaunchIntent<'a> {
    /// The Kontor session binding this seat is placed under.
    pub binding_id: &'a str,
    /// The agent run being launched.
    pub agent_run_id: &'a str,
    /// The native place it is created in.
    pub workspace_id: &'a str,
    /// The role slot it fills.
    pub role_slot_id: &'a str,
    /// The complete `create_agent_request.config` this launch will send.
    pub config: &'a serde_json::Value,
}

impl LaunchIntent<'_> {
    /// The digest that travels in
    /// [`label::LAUNCH_INTENT`](crate::wire::label::LAUNCH_INTENT).
    ///
    /// Field-separated so no two different intents can hash the same by
    /// concatenation, and carrying no value of anything it covers. The config is
    /// serialized canonically — `serde_json::Map` is a `BTreeMap` here — so the
    /// same intent digests identically on every host and on every attempt.
    #[must_use]
    pub fn digest(&self) -> ContentHash {
        let material = format!(
            "binding={}\nagent_run={}\nworkspace={}\nrole_slot={}\nconfig={}",
            self.binding_id, self.agent_run_id, self.workspace_id, self.role_slot_id, self.config,
        );
        ContentHash::of(material.as_bytes())
    }
}

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

    /// An OpenCode delivery seat renders its block here and carries it on the
    /// create, so this renderer must admit OpenCode at every posture.
    ///
    /// Refusing here would refuse OpenCode delivery outright. What actually
    /// governs a launch is the adapter's gate — the daemon must accept typed
    /// per-agent `providerOptions` — and then the first turn it binds on.
    #[test]
    fn opencode_renders_here_and_is_proved_at_the_launch_boundary() {
        // Every posture renders a block, because the block is what the create
        // carries. A posture that rendered none would create a seat with no
        // policy in `providerOptions` at all.
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            for provider in ["opencode", "opencode-work"] {
                let posture = seat_posture(provider, autonomy, &[])
                    .unwrap_or_else(|error| panic!("{provider} {autonomy:?}: {error:?}"));
                assert!(
                    posture.permission.is_some(),
                    "{provider} {autonomy:?} carries the block the proof compares against"
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

    fn intent<'a>(config: &'a serde_json::Value) -> LaunchIntent<'a> {
        LaunchIntent {
            binding_id: "bind-1",
            agent_run_id: "run-1",
            workspace_id: "wks-1",
            role_slot_id: "implement-a",
            config,
        }
    }

    fn create_config() -> serde_json::Value {
        serde_json::json!({
            "provider": "opencode",
            "cwd": "/w/task-1",
            "model": "deepseek/deepseek-v4-flash",
            "title": "Implement",
            "modeId": "build",
            "thinkingOptionId": "max",
            "providerOptions": { "permission": opencode(SeatAutonomy::Bounded, &[]) },
            "mcpServers": { "kontor": { "type": "local" } },
        })
    }

    /// **Every effective create field** moves the digest. A field the digest
    /// does not cover is a field a reconciling census would accept a different
    /// value of.
    #[test]
    fn a_launch_intent_digest_covers_every_effective_create_field() {
        let base = create_config();
        let baseline = intent(&base).digest();

        for (field, value) in [
            ("provider", serde_json::json!("claude")),
            ("cwd", serde_json::json!("/w/other")),
            ("model", serde_json::json!("other-model")),
            ("title", serde_json::json!("Other")),
            ("modeId", serde_json::json!("plan")),
            ("thinkingOptionId", serde_json::json!("low")),
            (
                "providerOptions",
                serde_json::json!({ "permission": opencode(SeatAutonomy::Advisory, &[]) }),
            ),
            (
                "mcpServers",
                serde_json::json!({ "kontor": { "type": "remote" } }),
            ),
        ] {
            let mut changed = base.clone();
            changed[field] = value;
            assert_ne!(
                baseline,
                intent(&changed).digest(),
                "`{field}` must move the launch-intent digest"
            );
        }

        // And a field removed entirely, not merely changed.
        let mut without = base.clone();
        without.as_object_mut().expect("map").remove("mcpServers");
        assert_ne!(
            baseline,
            intent(&without).digest(),
            "dropping a field must move the digest too"
        );
        assert_eq!(baseline, intent(&base).digest(), "and it is stable");
    }

    /// The identity fields are covered as well as the configuration.
    #[test]
    fn a_launch_intent_digest_covers_its_identity() {
        let config = create_config();
        let baseline = intent(&config).digest();
        for mutate in [
            |i: &mut LaunchIntent| i.binding_id = "bind-2",
            |i: &mut LaunchIntent| i.agent_run_id = "run-2",
            |i: &mut LaunchIntent| i.workspace_id = "wks-2",
            |i: &mut LaunchIntent| i.role_slot_id = "implement-b",
        ] {
            let mut changed = intent(&config);
            mutate(&mut changed);
            assert_ne!(baseline, changed.digest());
        }
    }

    /// Fields are separated, so no two intents collide by running together.
    #[test]
    fn a_launch_intent_digest_cannot_collide_by_concatenation() {
        let config = serde_json::json!({});
        let mut first = intent(&config);
        first.binding_id = "a";
        first.agent_run_id = "bc";
        let mut second = intent(&config);
        second.binding_id = "ab";
        second.agent_run_id = "c";
        assert_ne!(first.digest(), second.digest());
    }

    /// It carries no value of what it covers.
    #[test]
    fn a_launch_intent_digest_carries_no_value() {
        let config = create_config();
        let rendered = intent(&config).digest().to_string();
        for secret in ["bind-1", "run-1", "wks-1", "implement-a", "deny", "allow"] {
            assert!(
                !rendered.contains(secret),
                "the digest leaks `{secret}`: {rendered}"
            );
        }
    }
}
