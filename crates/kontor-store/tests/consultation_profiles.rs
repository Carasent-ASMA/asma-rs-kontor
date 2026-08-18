//! Published Advisor profile and Committee template revisions.
//!
//! A consultation pins the exact revision it was invoked under, so these tests
//! are about the two things that pinning depends on: a version is the next one
//! or it is refused, and what comes back out is byte-for-byte what went in.

use kontor_core::consultation::ConsultationFamily;
use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalName, ProjectId, SpecVersion, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    NewProject, ProjectRepository, RepositoryError, StoredConsultationProfileRevision,
};
use kontor_store::SqliteStore;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

struct World {
    _home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
}

fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Consultation project"),
            root_path: name("/tmp/op05-consultations"),
            created_at: at("2026-08-17T09:00:00Z"),
        })
        .expect("the project is created");
    World {
        _home: home,
        store,
        project_id,
    }
}

/// A stand-in definition. These tests are about the revision machinery, not
/// about what a publishable profile says — the specification's own suite covers
/// that, and using a canonical document here keeps the two independent.
fn definition(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

fn revision(
    project_id: ProjectId,
    family: ConsultationFamily,
    profile_id: &str,
    version: u32,
    marker: &str,
) -> StoredConsultationProfileRevision {
    let document = definition(marker);
    StoredConsultationProfileRevision {
        project_id,
        family,
        profile_id: profile_id.to_owned(),
        version: SpecVersion::parse(version).expect("a version"),
        name: name("Independent review"),
        definition: document.json().to_owned(),
        definition_hash: document.hash().clone(),
        published_at: at("2026-08-17T09:05:00Z"),
    }
}

const PROFILE: &str = "01991c00-0000-7000-8000-000000000001";
const OTHER: &str = "01991c00-0000-7000-8000-000000000002";

#[test]
fn the_first_revision_of_a_profile_is_version_one() {
    let world = world();
    world
        .store
        .publish_consultation_profile_revision(&revision(
            world.project_id,
            ConsultationFamily::Committee,
            PROFILE,
            1,
            "first",
        ))
        .expect("version one starts a profile");
}

#[test]
fn a_first_revision_that_skips_version_one_is_refused() {
    let world = world();
    let error = world
        .store
        .publish_consultation_profile_revision(&revision(
            world.project_id,
            ConsultationFamily::Committee,
            PROFILE,
            2,
            "first",
        ))
        .expect_err("a profile cannot start at version two");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
}

#[test]
fn revisions_must_be_consecutive() {
    let world = world();
    for version in 1..=2 {
        world
            .store
            .publish_consultation_profile_revision(&revision(
                world.project_id,
                ConsultationFamily::Committee,
                PROFILE,
                version,
                "consecutive",
            ))
            .expect("the next revision publishes");
    }
    let error = world
        .store
        .publish_consultation_profile_revision(&revision(
            world.project_id,
            ConsultationFamily::Committee,
            PROFILE,
            4,
            "gap",
        ))
        .expect_err("a gap would leave a pinned predecessor unreadable");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
}

#[test]
fn republishing_a_version_is_refused() {
    let world = world();
    let first = revision(
        world.project_id,
        ConsultationFamily::Committee,
        PROFILE,
        1,
        "original",
    );
    world
        .store
        .publish_consultation_profile_revision(&first)
        .expect("version one publishes");
    // A different document under the same version is exactly the case a run
    // pinning version one must be protected from.
    let error = world
        .store
        .publish_consultation_profile_revision(&revision(
            world.project_id,
            ConsultationFamily::Committee,
            PROFILE,
            1,
            "rewritten",
        ))
        .expect_err("a published revision cannot be republished");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
}

#[test]
fn each_profile_versions_independently() {
    let world = world();
    for profile in [PROFILE, OTHER] {
        world
            .store
            .publish_consultation_profile_revision(&revision(
                world.project_id,
                ConsultationFamily::Committee,
                profile,
                1,
                profile,
            ))
            .expect("each profile starts at version one of its own");
    }
    let listed = world
        .store
        .list_consultation_profile_revisions(world.project_id, ConsultationFamily::Committee)
        .expect("the catalog reads");
    assert_eq!(listed.len(), 2);
}

#[test]
fn the_two_families_are_separate_catalogs() {
    let world = world();
    for family in [ConsultationFamily::Advisor, ConsultationFamily::Committee] {
        world
            .store
            .publish_consultation_profile_revision(&revision(
                world.project_id,
                family,
                PROFILE,
                1,
                family.as_str(),
            ))
            .expect("one id may exist in both families without colliding");
    }
    let advisors = world
        .store
        .list_consultation_profile_revisions(world.project_id, ConsultationFamily::Advisor)
        .expect("the Advisor catalog reads");
    let committees = world
        .store
        .list_consultation_profile_revisions(world.project_id, ConsultationFamily::Committee)
        .expect("the Committee catalog reads");
    assert_eq!(advisors.len(), 1);
    assert_eq!(committees.len(), 1);
    assert_eq!(advisors[0].family, ConsultationFamily::Advisor);
    assert_eq!(committees[0].family, ConsultationFamily::Committee);
}

#[test]
fn a_published_definition_reads_back_byte_for_byte() {
    let world = world();
    let published = revision(
        world.project_id,
        ConsultationFamily::Advisor,
        PROFILE,
        1,
        "exact",
    );
    world
        .store
        .publish_consultation_profile_revision(&published)
        .expect("it publishes");
    let listed = world
        .store
        .list_consultation_profile_revisions(world.project_id, ConsultationFamily::Advisor)
        .expect("the catalog reads");
    assert_eq!(listed[0].definition, published.definition);
    assert_eq!(listed[0].definition_hash, published.definition_hash);
    assert_eq!(listed[0].name, published.name);
    assert_eq!(listed[0].version, published.version);
    // And the digest really is over those bytes, not merely stored beside them.
    assert_eq!(
        &ContentHash::of(listed[0].definition.as_bytes()),
        &listed[0].definition_hash
    );
}

#[test]
fn a_catalog_reads_in_a_stable_order() {
    let world = world();
    // Published newest-id-first, so file order cannot be what makes the read
    // look sorted.
    for profile in [OTHER, PROFILE] {
        for version in 1..=2 {
            world
                .store
                .publish_consultation_profile_revision(&revision(
                    world.project_id,
                    ConsultationFamily::Committee,
                    profile,
                    version,
                    "ordered",
                ))
                .expect("it publishes");
        }
    }
    let listed = world
        .store
        .list_consultation_profile_revisions(world.project_id, ConsultationFamily::Committee)
        .expect("the catalog reads");
    let order: Vec<(&str, u32)> = listed
        .iter()
        .map(|stored| (stored.profile_id.as_str(), stored.version.get()))
        .collect();
    assert_eq!(
        order,
        vec![(PROFILE, 1), (PROFILE, 2), (OTHER, 1), (OTHER, 2)]
    );
}

#[test]
fn another_projects_revisions_do_not_resolve() {
    let world = world();
    world
        .store
        .publish_consultation_profile_revision(&revision(
            world.project_id,
            ConsultationFamily::Committee,
            PROFILE,
            1,
            "scoped",
        ))
        .expect("it publishes");
    let elsewhere = world
        .store
        .list_consultation_profile_revisions(ProjectId::generate(), ConsultationFamily::Committee)
        .expect("the read succeeds");
    assert!(
        elsewhere.is_empty(),
        "a valid id from another project must not resolve"
    );
}
