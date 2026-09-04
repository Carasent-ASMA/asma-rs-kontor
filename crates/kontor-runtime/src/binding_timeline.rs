//! Transient, binding-scoped timeline projection for governed handoffs.
//!
//! Session content remains runtime-owned and is never persisted here. This
//! boundary binds every source event to the exact Kontor binding and native
//! runtime generation that produced it, validates one continuous timeline
//! epoch, and exposes only message events to a handoff summarizer.

use kontor_core::id::RuntimeBindingId;
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::SuccessionTimelineRange;

use crate::timeline::{SessionEvent, SessionEventKind};

/// One transient session event carrying the authority that scoped its read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingTimelineEvent {
    runtime_binding_id: RuntimeBindingId,
    native_identity: NativeRuntimeIdentity,
    event: SessionEvent,
}

impl BindingTimelineEvent {
    /// Bind a runtime event to the exact immutable session read that returned it.
    #[must_use]
    pub const fn new(
        runtime_binding_id: RuntimeBindingId,
        native_identity: NativeRuntimeIdentity,
        event: SessionEvent,
    ) -> Self {
        Self {
            runtime_binding_id,
            native_identity,
            event,
        }
    }
}

/// A structural refusal produced before session content reaches a summarizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BindingTimelineProjectionError {
    /// An event was read through another Kontor binding.
    #[error("timeline event belongs to another runtime binding")]
    BindingMismatch,
    /// An event belongs to another native session or runtime generation.
    #[error("timeline event belongs to another native runtime identity")]
    NativeIdentityMismatch,
    /// Events from more than one native content epoch were supplied.
    #[error("timeline projection may not span native content epochs")]
    MixedEpochs,
    /// The supplied positions do not form one positive contiguous range.
    #[error("timeline projection requires a positive contiguous source range")]
    NonContiguousRange,
    /// One position carried contradictory native event content.
    #[error("timeline projection contains a contradictory duplicate position")]
    ConflictingDuplicate,
}

/// Message-only view of one immutable binding generation's timeline.
///
/// The covered range describes every source event examined, including
/// non-message events removed by the projection. It therefore remains exact
/// evidence of the native range used for a handoff without retaining tool,
/// permission or diagnostic content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingMessageTimeline {
    runtime_binding_id: RuntimeBindingId,
    native_identity: NativeRuntimeIdentity,
    covered_range: Option<SuccessionTimelineRange>,
    messages: Vec<SessionEvent>,
}

impl BindingMessageTimeline {
    /// Project raw, transient events for one exact binding into message content.
    ///
    /// Input order is not trusted. Events are ordered by their native
    /// [`crate::TimelinePosition`], exact replays are collapsed, and a missing
    /// or contradictory position is refused before filtering.
    ///
    /// # Errors
    /// Returns a structural refusal when any event belongs to another Kontor
    /// binding or native identity/generation, events span epochs, or their
    /// positions do not prove one contiguous source range.
    pub fn project(
        runtime_binding_id: RuntimeBindingId,
        native_identity: NativeRuntimeIdentity,
        events: impl IntoIterator<Item = BindingTimelineEvent>,
    ) -> Result<Self, BindingTimelineProjectionError> {
        let mut events = events.into_iter().collect::<Vec<_>>();
        for candidate in &events {
            if candidate.runtime_binding_id != runtime_binding_id {
                return Err(BindingTimelineProjectionError::BindingMismatch);
            }
            if candidate.native_identity != native_identity {
                return Err(BindingTimelineProjectionError::NativeIdentityMismatch);
            }
        }

        events.sort_by_key(|candidate| candidate.event.position);
        let mut accepted = Vec::with_capacity(events.len());
        for candidate in events {
            let Some(previous): Option<&BindingTimelineEvent> = accepted.last() else {
                if candidate.event.position.sequence == 0 {
                    return Err(BindingTimelineProjectionError::NonContiguousRange);
                }
                accepted.push(candidate);
                continue;
            };
            if candidate.event.position.epoch != previous.event.position.epoch {
                return Err(BindingTimelineProjectionError::MixedEpochs);
            }
            if candidate.event.position == previous.event.position {
                if candidate.event == previous.event {
                    continue;
                }
                return Err(BindingTimelineProjectionError::ConflictingDuplicate);
            }
            if candidate.event.position.sequence != previous.event.position.sequence + 1 {
                return Err(BindingTimelineProjectionError::NonContiguousRange);
            }
            accepted.push(candidate);
        }

        let covered_range =
            accepted
                .first()
                .zip(accepted.last())
                .map(|(first, last)| SuccessionTimelineRange {
                    epoch: first.event.position.epoch,
                    start_sequence: first.event.position.sequence,
                    end_sequence: last.event.position.sequence,
                });
        let messages = accepted
            .into_iter()
            .filter_map(|candidate| {
                (candidate.event.kind == SessionEventKind::Message).then_some(candidate.event)
            })
            .collect();

        Ok(Self {
            runtime_binding_id,
            native_identity,
            covered_range,
            messages,
        })
    }

    /// Exact Kontor runtime binding whose content was projected.
    #[must_use]
    pub const fn runtime_binding_id(&self) -> RuntimeBindingId {
        self.runtime_binding_id
    }

    /// Exact native session identity, including its immutable generation.
    #[must_use]
    pub const fn native_identity(&self) -> &NativeRuntimeIdentity {
        &self.native_identity
    }

    /// Immutable native runtime generation covered by this projection.
    #[must_use]
    pub const fn binding_generation(&self) -> u64 {
        self.native_identity.generation
    }

    /// Exact contiguous source range examined before structural filtering.
    #[must_use]
    pub const fn covered_range(&self) -> Option<SuccessionTimelineRange> {
        self.covered_range
    }

    /// Message events in ascending native timeline order.
    #[must_use]
    pub fn messages(&self) -> &[SessionEvent] {
        &self.messages
    }

    /// Consume the projection and return its ordered message events.
    #[must_use]
    pub fn into_messages(self) -> Vec<SessionEvent> {
        self.messages
    }
}
