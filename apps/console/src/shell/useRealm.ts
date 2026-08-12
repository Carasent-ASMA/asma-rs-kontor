/**
 * Attaching to one realm, snapshotting it, and following its durable feed.
 *
 * # The order, and why it is the whole point
 *
 * 1. read the realm's identity;
 * 2. attach every later read to it;
 * 3. read health;
 * 4. take a **control-plane snapshot** and note the position it is consistent
 *    with;
 * 5. subscribe to `/v1/events` **strictly after** that position.
 *
 * Step 4 is not optional and step 5 cannot precede it. Subscribing first leaves
 * a window whose events are attributed to a state that already contained them;
 * subscribing from the start of retained history instead makes the console's
 * picture silently depend on how much history the realm happens to still hold.
 *
 * # What the contract can and cannot anchor on
 *
 * There is no realm-wide control-plane projection in the merged contract: the
 * only routes carrying a `snapshot_cursor` address one aggregate by id. So the
 * snapshot is of the aggregates this console was asked to observe, and until it
 * has been asked to observe one there is **no subscription at all** — which is
 * the honest state, because there is no position for delivery to be after. The
 * top bar says so rather than showing an idle feed as a healthy one.
 *
 * # What resnapshots, and what does not
 *
 * Only the realm's own `resnapshot_required` (HTTP 410) and a realm mismatch.
 * A jump in delivered cursors is *never* treated as a gap: `/v1/events` carries
 * only the kinds a runtime event can express, so command intents and census rows
 * consume positions that are never delivered. Holes in the delivered sequence are
 * normal and permanent, and a console that inferred a gap from them would
 * resnapshot forever against a healthy realm.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ForeignRealm, KontorClient, Refused } from '../api/client'
import type { Endpoint } from '../api/endpoint'
import type { ControlEvent, Health, Realm } from '../api/types'
import {
  applyControlSnapshot,
  applyEvent,
  applyRunSnapshot,
  applyTaskSnapshot,
  feedInterrupted,
  feedLive,
  initialControlState,
  resnapshotRequired,
  type ControlState,
} from '../state/control'

/** How long to wait before reopening a dropped feed. */
const RECONNECT_DELAY_MS = 2000

/** What the shell knows about the realm it is attached to. */
export interface RealmConnection {
  /** The client, once the realm's identity has been read. */
  readonly client: KontorClient | null
  /** The realm's identity. */
  readonly realm: Realm | null
  /** The realm's liveness and startup state. */
  readonly health: Health | null
  /** The control projection. */
  readonly control: ControlState | null
  /** What went wrong, in the realm's own words where it said any. */
  readonly error: string | null
  /** Observe one run: snapshot it, and anchor the feed if nothing has yet. */
  openRun: (agentRunId: string) => Promise<void>
  /** Read one task and adopt its snapshot. */
  openTask: (projectId: string, taskId: string) => Promise<void>
  /** Read health again. */
  refreshHealth: () => Promise<void>
}

/** Attach to one realm and keep the control projection current. */
export function useRealm(endpoint: Endpoint | null): RealmConnection {
  const [client, setClient] = useState<KontorClient | null>(null)
  const [realm, setRealm] = useState<Realm | null>(null)
  const [health, setHealth] = useState<Health | null>(null)
  const [control, setControl] = useState<ControlState | null>(null)
  const [error, setError] = useState<string | null>(null)
  /**
   * The snapshot position the subscription is anchored at.
   *
   * `null` until something has been snapshotted, and the effect that opens the
   * feed does nothing while it is — which is what makes "snapshot before
   * subscribe" structural rather than a matter of call order.
   */
  const [anchor, setAnchor] = useState<number | null>(null)
  /** Bumped to reopen the feed without moving the anchor. */
  const [attempt, setAttempt] = useState(0)
  // The feed resumes from the newest applied position when it reopens, and a
  // state value captured in the effect's closure would be the one from when it
  // was opened.
  const cursor = useRef<number | null>(null)
  /** The aggregates this console was asked to observe. */
  const observed = useRef(new Set<string>())

  const connection = useMemo(
    () => (endpoint ? new KontorClient(endpoint) : null),
    [endpoint],
  )

  // Attach: identity, then health. Deliberately no subscription — there is
  // nothing to be strictly after yet.
  useEffect(() => {
    if (!connection) {
      setClient(null)
      setRealm(null)
      setHealth(null)
      setControl(null)
      return undefined
    }
    let live = true
    cursor.current = null
    observed.current = new Set()
    setAnchor(null)

    void (async () => {
      try {
        const identity = await connection.realm()
        if (!live) {
          return
        }
        // Everything read after this point is checked against it.
        connection.attach(identity.realm_id)
        const liveness = await connection.health()
        if (!live) {
          return
        }
        setRealm(identity)
        setHealth(liveness)
        setControl(initialControlState(identity.realm_id))
        setClient(connection)
        setError(null)
      } catch (cause) {
        if (live) {
          setError(describe(cause))
        }
      }
    })()

    return () => {
      live = false
    }
  }, [connection])

  /** Take a fresh control-plane snapshot and re-anchor the feed to it. */
  const resnapshot = useCallback(async (): Promise<void> => {
    if (!connection || observed.current.size === 0) {
      return
    }
    try {
      const snapshot = await connection.controlSnapshot([...observed.current])
      cursor.current = snapshot.snapshotCursor
      setControl((previous) => (previous ? applyControlSnapshot(previous, snapshot) : previous))
      setAnchor(snapshot.snapshotCursor)
      // The anchor may be unchanged — a realm that discarded our position can
      // still snapshot at the same number — so the reopen is asked for
      // explicitly rather than left to the anchor having moved.
      setAttempt((previous) => previous + 1)
      setError(null)
    } catch (cause) {
      setError(describe(cause))
    }
  }, [connection])

  // Follow the feed, strictly after the snapshot the console is anchored at.
  useEffect(() => {
    if (!connection || anchor === null) {
      return undefined
    }
    let live = true
    let retry: ReturnType<typeof setTimeout> | null = null
    // Resume from the newest applied position, which is the anchor until the
    // first event lands.
    const resume = cursor.current ?? anchor

    const stream = connection.controlFeed(resume, {
      onEvent: (event) => {
        if (!live) {
          return
        }
        setControl((previous) => {
          if (!previous) {
            return previous
          }
          const applied = applyEvent(previous, event as ControlEvent)
          if (applied.outcome === 'applied') {
            cursor.current = (event as ControlEvent).cursor
          }
          return applied.state
        })
      },
      onClosed: (reason) => {
        if (!live) {
          return
        }
        // Exactly two things mean "read everything again": the realm saying our
        // position is outside what it retains, and a frame arriving from a realm
        // this console is not attached to. Both mean what is held can no longer
        // be trusted to describe this realm, and both mean it now rather than in
        // two seconds — there is nothing to wait for.
        //
        // Nothing else qualifies. In particular a jump in delivered cursors is
        // not a gap: `/v1/events` carries only the kinds a runtime event can
        // express, so other rows consume positions that are never delivered.
        const discarded =
          (reason instanceof Refused && reason.body.code === 'resnapshot_required') ||
          reason instanceof ForeignRealm
        if (discarded) {
          setControl((previous) => (previous ? resnapshotRequired(previous) : previous))
          void resnapshot()
          return
        }
        if (reason) {
          setControl((previous) => (previous ? feedInterrupted(previous) : previous))
        }
        retry = setTimeout(() => setAttempt((previous) => previous + 1), RECONNECT_DELAY_MS)
      },
    })
    setControl((previous) => (previous ? feedLive(previous) : previous))

    return () => {
      live = false
      stream.close()
      if (retry) {
        clearTimeout(retry)
      }
    }
  }, [connection, anchor, attempt, resnapshot])

  const openRun = useCallback(
    async (agentRunId: string) => {
      if (!client) {
        return
      }
      const first = observed.current.size === 0
      observed.current.add(agentRunId)
      try {
        if (first) {
          // The console's control-plane snapshot. The feed is opened by the
          // effect above once this sets the anchor, and never before.
          const snapshot = await client.controlSnapshot([...observed.current])
          cursor.current = snapshot.snapshotCursor
          setControl((previous) =>
            previous ? applyControlSnapshot(previous, snapshot) : previous,
          )
          setAnchor(snapshot.snapshotCursor)
        } else {
          const snapshot = await client.run(agentRunId)
          setControl((previous) => (previous ? applyRunSnapshot(previous, snapshot) : previous))
        }
        setError(null)
      } catch (cause) {
        observed.current.delete(agentRunId)
        setError(describe(cause))
      }
    },
    [client],
  )

  const openTask = useCallback(
    async (projectId: string, taskId: string) => {
      if (!client) {
        return
      }
      try {
        const snapshot = await client.task(projectId, taskId)
        setControl((previous) => (previous ? applyTaskSnapshot(previous, snapshot) : previous))
        setError(null)
      } catch (cause) {
        setError(describe(cause))
      }
    },
    [client],
  )

  const refreshHealth = useCallback(async () => {
    if (!client) {
      return
    }
    try {
      setHealth(await client.health())
    } catch (cause) {
      setError(describe(cause))
    }
  }, [client])

  return { client, realm, health, control, error, openRun, openTask, refreshHealth }
}

/**
 * Say what went wrong, preferring the realm's own words.
 *
 * A refusal carries a stable code and a static rule; both are shown, because
 * "forbidden" alone tells an operator nothing about which authority was short.
 */
export function describe(cause: unknown): string {
  if (cause instanceof Refused) {
    return `${cause.body.code}: ${cause.body.rule}`
  }
  if (cause instanceof Error) {
    return cause.message
  }
  return 'the request failed'
}
