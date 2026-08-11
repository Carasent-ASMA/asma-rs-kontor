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
        crate::state::BarrierState,
    )),
    tags(
        (name = "control", description = "Liveness, identity, snapshots, commands and the durable event feed."),
        (name = "sessions", description = "Session content, read from the runtime and never from this realm's log.")
    )
)]
pub struct ApiDoc;

/// The document, built once per call.
#[must_use]
pub fn document() -> Document {
    ApiDoc::openapi()
}
