//! The four prepared ASMA-8117 Team Definition successor documents.
//!
//! ASMA-8117 does not publish, select, deploy or migrate anything. What it owes
//! is four complete naming-only candidates that a later scope can publish
//! verbatim, plus the proof that each one differs from its exact live source
//! only in its immutable version and the agreed container-template token
//! substitutions. These tests are that proof, and they fail if either the
//! candidate or the recorded source drifts by a single field.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kontor_core::id::ContentHash;
use kontor_core::naming::{NativeNameSegment, NativeNameToken};
use kontor_core::spec::TeamDefinitionSpec;

/// Prepared successors live with the ASMA-8117 evidence, deliberately outside
/// the bundled operational profile so no deployment can publish them.
fn evidence_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/evidence/ASMA-8117/team-definition-successors")
}

fn read_json(relative: &str) -> serde_json::Value {
    let path = evidence_root().join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is valid JSON: {error}", path.display()))
}

fn manifest() -> serde_json::Value {
    read_json("MANIFEST.json")
}

fn entries() -> Vec<serde_json::Value> {
    manifest()["successors"]
        .as_array()
        .expect("the manifest lists its successors")
        .clone()
}

fn definition(value: &serde_json::Value) -> TeamDefinitionSpec {
    serde_json::from_value(value.clone()).expect("the document deserializes as a Team Definition")
}

fn canonical_hash(definition: &TeamDefinitionSpec) -> ContentHash {
    definition
        .canonicalize()
        .expect("a validated definition canonicalizes")
        .hash()
        .clone()
}

fn hash(text: &str) -> ContentHash {
    ContentHash::parse(text).expect("a recorded canonical hash")
}

/// The one agreed substitution per container kind.
fn substitution(kind: &str) -> (NativeNameToken, NativeNameToken) {
    match kind {
        "ESW" | "ECP" => (NativeNameToken::EpicItemCode, NativeNameToken::EpicJiraKey),
        "TSW" => (NativeNameToken::TaskItemCode, NativeNameToken::TaskJiraKey),
        "ASW" | "CSW" => (
            NativeNameToken::ScopeItemCode,
            NativeNameToken::ScopeJiraKey,
        ),
        other => panic!("no substitution is agreed for a {other} container"),
    }
}

#[test]
fn the_manifest_covers_exactly_the_four_live_pinned_source_revisions() {
    let entries = entries();
    assert_eq!(entries.len(), 4, "the census recorded four live variants");

    let recorded: BTreeMap<(String, u64), String> = entries
        .iter()
        .map(|entry| {
            (
                (
                    entry["source"]["definition_id"]
                        .as_str()
                        .expect("a source lineage")
                        .to_owned(),
                    entry["source"]["version"]
                        .as_u64()
                        .expect("a source version"),
                ),
                entry["source"]["canonical_hash"]
                    .as_str()
                    .expect("a source hash")
                    .to_owned(),
            )
        })
        .collect();

    // The exact tuples and hashes the 2026-09-06 project-state read recorded.
    let census = [
        (
            "01936f5a-2000-7000-8000-000000000001",
            1,
            "217747248d527556fa452a0b6380215a3699def6ba73cedbaabdc00da3b4da56",
        ),
        (
            "01936f5a-2000-7000-8000-000000000001",
            2,
            "31cdff80e27cbe1e4043e150d2cdbc43ff79a8fa7fd8d892d2b5049a600f2e13",
        ),
        (
            "01936f5a-2000-7000-8000-000000000001",
            3,
            "3818bade075891fe26fbc3d199b3f29204bc003082a1342983a208d3a868e35c",
        ),
        (
            "01a07400-1000-7000-8000-000000008098",
            2,
            "d2f1131e1b9548873c7a02a0c15761c982e70779c8d7e6ded872439659d79a43",
        ),
    ];
    assert_eq!(recorded.len(), census.len());
    for (lineage, version, expected) in census {
        assert_eq!(
            recorded
                .get(&(lineage.to_owned(), version))
                .map(String::as_str),
            Some(expected),
            "the manifest binds {lineage} v{version} to its exact recorded hash"
        );
    }
}

#[test]
fn every_recorded_source_document_still_hashes_to_its_published_identity() {
    for entry in entries() {
        let source = entry["source"].clone();
        let document = definition(&read_json(
            source["document"].as_str().expect("a source path"),
        ));
        let recorded = source["canonical_hash"].as_str().expect("a source hash");

        assert_eq!(
            document.definition_id.to_string(),
            source["definition_id"].as_str().expect("a lineage"),
        );
        assert_eq!(
            u64::from(document.version.get()),
            source["version"].as_u64().expect("a version"),
        );
        assert_eq!(
            canonical_hash(&document),
            hash(recorded),
            "the stored copy of {} v{} is not the published document",
            source["definition_id"],
            source["version"],
        );
    }
}

#[test]
fn every_candidate_is_publishable_and_hashes_to_its_recorded_identity() {
    for entry in entries() {
        let candidate = entry["candidate"].clone();
        let document = definition(&read_json(
            candidate["document"].as_str().expect("a candidate path"),
        ));

        document
            .validate()
            .expect("a prepared successor is publishable as written");
        assert_eq!(
            canonical_hash(&document),
            hash(candidate["canonical_hash"].as_str().expect("a hash")),
            "the manifest records the exact bytes ASMA-8120 would publish",
        );
    }
}

#[test]
fn every_candidate_targets_an_unused_version_of_its_own_source_lineage() {
    // OQ-8117-01 resolved as option (a): keep the lineage, take the next unused
    // version. Inventing a lineage would invent an identity; reusing a
    // published version would collide with immutable bytes.
    let published: BTreeMap<&str, u32> = BTreeMap::from([
        ("01936f5a-2000-7000-8000-000000000001", 3),
        ("01a07400-1000-7000-8000-000000008098", 2),
    ]);
    let mut targets = Vec::new();
    for entry in entries() {
        let lineage = entry["source"]["definition_id"]
            .as_str()
            .expect("a lineage")
            .to_owned();
        assert_eq!(
            entry["candidate"]["definition_id"].as_str(),
            Some(lineage.as_str()),
            "a naming-only successor keeps its source lineage",
        );
        let version = entry["candidate"]["version"].as_u64().expect("a version");
        assert!(
            version > u64::from(*published.get(lineage.as_str()).expect("a known lineage")),
            "{lineage} v{version} is not an unused version",
        );
        targets.push((lineage, version));
    }
    targets.sort();
    targets.dedup();
    assert_eq!(targets.len(), 4, "two candidates claim one address");
}

#[test]
fn a_candidate_differs_from_its_source_only_by_version_and_the_agreed_tokens() {
    for entry in entries() {
        let source = definition(&read_json(
            entry["source"]["document"].as_str().expect("a source path"),
        ));
        let candidate = definition(&read_json(
            entry["candidate"]["document"]
                .as_str()
                .expect("a candidate path"),
        ));

        // Everything that carries identity or behavior is byte-identical.
        assert_eq!(candidate.schema_version, source.schema_version);
        assert_eq!(candidate.definition_id, source.definition_id);
        assert_eq!(candidate.name, source.name, "the definition name is exact");
        assert_eq!(candidate.topology, source.topology, "the pin is exact");
        assert_eq!(
            candidate.separator, source.separator,
            "the separator glyph is exact",
        );
        assert_ne!(candidate.version, source.version);
        assert_eq!(
            candidate.containers.len(),
            source.containers.len(),
            "container order and count are exact",
        );

        for (after, before) in candidate.containers.iter().zip(&source.containers) {
            assert_eq!(after.kind, before.kind, "container order is exact");
            assert_eq!(after.parent, before.parent);
            assert_eq!(after.prefix, before.prefix);
            assert_eq!(
                after.projection_capabilities,
                before.projection_capabilities
            );
            assert_eq!(after.read_only, before.read_only);
            assert_eq!(
                after.seat_name_template, before.seat_name_template,
                "seat rendering is untouched by a container-naming successor",
            );
            assert_eq!(after.slots, before.slots, "slot order and labels are exact");
            assert_eq!(after.team_slots, before.team_slots);

            // The only admitted difference: exactly one token, swapped for the
            // successor its container kind is assigned, in place.
            let (from, to) = substitution(after.kind.as_str());
            let old = before.name_template.segments().expect("a typed template");
            let new = after.name_template.segments().expect("a typed template");
            assert_eq!(new.len(), old.len(), "no segment is added or removed");
            let mut substitutions = 0;
            for (after_segment, before_segment) in new.iter().zip(old) {
                if before_segment == &NativeNameSegment::Token(from) {
                    assert_eq!(after_segment, &NativeNameSegment::Token(to));
                    substitutions += 1;
                } else {
                    assert_eq!(
                        after_segment, before_segment,
                        "only the assigned token may move",
                    );
                }
            }
            assert_eq!(
                substitutions, 1,
                "the {} template substitutes exactly one token",
                after.kind,
            );
        }
    }
}
