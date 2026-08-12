/**
 * One session room's content, reduced from history pages and live frames.
 *
 * Pure, like the control projection, and for the same reason: every rule below
 * is a rule about *ordering and doubt*, and those are the ones that quietly stop
 * holding when they can only be exercised against a live runtime.
 *
 * # The sequence a room follows
 *
 * 1. page `/v1/sessions/{run}/timeline` until `next` comes back absent;
 * 2. subscribe `/stream?after=<the anchor the last page returned>` — the anchor
 *    is mandatory, and delivery is strictly after it;
 * 3. reduce everything by `(epoch, sequence)`.
 *
 * # The doubt rule
 *
 * Four things mean the console can no longer say what this session contains: the
 * realm sends `timeline_refetch_required`, the epoch changes, a sequence is
 * skipped, or the live subscription drops and takes with it any knowledge of what
 * happened while it was shut. All four resolve the same way — close the stream,
 * **discard the content held**, and read the canonical timeline again.
 *
 * What none of them do is say anything about the *run*. Missing content is
 * missing content; a session whose transcript cannot be followed is not a session
 * that ended, and this module has no way to express that it did.
 */
import type { StreamFrame, TimelineItem, TimelinePage } from '../api/types'

/**
 * The kinds the contract's session vocabulary defines.
 *
 * The realm subscribes to every one of them, so a room that recognized only some
 * would present a filtered transcript as a complete one. Anything outside this
 * set still renders — as an item of its own kind, never dropped — which is what
 * keeps a future kind from silently disappearing from a transcript.
 */
export const PERMISSION_REQUEST = 'permission_request'
/** The resolution of a permission request. */
export const PERMISSION_RESOLVED = 'permission_resolved'

/** Where a room stands with the session's content. */
export type SessionPhase =
  /** Reading the canonical timeline. */
  | 'loading'
  /** History is complete and the live subscription is open. */
  | 'live'
  /** History is complete; no subscription is open. */
  | 'idle'
  /** The content held cannot be trusted; the timeline must be read again. */
  | 'refetch_required'

/** How a locally sent permission answer was received. */
export type ResponseState =
  /** Sent, no answer yet. */
  | 'sending'
  /** Applied by the runtime. */
  | 'applied'
  /**
   * A retry was answered with the original acknowledgement.
   *
   * Distinguished from `applied` because it is the evidence that retrying under
   * a held key produced no second effect — which is the property the stable key
   * exists for, and worth showing rather than flattening into success.
   */
  | 'replayed'
  /** The same key was already used to commit a different effect. */
  | 'conflict'
  /** This runtime does not take permission answers. */
  | 'unsupported'
  /** The realm refused for some other stated reason. */
  | 'refused'

/** The local record of one permission answer. */
export interface ResponseReceipt {
  /** The stable response key this answer is sent under, across retries. */
  readonly responseId: string
  /** The decision that was sent. */
  readonly decision: string
  /** How it was received. */
  readonly state: ResponseState
  /** The realm's own stable code, when it refused. */
  readonly code: string | null
  /** The realm's own static rule, when it refused. */
  readonly rule: string | null
}

/** Everything one room believes about one session's content. */
export interface SessionState {
  /** The realm the session belongs to. */
  readonly realmId: string
  /** The run whose session this is. */
  readonly agentRunId: string
  /** The content epoch every held item belongs to. */
  readonly epoch: number | null
  /** The items, in ascending position order. */
  readonly items: readonly TimelineItem[]
  /** The history continuation, or `null` when history is exhausted. */
  readonly next: string | null
  /** The anchor a live subscription must start strictly after. */
  readonly anchor: string | null
  /** Where the room stands. */
  readonly phase: SessionPhase
  /** Why the content was discarded, when it was. */
  readonly refetchReason: string | null
  /** Locally sent permission answers, keyed by the runtime's request id. */
  readonly responses: ReadonlyMap<string, ResponseReceipt>
  /** The positions already reduced, so a redelivery is dropped not doubled. */
  readonly seen: ReadonlySet<string>
}

/** What happened to one offered page or frame. */
export type SessionOutcome =
  /** Reduced normally. */
  | 'applied'
  /** Already reduced at that position. */
  | 'duplicate'
  /** From another realm or another run. */
  | 'foreign'
  /** The content held was discarded; the timeline must be read again. */
  | 'refetch_required'

/** The result of offering a page or a frame to a room. */
export interface SessionApplied {
  /** The room, unchanged unless something was reduced or discarded. */
  readonly state: SessionState
  /** What happened. */
  readonly outcome: SessionOutcome
}

/** An empty room for one session. */
export function initialSessionState(realmId: string, agentRunId: string): SessionState {
  return {
    realmId,
    agentRunId,
    epoch: null,
    items: [],
    next: null,
    anchor: null,
    phase: 'loading',
    refetchReason: null,
    responses: new Map(),
    seen: new Set(),
  }
}

/** The position key one item occupies. */
function positionKey(epoch: number, sequence: number): string {
  return `${epoch}:${sequence}`
}

/** The newest position reduced, or `null` when nothing has been. */
export function lastPosition(
  state: SessionState,
): { epoch: number; sequence: number } | null {
  const last = state.items.at(-1)
  return last ? { epoch: last.epoch, sequence: last.sequence } : null
}

/**
 * Discard the content held and demand a fresh read.
 *
 * The reason is kept and shown. A room that cleared itself without saying why
 * would be indistinguishable from a session that never had any content.
 */
export function requireRefetch(state: SessionState, reason: string): SessionState {
  return {
    ...state,
    epoch: null,
    items: [],
    next: null,
    anchor: null,
    phase: 'refetch_required',
    refetchReason: reason,
    seen: new Set(),
    // Locally sent answers are kept: they record what this operator did, and
    // the runtime's ledger did not forget them just because we did.
    responses: state.responses,
  }
}

/** Begin reading the canonical timeline again, from the start. */
export function beginRefetch(state: SessionState): SessionState {
  return {
    ...initialSessionState(state.realmId, state.agentRunId),
    responses: state.responses,
    refetchReason: state.refetchReason,
  }
}

/**
 * Reduce one page of canonical history.
 *
 * A page that changes the epoch is not a continuation of what is held — the
 * runtime renumbered — so the content held is discarded rather than appended to.
 */
export function applyTimelinePage(
  state: SessionState,
  page: TimelinePage,
): SessionApplied {
  if (page.realm_id !== state.realmId || page.agent_run_id !== state.agentRunId) {
    return { state, outcome: 'foreign' }
  }
  if (state.epoch !== null && page.epoch !== state.epoch) {
    return {
      state: requireRefetch(state, 'the runtime renumbered this session while it was being read'),
      outcome: 'refetch_required',
    }
  }

  const seen = new Set(state.seen)
  const items = [...state.items]
  let reduced = 0
  for (const item of page.items) {
    const key = positionKey(item.epoch, item.sequence)
    if (seen.has(key)) {
      continue
    }
    seen.add(key)
    items.push(item)
    reduced += 1
  }
  items.sort(byPosition)

  return {
    state: {
      ...state,
      epoch: page.epoch,
      items,
      next: page.next ?? null,
      anchor: page.anchor,
      // Only a page with no continuation completes the history. Until then the
      // room is still loading, and subscribing would anchor to the middle of it.
      phase: page.next ? 'loading' : 'idle',
      seen,
    },
    outcome: reduced > 0 || page.items.length === 0 ? 'applied' : 'duplicate',
  }
}

/** Note that the live subscription is open. */
export function streamOpened(state: SessionState): SessionState {
  return state.phase === 'idle' ? { ...state, phase: 'live' } : state
}

/**
 * Reduce one live content frame.
 *
 * Delivery is strictly after the anchor, so a frame at or behind the newest
 * reduced position is a redelivery. A frame that skips a sequence, or arrives in
 * a different epoch, means content this room will never see — which is the doubt
 * rule, not a gap to be papered over.
 */
export function applyLiveFrame(state: SessionState, frame: StreamFrame): SessionApplied {
  if (frame.realm_id !== state.realmId || frame.agent_run_id !== state.agentRunId) {
    return { state, outcome: 'foreign' }
  }
  const item = frame.item
  if (state.epoch !== null && item.epoch !== state.epoch) {
    return {
      state: requireRefetch(state, 'the runtime renumbered this session’s content'),
      outcome: 'refetch_required',
    }
  }
  const last = lastPosition(state)
  if (last) {
    if (item.sequence <= last.sequence) {
      return { state, outcome: 'duplicate' }
    }
    if (item.sequence > last.sequence + 1) {
      return {
        state: requireRefetch(
          state,
          'the live stream skipped a position, so content is missing from this transcript',
        ),
        outcome: 'refetch_required',
      }
    }
  }
  const key = positionKey(item.epoch, item.sequence)
  if (state.seen.has(key)) {
    return { state, outcome: 'duplicate' }
  }
  const seen = new Set(state.seen)
  seen.add(key)
  return {
    state: {
      ...state,
      epoch: item.epoch,
      items: [...state.items, item],
      phase: 'live',
      seen,
    },
    outcome: 'applied',
  }
}

/**
 * The realm reported that the timeline can no longer be followed.
 *
 * The realm's own static rule is kept as the reason, so what is shown is what the
 * realm said rather than this console's paraphrase of it.
 */
export function applyStreamRefusal(state: SessionState, rule: string): SessionState {
  return requireRefetch(state, rule)
}

/**
 * The live subscription dropped.
 *
 * Whatever the runtime emitted while it was shut is unknown and unrecoverable
 * from here, so this is the doubt rule too. Resubscribing from the old anchor
 * would present a transcript with an invisible hole in it.
 */
export function streamInterrupted(state: SessionState): SessionState {
  if (state.phase === 'refetch_required') {
    return state
  }
  return requireRefetch(
    state,
    'the live subscription dropped, so what the session emitted while it was closed is unknown',
  )
}

/** Record that an answer to one permission request has been sent. */
export function responseSent(
  state: SessionState,
  requestId: string,
  responseId: string,
  decision: string,
): SessionState {
  const responses = new Map(state.responses)
  responses.set(requestId, { responseId, decision, state: 'sending', code: null, rule: null })
  return { ...state, responses }
}

/** Record how the realm received one permission answer. */
export function responseSettled(
  state: SessionState,
  requestId: string,
  settled: Pick<ResponseReceipt, 'state' | 'code' | 'rule'>,
): SessionState {
  const existing = state.responses.get(requestId)
  if (!existing) {
    return state
  }
  const responses = new Map(state.responses)
  responses.set(requestId, { ...existing, ...settled })
  return { ...state, responses }
}

/** One permission request the transcript raised, and where it stands. */
export interface PermissionEntry {
  /** The runtime's own request id. */
  readonly requestId: string
  /** The item that raised it. */
  readonly request: TimelineItem
  /** The item that resolved it, when the transcript shows one. */
  readonly resolution: TimelineItem | null
  /** This operator's own answer, when one was sent from here. */
  readonly receipt: ResponseReceipt | null
}

/**
 * The permission requests this transcript raises, in the order it raised them.
 *
 * Derived rather than stored: the transcript is the record, and a separate ledger
 * would be a second place for the truth to live. A request the transcript shows
 * resolved is resolved — whoever answered it, and whether or not this console was
 * the one that did.
 */
export function permissionLedger(state: SessionState): readonly PermissionEntry[] {
  const resolutions = new Map<string, TimelineItem>()
  for (const item of state.items) {
    if (item.kind === PERMISSION_RESOLVED && item.permission_id) {
      resolutions.set(item.permission_id, item)
    }
  }
  const entries: PermissionEntry[] = []
  for (const item of state.items) {
    if (item.kind !== PERMISSION_REQUEST || !item.permission_id) {
      continue
    }
    entries.push({
      requestId: item.permission_id,
      request: item,
      resolution: resolutions.get(item.permission_id) ?? null,
      receipt: state.responses.get(item.permission_id) ?? null,
    })
  }
  return entries
}

/** Whether one permission entry may still be answered from here. */
export function isAnswerable(entry: PermissionEntry): boolean {
  return entry.resolution === null && entry.receipt?.state !== 'sending'
}

/** Order two items by their position inside the session. */
function byPosition(left: TimelineItem, right: TimelineItem): number {
  return left.epoch === right.epoch
    ? left.sequence - right.sequence
    : left.epoch - right.epoch
}
