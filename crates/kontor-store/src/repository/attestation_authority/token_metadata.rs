//! Prepared commitments on the existing writer. Stored observations authenticate
//! nothing: no issuance, signature verification, live possession or admission.

use kontor_core::consultation::ConsultationFamily;
use kontor_core::id::{
    AggregateRevision, ExternalName, MiniProjectId, RoleSlotId, RuntimeKindKey, SeatBindingId,
    TaskId, TopologyNodeId,
};
use kontor_core::repository::{
    AttestationSeatProvenance, AttestationTokenProjection, PrepareAttestationToken,
    StoredPreparedAttestationToken,
};
use kontor_core::state::NativeRuntimeIdentity;

use super::*;

fn token_head_in(
    conn: &Connection,
    scope: &AttestationAuthorityScope,
) -> RepositoryResult<Option<u64>> {
    let revision: Option<i64> = conn
        .query_row(
            "SELECT revision FROM attestation_token_heads WHERE project_id=?1 AND application=?2",
            params![scope.project_id.to_string(), scope.application.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    revision.map(unsigned).transpose()
}

fn unsigned(value: i64) -> RepositoryResult<u64> {
    u64::try_from(value).map_err(|_| conflict(SUBJECT, "invalid stored token integer"))
}

fn revision(value: i64) -> RepositoryResult<AggregateRevision> {
    AggregateRevision::parse(unsigned(value)?).map_err(RepositoryError::from)
}

fn token_from_row(row: &rusqlite::Row<'_>) -> RepositoryResult<StoredPreparedAttestationToken> {
    let domain: String = row.get("provenance_domain").map_err(backend)?;
    let provenance = match domain.as_str() {
        "hosted" => AttestationSeatProvenance::Hosted,
        "consultation" => {
            let family: String = row.get("run_family").map_err(backend)?;
            let run: String = row.get("run_id").map_err(backend)?;
            let slot: String = row.get("role_slot_id").map_err(backend)?;
            AttestationSeatProvenance::Consultation {
                run_id: super::super::consultation_run_id(
                    ConsultationFamily::parse(&family)?,
                    &run,
                )?,
                role_slot_id: RoleSlotId::parse(&slot)?,
                run_revision: revision(row.get("run_revision").map_err(backend)?)?,
            }
        }
        _ => return Err(conflict(SUBJECT, "unknown stored occupancy domain")),
    };
    let string = |name| row.get::<_, String>(name).map_err(backend);
    let number = |name| row.get::<_, i64>(name).map_err(backend).and_then(unsigned);
    let task: Option<String> = row.get("task_id").map_err(backend)?;
    let revoked: Option<i64> = row.get("revoked_revision").map_err(backend)?;
    Ok(StoredPreparedAttestationToken {
        issuer: ExternalId::parse(&string("issuer")?)?,
        key_id: ExternalId::parse(&string("key_id")?)?,
        token_id: ExternalId::parse(&string("token_id")?)?,
        payload_digest: ContentHash::parse(&string("payload_digest")?)?,
        key_material_digest: ContentHash::parse(&string("key_material_digest")?)?,
        key_registered_revision: number("key_registered_revision")?,
        preparation_key_head_revision: number("preparation_key_head_revision")?,
        not_before: number("not_before")?,
        expires_at: number("expires_at")?,
        mini_project_id: MiniProjectId::parse(&string("mini_project_id")?)?,
        task_id: task.as_deref().map(TaskId::parse).transpose()?,
        seat_binding_id: SeatBindingId::parse(&string("seat_binding_id")?)?,
        topology_node_id: TopologyNodeId::parse(&string("topology_node_id")?)?,
        binding_revision: AggregateRevision::parse(number("binding_revision")?)?,
        node_revision: AggregateRevision::parse(number("node_revision")?)?,
        provenance,
        occupancy_generation: number("occupancy_generation")?,
        native_identity: NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(&string("runtime_kind")?)?,
            host: ExternalName::parse(&string("host")?)?,
            generation: number("native_generation")?,
            native_id: ExternalId::parse(&string("native_id")?)?,
        },
        registered_revision: number("registered_revision")?,
        revoked_revision: revoked.map(unsigned).transpose()?,
    })
}

fn projection_in(
    conn: &Connection,
    scope: &AttestationAuthorityScope,
    issuer: &ExternalId,
    token: &ExternalId,
) -> RepositoryResult<Option<AttestationTokenProjection>> {
    let row = conn
        .query_row(
            "SELECT h.revision AS head_revision, t.* FROM attestation_token_heads h
         LEFT JOIN prepared_attestation_tokens t ON t.project_id=h.project_id
          AND t.application=h.application AND t.issuer=?3 AND t.token_id=?4
         WHERE h.project_id=?1 AND h.application=?2",
            params![
                scope.project_id.to_string(),
                scope.application.as_str(),
                issuer.as_str(),
                token.as_str()
            ],
            |row| {
                let found: Option<String> = row.get("token_id")?;
                Ok((
                    row.get::<_, i64>("head_revision")?,
                    found.map(|_| token_from_row(row)),
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(|(head, token)| {
        Ok(AttestationTokenProjection {
            scope: scope.clone(),
            head_revision: unsigned(head)?,
            selected_token: token.transpose()?,
        })
    })
    .transpose()
}

fn validate_request(request: &PrepareAttestationToken) -> RepositoryResult<()> {
    validate_scope(&request.scope)?;
    for id in [
        &request.issuer,
        &request.key_id,
        &request.token_id,
        &request.expected_native_identity.native_id,
    ] {
        validate_id(id)?;
    }
    validate_expected(Some(request.expected_key_head_revision))?;
    validate_expected(request.expected_token_head_revision)?;
    sqlite_integer(request.expected_occupancy_generation)?;
    sqlite_integer(request.expected_native_identity.generation)?;
    sqlite_integer(request.not_before)?;
    sqlite_integer(request.expires_at)?;
    if request.expected_occupancy_generation == 0
        || request.expected_native_identity.generation == 0
        || request.not_before >= request.expires_at
        || request.expected_native_identity.runtime_kind.as_str().len() > 256
        || request.expected_native_identity.host.as_str().len() > 256
    {
        return Err(
            DomainError::invalid(SUBJECT, "invalid token bounds, generation or interval").into(),
        );
    }
    Ok(())
}

struct Membership {
    node: TopologyNodeId,
    slot: RoleSlotId,
    binding_task: Option<TaskId>,
    node_task: Option<TaskId>,
    binding_revision: AggregateRevision,
    node_revision: AggregateRevision,
}

fn membership_in(
    conn: &Connection,
    request: &PrepareAttestationToken,
) -> RepositoryResult<Membership> {
    let contained: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mini_projects WHERE id=?1 AND project_id=?2)
          AND (?3 IS NULL OR EXISTS(SELECT 1 FROM tasks WHERE id=?3 AND project_id=?2 AND mini_project_id=?1))",
        params![request.mini_project_id.to_string(), request.scope.project_id.to_string(),
                request.task_id.map(|id| id.to_string())], |row| row.get(0),
    ).map_err(backend)?;
    if !contained {
        return Err(conflict(
            SUBJECT,
            "token scope is not contained in the owning project",
        ));
    }
    let found = conn.query_row(
        "SELECT b.topology_node_id, b.role_slot_id, b.task_id, n.task_id, b.revision, n.revision,
          b.project_id, n.project_id, n.mini_project_id, b.lifecycle, n.lifecycle
         FROM seat_bindings b JOIN topology_nodes n ON n.id=b.topology_node_id WHERE b.id=?1",
        [request.seat_binding_id.to_string()], |row| Ok((
            row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,Option<String>>(2)?,
            row.get::<_,Option<String>>(3)?,row.get::<_,i64>(4)?,row.get::<_,i64>(5)?,
            row.get::<_,String>(6)?,row.get::<_,String>(7)?,row.get::<_,Option<String>>(8)?,
            row.get::<_,String>(9)?,row.get::<_,String>(10)?)),
    ).optional().map_err(backend)?.ok_or(RepositoryError::NotFound {subject: SUBJECT})?;
    let (
        node,
        slot,
        task,
        node_task,
        binding_rev,
        node_rev,
        binding_project,
        node_project,
        epic,
        binding_state,
        node_state,
    ) = found;
    if binding_project != request.scope.project_id.to_string()
        || node_project != binding_project
        || epic != Some(request.mini_project_id.to_string())
        || binding_state != "active"
        || node_state != "active"
    {
        return Err(conflict(
            SUBJECT,
            "seat/node scope or lifecycle does not match",
        ));
    }
    Ok(Membership {
        node: TopologyNodeId::parse(&node)?,
        slot: RoleSlotId::parse(&slot)?,
        binding_task: task.as_deref().map(TaskId::parse).transpose()?,
        node_task: node_task.as_deref().map(TaskId::parse).transpose()?,
        binding_revision: revision(binding_rev)?,
        node_revision: revision(node_rev)?,
    })
}

fn native_unique_in(conn: &Connection, identity: &NativeRuntimeIdentity) -> RepositoryResult<()> {
    let count: i64 = conn.query_row(
        "SELECT (SELECT count(*) FROM hosted_topology_seats WHERE runtime_kind=?1 AND host=?2 AND generation=?3 AND native_id=?4)
          + (SELECT count(*) FROM consultation_seats WHERE runtime_kind=?1 AND host=?2 AND generation=?3 AND native_id=?4)
          + (SELECT count(*) FROM topology_node_containers WHERE runtime_kind=?1 AND host=?2 AND generation=?3 AND native_id=?4)",
        params![identity.runtime_kind.as_str(), identity.host.as_str(), sqlite_integer(identity.generation)?, identity.native_id.as_str()],
        |row| row.get(0),
    ).map_err(backend)?;
    if count != 1 {
        return Err(conflict(
            SUBJECT,
            "native metadata is absent or ambiguous with another subject",
        ));
    }
    Ok(())
}

fn consultation_provenance_in(
    conn: &Connection,
    request: &PrepareAttestationToken,
    member: &Membership,
) -> RepositoryResult<AttestationSeatProvenance> {
    let native = &request.expected_native_identity;
    let binding = request.seat_binding_id.to_string();
    let project = request.scope.project_id.to_string();
    let native_generation = sqlite_integer(native.generation)?;
    let native_params = params![
        binding,
        project,
        native.runtime_kind.as_str(),
        native.host.as_str(),
        native_generation,
        native.native_id.as_str()
    ];
    if member.binding_task.is_some() || member.node_task.is_some() {
        return Err(conflict(
            SUBJECT,
            "consultation container cannot claim a delivery task",
        ));
    }
    let run = conn.query_row(
    "SELECT r.family, r.run_id, r.revision, s.role_slot_id, s.occupancy_generation,
       r.project_id, r.mini_project_id, r.topology_node_id, r.subject_kind, r.subject_task_id, r.state
     FROM consultation_seats s JOIN consultation_runs r ON r.run_id=s.run_id
     WHERE s.seat_binding_id=?1 AND s.project_id=?2 AND s.runtime_kind=?3
       AND s.host=?4 AND s.generation=?5 AND s.native_id=?6",
    native_params, |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,
        row.get::<_,String>(3)?,row.get::<_,i64>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?,
        row.get::<_,String>(7)?,row.get::<_,Option<String>>(8)?,row.get::<_,Option<String>>(9)?,row.get::<_,String>(10)?)),
).optional().map_err(backend)?.ok_or_else(|| conflict(SUBJECT, "consultation native metadata is unknown"))?;
    let (family, run, rev, slot, generation, project, epic, node, subject, task, state) = run;
    let subject_matches = match subject.as_deref() {
        Some("epic") => task.is_none() && request.task_id.is_none(),
        Some("task") => task.is_some() && task == request.task_id.map(|id| id.to_string()),
        _ => false,
    };
    if project != request.scope.project_id.to_string()
        || epic != request.mini_project_id.to_string()
        || node != member.node.to_string()
        || slot != member.slot.as_str()
        || unsigned(generation)? != request.expected_occupancy_generation
        || !subject_matches
        || !matches!(
            state.as_str(),
            "materializing" | "running" | "awaiting_judge"
        )
    {
        return Err(conflict(
            SUBJECT,
            "consultation run/slot/scope/generation/lifecycle does not match",
        ));
    }
    Ok(AttestationSeatProvenance::Consultation {
        run_id: super::super::consultation_run_id(ConsultationFamily::parse(&family)?, &run)?,
        role_slot_id: member.slot.clone(),
        run_revision: revision(rev)?,
    })
}

fn provenance_in(
    conn: &Connection,
    request: &PrepareAttestationToken,
    member: &Membership,
) -> RepositoryResult<AttestationSeatProvenance> {
    let (hosted, consultation): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT count(*) FROM hosted_topology_seats WHERE seat_binding_id=?1),
                (SELECT count(*) FROM consultation_seats WHERE seat_binding_id=?1)",
            [request.seat_binding_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(backend)?;
    let native = &request.expected_native_identity;
    let binding = request.seat_binding_id.to_string();
    let project = request.scope.project_id.to_string();
    let native_generation = sqlite_integer(native.generation)?;
    let native_params = params![
        binding,
        project,
        native.runtime_kind.as_str(),
        native.host.as_str(),
        native_generation,
        native.native_id.as_str()
    ];
    let provenance = match (hosted, consultation) {
        (1, 0) => {
            if member.binding_task != request.task_id || member.node_task != request.task_id {
                return Err(conflict(
                    SUBJECT,
                    "hosted task scope does not match exactly",
                ));
            }
            let generation: Option<i64> = conn.query_row(
                "SELECT 1+(SELECT count(*) FROM hosted_topology_seat_history h WHERE h.project_id=s.project_id AND h.seat_binding_id=s.seat_binding_id)
                 FROM hosted_topology_seats s WHERE s.seat_binding_id=?1 AND s.project_id=?2
                  AND s.runtime_kind=?3 AND s.host=?4 AND s.generation=?5 AND s.native_id=?6",
                native_params, |row| row.get(0),
            ).optional().map_err(backend)?;
            if generation.map(unsigned).transpose()? != Some(request.expected_occupancy_generation)
            {
                return Err(conflict(
                    SUBJECT,
                    "hosted generation or native metadata does not match",
                ));
            }
            AttestationSeatProvenance::Hosted
        }
        (0, 1) => consultation_provenance_in(conn, request, member)?,
        _ => {
            return Err(conflict(
                SUBJECT,
                "actual seat provenance is absent or ambiguous",
            ));
        }
    };
    native_unique_in(conn, native)?;
    Ok(provenance)
}

fn insert_in(
    conn: &Connection,
    request: &PrepareAttestationToken,
    member: &Membership,
    provenance: &AttestationSeatProvenance,
    key: &StoredAttestationKey,
    next: i64,
) -> RepositoryResult<()> {
    let (domain, family, run, slot, run_rev) = match provenance {
        AttestationSeatProvenance::Hosted => ("hosted", None, None, None, None),
        AttestationSeatProvenance::Consultation {
            run_id,
            role_slot_id,
            run_revision,
        } => (
            "consultation",
            Some(run_id.family().as_str()),
            Some(run_id.as_text()),
            Some(role_slot_id.as_str()),
            Some(sqlite_integer(run_revision.get())?),
        ),
    };
    let native = &request.expected_native_identity;
    conn.execute(
        "INSERT INTO prepared_attestation_tokens
         (project_id,application,issuer,key_id,token_id,payload_digest,key_material_digest,key_registered_revision,
          preparation_key_head_revision,not_before,expires_at,mini_project_id,task_id,seat_binding_id,topology_node_id,
          binding_revision,node_revision,provenance_domain,run_family,run_id,role_slot_id,run_revision,
          occupancy_generation,runtime_kind,host,native_generation,native_id,registered_revision,revoked_revision)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,NULL)",
        params![request.scope.project_id.to_string(),request.scope.application.as_str(),request.issuer.as_str(),
            request.key_id.as_str(),request.token_id.as_str(),request.payload_digest.as_str(),key.material_digest.as_str(),
            sqlite_integer(key.registered_revision)?,sqlite_integer(request.expected_key_head_revision)?,
            sqlite_integer(request.not_before)?,sqlite_integer(request.expires_at)?,request.mini_project_id.to_string(),
            request.task_id.map(|id| id.to_string()),request.seat_binding_id.to_string(),member.node.to_string(),
            sqlite_integer(member.binding_revision.get())?,sqlite_integer(member.node_revision.get())?,domain,family,run,slot,run_rev,
            sqlite_integer(request.expected_occupancy_generation)?,native.runtime_kind.as_str(),native.host.as_str(),
            sqlite_integer(native.generation)?,native.native_id.as_str(),next],
    ).map_err(backend)?;
    Ok(())
}

pub(super) fn prepare(
    store: &SqliteStore,
    request: &PrepareAttestationToken,
) -> RepositoryResult<AttestationTokenProjection> {
    validate_request(request)?;
    let transaction = store.begin()?;
    store.require_attestation_scope(&transaction, &request.scope)?;
    let member = membership_in(&transaction, request)?;
    require_head(
        head_in(&transaction, &request.scope)?,
        Some(request.expected_key_head_revision),
    )?;
    let head = token_head_in(&transaction, &request.scope)?;
    require_head(head, request.expected_token_head_revision)?;
    let key = super::projection_in(
        &transaction,
        &request.scope,
        &request.issuer,
        &request.key_id,
    )?
    .and_then(|projection| projection.selected_key)
    .ok_or(RepositoryError::NotFound { subject: SUBJECT })?;
    if key.revoked_revision.is_some()
        || request.not_before < key.not_before
        || request.expires_at > key.expires_at
    {
        return Err(conflict(
            SUBJECT,
            "prepared interval or selected key is not eligible",
        ));
    }
    let provenance = provenance_in(&transaction, request, &member)?;
    if projection_in(
        &transaction,
        &request.scope,
        &request.issuer,
        &request.token_id,
    )?
    .is_some_and(|projection| projection.selected_token.is_some())
    {
        return Err(conflict(
            SUBJECT,
            "issuer/token identity is permanently used",
        ));
    }
    insert_in(
        &transaction,
        request,
        &member,
        &provenance,
        &key,
        next_revision(head)?,
    )?;
    let result = projection_in(
        &transaction,
        &request.scope,
        &request.issuer,
        &request.token_id,
    )?
    .ok_or_else(|| conflict(SUBJECT, "prepared projection is absent"))?;
    transaction.commit().map_err(backend)?;
    Ok(result)
}

pub(super) fn revoke(
    store: &SqliteStore,
    scope: &AttestationAuthorityScope,
    issuer: &ExternalId,
    token: &ExternalId,
    expected: u64,
) -> RepositoryResult<AttestationTokenProjection> {
    validate_scope(scope)?;
    validate_id(issuer)?;
    validate_id(token)?;
    validate_expected(Some(expected))?;
    let transaction = store.begin()?;
    store.require_attestation_scope(&transaction, scope)?;
    let head = token_head_in(&transaction, scope)?;
    require_head(head, Some(expected))?;
    let current = projection_in(&transaction, scope, issuer, token)?
        .and_then(|projection| projection.selected_token)
        .ok_or(RepositoryError::NotFound { subject: SUBJECT })?;
    if current.revoked_revision.is_none() {
        transaction
            .execute(
                "UPDATE prepared_attestation_tokens SET revoked_revision=?4 WHERE project_id=?1
             AND application=?2 AND issuer=?3 AND token_id=?5 AND revoked_revision IS NULL",
                params![
                    scope.project_id.to_string(),
                    scope.application.as_str(),
                    issuer.as_str(),
                    next_revision(head)?,
                    token.as_str()
                ],
            )
            .map_err(backend)?;
    }
    let result = projection_in(&transaction, scope, issuer, token)?
        .ok_or_else(|| conflict(SUBJECT, "revoked projection is absent"))?;
    transaction.commit().map_err(backend)?;
    Ok(result)
}

pub(super) fn read(
    store: &SqliteStore,
    scope: &AttestationAuthorityScope,
    issuer: &ExternalId,
    token: &ExternalId,
) -> RepositoryResult<Option<AttestationTokenProjection>> {
    validate_scope(scope)?;
    validate_id(issuer)?;
    validate_id(token)?;
    if scope.realm_id != store.realm_id() {
        return Ok(None);
    }
    projection_in(&store.connection, scope, issuer, token)
}
