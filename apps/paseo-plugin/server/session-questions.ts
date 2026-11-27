import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import {
  acceptPendingQuestions, readPendingQuestions, recommendPermission, questionRequestHash, type AcceptQuestionsInput, type AcceptQuestionsItem,
  type PendingQuestions, type PermissionRequestLike, type SessionQuestionItem, type SessionQuestionResult,
} from "../shared/session-questions";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

/** One host list read, page-walked when the host reports more. No second inventory is kept. */
export type SessionQuestionsClient = Pick<PaseoApi, "agents">;

export const QUESTION_CONFIRM_MS = 5_000;
export const QUESTION_POLL_MS = 100;
const QUESTION_PAGE_LIMIT = 200;
const QUESTION_MAX_PAGES = 1_000;

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Every live agent, without archived rows or duplicate IDs across pages. */
async function liveAgents(client: SessionQuestionsClient): Promise<Agent[]> {
  const agents = new Map<string, Agent>();
  const cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < QUESTION_MAX_PAGES; page++) {
    const result = await client.agents.list({
      filter: { includeArchived: false },
      page: { limit: QUESTION_PAGE_LIMIT, ...(cursor === undefined ? {} : { cursor }) },
    });
    for (const { agent } of result.entries) {
      if (agent.archivedAt == null) {
        agents.set(agent.id, agent);
      }
    }
    if (!result.pageInfo.hasMore) {
      return [...agents.values()];
    }
    const next = result.pageInfo.nextCursor;
    if (!next || cursors.has(next)) {
      throw new Error("The host reported more sessions without a usable next page; pending permissions are not complete.");
    }
    cursors.add(next);
    cursor = next;
  }
  throw new Error(`The host still reports more sessions after ${QUESTION_MAX_PAGES} pages; pending permissions are not complete.`);
}

/** One review row per pending request. The recommendation and the hash come from the request content only. */
export function pendingQuestionItems(agents: readonly Agent[]): SessionQuestionItem[] {
  return agents.flatMap((agent) => (agent.pendingPermissions ?? []).map((request) => {
    const recommendation = recommendPermission(request as PermissionRequestLike);
    return {
      agentId: agent.id,
      agentTitle: agent.title?.trim() || agent.id,
      agentStatus: agent.status,
      requestId: request.id,
      kind: request.kind,
      title: request.title?.trim() || request.name?.trim() || request.kind,
      label: recommendation.label,
      response: recommendation.response,
      hash: questionRequestHash(request as PermissionRequestLike),
      supported: recommendation.supported,
      reason: recommendation.reason,
    };
  }));
}

export type SessionQuestionsOptions = {
  /** How long one sent answer may take to disappear from the agent before it reports uncertain. */
  readonly confirmMs?: number;
  readonly pollMs?: number;
  readonly wait?: (ms: number) => Promise<void>;
  readonly now?: () => number;
  /** Pins whose earlier answer was not confirmed; this worker never sends one of them again. */
  readonly uncertain?: Set<string>;
};

const defaultWait = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * The read lists every live pending request. The write re-reads each named
 * agent and request, compares the review's hash, recomputes the response from
 * that request, sends it once, then polls for disappearance. It never sends for
 * an item the review marked unsupported, a changed item, or a second time for
 * an item it reported uncertain.
 */
export function createSessionQuestionsService(client: SessionQuestionsClient, options: SessionQuestionsOptions = {}) {
  const confirmMs = options.confirmMs ?? QUESTION_CONFIRM_MS;
  const pollMs = options.pollMs ?? QUESTION_POLL_MS;
  const wait = options.wait ?? defaultWait;
  const now = options.now ?? (() => Date.now());
  const uncertain = options.uncertain ?? new Set<string>();

  const reRead = async (agentId: string): Promise<{ readonly handle: ReturnType<SessionQuestionsClient["agents"]["ref"]>; readonly agent: Agent | null }> => {
    const handle = client.agents.ref(agentId);
    return { handle, agent: (await handle.refresh())?.agent ?? null };
  };

  const result = (item: AcceptQuestionsItem, outcome: SessionQuestionResult["outcome"], message: string | null): SessionQuestionResult =>
    ({ agentId: item.agentId, requestId: item.requestId, outcome, message });

  async function answer(item: AcceptQuestionsItem): Promise<SessionQuestionResult> {
    let handle: ReturnType<SessionQuestionsClient["agents"]["ref"]>;
    let agent: Agent | null;
    try {
      ({ handle, agent } = await reRead(item.agentId));
    } catch (error) {
      return result(item, "uncertain", `The session could not be re-read; nothing was sent. ${errorText(error)}`);
    }
    if (agent === null) {
      return result(item, "already-answered", "This request is no longer pending.");
    }
    const request = (agent.pendingPermissions ?? []).find((entry) => entry.id === item.requestId);
    if (!request) {
      return result(item, "already-answered", "This request is no longer pending.");
    }
    if (questionRequestHash(request as PermissionRequestLike) !== item.hash) {
      return result(item, "changed", "The request changed since the review; read it again.");
    }
    const recommendation = recommendPermission(request as PermissionRequestLike);
    if (!recommendation.supported || recommendation.response === null) {
      return result(item, "unsupported", recommendation.reason ?? "The operator must answer this request.");
    }
    const pin = `${item.agentId}\u0000${item.requestId}\u0000${item.hash}`;
    if (uncertain.has(pin)) {
      return result(item, "uncertain", "An earlier answer for this exact request was not confirmed; it is not resent. Check the session before answering it manually.");
    }
    try {
      await handle.respondToPermission({ requestId: item.requestId, response: recommendation.response });
    } catch (error) {
      uncertain.add(pin);
      return result(item, "uncertain", `The answer did not report success; it may have been delivered. Check before trying again; it is not resent. ${errorText(error)}`);
    }

    const deadline = now() + confirmMs;
    for (;;) {
      // undefined means this read failed; that is not evidence the request left the agent.
      let after: Agent | null | undefined;
      try {
        after = (await handle.refresh())?.agent ?? null;
      } catch {
        after = undefined;
      }
      if (after === null || (after !== undefined && !(after.pendingPermissions ?? []).some((entry) => entry.id === item.requestId))) {
        uncertain.delete(pin);
        return result(item, "answered", null);
      }
      const remaining = deadline - now();
      if (remaining <= 0) {
        uncertain.add(pin);
        return result(item, "uncertain", "The request was still listed after the answer; it is not resent.");
      }
      await wait(Math.min(pollMs, remaining));
    }
  }

  return {
    async read(): Promise<PendingQuestions> {
      return { items: pendingQuestionItems(await liveAgents(client)) };
    },
    async accept(input: AcceptQuestionsInput): Promise<{ results: SessionQuestionResult[] }> {
      const seen = new Set<string>();
      const results: SessionQuestionResult[] = [];
      for (const item of input.items) {
        const key = `${item.agentId}\u0000${item.requestId}`;
        if (seen.has(key)) {
          continue;
        }
        seen.add(key);
        results.push(await answer(item));
      }
      return { results };
    },
  };
}

export function contributeSessionQuestions(server: PluginServerContext) {
  const uncertain = new Set<string>();
  server.handle(readPendingQuestions, (_input, { paseo }) => createSessionQuestionsService(paseo, { uncertain }).read());
  server.handle(acceptPendingQuestions, (input, { paseo }) => createSessionQuestionsService(paseo, { uncertain }).accept(input));
}
