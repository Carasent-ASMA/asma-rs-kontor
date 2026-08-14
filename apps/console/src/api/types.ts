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
