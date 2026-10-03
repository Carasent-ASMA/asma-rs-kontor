//! Governed `planning_pair@1` operations (ASMA-8282, disposition D-2).
//!
//! Ten registered routes beside the unchanged Advisor and Committee ones. The
//! registry tier on each is a floor, never authentication: invocation,
//! clarification and disposition are accepted only from the frozen caller's own
//! scoped seat credential, and a finding or answer only from the member seat's
//! own. An ambient Admin or Operator credential cannot act as either, and no
//! caller id or slot string in a body is treated as a credential.
//!
//! Nothing here can carry a verdict, a Judge, an aggregate or a settlement: the
//! request and response shapes have no field for one.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use kontor_core::id::{
    AggregateRevision, BoundedText, ContentHash, ExternalId, ExternalName, MiniProjectId,
    PlanningPairRunId, ProjectId, RuntimeKindKey, SeatBindingId, TaskId, TopologyNodeId,
};
use kontor_core::planning_pair::PlanningPairSlot;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::applications::{
    AdviceDispositionDto, AppliedProfileDto, MutationReceiptDto, ObservedBindingDto,
    ProfileApplyRequest, ProfileCatalogDto, ProfilePreviewDto, ProfilePreviewRequest,
    ProfileRevisionDto, RuntimeModelRouteRequest, resolve_epic_selector,
};
use crate::control::{idempotency_key, parse_id};
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;
use crate::{Caller, CallerCapability};

/// One of the two member slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum PlanningPairSlotDto {
    /// `SEAT A`.
    #[serde(rename = "seat-a")]
    SeatA,
    /// `SEAT B`.
    #[serde(rename = "seat-b")]
    SeatB,
}

impl PlanningPairSlotDto {
    /// The domain slot.
    #[must_use]
    pub const fn slot(self) -> PlanningPairSlot {
        match self {
            Self::SeatA => PlanningPairSlot::SeatA,
            Self::SeatB => PlanningPairSlot::SeatB,
        }
    }

    /// The wire slot of one domain slot.
    #[must_use]
    pub const fn of(slot: PlanningPairSlot) -> Self {
        match slot {
            PlanningPairSlot::SeatA => Self::SeatA,
            PlanningPairSlot::SeatB => Self::SeatB,
        }
    }
}

/// The protocol a caller explicitly selects. There is exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum PlanningPairProtocolDto {
    /// The bounded two-member planning pair.
    #[serde(rename = "planning_pair@1")]
    PlanningPairV1,
}

/// The exact published document revision an invocation pins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairProfileRefDto {
    /// The document id shared by every revision.
    pub id: String,
    /// The pinned revision.
    #[schema(value_type = u32)]
    pub version: kontor_core::id::SpecVersion,
    /// The canonical hash the caller read; a different published hash refuses.
    #[schema(value_type = String)]
    pub definition_hash: ContentHash,
}

/// One member's placement: the existing binding the TPM names for it and the
/// eligibility it states. The route is the shared allocator's, never the
/// caller's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairMemberPlacementRequest {
    /// The member slot.
    pub slot: PlanningPairSlotDto,
    /// The existing canonical binding this member resolves.
    pub binding_key: String,
    /// Account aliases that cannot take this member now.
    #[serde(default)]
    pub unavailable_accounts: Vec<String>,
    /// Vendors this member must avoid.
    #[serde(default)]
    pub excluded_vendors: Vec<String>,
}

/// Invoke one planning pair against an epic, as its frozen caller.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InvokePlanningPairRequest {
    /// The protocol, named explicitly. Nothing substitutes another.
    pub protocol: PlanningPairProtocolDto,
    /// The exact document revision, including its canonical hash.
    pub profile: PlanningPairProfileRefDto,
    /// Short semantic subject the pinned Team Definition renders.
    #[schema(value_type = String)]
    pub topic: ExternalName,
    /// What the caller asks.
    #[schema(value_type = String)]
    pub question: BoundedText,
    /// Optional ticket scope inside the epic; absent means the epic.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Exactly two members, `seat-a` then `seat-b`.
    pub members: Vec<PlanningPairMemberPlacementRequest>,
    /// The epic revision the caller read.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// One member's finding or clarification answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordPlanningPairContributionRequest {
    /// The advice, verbatim.
    #[schema(value_type = String)]
    pub advice: BoundedText,
    /// The run revision the member read.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// The caller's one clarification question.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestPlanningPairClarificationRequest {
    /// What the caller needs clarified.
    #[schema(value_type = String)]
    pub question: BoundedText,
    /// One or both members, each once.
    pub addressed: Vec<PlanningPairSlotDto>,
    /// The run revision the caller read.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// What the caller decided about one member's advice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairMemberDispositionDto {
    /// The member.
    pub slot: PlanningPairSlotDto,
    /// The exact finding the decision is about.
    #[schema(value_type = String)]
    pub finding: ContentHash,
    /// The exact clarification answer, when the member gave one.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<ContentHash>,
    /// Accepted, partially accepted or rejected.
    pub disposition: AdviceDispositionDto,
}

/// The caller's disposition. A decision, not a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordPlanningPairDispositionRequest {
    /// One entry per member, `seat-a` then `seat-b`.
    pub members: Vec<PlanningPairMemberDispositionDto>,
    /// Why the caller decided as it did.
    #[schema(value_type = String)]
    pub rationale: BoundedText,
    /// The run revision the caller read.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// One released finding or answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairContributionDto {
    /// `findings` or `clarification`.
    pub round: String,
    /// The member that gave it.
    pub slot: PlanningPairSlotDto,
    /// The advice, verbatim.
    #[schema(value_type = String)]
    pub advice: BoundedText,
    /// Its canonical address.
    #[schema(value_type = String)]
    pub document_hash: ContentHash,
}

/// The released clarification round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairClarificationDto {
    /// The caller's question.
    #[schema(value_type = String)]
    pub question: BoundedText,
    /// The members it addressed.
    pub addressed: Vec<PlanningPairSlotDto>,
    /// Every addressed member's answer.
    pub answers: Vec<PlanningPairContributionDto>,
}

/// The caller's recorded disposition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairDispositionRecordDto {
    /// One entry per member, `seat-a` then `seat-b`.
    pub members: Vec<PlanningPairMemberDispositionDto>,
    /// Why the caller decided as it did.
    #[schema(value_type = String)]
    pub rationale: BoundedText,
}

/// One frozen member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairMemberDto {
    /// The member slot.
    pub slot: PlanningPairSlotDto,
    /// The exact native seat title.
    pub label: String,
    /// The persistent SeatBinding.
    #[schema(value_type = String)]
    pub seat_binding_id: SeatBindingId,
    /// The native-filler generation scoped credentials are fenced to.
    pub occupancy_generation: u64,
    /// The binding the TPM named.
    pub binding_key: String,
    /// The actual vendor the shared allocator held distinct.
    pub vendor: String,
    /// The route the shared allocator chose.
    pub model_route: RuntimeModelRouteRequest,
    /// The native readback, once a supported runtime launched the member.
    /// Absent means no native launch is recorded: persistence alone never
    /// reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_binding: Option<ObservedBindingDto>,
    /// The exact native session the member is known by at its current
    /// generation, qualified or not. Only a bound `observed_binding` qualifies
    /// the member; this is what its caller names to recover it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_native: Option<PlanningPairKnownNativeDto>,
}

/// One member's known native session: what a trusted runtime readback named,
/// never a qualification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairKnownNativeDto {
    /// The exact session.
    pub native_identity: PlanningPairNativeIdentityDto,
    /// Its provider conversation, when one was reported.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<ExternalId>,
    /// Why its launch readback did not qualify the member, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readback_refusal: Option<String>,
    /// When it was read back.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: kontor_core::id::Timestamp,
}

/// Who the projection was rendered for, and so what it may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlanningPairViewerDto {
    /// The frozen caller: released findings, answers and dissent.
    Caller,
    /// One member: its own contributions only, never its peer's.
    Member,
    /// An ambient realm reader: the run and its members, no contribution.
    Observer,
}

/// One planning pair as its reader may see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairRunDto {
    /// The Realm it belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The consultation.
    #[schema(value_type = String)]
    pub planning_pair_run_id: PlanningPairRunId,
    /// The epic it advises.
    #[schema(value_type = String)]
    pub epic_id: MiniProjectId,
    /// The ticket it advises, when it was invoked at one.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Always `planning_pair@1`.
    pub protocol: PlanningPairProtocolDto,
    /// The pinned document revision.
    pub profile: ProfileRevisionDto,
    /// The frozen semantic topic.
    #[schema(value_type = String)]
    pub topic: ExternalName,
    /// The caller's question.
    #[schema(value_type = String)]
    pub question: BoundedText,
    /// The frozen caller seat.
    #[schema(value_type = String)]
    pub caller_seat_binding_id: SeatBindingId,
    /// The container node.
    #[schema(value_type = String)]
    pub topology_node_id: TopologyNodeId,
    /// The rendered container name, when the pinned Team Definition names it.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_name: Option<ExternalName>,
    /// `materializing`, `running`, `needs_human` or `disposed`.
    pub state: String,
    /// The protocol phase the latest record restores to.
    pub phase: String,
    /// The frozen placement's canonical hash.
    #[schema(value_type = String)]
    pub placement_hash: ContentHash,
    /// Both frozen members.
    pub members: Vec<PlanningPairMemberDto>,
    /// Who this projection was rendered for.
    pub viewer: PlanningPairViewerDto,
    /// Both findings, once released and only to the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<Vec<PlanningPairContributionDto>>,
    /// A member reader's own contributions, sealed or not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own_contributions: Vec<PlanningPairContributionDto>,
    /// The clarification round, once every addressed member answered, and
    /// only to the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarification: Option<PlanningPairClarificationDto>,
    /// The caller's disposition, once recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<PlanningPairDispositionRecordDto>,
    /// The advice the caller did not wholly adopt, kept verbatim; to the
    /// caller only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_dissent: Vec<PlanningPairContributionDto>,
    /// The record revision the projection restored.
    #[schema(value_type = u64)]
    pub record_revision: AggregateRevision,
    /// The run revision.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The control-plane position the answer is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
    /// The receipt, for a write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<MutationReceiptDto>,
}

/// One exact native session: its runtime family, host and runtime generation,
/// and its native id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanningPairNativeIdentityDto {
    /// The runtime family.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The runtime host that owns the generation.
    #[schema(value_type = String)]
    pub host: ExternalName,
    /// The runtime generation.
    pub generation: u64,
    /// The native session id within it.
    #[schema(value_type = String)]
    pub native_id: ExternalId,
}

/// Requalify one member on its exact known native session, as the pair's
/// frozen caller.
///
/// Every field is a compare-and-swap assertion. The member and the pair come
/// from the path and the caller from its credential; the session is the one
/// already known for the member, never one named here. There is no route,
/// provider, profile, generation, credential or new-session field.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoverPlanningPairSeatRequest {
    /// The run revision the caller read.
    #[schema(value_type = u64)]
    pub expected_run_revision: AggregateRevision,
    /// The member's occupancy generation the caller read.
    pub expected_member_occupancy_generation: u64,
    /// The member's known native session the caller read.
    pub expected_native_identity: PlanningPairNativeIdentityDto,
    /// The member's provider conversation, required exactly when one is
    /// recorded for the known session.
    #[schema(value_type = Option<String>)]
    #[serde(default)]
    pub expected_provider_session_id: Option<ExternalId>,
}

/// One member requalified on its exact known native session.
///
/// The same SeatBinding, generation, session and frozen placement: nothing is
/// created, replaced or rerouted, and nothing here is a verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanningPairSeatRecoveryDto {
    /// The pair as its caller now sees it.
    pub planning_pair: PlanningPairRunDto,
    /// The member's persistent SeatBinding.
    #[schema(value_type = String)]
    pub seat_binding_id: SeatBindingId,
    /// The member slot.
    pub slot: PlanningPairSlotDto,
    /// The member's unchanged occupancy generation.
    pub member_occupancy_generation: u64,
    /// The same native session, read back again.
    pub native_identity: PlanningPairNativeIdentityDto,
    /// The same provider conversation, when one is recorded.
    #[schema(value_type = Option<String>)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<ExternalId>,
    /// The frozen placement the member's provenance was read back against.
    #[schema(value_type = String)]
    pub placement_hash: ContentHash,
    /// The run revision the requalification was recorded at.
    #[schema(value_type = u64)]
    pub recovered_revision: AggregateRevision,
    /// The recovery's receipt.
    pub receipt: MutationReceiptDto,
}

/// The exact persistent seat a scoped credential authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanningPairSeat {
    /// The SeatBinding.
    pub seat_binding_id: SeatBindingId,
    /// The native-filler generation the credential is fenced to.
    pub occupancy_generation: u64,
}

/// Who is reading a planning pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningPairReader {
    /// An ambient realm credential at Observer or above.
    Ambient,
    /// One persistent seat's scoped credential.
    Seat(PlanningPairSeat),
}

/// The scoped seat a write route requires. Ambient credentials, at any tier,
/// are refused: none of them may act as a pair's caller or member.
fn scoped_seat(state: &ApiState, caller: Caller) -> Result<PlanningPairSeat, ApiError> {
    let seat_binding_id = caller.require_scoped_seat(state)?;
    let occupancy_generation = caller.occupancy_generation().ok_or_else(|| {
        state.refuse(
            ApiErrorCode::Forbidden,
            "the scoped seat credential carries no occupancy generation",
        )
    })?;
    Ok(PlanningPairSeat {
        seat_binding_id,
        occupancy_generation,
    })
}

/// Every published planning pair document revision.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/planning-pair-profiles", tag = "applications",
    params(("project_id" = String, Path, description = "The owning project")),
    responses(
        (status = 200, body = ProfileCatalogDto),
        (status = 401), (status = 403), (status = 404),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn planning_pair_profiles(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<ProfileCatalogDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state.applications().planning_pair_profiles(project_id)?,
    ))
}

/// Judge one planning pair document. Commits nothing.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/planning-pair-profiles:preview", tag = "applications",
    params(("project_id" = String, Path, description = "The owning project")),
    request_body = ProfilePreviewRequest,
    responses(
        (status = 200, body = ProfilePreviewDto),
        (status = 401), (status = 403), (status = 404),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn preview_planning_pair_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    Json(request): Json<ProfilePreviewRequest>,
) -> Result<Json<ProfilePreviewDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .preview_planning_pair_profile(project_id, &request)?,
    ))
}

/// Publish one planning pair document revision.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/planning-pair-profiles:apply", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ProfileApplyRequest,
    responses(
        (status = 200, body = AppliedProfileDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn apply_planning_pair_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ProfileApplyRequest>,
) -> Result<Json<AppliedProfileDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .apply_planning_pair_profile(&key, project_id, &request)
            .await?,
    ))
}

/// Invoke one planning pair as its caller's own scoped seat.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/planning-pair-runs:invoke",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = InvokePlanningPairRequest,
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn invoke_planning_pair_run(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<InvokePlanningPairRequest>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = resolve_epic_selector(&state, project_id, &epic_id)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .invoke_planning_pair_run(&key, project_id, epic_id, seat, &request)
            .await?,
    ))
}

/// Read one planning pair as its reader may see it.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair")
    ),
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn planning_pair_run(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id)): Path<(String, String)>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let reader = if caller.seat().is_some() {
        PlanningPairReader::Seat(scoped_seat(&state, caller)?)
    } else {
        caller.require(&state, CallerCapability::Observer)?;
        PlanningPairReader::Ambient
    };
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    Ok(Json(
        state
            .applications()
            .planning_pair_run(project_id, run_id, reader)?,
    ))
}

/// Record the authenticated member's sealed finding.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/findings:record",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair"),
        ("Idempotency-Key" = String, Header, description = "The member's stable key")
    ),
    request_body = RecordPlanningPairContributionRequest,
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn record_planning_pair_finding(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordPlanningPairContributionRequest>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .record_planning_pair_finding(&key, project_id, run_id, seat, &request)
            .await?,
    ))
}

/// Ask the caller's one clarification question.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/clarification:request",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = RequestPlanningPairClarificationRequest,
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn request_planning_pair_clarification(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RequestPlanningPairClarificationRequest>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .request_planning_pair_clarification(&key, project_id, run_id, seat, &request)
            .await?,
    ))
}

/// Record the authenticated addressed member's sealed answer.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/answers:record",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair"),
        ("Idempotency-Key" = String, Header, description = "The member's stable key")
    ),
    request_body = RecordPlanningPairContributionRequest,
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn record_planning_pair_answer(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordPlanningPairContributionRequest>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .record_planning_pair_answer(&key, project_id, run_id, seat, &request)
            .await?,
    ))
}

/// Record the caller's disposition. Terminal; never a settlement.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/disposition:record",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = RecordPlanningPairDispositionRequest,
    responses(
        (status = 200, body = PlanningPairRunDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn record_planning_pair_disposition(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordPlanningPairDispositionRequest>,
) -> Result<Json<PlanningPairRunDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .record_planning_pair_disposition(&key, project_id, run_id, seat, &request)
            .await?,
    ))
}

/// Requalify one member on its exact known native session, as the pair's
/// frozen caller's own scoped seat. Never a replacement or a new session.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/seats/{seat_binding_id}/recover",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("planning_pair_run_id" = String, Path, description = "The planning pair"),
        ("seat_binding_id" = String, Path, description = "The member seat to requalify"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = RecoverPlanningPairSeatRequest,
    responses(
        (status = 200, body = PlanningPairSeatRecoveryDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "The owning application service is not composed")
    )
)]
pub async fn recover_planning_pair_seat(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, run_id, seat_binding_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecoverPlanningPairSeatRequest>,
) -> Result<Json<PlanningPairSeatRecoveryDto>, ApiError> {
    let seat = scoped_seat(&state, caller)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let run_id = parse_id(&state, PlanningPairRunId::parse(&run_id))?;
    let seat_binding_id = parse_id(&state, SeatBindingId::parse(&seat_binding_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .recover_planning_pair_seat(&key, project_id, run_id, seat_binding_id, seat, &request)
            .await?,
    ))
}
