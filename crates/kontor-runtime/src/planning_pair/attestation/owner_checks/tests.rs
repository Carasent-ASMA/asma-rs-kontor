use std::sync::OnceLock;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair};
use kontor_core::id::{ExternalName, MiniProjectId, ProjectId, RealmId, RuntimeKindKey, TaskId};

use super::super::{
    APPLICATION_AUDIENCE, AttestationClaims, PlanningPairOperation, PublicKeySnapshot,
    SIGNATURE_DOMAIN, VerificationPolicy, VerificationRequest, verify,
};
use super::*;

struct Fixture {
    verified: VerifiedAttestation,
    key: PublicKeyEntry,
}

fn fixture() -> Fixture {
    static KEY: OnceLock<RsaKeyPair> = OnceLock::new();
    let key =
        KEY.get_or_init(|| RsaKeyPair::generate(KeySize::Rsa2048).expect("test-only RSA key"));
    let claims = AttestationClaims {
        schema_version: 1,
        issuer: ExternalId::parse("test-owner-issuer").expect("issuer"),
        key_id: ExternalId::parse("test-key-1").expect("key"),
        audience: APPLICATION_AUDIENCE.to_owned(),
        scope: AttestationScope {
            realm_id: RealmId::generate(),
            project_id: ProjectId::generate(),
            epic_id: MiniProjectId::generate(),
            task_id: Some(TaskId::generate()),
        },
        seat_binding_id: SeatBindingId::generate(),
        occupancy_generation: 7,
        native_identity: AttestationNativeIdentity {
            runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime"),
            host: ExternalName::parse("test-host").expect("host"),
            generation: 3,
            native_id: ExternalId::parse("test-native-session").expect("session"),
        },
        allowed_operations: vec![PlanningPairOperation::InvokePlanningPairRun],
        token_id: ExternalId::parse("test-token-1").expect("token"),
        not_before: 1_000,
        expires_at: 1_100,
    };
    let public_key = PublicKeyEntry {
        issuer: claims.issuer.clone(),
        key_id: claims.key_id.clone(),
        public_key_der: key.public_key().as_ref().to_vec(),
        not_before: 900,
        expires_at: 1_200,
        revoked: false,
    };
    let snapshot = PublicKeySnapshot {
        revision: 9,
        keys: vec![public_key.clone()],
    };
    let request = VerificationRequest {
        audience: APPLICATION_AUDIENCE,
        scope: &claims.scope,
        seat_binding_id: claims.seat_binding_id,
        occupancy_generation: claims.occupancy_generation,
        native_identity: &claims.native_identity,
        operation: PlanningPairOperation::InvokePlanningPairRun,
        now: 1_020,
        policy: VerificationPolicy {
            maximum_lifetime_seconds: 100,
        },
    };
    let payload = serde_json::to_vec(&claims).expect("claims");
    let mut signed = SIGNATURE_DOMAIN.to_vec();
    signed.extend_from_slice(&payload);
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        &signed,
        &mut signature,
    )
    .expect("test-only signature");
    let verified =
        verify(&payload, &signature, &snapshot, &request).expect("real verified fixture");
    Fixture {
        verified,
        key: public_key,
    }
}

fn observations(fixture: &Fixture) -> OwnerFacts<'_> {
    let claims = fixture.verified.claims();
    OwnerFacts {
        key_set_revision: 9,
        key: &fixture.key,
        token: TokenObservation {
            token_id: &claims.token_id,
            revoked: false,
            not_before: 1_000,
            expires_at: 1_100,
        },
        seat: SeatObservation {
            active: true,
            node_active: true,
            scope: &claims.scope,
            seat_binding_id: claims.seat_binding_id,
            occupancy_generation: 7,
            native_identity: &claims.native_identity,
        },
        operation: PlanningPairOperation::InvokePlanningPairRun,
        now: 1_040,
    }
}

#[test]
fn matching_observations_return_unit_diagnostic_consistency_only() {
    let fixture = fixture();
    let result: Result<(), OwnerCheckRefusal> =
        compare_owner_facts(&fixture.verified, &observations(&fixture));
    assert_eq!(result, Ok(()));
    // No actor, grant, readiness change, receipt or native effect is produced.
}

#[test]
fn the_current_requested_operation_must_be_signed() {
    let fixture = fixture();
    let mut facts = observations(&fixture);
    facts.operation = PlanningPairOperation::RecoverPlanningPairSeat;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::Operation)
    );
}

#[test]
fn zero_older_and_newer_key_set_revisions_are_refused() {
    let fixture = fixture();
    let mut facts = observations(&fixture);
    for revision in [0, 8, 10] {
        facts.key_set_revision = revision;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            Err(OwnerCheckRefusal::KeySetRevision),
            "revision {revision}"
        );
    }
}

#[test]
fn selected_issuer_key_and_token_substitutions_are_refused() {
    let fixture = fixture();
    let mut key = fixture.key.clone();
    key.issuer = ExternalId::parse("other-issuer").expect("issuer");
    let mut facts = observations(&fixture);
    facts.key = &key;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::KeyIdentity)
    );
    let mut key = fixture.key.clone();
    key.key_id = ExternalId::parse("other-key").expect("key");
    facts.key = &key;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::KeyIdentity)
    );
    facts = observations(&fixture);
    let token = ExternalId::parse("other-token").expect("token");
    facts.token.token_id = &token;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::TokenIdentity)
    );
}

#[test]
fn key_and_token_revocation_after_verification_are_refused() {
    let fixture = fixture();
    let mut key = fixture.key.clone();
    key.revoked = true;
    let mut facts = observations(&fixture);
    facts.key = &key;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::KeyRevoked)
    );
    facts = observations(&fixture);
    facts.token.revoked = true;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::TokenRevoked)
    );
}

#[test]
fn active_seat_and_active_node_are_both_required() {
    let fixture = fixture();
    let mut facts = observations(&fixture);
    facts.seat.active = false;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::SeatInactive)
    );
    facts = observations(&fixture);
    facts.seat.node_active = false;
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::NodeInactive)
    );
}

#[test]
fn every_current_scope_component_and_task_presence_must_match() {
    let fixture = fixture();
    let scope = &fixture.verified.claims().scope;
    let edits: &[fn(&mut AttestationScope)] = &[
        |s| s.realm_id = RealmId::generate(),
        |s| s.project_id = ProjectId::generate(),
        |s| s.epic_id = MiniProjectId::generate(),
        |s| s.task_id = Some(TaskId::generate()),
        |s| s.task_id = None,
    ];
    for edit in edits {
        let mut current = scope.clone();
        edit(&mut current);
        let mut facts = observations(&fixture);
        facts.seat.scope = &current;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            Err(OwnerCheckRefusal::Scope)
        );
    }
}

#[test]
fn current_binding_and_nonzero_occupancy_generation_must_match() {
    let fixture = fixture();
    let mut facts = observations(&fixture);
    facts.seat.seat_binding_id = SeatBindingId::generate();
    assert_eq!(
        compare_owner_facts(&fixture.verified, &facts),
        Err(OwnerCheckRefusal::SeatBinding)
    );
    facts = observations(&fixture);
    for generation in [0, 6, 8] {
        facts.seat.occupancy_generation = generation;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            Err(OwnerCheckRefusal::OccupancyGeneration),
            "generation {generation}"
        );
    }
}

#[test]
fn every_native_identity_component_and_positive_generation_must_match() {
    let fixture = fixture();
    let identity = &fixture.verified.claims().native_identity;
    let edits: &[fn(&mut AttestationNativeIdentity)] = &[
        |n| n.runtime_kind = RuntimeKindKey::parse("other-runtime").expect("runtime"),
        |n| n.host = ExternalName::parse("other-host").expect("host"),
        |n| n.generation = 0,
        |n| n.generation = 4,
        |n| n.native_id = ExternalId::parse("other-session").expect("session"),
    ];
    for edit in edits {
        let mut current = identity.clone();
        edit(&mut current);
        let mut facts = observations(&fixture);
        facts.seat.native_identity = &current;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            Err(OwnerCheckRefusal::NativeIdentity)
        );
    }
}

#[test]
fn current_key_interval_is_inclusive_at_start_and_exclusive_at_end() {
    let fixture = fixture();
    for (start, end, now, expected) in [
        (1_000, 1_100, 1_000, Ok(())),
        (1_000, 1_100, 1_099, Ok(())),
        (1_041, 1_100, 1_040, Err(OwnerCheckRefusal::KeyValidity)),
        (900, 1_040, 1_040, Err(OwnerCheckRefusal::KeyValidity)),
        (1_040, 1_040, 1_040, Err(OwnerCheckRefusal::KeyValidity)),
        (1_041, 1_040, 1_040, Err(OwnerCheckRefusal::KeyValidity)),
    ] {
        let mut key = fixture.key.clone();
        key.not_before = start;
        key.expires_at = end;
        let mut facts = observations(&fixture);
        facts.key = &key;
        facts.now = now;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            expected,
            "key {start}..{end} at {now}"
        );
    }
}

#[test]
fn signed_token_expiry_between_verification_and_observation_is_refused() {
    let fixture = fixture(); // Cryptographically verified at 1020.
    let mut facts = observations(&fixture);
    // A wider ledger interval cannot extend the signed token's validity.
    facts.token.not_before = 900;
    facts.token.expires_at = 1_200;
    for (now, expected) in [
        (999, Err(OwnerCheckRefusal::TokenValidity)),
        (1_000, Ok(())),
        (1_099, Ok(())),
        (1_100, Err(OwnerCheckRefusal::TokenValidity)),
        (1_101, Err(OwnerCheckRefusal::TokenValidity)),
    ] {
        facts.now = now;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            expected,
            "signed token at {now}"
        );
    }
}

#[test]
fn current_token_interval_can_narrow_but_never_bypass_signed_validity() {
    let fixture = fixture();
    for (start, end, now, expected) in [
        (1_030, 1_050, 1_030, Ok(())),
        (1_030, 1_050, 1_049, Ok(())),
        (1_030, 1_050, 1_050, Err(OwnerCheckRefusal::TokenValidity)),
        (1_041, 1_100, 1_040, Err(OwnerCheckRefusal::TokenValidity)),
        (1_040, 1_040, 1_040, Err(OwnerCheckRefusal::TokenValidity)),
        (1_041, 1_040, 1_040, Err(OwnerCheckRefusal::TokenValidity)),
    ] {
        let mut facts = observations(&fixture);
        facts.token.not_before = start;
        facts.token.expires_at = end;
        facts.now = now;
        assert_eq!(
            compare_owner_facts(&fixture.verified, &facts),
            expected,
            "observed token {start}..{end} at {now}"
        );
    }
}
