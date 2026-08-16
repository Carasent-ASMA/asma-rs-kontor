//! The generated contract document.
//!
//! It is derived from the handlers and the DTOs rather than written by hand, so it
//! cannot describe a route that does not exist or a field that was renamed. That
//! also makes it a useful canary: a secret, a runtime endpoint or a transcript
//! field could only appear here by first appearing in a DTO, which is what the
//! disclosure tests scan for.

use utoipa::OpenApi;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, openapi::OpenApi as Document};

/// Declare the one credential scheme every route requires.
struct RealmBearer;

impl Modify for RealmBearer {
    fn modify(&self, openapi: &mut Document) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "realm_bearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(
                            "One of the realm's tier secrets, read from its 0600 credential file.",
                        ))
                        .build(),
                ),
            );
        }
    }
}

/// The Kontor loopback contract.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kontor control plane",
        description = "The loopback HTTP and SSE contract of one Kontor realm.",
        version = "1.0.0"
    ),
    modifiers(&RealmBearer),
    paths(
        crate::memory::list,
        crate::memory::history,
        crate::memory::propose,
        crate::memory::approve,
        crate::memory::tombstone,
        crate::memory::purge,
        crate::memory::import_preview,
        crate::memory::import_apply,
        crate::memory::freeze,
        crate::memory::switch,
        crate::control::health,
        crate::control::realm,
        crate::control::run_snapshot,
        crate::control::task_snapshot,
        crate::control::command,
        crate::control::events,
        crate::sessions::timeline,
        crate::sessions::stream,
        crate::control::context_policy_preview,
        crate::sessions::compact,
        crate::sessions::send_message,
        crate::sessions::respond_permission,
        crate::applications::ensure_project,
        crate::applications::work_profiles,
        crate::applications::team_templates,
        crate::applications::model_catalog,
        crate::applications::teams,
        crate::applications::save_team_draft,
        crate::applications::publish_team,
        crate::applications::account_profiles,
        crate::applications::ensure_account_profile,
        crate::applications::runtime_capabilities,
        crate::applications::apply_epic,
        crate::applications::read_epic,
        crate::applications::arm,
        crate::applications::disarm,
        crate::applications::plan,
        crate::applications::start,
        crate::applications::lifecycle,
        crate::applications::resolve_context,
        crate::applications::record_gate,
        crate::applications::select_profile,
        crate::applications::select_team,
        crate::applications::select_account,
        crate::applications::ticket_reconcile_plan,
        crate::applications::ticket_reconcile_apply,
        crate::applications::settle_runtime,
        crate::applications::settle_turn,
        crate::applications::attest_late_handoff,
        crate::applications::replace_seat,
        crate::applications::waive_role_slot,
        crate::applications::register_pack,
        crate::applications::profile_packs,
        crate::applications::work_profile,
        crate::applications::validate_work_profile,
        crate::applications::trigger,
        crate::applications::submit_intake,
        crate::applications::intake_receipt,
        crate::applications::connector_field_specs,
        crate::applications::connector_workflow_specs,
        crate::applications::ticket_conflicts,
        crate::applications::resolve_ticket_conflict,
        crate::applications::pull_ticket_comments,
        crate::applications::ticket_comments,
        crate::applications::claim_ticket,
    ),
    components(schemas(
        crate::applications::AppliedDto,
        crate::applications::AppliedEpicDto,
        crate::applications::AppliedLinkDto,
        crate::applications::AppliedTaskDto,
        crate::applications::ApplyEpicRequest,
        crate::applications::ArmRequest,
        crate::applications::AccountProfileDto,
        crate::applications::AuthorizationProjectionDto,
        crate::applications::BlockedTaskDto,
        crate::applications::BudgetBoundsRequest,
        crate::applications::DisarmRequest,
        crate::applications::EnsureAccountProfileRequest,
        crate::applications::EnsureProjectRequest,
        crate::applications::EpicProjectionDto,
        crate::applications::EpicTaskProjectionDto,
        crate::applications::GateProjectionDto,
        crate::applications::EpicTaskRequest,
        crate::applications::LifecycleAction,
        crate::applications::LifecycleOutcomeDto,
        crate::applications::LifecycleRequest,
        crate::applications::ProjectDto,
        crate::applications::ReadyTaskDto,
        crate::applications::RevisionRefDto,
        crate::applications::RuntimeCapabilityDto,
        crate::applications::SchedulerPlanDto,
        crate::applications::SchedulerStartDto,
        crate::applications::SeatProjectionDto,
        crate::applications::StartRequest,
        crate::applications::StartedSeatDto,
        crate::applications::TeamRunProjectionDto,
        crate::applications::TeamTemplateCatalogDto,
        crate::applications::ModelCatalogDto,
        crate::applications::TeamDraftRequest,
        crate::applications::TeamDraftDto,
        crate::applications::PublishedTeamRevisionDto,
        crate::applications::TeamsProjectionDto,
        crate::applications::TicketLinkRequest,
        crate::applications::WorkProfileCatalogDto,
        crate::applications::GateVerdictDto,
        crate::applications::ProvenanceDto,
        crate::applications::RecordGateRequest,
        crate::applications::RedactionDto,
        crate::applications::ResolveContextRequest,
        crate::applications::ResolvedContextDto,
        crate::applications::SelectionDto,
        crate::applications::SelectionRequest,
        crate::applications::TicketFieldDiffDto,
        crate::applications::TicketReconcileAppliedDto,
        crate::applications::TicketReconcileApplyRequest,
        crate::applications::TicketReconcilePlanDto,
        crate::applications::RuntimeSettlementDto,
        crate::applications::SettleTurnRequest,
        crate::applications::SettledTurnDto,
        crate::applications::AttestLateHandoffRequest,
        crate::applications::LateHandoffAttestationDto,
        crate::applications::ReplaceSeatRequest,
        crate::applications::ReplacedSeatDto,
        crate::applications::WaiveRoleSlotRequest,
        crate::applications::RoleSlotWaiverDto,
        crate::applications::TurnFollowUpDto,
        crate::applications::ConnectorSpecDto,
        crate::applications::ProfilePackDto,
        crate::applications::RegisterPackRequest,
        crate::applications::IntakeReceiptDto,
        crate::applications::ProfileArtifactDto,
        crate::applications::ProfileHandoffDto,
        crate::applications::ProfilePhaseDto,
        crate::applications::ProfileValidationDto,
        crate::applications::ResolveConflictRequest,
        crate::applications::SubmitIntakeRequest,
        crate::applications::TicketClaimDto,
        crate::applications::TicketCommentDto,
        crate::applications::TicketCommentPullDto,
        crate::applications::TicketConflictDto,
        crate::applications::TriggerSpecDto,
        crate::applications::WorkProfileDetailDto,
        crate::dto::AppliedRevisionsDto,
        crate::dto::BindingDto,
        crate::dto::CommandRequest,
        crate::dto::EventDto,
        crate::dto::GapDto,
        crate::dto::HealthDto,
        crate::dto::MessageAckDto,
        crate::dto::MessageRequest,
        crate::dto::PermissionAckDto,
        crate::dto::PermissionRequestBody,
        crate::dto::ProjectionDto,
        crate::dto::RealmDto,
        crate::dto::ReceiptDto,
        crate::dto::ReceiptResponse,
        crate::dto::RunDto,
        crate::dto::StreamFrameDto,
        crate::dto::StreamRefusalDto,
        crate::dto::TaskDto,
        crate::dto::TimelineDto,
        crate::dto::TimelineItemDto,
        crate::error::ApiErrorBody,
        crate::state::BarrierState,
    )),
    tags(
        (name = "control", description = "Liveness, identity, snapshots, commands and the durable event feed."),
        (name = "sessions", description = "Session content, read from the runtime and never from this realm's log."),
        (name = "applications", description = "Declarative work-graph, arming, scheduling and lifecycle operations. Each one runs its application service and answers with the durable projection.")
    )
)]
pub struct ApiDoc;

/// The document, built once per call.
#[must_use]
pub fn document() -> Document {
    ApiDoc::openapi()
}
