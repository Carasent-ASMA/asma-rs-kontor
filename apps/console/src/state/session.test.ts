/**
 * A session room reads history, then follows strictly after it, and doubts
 * loudly.
 *
 * The mutants this file exists to kill:
 *
 * * subscribing before the history is exhausted, or from anything but the anchor
 *   the last page returned;
 * * appending a frame that is not strictly after what is held;
 * * reducing a redelivered position twice;
 * * continuing across an epoch change or a skipped sequence;
 * * resuming a dropped subscription as though nothing could have been missed;
 * * keeping content after the realm said the timeline must be reread;
 * * concluding anything about the *run* from content that is missing;
 * * offering a permission that is already resolved.
 */
import { describe, expect, it } from 'vitest'
import {
  applyLiveFrame,
  applyStreamRefusal,
  applyTimelinePage,
  beginRefetch,
  initialSessionState,
  isAnswerable,
  permissionLedger,
  responseSent,
  responseSettled,
  streamInterrupted,
  streamOpened,
} from './session'
import {
  OTHER_REALM,
  REALM,
  RUN,
  streamFrame,
  timelineItem,
  timelinePage,
} from '../test/fixtures'

/** A room that has read one complete page of history. */
function loaded() {
  const state = initialSessionState(REALM, RUN)
  return applyTimelinePage(
    state,
    timelinePage([timelineItem(1), timelineItem(2), timelineItem(3)]),
  ).state
}

describe('session room', () => {
  it('pages history until the continuation is absent', () => {
    let state = initialSessionState(REALM, RUN)
    state = applyTimelinePage(
      state,
      timelinePage([timelineItem(1), timelineItem(2)], { next: 'more', anchor: 'a-1-2' }),
    ).state
    // Still loading: subscribing here would anchor to the middle of the history.
    expect(state.phase).toBe('loading')
    expect(state.next).toBe('more')

    state = applyTimelinePage(
      state,
      timelinePage([timelineItem(3)], { next: null, anchor: 'a-1-3' }),
    ).state
    expect(state.phase).toBe('idle')
    expect(state.next).toBeNull()
    expect(state.anchor).toBe('a-1-3')
    expect(state.items.map((item) => item.sequence)).toEqual([1, 2, 3])
  })

  it('opens the live subscription only once history is exhausted', () => {
    const partial = applyTimelinePage(
      initialSessionState(REALM, RUN),
      timelinePage([timelineItem(1)], { next: 'more' }),
    ).state
    expect(streamOpened(partial).phase).toBe('loading')
    expect(streamOpened(loaded()).phase).toBe('live')
  })

  it('appends a frame that is strictly after what is held', () => {
    const applied = applyLiveFrame(loaded(), streamFrame(timelineItem(4)))
    expect(applied.outcome).toBe('applied')
    expect(applied.state.items.map((item) => item.sequence)).toEqual([1, 2, 3, 4])
    expect(applied.state.phase).toBe('live')
  })

  it('drops a redelivered position instead of doubling it', () => {
    const state = loaded()
    for (const sequence of [3, 2, 1]) {
      const replayed = applyLiveFrame(state, streamFrame(timelineItem(sequence)))
      expect(replayed.outcome).toBe('duplicate')
      expect(replayed.state.items).toHaveLength(3)
    }
  })

  it('drops a page that redelivers positions already reduced', () => {
    const state = loaded()
    const replayed = applyTimelinePage(
      state,
      timelinePage([timelineItem(2), timelineItem(3)]),
    )
    expect(replayed.outcome).toBe('duplicate')
    expect(replayed.state.items).toHaveLength(3)
  })

  it('discards its content when the live stream skips a sequence', () => {
    const skipped = applyLiveFrame(loaded(), streamFrame(timelineItem(9)))
    expect(skipped.outcome).toBe('refetch_required')
    expect(skipped.state.phase).toBe('refetch_required')
    expect(skipped.state.items).toEqual([])
    expect(skipped.state.refetchReason).toMatch(/skipped/)
  })

  it('discards its content when the runtime renumbers the session', () => {
    const renumbered = applyLiveFrame(
      loaded(),
      streamFrame(timelineItem(1, { epoch: 9 })),
    )
    expect(renumbered.outcome).toBe('refetch_required')
    expect(renumbered.state.items).toEqual([])
    expect(renumbered.state.epoch).toBeNull()

    // The same rule holds for a history page that changes epoch mid-read.
    const paged = applyTimelinePage(
      loaded(),
      timelinePage([timelineItem(1, { epoch: 9 })], { epoch: 9 }),
    )
    expect(paged.outcome).toBe('refetch_required')
    expect(paged.state.items).toEqual([])
  })

  it('discards its content when the realm reports the timeline cannot be followed', () => {
    const refused = applyStreamRefusal(
      loaded(),
      'the runtime renumbered or skipped this session content',
    )
    expect(refused.phase).toBe('refetch_required')
    expect(refused.items).toEqual([])
    // The realm's own words, not this console's paraphrase.
    expect(refused.refetchReason).toBe(
      'the runtime renumbered or skipped this session content',
    )
  })

  it('treats a dropped subscription as content it cannot account for', () => {
    const dropped = streamInterrupted(loaded())
    expect(dropped.phase).toBe('refetch_required')
    expect(dropped.items).toEqual([])
    expect(dropped.anchor).toBeNull()
    expect(dropped.refetchReason).toMatch(/unknown/)
  })

  it('concludes nothing about the run from content it is missing', () => {
    const broken = streamInterrupted(loaded())
    // There is no field here that could say the run ended, and the reason names
    // the transcript rather than the run.
    expect(Object.keys(broken)).not.toContain('lifecycle')
    expect(broken.refetchReason).not.toMatch(/failed|终|terminated|cancelled/i)
  })

  it('rereads the canonical timeline from the start after a refetch', () => {
    const restarted = beginRefetch(streamInterrupted(loaded()))
    expect(restarted.phase).toBe('loading')
    expect(restarted.items).toEqual([])
    expect(restarted.seen.size).toBe(0)
    // The reason survives so the room can still say why it started over.
    expect(restarted.refetchReason).toMatch(/unknown/)
  })

  it('refuses a page or a frame belonging to another realm or another run', () => {
    const state = loaded()
    expect(applyTimelinePage(state, timelinePage([], { realm_id: OTHER_REALM })).outcome).toBe(
      'foreign',
    )
    expect(
      applyLiveFrame(state, streamFrame(timelineItem(4), { realm_id: OTHER_REALM })).outcome,
    ).toBe('foreign')
    expect(
      applyLiveFrame(state, streamFrame(timelineItem(4), { agent_run_id: 'someone-else' }))
        .outcome,
    ).toBe('foreign')
  })

  it('keeps a permission answerable until the transcript resolves it', () => {
    let state = applyTimelinePage(
      initialSessionState(REALM, RUN),
      timelinePage([
        timelineItem(1),
        timelineItem(2, { kind: 'permission_request', permission_id: 'perm-1' }),
      ]),
    ).state

    let ledger = permissionLedger(state)
    expect(ledger).toHaveLength(1)
    expect(isAnswerable(ledger[0]!)).toBe(true)

    // While an answer is in flight it may not be sent again.
    state = responseSent(state, 'perm-1', 'response-key', 'allow')
    expect(isAnswerable(permissionLedger(state)[0]!)).toBe(false)

    state = responseSettled(state, 'perm-1', { state: 'applied', code: null, rule: null })
    state = applyLiveFrame(
      state,
      streamFrame(timelineItem(3, { kind: 'permission_resolved', permission_id: 'perm-1' })),
    ).state

    ledger = permissionLedger(state)
    expect(ledger[0]?.resolution).not.toBeNull()
    expect(isAnswerable(ledger[0]!)).toBe(false)
    expect(ledger[0]?.receipt?.state).toBe('applied')
  })

  it('resolves a permission answered by someone else', () => {
    // Whoever answered it, the transcript is the record.
    const state = applyTimelinePage(
      initialSessionState(REALM, RUN),
      timelinePage([
        timelineItem(1, { kind: 'permission_request', permission_id: 'perm-2' }),
        timelineItem(2, { kind: 'permission_resolved', permission_id: 'perm-2' }),
      ]),
    ).state
    const entry = permissionLedger(state)[0]!
    expect(entry.receipt).toBeNull()
    expect(isAnswerable(entry)).toBe(false)
  })

  it('carries a refused answer as the realm stated it', () => {
    let state = responseSent(loaded(), 'perm-3', 'response-key', 'deny')
    state = responseSettled(state, 'perm-3', {
      state: 'conflict',
      code: 'idempotency_conflict',
      rule: 'the identifier was already used to commit a different effect',
    })
    expect(state.responses.get('perm-3')).toMatchObject({
      state: 'conflict',
      code: 'idempotency_conflict',
      responseId: 'response-key',
    })
  })

  it('keeps answers already sent when the content is discarded', () => {
    const state = streamInterrupted(responseSent(loaded(), 'perm-4', 'response-key', 'allow'))
    // The runtime's ledger did not forget the answer just because this room did.
    expect(state.responses.get('perm-4')?.responseId).toBe('response-key')
  })
})
