/**
 * The only way this console reaches a realm.
 *
 * Every call is an authenticated `/v1` route of `kontor-api`. There is no other
 * transport in this application: no runtime is called directly, no local file is
 * read, no store is opened, and no endpoint is discovered from a response. A view
 * that wants data the contract does not serve has to say so rather than find
 * another way to it.
 *
 * Two rules are enforced here rather than in the views, because a view that
 * forgets them fails silently:
 *
 * 1. **One realm.** Once the realm is known, every body and every frame that
 *    names a realm must name that one, or it is refused before a reducer sees it.
 * 2. **Typed refusals.** A non-2xx answer becomes a {@link Refused} carrying the
 *    contract's own code, so callers branch on `code` and never on prose.
 */
import type {
  AdvisorRun,
  AdvanceCompletionRequest,
  CodeHelpProjection,
  CommitteeRun,
  CompletionOutcome,
  CompletionState,
  ConsultationSeatRecovery,
  CoreTeam,
  CoreTeamApplyRequest,
  CoreTeamOutcome,
  CoreTeamPreview,
  CoreTeamPreviewRequest,
  EnsureQuickSessionRequest,
  EpicProjection,
  Health,
  InvokeConsultationRequest,
  MessageAck,
  ModelCatalogProjection,
  PermissionAck,
  ProfileCatalog,
  ProjectCapacity,
  PromotedSession,
  PromotionApplyRequest,
  PromotionPreview,
  ProviderQuotaState,
  QuickRoles,
  QuickSession,
  Realm,
  RecoverConsultationSeatRequest,
  ReplacedSeat,
  ReplaceSeatRequest,
  RemediateCompletionRequest,
  Refusal,
  RunSnapshot,
  RuntimeSettlement,
  SeatBindingOutcome,
  SeatBindingRequest,
  SeatQuotaState,
  SeatRecovery,
  StreamFrame,
  StreamRefusal,
  TaskSnapshot,
  TimelinePage,
  TeamsProjection,
  TeamDraftRequest,
  TopologyProjection,
} from './types'
import type { Endpoint } from './endpoint'
import { SseParser } from './sse'

/** The realm refused the request, in its own words. */
export class Refused extends Error {
  /** The HTTP status the refusal was reported with. */
  readonly status: number
  /** The contract's refusal envelope. */
  readonly body: Refusal

  constructor(status: number, body: Refusal) {
    super(`${body.code}: ${body.rule}`)
    this.name = 'Refused'
    this.status = status
    this.body = body
  }
}

/**
 * The realm could not be reached, or answered something this contract cannot be.
 *
 * Kept separate from {@link Refused} so a channel failure is never rendered as a
 * decision the realm made.
 */
export class Unreachable extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options)
    this.name = 'Unreachable'
  }
}

/** A value arrived carrying a realm this console is not attached to. */
export class ForeignRealm extends Error {
  /** The realm this console is attached to. */
  readonly expected: string
  /** The realm the value claimed. */
  readonly received: string

  constructor(expected: string, received: string) {
    super('a value arrived from another realm and was discarded')
    this.name = 'ForeignRealm'
    this.expected = expected
    this.received = received
  }
}

/** An open server-sent stream. */
export interface StreamHandle {
  /** Stop reading and release the connection. */
  close(): void
}

/**
 * A control-plane snapshot: values, and the position they are consistent with.
 *
 * A subscriber takes one of these and then resumes `/v1/events` **strictly
 * after** `snapshotCursor`. Subscribing without one leaves a window whose events
 * are attributed to a state that already contained them.
 */
export interface ControlSnapshot {
  /** The realm every value came from. */
  readonly realmId: string
  /**
   * The position the subscription must resume strictly after.
   *
   * The *lowest* cursor of the snapshots read, so no event affecting any of them
   * can fall between the read and the subscription. An event already included in
   * a later snapshot is redelivered rather than missed, and a redelivery is
   * something the projection can recognize — a hole is not.
   */
  readonly snapshotCursor: number
  /** The runs read, each with the position its own value is consistent with. */
  readonly runs: readonly RunSnapshot[]
}

/** What a caller wants to be told about the durable control-plane feed. */
export interface ControlFeedHandlers {
  /** One delivered control-plane event. */
  onEvent(event: unknown): void
  /** The stream ended, or could not be opened. */
  onClosed(reason: Refused | Unreachable | ForeignRealm | null): void
}

/** What a caller wants to be told about one session's live content. */
export interface SessionStreamHandlers {
  /** One live content frame. */
  onFrame(frame: StreamFrame): void
  /** The runtime renumbered or skipped content; the timeline must be reread. */
  onRefetchRequired(refusal: StreamRefusal): void
  /** The stream ended, or could not be opened. */
  onClosed(reason: Refused | Unreachable | ForeignRealm | null): void
}

/** How the client reaches the network, so tests can supply their own. */
export interface ClientOptions {
  /** The `fetch` implementation to use. Defaults to the global one. */
  readonly fetchImpl?: typeof fetch
}

/** The SSE event name the control feed delivers events under. */
const CONTROL_EVENT = 'control'
/** The SSE event name the session stream delivers content under. */
const CONTENT_EVENT = 'content'
/** The SSE event name a broken timeline is reported under. */
const REFUSAL_EVENT = 'error'

/** A JSON body that may or may not name a realm. */
type MaybeRealmed = { realm_id?: unknown }

/** The authenticated `/v1` surface of one realm. */
export class KontorClient {
  readonly #endpoint: Endpoint
  readonly #fetch: typeof fetch
  #expectedRealm: string | null = null

  constructor(endpoint: Endpoint, options: ClientOptions = {}) {
    this.#endpoint = endpoint
    // Bound to `globalThis`: an unbound `fetch` throws "illegal invocation".
    this.#fetch = options.fetchImpl ?? globalThis.fetch.bind(globalThis)
  }

  /** The realm this console is attached to, once it has been read. */
  get realmId(): string | null {
    return this.#expectedRealm
  }

  /**
   * Attach this client to one realm.
   *
   * Everything read afterwards is checked against it. Re-attaching to a
   * different realm is what a caller does when the operator switches endpoints;
   * it never merges with what was read before, because every cache is keyed by
   * realm as well as by id.
   */
  attach(realmId: string): void {
    this.#expectedRealm = realmId
  }

  /** Liveness, identity and how far startup has got. */
  async health(): Promise<Health> {
    return this.#json<Health>('/v1/health')
  }

  /** This realm's immutable identity. */
  async realm(): Promise<Realm> {
    return this.#json<Realm>('/v1/realm')
  }

  /** One agent run, with the position its snapshot is consistent with. */
  async run(agentRunId: string): Promise<RunSnapshot> {
    return this.#json<RunSnapshot>(`/v1/runs/${encodeURIComponent(agentRunId)}`)
  }

  /**
   * The control-plane snapshot the durable feed is resumed from.
   *
   * # Why this takes ids
   *
   * The merged contract serves no realm-wide control-plane projection: the only
   * routes that carry a `snapshot_cursor` are the aggregate snapshots, and both
   * address one aggregate by id. So the console's control-plane snapshot *is*
   * the set of aggregates it was asked to observe, read together, anchored at
   * the lowest position any of them is consistent with.
   *
   * When the contract gains a realm-wide projection (KON-MVP-16), this method
   * becomes one call and the parameter goes away. Nothing above it changes:
   * callers already treat the result as one snapshot with one cursor, which is
   * the shape that projection will have.
   *
   * An id the realm does not have is left out rather than failing the whole
   * read — an aggregate that has been purged is not a reason to refuse to
   * snapshot the others. If nothing could be read at all, the first refusal is
   * raised, because then there is no position to anchor to.
   *
   * @throws {Refused} when no aggregate could be read.
   */
  async controlSnapshot(agentRunIds: readonly string[]): Promise<ControlSnapshot> {
    const results = await Promise.allSettled(
      agentRunIds.map(async (agentRunId) => this.run(agentRunId)),
    )
    const runs: RunSnapshot[] = []
    let refusal: unknown = null
    for (const result of results) {
      if (result.status === 'fulfilled') {
        runs.push(result.value)
      } else if (refusal === null) {
        refusal = result.reason
      }
    }
    if (runs.length === 0) {
      throw refusal ??
        new Unreachable('a control-plane snapshot needs at least one aggregate to read')
    }
    return {
      realmId: runs[0]?.realm_id ?? '',
      snapshotCursor: runs.reduce(
        (lowest, snapshot) => Math.min(lowest, snapshot.snapshot_cursor),
        Number.POSITIVE_INFINITY,
      ),
      runs,
    }
  }

  /** One task, with the position its snapshot is consistent with. */
  async task(projectId: string, taskId: string): Promise<TaskSnapshot> {
    return this.#json<TaskSnapshot>(
      `/v1/projects/${encodeURIComponent(projectId)}/tasks/${encodeURIComponent(taskId)}`,
    )
  }

  /** The model catalog discovered and served by this Realm. */
  async modelCatalog(): Promise<ModelCatalogProjection> {
    return this.#json<ModelCatalogProjection>('/v1/catalog')
  }

  /** Current Teams drafts and immutable revisions at one Realm cursor. */
  async teams(): Promise<TeamsProjection> {
    return this.#json<TeamsProjection>('/v1/teams')
  }

  /** Create or replace one server-held Teams draft. */
  async saveTeamDraft(draft: TeamDraftRequest, commandId: string): Promise<TeamsProjection> {
    return this.#json<TeamsProjection>('/v1/teams/drafts:save', {
      method: 'POST',
      headers: { 'Idempotency-Key': commandId, 'Content-Type': 'application/json' },
      body: JSON.stringify(draft),
    })
  }

  /** Publish the next immutable revision of one server-held Teams draft. */
  async publishTeam(teamId: string, commandId: string): Promise<TeamsProjection> {
    return this.#json<TeamsProjection>(`/v1/teams/${encodeURIComponent(teamId)}/publish`, {
      method: 'POST',
      headers: { 'Idempotency-Key': commandId },
    })
  }

  /** One Operational epic. */
  async epic(projectId: string, epicId: string): Promise<EpicProjection> {
    return this.#json<EpicProjection>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}`,
    )
  }

  /** The authoritative topology, optionally narrowed to one epic. */
  async topology(projectId: string, epicId?: string): Promise<TopologyProjection> {
    const query = epicId ? `?epic_id=${encodeURIComponent(epicId)}` : ''
    return this.#json<TopologyProjection>(
      `/v1/projects/${encodeURIComponent(projectId)}/topology:inspect${query}`,
    )
  }

  /** The project's Core Team. */
  async coreTeam(projectId: string): Promise<CoreTeam> {
    return this.#json<CoreTeam>(`/v1/projects/${encodeURIComponent(projectId)}/core-team`)
  }

  /** Preview a Core Team composition without committing it. */
  async previewCoreTeam(
    projectId: string,
    request: CoreTeamPreviewRequest,
  ): Promise<CoreTeamPreview> {
    return this.#post<CoreTeamPreview>(
      `/v1/projects/${encodeURIComponent(projectId)}/core-team:preview`,
      request,
    )
  }

  /** Apply exactly the Core Team preview the operator confirmed. */
  async applyCoreTeam(
    projectId: string,
    request: CoreTeamApplyRequest,
    commandId: string,
  ): Promise<CoreTeamOutcome> {
    return this.#command<CoreTeamOutcome>(
      `/v1/projects/${encodeURIComponent(projectId)}/core-team:apply`,
      commandId,
      request,
    )
  }

  /** The catalog-backed roles a Quick session may select. */
  async quickRoles(projectId: string): Promise<QuickRoles> {
    return this.#json<QuickRoles>(`/v1/projects/${encodeURIComponent(projectId)}/quick-roles`)
  }

  /** Open one Quick session, or read back the one this key already opened. */
  async ensureQuickSession(
    projectId: string,
    request: EnsureQuickSessionRequest,
    commandId: string,
  ): Promise<QuickSession> {
    return this.#command<QuickSession>(
      `/v1/projects/${encodeURIComponent(projectId)}/quick-sessions:ensure`,
      commandId,
      request,
    )
  }

  /**
   * Preview promoting one Quick session to an epic.
   *
   * POST, because the contract declares this path POST-only and a GET is
   * refused with 405 before the handler runs. It carries no body and no
   * idempotency key: the handler is addressed entirely by its path, and a pure
   * preview records nothing to replay.
   */
  async previewPromotion(projectId: string, quickSessionId: string): Promise<PromotionPreview> {
    return this.#json<PromotionPreview>(
      `/v1/projects/${encodeURIComponent(projectId)}/quick-sessions/${encodeURIComponent(quickSessionId)}/promotion:preview`,
      { method: 'POST' },
    )
  }

  /** Apply exactly the promotion preview the operator confirmed. */
  async applyPromotion(
    projectId: string,
    quickSessionId: string,
    request: PromotionApplyRequest,
    commandId: string,
  ): Promise<PromotedSession> {
    return this.#command<PromotedSession>(
      `/v1/projects/${encodeURIComponent(projectId)}/quick-sessions/${encodeURIComponent(quickSessionId)}/promotion:apply`,
      commandId,
      request,
    )
  }

  /** The server-owned admission picture for one project. */
  async projectCapacity(projectId: string): Promise<ProjectCapacity> {
    return this.#json<ProjectCapacity>(`/v1/projects/${encodeURIComponent(projectId)}/capacity`)
  }

  /** Every recorded provider/account quota state in this project. */
  async providerQuotaStates(projectId: string): Promise<ProviderQuotaState[]> {
    return this.#json<ProviderQuotaState[]>(
      `/v1/projects/${encodeURIComponent(projectId)}/provider-quota-states`,
    )
  }

  /** Every live delivery seat joined to its exact account and provider quota projections. */
  async seatQuotaStates(projectId: string): Promise<SeatQuotaState[]> {
    return this.#json<SeatQuotaState[]>(
      `/v1/projects/${encodeURIComponent(projectId)}/seat-quota-states`,
    )
  }

  /** Ask the runtime to settle one exact run; the caller supplies no outcome. */
  async runtimeSettle(
    projectId: string,
    agentRunId: string,
    commandId: string,
  ): Promise<RuntimeSettlement> {
    return this.#command<RuntimeSettlement>(
      `/v1/projects/${encodeURIComponent(projectId)}/agent-runs/${encodeURIComponent(agentRunId)}/runtime:settle`,
      commandId,
      undefined,
    )
  }

  /** Replace one terminal persistent delivery seat under the server's exact CAS request. */
  async replaceSeat(
    projectId: string,
    agentRunId: string,
    request: ReplaceSeatRequest,
    commandId: string,
  ): Promise<ReplacedSeat> {
    return this.#command<ReplacedSeat>(
      `/v1/projects/${encodeURIComponent(projectId)}/agent-runs/${encodeURIComponent(agentRunId)}/successors:replace`,
      commandId,
      request,
    )
  }

  /** Recover one quota-blocked delivery seat from fresh server-owned evidence. */
  async recoverSeat(
    projectId: string,
    agentRunId: string,
    commandId: string,
  ): Promise<SeatRecovery> {
    return this.#command<SeatRecovery>(
      `/v1/projects/${encodeURIComponent(projectId)}/agent-runs/${encodeURIComponent(agentRunId)}/successors:recover`,
      commandId,
      undefined,
    )
  }

  /** Recover one idle Committee native filler while preserving its logical SeatBinding. */
  async recoverConsultationSeat(
    projectId: string,
    committeeRunId: string,
    seatBindingId: string,
    request: RecoverConsultationSeatRequest,
    commandId: string,
  ): Promise<ConsultationSeatRecovery> {
    return this.#command<ConsultationSeatRecovery>(
      `/v1/projects/${encodeURIComponent(projectId)}/committee-runs/${encodeURIComponent(committeeRunId)}/seats/${encodeURIComponent(seatBindingId)}/recover`,
      commandId,
      request,
    )
  }

  /** Ask Kontor to observe one exact topology SeatBinding. */
  async seatAttention(
    projectId: string,
    seatBindingId: string,
    request: SeatBindingRequest,
    commandId: string,
  ): Promise<SeatBindingOutcome> {
    return this.#command<SeatBindingOutcome>(
      `/v1/projects/${encodeURIComponent(projectId)}/seat-bindings/${encodeURIComponent(seatBindingId)}/attention`,
      commandId,
      request,
    )
  }

  /** Help for the controlled codes pinned by one epic. */
  async codeHelp(projectId: string, epicId: string): Promise<CodeHelpProjection> {
    return this.#json<CodeHelpProjection>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/code-help`,
    )
  }

  /** Every published Advisor profile revision. */
  async advisorProfiles(projectId: string): Promise<ProfileCatalog> {
    return this.#json<ProfileCatalog>(
      `/v1/projects/${encodeURIComponent(projectId)}/advisor-profiles`,
    )
  }

  /** Every published Committee template revision. */
  async committeeTemplates(projectId: string): Promise<ProfileCatalog> {
    return this.#json<ProfileCatalog>(
      `/v1/projects/${encodeURIComponent(projectId)}/committee-templates`,
    )
  }

  /** Every published Completion profile revision. */
  async completionProfiles(projectId: string): Promise<ProfileCatalog> {
    return this.#json<ProfileCatalog>(
      `/v1/projects/${encodeURIComponent(projectId)}/completion-profiles`,
    )
  }

  /** Invoke one Advisor consultation. */
  async invokeAdvisor(
    projectId: string,
    epicId: string,
    request: InvokeConsultationRequest,
    commandId: string,
  ): Promise<AdvisorRun> {
    return this.#command<AdvisorRun>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/advisor-runs:invoke`,
      commandId,
      request,
    )
  }

  /** Invoke one Committee consultation. */
  async invokeCommittee(
    projectId: string,
    epicId: string,
    request: InvokeConsultationRequest,
    commandId: string,
  ): Promise<CommitteeRun> {
    return this.#command<CommitteeRun>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/committee-runs:invoke`,
      commandId,
      request,
    )
  }

  /** One epic's current completion state. */
  async completion(projectId: string, epicId: string): Promise<CompletionState> {
    return this.#json<CompletionState>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/completion`,
    )
  }

  /** Advance completion from the revision the operator confirmed. */
  async advanceCompletion(
    projectId: string,
    epicId: string,
    request: AdvanceCompletionRequest,
    commandId: string,
  ): Promise<CompletionOutcome> {
    return this.#command<CompletionOutcome>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/completion:advance`,
      commandId,
      request,
    )
  }

  /** Return completion to remediation from the revision the operator confirmed. */
  async remediateCompletion(
    projectId: string,
    epicId: string,
    request: RemediateCompletionRequest,
    commandId: string,
  ): Promise<CompletionOutcome> {
    return this.#command<CompletionOutcome>(
      `/v1/projects/${encodeURIComponent(projectId)}/epics/${encodeURIComponent(epicId)}/completion:remediate`,
      commandId,
      request,
    )
  }

  /** One page of a session's recorded content. */
  async timeline(
    agentRunId: string,
    after?: string | null,
    limit?: number,
  ): Promise<TimelinePage> {
    const query = new URLSearchParams()
    if (after) {
      query.set('after', after)
    }
    if (limit !== undefined) {
      query.set('limit', String(limit))
    }
    const suffix = query.toString() ? `?${query.toString()}` : ''
    return this.#json<TimelinePage>(
      `/v1/sessions/${encodeURIComponent(agentRunId)}/timeline${suffix}`,
    )
  }

  /**
   * Deliver one message into a session.
   *
   * `messageId` is presented as the `Idempotency-Key`, and the contract *is* that
   * the key is the stable client message id. The caller keeps it across retries;
   * this method never mints one, because a client that generates a key per
   * attempt has turned a retry into a second message.
   */
  async sendMessage(
    agentRunId: string,
    body: string,
    messageId: string,
  ): Promise<MessageAck> {
    const envelope = await this.#json<MessageAck & MaybeRealmed>(
      `/v1/sessions/${encodeURIComponent(agentRunId)}/messages`,
      {
        method: 'POST',
        headers: { 'Idempotency-Key': messageId, 'Content-Type': 'application/json' },
        body: JSON.stringify({ body }),
      },
    )
    return envelope
  }

  /**
   * Answer one permission request raised inside a session.
   *
   * `responseId` is the caller's stable response key, held across retries for the
   * same reason a message id is.
   */
  async respondPermission(
    agentRunId: string,
    requestId: string,
    decision: string,
    responseId: string,
  ): Promise<PermissionAck> {
    return this.#json<PermissionAck>(
      `/v1/sessions/${encodeURIComponent(agentRunId)}/permissions/${encodeURIComponent(requestId)}`,
      {
        method: 'POST',
        headers: { 'Idempotency-Key': responseId, 'Content-Type': 'application/json' },
        body: JSON.stringify({ decision }),
      },
    )
  }

  /**
   * Follow the durable control-plane feed strictly after a position.
   *
   * Only `?after=` is presented, never `Last-Event-ID` as well: the contract
   * refuses a caller that sends both and disagrees, and there is nothing to gain
   * from having two spellings of one belief about our own position.
   */
  controlFeed(after: number | null, handlers: ControlFeedHandlers): StreamHandle {
    const suffix = after === null ? '' : `?after=${encodeURIComponent(String(after))}`
    return this.#stream(`/v1/events${suffix}`, (frame) => {
      if (frame.event !== CONTROL_EVENT) {
        return null
      }
      const parsed = this.#parse(frame.data)
      const foreign = this.#foreign(parsed)
      if (foreign) {
        return foreign
      }
      handlers.onEvent(parsed)
      return null
    }, handlers.onClosed)
  }

  /**
   * Follow one session's content strictly after a validated timeline anchor.
   *
   * The anchor is mandatory in the contract, so it is mandatory here.
   */
  sessionStream(
    agentRunId: string,
    anchor: string,
    handlers: SessionStreamHandlers,
  ): StreamHandle {
    const path =
      `/v1/sessions/${encodeURIComponent(agentRunId)}/stream` +
      `?after=${encodeURIComponent(anchor)}`
    return this.#stream(path, (frame) => {
      const parsed = this.#parse(frame.data)
      const foreign = this.#foreign(parsed)
      if (foreign) {
        return foreign
      }
      if (frame.event === REFUSAL_EVENT) {
        handlers.onRefetchRequired(parsed as StreamRefusal)
        return null
      }
      if (frame.event === CONTENT_EVENT) {
        handlers.onFrame(parsed as StreamFrame)
      }
      return null
    }, handlers.onClosed)
  }

  /** The headers every request carries. */
  #headers(extra?: HeadersInit): Headers {
    const headers = new Headers(extra)
    headers.set('Authorization', `Bearer ${this.#endpoint.token}`)
    return headers
  }

  /** Read one JSON route, refusing anything from another realm. */
  async #json<T>(path: string, init: RequestInit = {}): Promise<T> {
    let response: Response
    try {
      response = await this.#fetch(`${this.#endpoint.baseUrl}${path}`, {
        ...init,
        headers: this.#headers(init.headers),
      })
    } catch (cause) {
      throw new Unreachable('the realm could not be reached', { cause })
    }
    if (!response.ok) {
      throw await this.#refusal(response)
    }
    let body: unknown
    try {
      body = await response.json()
    } catch (cause) {
      throw new Unreachable('the realm answered something that is not JSON', { cause })
    }
    const foreign = this.#foreign(body)
    if (foreign) {
      throw foreign
    }
    return body as T
  }

  /** POST one JSON body without creating a mutation key. */
  async #post<T>(path: string, body: unknown): Promise<T> {
    return this.#json<T>(path, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
  }

  /** POST one receipt-backed command under the caller's stable key. */
  async #command<T>(path: string, commandId: string, body: unknown): Promise<T> {
    return this.#json<T>(path, {
      method: 'POST',
      headers: { 'Idempotency-Key': commandId, 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
  }

  /** Turn a non-2xx answer into the refusal it claims to be. */
  async #refusal(response: Response): Promise<Refused | Unreachable> {
    let body: unknown
    try {
      body = await response.json()
    } catch {
      return new Unreachable(
        `the realm refused with status ${response.status} and no contract envelope`,
      )
    }
    if (
      body !== null &&
      typeof body === 'object' &&
      typeof (body as Refusal).code === 'string'
    ) {
      return new Refused(response.status, body as Refusal)
    }
    return new Unreachable(
      `the realm refused with status ${response.status} and no contract envelope`,
    )
  }

  /** Parse one frame's data, tolerating nothing. */
  #parse(data: string): unknown {
    try {
      return JSON.parse(data)
    } catch {
      return null
    }
  }

  /** Refuse a value that names a realm this console is not attached to. */
  #foreign(body: unknown): ForeignRealm | null {
    const expected = this.#expectedRealm
    if (expected === null || body === null || typeof body !== 'object') {
      return null
    }
    const claimed = (body as MaybeRealmed).realm_id
    if (typeof claimed === 'string' && claimed !== expected) {
      return new ForeignRealm(expected, claimed)
    }
    return null
  }

  /**
   * Read one SSE route until it ends or the caller closes it.
   *
   * `onFrame` returns a fatal error to stop on, or `null` to continue.
   */
  #stream(
    path: string,
    onFrame: (frame: { event: string; data: string }) => ForeignRealm | null,
    onClosed: (reason: Refused | Unreachable | ForeignRealm | null) => void,
  ): StreamHandle {
    const controller = new AbortController()
    let closedByCaller = false

    const run = async (): Promise<void> => {
      let response: Response
      try {
        response = await this.#fetch(`${this.#endpoint.baseUrl}${path}`, {
          headers: this.#headers({ Accept: 'text/event-stream' }),
          signal: controller.signal,
        })
      } catch (cause) {
        onClosed(
          closedByCaller ? null : new Unreachable('the stream could not be opened', { cause }),
        )
        return
      }
      if (!response.ok) {
        onClosed(await this.#refusal(response))
        return
      }
      if (!response.body) {
        onClosed(new Unreachable('the stream carried no body'))
        return
      }
      const reader = response.body.getReader()
      const decoder = new TextDecoder()
      const parser = new SseParser()
      try {
        for (;;) {
          const { done, value } = await reader.read()
          if (done) {
            break
          }
          const chunk = decoder.decode(value, { stream: true })
          for (const frame of parser.feed(chunk)) {
            const fatal = onFrame(frame)
            if (fatal) {
              controller.abort()
              onClosed(fatal)
              return
            }
          }
        }
        onClosed(null)
      } catch (cause) {
        onClosed(
          closedByCaller ? null : new Unreachable('the stream ended unexpectedly', { cause }),
        )
      }
    }

    void run()

    return {
      close(): void {
        closedByCaller = true
        controller.abort()
      },
    }
  }
}
