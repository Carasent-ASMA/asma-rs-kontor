//! Durable prepared commitments and exact caller expectations; no live proof.
#[path = "attestation_token_metadata/fixture.rs"]
mod fixture;
#[path = "attestation_token_metadata/guards.rs"]
mod guards;
mod support;

use fixture::*;
use kontor_core::id::{
    AggregateRevision, ContentHash, MiniProjectId, ProjectId, RuntimeKindKey, SeatBindingId,
};
use kontor_core::repository::{
    AttestationAuthorityRepository, AttestationSeatProvenance, RegisterAttestationKey,
    RepositoryError,
};
use kontor_store::SqliteStore;

#[test]
fn hosted_preparation_is_coherent_durable_and_does_not_advance_key_head() {
    let f = Fixture::build();
    let request = f.request();
    assert!(
        f.store
            .read_prepared_attestation_token(&f.scope, &request.issuer, &request.token_id)
            .expect("read")
            .is_none()
    );
    let projection = f
        .store
        .prepare_attestation_token(&request)
        .expect("prepare");
    let token = projection.selected_token.as_ref().expect("token");
    assert_eq!(projection.head_revision, 1);
    assert_eq!(token.provenance, AttestationSeatProvenance::Hosted);
    assert_eq!(token.topology_node_id, f.node);
    assert_eq!(token.native_identity, request.expected_native_identity);
    assert_eq!(token.payload_digest, request.payload_digest);
    assert_eq!(token.key_material_digest, ContentHash::of(&[1, 2, 3]));
    assert_eq!(
        (
            token.key_registered_revision,
            token.preparation_key_head_revision,
            token.occupancy_generation
        ),
        (1, 1, 1)
    );
    assert_eq!(
        (token.binding_revision, token.node_revision),
        (AggregateRevision::INITIAL, AggregateRevision::INITIAL)
    );
    assert_eq!(
        f.store
            .read_attestation_key_authority(&f.scope, &request.issuer, &request.key_id)
            .expect("key")
            .expect("head")
            .head_revision,
        1
    );
    let reopened = SqliteStore::open(&f.home.path().join("kontor.db")).expect("reopen");
    assert_eq!(
        reopened
            .read_prepared_attestation_token(&f.scope, &request.issuer, &request.token_id)
            .expect("read"),
        Some(projection)
    );
    assert!(
        reopened
            .read_prepared_attestation_token(&f.scope, &request.issuer, &id("missing"))
            .expect("read")
            .expect("head")
            .selected_token
            .is_none()
    );
}

#[test]
fn both_heads_use_exact_cas_and_failed_preparation_rolls_back() {
    let f = Fixture::build();
    let base = f.request();
    for head in [Some(1), Some(2)] {
        let mut r = base.clone();
        r.expected_token_head_revision = head;
        f.refuse_unchanged(&r);
    }
    for head in [0, 2] {
        let mut r = base.clone();
        r.expected_key_head_revision = head;
        f.refuse_unchanged(&r);
    }
    f.store.prepare_attestation_token(&base).expect("prepare");
    let mut r = base;
    r.token_id = id("token-2");
    f.refuse_unchanged(&r);
    r.expected_token_head_revision = Some(2);
    f.refuse_unchanged(&r);
    r.expected_token_head_revision = Some(1);
    assert_eq!(
        f.store
            .prepare_attestation_token(&r)
            .expect("next")
            .head_revision,
        2
    );
    assert_eq!(f.counts(), (2, 1, 2));
}

#[test]
fn permanent_issuer_token_identity_refuses_even_identical_reuse_across_rotation() {
    let f = Fixture::build();
    let mut r = f.request();
    f.store.prepare_attestation_token(&r).expect("prepare");
    r.expected_token_head_revision = Some(1);
    assert!(matches!(
        f.refuse_unchanged(&r),
        RepositoryError::Conflict {
            rule: "issuer/token identity is permanently used",
            ..
        }
    ));
    f.store
        .register_attestation_key(&RegisterAttestationKey {
            scope: f.scope.clone(),
            expected_head_revision: Some(1),
            issuer: r.issuer.clone(),
            key_id: id("key-2"),
            public_key_der: vec![9],
            not_before: 0,
            expires_at: 2000,
        })
        .expect("rotation");
    r.expected_key_head_revision = 2;
    r.key_id = id("key-2");
    f.refuse_unchanged(&r);
    r.token_id = id("token-2");
    let p = f.store.prepare_attestation_token(&r).expect("new identity");
    let t = p.selected_token.expect("token");
    assert_eq!(
        (t.key_registered_revision, t.preparation_key_head_revision),
        (2, 2)
    );
    // An older, still-unrevoked selected key stays eligible under the current
    // key head. Its registration revision remains historical provenance.
    r.token_id = id("token-3");
    r.key_id = id("key-1");
    r.expected_token_head_revision = Some(2);
    let t = f
        .store
        .prepare_attestation_token(&r)
        .expect("older unrevoked key")
        .selected_token
        .expect("token");
    assert_eq!(
        (
            t.key_registered_revision,
            t.preparation_key_head_revision,
            t.registered_revision
        ),
        (1, 2, 3)
    );
}

#[test]
fn fresh_checks_precede_identity_reuse_and_leave_existing_commitment_unchanged() {
    let f = Fixture::build();
    let mut r = f.request();
    let before = f.store.prepare_attestation_token(&r).expect("prepare");
    r.expected_token_head_revision = Some(1);
    r.expected_occupancy_generation = 2;
    assert!(matches!(
        f.refuse_unchanged(&r),
        RepositoryError::Conflict {
            rule: "hosted generation or native metadata does not match",
            ..
        }
    ));
    r.expected_occupancy_generation = 1;
    r.expected_key_head_revision = 2;
    assert!(matches!(
        f.refuse_unchanged(&r),
        RepositoryError::Conflict {
            rule: "expected head revision does not match",
            ..
        }
    ));
    assert_eq!(
        f.store
            .read_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id)
            .expect("read"),
        Some(before)
    );
}

#[test]
fn irreversible_revocation_remains_available_after_key_revocation_and_seat_retirement() {
    let f = Fixture::build();
    let r = f.request();
    f.store.prepare_attestation_token(&r).expect("prepare");
    f.store
        .revoke_attestation_key(&f.scope, &r.issuer, &r.key_id, 1)
        .expect("revoke key");
    f.sql()
        .execute(
            "UPDATE seat_bindings SET lifecycle='retired' WHERE id=?1",
            [f.seat.to_string()],
        )
        .expect("retire");
    assert!(
        f.store
            .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 2)
            .is_err()
    );
    assert_eq!(f.counts(), (1, 1, 1));
    let revoked = f
        .store
        .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 1)
        .expect("revoke");
    assert_eq!(revoked.head_revision, 2);
    assert_eq!(
        revoked
            .selected_token
            .as_ref()
            .expect("token")
            .revoked_revision,
        Some(2)
    );
    assert_eq!(
        f.store
            .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 2)
            .expect("unchanged"),
        revoked
    );
    assert!(
        f.store
            .revoke_prepared_attestation_token(&f.scope, &r.issuer, &r.token_id, 1)
            .is_err()
    );
    assert_eq!(f.counts(), (1, 1, 2));
}

#[test]
fn bounds_intervals_and_native_generation_refuse_without_token_head() {
    let f = Fixture::build();
    let base = f.request();
    let mut bad = Vec::new();
    for (start, end) in [
        (0, 11),
        (10, 1001),
        (10, 10),
        (12, 11),
        (u64::MAX, u64::MAX),
        (10, u64::MAX),
    ] {
        let mut r = base.clone();
        r.not_before = start;
        r.expires_at = end;
        bad.push(r);
    }
    for g in [0, u64::MAX] {
        let mut r = base.clone();
        r.expected_occupancy_generation = g;
        bad.push(r);
        let mut r = base.clone();
        r.expected_native_identity.generation = g;
        bad.push(r);
    }
    for which in 0..4 {
        let mut r = base.clone();
        let long = id(&"é".repeat(129));
        match which {
            0 => r.issuer = long,
            1 => r.key_id = long,
            2 => r.token_id = long,
            _ => r.expected_native_identity.native_id = long,
        };
        bad.push(r);
    }
    for r in bad {
        f.refuse_unchanged(&r);
    }
    assert_eq!(f.counts(), (0, 0, 0));
    assert!(
        f.store.prepare_attestation_token(&base).is_ok(),
        "inclusive key start and exact exclusive end are contained"
    );
}

#[test]
fn selected_key_absence_revocation_and_exact_native_components_are_checked() {
    let f = Fixture::build();
    let base = f.request();
    let mut bad = base.clone();
    bad.key_id = id("unknown");
    f.refuse_unchanged(&bad);
    for part in 0..4 {
        let mut r = base.clone();
        match part {
            0 => r.expected_native_identity.native_id = id("wrong"),
            1 => r.expected_native_identity.host = name("wrong"),
            2 => {
                r.expected_native_identity.runtime_kind =
                    RuntimeKindKey::parse("wrong").expect("kind")
            }
            _ => r.expected_native_identity.generation = 12,
        };
        f.refuse_unchanged(&r);
    }
    f.store
        .revoke_attestation_key(&f.scope, &base.issuer, &base.key_id, 1)
        .expect("revoke");
    let mut r = base;
    r.expected_key_head_revision = 2;
    f.refuse_unchanged(&r);
}

#[test]
fn scope_and_absent_task_are_exact_and_cross_scope_reads_do_not_leak() {
    let f = Fixture::build();
    let base = f.request();
    for case in 0..6 {
        let mut r = base.clone();
        match case {
            0 => r.scope.realm_id = kontor_core::id::RealmId::generate(),
            1 => r.scope.project_id = ProjectId::generate(),
            2 => r.mini_project_id = MiniProjectId::generate(),
            3 => r.task_id = Some(f.task),
            4 => r.seat_binding_id = SeatBindingId::generate(),
            _ => r.scope.application = id("other"),
        };
        f.refuse_unchanged(&r);
    }
    f.store.prepare_attestation_token(&base).expect("prepare");
    let mut scope = f.scope.clone();
    scope.realm_id = kontor_core::id::RealmId::generate();
    assert!(
        f.store
            .read_prepared_attestation_token(&scope, &base.issuer, &base.token_id)
            .expect("read")
            .is_none()
    );
    scope = f.scope.clone();
    scope.project_id = ProjectId::generate();
    assert!(
        f.store
            .read_prepared_attestation_token(&scope, &base.issuer, &base.token_id)
            .expect("read")
            .is_none()
    );
}
