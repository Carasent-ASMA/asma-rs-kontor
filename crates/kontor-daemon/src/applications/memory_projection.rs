//! Qualification completes outside the store lock; activation owns late CAS.
use super::*;
use kontor_store::memory::{
    ProjectionPreview, ProjectionQualification, ProjectionReadback, ProjectionRebuildInput,
    ProjectionRebuildStage,
};

/// Narrow supplied-dependency seam for adverse qualification fixtures.
/// Production delegates to the composed Client; no credential leaves this port.
#[async_trait]
pub trait ProjectionQualifier: Send + Sync {
    /// Qualify only the exact staged preview, returning redacted transport errors.
    async fn qualify(
        &self,
        preview: &ProjectionPreview,
    ) -> Result<ProjectionQualification, kontor_memory_cognee::Error>;
}

#[async_trait]
impl ProjectionQualifier for kontor_memory_cognee::Client {
    async fn qualify(
        &self,
        preview: &ProjectionPreview,
    ) -> Result<ProjectionQualification, kontor_memory_cognee::Error> {
        self.qualify(preview).await
    }
}

impl Services {
    /// Same application caller as production, with a synthetic adverse qualifier.
    /// A supplied client is still required. This is never an HTTP activation port.
    #[doc(hidden)]
    pub async fn rebuild_memory_projection_with_qualifier(
        &self,
        project: ProjectId,
        key: &IdempotencyKey,
        request: &kontor_api::memory::ProjectionRebuildRequest,
        qualifier: Option<&dyn ProjectionQualifier>,
    ) -> Result<ProjectionReadback, ApiError> {
        let state = self.state()?;
        let input = ProjectionRebuildInput {
            expected_generation: request.expected_generation,
            expected_memory_cursor: request.expected_memory_cursor,
            preview_digest: request.preview_digest.clone(),
        };
        if let Some(original) = state
            .with_store(|store| store.replay_projection_rebuild(project, key, &input))
            .map_err(|error| kontor_api::memory::map(state, error))?
        {
            return Ok(original);
        }
        let Some(qualifier) = qualifier.filter(|_| self.memory_cognee.get().is_some()) else {
            return Err(self.deny(
                ApiErrorCode::ProjectionUnavailable,
                "the semantic projection adapter is unavailable",
            ));
        };
        let preview = match state
            .with_store(|store| store.stage_projection_rebuild(project, key, &input))
            .map_err(|error| kontor_api::memory::map(state, error))?
        {
            ProjectionRebuildStage::Replay(original) => return Ok(original),
            ProjectionRebuildStage::Staged(preview) => preview,
        };
        let qualification = qualifier.qualify(&preview).await.map_err(|_| {
            self.deny(
                ApiErrorCode::ProjectionUnavailable,
                "the semantic projection qualification is unavailable",
            )
        })?;
        state
            .with_store(|store| {
                store.activate_projection_rebuild(project, key, &input, &qualification)
            })
            .map_err(|error| kontor_api::memory::map(state, error))
    }
}
