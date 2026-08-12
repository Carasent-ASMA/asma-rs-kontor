/**
 * The control-plane projection, reduced from a snapshot and the durable feed.
 *
 * Pure: no fetch, no timers, no React. Everything the console believes about the
 * control plane is produced by these functions from values the contract handed
 * it, which is what makes the rules below testable without a socket.
 *
 * # The order that makes a subscription honest
 *
 * A reader takes a snapshot, notes the `snapshot_cursor` it is consistent with,
 * and subscribes **strictly after** it. Doing it the other way round — subscribe,
 * then snapshot — leaves a window whose events are attributed to a state that
 * already contained them.
 *
 * # What is never inferred
 *
 * A cursor discontinuity is *not* a gap. `/v1/events` delivers only the event
 * kinds a `RuntimeEvent` can express, and command intents and census rows consume
 * positions in the same log without ever being delivered — so holes in the
 * delivered sequence are normal and permanent. The realm reports a genuinely
 * unreachable position as `resnapshot_required` (HTTP 410), and *that* is the
 * only thing treated as one. A console that inferred gaps from the numbers would
 * resnapshot forever against a perfectly healthy realm.
 *
 * # What is never collapsed
 *
 * `stale`, `diverged`, `runtime_unavailable`, `orphaned` and `lost_contact` are
 * statements about evidence, not outcomes. Nothing here maps them to a terminal
 * state, and the run's `outcome` is carried separately by the contract precisely
 * so that it cannot be confused with them.
 */
import type { ControlEvent, EntityKey, Run, RunSnapshot, Task, TaskSnapshot } from '../api/types'
import { entityKey } from '../api/types'
import type { ControlSnapshot } from '../api/client'

/** How the console currently stands with the durable feed. */
export type FeedContact =
  /** Nothing has been snapshotted yet, so there is nothing to subscribe after. */
  | 'idle'
  /** Subscribed and current. */
  | 'live'
  /** The subscription dropped; what the console holds is as of `cursor`. */
  | 'interrupted'
  /** The realm refused our position; a fresh snapshot is required. */
  | 'resnapshot_required'

/** One cached run, and whether the feed has moved past what it says. */
export interface CachedRun {
  /** The snapshot as the realm served it. */
  readonly value: Run
  /** The control-plane position the snapshot was consistent with. */
  readonly snapshotCursor: number
  /**
   * Whether an event about this run arrived after the snapshot was taken.
   *
   * A behind snapshot is shown as behind. Hiding it would present a value the
   * console already knows is superseded as though it were current.
   */
  readonly behind: boolean
}

/** One cached task, and the position its snapshot was consistent with. */
export interface CachedTask {
  /** The snapshot as the realm served it. */
  readonly value: Task
  /** The control-plane position the snapshot was consistent with. */
  readonly snapshotCursor: number
}

/** Everything the console believes about one realm's control plane. */
export interface ControlState {
  /** The realm every cached value belongs to. */
  readonly realmId: string
  /**
   * The snapshot position the subscription is anchored at.
   *
   * `null` means nothing has been snapshotted, and therefore that nothing may be
   * subscribed either: there is no position for delivery to be strictly after.
   */
  readonly anchor: number | null
  /** The newest control-plane position applied, or the anchor before the first. */
  readonly cursor: number | null
  /** Runs, keyed by `(realm_id, agent_run_id)`. */
  readonly runs: ReadonlyMap<EntityKey, CachedRun>
  /** Tasks, keyed by `(realm_id, task_id)`. */
  readonly tasks: ReadonlyMap<EntityKey, CachedTask>
  /**
   * Runs the feed has mentioned, newest activity first.
   *
   * The contract serves no run *list*, so this is the only honest inventory the
   * console has: the runs this realm actually reported events about. It is
   * evidence of activity, never a claim to be every run in the realm.
   */
  readonly observed: readonly string[]
  /** How the console stands with the feed. */
  readonly contact: FeedContact
  /** The instant the newest event was recorded, as the realm reported it. */
  readonly newestRecordedAt: string | null
}

/** Why an event did not move the projection. */
export type ApplyOutcome =
  /** Reduced normally. */
  | 'applied'
  /** At or behind the newest applied position; already accounted for. */
  | 'duplicate'
  /** From another realm; discarded without being reduced. */
  | 'foreign_realm'

/** The result of offering one event to the projection. */
export interface Applied {
  /** The projection, unchanged unless the outcome is `applied`. */
  readonly state: ControlState
  /** What happened to the event. */
  readonly outcome: ApplyOutcome
}

/** How many runs the observed inventory keeps. */
const OBSERVED_LIMIT = 200

/** An empty projection for one realm. */
export function initialControlState(realmId: string): ControlState {
  return {
    realmId,
    anchor: null,
    cursor: null,
    runs: new Map(),
    tasks: new Map(),
    observed: [],
    contact: 'idle',
    newestRecordedAt: null,
  }
}

/**
 * Adopt the control-plane snapshot the subscription will be anchored at.
 *
 * This is the *first* half of the rule the whole projection rests on: take a
 * snapshot, note the position it is consistent with, and only then subscribe —
 * strictly after that position. Doing it the other way round leaves a window
 * whose events are attributed to a state that already contained them.
 *
 * Every value in a control snapshot is current as of the anchor by definition,
 * so none of them is marked behind. A resnapshot clears the obligation that
 * caused it for the same reason: the values have just been read again.
 */
export function applyControlSnapshot(
  state: ControlState,
  snapshot: ControlSnapshot,
): ControlState {
  if (snapshot.realmId !== state.realmId) {
    return state
  }
  const runs = new Map(state.runs)
  let observed = state.observed
  for (const read of snapshot.runs) {
    if (read.realm_id !== state.realmId) {
      continue
    }
    runs.set(entityKey(state.realmId, read.value.agent_run_id), {
      value: read.value,
      snapshotCursor: read.snapshot_cursor,
      behind: false,
    })
    observed = remember(observed, read.value.agent_run_id)
  }
  return {
    ...state,
    anchor: snapshot.snapshotCursor,
    cursor: snapshot.snapshotCursor,
    runs,
    observed,
    contact: 'idle',
  }
}

/**
 * Adopt one run snapshot.
 *
 * The snapshot's own cursor is kept rather than the feed's: it is the position
 * *this value* is consistent with, and comparing an event to it is how the
 * console knows the value has been superseded.
 */
export function applyRunSnapshot(state: ControlState, snapshot: RunSnapshot): ControlState {
  if (snapshot.realm_id !== state.realmId) {
    return state
  }
  const key = entityKey(state.realmId, snapshot.value.agent_run_id)
  const runs = new Map(state.runs)
  runs.set(key, {
    value: snapshot.value,
    snapshotCursor: snapshot.snapshot_cursor,
    // A snapshot taken at or after the newest applied event is current by
    // construction; one taken before it is already behind and says so.
    behind: state.cursor !== null && state.cursor > snapshot.snapshot_cursor,
  })
  return { ...state, runs, observed: remember(state.observed, snapshot.value.agent_run_id) }
}

/** Adopt one task snapshot. */
export function applyTaskSnapshot(state: ControlState, snapshot: TaskSnapshot): ControlState {
  if (snapshot.realm_id !== state.realmId) {
    return state
  }
  const key = entityKey(state.realmId, snapshot.value.task_id)
  const tasks = new Map(state.tasks)
  tasks.set(key, { value: snapshot.value, snapshotCursor: snapshot.snapshot_cursor })
  return { ...state, tasks }
}

/**
 * Reduce one delivered control-plane event.
 *
 * Delivery is strictly after the position the caller asked from, so an event at
 * or behind the newest applied one is a redelivery and is dropped rather than
 * reduced twice.
 */
export function applyEvent(state: ControlState, event: ControlEvent): Applied {
  if (event.realm_id !== state.realmId) {
    return { state, outcome: 'foreign_realm' }
  }
  if (state.cursor !== null && event.cursor <= state.cursor) {
    return { state, outcome: 'duplicate' }
  }
  const key = entityKey(state.realmId, event.agent_run_id)
  const cached = state.runs.get(key)
  const runs = cached
    ? new Map(state.runs).set(key, {
        ...cached,
        // The event moved past the snapshot, so what is cached is now behind.
        behind: event.cursor > cached.snapshotCursor,
      })
    : state.runs
  return {
    state: {
      ...state,
      cursor: event.cursor,
      runs,
      observed: remember(state.observed, event.agent_run_id),
      contact: 'live',
      newestRecordedAt: event.recorded_at,
    },
    outcome: 'applied',
  }
}

/** Note that the subscription dropped without losing what was already read. */
export function feedInterrupted(state: ControlState): ControlState {
  return state.contact === 'resnapshot_required'
    ? state
    : { ...state, contact: 'interrupted' }
}

/** Note that the subscription is open and current. */
export function feedLive(state: ControlState): ControlState {
  return { ...state, contact: 'live' }
}

/**
 * The realm refused our position: everything must be read again.
 *
 * The cached values are kept and the contact state says why they cannot be
 * trusted to be current. Emptying them would replace "these are as of a position
 * the realm has discarded" with "there is nothing here", and only one of those
 * is true.
 */
export function resnapshotRequired(state: ControlState): ControlState {
  return { ...state, contact: 'resnapshot_required' }
}

/** Move one run to the front of the observed inventory. */
function remember(observed: readonly string[], agentRunId: string): readonly string[] {
  if (observed[0] === agentRunId) {
    return observed
  }
  const next = [agentRunId, ...observed.filter((id) => id !== agentRunId)]
  return next.length > OBSERVED_LIMIT ? next.slice(0, OBSERVED_LIMIT) : next
}

/** Read one cached run, in this realm only. */
export function cachedRun(state: ControlState, agentRunId: string): CachedRun | undefined {
  return state.runs.get(entityKey(state.realmId, agentRunId))
}

/** Read one cached task, in this realm only. */
export function cachedTask(state: ControlState, taskId: string): CachedTask | undefined {
  return state.tasks.get(entityKey(state.realmId, taskId))
}
