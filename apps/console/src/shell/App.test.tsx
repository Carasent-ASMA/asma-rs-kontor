/**
 * The console, end to end, against a stubbed `fetch`.
 *
 * No socket and no process: every route is answered from a table. What is being
 * proved is the order the shell does things in — read the realm, attach to it,
 * read health, then subscribe — and that what reaches the screen is only ever
 * what a route answered.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { App } from './App'
import type { CredentialStore } from './credentials'
import { setViewport } from '../test/viewport'
import {
  OTHER_REALM,
  REALM,
  RUN,
  controlEvent,
  run,
  runSnapshot,
  timelineItem,
  timelinePage,
} from '../test/fixtures'

/** A store that keeps nothing, like the browser one. */
const NO_STORE: CredentialStore = {
  durable: false,
  load: async () => null,
  save: async () => {},
  clear: async () => {},
}

/** One JSON answer. */
function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

/** One server-sent stream that ends after the given frames. */
function sse(text: string): Response {
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      controller.close()
    },
  })
  return new Response(stream, {
    status: 200,
    headers: { 'content-type': 'text/event-stream' },
  })
}

/** The realm identity every test attaches to. */
const IDENTITY = {
  realm_id: REALM,
  schema_version: 1,
  created_at: '2026-08-01T00:00:00Z',
  display_label: 'a realm',
}

/** The health answer. */
const HEALTH = {
  realm_id: REALM,
  live: true,
  schema_version: 1,
  reconciliation: 'open',
  scheduling_open: true,
  runtimes: ['k-1'],
}

/** Answer routes from a table, recording the order they were called in. */
function stubFetch(routes: (url: string) => Response | undefined) {
  const calls: string[] = []
  const impl = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)
    calls.push(url)
    const answer = routes(url)
    return answer ?? json({ realm_id: REALM, code: 'not_found', rule: 'no such route' }, 404)
  })
  vi.stubGlobal('fetch', impl)
  return calls
}

/** Point the console at a realm through the connection form. */
async function connect(): Promise<void> {
  render(<App store={NO_STORE} />)
  fireEvent.change(screen.getByLabelText('Realm endpoint'), {
    target: { value: 'http://127.0.0.1:7777' },
  })
  fireEvent.change(screen.getByLabelText('Realm bearer'), {
    target: { value: 'realm-secret' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
  await screen.findByRole('heading', { level: 1, name: 'a realm' })
}

describe('the console', () => {
  beforeEach(() => setViewport('desktop'))
  afterEach(() => vi.unstubAllGlobals())

  it('will not connect without a realm bearer', async () => {
    stubFetch(() => undefined)
    render(<App store={NO_STORE} />)
    fireEvent.change(screen.getByLabelText('Realm endpoint'), {
      target: { value: 'http://127.0.0.1:7777' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(/realm bearer is required/)
  })

  it('warns before connecting to an endpoint that is not loopback', () => {
    stubFetch(() => undefined)
    render(<App store={NO_STORE} />)
    fireEvent.change(screen.getByLabelText('Realm endpoint'), {
      target: { value: 'https://somewhere.example' },
    })
    expect(screen.getByRole('note')).toHaveTextContent(/not a loopback address/)
  })

  it('does not subscribe before it has a snapshot to be strictly after', async () => {
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      return undefined
    })
    await connect()

    // Attaching reads identity and health and stops there. There is no
    // control-plane position yet, so a subscription could only start from one
    // this console never held.
    await waitFor(() => expect(calls.some((url) => url.endsWith('/v1/health'))).toBe(true))
    expect(calls.some((url) => url.includes('/v1/events'))).toBe(false)
    expect(screen.getByText(/nothing snapshotted yet/)).toBeInTheDocument()
  })

  it('snapshots first, then subscribes strictly after the snapshot cursor', async () => {
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 41))
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    await waitFor(() => expect(calls.some((url) => url.includes('/v1/events'))).toBe(true))
    const order = calls.map((url) => new URL(url).pathname)
    expect(order.indexOf('/v1/realm')).toBeLessThan(order.indexOf('/v1/health'))
    // The snapshot is read before the subscription is opened, not alongside it.
    expect(order.indexOf(`/v1/runs/${RUN}`)).toBeLessThan(order.indexOf('/v1/events'))
    // And the subscription resumes strictly after the position that snapshot is
    // consistent with — never from the start of retained history.
    expect(calls.find((url) => url.includes('/v1/events'))).toContain('after=41')
    expect(screen.getByText(/following the realm/)).toBeInTheDocument()
  })

  it('lists the run it snapshotted plus the ones the feed names after it', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 41))
      if (url.includes('/v1/events')) {
        // A run this console never opened, named by an event after the anchor.
        const other = controlEvent({ cursor: 42, agent_run_id: 'run-from-the-feed' })
        return sse(`event: control\ndata: ${JSON.stringify(other)}\n\n`)
      }
      return undefined
    })
    await connect()

    // Before anything is snapshotted the board says why nothing is being
    // followed, rather than showing an empty list as a quiet realm.
    expect(screen.getByText(/nothing is being followed until one is/)).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    expect(await screen.findByRole('button', { name: new RegExp(RUN) })).toBeInTheDocument()
    expect(
      await screen.findByRole('button', { name: /run-from-the-feed/ }),
    ).toBeInTheDocument()
    expect(screen.getByText(/not every run in the realm/)).toBeInTheDocument()
    // The newest position reached the bar.
    expect(screen.getByText('42')).toBeInTheDocument()
  })

  it('shows a run’s orthogonal states without collapsing them into an outcome', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      if (url.includes(`/v1/runs/${RUN}`)) {
        return json(
          runSnapshot(
            run({
              projection: {
                ...run().projection,
                derived: 'lost_contact',
                observed: 'unknown',
                freshness: 'stale',
                outcome: null,
              },
            }),
            9,
          ),
        )
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    // Twice: once in the list, once in the detail.
    await waitFor(() => expect(screen.getAllByText('lost_contact')).toHaveLength(2))
    // Lost contact is a statement about evidence. The outcome field is the only
    // thing that can say a run finished, and it is empty.
    const outcome = screen.getByText('outcome').closest('.fact') as HTMLElement
    expect(within(outcome).getByLabelText('not reported by the realm')).toBeInTheDocument()
    expect(screen.getAllByText('stale').length).toBeGreaterThan(0)
  })

  it('resnapshots when the realm says our position is gone, and not before', async () => {
    let feeds = 0
    let snapshots = 0
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes(`/v1/runs/${RUN}`)) {
        // The second read is the resnapshot, and it lands further on.
        snapshots += 1
        return json(runSnapshot(run(), snapshots === 1 ? 41 : 90))
      }
      if (url.includes('/v1/events')) {
        feeds += 1
        return feeds === 1
          ? json(
              {
                realm_id: REALM,
                code: 'resnapshot_required',
                rule: 'the requested position is outside the retained control-plane history',
                oldest_retained_cursor: 80,
                newest_cursor: 90,
              },
              410,
            )
          : sse('')
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    // The 410 is answered by reading the snapshot again and resubscribing after
    // the *new* position, rather than by retrying the position the realm just
    // said it does not have.
    await waitFor(() => expect(feeds).toBeGreaterThan(1))
    const subscriptions = calls.filter((url) => url.includes('/v1/events'))
    expect(subscriptions[0]).toContain('after=41')
    expect(subscriptions[1]).toContain('after=90')
    expect(snapshots).toBeGreaterThan(1)
  })

  it('resnapshots when a frame arrives from another realm', async () => {
    let feeds = 0
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 41))
      if (url.includes('/v1/events')) {
        feeds += 1
        if (feeds === 1) {
          const foreign = controlEvent({ cursor: 42, realm_id: OTHER_REALM })
          return sse(`event: control\ndata: ${JSON.stringify(foreign)}\n\n`)
        }
        return sse('')
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    // Nothing from the other realm reached the projection, and the console read
    // everything again rather than carrying on as though it had.
    await waitFor(() => expect(feeds).toBeGreaterThan(1))
    expect(calls.filter((url) => url.includes(`/v1/runs/${RUN}`)).length).toBeGreaterThan(1)
    expect(screen.queryByText(String(42))).toBeNull()
  })

  it('does not resnapshot on a jump in delivered cursors', async () => {
    let feeds = 0
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 41))
      if (url.includes('/v1/events')) {
        feeds += 1
        if (feeds === 1) {
          // 42 then 900: command intents and census rows consume positions that
          // are never delivered, so holes here are normal and permanent.
          const first = JSON.stringify(controlEvent({ cursor: 42 }))
          const jumped = JSON.stringify(controlEvent({ cursor: 900 }))
          return sse(
            `event: control\ndata: ${first}\n\nevent: control\ndata: ${jumped}\n\n`,
          )
        }
        return sse('')
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    // Both events were reduced and the newest position is the jumped-to one.
    await waitFor(() => expect(screen.getByText('900')).toBeInTheDocument())
    // Exactly one snapshot read: the jump demanded nothing.
    expect(calls.filter((url) => url.includes(`/v1/runs/${RUN}`))).toHaveLength(1)
    expect(screen.queryByText(/no longer retains the position/)).toBeNull()
  })

  it('reads a session’s history and then follows strictly after its anchor', async () => {
    const calls = stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 9))
      if (url.includes('/timeline')) {
        return json(
          timelinePage([timelineItem(1), timelineItem(2)], { next: null, anchor: 'anchor-1-2' }),
        )
      }
      if (url.includes('/stream')) {
        const frame = { realm_id: REALM, agent_run_id: RUN, item: timelineItem(3) }
        return sse(`event: content\ndata: ${JSON.stringify(frame)}\n\n`)
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))
    fireEvent.click(screen.getByRole('button', { name: 'Session' }))

    await waitFor(() => expect(calls.some((url) => url.includes('/stream'))).toBe(true))
    // The subscription starts strictly after the anchor the history read
    // returned, and never from a position of its own invention.
    expect(calls.find((url) => url.includes('/stream'))).toContain('after=anchor-1-2')
  })

  it('says the transcript must be reread, and nothing about the run', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      if (url.includes('/timeline')) {
        return json(timelinePage([timelineItem(1)], { next: null, anchor: 'anchor-1-1' }))
      }
      if (url.includes('/stream')) {
        const refusal = {
          realm_id: REALM,
          code: 'timeline_refetch_required',
          rule: 'the runtime renumbered or skipped this session content',
        }
        return sse(`event: error\ndata: ${JSON.stringify(refusal)}\n\n`)
      }
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))
    fireEvent.click(screen.getByRole('button', { name: 'Session' }))

    // The realm's own words, and a way to read it again.
    expect(
      await screen.findByText(/renumbered or skipped this session content/),
    ).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Read it again' })).toBeInTheDocument()
  })

  it('says what is missing on every view the contract does not serve yet', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      return undefined
    })
    await connect()

    for (const [view, subject] of [
      ['Intake', /intake inbox/i],
      ['Workflow', /external-workflow inspector/i],
      ['Schedule', /When work may be dispatched/i],
    ] as const) {
      fireEvent.click(screen.getByRole('button', { name: view }))
      expect(await screen.findByText(subject)).toBeInTheDocument()
      // No data at all rather than data from somewhere the contract does not have.
      expect(screen.getByText(/serves no projection for this yet/)).toBeInTheDocument()
    }
  })

  it('is reachable and dismissible from the keyboard on a phone', async () => {
    setViewport('phone')
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      if (url.includes(`/v1/runs/${RUN}`)) return json(runSnapshot(run(), 9))
      return undefined
    })
    await connect()

    fireEvent.change(screen.getByLabelText('Open a run by id'), { target: { value: RUN } })
    fireEvent.click(screen.getByRole('button', { name: 'Read' }))

    const drawer = await screen.findByRole('dialog', { name: 'run detail' })
    expect(document.activeElement).toBe(drawer)
    fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('offers a way past the bar and the rail to the view', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) return json(IDENTITY)
      if (url.endsWith('/v1/health')) return json(HEALTH)
      if (url.includes('/v1/events')) return sse('')
      return undefined
    })
    await connect()
    expect(screen.getByRole('link', { name: /Skip to the view/ })).toHaveAttribute('href', '#view')
  })

  it('reports a refusal in the realm’s own words', async () => {
    stubFetch((url) => {
      if (url.endsWith('/v1/realm')) {
        return json(
          {
            realm_id: REALM,
            code: 'unauthenticated',
            rule: 'the presented credential is not one of this realm’s',
          },
          401,
        )
      }
      return undefined
    })
    render(<App store={NO_STORE} />)
    fireEvent.change(screen.getByLabelText('Realm endpoint'), {
      target: { value: 'http://127.0.0.1:7777' },
    })
    fireEvent.change(screen.getByLabelText('Realm bearer'), { target: { value: 'wrong' } })
    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(/unauthenticated/)
  })
})
