//! Golden, determinism, precedence, provenance and immutability cases.
//!
//! The mutants this suite exists to kill:
//!
//! * swapping, reordering or deriving the six layer ranks from anything other
//!   than the approved architecture order;
//! * merging in caller collection order instead of `(rank, source key)`;
//! * concatenating arrays, deleting on `null`, or merging a type change instead
//!   of replacing it;
//! * recording provenance for anything but the winning source of each leaf, or
//!   leaving a losing entry behind after a replacement;
//! * letting preview and start diverge;
//! * letting a started snapshot observe a later change to its sources;
//! * accepting a duplicate `(layer, source key)` and relying on collection order
//!   to break the tie — or, the other way round, rejecting the same key in two
//!   different layers, which is legal and carries its own precedence rank.

use std::collections::BTreeSet;

use kontor_context::{
    ContextLayer, ContextSource, ProvenanceEntry, ReferenceInputs, ResolutionRequest,
    ResolvedContextPack, RunBinding, WorkspaceRef, preview, start_run,
};
use kontor_core::DomainError;
use kontor_core::id::{
    BoundedText, ContextPackId, ExternalId, ExternalName, RealmId, SchemaVersion, SpecVersion,
    Timestamp, parse_utc_timestamp,
};
use serde_json::{Value, json};

const SOURCES: &str = include_str!("fixtures/resolution/sources.json");
const REFERENCES: &str = include_str!("fixtures/resolution/references.json");
const EXPECTED_PACK: &str = include_str!("fixtures/resolution/expected_pack.json");

const REALM: &str = "0192f0a1-0000-7000-8000-00000000f001";
const PACK_ID: &str = "0192f0a1-0000-7000-8000-00000000c001";
const RUN_ID: &str = "0192f0a1-0000-7000-8000-0000000000a1";

fn realm() -> RealmId {
    RealmId::parse(REALM).expect("fixture realm id is canonical")
}

fn golden_sources() -> Vec<ContextSource> {
    serde_json::from_str(SOURCES).expect("golden sources deserialize")
}

fn golden_references() -> ReferenceInputs {
    serde_json::from_str(REFERENCES).expect("golden references deserialize")
}

fn resolve(sources: &[ContextSource], references: &ReferenceInputs) -> ResolvedContextPack {
    preview(&ResolutionRequest {
        realm_id: realm(),
        sources,
        references,
    })
    .expect("golden fixture resolves")
}

fn golden_pack() -> ResolvedContextPack {
    resolve(&golden_sources(), &golden_references())
}

fn source(layer: ContextLayer, source_id: &str, revision: u32, content: Value) -> ContextSource {
    ContextSource {
        schema_version: SchemaVersion::parse(1).expect("schema version 1"),
        realm_id: realm(),
        layer,
        source_id: source_id.to_owned(),
        revision: SpecVersion::parse(revision).expect("positive revision"),
        restricted_references: Vec::new(),
        redactions: Vec::new(),
        content,
    }
}

fn binding() -> RunBinding {
    RunBinding {
        agent_run_id: kontor_core::id::AgentRunId::parse(RUN_ID).expect("canonical run id"),
        workspace: WorkspaceRef {
            root: BoundedText::parse("/work/kontor").expect("bounded root"),
            branch: ExternalName::parse("feat/context-packs").expect("bounded branch"),
            baseline_commit: ExternalId::parse("45126a6").expect("bounded commit"),
        },
        started_at: at("2026-08-10T09:00:00Z"),
    }
}

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

/// Every leaf pointer of a resolved value, using the same leaf rule as the
/// resolver: an object recurses, an empty object and everything else is a leaf.
fn leaf_pointers(value: &Value, path: &mut String, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(members) if !members.is_empty() => {
            for (key, member) in members {
                let restore = path.len();
                path.push('/');
                path.push_str(&key.replace('~', "~0").replace('/', "~1"));
                leaf_pointers(member, path, out);
                path.truncate(restore);
            }
        }
        _ => {
            out.insert(path.clone());
        }
    }
}

/// Every permutation of `items`, smallest useful helper that does the job.
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(index);
        for mut tail in permutations(&rest) {
            tail.insert(0, head.clone());
            out.push(tail);
        }
    }
    out
}

#[test]
fn golden_resolution_matches_canonical_snapshot() {
    let pack = golden_pack();
    assert_eq!(
        pack.document().json(),
        EXPECTED_PACK.trim_end(),
        "the canonical pack document must match the reviewed golden byte for byte"
    );
    // The digest is the core document digest of exactly those bytes.
    assert_eq!(
        pack.hash().as_str(),
        kontor_core::id::ContentHash::of(EXPECTED_PACK.trim_end().as_bytes()).as_str()
    );
    assert_eq!(pack.schema_version(), SchemaVersion::parse(1).expect("v1"));
    assert_eq!(pack.realm_id(), realm());
}

#[test]
fn same_inputs_in_any_collection_order_have_the_same_hash() {
    let sources = golden_sources();
    let references = golden_references();
    let expected = resolve(&sources, &references);

    let mut reversed = sources.clone();
    reversed.reverse();
    let mut rotated = sources.clone();
    rotated.rotate_left(3);

    for candidate in [reversed, rotated] {
        let actual = resolve(&candidate, &references);
        assert_eq!(actual.hash(), expected.hash());
        assert_eq!(actual.document().json(), expected.document().json());
    }
}

#[test]
fn source_permutations_preserve_resolution_and_hash() {
    let sources = golden_sources();
    let references = golden_references();
    let expected = resolve(&sources, &references);
    let orders = permutations(&sources);
    assert_eq!(orders.len(), 720, "all six sources must be permuted");

    for order in orders {
        let actual = resolve(&order, &references);
        assert_eq!(
            actual.document().json(),
            expected.document().json(),
            "collection order must not reach the resolved pack"
        );
        assert_eq!(actual.hash(), expected.hash());
        assert_eq!(actual.provenance(), expected.provenance());
        assert_eq!(actual.redactions(), expected.redactions());
    }
}

#[test]
fn all_six_layers_follow_architecture_precedence() {
    assert_eq!(
        ContextLayer::ALL,
        [
            ContextLayer::GlobalProfile,
            ContextLayer::ProjectProfile,
            ContextLayer::Scope,
            ContextLayer::TeamRoleProfile,
            ContextLayer::TaskAdditions,
            ContextLayer::RunOverride,
        ]
        .as_slice(),
        "the layer list is the approved architecture order"
    );
    for pair in ContextLayer::ALL.windows(2) {
        assert!(
            pair[0].rank() < pair[1].rank(),
            "{:?} must rank below {:?}",
            pair[0],
            pair[1]
        );
    }

    let pack = golden_pack();
    // Every layer contributed, and the strongest one owns the conflicting path.
    for layer in ContextLayer::ALL {
        let key = serde_json::to_value(layer).expect("layer spelling");
        let key = key.as_str().expect("layer spelling is a string");
        assert_eq!(
            pack.resolved().pointer(&format!("/layers/{key}")),
            Some(&json!(true)),
            "layer {key} must have contributed"
        );
    }
    assert_eq!(
        pack.resolved().pointer("/winner"),
        Some(&json!("run_override"))
    );
}

#[test]
fn swapping_any_adjacent_precedence_rank_changes_the_expected_golden() {
    // Each adjacent pair is resolved on its own with one conflicting path and
    // one layer sentinel, so swapping the ranks of any two neighbours flips a
    // winner that this test names explicitly.
    for pair in ContextLayer::ALL.windows(2) {
        let (weaker, stronger) = (pair[0], pair[1]);
        let sources = vec![
            source(
                stronger,
                "b.stronger",
                1,
                json!({ "contested": "stronger", "only_stronger": true }),
            ),
            source(
                weaker,
                "a.weaker",
                1,
                json!({ "contested": "weaker", "only_weaker": true }),
            ),
        ];
        let pack = resolve(&sources, &ReferenceInputs::new());
        assert_eq!(
            pack.resolved().pointer("/contested"),
            Some(&json!("stronger")),
            "{stronger:?} must beat {weaker:?}"
        );
        let winner = pack
            .provenance()
            .iter()
            .find(|entry| entry.path.as_str() == "/contested")
            .expect("the contested path has provenance");
        assert_eq!(winner.layer, stronger);
        assert_eq!(winner.source_id, "b.stronger");
        // The loser still contributes its own uncontested path.
        assert_eq!(pack.resolved().pointer("/only_weaker"), Some(&json!(true)));
    }

    // And in the golden, the strongest layer owns the contested path even though
    // its source key sorts last.
    let pack = golden_pack();
    let winner = pack
        .provenance()
        .iter()
        .find(|entry| entry.path.as_str() == "/winner")
        .expect("the contested path has provenance");
    assert_eq!(winner.layer, ContextLayer::RunOverride);
    assert_eq!(winner.source_id, "run.override");
    assert_eq!(winner.revision, SpecVersion::parse(6).expect("revision 6"));
}

#[test]
fn later_layer_replaces_scalar_array_and_type_change() {
    let pack = golden_pack();
    // Array: replaced as a whole, never concatenated.
    assert_eq!(
        pack.resolved().pointer("/allowed_tools"),
        Some(&json!(["read", "write"]))
    );
    // Type change: a string is replaced by an object.
    assert_eq!(
        pack.resolved().pointer("/goal"),
        Some(&json!({ "statement": "land the context pack", "owner": "implementer" }))
    );
    // Explicit null replaces the whole earlier object; it does not delete.
    assert_eq!(pack.resolved().pointer("/budget"), Some(&Value::Null));
    assert!(
        pack.resolved()
            .as_object()
            .expect("pack is an object")
            .contains_key("budget"),
        "null is a replacement, not a deletion"
    );
    // The losing object's descendants leave no provenance behind.
    assert!(
        !pack
            .provenance()
            .iter()
            .any(|entry| entry.path.as_str().starts_with("/budget/")),
        "a replaced subtree keeps no losing provenance"
    );

    // Empty array and empty object follow the same rules.
    let sources = vec![
        source(
            ContextLayer::GlobalProfile,
            "global.profile",
            1,
            json!({ "list": [1, 2], "object": { "kept": true } }),
        ),
        source(
            ContextLayer::RunOverride,
            "run.override",
            1,
            json!({ "list": [], "object": {} }),
        ),
    ];
    let pack = resolve(&sources, &ReferenceInputs::new());
    assert_eq!(pack.resolved().pointer("/list"), Some(&json!([])));
    assert_eq!(
        pack.resolved().pointer("/object"),
        Some(&json!({ "kept": true })),
        "an empty object merges member by member and therefore changes nothing"
    );
}

#[test]
fn object_members_merge_recursively() {
    let pack = golden_pack();
    assert_eq!(
        pack.resolved().pointer("/nested/shared"),
        Some(&json!({
            "global": "g",
            "project": "p",
            "scope": "s",
            "team": "t",
            "task": "a",
            "run": "r"
        })),
        "every layer's member of the same object survives"
    );
    // Each member keeps its own winning source.
    for (member, expected) in [
        ("global", ContextLayer::GlobalProfile),
        ("project", ContextLayer::ProjectProfile),
        ("scope", ContextLayer::Scope),
        ("team", ContextLayer::TeamRoleProfile),
        ("task", ContextLayer::TaskAdditions),
        ("run", ContextLayer::RunOverride),
    ] {
        let path = format!("/nested/shared/{member}");
        let entry = pack
            .provenance()
            .iter()
            .find(|entry| entry.path.as_str() == path)
            .unwrap_or_else(|| panic!("{path} has provenance"));
        assert_eq!(entry.layer, expected);
    }
}

#[test]
fn unique_highest_layer_wins_each_conflicting_path() {
    let pack = golden_pack();
    for (path, layer, source_id) in [
        ("/winner", ContextLayer::RunOverride, "run.override"),
        ("/budget", ContextLayer::RunOverride, "run.override"),
        (
            "/allowed_tools",
            ContextLayer::ProjectProfile,
            "project.profile",
        ),
        (
            "/goal/statement",
            ContextLayer::TeamRoleProfile,
            "team.role.implementer",
        ),
    ] {
        let winners: Vec<&ProvenanceEntry> = pack
            .provenance()
            .iter()
            .filter(|entry| entry.path.as_str() == path)
            .collect();
        assert_eq!(winners.len(), 1, "{path} must have exactly one winner");
        assert_eq!(winners[0].layer, layer);
        assert_eq!(winners[0].source_id, source_id);
    }
}

#[test]
fn provenance_names_the_winning_source_for_every_leaf() {
    let pack = golden_pack();
    let mut expected = BTreeSet::new();
    leaf_pointers(pack.resolved(), &mut String::new(), &mut expected);
    let recorded: BTreeSet<String> = pack
        .provenance()
        .iter()
        .map(|entry| entry.path.as_str().to_owned())
        .collect();
    assert_eq!(
        recorded, expected,
        "provenance covers every resolved leaf and nothing else"
    );
    assert!(!expected.is_empty(), "the golden pack has leaves");

    // Every entry names a source that actually exists at that revision.
    let sources = golden_sources();
    for entry in pack.provenance() {
        let declared = sources
            .iter()
            .find(|source| source.source_id == entry.source_id)
            .unwrap_or_else(|| panic!("{} names a declared source", entry.path.as_str()));
        assert_eq!(declared.layer, entry.layer);
        assert_eq!(declared.revision, entry.revision);
    }
    // The reference-resolved leaves belong to the source that declared them.
    for path in ["/decisions/adr_ref/adr", "/decisions/adr_ref/title"] {
        let entry = pack
            .provenance()
            .iter()
            .find(|entry| entry.path.as_str() == path)
            .unwrap_or_else(|| panic!("{path} has provenance"));
        assert_eq!(entry.source_id, "scope.mini-project");
    }
}

#[test]
fn preview_and_started_snapshot_have_identical_pack_hash_and_provenance() {
    let sources = golden_sources();
    let references = golden_references();
    let request = ResolutionRequest {
        realm_id: realm(),
        sources: &sources,
        references: &references,
    };
    let previewed = preview(&request).expect("preview resolves");
    let snapshot = start_run(
        &request,
        ContextPackId::parse(PACK_ID).expect("canonical pack id"),
        binding(),
    )
    .expect("start resolves");

    assert_eq!(snapshot.hash(), previewed.hash());
    assert_eq!(
        snapshot.pack().document().json(),
        previewed.document().json()
    );
    assert_eq!(snapshot.pack().provenance(), previewed.provenance());
    assert_eq!(snapshot.pack().redactions(), previewed.redactions());
    assert_eq!(snapshot.pack().resolved(), previewed.resolved());
    // The run binding is the only thing start adds, and it is not in the digest.
    assert_eq!(
        snapshot.context_pack_id(),
        ContextPackId::parse(PACK_ID).expect("canonical pack id")
    );
    assert_eq!(snapshot.run(), &binding());
}

#[test]
fn started_snapshot_is_unchanged_after_sources_are_mutated_and_reresolved() {
    let mut sources = golden_sources();
    let references = golden_references();
    let snapshot = start_run(
        &ResolutionRequest {
            realm_id: realm(),
            sources: &sources,
            references: &references,
        },
        ContextPackId::parse(PACK_ID).expect("canonical pack id"),
        binding(),
    )
    .expect("start resolves");

    let frozen_json = snapshot.pack().document().json().to_owned();
    let frozen_hash = snapshot.hash().clone();
    let frozen_provenance: Vec<ProvenanceEntry> = snapshot.pack().provenance().to_vec();
    let frozen_resolved = snapshot.pack().resolved().clone();

    // Every source changes underneath the started run.
    for source in &mut sources {
        source.content["winner"] = json!("mutated after start");
        source.content["added_after_start"] = json!(true);
    }
    let reresolved = resolve(&sources, &references);

    assert_ne!(
        reresolved.hash(),
        &frozen_hash,
        "the changed sources must produce a different pack"
    );
    assert_eq!(
        reresolved.resolved().pointer("/added_after_start"),
        Some(&json!(true))
    );

    assert_eq!(snapshot.pack().document().json(), frozen_json);
    assert_eq!(snapshot.hash(), &frozen_hash);
    assert_eq!(snapshot.pack().provenance(), frozen_provenance.as_slice());
    assert_eq!(snapshot.pack().resolved(), &frozen_resolved);
    assert_eq!(
        snapshot.pack().resolved().pointer("/winner"),
        Some(&json!("run_override"))
    );
    assert!(
        snapshot
            .pack()
            .resolved()
            .pointer("/added_after_start")
            .is_none(),
        "a started pack cannot observe a later source change"
    );
}

#[test]
fn duplicate_source_key_is_rejected() {
    // Uniqueness is per `(layer, source key)`: two entries at the same rank would
    // need collection order to break the tie, so they reject.
    for layer in ContextLayer::ALL {
        let sources = vec![
            source(*layer, "same.key", 1, json!({ "a": 1 })),
            source(*layer, "same.key", 2, json!({ "a": 2 })),
        ];
        let error = preview(&ResolutionRequest {
            realm_id: realm(),
            sources: &sources,
            references: &ReferenceInputs::new(),
        })
        .expect_err("a duplicate source key inside one layer rejects");
        assert!(
            matches!(
                error,
                DomainError::Invalid {
                    subject: "ContextSource",
                    ..
                }
            ),
            "{layer:?} must reject a repeated key, got {error:?}"
        );
    }
}

#[test]
fn the_same_source_key_in_different_layers_is_legal_and_resolved_by_precedence() {
    // One profile key legitimately contributes at several ranks. That is not a
    // duplicate: `(layer, key)` still totally orders the sources.
    let sources = vec![
        source(
            ContextLayer::RunOverride,
            "shared.key",
            2,
            json!({ "contested": "run_override", "only_run": true }),
        ),
        source(
            ContextLayer::GlobalProfile,
            "shared.key",
            1,
            json!({ "contested": "global_profile", "only_global": true }),
        ),
        source(
            ContextLayer::Scope,
            "shared.key",
            3,
            json!({ "contested": "scope", "only_scope": true }),
        ),
    ];
    let pack = resolve(&sources, &ReferenceInputs::new());

    // Precedence, not collection order and not the shared key, decides.
    assert_eq!(
        pack.resolved().pointer("/contested"),
        Some(&json!("run_override"))
    );
    // Every layer's uncontested contribution survives.
    for path in ["/only_global", "/only_scope", "/only_run"] {
        assert_eq!(pack.resolved().pointer(path), Some(&json!(true)), "{path}");
    }

    // Provenance still names exactly one winner, at the right layer and revision.
    let winners: Vec<&ProvenanceEntry> = pack
        .provenance()
        .iter()
        .filter(|entry| entry.path.as_str() == "/contested")
        .collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(winners[0].layer, ContextLayer::RunOverride);
    assert_eq!(winners[0].source_id, "shared.key");
    assert_eq!(
        winners[0].revision,
        SpecVersion::parse(2).expect("revision 2")
    );
    // Each layer keeps its own revision on the leaf it owns.
    for (path, layer, revision) in [
        ("/only_global", ContextLayer::GlobalProfile, 1),
        ("/only_scope", ContextLayer::Scope, 3),
        ("/only_run", ContextLayer::RunOverride, 2),
    ] {
        let entry = pack
            .provenance()
            .iter()
            .find(|entry| entry.path.as_str() == path)
            .unwrap_or_else(|| panic!("{path} has provenance"));
        assert_eq!(entry.layer, layer);
        assert_eq!(
            entry.revision,
            SpecVersion::parse(revision).expect("positive revision")
        );
    }

    // And the result is still order-independent.
    let expected = pack;
    for order in permutations(&sources) {
        let actual = resolve(&order, &ReferenceInputs::new());
        assert_eq!(actual.document().json(), expected.document().json());
    }
}

#[test]
fn empty_source_list_resolves_to_an_empty_pack() {
    let pack = resolve(&[], &ReferenceInputs::new());
    assert_eq!(pack.resolved(), &json!({}));
    assert!(pack.provenance().is_empty());
    assert!(pack.redactions().is_empty());
    assert_eq!(pack.realm_id(), realm());
}

#[test]
fn malformed_source_key_and_non_object_content_are_rejected() {
    let mut foreign = source(ContextLayer::GlobalProfile, "global.profile", 1, json!({}));
    foreign.source_id = "Global Profile".to_owned();
    let error = preview(&ResolutionRequest {
        realm_id: realm(),
        sources: std::slice::from_ref(&foreign),
        references: &ReferenceInputs::new(),
    })
    .expect_err("an upper-case, space-bearing source key rejects");
    assert!(matches!(
        error,
        DomainError::Invalid {
            subject: "ContextSource.source_id",
            ..
        }
    ));

    let mut not_an_object = source(ContextLayer::GlobalProfile, "global.profile", 1, json!([]));
    not_an_object.content = json!(["not", "an", "object"]);
    let error = preview(&ResolutionRequest {
        realm_id: realm(),
        sources: std::slice::from_ref(&not_an_object),
        references: &ReferenceInputs::new(),
    })
    .expect_err("non-object content rejects");
    assert!(matches!(
        error,
        DomainError::Invalid {
            subject: "ContextSource",
            ..
        }
    ));
}
