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

/// Render one declared posture for one provider.
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
    let mode = paseo_mode(provider, autonomy)?;
    let harness = built_in_provider(provider);
    // Only opencode reads a written permission block. Cursor and Claude spell
    // every posture they have as a mode, and Codex the same; giving them a block
    // would be a second statement of a rule their mode already makes.
    let opencode = harness == "opencode";
    Ok(SeatPosture {
        mode,
        permission: opencode
            .then(|| opencode_permission(autonomy, allowances))
            .flatten(),
        // Reported only where the provider actually exposes the toggle —
        // verified live against Paseo 0.6.1, that is opencode and cursor;
        // claude and codex expose no features at all.
        auto_accept: matches!(harness, "opencode" | "cursor")
            .then(|| autonomy == SeatAutonomy::Bounded),
    })
}

/// The `permission` block one posture renders to, or `None` when it needs none.
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
    match autonomy {
        // Everything Kontor already authorized, without a second question per
        // tool call — including reading outside the worktree, which is what the
        // wedged seats of 2026-08-22 were actually stopped on.
        SeatAutonomy::Bounded => Some(serde_json::json!({
            "read": "allow",
            "edit": "allow",
            "glob": "allow",
            "grep": "allow",
            "list": "allow",
            "lsp": "allow",
            "skill": "allow",
            "task": "allow",
            "todowrite": "allow",
            "question": "allow",
            "webfetch": "allow",
            "websearch": "allow",
            "external_directory": { "*": "allow" },
            "bash": bash("allow", allowances),
        })),
        // The floor, and nothing else. Naming only `bash` is deliberate: an
        // unlisted tool keeps opencode's own default, so a supervised seat asks
        // exactly what it asked before this existed and gains the denials. A
        // block that also spelled out `read: ask` would be this change quietly
        // making supervised seats *more* likely to stall — which is the outage.
        SeatAutonomy::Supervised => Some(serde_json::json!({ "bash": bash("ask", allowances) })),
        // Advisory seats launch `--mode plan`, which already refuses every edit.
        // An allow-list for a seat that may not act would be a second, weaker
        // statement of the same rule.
        SeatAutonomy::Advisory => None,
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
        seat_posture("opencode", autonomy, allowances)
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

    /// `ask` gains the floor and *only* the floor: this change must never be the
    /// reason a seat that used to run starts asking about more than it did.
    #[test]
    fn ask_gains_the_floor_and_no_new_asks() {
        let permission = opencode(SeatAutonomy::Supervised, &[]);
        assert_eq!(permission["bash"]["*"], "ask");
        for pattern in DESTRUCTIVE_BASH_DENIES {
            assert_eq!(permission["bash"][*pattern], "deny");
        }
        let spelled: Vec<&String> = permission.as_object().expect("an object").keys().collect();
        assert_eq!(
            spelled,
            vec!["bash"],
            "every other tool keeps opencode's own default rather than a new `ask`"
        );
    }

    /// `plan` is already read-only by mode; a block would restate it weakly.
    #[test]
    fn plan_writes_no_block_but_still_pins_the_mode() {
        let posture = seat_posture("opencode", SeatAutonomy::Advisory, &[]).expect("plan");
        assert_eq!(posture.mode, Some("plan"));
        assert!(posture.permission.is_none());
        assert_eq!(posture.auto_accept, Some(false));
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
            let plain = seat_posture("opencode", autonomy, &[]).expect("posture");
            let relaxed = seat_posture(
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
            ("cursor", Some("agent"), Some("ask"), Some("plan")),
            ("opencode", Some("build"), Some("build"), Some("plan")),
        ];
        for (provider, bounded, supervised, advisory) in expected {
            assert_eq!(
                seat_posture(provider, SeatAutonomy::Bounded, &[])
                    .expect("bounded")
                    .mode,
                bounded,
                "{provider} autonomous"
            );
            assert_eq!(
                seat_posture(provider, SeatAutonomy::Supervised, &[])
                    .expect("supervised")
                    .mode,
                supervised,
                "{provider} ask"
            );
            match advisory {
                Some(mode) => assert_eq!(
                    seat_posture(provider, SeatAutonomy::Advisory, &[])
                        .expect("advisory")
                        .mode,
                    Some(mode),
                    "{provider} plan"
                ),
                // Codex has no read-only mode, so an advisory seat on it is
                // refused rather than quietly run under a writing one.
                None => assert!(matches!(
                    seat_posture(provider, SeatAutonomy::Advisory, &[]),
                    Err(RuntimeError::PermissionModeUnsupported { .. })
                )),
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
            seat_posture("opencode", SeatAutonomy::Bounded, &[])
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
                seat_posture(provider, SeatAutonomy::Bounded, &[])
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
            seat_posture("opencode-work", SeatAutonomy::Bounded, &[]).expect("alias"),
            seat_posture("opencode", SeatAutonomy::Bounded, &[]).expect("harness"),
        );
        assert_eq!(
            seat_posture("cursor-personal", SeatAutonomy::Supervised, &[])
                .expect("alias")
                .mode,
            Some("ask"),
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
}
