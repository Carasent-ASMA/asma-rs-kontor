//! The three consultation launch columns preserve private replay/CAS identities.
use super::*;
use kontor_core::consultation::CommitteeRole;
use kontor_core::id::{AccountProfileId, CredentialAlias, ProviderUsageObservationId, RoleKey};
use kontor_core::launch_route::{ResolvedRoute, StoredLaunchRoute};
use kontor_core::repository::{
    CapacityRepository, CredentialReference, CredentialReferenceKind, NewAccountProfile,
    NewConsultationMaterializationReroute, NewConsultationRecoveryAttempt, NewProviderQuotaState,
    NewProviderUsageObservation, ProviderUsageObservation, StoredConsultationSeat,
};
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};

fn route(alias: &str) -> StoredLaunchRoute {
    StoredLaunchRoute::resolved(ResolvedRoute {
        schema_version: 1,
        slot: "codex-primary".into(),
        route_provider: "codex".into(),
        route_harness: "paseo".into(),
        native_alias: alias.into(),
        qualification: None,
        route_model_id: "gpt-example".into(),
        canonical_model: "gpt-example".into(),
        canonical_effort: None,
        canonical_vendor: "openai".into(),
        policy_hash: ContentHash::of(b"policy"),
        host_route_hash: ContentHash::of(alias.as_bytes()),
        resolved_at: at("2026-09-01T12:00:00Z"),
    })
    .unwrap()
}

fn initial(
    f: &Fixture,
    launch: StoredLaunchRoute,
) -> (StoredConsultationRun, StoredConsultationSeat) {
    let (mut run, mut node) = prepared_consultation(f, None, None);
    run.id = ConsultationRunId::Committee(CommitteeRunId::generate());
    node.kind = TopologyKindKey::parse("CSW").unwrap();
    f.store
        .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
            project_id: f.project_id,
            family: ConsultationFamily::Committee,
            profile_id: run.profile_id.clone(),
            version: run.profile_version,
            name: name("Synthetic review"),
            definition: "{\"schema_version\":1,\"seats\":[\"SA\"]}".into(),
            definition_hash: run.definition_hash.clone(),
            published_at: f.created_at,
        })
        .unwrap();
    let caller = f
        .store
        .get_seat_binding(f.project_id, run.caller_seat_binding_id)
        .unwrap()
        .unwrap();
    let binding = NewSeatBinding {
        id: SeatBindingId::generate(),
        project_id: f.project_id,
        topology_node_id: node.id,
        role_slot_id: RoleSlotId::parse("reviewer-a").unwrap(),
        role: caller.role,
        task_id: None,
        team_run_id: None,
        attach_deadline: at("2026-09-01T12:10:00Z"),
        parent_seat_binding_id: None,
        created_at: f.created_at,
    };
    let seat = StoredConsultationSeat {
        run_id: run.id,
        role_slot_id: binding.role_slot_id.clone(),
        committee_role: Some(CommitteeRole::Reviewer),
        logical_role: RoleKey::parse("reviewer").unwrap(),
        seat_binding_id: binding.id,
        model_rung: launch,
        occupancy_generation: 1,
        native_identity: None,
        provider_session_id: None,
        observed_at: None,
    };
    f.store
        .create_consultation_run(&run, &node, &[(&seat, &binding)])
        .unwrap();
    (run, seat)
}

#[test]
fn consultation_initial_envelope_and_legacy_reads_never_rewrite_evidence() {
    for launch in [
        route("opaque-original"),
        route("opaque-original").canonical_rung().into(),
    ] {
        let f = fixture();
        let (run, seat) = initial(&f, launch);
        let db = f.home.path().join("kontor.db");
        let read_bytes = || -> String {
            rusqlite::Connection::open(&db)
                .unwrap()
                .query_row(
                    "SELECT model_rung FROM consultation_seats WHERE seat_binding_id=?1",
                    [seat.seat_binding_id.to_string()],
                    |row| row.get(0),
                )
                .unwrap()
        };
        let bytes = read_bytes();
        let reopened = SqliteStore::open(&db).unwrap();
        let loaded = reopened
            .get_consultation_seat_by_binding(f.project_id, seat.seat_binding_id)
            .unwrap()
            .unwrap();
        assert_eq!(loaded, seat);
        assert_eq!(read_bytes(), bytes);
        assert_eq!(
            reopened
                .get_consultation_run(f.project_id, run.id)
                .unwrap()
                .unwrap()
                .context,
            run.context
        );
        assert!(!run.context.to_string().contains("opaque-original"));
        let mut substituted = loaded;
        substituted.model_rung = route("opaque-substitute");
        substituted.native_identity = Some(NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("paseo").unwrap(),
            host: name("synthetic-host"),
            generation: 1,
            native_id: ExternalId::parse("native-initial").unwrap(),
        });
        assert!(
            reopened
                .bind_consultation_seat(f.project_id, &substituted)
                .is_err()
        );
        assert_eq!(read_bytes(), bytes);
    }
}

#[test]
fn consultation_recovery_replays_the_exact_envelope_after_restart() {
    let f = fixture();
    let (run, mut seat) = initial(&f, route("opaque-original"));
    seat.native_identity = Some(NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").unwrap(),
        host: name("synthetic-host"),
        generation: 1,
        native_id: ExternalId::parse("native-predecessor").unwrap(),
    });
    seat.observed_at = Some(f.created_at);
    f.store.bind_consultation_seat(f.project_id, &seat).unwrap();
    let profile = CanonicalDocument::from_serializable(&serde_json::json!({"schema_version":1,"ordered_routes":[route("opaque-successor").canonical_rung()],"private_launch_route":route("opaque-successor")})).unwrap();
    let request = NewConsultationRecoveryAttempt {
        project_id: f.project_id,
        predecessor: seat,
        expected_revision: run.revision,
        recovery_reason: "provider_unavailable".into(),
        request_intent_hash: ContentHash::of(b"recovery-request"),
        recovery_profile: profile,
        selected_model_rung: route("opaque-successor"),
        prepared_at: f.created_at,
    };
    let first = f
        .store
        .prepare_consultation_recovery_attempt(&request)
        .unwrap();
    let reopened = SqliteStore::open(&f.home.path().join("kontor.db")).unwrap();
    assert_eq!(
        reopened
            .prepare_consultation_recovery_attempt(&request)
            .unwrap(),
        first
    );
    let mut substituted = request.clone();
    substituted.selected_model_rung = route("opaque-current-host");
    assert!(
        reopened
            .prepare_consultation_recovery_attempt(&substituted)
            .is_err()
    );
    let stored = reopened
        .get_consultation_recovery_attempt(
            f.project_id,
            first.run_id,
            &first.role_slot_id,
            &first.predecessor_native_id,
        )
        .unwrap()
        .unwrap();
    assert_eq!(stored, first);
    let mut successor = request.predecessor.clone();
    successor.model_rung = request.selected_model_rung.clone();
    successor.occupancy_generation += 1;
    successor.native_identity.as_mut().unwrap().native_id =
        ExternalId::parse("native-successor").unwrap();
    reopened
        .mark_consultation_recovery_predecessor_retired(&first, f.created_at)
        .unwrap();
    reopened
        .mark_consultation_recovery_successor_observed(&first, &successor)
        .unwrap();
    let mut stale = request.predecessor.clone();
    stale.model_rung = route("opaque-stale-predecessor");
    assert!(
        reopened
            .replace_consultation_seat(
                f.project_id,
                &stale,
                &successor,
                first.prepared_run_revision,
                f.created_at,
                "provider_unavailable"
            )
            .is_err()
    );
    reopened
        .replace_consultation_seat(
            f.project_id,
            &request.predecessor,
            &successor,
            first.prepared_run_revision,
            f.created_at,
            "provider_unavailable",
        )
        .unwrap();
    assert_eq!(
        reopened
            .get_consultation_seat_by_binding(f.project_id, successor.seat_binding_id)
            .unwrap()
            .unwrap(),
        successor
    );
    assert!(!run.context.to_string().contains("opaque-"));
}

#[test]
fn consultation_reroute_replays_full_private_identity_without_host_substitution() {
    let f = fixture();
    let (run, seat) = initial(&f, route("opaque-original"));
    let account = AccountProfileId::generate();
    let empty =
        CanonicalDocument::from_serializable(&serde_json::json!({"schema_version":1})).unwrap();
    f.store
        .create_account_profile(&NewAccountProfile {
            id: account,
            project_id: f.project_id,
            label: name("Synthetic account"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("paseo").unwrap(),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::Keychain,
                alias: CredentialAlias::parse("synthetic-reference").unwrap(),
            },
            environment: empty.clone(),
            routing: empty.clone(),
            capability: empty,
            provider_identity: None,
            enabled: true,
            created_at: f.created_at,
        })
        .unwrap();
    let observation = ProviderUsageObservation {
        id: ProviderUsageObservationId::generate(),
        project_id: f.project_id,
        account_profile_id: account,
        provider: seat.model_rung.canonical_rung().provider.0,
        evidence_hash: ContentHash::of(b"available"),
        state: ProviderQuotaKind::Available,
        resets_at: None,
        windows: Vec::new(),
        observed_at: f.created_at,
    };
    f.store
        .record_provider_usage_observation(&NewProviderUsageObservation {
            observation: observation.clone(),
            quota_state: Some(NewProviderQuotaState {
                project_id: f.project_id,
                account_profile_id: account,
                provider: observation.provider.clone(),
                state: observation.state,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: observation.evidence_hash.clone(),
                provenance: None,
                source: ProviderQuotaSource::ProviderReport,
                observed_at: f.created_at,
                expected_revision: AggregateRevision::INITIAL,
                updated_at: f.created_at,
            }),
            idempotency_key: None,
            intent_hash: None,
        })
        .unwrap();
    let request = NewConsultationMaterializationReroute {
        project_id: f.project_id,
        predecessor: seat,
        expected_revision: run.revision,
        successor_model_rung: route("opaque-successor"),
        reason: "permission_mode_unsupported".into(),
        recovery_profile: CanonicalDocument::from_serializable(
            &serde_json::json!({"schema_version":1,"private_launch_route":route("opaque-successor")}),
        )
        .unwrap(),
        request_intent_hash: ContentHash::of(b"reroute-request"),
        idempotency_key: IdempotencyKey::parse("private-reroute").unwrap(),
        headroom_observation: observation,
        headroom_fresh_after: f.created_at,
        rerouted_at: f.created_at,
    };
    let first = f
        .store
        .reroute_unmaterialized_consultation_seat(&request)
        .unwrap();
    let reopened = SqliteStore::open(&f.home.path().join("kontor.db")).unwrap();
    assert_eq!(
        reopened
            .reroute_unmaterialized_consultation_seat(&request)
            .unwrap(),
        first
    );
    let mut substituted = request;
    substituted.successor_model_rung = route("opaque-current-host");
    assert!(
        reopened
            .reroute_unmaterialized_consultation_seat(&substituted)
            .is_err()
    );
    assert_eq!(
        reopened
            .get_consultation_seat_by_binding(f.project_id, first.seat_binding_id)
            .unwrap()
            .unwrap()
            .model_rung,
        first.successor_model_rung
    );
    assert_eq!(
        reopened
            .get_consultation_run(f.project_id, run.id)
            .unwrap()
            .unwrap()
            .context,
        run.context
    );
}
