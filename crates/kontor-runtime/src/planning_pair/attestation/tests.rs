use std::sync::OnceLock;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair};

use super::*;

fn signing_key() -> &'static RsaKeyPair {
    static KEY: OnceLock<RsaKeyPair> = OnceLock::new();
    KEY.get_or_init(|| RsaKeyPair::generate(KeySize::Rsa2048).expect("test-only RSA key"))
}

fn claims() -> AttestationClaims {
    AttestationClaims {
        schema_version: 1,
        issuer: ExternalId::parse("dedicated-test-issuer").expect("issuer"),
        key_id: ExternalId::parse("test-key-1").expect("key id"),
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
            runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime kind"),
            host: ExternalName::parse("test-host").expect("host"),
            generation: 3,
            native_id: ExternalId::parse("native-test-session").expect("native id"),
        },
        allowed_operations: vec![PlanningPairOperation::InvokePlanningPairRun],
        token_id: ExternalId::parse("test-token-1").expect("token id"),
        not_before: 1_000,
        expires_at: 1_060,
    }
}

fn snapshot(claims: &AttestationClaims) -> PublicKeySnapshot {
    PublicKeySnapshot {
        revision: 9,
        keys: vec![PublicKeyEntry {
            issuer: claims.issuer.clone(),
            key_id: claims.key_id.clone(),
            public_key_der: signing_key().public_key().as_ref().to_vec(),
            not_before: 100,
            expires_at: 5_000,
            revoked: false,
        }],
    }
}

fn request(claims: &AttestationClaims) -> VerificationRequest<'_> {
    VerificationRequest {
        audience: APPLICATION_AUDIENCE,
        scope: &claims.scope,
        seat_binding_id: claims.seat_binding_id,
        occupancy_generation: claims.occupancy_generation,
        native_identity: &claims.native_identity,
        operation: PlanningPairOperation::InvokePlanningPairRun,
        now: 1_030,
        policy: VerificationPolicy {
            maximum_lifetime_seconds: 60,
        },
    }
}

fn sign(payload: &[u8], domain: &[u8]) -> Vec<u8> {
    let key = signing_key();
    let mut message = domain.to_vec();
    message.extend_from_slice(payload);
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        &message,
        &mut signature,
    )
    .expect("test-only signature");
    signature
}

fn encoded(claims: &AttestationClaims) -> (Vec<u8>, Vec<u8>) {
    let payload = serde_json::to_vec(claims).expect("typed claims");
    let signature = sign(&payload, SIGNATURE_DOMAIN);
    (payload, signature)
}

fn refusal(
    claims: &AttestationClaims,
    keys: &PublicKeySnapshot,
    request: &VerificationRequest<'_>,
) -> AttestationRefusal {
    let (payload, signature) = encoded(claims);
    verify(&payload, &signature, keys, request).expect_err("must refuse")
}

#[test]
fn valid_signature_returns_only_claims_and_snapshot_revision() {
    let claims = claims();
    let keys = snapshot(&claims);
    let (payload, signature) = encoded(&claims);
    let verified = verify(&payload, &signature, &keys, &request(&claims)).expect("verified");
    assert_eq!(verified.claims(), &claims);
    assert_eq!(verified.snapshot_revision(), 9);
    // No owner application is called and no authority is returned.
}

#[test]
fn exact_bytes_signature_and_domain_are_required() {
    let claims = claims();
    let keys = snapshot(&claims);
    let req = request(&claims);
    let (mut payload, mut signature) = encoded(&claims);
    payload.push(b' '); // Valid equivalent JSON, different signed bytes.
    assert_eq!(
        verify(&payload, &signature, &keys, &req).expect_err("tampered payload"),
        AttestationRefusal::Signature
    );
    payload.pop();
    signature[0] ^= 1;
    assert_eq!(
        verify(&payload, &signature, &keys, &req).expect_err("tampered signature"),
        AttestationRefusal::Signature
    );
    let wrong_domain = sign(&payload, b"");
    assert_eq!(
        verify(&payload, &wrong_domain, &keys, &req).expect_err("wrong domain"),
        AttestationRefusal::Signature
    );
}

#[test]
fn unknown_issuer_key_and_wrong_public_key_are_refused() {
    let claims = claims();
    let mut keys = snapshot(&claims);
    let req = request(&claims);
    keys.keys[0].issuer = ExternalId::parse("other-issuer").expect("issuer");
    assert_eq!(
        refusal(&claims, &keys, &req),
        AttestationRefusal::UnknownKey
    );
    keys = snapshot(&claims);
    keys.keys[0].key_id = ExternalId::parse("other-key").expect("key");
    assert_eq!(
        refusal(&claims, &keys, &req),
        AttestationRefusal::UnknownKey
    );
    keys = snapshot(&claims);
    let wrong = RsaKeyPair::generate(KeySize::Rsa2048).expect("other test-only key");
    keys.keys[0].public_key_der = wrong.public_key().as_ref().to_vec();
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Signature);
}

#[test]
fn revoked_and_invalid_key_intervals_are_refused() {
    let claims = claims();
    let req = request(&claims);
    let mut keys = snapshot(&claims);
    keys.keys[0].revoked = true;
    assert_eq!(
        refusal(&claims, &keys, &req),
        AttestationRefusal::RevokedKey
    );
    for (start, end) in [(1_031, 5_000), (100, 1_030), (1_001, 5_000), (100, 1_059)] {
        keys = snapshot(&claims);
        keys.keys[0].not_before = start;
        keys.keys[0].expires_at = end;
        assert_eq!(
            refusal(&claims, &keys, &req),
            AttestationRefusal::KeyValidity,
            "key interval {start}..{end}"
        );
    }
    keys = snapshot(&claims);
    keys.keys[0].not_before = claims.not_before;
    keys.keys[0].expires_at = claims.expires_at;
    let (payload, signature) = encoded(&claims);
    assert!(
        verify(&payload, &signature, &keys, &req).is_ok(),
        "exact containing key interval"
    );
}

#[test]
fn audience_and_ungranted_operation_are_refused() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let mut claims = original.clone();
    claims.audience = "asma.other.application.v1".to_owned();
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Audience);
    let mut req = request(&original);
    req.operation = PlanningPairOperation::RecoverPlanningPairSeat;
    assert_eq!(
        refusal(&original, &keys, &req),
        AttestationRefusal::Operation
    );
}

#[test]
fn every_scope_component_must_match_exactly() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let mut scopes = Vec::new();
    let mut scope = original.scope.clone();
    scope.realm_id = RealmId::generate();
    scopes.push(scope);
    let mut scope = original.scope.clone();
    scope.project_id = ProjectId::generate();
    scopes.push(scope);
    let mut scope = original.scope.clone();
    scope.epic_id = MiniProjectId::generate();
    scopes.push(scope);
    let mut scope = original.scope.clone();
    scope.task_id = Some(TaskId::generate());
    scopes.push(scope);
    let mut scope = original.scope.clone();
    scope.task_id = None;
    scopes.push(scope);
    for scope in scopes {
        let mut altered = original.clone();
        altered.scope = scope;
        assert_eq!(refusal(&altered, &keys, &req), AttestationRefusal::Scope);
    }
}

#[test]
fn seat_and_generation_must_match_exactly() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let mut claims = original.clone();
    claims.seat_binding_id = SeatBindingId::generate();
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Seat);
    claims = original.clone();
    claims.occupancy_generation += 1;
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Seat);
}

#[test]
fn every_native_identity_component_must_match_exactly() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let mut identities = Vec::new();
    let mut identity = original.native_identity.clone();
    identity.runtime_kind = RuntimeKindKey::parse("other-runtime").expect("runtime");
    identities.push(identity);
    let mut identity = original.native_identity.clone();
    identity.host = ExternalName::parse("other-host").expect("host");
    identities.push(identity);
    let mut identity = original.native_identity.clone();
    identity.generation += 1;
    identities.push(identity);
    let mut identity = original.native_identity.clone();
    identity.native_id = ExternalId::parse("other-session").expect("session");
    identities.push(identity);
    for identity in identities {
        let mut claims = original.clone();
        claims.native_identity = identity;
        assert_eq!(
            refusal(&claims, &keys, &req),
            AttestationRefusal::NativeIdentity
        );
    }
}

#[test]
fn token_interval_is_inclusive_at_start_exclusive_at_expiry_and_policy_bounded() {
    let claims = claims();
    let keys = snapshot(&claims);
    let (payload, signature) = encoded(&claims);
    let mut req = request(&claims);
    for now in [999, 1_060, 1_061] {
        req.now = now;
        assert_eq!(
            verify(&payload, &signature, &keys, &req).expect_err("outside token interval"),
            AttestationRefusal::Validity,
            "now {now}"
        );
    }
    for now in [1_000, 1_059] {
        req.now = now;
        assert!(
            verify(&payload, &signature, &keys, &req).is_ok(),
            "now {now}"
        );
    }
    req.now = 1_030;
    req.policy.maximum_lifetime_seconds = 59;
    assert_eq!(
        verify(&payload, &signature, &keys, &req).expect_err("lifetime exceeds explicit policy"),
        AttestationRefusal::Validity
    );
}

#[test]
fn malformed_claims_and_zero_generations_are_refused() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let edits: &[fn(&mut AttestationClaims)] = &[
        |c| c.schema_version = 2,
        |c| c.occupancy_generation = 0,
        |c| c.native_identity.generation = 0,
        |c| c.allowed_operations.clear(),
        |c| c.allowed_operations.push(c.allowed_operations[0]),
        |c| c.expires_at = c.not_before,
        |c| c.expires_at = c.not_before - 1,
    ];
    for edit in edits {
        let mut claims = original.clone();
        edit(&mut claims);
        assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Claims);
    }
}

#[test]
fn unknown_duplicate_fields_and_invalid_ids_are_refused_without_defaults() {
    let original = claims();
    let keys = snapshot(&original);
    let req = request(&original);
    let value = serde_json::to_value(&original).expect("claims JSON");
    let edits: &[fn(&mut serde_json::Value)] = &[
        |v| v["unknown"] = true.into(),
        |v| v["scope"]["unknown"] = true.into(),
        |v| v["native_identity"]["unknown"] = true.into(),
        |v| v["scope"]["realm_id"] = "not-a-uuid".into(),
        |v| v["scope"]["project_id"] = "not-a-uuid".into(),
        |v| v["scope"]["epic_id"] = "not-a-uuid".into(),
        |v| v["scope"]["task_id"] = "not-a-uuid".into(),
        |v| v["seat_binding_id"] = "not-a-uuid".into(),
        |v| v["issuer"] = "".into(),
        |v| v["key_id"] = "".into(),
        |v| v["token_id"] = "".into(),
        |v| v["native_identity"]["native_id"] = "".into(),
        |v| v["allowed_operations"] = serde_json::json!(["apply_planning_pair_profile"]),
        |v| {
            v.as_object_mut().expect("object").remove("token_id");
        },
    ];
    for edit in edits {
        let mut value = value.clone();
        edit(&mut value);
        let payload = serde_json::to_vec(&value).expect("JSON");
        let signature = sign(&payload, SIGNATURE_DOMAIN);
        assert_eq!(
            verify(&payload, &signature, &keys, &req).expect_err("malformed claims"),
            AttestationRefusal::Payload
        );
    }
    let base =
        String::from_utf8(serde_json::to_vec(&original).expect("claims JSON")).expect("UTF8");
    let duplicates = [
        base.replacen("{", "{\"schema_version\":1,", 1),
        base.replacen("\"scope\":{", "\"scope\":{\"task_id\":null,", 1),
        base.replacen(
            "\"native_identity\":{",
            "\"native_identity\":{\"generation\":3,",
            1,
        ),
    ];
    for payload in duplicates {
        let signature = sign(payload.as_bytes(), SIGNATURE_DOMAIN);
        assert_eq!(
            verify(payload.as_bytes(), &signature, &keys, &req).expect_err("duplicate field"),
            AttestationRefusal::Payload
        );
    }
}

#[test]
fn bounds_are_checked_before_parsing_or_signature_verification() {
    let claims = claims();
    let keys = snapshot(&claims);
    let req = request(&claims);
    assert_eq!(
        verify(&vec![b'!'; MAX_PAYLOAD_BYTES + 1], &[0; 256], &keys, &req)
            .expect_err("oversized malformed payload"),
        AttestationRefusal::Size
    );
    for signature_length in [0, 255, 1_025] {
        assert_eq!(
            verify(b"not JSON", &vec![0; signature_length], &keys, &req)
                .expect_err("signature size checked first"),
            AttestationRefusal::Size
        );
    }
    let (mut payload, _) = encoded(&claims);
    payload.resize(MAX_PAYLOAD_BYTES, b' ');
    let signature = sign(&payload, SIGNATURE_DOMAIN);
    assert!(
        verify(&payload, &signature, &keys, &req).is_ok(),
        "exact payload ceiling accepted"
    );
    assert_eq!(
        verify(b"", &signature, &keys, &req).expect_err("empty payload"),
        AttestationRefusal::Size
    );
}

#[test]
fn snapshot_ambiguity_invalid_revision_and_key_bounds_are_refused() {
    let claims = claims();
    let req = request(&claims);
    let edits: &[fn(&mut PublicKeySnapshot)] = &[
        |k| k.revision = 0,
        |k| k.keys.clear(),
        |k| k.keys.push(k.keys[0].clone()),
        |k| k.keys[0].public_key_der.clear(),
        |k| k.keys[0].public_key_der.resize(MAX_PUBLIC_KEY_BYTES + 1, 0),
        |k| k.keys[0].expires_at = k.keys[0].not_before,
        |k| k.keys.resize(MAX_KEYS + 1, k.keys[0].clone()),
    ];
    for edit in edits {
        let mut keys = snapshot(&claims);
        edit(&mut keys);
        assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Snapshot);
    }
}

#[test]
fn explicit_policy_audience_and_expected_generations_must_be_valid() {
    let claims = claims();
    let keys = snapshot(&claims);
    let mut req = request(&claims);
    req.policy.maximum_lifetime_seconds = 0;
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Request);
    req = request(&claims);
    req.audience = "other-audience";
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Request);
    req = request(&claims);
    req.occupancy_generation = 0;
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Request);
    let mut native = claims.native_identity.clone();
    native.generation = 0;
    req = request(&claims);
    req.native_identity = &native;
    assert_eq!(refusal(&claims, &keys, &req), AttestationRefusal::Request);
}

#[test]
fn all_and_only_the_six_existing_command_names_are_supported() {
    let mut claims = claims();
    claims.scope.task_id = None;
    claims.allowed_operations = vec![
        PlanningPairOperation::InvokePlanningPairRun,
        PlanningPairOperation::RecordPlanningPairFinding,
        PlanningPairOperation::RequestPlanningPairClarification,
        PlanningPairOperation::RecordPlanningPairAnswer,
        PlanningPairOperation::RecordPlanningPairDisposition,
        PlanningPairOperation::RecoverPlanningPairSeat,
    ];
    let expected = [
        "invoke_planning_pair_run",
        "record_planning_pair_finding",
        "request_planning_pair_clarification",
        "record_planning_pair_answer",
        "record_planning_pair_disposition",
        "recover_planning_pair_seat",
    ];
    let keys = snapshot(&claims);
    let (payload, signature) = encoded(&claims);
    for (operation, name) in claims.allowed_operations.iter().zip(expected) {
        assert_eq!(serde_json::to_value(operation).expect("operation"), name);
        let mut req = request(&claims);
        req.operation = *operation;
        assert!(verify(&payload, &signature, &keys, &req).is_ok(), "{name}");
    }
    assert!(
        serde_json::from_str::<PlanningPairOperation>("\"apply_planning_pair_profile\"").is_err()
    );
}
