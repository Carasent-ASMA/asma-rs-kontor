//! Contract tests for the transient, immutable-binding timeline projection.

use kontor_core::id::{
    CanonicalDocument, ExternalId, ExternalName, RuntimeBindingId, RuntimeKindKey,
    parse_utc_timestamp,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::SuccessionTimelineRange;
use kontor_runtime::timeline::EventSubject;
use kontor_runtime::{
    BindingMessageTimeline, BindingTimelineEvent, BindingTimelineProjectionError, SessionEvent,
    SessionEventKind, TimelinePosition,
};

fn identity(generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime kind"),
        host: ExternalName::parse("local").expect("host"),
        generation,
        native_id: ExternalId::parse("native-session-17").expect("native id"),
    }
}

fn event(
    runtime_binding_id: RuntimeBindingId,
    native_identity: &NativeRuntimeIdentity,
    kind: SessionEventKind,
    epoch: u64,
    sequence: u64,
) -> BindingTimelineEvent {
    event_with_marker(
        runtime_binding_id,
        native_identity,
        kind,
        epoch,
        sequence,
        "canonical",
    )
}

fn event_with_marker(
    runtime_binding_id: RuntimeBindingId,
    native_identity: &NativeRuntimeIdentity,
    kind: SessionEventKind,
    epoch: u64,
    sequence: u64,
    marker: &str,
) -> BindingTimelineEvent {
    BindingTimelineEvent::new(
        runtime_binding_id,
        native_identity.clone(),
        SessionEvent {
            kind,
            position: TimelinePosition { epoch, sequence },
            subject: EventSubject::None,
            native_event_id: None,
            emitted_at: parse_utc_timestamp("2026-09-04T09:00:00Z").expect("canonical timestamp"),
            payload: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "sequence": sequence,
                "marker": marker,
            }))
            .expect("canonical event payload"),
        },
    )
}

#[test]
fn projection_returns_only_messages_in_position_order_and_covers_the_source_range() {
    let binding_id = RuntimeBindingId::generate();
    let native = identity(7);
    let projected = BindingMessageTimeline::project(
        binding_id,
        native.clone(),
        [
            event(binding_id, &native, SessionEventKind::Log, 3, 6),
            event(binding_id, &native, SessionEventKind::StateChange, 3, 7),
            event(binding_id, &native, SessionEventKind::Message, 3, 5),
            event(
                binding_id,
                &native,
                SessionEventKind::PermissionResolved,
                3,
                4,
            ),
            event(
                binding_id,
                &native,
                SessionEventKind::PermissionRequest,
                3,
                3,
            ),
            event(binding_id, &native, SessionEventKind::ToolCall, 3, 2),
            event(binding_id, &native, SessionEventKind::Message, 3, 1),
        ],
    )
    .expect("one binding generation and epoch project deterministically");

    assert_eq!(projected.runtime_binding_id(), binding_id);
    assert_eq!(projected.native_identity(), &native);
    assert_eq!(projected.binding_generation(), 7);
    assert_eq!(
        projected.covered_range(),
        Some(SuccessionTimelineRange {
            epoch: 3,
            start_sequence: 1,
            end_sequence: 7,
        })
    );
    assert_eq!(
        projected
            .messages()
            .iter()
            .map(|event| (event.kind, event.position.sequence))
            .collect::<Vec<_>>(),
        vec![
            (SessionEventKind::Message, 1),
            (SessionEventKind::Message, 5),
        ]
    );
}

#[test]
fn projection_refuses_foreign_bindings_generations_and_mixed_epochs() {
    let binding_id = RuntimeBindingId::generate();
    let other_binding_id = RuntimeBindingId::generate();
    let native = identity(7);
    let newer_generation = identity(8);

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [event(
                other_binding_id,
                &native,
                SessionEventKind::Message,
                3,
                1,
            )],
        )
        .expect_err("a foreign binding must not be projected"),
        BindingTimelineProjectionError::BindingMismatch
    );

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [event(
                binding_id,
                &newer_generation,
                SessionEventKind::Message,
                3,
                1,
            )],
        )
        .expect_err("a reused native id in another generation is foreign"),
        BindingTimelineProjectionError::NativeIdentityMismatch
    );

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [
                event(binding_id, &native, SessionEventKind::Message, 3, 1),
                event(binding_id, &native, SessionEventKind::Message, 4, 2),
            ],
        )
        .expect_err("one handoff may not span timeline epochs"),
        BindingTimelineProjectionError::MixedEpochs
    );
}

#[test]
fn projection_refuses_zero_starting_positions_and_noncontiguous_ranges() {
    let binding_id = RuntimeBindingId::generate();
    let native = identity(7);

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [event(binding_id, &native, SessionEventKind::Message, 3, 0,)],
        )
        .expect_err("zero is an anchor, not an event position"),
        BindingTimelineProjectionError::NonContiguousRange
    );

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [
                event(binding_id, &native, SessionEventKind::Message, 3, 1),
                event(binding_id, &native, SessionEventKind::Message, 3, 3),
            ],
        )
        .expect_err("a covered range may not conceal a missing source position"),
        BindingTimelineProjectionError::NonContiguousRange
    );
}

#[test]
fn projection_collapses_exact_replays_without_changing_coverage() {
    let binding_id = RuntimeBindingId::generate();
    let native = identity(7);
    let first = event(binding_id, &native, SessionEventKind::Message, 3, 1);
    let projected = BindingMessageTimeline::project(
        binding_id,
        native,
        [
            first.clone(),
            event(binding_id, &identity(7), SessionEventKind::Message, 3, 2),
            first,
        ],
    )
    .expect("an exact replay is one source position");

    assert_eq!(
        projected.covered_range(),
        Some(SuccessionTimelineRange {
            epoch: 3,
            start_sequence: 1,
            end_sequence: 2,
        })
    );
    assert_eq!(
        projected
            .messages()
            .iter()
            .map(|event| event.position.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn projection_refuses_contradictory_content_at_one_position() {
    let binding_id = RuntimeBindingId::generate();
    let native = identity(7);

    assert_eq!(
        BindingMessageTimeline::project(
            binding_id,
            native.clone(),
            [
                event_with_marker(
                    binding_id,
                    &native,
                    SessionEventKind::Message,
                    3,
                    1,
                    "first",
                ),
                event_with_marker(
                    binding_id,
                    &native,
                    SessionEventKind::Message,
                    3,
                    1,
                    "contradiction",
                ),
            ],
        )
        .expect_err("one native position cannot carry two payloads"),
        BindingTimelineProjectionError::ConflictingDuplicate
    );
}
