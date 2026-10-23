import { createPaseoClient, type PaseoAgent, type PaseoAgentListResult } from "@getpaseo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createSessionReadActions, sessionReadTarget } from "../client/session-read";
import { toDirectoryEntry } from "../client/session-directory";

function agent(id: string, patch: Partial<PaseoAgent> = {}): PaseoAgent {
  return {
    id, title: "Duplicate title", provider: "codex", cwd: "/fixtures/read", model: null,
    createdAt: "2026-01-01T00:00:00.000Z", updatedAt: "2026-01-01T00:00:01.000Z", lastUserMessageAt: null,
    status: "idle", currentModeId: null, availableModes: [], pendingPermissions: [], persistence: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: true,
      supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    requiresAttention: true, attentionReason: "finished", attentionTimestamp: "2026-01-01T00:00:01.000Z",
    archivedAt: null, labels: {}, ...patch,
  };
}

function page(agents: PaseoAgent[], nextCursor: string | null = null): PaseoAgentListResult {
  return { requestId: "page", entries: agents.map((agent) => ({ agent, project: {
    projectKey: "project-1", projectName: "Project", checkout: { cwd: "/fixtures/read", isGit: false,
      worktreeRoot: null, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false, mainRepoRoot: null },
  } })),
    pageInfo: { hasMore: nextCursor !== null, nextCursor, prevCursor: null } };
}

const clients: ReturnType<typeof createPaseoClient>[] = [];
afterEach(async () => {
  for (const client of clients.splice(0)) await client.close();
  vi.restoreAllMocks();
});

function setup(records: PaseoAgent[] = [agent("a")], hostId = "host-a") {
  const client = createPaseoClient({ url: "ws://127.0.0.1:1" });
  clients.push(client);
  Object.defineProperty(client.agents, "supportsMarkSeen", { configurable: true, get: () => true });
  const native = new Map(records.map((entry) => [entry.id, entry]));
  const requests: { id: string; expectedUpdatedAt: string; expectedAttentionTimestamp: string | null }[] = [];
  const list = vi.spyOn(client.agents, "list").mockResolvedValue(page(records));
  const ref = client.agents.ref.bind(client.agents);
  const refresh = vi.fn(async (id: string) => {
    const current = native.get(id);
    return current ? { requestId: "readback", agent: current, project: null } : null;
  });
  const markSeen = vi.fn<(
    id: string, expected: { expectedUpdatedAt: string; expectedAttentionTimestamp: string | null },
  ) => Promise<unknown>>(async (id, expected) => {
    requests.push({ id, ...expected });
    const current = native.get(id);
    const seen = current ? { ...current, requiresAttention: false, attentionReason: null, attentionTimestamp: null } : null;
    if (seen) native.set(id, seen);
    return { requestId: "ack", agentId: id, outcome: current ? "seen" : "missing", agent: seen, error: null };
  });
  vi.spyOn(client.agents, "ref").mockImplementation((id) => {
    const exactId = typeof id === "string" ? id : id.id;
    return { ...ref(id), refresh: () => refresh(exactId), markSeen: (expected: { expectedUpdatedAt: string; expectedAttentionTimestamp: string | null }) => markSeen(exactId, expected) };
  });
  const actions = createSessionReadActions(hostId, () => client);
  const target = (id = "a") => sessionReadTarget(hostId, toDirectoryEntry({ agent: native.get(id) ?? agent(id), project: null }));
  return { client, native, requests, list, markSeen, refresh, actions, target, ref };
}

describe("native read acknowledgement", () => {
  it("acknowledges one exact ID and frozen revision without provider or lifecycle calls", async () => {
    const s = setup([agent("a", { status: "error", lastError: "Still broken" }), agent("a-longer")]);
    expect(await s.actions.markOne(s.target())).toMatchObject({ hostId: "host-a", agentId: "a", outcome: "seen" });
    expect(s.requests).toEqual([{ id: "a", expectedUpdatedAt: "2026-01-01T00:00:01.000Z", expectedAttentionTimestamp: "2026-01-01T00:00:01.000Z" }]);
    expect(s.native.get("a")).toMatchObject({ status: "error", lastError: "Still broken", requiresAttention: false });
    expect(s.native.get("a-longer")?.requiresAttention).toBe(true);
    expect(s.refresh).not.toHaveBeenCalled();
  });

  it("refuses an older public SDK without sending legacy clear-attention", async () => {
    const s = setup();
    vi.mocked(s.client.agents.ref).mockImplementation(s.ref);
    expect((await s.actions.markOne(s.target())).outcome).toBe("unsupported");
    expect(s.markSeen).not.toHaveBeenCalled();
    expect(s.refresh).not.toHaveBeenCalled();
  });

  it("refuses an older host even when the SDK already has the new method", async () => {
    const s = setup();
    Object.defineProperty(s.client.agents, "supportsMarkSeen", { get: () => false });
    expect((await s.actions.markOne(s.target())).outcome).toBe("unsupported");
    await expect(s.actions.prepareHost()).rejects.toThrow("needs metadata-only read acknowledgement support");
    expect(s.markSeen).not.toHaveBeenCalled();
    expect(s.list).not.toHaveBeenCalled();
  });

  it("walks real pages including unread archives and separates input from acknowledgement", async () => {
    const s = setup([agent("active"), agent("archived", { archivedAt: "2026-01-02T00:00:00.000Z" }), agent("input", { attentionReason: "permission" })]);
    s.list.mockResolvedValueOnce(page([s.native.get("active")!], "next"))
      .mockResolvedValueOnce(page([s.native.get("archived")!, s.native.get("input")!]));
    const preview = await s.actions.prepareHost();
    expect(preview).toMatchObject({ hostId: "host-a", pages: 2, needsInputCount: 1, recordCount: 3 });
    expect(preview.targets.map(({ id }) => id)).toEqual(["active", "archived"]);
    expect(s.list.mock.calls.map(([options]) => options)).toEqual([
      { filter: { includeArchived: true, includeBackground: true, includeUnavailablePersisted: true }, sort: [{ key: "updated_at", direction: "desc" }], page: { limit: 200 } },
      { filter: { includeArchived: true, includeBackground: true, includeUnavailablePersisted: true }, sort: [{ key: "updated_at", direction: "desc" }], page: { limit: 200, cursor: "next" } },
    ]);
    const outcomes = [];
    for await (const item of s.actions.markHost(preview)) outcomes.push(item.outcome);
    expect(outcomes).toEqual(["seen", "seen"]);
    expect(s.native.get("input")?.requiresAttention).toBe(true);
  });

  it("rejects an incomplete paginated read before any acknowledgement", async () => {
    const s = setup();
    s.list.mockResolvedValue(page([agent("a")], "loop"));
    await expect(s.actions.prepareHost()).rejects.toThrow("repeated page cursor");
    expect(s.markSeen).not.toHaveBeenCalled();
  });

  it("never acknowledges another host even if a batch mixes host identities", async () => {
    const s = setup();
    const foreign = { ...s.target(), hostId: "host-b" };
    await expect(s.actions.markOne(foreign)).rejects.toThrow("different host");
    const batch = s.actions.markHost({ hostId: "host-a", targets: [s.target(), foreign], needsInputCount: 0, recordCount: 2, pages: 1 });
    await expect(batch.next()).rejects.toThrow("different host");
    expect(s.markSeen).not.toHaveBeenCalled();
  });

  it("keeps a prepared batch on its original client when the selected host changes", async () => {
    const original = setup();
    const foreign = setup([agent("a")], "host-b");
    let selected = original.client;
    const actions = createSessionReadActions("host-a", () => selected);
    const preview = await actions.prepareHost();
    selected = foreign.client;
    const results = [];
    for await (const item of actions.markHost(preview)) results.push(item);
    expect(results).toMatchObject([{ hostId: "host-a", agentId: "a", outcome: "seen" }]);
    expect(original.native.get("a")?.requiresAttention).toBe(false);
    expect(foreign.native.get("a")?.requiresAttention).toBe(true);
    expect(foreign.markSeen).not.toHaveBeenCalled();
  });

  it("recovers a lost response from fresh native metadata without sending twice", async () => {
    const s = setup();
    s.markSeen.mockImplementationOnce(async () => {
      s.native.set("a", agent("a", { requiresAttention: false, attentionReason: null, attentionTimestamp: null }));
      throw new Error("Lost ACK");
    });
    expect((await s.actions.markOne(s.target())).outcome).toBe("already_seen");
    expect(s.markSeen).toHaveBeenCalledTimes(1);
    expect(s.refresh).toHaveBeenCalledExactlyOnceWith("a");
  });

  it.each(["auth", "timeout"])("keeps %s and readback failure unknown, never missing", async (failure) => {
    const s = setup();
    s.markSeen.mockRejectedValueOnce(new Error(failure));
    s.refresh.mockRejectedValueOnce(new Error(failure));
    expect((await s.actions.markOne(s.target())).outcome).toBe("unknown");
    expect(s.native.get("a")?.requiresAttention).toBe(true);
  });

  it("leaves concurrent results unread and reports partial results independently", async () => {
    const s = setup([agent("a"), agent("b"), agent("c")]);
    const preview = await s.actions.prepareHost();
    s.markSeen.mockImplementationOnce(async () => ({ requestId: "ack", agentId: "a", outcome: "changed", agent: agent("a", { updatedAt: "2026-01-01T00:00:02.000Z" }), error: null }));
    s.markSeen.mockRejectedValueOnce(new Error("Timeout"));
    s.refresh.mockRejectedValueOnce(new Error("Timeout"));
    const results = [];
    for await (const item of s.actions.markHost(preview)) results.push(item);
    expect(results.map(({ agentId, outcome }) => [agentId, outcome])).toEqual([["a", "changed"], ["b", "unknown"], ["c", "seen"]]);
    expect(s.native.get("a")?.requiresAttention).toBe(true);
    expect(s.native.get("b")?.requiresAttention).toBe(true);
  });

  it("does not fabricate success when the native snapshot already contains a new unread result", async () => {
    const s = setup();
    s.markSeen.mockResolvedValueOnce({ requestId: "ack", agentId: "a", outcome: "seen", agent: agent("a"), error: null });
    expect((await s.actions.markOne(s.target())).outcome).toBe("new_unread");
  });

  it("requires an exact native absence and refuses mismatched response IDs", async () => {
    const s = setup();
    s.native.delete("a");
    expect((await s.actions.markOne(s.target())).outcome).toBe("missing");
    s.markSeen.mockResolvedValueOnce({ requestId: "ack", agentId: "a-longer", outcome: "seen", agent: agent("a-longer", { requiresAttention: false }), error: null });
    expect((await s.actions.markOne(s.target())).outcome).toBe("unknown");
  });

  it("freezes the counted batch and reports unattempted items after cancellation", async () => {
    const s = setup([agent("a"), agent("b"), agent("future")]);
    const targets = [s.target("a"), s.target("b")];
    const controller = new AbortController();
    const batch = s.actions.markHost({ hostId: "host-a", targets, needsInputCount: 0, recordCount: 3, pages: 1 }, controller.signal);
    expect((await batch.next()).value).toMatchObject({ agentId: "a", outcome: "seen" });
    targets.push(s.target("future"));
    controller.abort();
    expect((await batch.next()).value).toMatchObject({ agentId: "b", outcome: "not_sent" });
    expect((await batch.next()).done).toBe(true);
    expect(s.requests.map(({ id }) => id)).toEqual(["a"]);
    expect(s.native.get("future")?.requiresAttention).toBe(true);
  });
});
