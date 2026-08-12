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
        crate::control::health,
        crate::control::realm,
        crate::control::run_snapshot,
        crate::control::task_snapshot,
        crate::control::command,
        crate::control::events,
        crate::query::project,
        crate::query::tasks,
        crate::query::gates,
        crate::query::mission,
        crate::query::profile,
        crate::query::receipt,
        crate::query::accounts,
        crate::query::account,
        crate::query::runtimes,
        crate::query::scheduler_contention,
        crate::wired::projects,
        crate::wired::missions,
        crate::wired::runs,
        crate::wired::scheduler_plan,
        crate::wired::tickets,
        crate::wired::ticket,
        crate::wired::ticket_comments,
        crate::wired::ticket_transitions,
        crate::wired::runtime_sessions,
        crate::sessions::timeline,
        crate::sessions::stream,
        crate::sessions::send_message,
        crate::sessions::respond_permission,
    ),
    components(schemas(
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
        crate::query::AccountDto,
        crate::query::ContentionDto,
        crate::query::GateEvaluationDto,
        crate::query::GateInspectionDto,
        crate::query::MissionDto,
        crate::query::ModuleClaimDto,
        crate::query::PhaseDto,
        crate::query::ProfileDto,
        crate::query::ProjectDto,
        crate::query::ReceiptInspectionDto,
        crate::query::ReceiptTransitionDto,
        crate::query::RuntimeDto,
        crate::query::TaskSummaryDto,
        crate::state::BarrierState,
        crate::wired::BlockerDto,
        crate::wired::CommentDto,
        crate::wired::ConflictDto,
        crate::wired::DecisionDto,
        crate::wired::MissionEntryDto,
        crate::wired::NativeSessionDto,
        crate::wired::ObservationDto,
        crate::wired::PlanDto,
        crate::wired::ProjectEntryDto,
        crate::wired::ProjectionDto,
        crate::wired::RunEntryDto,
        crate::wired::TicketDto,
        crate::wired::TicketLinkDto,
        crate::wired::TransitionDto,
    )),
    tags(
        (name = "control", description = "Liveness, identity, snapshots, commands and the durable event feed."),
        (name = "query", description = "The KON-MVP-16 read amendment: what a CLI or MCP caller needs to find and explain work."),
        (name = "sessions", description = "Session content, read from the runtime and never from this realm's log.")
    )
)]
pub struct ApiDoc;

/// The document, built once per call.
#[must_use]
pub fn document() -> Document {
    ApiDoc::openapi()
}
