import type { PaseoAgentListResult } from "@getpaseo/client";
import { describe, expect, it, vi } from "vitest";
import { createSessionQuestionsService, pendingQuestionItems, type SessionQuestionsClient } from "../server/session-questions";
import { questionRequestHash } from "../shared/session-questions";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Request = Agent["pendingPermissions"][number];

const capabilities = {
  supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false,
  supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true,
};

const agent = (id: string, pendingPermissions: Agent["pendingPermissions"], patch: Partial<Agent> = {}): Agent => ({
  id, provider: "codex", model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", cwd: "/fixture", workspaceId: "workspace-a",
  title: `Session ${id}`, status: "running", createdAt: "2026-01-01", updatedAt: "2026-01-01", lastUserMessageAt: null,
  capabilities, currentModeId: "default", availableModes: [], pendingPermissions, persistence: null, labels: {},
  requiresAttention: true, attentionReason: "permission", attentionTimestamp: "2026-01-01", archivedAt: null, ...patch,
});

const request = (id: string, patch: Partial<Request> = {}): Request => ({
  id, provider: "codex", name: "bash", kind: "tool", title: "Run command", input: { command: "ls" }, ...patch,
});

const allow = (id: string, label: string, variant: "primary" | "secondary" | "danger" = "primary"): NonNullable<Request["actions"]>[number] =>
  ({ id, label, behavior: "allow", variant });

const project = {
  projectKey: "fixture", projectName: "Fixture",
  checkout: { worktreeRoot: null, cwd: "/fixture", isGit: false as const, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false as const, mainRepoRoot: null },
};

const page = (entries: Agent[], patch: { hasMore?: boolean; nextCursor?: string | null } = {}) => ({
  requestId: "fixture",
  entries: entries.map((value) => ({ agent: value, project })),
  pageInfo: { hasMore: patch.hasMore ?? false, nextCursor: patch.nextCursor ?? null, prevCursor: null },
}) as PaseoAgentListResult;

function host(initial: Agent[], options: { afterSend?: (agentId: string, requestId: string) => void; refreshError?: Error } = {}) {
  const records = new Map(initial.map((value) => [value.id, value]));
  const list = vi.fn(async () => page([...records.values()]));
  const refresh = vi.fn(async (id: string) => {
    if (options.refreshError) throw options.refreshError;
    const value = records.get(id);
    return value ? { agent: value } : null;
  });
  const respond = vi.fn(async (id: string, input: { requestId: string; response: unknown }) => {
    options.afterSend?.(id, input.requestId);
  });
  const client = {
    agents: {
      list,
      ref: (id: string) => ({
        refresh: () => refresh(id),
        respondToPermission: (input: { requestId: string; response: unknown }) => respond(id, input),
      }),
    },
  } as unknown as SessionQuestionsClient;
  return { client, list, refresh, respond, records };
}

const serviceOf = (client: SessionQuestionsClient, confirmMs = 0) =>
  createSessionQuestionsService(client, { confirmMs, pollMs: 0, wait: async () => {} });

describe("pending question read", () => {
  it("lists every live pending request with its exact recommendation, label and pin", async () => {
    const plan = request("plan-request", { name: "ExitPlanMode", kind: "plan", title: "Ready to implement?", input: { plan: "Do it." }, actions: [allow("implement", "Implement", "primary")] });
    const mode = request("mode-request", { name: "SwitchMode", kind: "mode", title: "Switch to plan", actions: [allow("switch", "Switch")] });
    const question = request("question-request", {
      name: "AskUserQuestion", kind: "question", title: "Choose release", input: {
        questions: [{ header: "Release", question: "Which?", options: [{ label: "Wait" }, { label: "Now (Recommended)" }] }],
      },
    });
    const a = agent("agent-a", [
      request("tool-primary", { name: "bash", actions: [allow("allow-once", "Allow once")] }),
      plan, mode, question,
      request("tool-plain", { name: "read", actions: undefined }),
    ]);
    const h = host([a, agent("agent-empty", []), agent("agent-archived", [request("old")], { archivedAt: "2026-02-01" })]);
    const result = await serviceOf(h.client).read();

    expect(h.list).toHaveBeenCalledTimes(1);
    expect(result.items.map((item) => item.requestId)).toEqual(["tool-primary", "plan-request", "mode-request", "question-request", "tool-plain"]);
    expect(result.items).toMatchObject([
      { agentId: "agent-a", agentTitle: "Session agent-a", agentStatus: "running", kind: "tool", title: "Run command", label: "Allow once · bash",
        response: { behavior: "allow", selectedActionId: "allow-once" }, supported: true, reason: null },
      { kind: "plan", title: "Ready to implement?", label: "Implement · ExitPlanMode", response: { behavior: "allow", selectedActionId: "implement" }, supported: true, reason: null },
      { kind: "mode", title: "Switch to plan", label: "Switch · SwitchMode", response: { behavior: "allow", selectedActionId: "switch" }, supported: true, reason: null },
      { kind: "question", title: "Choose release", label: "Release: Now (Recommended)",
        response: { behavior: "allow", updatedInput: { questions: [{ header: "Release", question: "Which?", options: [{ label: "Wait" }, { label: "Now (Recommended)" }] }], answers: { Release: "Now (Recommended)" } } },
        supported: true, reason: null },
      { kind: "tool", title: "Run command", label: "Allow · read", response: { behavior: "allow" }, supported: true, reason: null },
    ]);
    for (const item of result.items) {
      expect(item.hash).toMatch(/^[a-f0-9]{64}$/);
    }
    expect(result.items[1]!.hash).toBe(questionRequestHash(plan));
  });

  it("lists each unsupported reason and never gives it a response", async () => {
    const written = request("written", { kind: "question", name: "AskUserQuestion", input: { questions: [{ header: "Name", question: "Your name?", options: [] }] } });
    const malformed = request("malformed", { kind: "question", name: "AskUserQuestion", input: { questions: "not-a-list" } });
    const empty = request("empty", { kind: "question", name: "AskUserQuestion", input: { questions: [] } });
    const dangerous = request("dangerous", { name: "write", actions: [allow("danger", "Allow all", "danger")] });
    const denied = request("denied", { name: "delete", kind: "other", actions: [{ id: "deny", label: "Deny", behavior: "deny", variant: "danger" }] });
    const result = await serviceOf(host([agent("a", [written, malformed, empty, dangerous, denied])]).client).read();

    expect(result.items.map((item) => item.requestId)).toEqual(["written", "malformed", "empty", "dangerous", "denied"]);
    for (const item of result.items) {
      expect(item.supported).toBe(false);
      expect(item.response).toBeNull();
      expect(item.label).toBe("Needs the operator");
      expect(item.reason).toBeTruthy();
    }
    expect(result.items.map((item) => item.reason)).toEqual([
      expect.stringContaining("written answer is required"),
      expect.stringContaining("could not be read"),
      expect.stringContaining("no questions"),
      expect.stringContaining("Only a dangerous allow action"),
      expect.stringContaining("No allow action"),
    ]);
  });

  it("reads every page and contributes nothing for an agent without pending requests", async () => {
    const first = agent("first", [request("one")]);
    const second = agent("second", [request("two")]);
    const list = vi.fn()
      .mockResolvedValueOnce(page([first], { hasMore: true, nextCursor: "next" }))
      .mockResolvedValueOnce(page([second]));
    const client = { agents: { list, ref: vi.fn() } } as unknown as SessionQuestionsClient;
    const result = await serviceOf(client).read();
    expect(list).toHaveBeenCalledTimes(2);
    expect(result.items.map((item) => item.requestId)).toEqual(["one", "two"]);

    expect(pendingQuestionItems([agent("quiet", [])])).toEqual([]);
  });
});

describe("pending question batch", () => {
  it("answers a matching pin with the exact response, then polls the request away", async () => {
    const pending = request("r1", { name: "bash", actions: [allow("allow-once", "Allow once")] });
    const a = agent("a", [pending]);
    const h = host([a], {
      afterSend: (id, requestId) => {
        const current = h.records.get(id)!;
        h.records.set(id, { ...current, pendingPermissions: current.pendingPermissions.filter((entry) => entry.id !== requestId) });
      },
    });
    const result = await serviceOf(h.client).accept({ items: [{ agentId: "a", requestId: "r1", hash: questionRequestHash(pending) }] });

    expect(h.respond).toHaveBeenCalledExactlyOnceWith("a", { requestId: "r1", response: { behavior: "allow", selectedActionId: "allow-once" } });
    expect(result.results).toEqual([{ agentId: "a", requestId: "r1", outcome: "answered", message: null }]);
  });

  it("reports changed and sends nothing when the pin no longer matches", async () => {
    const pending = request("r1", { actions: [allow("allow-once", "Allow once")] });
    const h = host([agent("a", [pending])]);
    const result = await serviceOf(h.client).accept({ items: [{ agentId: "a", requestId: "r1", hash: "f".repeat(64) }] });

    expect(h.respond).not.toHaveBeenCalled();
    expect(result.results).toEqual([{ agentId: "a", requestId: "r1", outcome: "changed", message: "The request changed since the review; read it again." }]);
  });

  it("reports already-answered when the request or the agent vanished before the send", async () => {
    const h = host([agent("a", [])]);
    const gone = await serviceOf(h.client).accept({ items: [{ agentId: "a", requestId: "r1", hash: "a".repeat(64) }] });
    expect(h.respond).not.toHaveBeenCalled();
    expect(gone.results[0]).toMatchObject({ outcome: "already-answered" });

    const missing = host([]);
    const noAgent = await serviceOf(missing.client).accept({ items: [{ agentId: "gone", requestId: "r1", hash: "a".repeat(64) }] });
    expect(missing.respond).not.toHaveBeenCalled();
    expect(noAgent.results[0]).toMatchObject({ outcome: "already-answered" });
  });

  it("reports uncertain after the bound with exactly one send, and never resends that exact pin", async () => {
    const pending = request("r1", { actions: [allow("allow-once", "Allow once")] });
    const h = host([agent("a", [pending])]);
    const service = serviceOf(h.client);
    const items = [{ agentId: "a", requestId: "r1", hash: questionRequestHash(pending) }];

    const first = await service.accept({ items });
    expect(h.respond).toHaveBeenCalledTimes(1);
    expect(first.results[0]).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("still listed") });

    const second = await service.accept({ items });
    expect(h.respond).toHaveBeenCalledTimes(1);
    expect(second.results[0]).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("not resent") });
  });

  it("deduplicates repeated items and reports one result per unique request", async () => {
    const pending = request("r1", { actions: [allow("allow-once", "Allow once")] });
    const h = host([agent("a", [pending])], {
      afterSend: (id, requestId) => {
        const current = h.records.get(id)!;
        h.records.set(id, { ...current, pendingPermissions: current.pendingPermissions.filter((entry) => entry.id !== requestId) });
      },
    });
    const item = { agentId: "a", requestId: "r1", hash: questionRequestHash(pending) };
    const result = await serviceOf(h.client).accept({ items: [item, item, { ...item }] });

    expect(h.respond).toHaveBeenCalledTimes(1);
    expect(result.results).toHaveLength(1);
    expect(result.results[0]).toMatchObject({ outcome: "answered" });
  });

  it("never sends an unsupported request, even when its pin matches", async () => {
    const pending = request("r1", { kind: "question", name: "AskUserQuestion", input: { questions: [{ header: "Name", question: "Your name?", options: [] }] } });
    const h = host([agent("a", [pending])]);
    const result = await serviceOf(h.client).accept({ items: [{ agentId: "a", requestId: "r1", hash: questionRequestHash(pending) }] });

    expect(h.respond).not.toHaveBeenCalled();
    expect(result.results[0]).toMatchObject({ outcome: "unsupported", message: expect.stringContaining("written answer is required") });
  });

  it("reports uncertain without a send when the session cannot be re-read", async () => {
    const pending = request("r1", { actions: [allow("allow-once", "Allow once")] });
    const h = host([agent("a", [pending])], { refreshError: new Error("host read failed") });
    const result = await serviceOf(h.client).accept({ items: [{ agentId: "a", requestId: "r1", hash: questionRequestHash(pending) }] });

    expect(h.respond).not.toHaveBeenCalled();
    expect(result.results[0]).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("nothing was sent") });
  });

  it("reports uncertain when the send itself fails, and does not resend it", async () => {
    const pending = request("r1", { actions: [allow("allow-once", "Allow once")] });
    const h = host([agent("a", [pending])]);
    h.respond.mockRejectedValueOnce(new Error("transport closed"));
    const service = serviceOf(h.client);
    const items = [{ agentId: "a", requestId: "r1", hash: questionRequestHash(pending) }];

    const first = await service.accept({ items });
    expect(first.results[0]).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("may have been delivered") });
    const second = await service.accept({ items });
    expect(h.respond).toHaveBeenCalledTimes(1);
    expect(second.results[0]).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("not resent") });
  });
});
