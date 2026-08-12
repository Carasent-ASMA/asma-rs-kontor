/**
 * One session room: read the canonical timeline, then follow it strictly after.
 *
 * The doubt rule is implemented here rather than left to the caller, because
 * every path into it ends the same way: close the stream, discard the content,
 * read the timeline again. A room that handled three of the four cases would be
 * a room that shows a transcript with an invisible hole in it a quarter of the
 * time.
 */
import { useCallback, useEffect, useRef, useState } from 'react'
import type { KontorClient } from '../api/client'
import { Refused } from '../api/client'
import type { StreamFrame } from '../api/types'
import { KeyLedger } from '../state/ids'
import {
  applyLiveFrame,
  applyStreamRefusal,
  applyTimelinePage,
  beginRefetch,
  initialSessionState,
  permissionLedger,
  responseSent,
  responseSettled,
  streamOpened,
  streamInterrupted,
  type PermissionEntry,
  type ResponseState,
  type SessionState,
} from '../state/session'

/** How many pages one history read will take before giving up. */
const MAX_PAGES = 200

/**
 * How many times in a row the room will reread the timeline on its own.
 *
 * The doubt rule says to discard the content and read the canonical timeline
 * again — but a runtime that refuses the subscription *every* time would turn
 * that into an unbounded loop of full history reads against the realm. After
 * this many consecutive refetches the room stops and says so, and an operator
 * decides whether to try again.
 */
const MAX_AUTO_REFETCH = 2

/** What a session room offers its view. */
export interface SessionRoom {
  /** The reduced content. */
  readonly state: SessionState
  /** The permission requests the transcript raises. */
  readonly permissions: readonly PermissionEntry[]
  /** What went wrong, in the realm's own words. */
  readonly error: string | null
  /** Whether a message or a decision is in flight. */
  readonly busy: boolean
  /** Send one message under a key held across retries. */
  send: (body: string) => Promise<void>
  /** Answer one permission request under a key held across retries. */
  decide: (requestId: string, decision: string) => Promise<void>
  /** Read the canonical timeline again from the start. */
  reload: () => void
}

/** Open one session room. */
export function useSessionRoom(
  client: KontorClient | null,
  realmId: string | null,
  agentRunId: string | null,
): SessionRoom {
  const [state, setState] = useState<SessionState>(() =>
    initialSessionState(realmId ?? '', agentRunId ?? ''),
  )
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [generation, setGeneration] = useState(0)
  // One ledger per room: a key belongs to an intent, and an intent survives every
  // attempt made to carry it out.
  const keys = useRef(new KeyLedger())
  // The number of attempts each intent has taken, so a 200 on attempt two is
  // reported as the replay it is rather than as a fresh effect.
  const attempts = useRef(new Map<string, number>())
  // Consecutive automatic refetches, so a session that cannot be followed at all
  // is reported once instead of read forever.
  const refetches = useRef(0)

  const reload = useCallback(() => {
    refetches.current = 0
    setGeneration((previous) => previous + 1)
  }, [])

  useEffect(() => {
    if (!client || !realmId || !agentRunId) {
      return undefined
    }
    let live = true
    let stream: { close(): void } | null = null
    // The reducer's own state is the authority while a page loop is running;
    // React's is a snapshot from before the loop started.
    let current = initialSessionState(realmId, agentRunId)
    setState(current)
    setError(null)

    const publish = (next: SessionState): void => {
      current = next
      if (live) {
        setState(next)
      }
    }

    const restart = (): void => {
      stream?.close()
      stream = null
      refetches.current += 1
      if (refetches.current > MAX_AUTO_REFETCH) {
        // The content stays discarded and the reason stays on screen. Reading it
        // again is now an operator's decision, not a loop.
        return
      }
      publish(beginRefetch(current))
      if (live) {
        // A fresh generation runs this effect again from the top.
        setGeneration((previous) => previous + 1)
      }
    }

    void (async () => {
      try {
        // 1. Page the canonical timeline until the continuation is absent.
        let after: string | null = null
        for (let page = 0; page < MAX_PAGES; page += 1) {
          const read = await client.timeline(agentRunId, after)
          if (!live) {
            return
          }
          const applied = applyTimelinePage(current, read)
          publish(applied.state)
          if (applied.outcome === 'refetch_required') {
            restart()
            return
          }
          after = applied.state.next
          if (after === null) {
            break
          }
        }
        if (!live || current.next !== null) {
          return
        }
        const anchor = current.anchor
        if (anchor === null) {
          return
        }

        // 2. Follow strictly after the anchor that read returned.
        publish(streamOpened(current))
        stream = client.sessionStream(agentRunId, anchor, {
          onFrame: (frame: StreamFrame) => {
            if (!live) {
              return
            }
            const applied = applyLiveFrame(current, frame)
            publish(applied.state)
            if (applied.outcome === 'refetch_required') {
              restart()
            } else if (applied.outcome === 'applied') {
              // The session is being followed again, so the next interruption
              // gets its own budget of automatic rereads.
              refetches.current = 0
            }
          },
          onRefetchRequired: (refusal) => {
            if (!live) {
              return
            }
            publish(applyStreamRefusal(current, refusal.rule))
            restart()
          },
          onClosed: (reason) => {
            if (!live || reason === null) {
              return
            }
            // Whatever the session emitted while the subscription was shut is
            // unknowable from here, so this is the doubt rule too.
            publish(streamInterrupted(current))
            restart()
          },
        })
      } catch (cause) {
        if (live) {
          setError(describeRefusal(cause))
        }
      }
    })()

    return () => {
      live = false
      stream?.close()
    }
  }, [client, realmId, agentRunId, generation])

  const send = useCallback(
    async (body: string) => {
      if (!client || !agentRunId || body.trim() === '') {
        return
      }
      // One key for this draft, minted once and presented on every attempt: the
      // realm keys the message's effect on it, so a fresh key per attempt would
      // turn a retry into a second message.
      const subject = `message:${agentRunId}:${body}`
      const key = keys.current.key(subject)
      const attempt = (attempts.current.get(subject) ?? 0) + 1
      attempts.current.set(subject, attempt)
      setBusy(true)
      try {
        await client.sendMessage(agentRunId, body, key)
        keys.current.release(subject)
        attempts.current.delete(subject)
        setError(null)
      } catch (cause) {
        // The key is deliberately *not* released: the next attempt is a retry of
        // this message and must present the same one.
        setError(describeRefusal(cause))
      } finally {
        setBusy(false)
      }
    },
    [client, agentRunId],
  )

  const decide = useCallback(
    async (requestId: string, decision: string) => {
      if (!client || !agentRunId) {
        return
      }
      const subject = `permission:${agentRunId}:${requestId}`
      const key = keys.current.key(subject)
      const attempt = (attempts.current.get(subject) ?? 0) + 1
      attempts.current.set(subject, attempt)
      setBusy(true)
      setState((previous) => responseSent(previous, requestId, key, decision))
      try {
        await client.respondPermission(agentRunId, requestId, decision, key)
        setState((previous) =>
          responseSettled(previous, requestId, {
            // A second attempt answered 200 is the original acknowledgement being
            // replayed, which is the evidence the held key is doing its job.
            state: attempt > 1 ? 'replayed' : 'applied',
            code: null,
            rule: null,
          }),
        )
        setError(null)
      } catch (cause) {
        const settled = settlementOf(cause)
        setState((previous) => responseSettled(previous, requestId, settled))
        setError(describeRefusal(cause))
      } finally {
        setBusy(false)
      }
    },
    [client, agentRunId],
  )

  return { state, permissions: permissionLedger(state), error, busy, send, decide, reload }
}

/** How a refusal settles a permission answer. */
function settlementOf(cause: unknown): {
  state: ResponseState
  code: string | null
  rule: string | null
} {
  if (!(cause instanceof Refused)) {
    return { state: 'refused', code: null, rule: describeRefusal(cause) }
  }
  const code = cause.body.code
  const state: ResponseState =
    code === 'idempotency_conflict'
      ? 'conflict'
      : code === 'unsupported_capability'
        ? 'unsupported'
        : 'refused'
  return { state, code, rule: cause.body.rule }
}

/** Say what the realm refused, in its own words. */
function describeRefusal(cause: unknown): string {
  if (cause instanceof Refused) {
    return `${cause.body.code}: ${cause.body.rule}`
  }
  return cause instanceof Error ? cause.message : 'the session request failed'
}
