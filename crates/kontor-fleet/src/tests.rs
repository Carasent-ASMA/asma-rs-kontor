use kontor_core::id::{
    CanonicalDocument, ContentHash, ExternalName, RoleCatalogId, RoleCode, RoleSlotId,
    SCHEMA_VERSION, SchemaVersion, SpecVersion,
};
use kontor_core::spec::{CatalogRoleRef, EffortLevel};
use serde::Serialize;
use std::collections::BTreeSet;

use super::rule::*;
use super::*;

const EXAMPLE: &str = include_str!("../../../config/examples/fleet.yml");

const TEAM: &str = "team/01936f5a-0000-7000-8000-000000000102/implement";
const COMMITTEE: &str = "committee/01991c00-0000-7000-8000-000000000001/reviewer-a";
const ADVISOR: &str = "advisor/01a02d00-0000-7000-8000-00000000ad01";

/// The `kontor-teams` hashing envelope around a Core Team revision.
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

fn role(code: &str, title: &str, catalog_id: RoleCatalogId) -> CatalogRoleRef {
    CatalogRoleRef {
        catalog_id,
        catalog_revision: SpecVersion::FIRST,
        role_code: RoleCode::parse(code).expect("role code"),
        standard_title: ExternalName::parse(title).expect("title"),
        custom_display_name: None,
    }
}

fn seat(slot: &str, role: CatalogRoleRef) -> Seat {
    Seat {
        role_slot_id: RoleSlotId::parse(slot).expect("slot"),
        role,
        presence: "required",
        ad_hoc_allowed: false,
    }
}

fn leadership_seats() -> Vec<Seat> {
    let catalog = RoleCatalogId::generate();
    vec![
        seat("tpm", role("TPM", "Technical Program Manager", catalog)),
        seat("lsa", role("LSA", "Lead Software Architect", catalog)),
    ]
}

fn revision(version: SpecVersion, seats: &[Seat]) -> CanonicalDocument {
    CanonicalDocument::from_serializable(&Envelope {
        schema_version: SCHEMA_VERSION,
        value: Roster {
            version,
            catalog_hash: ContentHash::of(b"catalog"),
            seats,
        },
    })
    .expect("canonical revision")
}

fn key(revision: &CanonicalDocument, seat: &Seat) -> LeadershipKey {
    LeadershipKey::for_pinned_seat(revision, &seat.role_slot_id, &seat.role).expect("proved seat")
}

/// One schema_version 2 policy binding every seat class this successor covers.
fn policy(lsa: &str, tpm: &str) -> String {
    format!(
        "\
schema_version: 2
domains:
  claude: {{ provider: claude, accounts: [claude-personal, claude-work] }}
  codex: {{ provider: codex, accounts: [codex-work] }}
  cursor: {{ provider: cursor, accounts: [cursor] }}
models:
  opus-5.5: {{ domain: claude, id: claude-opus-5-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: false }}
  opus-5: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}
  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}
  grok-4.6: {{ domain: cursor, id: grok-4.6, vendor: xai, efforts: [high], vision: true, calibrated: true }}
chains:
  claude-first:
    - [opus-5@xhigh]
    - [sol@xhigh]
  codex-first:
    - [sol@xhigh]
    - [opus-5.5@xhigh, opus-5@xhigh]
  review:
    - [grok-4.6@high]
    - [sol@xhigh]
bindings:
  {TEAM}: claude-first
  {COMMITTEE}: review
  {ADVISOR}: codex-first
  {lsa}: codex-first
  {tpm}: claude-first
rules:
  calibration_required: [{lsa}]
"
    )
}

fn refused(result: Result<FleetSnapshot, FleetError>) -> &'static str {
    match result {
        Err(FleetError::Invalid { rule }) => rule,
        other => panic!("expected a rule refusal, got {other:?}"),
    }
}

fn accounts(resolution: &FleetResolution) -> Vec<(&str, &str, u16, u16)> {
    resolution
        .routes
        .iter()
        .map(|route| {
            (
                route.rung.provider.0.as_str(),
                route.rung.model.0.as_str(),
                route.step,
                route.sub_step,
            )
        })
        .collect()
}

#[test]
fn a_v1_document_reads_the_same_through_both_entry_points() {
    let fleet = FleetSnapshot::parse(EXAMPLE).expect("the fleet reader accepts the example");
    let policy = FleetSnapshot::parse_policy(EXAMPLE).expect("the policy reader accepts it too");
    assert_eq!(fleet.hash(), policy.hash());
    assert_eq!(policy.schema_version(), 1);
    for key in [TEAM, COMMITTEE, ADVISOR] {
        assert_eq!(fleet.resolve(key), policy.resolve(key), "{key}");
        assert!(policy.resolve(key).is_some(), "{key} is bound");
    }
}

#[test]
fn a_v1_document_gains_no_successor_key_through_the_policy_reader() {
    let seats = leadership_seats();
    let revision = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&revision, &seats[1]);
    for spelling in [
        format!("core/{}", "01936f5a-0000-7000-8000-000000000101/scope"),
        lsa.as_str().to_owned(),
    ] {
        let yaml = EXAMPLE.replacen(
            "  team/01936f5a-0000-7000-8000-000000000101/scope:",
            &format!("  {spelling}:"),
            1,
        );
        assert_eq!(refused(FleetSnapshot::parse(&yaml)), V23, "{spelling}");
        assert_eq!(
            refused(FleetSnapshot::parse_policy(&yaml)),
            V23,
            "{spelling}"
        );
    }
}

#[test]
fn the_fleet_reader_does_not_read_the_successor() {
    let seats = leadership_seats();
    let revision = revision(SpecVersion::FIRST, &seats);
    let yaml = policy(
        key(&revision, &seats[1]).as_str(),
        key(&revision, &seats[0]).as_str(),
    );
    assert_eq!(refused(FleetSnapshot::parse(&yaml)), V01);
    assert_eq!(
        FleetSnapshot::parse_policy(&yaml)
            .expect("the policy reader accepts it")
            .schema_version(),
        2
    );
}

#[test]
fn the_policy_reader_refuses_other_schema_versions() {
    for version in ["0", "3", "10"] {
        let yaml = EXAMPLE.replacen(
            "schema_version: 1",
            &format!("schema_version: {version}"),
            1,
        );
        assert_eq!(
            refused(FleetSnapshot::parse_policy(&yaml)),
            V31,
            "{version}"
        );
    }
    assert!(matches!(
        FleetSnapshot::parse_policy("schema_version: [2"),
        Err(FleetError::PolicyDocument)
    ));
}

#[test]
fn every_leadership_spelling_other_than_the_pinned_revision_is_refused() {
    let hash = ContentHash::of(b"roster");
    let upper = hash.as_str().to_ascii_uppercase();
    let short = &hash.as_str()[..63];
    for spelling in [
        "leadership/lsa".to_owned(),
        "leadership/tpm".to_owned(),
        "leadership/LSA".to_owned(),
        "core/lsa".to_owned(),
        "core/LSA".to_owned(),
        format!("leadership/{upper}/lsa"),
        format!("leadership/{short}/lsa"),
        format!("leadership/{hash}/LSA"),
        format!("leadership/{hash}/lsa/extra"),
        format!("leadership/{hash}/"),
        "leadership//lsa".to_owned(),
        "leadership/project-1/lsa".to_owned(),
    ] {
        let yaml = policy(&spelling, &format!("leadership/{hash}/tpm"));
        assert_eq!(
            refused(FleetSnapshot::parse_policy(&yaml)),
            V32,
            "{spelling}"
        );
    }
}

#[test]
fn the_successor_adds_no_section_or_field() {
    let seats = leadership_seats();
    let revision = revision(SpecVersion::FIRST, &seats);
    let yaml = policy(
        key(&revision, &seats[1]).as_str(),
        key(&revision, &seats[0]).as_str(),
    );
    for extended in [
        yaml.replacen(
            "schema_version: 2",
            "schema_version: 2\noperator_exceptions: []",
            1,
        ),
        yaml.replacen("rules:", "rules:\n  operator_exception: [cursor]", 1),
        yaml.replacen(
            "calibrated: true }",
            "calibrated: true, operator_accepted: true }",
            1,
        ),
        yaml.replacen(
            "accounts: [cursor] }",
            "accounts: [cursor], exception: true }",
            1,
        ),
    ] {
        assert!(
            matches!(
                FleetSnapshot::parse_policy(&extended),
                Err(FleetError::PolicyDocument)
            ),
            "{extended}"
        );
    }
}

#[test]
fn a_leadership_key_names_the_complete_pinned_revision() {
    let seats = leadership_seats();
    let first = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&first, &seats[1]);
    assert_eq!(lsa.as_str(), format!("leadership/{}/lsa", first.hash()));
    assert_eq!(lsa.core_team_revision_hash(), first.hash());
    assert_eq!(lsa.core_team_version(), SpecVersion::FIRST);
    assert_eq!(lsa.role_slot_id().as_str(), "lsa");

    // Version, catalog hash and seat order are all part of the identity: the
    // same slot in a different revision is a different key.
    let next = revision(SpecVersion::FIRST.next().expect("next"), &seats);
    let reordered = revision(SpecVersion::FIRST, &[seats[1].clone(), seats[0].clone()]);
    assert_ne!(key(&next, &seats[1]), lsa);
    assert_ne!(key(&reordered, &seats[1]), lsa);
}

#[test]
fn a_leadership_seat_must_occur_exactly_once() {
    let seats = leadership_seats();
    let without_lsa = revision(SpecVersion::FIRST, &seats[..1]);
    let twice = revision(
        SpecVersion::FIRST,
        &[seats[0].clone(), seats[1].clone(), seats[1].clone()],
    );
    for document in [&without_lsa, &twice] {
        assert!(matches!(
            LeadershipKey::for_pinned_seat(document, &seats[1].role_slot_id, &seats[1].role),
            Err(FleetError::Invalid { rule }) if rule == L02
        ));
    }
}

#[test]
fn a_leadership_seat_must_carry_its_frozen_role() {
    let seats = leadership_seats();
    let document = revision(SpecVersion::FIRST, &seats);
    let mut relabelled = seats[1].role.clone();
    relabelled.standard_title = ExternalName::parse("Architect").expect("title");
    let mut other_catalog = seats[1].role.clone();
    other_catalog.catalog_id = RoleCatalogId::generate();
    for role in [&relabelled, &other_catalog, &seats[0].role] {
        assert!(matches!(
            LeadershipKey::for_pinned_seat(&document, &seats[1].role_slot_id, role),
            Err(FleetError::Invalid { rule }) if rule == L03
        ));
    }
}

#[test]
fn a_document_that_is_not_a_core_team_revision_builds_no_key() {
    #[derive(Serialize)]
    struct NotARoster {
        schema_version: SchemaVersion,
        value: &'static str,
    }
    let seats = leadership_seats();
    let document = CanonicalDocument::from_serializable(&NotARoster {
        schema_version: SCHEMA_VERSION,
        value: "lsa",
    })
    .expect("canonical");
    assert!(matches!(
        LeadershipKey::for_pinned_seat(&document, &seats[1].role_slot_id, &seats[1].role),
        Err(FleetError::Invalid { rule }) if rule == L01
    ));
}

#[test]
fn one_policy_resolves_leadership_delivery_and_consultation_with_provenance() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let (tpm, lsa) = (key(&roster, &seats[0]), key(&roster, &seats[1]));
    let snapshot =
        FleetSnapshot::parse_policy(&policy(lsa.as_str(), tpm.as_str())).expect("valid policy");

    let lsa_resolution = snapshot.resolve_leadership(&lsa).expect("LSA is bound");
    let tpm_resolution = snapshot.resolve_leadership(&tpm).expect("TPM is bound");
    let team = snapshot.resolve(TEAM).expect("delivery seat is bound");
    let committee = snapshot.resolve(COMMITTEE).expect("reviewer is bound");
    let advisor = snapshot.resolve(ADVISOR).expect("advisor is bound");

    for (resolution, binding_key, chain) in [
        (&lsa_resolution, lsa.as_str(), "codex-first"),
        (&tpm_resolution, tpm.as_str(), "claude-first"),
        (&team, TEAM, "claude-first"),
        (&committee, COMMITTEE, "review"),
        (&advisor, ADVISOR, "codex-first"),
    ] {
        assert_eq!(
            resolution.provenance,
            FleetProvenance {
                policy_hash: snapshot.hash().clone(),
                schema_version: 2,
                binding_key: binding_key.to_owned(),
                chain: chain.to_owned(),
            }
        );
    }
    // The LSA and the Advisor share one chain, and only the LSA key is
    // calibration-scoped: the uncalibrated Opus 5.5 drops out of the LSA's
    // routes alone, so rules apply to a leadership key as to any other key.
    assert_eq!(
        accounts(&advisor),
        [
            ("codex-work", "gpt-5.6-sol", 1, 1),
            ("claude-personal", "claude-opus-5-5", 2, 1),
            ("claude-work", "claude-opus-5-5", 2, 2),
            ("claude-personal", "claude-opus-5", 2, 3),
            ("claude-work", "claude-opus-5", 2, 4),
        ]
    );
    assert_eq!(
        accounts(&lsa_resolution),
        [
            ("codex-work", "gpt-5.6-sol", 1, 1),
            ("claude-personal", "claude-opus-5", 2, 1),
            ("claude-work", "claude-opus-5", 2, 2),
        ]
    );
    assert_eq!(
        accounts(&tpm_resolution),
        [
            ("claude-personal", "claude-opus-5", 1, 1),
            ("claude-work", "claude-opus-5", 1, 2),
            ("codex-work", "gpt-5.6-sol", 2, 1),
        ]
    );
    assert_eq!(accounts(&team), accounts(&tpm_resolution));
    assert_eq!(
        accounts(&committee),
        [
            ("cursor", "grok-4.6", 1, 1),
            ("codex-work", "gpt-5.6-sol", 2, 1)
        ]
    );
    assert_eq!(
        committee.routes[0].rung.effort,
        Some(EffortLevel::High),
        "effort is parsed by the one vocabulary"
    );
}

#[test]
fn a_leadership_binding_is_never_resolved_from_text() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let (tpm, lsa) = (key(&roster, &seats[0]), key(&roster, &seats[1]));
    let snapshot =
        FleetSnapshot::parse_policy(&policy(lsa.as_str(), tpm.as_str())).expect("valid policy");
    assert!(snapshot.resolve_leadership(&lsa).is_some());
    for text in [lsa.as_str(), "leadership/lsa", "core/lsa"] {
        assert!(snapshot.resolve(text).is_none(), "{text}");
        assert!(snapshot.routes_for(text).is_none(), "{text}");
    }
}

#[test]
fn equal_policy_bytes_resolve_identically() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let (tpm, lsa) = (key(&roster, &seats[0]), key(&roster, &seats[1]));
    let yaml = policy(lsa.as_str(), tpm.as_str());
    let one = FleetSnapshot::parse_policy(&yaml).expect("valid policy");
    let two = FleetSnapshot::parse_policy(&yaml).expect("valid policy");
    assert_eq!(one.hash(), two.hash());
    assert_eq!(one.raw(), two.raw());
    assert_eq!(one.resolve_leadership(&lsa), two.resolve_leadership(&lsa));
    for key in [TEAM, COMMITTEE, ADVISOR] {
        assert_eq!(one.resolve(key), two.resolve(key), "{key}");
    }
}

#[test]
fn every_successor_rule_string_is_stable() {
    assert_eq!(V31, "schema_version must be 1 or 2");
    assert_eq!(
        V32,
        "a binding key must be team/<id>/<slot>, committee/<id>/<slot>, advisor/<id> or leadership/<core-team-revision-hash>/<role-slot-id>"
    );
    assert_eq!(
        L01,
        "a leadership key needs the canonical document of a complete Core Team revision"
    );
    assert_eq!(
        L02,
        "a leadership seat must occur exactly once in its pinned Core Team revision"
    );
    assert_eq!(
        L03,
        "a leadership seat's role must match the frozen role snapshot of its slot"
    );
}

#[test]
fn a_requested_route_is_admitted_only_as_the_chain_offers_it() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let (tpm, lsa) = (key(&roster, &seats[0]), key(&roster, &seats[1]));
    let snapshot =
        FleetSnapshot::parse_policy(&policy(lsa.as_str(), tpm.as_str())).expect("valid policy");
    let resolution = snapshot.resolve_leadership(&lsa).expect("LSA is bound");
    let rung = |provider: &str, model: &str, effort: Option<EffortLevel>| ModelRung {
        provider: ProviderRef(provider.to_owned()),
        model: ModelRef(model.to_owned()),
        effort,
    };
    let offered = resolution
        .route_for(&rung(
            "claude-work",
            "claude-opus-5",
            Some(EffortLevel::Xhigh),
        ))
        .expect("step 2 offers Opus 5 on the work login");
    assert_eq!((offered.step, offered.sub_step), (2, 2));
    for refused in [
        // The chain's own model on an effort it does not name.
        rung("claude-work", "claude-opus-5", Some(EffortLevel::High)),
        // A model the chain lists but the LSA's calibration rule removed.
        rung("claude-work", "claude-opus-5-5", Some(EffortLevel::Xhigh)),
        // A listed model on an account the domain does not declare.
        rung("claude", "claude-opus-5", Some(EffortLevel::Xhigh)),
        // A route no chain binds to this seat.
        rung("cursor", "grok-4.6", Some(EffortLevel::High)),
    ] {
        assert!(resolution.route_for(&refused).is_none(), "{refused:?}");
    }
}

/// A policy with one route of every exclusion kind, for selection tests.
fn selection_policy(lsa: &str) -> String {
    format!(
        "\
schema_version: 2
domains:
  claude: {{ provider: claude, accounts: [claude-personal, claude-work] }}
  codex: {{ provider: codex, accounts: [codex-work, codex-personal] }}
  cursor: {{ provider: cursor, accounts: [cursor] }}
unavailable:
  domains: [cursor]
  accounts: [codex-personal]
models:
  opus-5.5: {{ domain: claude, id: claude-opus-5-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: false }}
  opus-5: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}
  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}
  grok-4.6: {{ domain: cursor, id: grok-4.6, vendor: xai, efforts: [high], vision: true, calibrated: true }}
chains:
  lead:
    - [opus-5.5@xhigh, opus-5@xhigh]
    - [sol@xhigh]
    - [grok-4.6@high]
bindings:
  {lsa}: lead
  {TEAM}: lead
  {ADVISOR}: lead
rules:
  calibration_required: [{lsa}]
"
    )
}

/// Provider, model, step and sub-step of one route.
type Position<'a> = (&'a str, &'a str, u16, u16);

/// Step, model, account and reason of one policy exclusion.
type Removed<'a> = (u16, &'a str, Option<&'a str>, ExclusionReason);

fn route_of(route: &FleetRoute) -> Position<'_> {
    (
        route.rung.provider.0.as_str(),
        route.rung.model.0.as_str(),
        route.step,
        route.sub_step,
    )
}

#[test]
fn the_policy_explains_every_route_it_removed() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&roster, &seats[1]);
    let snapshot = FleetSnapshot::parse_policy(&selection_policy(lsa.as_str())).expect("valid");
    let resolution = snapshot.resolve_leadership(&lsa).expect("bound");
    let excluded: Vec<Removed<'_>> = resolution
        .excluded
        .iter()
        .map(|exclusion| {
            (
                exclusion.step,
                exclusion.model.as_str(),
                exclusion.account.as_deref(),
                exclusion.reason,
            )
        })
        .collect();
    assert_eq!(
        excluded,
        [
            (1, "claude-opus-5-5", None, ExclusionReason::NotCalibrated),
            (
                2,
                "gpt-5.6-sol",
                Some("codex-personal"),
                ExclusionReason::UnavailableAccount
            ),
            (3, "grok-4.6", None, ExclusionReason::UnavailableDomain),
        ]
    );
    // The same chain on an unscoped key keeps the uncalibrated model.
    let team = snapshot.resolve(TEAM).expect("bound");
    assert!(
        team.excluded
            .iter()
            .all(|exclusion| exclusion.reason != ExclusionReason::NotCalibrated)
    );
    assert_eq!(
        route_of(&team.routes[0]),
        ("claude-personal", "claude-opus-5-5", 1, 1)
    );
}

#[test]
fn a_policy_chooses_the_first_eligible_route_and_says_why_it_passed_the_rest() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&roster, &seats[1]);
    let snapshot = FleetSnapshot::parse_policy(&selection_policy(lsa.as_str())).expect("valid");
    let resolution = snapshot.resolve_leadership(&lsa).expect("bound");

    let open = resolution.select(&Eligibility::default());
    assert_eq!(
        open.selected.as_ref().map(route_of),
        Some(("claude-personal", "claude-opus-5", 1, 1)),
        "with nothing unavailable the chain's first route is the choice"
    );
    assert_eq!(open.provenance, resolution.provenance);
    assert_eq!(open.excluded_by_policy, resolution.excluded);

    let eligibility = Eligibility {
        unavailable_accounts: BTreeSet::from(["claude-personal".to_owned()]),
        excluded_vendors: BTreeSet::new(),
    };
    let degraded = resolution.select(&eligibility);
    assert_eq!(
        degraded.selected.as_ref().map(route_of),
        Some(("claude-work", "claude-opus-5", 1, 2)),
        "the next account of the same step before the next step"
    );
    assert_eq!(degraded.eligibility, eligibility);
    let verdicts: Vec<(Position<'_>, Option<ExclusionReason>)> = degraded
        .considered
        .iter()
        .map(|considered| (route_of(&considered.route), considered.excluded))
        .collect();
    assert_eq!(
        verdicts,
        [
            (
                ("claude-personal", "claude-opus-5", 1, 1),
                Some(ExclusionReason::AccountUnavailableNow)
            ),
            (("claude-work", "claude-opus-5", 1, 2), None),
            (("codex-work", "gpt-5.6-sol", 2, 1), None),
        ]
    );

    let independent = resolution.select(&Eligibility {
        unavailable_accounts: BTreeSet::new(),
        excluded_vendors: BTreeSet::from(["anthropic".to_owned()]),
    });
    assert_eq!(
        independent.selected.as_ref().map(route_of),
        Some(("codex-work", "gpt-5.6-sol", 2, 1)),
        "a seat that must avoid a vendor descends past it"
    );
}

#[test]
fn an_eligibility_that_admits_nothing_is_the_defined_block() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&roster, &seats[1]);
    let snapshot = FleetSnapshot::parse_policy(&selection_policy(lsa.as_str())).expect("valid");
    let resolution = snapshot.resolve_leadership(&lsa).expect("bound");
    let blocked = resolution.select(&Eligibility {
        unavailable_accounts: BTreeSet::from(["codex-work".to_owned()]),
        excluded_vendors: BTreeSet::from(["anthropic".to_owned()]),
    });
    assert!(blocked.selected.is_none(), "no fallback to any other route");
    assert!(
        blocked
            .considered
            .iter()
            .all(|considered| considered.excluded.is_some()),
        "every passed route says why"
    );
    assert_eq!(blocked.considered.len(), resolution.routes.len());
}

#[test]
fn equal_policy_bytes_and_eligibility_choose_identically() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&roster, &seats[1]);
    let yaml = selection_policy(lsa.as_str());
    let eligibility = Eligibility {
        unavailable_accounts: BTreeSet::from(["claude-personal".to_owned()]),
        excluded_vendors: BTreeSet::new(),
    };
    let choose = |key: &str| {
        let snapshot = FleetSnapshot::parse_policy(&yaml).expect("valid");
        (
            snapshot
                .resolve_leadership(&lsa)
                .expect("LSA")
                .select(&eligibility),
            snapshot.resolve(key).expect("bound").select(&eligibility),
        )
    };
    for key in [TEAM, ADVISOR] {
        assert_eq!(choose(key), choose(key), "{key}");
    }
}

#[test]
fn only_the_activated_bytes_are_admitted() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    let lsa = key(&roster, &seats[1]);
    let activated = selection_policy(lsa.as_str());
    let hash = ContentHash::of(activated.as_bytes());

    let admitted = FleetSnapshot::activated(&hash, 2, &activated).expect("the activated bytes");
    assert_eq!(admitted.hash(), &hash);

    // An authoring edit, however valid, is not the activated policy.
    let edited = activated.replacen("    - [sol@xhigh]\n", "", 1);
    assert!(
        FleetSnapshot::parse_policy(&edited).is_ok(),
        "the edit is valid"
    );
    for (candidate, schema, rule) in [
        (edited.as_str(), 2, P07),
        (EXAMPLE, 2, P07),
        (activated.as_str(), 1, A09),
    ] {
        assert!(matches!(
            FleetSnapshot::activated(&hash, schema, candidate),
            Err(FleetError::Invalid { rule: refused }) if refused == rule
        ));
    }
    assert!(matches!(
        FleetSnapshot::published(&ContentHash::of(b"schema_version: ["), "schema_version: ["),
        Err(FleetError::PolicyDocument)
    ));
}

#[test]
fn a_published_policy_carries_no_credential_path_or_email() {
    let yaml = |account: &str, prefix: &str, model: &str| {
        format!(
            "schema_version: 2\n\
             domains:\n  codex: {{ provider: codex, accounts: [{account}]{prefix} }}\n\
             models:\n  sol: {{ domain: codex, id: {model}, vendor: openai, efforts: [xhigh] }}\n\
             chains:\n  c:\n    - [sol@xhigh]\n\
             bindings:\n  team/t/s: c\n"
        )
    };
    assert!(FleetSnapshot::parse_policy(&yaml("codex", "", "gpt-5.6-sol")).is_ok());
    for (account, prefix, model) in [
        ("codex-sk-abcdefghijklmnopqrstuvwxyz0123", "", "gpt-5.6-sol"),
        ("codex-igor@carasent.com", "", "gpt-5.6-sol"),
        (
            "codex",
            ", model_prefix: \"/Users/igor/provider-homes/\"",
            "/Users/igor/provider-homes/sol",
        ),
        ("codex", "", "~/.codex/sol"),
        ("codex", "", "ghp_abcdefghijklmnopqrstuvwxyz0123"),
        ("codex", "", "gpt-5.6-sol@work"),
    ] {
        let published = yaml(account, prefix, model);
        assert_eq!(
            refused(FleetSnapshot::parse_policy(&published)),
            V33,
            "{published}"
        );
        // The unmigrated fleet.yml reader keeps its exact v1 checks.
        assert!(
            FleetSnapshot::parse(&published.replacen("schema_version: 2", "schema_version: 1", 1))
                .is_ok(),
            "{published}"
        );
    }
    // No section or field exists to carry a credential in the first place.
    for field in ["token", "credential", "api_key", "provider_home", "quota"] {
        let extended = yaml("codex", &format!(", {field}: x"), "gpt-5.6-sol");
        assert!(matches!(
            FleetSnapshot::parse_policy(&extended),
            Err(FleetError::PolicyDocument)
        ));
    }
}

#[test]
fn an_older_roster_revision_keeps_its_own_leadership_binding() {
    let seats = leadership_seats();
    let first = revision(SpecVersion::FIRST, &seats);
    let second = revision(SpecVersion::FIRST.next().expect("next"), &seats);
    let (old_lsa, new_lsa) = (key(&first, &seats[1]), key(&second, &seats[1]));
    let yaml = format!(
        "schema_version: 2\n\
         domains:\n  codex: {{ provider: codex, accounts: [codex-work] }}\n  claude: {{ provider: claude, accounts: [claude-work] }}\n\
         models:\n  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh] }}\n  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh] }}\n\
         chains:\n  old:\n    - [sol@xhigh]\n  new:\n    - [opus@xhigh]\n\
         bindings:\n  {old_lsa}: old\n  {new_lsa}: new\n"
    );
    let snapshot = FleetSnapshot::parse_policy(&yaml).expect("valid");
    let chosen = |key: &LeadershipKey| {
        snapshot
            .resolve_leadership(key)
            .expect("bound")
            .select(&Eligibility::default())
            .selected
            .map(|route| route.rung.provider.0)
    };
    assert_eq!(
        chosen(&old_lsa).as_deref(),
        Some("codex-work"),
        "the pinned epic keeps its route"
    );
    assert_eq!(chosen(&new_lsa).as_deref(), Some("claude-work"));
}

#[test]
fn every_direct_mode_rule_string_is_stable() {
    assert_eq!(
        V33,
        "a fleet policy value must not carry credential material, a filesystem path or an email address"
    );
    assert_eq!(
        P07,
        "a published fleet policy does not hash to its content address"
    );
    assert_eq!(
        A09,
        "the activated fleet policy's schema_version differs from its activation record"
    );
}

#[test]
fn a_verified_roster_and_a_slot_build_the_same_key_as_the_pinned_seat() {
    let seats = leadership_seats();
    let roster = revision(SpecVersion::FIRST, &seats);
    for seat in &seats {
        let from_slot =
            LeadershipKey::for_pinned_slot(&roster, &seat.role_slot_id).expect("a pinned slot");
        assert_eq!(from_slot, key(&roster, seat), "{}", seat.role_slot_id);
    }
    let absent = RoleSlotId::parse("qa").expect("slot");
    assert!(matches!(
        LeadershipKey::for_pinned_slot(&roster, &absent),
        Err(FleetError::Invalid { rule }) if rule == L02
    ));
    let twice = revision(
        SpecVersion::FIRST,
        &[seats[0].clone(), seats[1].clone(), seats[1].clone()],
    );
    assert!(matches!(
        LeadershipKey::for_pinned_slot(&twice, &seats[1].role_slot_id),
        Err(FleetError::Invalid { rule }) if rule == L02
    ));
}
