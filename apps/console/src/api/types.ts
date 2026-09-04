/**
 * The wire vocabulary, named once.
 *
 * Every type here is an alias into `schema.d.ts`, which `openapi-typescript`
 * generates from `crates/kontor-api/openapi.json`. Nothing in this console
 * declares a wire shape of its own: a field this file cannot reach is a field
 * the realm does not serve, and that is the whole point — a hand-written
 * interface is how a console starts rendering data no contract promised.
 *
 * The committed document is pinned to the running crate by
 * `kontor-api/tests/openapi_contract.rs`, so a DTO change fails the Rust suite
 * until both the document and these types are regenerated.
 */
import type { components } from './schema'

type Schemas = components['schemas']

/** Liveness, identity and how far startup has got. */
export type Health = Schemas['HealthDto']
/** This realm's immutable identity. */
export type Realm = Schemas['RealmDto']
/** Whether startup reconciliation finished. */
export type BarrierState = Schemas['BarrierState']

/** A run snapshot and the control-plane position it is consistent with. */
export type RunSnapshot = Schemas['SnapshotDto_RunDto']
/** A task snapshot and the control-plane position it is consistent with. */
export type TaskSnapshot = Schemas['SnapshotDto_TaskDto']
/** One agent run, as a cross-boundary reader sees it. */
export type Run = RunSnapshot['value']
/** One task, its active workflow and its reduced gate states. */
export type Task = TaskSnapshot['value']
/** The native session one run is bound to. */
export type Binding = Schemas['BindingDto']
/** The orthogonal state of one run, plus how old its newest confirmation is. */
export type Projection = Schemas['ProjectionDto']
/** Which pinned specification revisions an aggregate is running under. */
export type AppliedRevisions = Schemas['AppliedRevisionsDto']
/** A recorded discontinuity a reader is owed. */
export type Gap = Schemas['GapDto']

/** One durable control-plane event. */
export type ControlEvent = Schemas['EventDto']

/** One page of a session's recorded content. */
export type TimelinePage = Schemas['TimelineDto']
/** One item of session content. */
export type TimelineItem = Schemas['TimelineItemDto']
/** One frame of live session content. */
export type StreamFrame = Schemas['StreamFrameDto']
/** The frame the live stream emits instead of an item when it cannot continue. */
export type StreamRefusal = Schemas['StreamRefusalDto']
/** The runtime's answer to one delivered message. */
export type MessageAck = Schemas['MessageAckDto']
/** The runtime's answer to one permission response. */
export type PermissionAck = Schemas['PermissionAckDto']
/** Realm-qualified model catalog projection. */
export type ModelCatalogProjection = Schemas['ModelCatalogDto']
/** One server-held Teams draft. */
export type TeamDraftProjection = Schemas['TeamDraftDto']
/** One Teams draft command body. */
export type TeamDraftRequest = Schemas['TeamDraftRequest']
/** One immutable published Teams revision. */
export type PublishedTeamRevision = Schemas['PublishedTeamRevisionDto']
/** Teams drafts and revisions at one realm cursor. */
export type TeamsProjection = Schemas['TeamsProjectionDto']

/** One project-scoped Operational epic. */
export type EpicProjection = Schemas['EpicProjectionDto']
/** The project's authoritative logical and observed session topology. */
export type TopologyProjection = Schemas['TopologyProjectionDto']
/** The project's current Core Team. */
export type CoreTeam = Schemas['CoreTeamDto']
/** A proposed Core Team composition. */
export type CoreTeamPreviewRequest = Schemas['CoreTeamPreviewRequest']
/** The server's preview of a Core Team change. */
export type CoreTeamPreview = Schemas['CoreTeamPreviewDto']
/** Apply an unchanged Core Team preview. */
export type CoreTeamApplyRequest = Schemas['CoreTeamApplyRequest']
/** One Core Team role plus its explicit epic-presence policy. */
export type CoreTeamSeatSelection = Schemas['CoreTeamSeatSelectionDto']
/** A receipt-backed Core Team write. */
export type CoreTeamOutcome = Schemas['CoreTeamOutcomeDto']
/** The roles a Quick session may select. */
export type QuickRoles = Schemas['QuickRolesDto']
/** Open one Quick session. */
export type EnsureQuickSessionRequest = Schemas['EnsureQuickSessionRequest']
/** One ensured Quick session. */
export type QuickSession = Schemas['QuickSessionDto']
/** The server's promotion preview. */
export type PromotionPreview = Schemas['PromotionPreviewDto']
/** Apply an unchanged promotion preview. */
export type PromotionApplyRequest = Schemas['PromotionApplyRequest']
/** A Quick session after promotion to an epic. */
export type PromotedSession = Schemas['PromotedSessionDto']
/** The server-owned admission and capacity picture for one project. */
export type ProjectCapacity = Schemas['ProjectCapacityDto']
/** Help for every controlled code pinned by one epic. */
export type CodeHelpProjection = Schemas['CodeHelpProjectionDto']
/** One server-owned controlled-code definition. */
export type CodeHelpEntry = Schemas['CodeHelpEntryDto']
/** Published revisions of one consultation or completion profile family. */
export type ProfileCatalog = Schemas['ProfileCatalogDto']
/** One published consultation or completion profile. */
export type ProfileRevision = Schemas['ProfileRevisionDto']
/** Invoke one Advisor or Committee consultation. */
export type InvokeConsultationRequest = Schemas['InvokeConsultationRequest']
/** One receipt-backed Advisor consultation. */
export type AdvisorRun = Schemas['AdvisorRunDto']
/** One receipt-backed Committee consultation. */
export type CommitteeRun = Schemas['CommitteeRunDto']
/** One epic's completion state. */
export type CompletionState = Schemas['CompletionStateDto']
/** A receipt-backed completion transition. */
export type CompletionOutcome = Schemas['CompletionOutcomeDto']
/** Advance completion from the revision shown to the operator. */
export type AdvanceCompletionRequest = Schemas['AdvanceCompletionRequest']
/** One of the two remediation authorities, as a closed tagged action. */
export type RemediationAction = Schemas['RemediationActionDto']
/** Return completion to remediation from the revision shown to the operator. */
export type RemediateCompletionRequest = Schemas['RemediateCompletionRequest']
/** The receipt every Operational mutation confirms with. */
export type MutationReceipt = Schemas['MutationReceiptDto']
/** One seat a topology node hosts. */
export type TopologySeat = Schemas['TopologySeatDto']
/** One selectable catalog role. */
export type RoleCatalogEntry = Schemas['RoleCatalogEntryDto']
/** The role identity accepted by a write. */
export type RoleSelection = Schemas['RoleSelectionDto']
/** One immutable server-owned revision reference. */
export type RevisionRef = Schemas['RevisionRefDto']
/** One provider/account quota projection. */
export type ProviderQuotaState = Schemas['ProviderQuotaStateDto']
/** One live delivery seat joined to its exact account and provider quota projections. */
export type SeatQuotaState = Schemas['SeatQuotaStateDto']
/** One bodyless, server-evidenced delivery-seat recovery result. */
export type SeatRecovery = Schemas['SeatRecoveryDto']
/** The runtime's evidence-backed result of settling one run. */
export type RuntimeSettlement = Schemas['RuntimeSettlementDto']
/** The exact Admin request to replace one terminal persistent delivery seat. */
export type ReplaceSeatRequest = Schemas['ReplaceSeatRequest']
/** One receipt-backed persistent-seat successor. */
export type ReplacedSeat = Schemas['ReplacedSeatDto']
/** A compare-and-swap request to observe one exact topology seat. */
export type SeatBindingRequest = Schemas['SeatBindingRequest']
/** The server readback after observing one topology seat. */
export type SeatBindingOutcome = Schemas['SeatBindingOutcomeDto']
/** A compare-and-swap request to recover one Committee native filler. */
export type RecoverConsultationSeatRequest = Schemas['RecoverConsultationSeatRequest']
/** A receipt-backed, identity-preserving Committee-seat recovery. */
export type ConsultationSeatRecovery = Schemas['ConsultationSeatRecoveryDto']

/** The JSON body every refusal is reported with. */
export type Refusal = Schemas['ApiErrorBody']

/**
 * An opaque document the contract types only as `Object`.
 *
 * `utoipa` renders those as `Record<string, never>`, which is TypeScript for
 * "no keys" — true of the schema and false of the value. Widening happens here,
 * in one place, rather than by asserting at each of the dozen call sites that
 * read a payload or a gate map.
 */
export type Opaque = Readonly<Record<string, unknown>>

/** Read an `Object`-typed contract field as the document it actually is. */
export function opaque(value: unknown): Opaque {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Opaque)
    : {}
}

/**
 * The realm-qualified identity of one cached aggregate.
 *
 * Every cache in this console is keyed by it, so two realms can never merge:
 * an id is only ever meaningful inside the realm that issued it.
 */
export type EntityKey = string & { readonly __entityKey: unique symbol }

/** Build the cache key for one aggregate in one realm. */
export function entityKey(realmId: string, aggregateId: string): EntityKey {
  return `${realmId}/${aggregateId}` as EntityKey
}
