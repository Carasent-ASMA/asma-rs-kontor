//! ASMA-8280 B-1: `kontor fleet-policy-resolve` is the registry's local
//! operation. It runs against `--state-root` alone — no daemon, credential
//! file or base URL — through the shared verified reader, and prints the
//! `FleetSelection` the one resolver chose, or fails closed.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use assert_cmd::Command;
use kontor_core::id::ContentHash;
use kontor_daemon::fleet::{ActivationFence, FleetSource};
use kontor_daemon::orchestration::{
    BundleSources, propose_core_team, propose_orchestration, resolve_bundle,
};
use kontor_fleet::Eligibility;
use kontor_fleet_activation::rule;

const TEAM: &str = "team/01936f5a-0000-7000-8000-000000000102/implement";

/// A v2 policy: both leadership slots of `roster`, and one delivery key.
fn policy(roster: &str) -> String {
    format!(
        "schema_version: 2\n\
         domains:\n  codex: {{ provider: codex, accounts: [codex-work] }}\n  claude: {{ provider: claude, accounts: [claude-personal] }}\n\
         models:\n  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}\n  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}\n\
         chains:\n  codex-first:\n    - [sol@xhigh]\n    - [opus@xhigh]\n\
         bindings:\n  leadership/{roster}/lsa: codex-first\n  leadership/{roster}/tpm: codex-first\n  {TEAM}: codex-first\n"
    )
}

/// A fresh state root holding one published and activated aligned bundle,
/// and nothing else: no credential file, no endpoint file, no daemon.
struct Realm {
    root: tempfile::TempDir,
    roster: String,
}

impl Realm {
    fn aligned() -> Self {
        let root = tempfile::tempdir().expect("state root");
        let catalog = kontor_profiles::bundled_operational_domain()
            .expect("the bundled domain")
            .role_catalogs
            .remove(0);
        let orchestration = propose_orchestration().expect("selector");
        let core_team = propose_core_team(&catalog).expect("proposal");
        // The roster hash the policy's leadership keys name comes from the
        // proposal itself, resolved once with a policy binding no leadership.
        let roster = resolve_bundle(
            &BundleSources {
                orchestration: &orchestration,
                fleet: &policy("0000000000000000000000000000000000000000000000000000000000000000"),
                core_team: &core_team,
            },
            &catalog,
        )
        .expect("the proposal resolves")
        .roster
        .hash()
        .to_string();
        let fleet = policy(&roster);
        let bundle = resolve_bundle(
            &BundleSources {
                orchestration: &orchestration,
                fleet: &fleet,
                core_team: &core_team,
            },
            &catalog,
        )
        .expect("the bundle resolves");
        assert_eq!(bundle.roster.hash().as_str(), roster);
        let source = FleetSource::at(root.path());
        let published = source.publish_bundle(&bundle).expect("published");
        source
            .activate_bundle(&published.bundle_hash, &ActivationFence::default())
            .expect("activated");
        Self { root, roster }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn leadership(&self, slot: &str) -> String {
        format!("leadership/{}/{slot}", self.roster)
    }
}

/// Run one command against `root` and read its one document and exit code.
fn kontor(root: &Path, arguments: &[&str]) -> (i32, serde_json::Value) {
    let output = Command::cargo_bin("kontor")
        .expect("the kontor binary")
        .arg("--state-root")
        .arg(root)
        .args(arguments)
        .output()
        .expect("kontor runs");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    let document = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("one JSON document, got {stdout:?}: {error}"));
    (output.status.code().expect("an exit code"), document)
}

fn resolve(root: &Path, key: &str, extra: &[&str]) -> (i32, serde_json::Value) {
    let mut arguments = vec![
        "--tier",
        "admin",
        "fleet-policy-resolve",
        "--binding-key",
        key,
    ];
    arguments.extend_from_slice(extra);
    kontor(root, &arguments)
}

/// A local refusal: nothing was dispatched, and the rule is the reader's.
fn assert_refused(answer: &(i32, serde_json::Value), exit: i32, code: &str, rule: &str) {
    let (status, document) = answer;
    assert_eq!(*status, exit, "{document}");
    assert_eq!(
        document["tool"], "kontor_fleet_policy_resolve",
        "{document}"
    );
    assert_eq!(document["code"], code, "{document}");
    assert_eq!(document["rule"], rule, "{document}");
    assert_eq!(document["dispatched"], false, "{document}");
}

fn write_private(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, bytes).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("mode");
}

#[test]
fn an_aligned_bundle_resolves_every_seat_class_without_a_daemon() {
    let realm = Realm::aligned();
    assert!(
        !realm.path().join("credentials.json").exists(),
        "no credential file exists to be read"
    );
    for key in [
        realm.leadership("lsa"),
        realm.leadership("tpm"),
        TEAM.to_owned(),
    ] {
        let (exit, envelope) = resolve(realm.path(), &key, &[]);
        assert_eq!(exit, 0, "{envelope}");
        assert_eq!(envelope["tool"], "kontor_fleet_policy_resolve");
        assert_eq!(envelope["status"], 200);
        let expected =
            kontor_fleet_activation::resolve(realm.path(), &key, &Eligibility::default())
                .expect("the shared reader resolves");
        assert_eq!(
            envelope["body"],
            serde_json::to_value(&expected).expect("JSON"),
            "the CLI prints the one resolver's selection verbatim"
        );
        assert_eq!(envelope["body"]["provenance"]["binding_key"], key.as_str());
        assert_eq!(
            envelope["body"]["selected"]["rung"]["provider"],
            "codex-work"
        );
    }

    // The stated eligibility is what the choice is made under, and is echoed.
    let (exit, envelope) = resolve(
        realm.path(),
        &realm.leadership("lsa"),
        &["--unavailable-accounts", r#"["codex-work"]"#],
    );
    assert_eq!(exit, 0, "{envelope}");
    assert_eq!(
        envelope["body"]["selected"]["rung"]["provider"],
        "claude-personal"
    );
    assert_eq!(
        envelope["body"]["eligibility"]["unavailable_accounts"],
        serde_json::json!(["codex-work"])
    );

    // Nothing eligible is the defined block result: printed in full, refused.
    let (exit, envelope) = resolve(
        realm.path(),
        &realm.leadership("lsa"),
        &["--excluded-vendors", r#"["openai", "anthropic"]"#],
    );
    assert_eq!(exit, 1, "{envelope}");
    assert_eq!(envelope["status"], 409);
    assert_eq!(envelope["body"]["code"], "placement_blocked");
    assert!(envelope["body"]["selection"]["selected"].is_null());
    assert_eq!(
        envelope["body"]["selection"]["considered"]
            .as_array()
            .expect("considered routes")
            .len(),
        2
    );
}

#[test]
fn the_local_read_fails_closed_and_never_falls_back() {
    // No activation: an unmigrated fleet.yml is never read in its place.
    let root = tempfile::tempdir().expect("state root");
    write_private(
        &root.path().join("fleet.yml"),
        policy("0".repeat(64).as_str()).as_bytes(),
    );
    assert_refused(&resolve(root.path(), TEAM, &[]), 6, "not_found", rule::A10);

    // A schema_version 1 activation serves delivery, never leadership.
    let fleet = policy(&"1".repeat(64));
    let hash = ContentHash::of(fleet.as_bytes());
    write_private(
        &root
            .path()
            .join("fleet-history")
            .join(format!("{hash}.yml")),
        fleet.as_bytes(),
    );
    write_private(
        &root.path().join("fleet-activation.json"),
        serde_json::json!({
            "schema_version": 1,
            "policy_hash": hash.as_str(),
            "policy_schema_version": 2,
            "activated_at": "2026-09-28T00:00:00Z",
        })
        .to_string()
        .as_bytes(),
    );
    let (exit, envelope) = resolve(root.path(), TEAM, &[]);
    assert_eq!(exit, 0, "{envelope}");
    assert_eq!(envelope["body"]["provenance"]["policy_hash"], hash.as_str());
    assert_refused(
        &resolve(
            root.path(),
            &format!("leadership/{}/lsa", "1".repeat(64)),
            &[],
        ),
        6,
        "not_found",
        rule::D01,
    );

    let realm = Realm::aligned();
    // A key outside the five families, and a roster the activation does not select.
    assert_refused(
        &resolve(realm.path(), "leadership/lsa", &[]),
        2,
        "invalid_request",
        kontor_fleet::rule::V32,
    );
    assert_refused(
        &resolve(
            realm.path(),
            &format!("leadership/{}/lsa", "2".repeat(64)),
            &[],
        ),
        2,
        "invalid_request",
        rule::D02,
    );
    assert_refused(
        &resolve(
            realm.path(),
            "team/01936f5a-0000-7000-8000-000000000999/implement",
            &[],
        ),
        6,
        "not_found",
        rule::D03,
    );

    // A group-writable pointer, then a rewritten roster: each refused by its rule.
    let pointer = realm.path().join("fleet-activation.json");
    std::fs::set_permissions(&pointer, std::fs::Permissions::from_mode(0o660)).expect("mode");
    assert_refused(
        &resolve(realm.path(), TEAM, &[]),
        2,
        "invalid_request",
        rule::A02,
    );
    std::fs::set_permissions(&pointer, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let roster = realm
        .path()
        .join("core-team-history")
        .join(format!("{}.json", realm.roster));
    write_private(&roster, b"{}");
    assert_refused(
        &resolve(realm.path(), &realm.leadership("lsa"), &[]),
        2,
        "invalid_request",
        rule::C08,
    );
}

#[test]
fn the_local_read_keeps_its_tier_its_schema_and_takes_no_base_url() {
    let realm = Realm::aligned();
    let key = realm.leadership("lsa");
    let at = |tier: &str| {
        kontor(
            realm.path(),
            &[
                "--tier",
                tier,
                "fleet-policy-resolve",
                "--binding-key",
                &key,
            ],
        )
    };
    // An observer is not admitted to actionable placement selection.
    let (exit, document) = at("observer");
    assert_eq!(exit, 3, "{document}");
    assert_eq!(document["code"], "forbidden", "{document}");
    assert_eq!(document["dispatched"], false);
    // Operator is the declared tier, and admin inherits it; both answer the
    // same selection without a credential file, a daemon or a base URL.
    let (exit, operator) = at("operator");
    assert_eq!(exit, 0, "{operator}");
    let (exit, admin) = at("admin");
    assert_eq!(exit, 0, "{admin}");
    assert_eq!(operator, admin);
    assert!(!realm.path().join("credentials.json").exists());
    let (exit, document) = resolve(realm.path(), &key, &["--base-url", "http://127.0.0.1:1"]);
    assert_eq!(exit, 2, "{document}");
    assert_eq!(document["code"], "invalid_request");
    let (exit, document) = resolve(
        realm.path(),
        &key,
        &["--unavailable-accounts", r#"{"codex-work": true}"#],
    );
    assert_eq!(exit, 2, "{document}");
    assert_eq!(document["code"], "invalid_request");
    assert!(
        document["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("unavailable_accounts")),
        "{document}"
    );
}

/// Joint mode at the operator tier.
fn allocate(
    root: &Path,
    allocation: &serde_json::Value,
    extra: &[&str],
) -> (i32, serde_json::Value) {
    let allocation = allocation.to_string();
    let mut arguments = vec![
        "--tier",
        "operator",
        "fleet-policy-resolve",
        "--allocation",
        allocation.as_str(),
    ];
    arguments.extend_from_slice(extra);
    kontor(root, &arguments)
}

#[test]
fn a_joint_allocation_is_one_snapshot_through_the_shared_allocator() {
    let realm = Realm::aligned();
    let request = serde_json::json!({
        "diversity": "distinct_vendor_per_reviewer",
        "slots": [
            {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": TEAM},
            {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": realm.leadership("lsa")},
            {"slot_id": "judge", "role": "judge", "binding_key": realm.leadership("tpm")},
        ],
    });
    let (exit, envelope) = allocate(realm.path(), &request, &[]);
    assert_eq!(exit, 0, "{envelope}");
    assert_eq!(envelope["status"], 200);
    let expected = kontor_fleet_activation::allocate(
        realm.path(),
        &serde_json::from_value(request.clone()).expect("a joint request"),
    )
    .expect("the shared reader allocates");
    assert_eq!(
        envelope["body"],
        serde_json::to_value(&expected).expect("JSON"),
        "the CLI prints the one allocator's answer verbatim"
    );
    let providers: Vec<_> = envelope["body"]["slots"]
        .as_array()
        .expect("slots")
        .iter()
        .map(|slot| slot["selected"]["rung"]["provider"].clone())
        .collect();
    assert_eq!(
        providers,
        [
            serde_json::json!("codex-work"),
            serde_json::json!("claude-personal"),
            serde_json::json!("codex-work"),
        ],
        "reviewers take distinct vendors; the judge is unconstrained"
    );
    assert_eq!(
        envelope["body"]["slots"][1]["considered"][0]["excluded"],
        "vendor_held"
    );
    assert_eq!(
        envelope["body"]["slots"][1]["considered"][0]["conflicts_with"],
        "reviewer-a"
    );
    assert!(envelope["body"]["provenance"]["source_bundle_hash"].is_string());

    // Both reviewers forced onto one vendor: blocked whole, with every reason.
    let request = serde_json::json!({
        "diversity": "distinct_vendor_per_reviewer",
        "slots": [
            {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": TEAM,
             "unavailable_accounts": ["claude-personal"]},
            {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": realm.leadership("lsa"),
             "unavailable_accounts": ["claude-personal"]},
        ],
    });
    let (exit, envelope) = allocate(realm.path(), &request, &[]);
    assert_eq!(exit, 1, "{envelope}");
    assert_eq!(envelope["status"], 409);
    assert_eq!(envelope["body"]["code"], "placement_blocked");
    assert_eq!(
        envelope["body"]["allocation"]["blocked"],
        "no_distinct_reviewer_vendors"
    );
    assert!(
        envelope["body"]["allocation"]["slots"]
            .as_array()
            .expect("slots")
            .iter()
            .all(|slot| slot["selected"].is_null())
    );
}

#[test]
fn a_joint_request_names_one_mode_and_the_declared_shape() {
    let realm = Realm::aligned();
    let slots = |extra: serde_json::Value| {
        let mut slot = serde_json::json!({"slot_id": "a", "role": "reviewer", "binding_key": TEAM});
        for (name, value) in extra.as_object().expect("an object") {
            slot[name] = value.clone();
        }
        serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": [slot]})
    };

    // Both modes, then neither.
    let (exit, document) = allocate(
        realm.path(),
        &slots(serde_json::json!({})),
        &["--binding-key", TEAM],
    );
    assert_refused(&(exit, document), 2, "invalid_request", rule::J01);
    let (exit, document) = kontor(
        realm.path(),
        &["--tier", "operator", "fleet-policy-resolve"],
    );
    assert_refused(&(exit, document), 2, "invalid_request", rule::J01);
    // Top-level eligibility belongs to a single binding.
    let (exit, document) = allocate(
        realm.path(),
        &slots(serde_json::json!({})),
        &["--unavailable-accounts", r#"["codex-work"]"#],
    );
    assert_refused(&(exit, document), 2, "invalid_request", rule::J02);
    // A repeated slot id is ambiguous.
    let repeated = serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": [
        {"slot_id": "a", "role": "reviewer", "binding_key": TEAM},
        {"slot_id": "a", "role": "judge", "binding_key": TEAM},
    ]});
    assert_refused(
        &allocate(realm.path(), &repeated, &[]),
        2,
        "invalid_request",
        rule::J04,
    );

    // The declared nested schema refuses an unknown field, an unknown role,
    // another diversity rule and a missing binding before anything is read.
    for (request, property) in [
        (
            slots(serde_json::json!({"weight": 1})),
            "allocation.slots[0].weight",
        ),
        (
            slots(serde_json::json!({"role": "chair"})),
            "allocation.slots[0].role",
        ),
        (
            serde_json::json!({"diversity": "none", "slots": []}),
            "allocation.diversity",
        ),
        (
            serde_json::json!({"diversity": "distinct_vendor_per_reviewer",
                               "slots": [{"slot_id": "a", "role": "reviewer"}]}),
            "allocation.slots[0].binding_key",
        ),
    ] {
        let (exit, document) = allocate(realm.path(), &request, &[]);
        assert_eq!(exit, 2, "{document}");
        assert_eq!(document["code"], "invalid_request", "{document}");
        assert!(
            document["rule"]
                .as_str()
                .is_some_and(|rule| rule.contains(property)),
            "{property}: {document}"
        );
    }

    // An observer is refused joint mode too.
    let (exit, document) = kontor(
        realm.path(),
        &[
            "--tier",
            "observer",
            "fleet-policy-resolve",
            "--allocation",
            &slots(serde_json::json!({})).to_string(),
        ],
    );
    assert_eq!(exit, 3, "{document}");
    assert_eq!(document["code"], "forbidden");
}
