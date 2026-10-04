use std::sync::OnceLock;

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::rsa::KeySize;
use aws_lc_rs::signature::{KeyPair as _, RSA_PKCS1_SHA256, RsaKeyPair};
use kontor_core::id::{
    AggregateRevision, ExternalId, ExternalName, MiniProjectId, ProjectId, RealmId, RuntimeKindKey,
    SeatBindingId, TaskId, TopologyNodeId,
};
use kontor_core::repository::attestation_authority::{
    AttestationAuthorityScope, AttestationSeatProvenance,
};
use kontor_core::state::NativeRuntimeIdentity;

use super::*;
use crate::planning_pair::attestation::{
    AttestationNativeIdentity, AttestationScope, PlanningPairOperation, SIGNATURE_DOMAIN,
    VerificationPolicy,
};

fn signing_key() -> &'static RsaKeyPair {
    static KEY: OnceLock<RsaKeyPair> = OnceLock::new();
    KEY.get_or_init(|| RsaKeyPair::generate(KeySize::Rsa2048).expect("test-only RSA key"))
}

fn sign(payload: &[u8]) -> Vec<u8> {
    let key = signing_key();
    let mut message = SIGNATURE_DOMAIN.to_vec();
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

#[derive(Clone)]
struct Fixture {
    claims: AttestationClaims,
    key: AttestationAuthorityProjection,
    token: AttestationTokenProjection,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let scope = AttestationScope {
            realm_id: RealmId::generate(),
            project_id: ProjectId::generate(),
            epic_id: MiniProjectId::generate(),
            task_id: Some(TaskId::generate()),
        };
        let claims = AttestationClaims {
            schema_version: 1,
            issuer: id("fixture-issuer"),
            key_id: id("fixture-key"),
            audience: APPLICATION_AUDIENCE.to_owned(),
            scope: scope.clone(),
            seat_binding_id: SeatBindingId::generate(),
            occupancy_generation: 7,
            native_identity: AttestationNativeIdentity {
                runtime_kind: RuntimeKindKey::parse("paseo").expect("kind"),
                host: ExternalName::parse("fixture-host").expect("host"),
                generation: 3,
                native_id: id("fixture-session"),
            },
            allowed_operations: vec![PlanningPairOperation::InvokePlanningPairRun],
            token_id: id("fixture-token"),
            not_before: 1_000,
            expires_at: 1_060,
        };
        let public_key_der = signing_key().public_key().as_ref().to_vec();
        let key = StoredAttestationKey {
            issuer: claims.issuer.clone(),
            key_id: claims.key_id.clone(),
            material_digest: ContentHash::of(&public_key_der),
            public_key_der,
            not_before: 100,
            expires_at: 5_000,
            registered_revision: 2,
            revoked_revision: None,
        };
        let payload = serde_json::to_vec(&claims).expect("claims");
        let token = StoredPreparedAttestationToken {
            issuer: claims.issuer.clone(),
            key_id: claims.key_id.clone(),
            token_id: claims.token_id.clone(),
            payload_digest: ContentHash::of(&payload),
            key_material_digest: key.material_digest.clone(),
            key_registered_revision: 2,
            preparation_key_head_revision: 4,
            not_before: claims.not_before,
            expires_at: claims.expires_at,
            mini_project_id: scope.epic_id,
            task_id: scope.task_id,
            seat_binding_id: claims.seat_binding_id,
            topology_node_id: TopologyNodeId::generate(),
            binding_revision: AggregateRevision::INITIAL,
            node_revision: AggregateRevision::INITIAL,
            provenance: AttestationSeatProvenance::Hosted,
            occupancy_generation: claims.occupancy_generation,
            native_identity: NativeRuntimeIdentity {
                runtime_kind: claims.native_identity.runtime_kind.clone(),
                host: claims.native_identity.host.clone(),
                generation: claims.native_identity.generation,
                native_id: claims.native_identity.native_id.clone(),
            },
            registered_revision: 3,
            revoked_revision: None,
        };
        let ledger_scope = AttestationAuthorityScope {
            realm_id: scope.realm_id,
            project_id: scope.project_id,
            application: id(APPLICATION_AUDIENCE),
        };
        let signature = sign(&payload);
        Self {
            claims,
            key: AttestationAuthorityProjection {
                scope: ledger_scope.clone(),
                head_revision: 8,
                selected_key: Some(key),
            },
            token: AttestationTokenProjection {
                scope: ledger_scope,
                head_revision: 9,
                selected_token: Some(token),
            },
            payload,
            signature,
        }
    }

    fn request(&self) -> VerificationRequest<'_> {
        VerificationRequest {
            audience: APPLICATION_AUDIENCE,
            scope: &self.claims.scope,
            seat_binding_id: self.claims.seat_binding_id,
            occupancy_generation: self.claims.occupancy_generation,
            native_identity: &self.claims.native_identity,
            operation: PlanningPairOperation::InvokePlanningPairRun,
            now: 1_030,
            policy: VerificationPolicy {
                maximum_lifetime_seconds: 60,
            },
        }
    }

    fn diagnose(&self) -> Result<(), PreparedDiagnosticRefusal> {
        diagnose_prepared_attestation(
            &self.payload,
            &self.signature,
            &self.key,
            &self.token,
            &self.request(),
        )
    }

    fn key_mut(&mut self) -> &mut StoredAttestationKey {
        self.key.selected_key.as_mut().expect("key")
    }
    fn token_mut(&mut self) -> &mut StoredPreparedAttestationToken {
        self.token.selected_token.as_mut().expect("token")
    }

    fn replace_payload(&mut self, payload: Vec<u8>) {
        self.signature = sign(&payload);
        self.token_mut().payload_digest = ContentHash::of(&payload);
        self.payload = payload;
    }
}

fn id(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("typed fixture id")
}

#[test]
fn consistency_returns_unit_with_historical_heads_and_no_current_provenance_claim() {
    let mut f = Fixture::new();
    assert_eq!(f.diagnose(), Ok(()));
    f.key.head_revision = 100;
    f.token.head_revision = 200;
    f.token_mut().topology_node_id = TopologyNodeId::generate();
    f.token_mut().binding_revision = AggregateRevision::parse(91).expect("revision");
    f.token_mut().node_revision = AggregateRevision::parse(92).expect("revision");
    assert_eq!(f.diagnose(), Ok(())); // History is not checked as current facts.
    f.claims.scope.task_id = None;
    f.token_mut().task_id = None;
    f.replace_payload(serde_json::to_vec(&f.claims).expect("claims"));
    assert_eq!(f.diagnose(), Ok(()));
    f.token_mut().task_id = Some(TaskId::generate());
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::Metadata));
}

#[test]
fn size_precedes_scope_absence_and_parsing() {
    let f = Fixture::new();
    let mut key = f.key.clone();
    key.scope.project_id = ProjectId::generate();
    key.selected_key = None;
    for payload in [vec![], vec![b'x'; MAX_PAYLOAD_BYTES + 1]] {
        assert_eq!(
            diagnose_prepared_attestation(&payload, &f.signature, &key, &f.token, &f.request()),
            Err(PreparedDiagnosticRefusal::Size)
        );
    }
    for size in [0, 255, 1_025] {
        assert_eq!(
            diagnose_prepared_attestation(&f.payload, &vec![0; size], &key, &f.token, &f.request()),
            Err(PreparedDiagnosticRefusal::Size)
        );
    }
}

#[test]
fn scopes_and_fixed_application_precede_absent_rows() {
    let f = Fixture::new();
    for field in 0..4 {
        let mut x = f.clone();
        match field {
            0 => {
                x.key.scope.realm_id = RealmId::generate();
                x.token.scope = x.key.scope.clone();
            }
            1 => {
                x.key.scope.project_id = ProjectId::generate();
                x.token.scope = x.key.scope.clone();
            }
            2 => {
                x.key.scope.application = id("other-app");
                x.token.scope = x.key.scope.clone();
            }
            _ => x.token.scope.application = id("other-app"),
        }
        x.key.selected_key = None;
        x.token.selected_token = None;
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Scope));
    }
    let mut req = f.request();
    req.audience = "other-app";
    assert_eq!(
        diagnose_prepared_attestation(&f.payload, &f.signature, &f.key, &f.token, &req),
        Err(PreparedDiagnosticRefusal::Scope)
    );
}

#[test]
fn selected_key_then_token_presence_precedes_bad_heads() {
    let mut f = Fixture::new();
    f.key.head_revision = 0;
    f.token.head_revision = 0;
    let saved = f.key.selected_key.take();
    f.token.selected_token = None;
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::KeyMissing));
    f.key.selected_key = saved;
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::TokenMissing));
}

#[test]
fn projection_revisions_and_revocation_order_are_validated_before_commitments() {
    let f = Fixture::new();
    for field in 0..15 {
        let mut x = f.clone();
        match field {
            0 => x.key.head_revision = 0,
            1 => x.token.head_revision = 0,
            2 => x.key_mut().registered_revision = 0,
            3 => x.key_mut().registered_revision = 9,
            4 => x.token_mut().registered_revision = 0,
            5 => x.token_mut().registered_revision = 10,
            6 => x.token_mut().key_registered_revision = 0,
            7 => x.token_mut().key_registered_revision = 9,
            8 => x.token_mut().preparation_key_head_revision = 0,
            9 => x.token_mut().preparation_key_head_revision = 1,
            10 => x.token_mut().preparation_key_head_revision = 9,
            11 => x.key_mut().revoked_revision = Some(2),
            12 => x.key_mut().revoked_revision = Some(9),
            13 => x.token_mut().revoked_revision = Some(3),
            _ => x.token_mut().revoked_revision = Some(10),
        }
        x.token_mut().payload_digest = ContentHash::of(b"wrong");
        assert_eq!(
            x.diagnose(),
            Err(PreparedDiagnosticRefusal::Projection),
            "field {field}"
        );
    }
}

#[test]
fn selected_der_and_utf8_identifier_bounds_precede_hashing() {
    let f = Fixture::new();
    for der in [vec![], vec![0; 2_049]] {
        let mut x = f.clone();
        x.key_mut().public_key_der = der;
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Projection));
    }
    let oversized = id(&"é".repeat(129)); //258 UTF-8 bytes, not129.
    for field in 0..5 {
        let mut x = f.clone();
        match field {
            0 => x.key_mut().issuer = oversized.clone(),
            1 => x.key_mut().key_id = oversized.clone(),
            2 => x.token_mut().issuer = oversized.clone(),
            3 => x.token_mut().key_id = oversized.clone(),
            _ => x.token_mut().token_id = oversized.clone(),
        }
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Projection));
    }
    let mut x = f.clone();
    x.token_mut().token_id = id(&"é".repeat(128));
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Metadata));
    let mut x = f.clone();
    let boundary = id(&"é".repeat(128));
    x.claims.issuer = boundary.clone();
    x.claims.key_id = boundary.clone();
    x.claims.token_id = boundary.clone();
    x.key_mut().issuer = boundary.clone();
    x.key_mut().key_id = boundary.clone();
    x.token_mut().issuer = boundary.clone();
    x.token_mut().key_id = boundary.clone();
    x.token_mut().token_id = boundary;
    x.replace_payload(serde_json::to_vec(&x.claims).expect("claims"));
    assert_eq!(x.diagnose(), Ok(()));
    let mut x = f.clone();
    x.key_mut().public_key_der = vec![0; 2_048];
    let digest = ContentHash::of(&x.key_mut().public_key_der);
    x.key_mut().material_digest = digest.clone();
    x.token_mut().key_material_digest = digest;
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
    x.key_mut().public_key_der = vec![0];
    let digest = ContentHash::of(&x.key_mut().public_key_der);
    x.key_mut().material_digest = digest.clone();
    x.token_mut().key_material_digest = digest;
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}

#[test]
fn key_material_commitments_are_recomputed_from_selected_der() {
    let f = Fixture::new();
    for field in 0..3 {
        let mut x = f.clone();
        match field {
            0 => x.key_mut().material_digest = ContentHash::of(b"other"),
            1 => x.token_mut().key_material_digest = ContentHash::of(b"other"),
            _ => x.key_mut().public_key_der[0] ^= 1,
        }
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::KeyCommitment));
    }
    let mut x = f.clone();
    let other = RsaKeyPair::generate(KeySize::Rsa2048).expect("test-only other key");
    x.key_mut().public_key_der = other.public_key().as_ref().to_vec();
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::KeyCommitment));
    let digest = ContentHash::of(&x.key_mut().public_key_der);
    x.key_mut().material_digest = digest.clone();
    x.token_mut().key_material_digest = digest;
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}

#[test]
fn selected_key_identity_and_immutable_registration_must_match_preparation() {
    let f = Fixture::new();
    for field in 0..3 {
        let mut x = f.clone();
        match field {
            0 => x.token_mut().issuer = id("other-issuer"),
            1 => x.token_mut().key_id = id("other-key"),
            _ => x.token_mut().key_registered_revision = 1,
        }
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::KeyCommitment));
    }
}

#[test]
fn exact_original_payload_digest_precedes_revocation_and_verification() {
    let mut f = Fixture::new();
    f.payload.push(b' ');
    assert_eq!(
        f.diagnose(),
        Err(PreparedDiagnosticRefusal::TokenCommitment)
    );
    f.token_mut().revoked_revision = Some(4);
    assert_eq!(
        f.diagnose(),
        Err(PreparedDiagnosticRefusal::TokenCommitment)
    );
    f.token_mut().payload_digest = ContentHash::of(&f.payload);
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::TokenRevoked));
    f.token_mut().revoked_revision = None;
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}

#[test]
fn token_revocation_precedes_interval_and_signature_checks() {
    let mut f = Fixture::new();
    f.token_mut().revoked_revision = Some(4);
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::TokenRevoked));
    f.token_mut().expires_at = 0;
    f.signature[0] ^= 1;
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::TokenRevoked));
}

#[test]
fn key_revocation_is_refused_by_existing_verifier() {
    let mut f = Fixture::new();
    f.key_mut().revoked_revision = Some(3);
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}

#[test]
fn invalid_or_uncontained_prepared_intervals_precede_verification() {
    let f = Fixture::new();
    for field in 0..5 {
        let mut x = f.clone();
        match field {
            0 => x.token_mut().expires_at = 1_000,
            1 => x.token_mut().not_before = 1_061,
            2 => x.token_mut().not_before = 99,
            3 => x.token_mut().expires_at = 5_001,
            _ => x.key_mut().expires_at = 100,
        }
        x.signature[0] ^= 1;
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Metadata));
    }
}

#[test]
fn signature_and_strict_payload_checks_are_delegated() {
    let f = Fixture::new();
    let mut x = f.clone();
    x.signature[0] ^= 1;
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
    for payload in [
        b"{invalid".to_vec(),
        b"{}".to_vec(),
        vec![b' '; MAX_PAYLOAD_BYTES],
    ] {
        let mut x = f.clone();
        x.replace_payload(payload);
        assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
    }
    let mut json = serde_json::to_value(&f.claims).expect("claims");
    json["unexpected"] = true.into();
    let mut x = f.clone();
    x.replace_payload(serde_json::to_vec(&json).expect("json"));
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
    let mut x = f.clone();
    x.claims.audience = "wrong-audience".to_owned();
    x.replace_payload(serde_json::to_vec(&x.claims).expect("claims"));
    assert_eq!(x.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}

#[test]
fn supplied_time_operation_policy_and_request_identity_use_existing_verifier() {
    let f = Fixture::new();
    for now in [999, 1_060, 5_000] {
        let mut req = f.request();
        req.now = now;
        assert_eq!(
            diagnose_prepared_attestation(&f.payload, &f.signature, &f.key, &f.token, &req),
            Err(PreparedDiagnosticRefusal::Verification)
        );
    }
    let mut req = f.request();
    req.now = 1_000;
    assert_eq!(
        diagnose_prepared_attestation(&f.payload, &f.signature, &f.key, &f.token, &req),
        Ok(())
    );
    for field in 0..4 {
        let mut req = f.request();
        match field {
            0 => req.operation = PlanningPairOperation::RecoverPlanningPairSeat,
            1 => req.policy.maximum_lifetime_seconds = 59,
            2 => req.occupancy_generation = 0,
            _ => req.seat_binding_id = SeatBindingId::generate(),
        }
        assert_eq!(
            diagnose_prepared_attestation(&f.payload, &f.signature, &f.key, &f.token, &req),
            Err(PreparedDiagnosticRefusal::Verification)
        );
    }
}

#[test]
fn signed_metadata_matches_every_exact_prepared_field_after_verification() {
    let f = Fixture::new();
    for field in 0..12 {
        let mut x = f.clone();
        match field {
            0 => x.token_mut().token_id = id("other-token"),
            1 => x.token_mut().mini_project_id = MiniProjectId::generate(),
            2 => x.token_mut().task_id = None,
            3 => x.token_mut().seat_binding_id = SeatBindingId::generate(),
            4 => x.token_mut().occupancy_generation = 8,
            5 => {
                x.token_mut().native_identity.runtime_kind =
                    RuntimeKindKey::parse("codex").expect("kind")
            }
            6 => {
                x.token_mut().native_identity.host =
                    ExternalName::parse("other-host").expect("host")
            }
            7 => x.token_mut().native_identity.generation = 4,
            8 => x.token_mut().native_identity.native_id = id("other-session"),
            9 => x.token_mut().not_before = 1_001,
            10 => x.token_mut().expires_at = 1_059,
            _ => x.token_mut().occupancy_generation = 0,
        }
        assert_eq!(
            x.diagnose(),
            Err(PreparedDiagnosticRefusal::Metadata),
            "field {field}"
        );
    }
}

#[test]
fn key_commitment_precedes_token_commitment_and_signature_precedes_final_metadata() {
    let mut f = Fixture::new();
    f.token_mut().key_material_digest = ContentHash::of(b"wrong");
    f.token_mut().payload_digest = ContentHash::of(b"wrong");
    f.token_mut().revoked_revision = Some(4);
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::KeyCommitment));
    let mut f = Fixture::new();
    f.signature[0] ^= 1;
    f.token_mut().task_id = None;
    assert_eq!(f.diagnose(), Err(PreparedDiagnosticRefusal::Verification));
}
