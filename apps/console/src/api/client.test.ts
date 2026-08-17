/**
 * The client is the whole boundary, so the boundary rules are tested here.
 *
 * No socket and no process: `fetch` is injected. The mutants this file exists to
 * kill: a request without the realm credential, a per-attempt idempotency key, a
 * refusal rendered as a channel failure, and a value from another realm reaching
 * a reducer.
 */
import { describe, expect, it, vi } from 'vitest'
import { ForeignRealm, KontorClient, Refused, Unreachable } from './client'
import { OTHER_REALM, REALM, RUN, controlEvent, runSnapshot } from '../test/fixtures'

/** The endpoint every test in this file is pointed at. */
const ENDPOINT = { baseUrl: 'http://127.0.0.1:7777', token: 'realm-secret' }

/** One JSON answer. */
function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

/** One server-sent stream that ends after the given text. */
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

/** A client whose every call is answered by `respond`. */
function clientWith(respond: (url: string, init?: RequestInit) => Response) {
  const calls: { url: string; init: RequestInit | undefined }[] = []
  const fetchImpl = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    calls.push({ url, init })
    return respond(url, init)
  }) as unknown as typeof fetch
  return { client: new KontorClient(ENDPOINT, { fetchImpl }), calls }
}

describe('client requests', () => {
  it('presents the realm credential on every route', async () => {
    const { client, calls } = clientWith(() => json({ realm_id: REALM, live: true }))
    await client.health()
    const headers = new Headers(calls[0]?.init?.headers)
    expect(headers.get('Authorization')).toBe('Bearer realm-secret')
  })

  it('sends one message under the key it was given, unchanged across retries', async () => {
    let attempt = 0
    const { client, calls } = clientWith(() => {
      attempt += 1
      return attempt === 1
        ? json({ realm_id: REALM, code: 'unavailable', rule: 'the runtime could not be reached' }, 503)
        : json({ realm_id: REALM, message_id: 'key-7', epoch: 1, sequence: 5 })
    })
    client.attach(REALM)

    await expect(client.sendMessage(RUN, 'hello', 'key-7')).rejects.toBeInstanceOf(Refused)
    const ack = await client.sendMessage(RUN, 'hello', 'key-7')
    expect(ack.message_id).toBe('key-7')

    const keys = calls.map((call) => new Headers(call.init?.headers).get('Idempotency-Key'))
    expect(keys).toEqual(['key-7', 'key-7'])
  })

  it('answers a permission under a stable response key and the runtime request id', async () => {
    const { client, calls } = clientWith(() => json({ realm_id: REALM, permission_id: 'perm-1' }))
    await client.respondPermission(RUN, 'perm-1', 'allow', 'response-key')
    expect(calls[0]?.url).toContain('/permissions/perm-1')
    expect(new Headers(calls[0]?.init?.headers).get('Idempotency-Key')).toBe('response-key')
    expect(calls[0]?.init?.body).toBe('{"decision":"allow"}')
  })

  it('uses the catalog and Teams API routes with caller-owned command keys', async () => {
    const { client, calls } = clientWith((url) => json(
      url.endsWith('/v1/catalog')
        ? { realm_id: REALM, snapshot_cursor: 3, providers: [], models: [] }
        : { realm_id: REALM, snapshot_cursor: 4, drafts: [], revisions: [] },
    ))
    await client.modelCatalog()
    await client.teams()
    await client.saveTeamDraft({ id: 'team-1', name: 'Team', slots: [] }, 'save-1')
    await client.publishTeam('team-1', 'publish-1')
    expect(calls.map((call) => new URL(call.url).pathname)).toEqual([
      '/v1/catalog', '/v1/teams', '/v1/teams/drafts:save', '/v1/teams/team-1/publish',
    ])
    expect(new Headers(calls[2]?.init?.headers).get('Idempotency-Key')).toBe('save-1')
    expect(new Headers(calls[3]?.init?.headers).get('Idempotency-Key')).toBe('publish-1')
  })

  it('keeps Operational reads, previews and receipt-backed applies on /v1', async () => {
    const { client, calls } = clientWith(() => json({ realm_id: REALM }))
    await client.topology('project 1', 'epic 1')
    await client.codeHelp('project 1', 'epic 1')
    await client.previewCoreTeam('project 1', { seats: [] })
    await client.applyCoreTeam(
      'project 1',
      { expected_revision: 4, preview_hash: 'hash-1', seats: [] },
      'apply-1',
    )

    expect(calls.map((call) => new URL(call.url).pathname)).toEqual([
      '/v1/projects/project%201/topology:inspect',
      '/v1/projects/project%201/epics/epic%201/code-help',
      '/v1/projects/project%201/core-team:preview',
      '/v1/projects/project%201/core-team:apply',
    ])
    expect(new URL(calls[0]?.url ?? '').searchParams.get('epic_id')).toBe('epic 1')
    expect(calls[2]?.init?.body).toBe('{"seats":[]}')
    expect(new Headers(calls[3]?.init?.headers).get('Idempotency-Key')).toBe('apply-1')
    expect(calls[3]?.init?.body).toBe(
      '{"expected_revision":4,"preview_hash":"hash-1","seats":[]}',
    )
  })

  it('reports a refusal with the contract code rather than as a channel failure', async () => {
    const { client } = clientWith(() =>
      json(
        {
          realm_id: REALM,
          code: 'resnapshot_required',
          rule: 'the requested position is outside the retained control-plane history',
          oldest_retained_cursor: 40,
          newest_cursor: 90,
        },
        410,
      ),
    )
    await expect(client.run(RUN)).rejects.toMatchObject({
      name: 'Refused',
      status: 410,
      body: { code: 'resnapshot_required', oldest_retained_cursor: 40 },
    })
  })

  it('reports an answer that is not a contract envelope as unreachable', async () => {
    const { client } = clientWith(() => new Response('<html>gateway</html>', { status: 502 }))
    await expect(client.run(RUN)).rejects.toBeInstanceOf(Unreachable)
  })

  it('refuses a body naming another realm', async () => {
    const { client } = clientWith(() => json(runSnapshot(undefined, 5, OTHER_REALM)))
    client.attach(REALM)
    await expect(client.run(RUN)).rejects.toBeInstanceOf(ForeignRealm)
  })

  it('resumes the durable feed strictly after a position, and only by ?after=', async () => {
    const { client, calls } = clientWith(() => sse(''))
    await new Promise<void>((resolve) => {
      client.controlFeed(41, { onEvent: () => {}, onClosed: () => resolve() })
    })
    expect(calls[0]?.url).toBe('http://127.0.0.1:7777/v1/events?after=41')
    // Presenting Last-Event-ID as well is refused by the contract when the two
    // disagree, so the client never has a second opinion about its position.
    expect(new Headers(calls[0]?.init?.headers).get('Last-Event-ID')).toBeNull()
  })

  it('delivers control frames and stops on one from another realm', async () => {
    const mine = JSON.stringify(controlEvent({ cursor: 5 }))
    const theirs = JSON.stringify(controlEvent({ cursor: 6, realm_id: OTHER_REALM }))
    const { client } = clientWith(() =>
      sse(`event: control\ndata: ${mine}\n\nevent: control\ndata: ${theirs}\n\n`),
    )
    client.attach(REALM)

    const seen: unknown[] = []
    const closed = await new Promise<unknown>((resolve) => {
      client.controlFeed(null, {
        onEvent: (event) => seen.push(event),
        onClosed: resolve,
      })
    })
    expect(seen).toHaveLength(1)
    expect(closed).toBeInstanceOf(ForeignRealm)
  })

  it('carries the mandatory anchor into a session subscription', async () => {
    const { client, calls } = clientWith(() => sse(''))
    await new Promise<void>((resolve) => {
      client.sessionStream(RUN, 'anchor-1-3', {
        onFrame: () => {},
        onRefetchRequired: () => {},
        onClosed: () => resolve(),
      })
    })
    expect(calls[0]?.url).toContain('/stream?after=anchor-1-3')
  })

  it('routes a broken timeline to the refetch handler, not to the content one', async () => {
    const refusal = JSON.stringify({
      realm_id: REALM,
      code: 'timeline_refetch_required',
      rule: 'the runtime renumbered or skipped this session content',
    })
    const { client } = clientWith(() => sse(`event: error\ndata: ${refusal}\n\n`))
    client.attach(REALM)

    const frames: unknown[] = []
    const refusals: unknown[] = []
    await new Promise<void>((resolve) => {
      client.sessionStream(RUN, 'anchor', {
        onFrame: (frame) => frames.push(frame),
        onRefetchRequired: (received) => refusals.push(received),
        onClosed: () => resolve(),
      })
    })
    expect(frames).toEqual([])
    expect(refusals).toHaveLength(1)
  })

  it('reports a refused subscription as the refusal it is', async () => {
    const { client } = clientWith(() =>
      json({ realm_id: REALM, code: 'unsupported_capability', rule: 'no live events' }, 422),
    )
    const closed = await new Promise<unknown>((resolve) => {
      client.sessionStream(RUN, 'anchor', {
        onFrame: () => {},
        onRefetchRequired: () => {},
        onClosed: resolve,
      })
    })
    expect(closed).toMatchObject({ name: 'Refused', status: 422 })
  })
})
