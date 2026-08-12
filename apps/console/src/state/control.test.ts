/**
 * The control projection reduces a snapshot and a strictly-after feed.
 *
 * The mutants this file exists to kill:
 *
 * * subscribing from a position that is not the snapshot's own;
 * * reducing a redelivered event twice;
 * * letting a value from another realm into a cache;
 * * merging two realms under one id;
 * * showing a snapshot the feed has already moved past as though it were current;
 * * turning an unreachable position into anything other than a resnapshot;
 * * reporting a stale, diverged or unreachable run as finished.
 */
import { describe, expect, it } from 'vitest'
import {
  applyControlSnapshot,
  applyEvent,
  applyRunSnapshot,
  applyTaskSnapshot,
  cachedRun,
  cachedTask,
  feedInterrupted,
  initialControlState,
  resnapshotRequired,
} from './control'
import {
  OTHER_REALM,
  PROJECT,
  REALM,
  RUN,
  controlEvent,
  run,
  runSnapshot,
  task,
  taskSnapshot,
} from '../test/fixtures'
import { entityKey } from '../api/types'

describe('control projection', () => {
  it('has no anchor, and therefore nothing to subscribe after, until it snapshots', () => {
    const state = initialControlState(REALM)
    // `anchor === null` is what stops the shell opening a subscription: there is
    // no position for delivery to be strictly after.
    expect(state.anchor).toBeNull()
    expect(state.cursor).toBeNull()
    expect(state.contact).toBe('idle')
  })

  it('anchors on the control snapshot and starts the cursor there', () => {
    const state = applyControlSnapshot(initialControlState(REALM), {
      realmId: REALM,
      snapshotCursor: 40,
      runs: [runSnapshot(run(), 40)],
    })
    expect(state.anchor).toBe(40)
    // The subscription resumes strictly after this, so the newest applied
    // position starts at the anchor rather than at nothing.
    expect(state.cursor).toBe(40)
    expect(cachedRun(state, RUN)?.behind).toBe(false)
    expect(state.observed).toEqual([RUN])
  })

  it('anchors at the lowest position any snapshot in it is consistent with', () => {
    const other = run({ agent_run_id: 'run-later' })
    const state = applyControlSnapshot(initialControlState(REALM), {
      realmId: REALM,
      snapshotCursor: 40,
      runs: [runSnapshot(run(), 40), runSnapshot(other, 55)],
    })
    expect(state.anchor).toBe(40)
    // Each value keeps the position *it* is consistent with, so an event at 50
    // supersedes the first and is already included in the second.
    expect(cachedRun(state, RUN)?.snapshotCursor).toBe(40)
    expect(cachedRun(state, 'run-later')?.snapshotCursor).toBe(55)

    const advanced = applyEvent(state, controlEvent({ cursor: 50 })).state
    expect(cachedRun(advanced, RUN)?.behind).toBe(true)
    const later = applyEvent(advanced, controlEvent({ cursor: 51, agent_run_id: 'run-later' }))
      .state
    expect(cachedRun(later, 'run-later')?.behind).toBe(false)
  })

  it('refuses a control snapshot from another realm', () => {
    const state = initialControlState(REALM)
    expect(
      applyControlSnapshot(state, {
        realmId: OTHER_REALM,
        snapshotCursor: 1,
        runs: [runSnapshot(run(), 1, OTHER_REALM)],
      }),
    ).toBe(state)
  })

  it('drops an event at or before the anchor as already accounted for', () => {
    const state = applyControlSnapshot(initialControlState(REALM), {
      realmId: REALM,
      snapshotCursor: 40,
      runs: [runSnapshot(run(), 40)],
    })
    // Delivery is strictly after the anchor, so anything at or below it is a
    // redelivery of something the snapshot already contains.
    expect(applyEvent(state, controlEvent({ cursor: 40 })).outcome).toBe('duplicate')
    expect(applyEvent(state, controlEvent({ cursor: 39 })).outcome).toBe('duplicate')
    expect(applyEvent(state, controlEvent({ cursor: 41 })).outcome).toBe('applied')
  })

  it('adopts a snapshot and keeps the position it is consistent with', () => {
    const state = applyRunSnapshot(initialControlState(REALM), runSnapshot(run(), 42))
    expect(cachedRun(state, RUN)?.snapshotCursor).toBe(42)
    expect(cachedRun(state, RUN)?.behind).toBe(false)
    // The snapshot's cursor is not the feed's: subscribing happens from the
    // snapshot's own position, and nothing has been delivered yet.
    expect(state.cursor).toBeNull()
  })

  it('reduces an event strictly after the newest applied position', () => {
    let state = applyRunSnapshot(initialControlState(REALM), runSnapshot(run(), 10))
    const first = applyEvent(state, controlEvent({ cursor: 11 }))
    expect(first.outcome).toBe('applied')
    state = first.state
    expect(state.cursor).toBe(11)

    const second = applyEvent(state, controlEvent({ cursor: 12 }))
    expect(second.outcome).toBe('applied')
    expect(second.state.cursor).toBe(12)
  })

  it('drops a redelivered event instead of reducing it twice', () => {
    const seeded = applyEvent(initialControlState(REALM), controlEvent({ cursor: 20 })).state
    for (const cursor of [20, 19, 1]) {
      const replayed = applyEvent(seeded, controlEvent({ cursor }))
      expect(replayed.outcome).toBe('duplicate')
      expect(replayed.state).toBe(seeded)
    }
  })

  it('refuses an event from another realm without reducing it', () => {
    const state = initialControlState(REALM)
    const foreign = applyEvent(state, controlEvent({ realm_id: OTHER_REALM, cursor: 99 }))
    expect(foreign.outcome).toBe('foreign_realm')
    expect(foreign.state).toBe(state)
    expect(foreign.state.cursor).toBeNull()
  })

  it('refuses a snapshot from another realm', () => {
    const state = initialControlState(REALM)
    expect(applyRunSnapshot(state, runSnapshot(run(), 1, OTHER_REALM))).toBe(state)
    expect(applyTaskSnapshot(state, taskSnapshot(task(), 1, OTHER_REALM))).toBe(state)
  })

  it('keys every cache by realm as well as by id', () => {
    const here = applyRunSnapshot(initialControlState(REALM), runSnapshot(run(), 1))
    // The same aggregate id in another realm is a different key, so a second
    // realm's value can never be read out of this realm's cache.
    expect(here.runs.has(entityKey(REALM, RUN))).toBe(true)
    expect(here.runs.has(entityKey(OTHER_REALM, RUN))).toBe(false)
    expect(cachedRun({ ...here, realmId: OTHER_REALM }, RUN)).toBeUndefined()
  })

  it('shows a snapshot the feed has moved past as behind', () => {
    let state = applyRunSnapshot(initialControlState(REALM), runSnapshot(run(), 10))
    expect(cachedRun(state, RUN)?.behind).toBe(false)
    state = applyEvent(state, controlEvent({ cursor: 11 })).state
    expect(cachedRun(state, RUN)?.behind).toBe(true)

    // Re-reading it at the newer position clears the flag rather than hiding it.
    state = applyRunSnapshot(state, runSnapshot(run(), 11))
    expect(cachedRun(state, RUN)?.behind).toBe(false)
  })

  it('records a task snapshot under its own id', () => {
    const state = applyTaskSnapshot(initialControlState(REALM), taskSnapshot(task(), 7))
    expect(cachedTask(state, task().task_id)?.value.project_id).toBe(PROJECT)
    expect(cachedTask(state, task().task_id)?.snapshotCursor).toBe(7)
  })

  it('keeps what it holds when the realm demands a resnapshot, and says why', () => {
    let state = applyRunSnapshot(initialControlState(REALM), runSnapshot(run(), 5))
    state = resnapshotRequired(state)
    expect(state.contact).toBe('resnapshot_required')
    // The values are still there, marked as of a position the realm discarded.
    expect(cachedRun(state, RUN)).toBeDefined()
    // And an interruption cannot downgrade that obligation.
    expect(feedInterrupted(state).contact).toBe('resnapshot_required')

    // A fresh snapshot is what clears it, because the values have just been read
    // again — not a flag flipped on its own.
    state = applyControlSnapshot(state, {
      realmId: REALM,
      snapshotCursor: 80,
      runs: [runSnapshot(run(), 80)],
    })
    expect(state.contact).toBe('idle')
    expect(state.anchor).toBe(80)
    expect(state.cursor).toBe(80)
    expect(cachedRun(state, RUN)?.behind).toBe(false)
  })

  it('shows an interrupted feed rather than a current one', () => {
    const live = applyEvent(initialControlState(REALM), controlEvent({ cursor: 3 })).state
    expect(live.contact).toBe('live')
    expect(feedInterrupted(live).contact).toBe('interrupted')
  })

  it('never turns a stale or unreachable run into a finished one', () => {
    // These are statements about evidence. The contract carries the outcome in
    // its own field, and nothing in the projection may synthesize one.
    for (const derived of ['stale', 'diverged', 'runtime_unavailable', 'orphaned', 'lost_contact']) {
      const state = applyRunSnapshot(
        initialControlState(REALM),
        runSnapshot(run({ projection: { ...run().projection, derived, freshness: 'stale' } })),
      )
      const cached = cachedRun(state, RUN)
      expect(cached?.value.projection.derived).toBe(derived)
      expect(cached?.value.projection.outcome).toBeNull()
      expect(cached?.value.closed_at).toBeNull()
    }
  })

  it('remembers the runs the feed reported, newest first, without claiming to be a list', () => {
    let state = initialControlState(REALM)
    state = applyEvent(state, controlEvent({ cursor: 1, agent_run_id: 'run-a' })).state
    state = applyEvent(state, controlEvent({ cursor: 2, agent_run_id: 'run-b' })).state
    state = applyEvent(state, controlEvent({ cursor: 3, agent_run_id: 'run-a' })).state
    expect(state.observed).toEqual(['run-a', 'run-b'])
  })

  it('does not treat a jump in delivered cursors as a gap', () => {
    // `/v1/events` delivers only the kinds a runtime event can express; command
    // intents and census rows consume positions without ever being delivered. A
    // console that inferred a gap from the numbers would resnapshot forever.
    let state = applyEvent(initialControlState(REALM), controlEvent({ cursor: 4 })).state
    const jumped = applyEvent(state, controlEvent({ cursor: 900 }))
    expect(jumped.outcome).toBe('applied')
    state = jumped.state
    expect(state.contact).toBe('live')
    expect(state.cursor).toBe(900)
  })
})
