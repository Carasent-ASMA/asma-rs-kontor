use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

use kontor_core::id::{ExternalName, RoleCode, SCHEMA_VERSION, SchemaVersion};
use kontor_core::spec::CatalogRoleRef;
use kontor_fleet::rule::{A09, L01, L02, P07, V32};

use super::rule::*;
use super::*;

const TEAM: &str = "team/01936f5a-0000-7000-8000-000000000102/implement";
const COMMITTEE: &str = "committee/01991c00-0000-7000-8000-000000000001/reviewer-a";
const ADVISOR: &str = "advisor/01a02d00-0000-7000-8000-00000000ad01";

#[derive(Serialize)]
struct Envelope<'a> {
    schema_version: SchemaVersion,
    value: Roster<'a>,
}

#[derive(Serialize)]
struct Roster<'a> {
    version: SpecVersion,
    catalog_hash: ContentHash,
    seats: &'a [Seat],
}

#[derive(Clone, Serialize)]
struct Seat {
    role_slot_id: RoleSlotId,
    role: CatalogRoleRef,
    presence: &'static str,
    ad_hoc_allowed: bool,
}

fn seat(slot: &str, code: &str, title: &str, catalog: RoleCatalogId) -> Seat {
    Seat {
        role_slot_id: RoleSlotId::parse(slot).expect("slot"),
        role: CatalogRoleRef {
            catalog_id: catalog,
            catalog_revision: SpecVersion::FIRST,
            role_code: RoleCode::parse(code).expect("code"),
            standard_title: ExternalName::parse(title).expect("title"),
            custom_display_name: None,
        },
        presence: "required",
        ad_hoc_allowed: false,
    }
}

fn roster(version: SpecVersion) -> (CanonicalDocument, RoleCatalogId) {
    let catalog = RoleCatalogId::generate();
    let seats = [
        seat("lsa", "LSA", "Lead Software Architect", catalog),
        seat("tpm", "TPM", "Technical Program Manager", catalog),
    ];
    let document = CanonicalDocument::from_serializable(&Envelope {
        schema_version: SCHEMA_VERSION,
        value: Roster {
            version,
            catalog_hash: ContentHash::of(b"catalog"),
            seats: &seats,
        },
    })
    .expect("canonical roster");
    (document, catalog)
}

fn policy(lsa: &str, tpm: &str) -> String {
    format!(
        "schema_version: 2\n\
         domains:\n  codex: {{ provider: codex, accounts: [codex-work, codex-personal] }}\n  claude: {{ provider: claude, accounts: [claude-personal] }}\n\
         models:\n  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}\n  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}\n\
         chains:\n  lead:\n    - [sol@xhigh]\n    - [opus@xhigh]\n  review:\n    - [opus@xhigh]\n\
         bindings:\n  {lsa}: lead\n  {tpm}: lead\n  {TEAM}: lead\n  {COMMITTEE}: review\n  {ADVISOR}: lead\n"
    )
}

fn private(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, bytes).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("mode");
}

/// One published and activated v2 bundle in a fresh state root.
struct Bundle {
    root: tempfile::TempDir,
    policy_hash: ContentHash,
    roster: CanonicalDocument,
    manifest: BundleManifest,
    bundle_hash: ContentHash,
}

impl Bundle {
    fn path(&self) -> &Path {
        self.root.path()
    }

    fn key(&self, slot: &str) -> String {
        format!("leadership/{}/{slot}", self.roster.hash())
    }

    fn record(&self) -> FleetActivation {
        FleetActivation {
            schema_version: ACTIVATION_V2,
            source_bundle_hash: Some(self.bundle_hash.clone()),
            policy_hash: self.policy_hash.clone(),
            policy_schema_version: 2,
            core_team_revision_hash: Some(self.roster.hash().clone()),
            activated_at: "2026-09-28T00:00:00Z".to_owned(),
        }
    }

    fn write_record(&self, record: &FleetActivation) {
        private(
            &self.path().join(FLEET_ACTIVATION_FILE),
            &serde_json::to_vec_pretty(record).expect("record JSON"),
        );
    }
}

fn published_bundle() -> Bundle {
    let root = tempfile::tempdir().expect("state root");
    let (roster, catalog) = roster(SpecVersion::FIRST);
    let yaml = policy(
        &format!("leadership/{}/lsa", roster.hash()),
        &format!("leadership/{}/tpm", roster.hash()),
    );
    let policy_hash = ContentHash::of(yaml.as_bytes());
    private(&policy_path(root.path(), &policy_hash), yaml.as_bytes());
    private(
        &roster_path(root.path(), roster.hash()),
        roster.json().as_bytes(),
    );
    let manifest = BundleManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        resolver: "kontor-fleet 0.2.1".to_owned(),
        sources: BTreeMap::from([
            (
                "orchestration.yml".to_owned(),
                ContentHash::of(b"orchestration"),
            ),
            ("fleet.yml".to_owned(), policy_hash.clone()),
            (
                "teams/core-team.yml".to_owned(),
                ContentHash::of(b"core team"),
            ),
        ]),
        policy_hash: policy_hash.clone(),
        policy_schema_version: 2,
        role_catalog: RoleCatalogPin {
            catalog_id: catalog,
            version: SpecVersion::FIRST,
            content_hash: ContentHash::of(b"catalog"),
        },
        core_team_revision_hash: roster.hash().clone(),
    };
    let document = manifest.canonicalize().expect("canonical manifest");
    private(
        &manifest_path(root.path(), document.hash()),
        document.json().as_bytes(),
    );
    let bundle = Bundle {
        root,
        policy_hash,
        roster,
        manifest,
        bundle_hash: document.hash().clone(),
    };
    bundle.write_record(&bundle.record());
    bundle
}

fn refused<T: std::fmt::Debug>(result: Result<T, FleetError>) -> &'static str {
    match result {
        Err(FleetError::Invalid { rule }) => rule,
        other => panic!("expected a fail-closed refusal, got {other:?}"),
    }
}

fn first(selection: &FleetSelection) -> (String, u16, u16) {
    let route = selection.selected.as_ref().expect("a selected route");
    (route.rung.provider.0.clone(), route.step, route.sub_step)
}

#[test]
fn a_verified_v2_bundle_resolves_every_seat_class_from_one_policy() {
    let bundle = published_bundle();
    let activated = load(bundle.path()).expect("the bundle verifies");
    assert_eq!(activated.record, bundle.record());
    let verified = activated.bundle.as_ref().expect("a v2 bundle");
    assert_eq!(verified.manifest, bundle.manifest);
    assert_eq!(verified.roster, bundle.roster);

    for key in [
        bundle.key("lsa"),
        bundle.key("tpm"),
        TEAM.to_owned(),
        ADVISOR.to_owned(),
    ] {
        let selection = resolve(bundle.path(), &key, &Eligibility::default()).expect("bound");
        assert_eq!(
            selection.provenance.policy_hash, bundle.policy_hash,
            "{key}"
        );
        assert_eq!(selection.provenance.binding_key, key);
        assert_eq!(first(&selection), ("codex-work".to_owned(), 1, 1), "{key}");
    }
    let reviewer = resolve(bundle.path(), COMMITTEE, &Eligibility::default()).expect("bound");
    assert_eq!(reviewer.provenance.chain, "review");

    let degraded = resolve(
        bundle.path(),
        &bundle.key("lsa"),
        &Eligibility {
            unavailable_accounts: BTreeSet::from(["codex-work".to_owned()]),
            excluded_vendors: BTreeSet::new(),
        },
    )
    .expect("bound");
    assert_eq!(first(&degraded), ("codex-personal".to_owned(), 1, 2));
    let blocked = resolve(
        bundle.path(),
        &bundle.key("lsa"),
        &Eligibility {
            unavailable_accounts: BTreeSet::new(),
            excluded_vendors: BTreeSet::from(["openai".to_owned(), "anthropic".to_owned()]),
        },
    )
    .expect("the defined block result");
    assert!(blocked.selected.is_none());
}

#[test]
fn a_leadership_binding_the_activation_cannot_cover_fails_closed() {
    let bundle = published_bundle();
    let other = roster(SpecVersion::FIRST.next().expect("next")).0;
    for (key, rule) in [
        (format!("leadership/{}/lsa", other.hash()), D02),
        (bundle.key("qa"), L02),
        (format!("leadership/{}/LSA", bundle.roster.hash()), V32),
        ("leadership/lsa".to_owned(), V32),
        ("core/lsa".to_owned(), V32),
        ("team/unbound/slot".to_owned(), D03),
    ] {
        assert_eq!(
            refused(resolve(bundle.path(), &key, &Eligibility::default())),
            rule,
            "{key}"
        );
    }
}

#[test]
fn a_v1_activation_resolves_delivery_and_never_leadership() {
    let bundle = published_bundle();
    let v1 = FleetActivation {
        schema_version: ACTIVATION_V1,
        source_bundle_hash: None,
        policy_hash: bundle.policy_hash.clone(),
        policy_schema_version: 2,
        core_team_revision_hash: None,
        activated_at: "2026-09-27T00:00:00Z".to_owned(),
    };
    bundle.write_record(&v1);
    let record_bytes =
        std::fs::read_to_string(bundle.path().join(FLEET_ACTIVATION_FILE)).expect("record");
    assert!(
        !record_bytes.contains("source_bundle_hash") && !record_bytes.contains("core_team"),
        "a v1 record keeps its exact v1 shape: {record_bytes}"
    );
    let activated = load(bundle.path()).expect("a v1 activation verifies");
    assert!(activated.bundle.is_none());
    assert!(resolve(bundle.path(), TEAM, &Eligibility::default()).is_ok());
    assert_eq!(
        refused(resolve(
            bundle.path(),
            &bundle.key("lsa"),
            &Eligibility::default()
        )),
        D01,
        "a v1 record cannot claim aligned direct leadership coverage"
    );
}

#[test]
fn a_missing_named_artifact_fails_closed() {
    type Target = fn(&Bundle) -> PathBuf;
    let cases: [(&str, Target, &str); 4] = [
        ("pointer", |b| b.path().join(FLEET_ACTIVATION_FILE), A10),
        ("manifest", |b| manifest_path(b.path(), &b.bundle_hash), M07),
        ("policy", |b| policy_path(b.path(), &b.policy_hash), A08),
        ("roster", |b| roster_path(b.path(), b.roster.hash()), C07),
    ];
    for (name, path, rule) in cases {
        let bundle = published_bundle();
        std::fs::remove_file(path(&bundle)).expect("remove");
        assert_eq!(refused(load(bundle.path())), rule, "{name}");
    }
}

#[test]
fn a_record_that_disagrees_with_its_manifest_fails_closed() {
    let other = roster(SpecVersion::FIRST.next().expect("next")).0;
    for edit in [
        |record: &mut FleetActivation| record.policy_hash = ContentHash::of(b"other policy"),
        |record: &mut FleetActivation| record.policy_schema_version = 1,
        |record: &mut FleetActivation| {
            record.core_team_revision_hash = Some(ContentHash::of(b"other roster"));
        },
    ] {
        let bundle = published_bundle();
        let mut record = bundle.record();
        edit(&mut record);
        bundle.write_record(&record);
        assert_eq!(refused(load(bundle.path())), A11);
    }
    let bundle = published_bundle();
    let mut record = bundle.record();
    record.core_team_revision_hash = Some(other.hash().clone());
    bundle.write_record(&record);
    assert_eq!(refused(load(bundle.path())), A11);
}

#[test]
fn a_malformed_or_mixed_record_fails_closed() {
    let bundle = published_bundle();
    let mut v1_with_bundle = bundle.record();
    v1_with_bundle.schema_version = ACTIVATION_V1;
    let mut v2_without_roster = bundle.record();
    v2_without_roster.core_team_revision_hash = None;
    let mut unknown_version = bundle.record();
    unknown_version.schema_version = 3;
    for record in [v1_with_bundle, v2_without_roster, unknown_version] {
        bundle.write_record(&record);
        assert_eq!(refused(load(bundle.path())), A07, "{record:?}");
    }
    private(
        &bundle.path().join(FLEET_ACTIVATION_FILE),
        br#"{"schema_version": 2, "policy_hash": "x", "extra": true}"#,
    );
    assert_eq!(refused(load(bundle.path())), A07);
}

#[test]
fn a_rewritten_or_non_canonical_artifact_fails_closed() {
    // Policy bytes that are not the activated ones.
    let bundle = published_bundle();
    private(
        &policy_path(bundle.path(), &bundle.policy_hash),
        policy("team/a/b", "team/c/d").as_bytes(),
    );
    assert_eq!(refused(load(bundle.path())), P07);

    // The record's policy schema is not the one the bytes validate under.
    let bundle = published_bundle();
    let mut record = bundle.record();
    record.policy_schema_version = 1;
    let mut manifest = bundle.manifest.clone();
    manifest.policy_schema_version = 1;
    let document = manifest.canonicalize().expect("canonical");
    private(
        &manifest_path(bundle.path(), document.hash()),
        document.json().as_bytes(),
    );
    record.source_bundle_hash = Some(document.hash().clone());
    bundle.write_record(&record);
    assert_eq!(refused(load(bundle.path())), A09);

    // A manifest stored pretty-printed is not its canonical bytes.
    let bundle = published_bundle();
    private(
        &manifest_path(bundle.path(), &bundle.bundle_hash),
        &serde_json::to_vec_pretty(&bundle.manifest).expect("pretty"),
    );
    assert_eq!(refused(load(bundle.path())), M08);

    // A roster rewritten under its address.
    let bundle = published_bundle();
    let (other, _) = roster(SpecVersion::FIRST.next().expect("next"));
    private(
        &roster_path(bundle.path(), bundle.roster.hash()),
        other.json().as_bytes(),
    );
    assert_eq!(refused(load(bundle.path())), C08);
}

#[test]
fn a_canonical_artifact_of_the_wrong_kind_fails_closed() {
    // A canonical document that is not a Core Team revision, published and
    // named as the roster, is refused before any leadership key exists.
    let bundle = published_bundle();
    let not_a_roster = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "value": "lsa",
    }))
    .expect("canonical");
    private(
        &roster_path(bundle.path(), not_a_roster.hash()),
        not_a_roster.json().as_bytes(),
    );
    let mut manifest = bundle.manifest.clone();
    manifest.core_team_revision_hash = not_a_roster.hash().clone();
    let document = manifest.canonicalize().expect("canonical");
    private(
        &manifest_path(bundle.path(), document.hash()),
        document.json().as_bytes(),
    );
    let mut record = bundle.record();
    record.source_bundle_hash = Some(document.hash().clone());
    record.core_team_revision_hash = Some(not_a_roster.hash().clone());
    bundle.write_record(&record);
    assert_eq!(refused(load(bundle.path())), L01);

    // A manifest with a field this build does not read.
    let bundle = published_bundle();
    let mut value = serde_json::to_value(&bundle.manifest).expect("value");
    value["activated"] = serde_json::Value::Bool(true);
    let extended = CanonicalDocument::from_value(&value).expect("canonical");
    private(
        &manifest_path(bundle.path(), extended.hash()),
        extended.json().as_bytes(),
    );
    let mut record = bundle.record();
    record.source_bundle_hash = Some(extended.hash().clone());
    bundle.write_record(&record);
    assert_eq!(refused(load(bundle.path())), M09);
}

#[test]
fn a_manifest_pinning_another_role_catalog_fails_closed() {
    // The same verification activation runs before it writes the record.
    let bundle = published_bundle();
    let verified = verify_bundle(bundle.path(), &bundle.bundle_hash).expect("verifies");
    assert_eq!(verified.roster, bundle.roster);
    assert_eq!(verified.policy.hash(), &bundle.policy_hash);

    for pin in [
        RoleCatalogPin {
            content_hash: ContentHash::of(b"another catalog"),
            ..bundle.manifest.role_catalog.clone()
        },
        RoleCatalogPin {
            catalog_id: RoleCatalogId::generate(),
            ..bundle.manifest.role_catalog.clone()
        },
        RoleCatalogPin {
            version: SpecVersion::FIRST.next().expect("second"),
            ..bundle.manifest.role_catalog.clone()
        },
    ] {
        let mut manifest = bundle.manifest.clone();
        manifest.role_catalog = pin;
        let document = manifest.canonicalize().expect("canonical");
        private(
            &manifest_path(bundle.path(), document.hash()),
            document.json().as_bytes(),
        );
        assert_eq!(refused(verify_bundle(bundle.path(), document.hash())), M10);
        let mut record = bundle.record();
        record.source_bundle_hash = Some(document.hash().clone());
        bundle.write_record(&record);
        assert_eq!(refused(load(bundle.path())), M10);
    }
}

#[test]
fn every_named_file_is_read_under_its_own_guard() {
    type Target = fn(&Bundle) -> PathBuf;
    let targets: [(Target, &str, &str, &str, &str); 4] = [
        (|b| b.path().join(FLEET_ACTIVATION_FILE), A01, A02, A04, A06),
        (
            |b| manifest_path(b.path(), &b.bundle_hash),
            M01,
            M02,
            M04,
            M06,
        ),
        (
            |b| policy_path(b.path(), &b.policy_hash),
            P01,
            P02,
            P04,
            P06,
        ),
        (
            |b| roster_path(b.path(), b.roster.hash()),
            C01,
            C02,
            C04,
            C06,
        ),
    ];
    for (target, symlink, writable, oversized, utf8) in targets {
        let bundle = published_bundle();
        let path = target(&bundle);
        let good = std::fs::read(&path).expect("artifact");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).expect("mode");
        assert_eq!(refused(load(bundle.path())), writable, "{}", path.display());

        private(&path, &[0xff, 0xfe]);
        assert_eq!(refused(load(bundle.path())), utf8, "{}", path.display());

        private(&path, &vec![b' '; 300 * 1024]);
        assert_eq!(
            refused(load(bundle.path())),
            oversized,
            "{}",
            path.display()
        );

        let real = bundle.path().join("real-target");
        private(&real, &good);
        std::fs::remove_file(&path).expect("remove");
        std::os::unix::fs::symlink(&real, &path).expect("symlink");
        assert_eq!(refused(load(bundle.path())), symlink, "{}", path.display());
    }
}

fn joint(value: serde_json::Value) -> JointAllocationRequest {
    serde_json::from_value(value).expect("a joint request")
}

#[test]
fn a_joint_allocation_resolves_every_slot_from_one_snapshot() {
    let bundle = published_bundle();
    let activated = load(bundle.path()).expect("the bundle verifies");
    let request = joint(serde_json::json!({
        "diversity": "distinct_vendor_per_reviewer",
        "slots": [
            {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": COMMITTEE},
            {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": TEAM},
            {"slot_id": "lead", "role": "judge", "binding_key": bundle.key("lsa"),
             "unavailable_accounts": ["codex-work"]},
        ],
    }));
    let selection = activated.allocate(&request).expect("allocates");
    assert!(selection.is_complete());
    assert_eq!(
        selection.provenance,
        ActivationProvenance {
            policy_hash: bundle.policy_hash.clone(),
            policy_schema_version: 2,
            source_bundle_hash: Some(bundle.bundle_hash.clone()),
            core_team_revision_hash: Some(bundle.roster.hash().clone()),
        }
    );
    let picked: Vec<(&str, &str, &str)> = selection
        .slots
        .iter()
        .map(|slot| {
            let selected = slot.allocation.selected.as_ref().expect("selected");
            (
                slot.binding_key.as_str(),
                slot.chain.as_str(),
                selected.rung.provider.0.as_str(),
            )
        })
        .collect();
    let lsa = bundle.key("lsa");
    assert_eq!(
        picked,
        [
            (COMMITTEE, "review", "claude-personal"),
            (TEAM, "lead", "codex-work"),
            (lsa.as_str(), "lead", "codex-personal"),
        ]
    );
    assert_eq!(
        selection.slots[2]
            .allocation
            .eligibility
            .unavailable_accounts,
        BTreeSet::from(["codex-work".to_owned()])
    );
    // The same snapshot through the one-call path: equal answer.
    assert_eq!(
        allocate(bundle.path(), &request).expect("allocates"),
        selection
    );
    let receipt = serde_json::to_value(&selection).expect("JSON");
    assert_eq!(receipt["slots"][0]["selected"]["vendor"], "anthropic");
    assert_eq!(receipt["slots"][1]["selected"]["step"], 1);
    assert_eq!(
        receipt["provenance"]["policy_hash"],
        bundle.policy_hash.as_str()
    );
}

#[test]
fn a_joint_allocation_is_whole_or_blocked_with_every_reason() {
    let bundle = published_bundle();
    // Reviewer A may not use either OpenAI account, so it takes Anthropic;
    // reviewer B's only route is Anthropic too.
    let request = joint(serde_json::json!({
        "diversity": "distinct_vendor_per_reviewer",
        "slots": [
            {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": TEAM,
             "unavailable_accounts": ["codex-work", "codex-personal"]},
            {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": COMMITTEE},
        ],
    }));
    let selection = allocate(bundle.path(), &request).expect("a defined block result");
    assert_eq!(
        selection.blocked,
        Some(AllocationFailure::NoDistinctReviewerVendors)
    );
    assert!(
        selection
            .slots
            .iter()
            .all(|slot| slot.allocation.selected.is_none())
    );
    assert_eq!(
        selection.slots[0].allocation.considered[0].excluded,
        Some(kontor_fleet::AllocationExclusion::AccountUnavailableNow)
    );
    assert_eq!(selection.provenance.policy_hash, bundle.policy_hash);
}

#[test]
fn a_joint_request_that_is_empty_repeated_or_unresolvable_fails_closed() {
    let bundle = published_bundle();
    let refuse = |value: serde_json::Value| refused(allocate(bundle.path(), &joint(value)));
    assert_eq!(
        refuse(serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": []})),
        J03
    );
    assert_eq!(
        refuse(
            serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": [
                {"slot_id": "a", "role": "reviewer", "binding_key": TEAM},
                {"slot_id": "a", "role": "judge", "binding_key": COMMITTEE},
            ]})
        ),
        J04
    );
    // One slot that cannot be resolved refuses every slot.
    for (key, rule) in [
        ("leadership/lsa".to_owned(), V32),
        (
            format!("leadership/{}/lsa", ContentHash::of(b"other roster")),
            D02,
        ),
        (
            "team/01936f5a-0000-7000-8000-000000000999/implement".to_owned(),
            D03,
        ),
    ] {
        assert_eq!(
            refuse(
                serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": [
                    {"slot_id": "a", "role": "reviewer", "binding_key": TEAM},
                    {"slot_id": "b", "role": "reviewer", "binding_key": key},
                ]})
            ),
            rule,
            "{key}"
        );
    }
    // The request shape is closed.
    assert!(
        serde_json::from_value::<JointAllocationRequest>(serde_json::json!({
            "diversity": "distinct_vendor_per_reviewer",
            "slots": [{"slot_id": "a", "role": "reviewer", "binding_key": TEAM, "weight": 1}],
        }))
        .is_err()
    );
    // An unverifiable activation refuses before any slot is resolved.
    private(
        &policy_path(bundle.path(), &bundle.policy_hash),
        policy("leadership/x/lsa", "leadership/x/tpm").as_bytes(),
    );
    assert_eq!(
        refuse(
            serde_json::json!({"diversity": "distinct_vendor_per_reviewer", "slots": [
                {"slot_id": "a", "role": "reviewer", "binding_key": TEAM},
            ]})
        ),
        P07
    );
}

#[test]
fn every_rule_string_is_stable() {
    assert_eq!(
        A07,
        "fleet-activation.json is not a schema_version 1 or 2 activation record"
    );
    assert_eq!(A10, "no fleet activation record exists");
    assert_eq!(
        A11,
        "the activation record and its bundle manifest disagree"
    );
    assert_eq!(M07, "the activated bundle manifest is not published");
    assert_eq!(
        M10,
        "a bundle manifest pins another role catalog than its Core Team revision was resolved against"
    );
    assert_eq!(C07, "the selected Core Team revision is not published");
    assert_eq!(
        D01,
        "a schema_version 1 activation selects no Core Team roster, so it resolves no leadership binding"
    );
    assert_eq!(
        D02,
        "the leadership binding names a Core Team revision the activation does not select"
    );
    assert_eq!(D03, "the activated policy binds no chain to this key");
    assert_eq!(J01, "name exactly one of binding_key and allocation");
    assert_eq!(J04, "a joint allocation names each slot_id once");
}
