//! `kontor-runtime` — Adapter contract, capabilities and normalized events for agent runtimes
//!
//! This crate owns the replaceable execution contract: what a runtime must be
//! able to prove, what Kontor is allowed to conclude from what it says, and how
//! a session's content is read without a gap or a duplicate. It has no provider
//! code and no transport: Paseo, AO and Codex adapters implement
//! [`adapter::RuntimeAdapter`] and change nothing here.
//!
//! Eight invariants hold across every module, and the contract suite exists to
//! keep them holding:
//!
//! 1. **An undeclared capability produces no effect.** Every operation passes
//!    [`capability::preflight`] before it reaches a runtime.
//! 2. **Trust is evidence, not optimism.** An advisory (Grade C) runtime may be
//!    discovered, inspected and read, but Kontor never drives it on its own
//!    authority and never closes a run on what it reports.
//! 3. **Capabilities freeze into the binding.** A binding keeps the exact
//!    capabilities, grade and limits of the moment it was created, so a later
//!    adapter upgrade cannot rewrite the evidence quality of an earlier run.
//! 4. **Native ids never become Kontor ids.** A native session or workspace id
//!    is correlation evidence inside
//!    [`kontor_core::state::NativeRuntimeIdentity`] and nowhere else.
//! 5. **One team run, one verified task workspace.** Preparation is idempotent
//!    per team run; every role of a same-runtime team launches through the same
//!    binding, and a role claiming another root — or no binding at all — is
//!    refused before it can edit anything.
//! 6. **Content is exactly once.** Cursor-paginated history anchors a live
//!    subscription that starts strictly after it; an epoch change or a sequence
//!    gap stops the stream and demands a refetch instead of continuing.
//! 7. **Uncertainty is not completion.** A command acknowledgement, a closed
//!    stream, an unreachable runtime or a Grade C report never closes a run.
//!    Only a matching authoritative event or a fresh inspect result can.
//! 8. **One seat, one session.** A launch exists only because a runtime admitted
//!    it: [`admission`] keys that decision on the team run and role slot, claims
//!    it atomically, and consumes it before the first native effect. Replay, a
//!    concurrent caller, freshly minted run and binding ids, borrowed authority
//!    and a restart race all end in a typed refusal with nothing started.

pub mod adapter;
pub mod admission;
pub mod capability;
pub mod container;
pub mod fake;
pub mod observation;
pub mod request;
pub mod scope;
pub mod timeline;
pub mod workspace;

pub use adapter::{
    ConsultationCredential, ConsultationFallbackDisposition, ConsultationLaunchOutcome,
    ConsultationLaunchRequest, ConsultationMessageRequest, ConsultationRouteProvenance,
    ConsultationRouteSource, ConsultationSeatRetireOutcome, ConsultationSeatRetireRequest,
    HostedSeatClaimOutcome, HostedSeatClaimPredecessor, HostedSeatClaimPreview,
    HostedSeatClaimRequest, HostedSeatInspectRequest, HostedSeatInspection, HostedSeatNativeState,
    HostedSeatTitleConflict, LaunchOutcome, MessageAck, PermissionAck, RuntimeAdapter,
    RuntimeError, RuntimeResult, ScopedSeatCredential,
};
pub use admission::{
    AdmissionLedger, AdmissionOutcome, AdmissionRequest, AdmissionTicket, ClaimedSeat,
    LaunchAuthority, OccupiedSeat, ReplacedBinding, RoleSlotKey, SeatFacts,
};
pub use capability::{
    IssuedBinding, IssuedBindingRegistry, LimitDemand, OperationContext, RuntimeBindingSnapshot,
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade, preflight,
};
pub use fake::{RequestKey, RuntimeScript, ScriptStep, ScriptedFakeRuntime};
pub use observation::{
    ControlPlaneObservation, CorrelationEvidence, NativeSession, ObservationSource,
    ReconciliationAction, ReconciliationFinding, ReconciliationReport, reconcile,
};
pub use request::{
    AdoptRequest, CancelRequest, CorrelationLabel, HistoryRequest, InspectRequest, LaunchParts,
    LaunchPlacement, LaunchRequest, LiveSubscribeRequest, MessageId, PermissionDecision,
    PermissionResponseRequest, PlacementClaim, ResumeRequest, SendMessageRequest,
};
pub use scope::{EpicScope, ExecutionScope, TaskScope};
pub use timeline::{
    HistoryCursor, HistoryPage, HistoryReader, LiveSubscription, SessionEvent, SessionEventKind,
    TimelineBreak, TimelinePosition,
};
pub use workspace::{
    WorkspaceBinding, WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspaceClaim,
    WorkspaceCorrelationEvidence, WorkspaceLabel, WorkspaceOutcome, WorkspacePrepareRequest,
    WorkspaceRoot,
};
