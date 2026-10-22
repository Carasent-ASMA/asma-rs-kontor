import { mkdtemp, open, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { createLifecycleService, lifecycleRow, type LifecycleClient } from "../server/session-lifecycle";
import { DEFAULT_RECOVERY_TARGET as target, previewLifecycle, executeLifecycleItem } from "../shared/session-lifecycle";
import { hasSessionAttention, toDirectoryEntry } from "../client/session-directory";
import { matchesStatus } from "../client/session-view";
import { openSessionById } from "../client/session-actions";
vi.mock("node:fs/promises", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs/promises")>();
  return { ...actual, open: vi.fn(actual.open), readFile: vi.fn(actual.readFile) };
});
const nativeFs = await vi.importActual<typeof import("node:fs/promises")>("node:fs/promises");
type Agent = PaseoAgentListResult["entries"][number]["agent"];
function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "codex", model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", cwd: "/fixtures/checkout", workspaceId: "workspace-a",
    title: "Duplicate title", status: "error", lastError: "Turn interrupted by transport failure", createdAt: "2001-01-01", updatedAt: "2001-01-01", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "full-access", availableModes: [], pendingPermissions: [], persistence: { provider: "codex", sessionId: `history-${id}` }, labels: {},
    requiresAttention: true, attentionReason: "error", attentionTimestamp: "2001-01-01", archivedAt: null, ...patch };
}
const dirs: string[] = [];
afterEach(async () => {
  vi.mocked(open).mockReset(); vi.mocked(readFile).mockReset();
  for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true });
});
async function setup(initial: Agent[] = [agent("a"), agent("b")]) {
  const root = await mkdtemp(join(tmpdir(), "kontor-lifecycle-test-")); dirs.push(root);
  const records = new Map(initial.map((value) => [value.id, value]));
  const send = vi.fn(async (_id: string) => {}), archive = vi.fn(async (id: string) => {
    const current = records.get(id)!; records.set(id, { ...current, status: "closed", archivedAt: "2026-10-21", activeTurn: undefined }); return { archivedAt: "2026-10-21" };
  });
  const refresh = vi.fn(async (id: string) => { const value = records.get(id); return value ? { agent: value, project: null } : null; });
  const client: LifecycleClient = { agents: {
    list: vi.fn(async () => ({ requestId: "fixture", entries: [...records.values()].map((value) => ({ agent: value, project: { projectKey: "fixture", projectName: "Fixture", checkout: { worktreeRoot: null, cwd: "/fixtures/checkout", isGit: false as const, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false as const, mainRepoRoot: null } } })), pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } })),
    ref: (id) => ({ refresh: () => refresh(id), send: async () => send(id), archive: () => archive(id), timeline: { refetch: async () => ({ entries: [{ type: "user_message", text: "Fictional task context" }], error: null }) } }),
  }, providers: { snapshot: vi.fn<LifecycleClient["providers"]["snapshot"]>(async () => ({ requestId: "fixture", generatedAt: "now", entries: [{ provider: "codex", enabled: true, status: "ready", models: [{ provider: "codex", id: "gpt-6.1-sol", label: "Sol", thinkingOptions: [{ id: "xhigh", label: "Extra high" }] }] }] })) } };
  const service = createLifecycleService(client, root);
  return { service, root, client, records, refresh, send, archive, preview: (action: "resume" | "archive" = "resume", ids = ["a"], scope: "filtered" | "host" = "filtered") => service.preview({ action, agentIds: ids, scope, target }) };
}
describe("native recovery classification", () => {
  it("does not infer failure from age, idle or a retained error notice", () => {
    for (const value of [agent("a", { status: "idle" }), agent("a", { status: "running" }), agent("a", { status: "closed" })]) expect(lifecycleRow(value, [value], "resume", target).eligible).toBe(false);
    expect(lifecycleRow(agent("a"), [], "resume", target).eligible).toBe(true);
  });
  it("keeps quota/auth, waits, replaced, missing-history and mismatched route visible with reasons", () => {
    const cases: Partial<Agent>[] = [{ lastError: "Usage limit reached" }, { lastError: "401 Unauthorized" }, { attentionReason: "permission" }, { labels: { "takeover.successor": "new" } }, { persistence: null }, { provider: "claude" }, { model: "other" }, { effectiveThinkingOptionId: "low" }];
    for (const patch of cases) { const row = lifecycleRow(agent("a", patch), [], "resume", target); expect(row.eligible).toBe(false); expect(row.reason.length).toBeGreaterThan(15); }
  });
  it("blocks every managed child while allowing explicit native stop/archive of active sessions", () => {
    const a = agent("a"), child = agent("child", { status: "running", workspaceId: "different-workspace", labels: { "paseo.parent-agent-id": "a" } });
    expect(lifecycleRow(a, [a, child], "archive", target)).toMatchObject({ eligible: false, children: [{ id: "child" }] });
    expect(lifecycleRow(agent("a", { status: "running" }), [], "archive", target).eligible).toBe(true);
  });
});
describe("durable lock metadata contention", () => {
  const lockPath = (root: string) => join(root, `intent-${createHash("sha256").update("a").digest("hex")}.json`);
  const priorReviewId = "00000000-0000-4000-8000-000000000001";
  it.each([
    ["empty", ""], ["partial", '{"reviewId":'], ["invalid JSON", "not json"],
    ["null", "null"], ["array", "[]"], ["scalar", '"metadata"'],
    ["missing fields", "{}"],
    ["wrong target", JSON.stringify({ reviewId: priorReviewId, agentId: "b", action: "archive" })],
    ["Resume owner", JSON.stringify({ reviewId: priorReviewId, agentId: "a", action: "resume" })],
    ["invalid review ID", JSON.stringify({ reviewId: "../other", agentId: "a", action: "archive" })],
    ["malformed UUID", JSON.stringify({ reviewId: "-".repeat(36), agentId: "a", action: "archive" })],
    ["invalid UUID version", JSON.stringify({ reviewId: "00000000-0000-f000-8000-000000000001", agentId: "a", action: "archive" })],
    ["invalid UUID variant", JSON.stringify({ reviewId: "00000000-0000-4000-0000-000000000001", agentId: "a", action: "archive" })],
    ["non-string review ID", JSON.stringify({ reviewId: 42, agentId: "a", action: "archive" })],
  ])("holds %s metadata without an effect, hint or lock removal", async (_label, contents) => {
    const s = await setup(), review = await s.preview(), file = lockPath(s.root);
    await writeFile(file, contents);
    const result = await s.service.execute(review.id, "a");
    expect(result).toMatchObject({ outcome: "blocked", message: expect.stringContaining("no second dispatch") });
    expect(result).not.toHaveProperty("reconcileReviewId");
    expect(await nativeFs.readFile(file, "utf8")).toBe(contents);
    expect(vi.mocked(open).mock.calls.filter(([path]) => path === file)).toHaveLength(1);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
    expect((await s.service.read(review.id))?.rows[0]?.outcome).toBe("pending");
  });
  it("offers only the exact Archive owner's valid reconciliation reference", async () => {
    const s = await setup(), owner = await s.preview("archive"), contender = await s.preview(), file = lockPath(s.root);
    const contents = JSON.stringify({ reviewId: owner.id, agentId: "a", action: "archive", phase: "intent" });
    await writeFile(file, contents);
    expect(await s.service.execute(contender.id, "a")).toMatchObject({ outcome: "blocked", reconcileReviewId: owner.id });
    expect(await nativeFs.readFile(file, "utf8")).toBe(contents);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each(["EACCES", "EIO", "ENOENT"])("holds an optional metadata read failure %s without reacquiring", async (code) => {
    const s = await setup(), review = await s.preview(), file = lockPath(s.root), contents = "owner is still publishing";
    await writeFile(file, contents);
    vi.mocked(readFile).mockImplementation(async (...args) => {
      if (args[0] === file) throw Object.assign(new Error("fixture read fault"), { code });
      return nativeFs.readFile(...args);
    });
    const result = await s.service.execute(review.id, "a");
    expect(result.outcome).toBe("blocked"); expect(result).not.toHaveProperty("reconcileReviewId");
    expect(await nativeFs.readFile(file, "utf8")).toBe(contents);
    expect(vi.mocked(open).mock.calls.filter(([path]) => path === file)).toHaveLength(1);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("does not reacquire when the owner removes its lock between EEXIST and metadata read", async () => {
    const s = await setup(), review = await s.preview(), file = lockPath(s.root);
    await writeFile(file, "owner metadata");
    vi.mocked(readFile).mockImplementation(async (...args) => {
      if (args[0] === file) await nativeFs.unlink(file);
      return nativeFs.readFile(...args);
    });
    const result = await s.service.execute(review.id, "a");
    expect(result.outcome).toBe("blocked"); expect(result).not.toHaveProperty("reconcileReviewId");
    await expect(nativeFs.stat(file)).rejects.toMatchObject({ code: "ENOENT" });
    expect(vi.mocked(open).mock.calls.filter(([path]) => path === file)).toHaveLength(1);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("blocks a contender during the winner's create-before-write window and preserves one effect across reload", async () => {
    const s = await setup(), owner = await s.preview(), contender = await s.preview(), file = lockPath(s.root);
    let created!: () => void, release!: () => void, held = false;
    const entered = new Promise<void>((resolve) => { created = resolve; });
    const released = new Promise<void>((resolve) => { release = resolve; });
    vi.mocked(open).mockImplementation(async (...args) => {
      const handle = await nativeFs.open(...args);
      if (args[0] === file && args[1] === "wx" && !held) { held = true; created(); await released; }
      return handle;
    });
    const winner = s.service.execute(owner.id, "a");
    try {
      await Promise.race([entered, winner.then(() => { throw new Error("Winner did not reach the publication barrier"); })]);
      expect(await nativeFs.readFile(file, "utf8")).toBe("");
      const result = await createLifecycleService(s.client, s.root).execute(contender.id, "a");
      expect(result.outcome).toBe("blocked"); expect(result).not.toHaveProperty("reconcileReviewId");
      expect(await nativeFs.readFile(file, "utf8")).toBe("");
      expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
    } finally { release(); await winner; }
    expect((await winner).outcome).toBe("submitted"); expect(s.send).toHaveBeenCalledTimes(1);
    const reloaded = createLifecycleService(s.client, s.root);
    expect((await reloaded.execute(owner.id, "a")).outcome).toBe("submitted");
    const fresh = await s.preview();
    expect((await reloaded.execute(fresh.id, "a")).outcome).toBe("blocked");
    expect(s.send).toHaveBeenCalledTimes(1); expect(s.archive).not.toHaveBeenCalled();
  });
});
describe("reviewed exact-ID effects and durable replay", () => {
  it("targets only the exact reviewed ID, keeps identity/history/checkout and reports dispatch rather than completion", async () => {
    const s = await setup(), before = s.records.get("a"); const review = await s.preview();
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    expect(s.send).toHaveBeenCalledExactlyOnceWith("a"); expect(s.archive).not.toHaveBeenCalled(); expect(s.records.get("a")).toBe(before);
    await expect(s.service.execute(review.id, "b")).rejects.toThrow("not in this review");
  });
  it("revalidates status, archive, placement and children changing while review is open", async () => {
    for (const patch of [{ status: "running" as const }, { archivedAt: "now" }, { cwd: "/other" }, { workspaceId: "other" }]) {
      const s = await setup(), review = await s.preview(); s.records.set("a", agent("a", patch));
      expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.send).not.toHaveBeenCalled();
    }
    const s = await setup(), review = await s.preview("archive"); s.records.set("child", agent("child", { labels: { "paseo.parent-agent-id": "a" } }));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.archive).not.toHaveBeenCalled();
  });
  it("fences double clicks, competing reviews and reload using durable per-session intents", async () => {
    const s = await setup(), one = await s.preview(), two = await s.preview();
    await Promise.all([s.service.execute(one.id, "a"), s.service.execute(one.id, "a"), s.service.execute(two.id, "a")]);
    expect(s.send).toHaveBeenCalledTimes(1);
    const reloaded = createLifecycleService(s.client, s.root);
    const settled = await Promise.all([reloaded.read(one.id), reloaded.read(two.id)]);
    const winner = settled.find((review) => review?.rows[0]?.outcome === "submitted"); expect(winner).toBeDefined();
    expect((await reloaded.execute(winner!.id, "a")).outcome).toBe("submitted"); expect(s.send).toHaveBeenCalledTimes(1);
    const fresh = await s.preview(); expect((await reloaded.execute(fresh.id, "a")).outcome).toBe("blocked"); expect(s.send).toHaveBeenCalledTimes(1);
  });
  it("holds an uncertain dispatch instead of retrying it after transport failure/reload", async () => {
    const s = await setup(); s.send.mockRejectedValueOnce(new Error("Lost acknowledgement")); const review = await s.preview();
    expect((await s.service.execute(review.id, "a")).outcome).toBe("uncertain");
    expect((await createLifecycleService(s.client, s.root).execute(review.id, "a")).outcome).toBe("uncertain");
    const another = await s.preview(); expect((await s.service.execute(another.id, "a")).outcome).toBe("blocked"); expect(s.send).toHaveBeenCalledTimes(1);
  });
  it("supports a mixed batch and retries only a fresh review of a known preflight block", async () => {
    const s = await setup(), review = await s.preview("resume", ["a", "b"]);
    s.records.set("a", agent("a", { status: "running" }));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked");
    expect((await s.service.execute(review.id, "b")).outcome).toBe("submitted");
    s.records.set("a", agent("a")); const retry = await s.preview(); expect((await s.service.execute(retry.id, "a")).outcome).toBe("submitted");
    const stored = await s.service.read(review.id); expect(stored?.rows.map((row) => row.outcome)).toEqual(["blocked", "submitted"]);
  });
  it("blocks missing history without sending or fabricating task context", async () => {
    const s = await setup(), original = s.client.agents.ref;
    s.client.agents.ref = (id) => ({ ...original(id), timeline: { refetch: async () => ({ entries: [], error: "Provider history not found" }) } });
    const review = await s.preview(); expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("history is unavailable") });
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("revalidates again after a slow provider/history preflight", async () => {
    const s = await setup(), original = s.client.agents.ref;
    s.client.agents.ref = (id) => ({ ...original(id), timeline: { refetch: async () => { s.records.set(id, agent(id, { archivedAt: "concurrently archived", status: "closed" })); return { entries: [{}], error: null }; } } });
    const review = await s.preview(); expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.send).not.toHaveBeenCalled();
  });
  it.each<Array<[string, Partial<Agent>]>[number]>([
    ["provider availability", { providerUnavailable: true }],
    ["effective runtime model", { runtimeInfo: { provider: "codex", sessionId: "history-a", model: "other-model", thinkingOptionId: "xhigh", modeId: "full-access" } }],
    ["persistence capability", { capabilities: { ...agent("a").capabilities, supportsSessionPersistence: false } }],
    ["permission wait", { attentionReason: "permission" }],
  ])("blocks a changed %s observed after history preflight even when updatedAt is unchanged", async (_label, patch) => {
    const s = await setup(), original = s.client.agents.ref, before = s.records.get("a")!;
    s.client.agents.ref = (id) => ({ ...original(id), timeline: { refetch: async () => { s.records.set(id, { ...s.records.get(id)!, ...patch }); return { entries: [{}], error: null }; } } });
    const review = await s.preview(), result = await s.service.execute(review.id, "a");
    expect(s.records.get("a")!.updatedAt).toBe(before.updatedAt);
    expect(result).toMatchObject({ outcome: "blocked", message: expect.stringContaining("changed during preflight") });
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("requires successful archive plus terminal readback; partial archive remains uncertain and history is never deleted", async () => {
    const s = await setup(), review = await s.preview("archive");
    s.archive.mockImplementationOnce(async (id) => { s.records.set(id, agent(id, { archivedAt: "now", status: "running" })); throw new Error("Runtime close failed"); });
    expect((await s.service.execute(review.id, "a")).outcome).toBe("uncertain"); expect(s.records.has("a")).toBe(true); expect(s.send).not.toHaveBeenCalled();
    const good = await setup(), ready = await good.preview("archive"); expect((await good.service.execute(ready.id, "a")).outcome).toBe("archived"); expect(good.records.has("a")).toBe(true);
    const files = await readdir(good.root); expect(files.some((file) => file.startsWith("result-"))).toBe(true); expect(await readFile(join(good.root, "latest.json"), "utf8")).toContain(ready.id);
  });
  it("keeps scope explicit, missing IDs visible and archive limited to one session", async () => {
    const s = await setup(); expect((await s.preview()).rows.map((row) => row.id)).toEqual(["a"]);
    expect((await s.preview("resume", [], "host")).rows).toHaveLength(2);
    expect((await s.preview("resume", ["missing"])).rows[0]).toMatchObject({ id: "missing", eligible: false, nativeStatus: "missing" });
    await expect(s.preview("archive", ["a", "b"])).rejects.toThrow("one exact session");
    expect(previewLifecycle.input.safeParse({ action: "archive", scope: "filtered", agentIds: ["a"], target, arbitraryPath: "/etc/passwd" }).success).toBe(false);
    expect(executeLifecycleItem.input.safeParse({ reviewId: "../../x", agentId: "a" }).success).toBe(false);
  });
  it("holds a wrong-ID post-archive readback uncertain and preserves its no-replay barrier", async () => {
    const s = await setup(), review = await s.preview("archive"); let reads = 0;
    s.archive.mockImplementationOnce(async () => ({ archivedAt: "acknowledged-only" }));
    s.refresh.mockImplementation(async (id) => ({ agent: ++reads === 3 ? agent("other-id", { archivedAt: "now", status: "closed" }) : s.records.get(id)!, project: null }));
    expect(await s.service.execute(review.id, "a")).toMatchObject({ id: "a", outcome: "uncertain", message: expect.stringContaining("exact-ID terminal") });
    expect(s.records.get("a")).toMatchObject({ status: "error", archivedAt: null });
    const reloaded = createLifecycleService(s.client, s.root);
    expect((await reloaded.read(review.id))?.rows[0]?.outcome).toBe("uncertain");
    expect((await reloaded.execute(review.id, "a")).outcome).toBe("uncertain");
    expect((await reloaded.execute((await s.preview("archive")).id, "a")).outcome).toBe("blocked");
    expect(s.archive).toHaveBeenCalledExactlyOnceWith("a"); expect(s.send).not.toHaveBeenCalled();
  });
});
describe("Attention availability evidence", () => {
  it("retains a present target after a well-formed wrong-ID refresh reply", async () => {
    const s = await setup(); s.refresh.mockResolvedValueOnce({ agent: agent("other-id"), project: null });
    const result = (await s.service.probe(["a"])).a!;
    expect(result).toMatchObject({ state: "unknown", reason: expect.stringContaining("different session") });
    const entry = toDirectoryEntry({ agent: s.records.get("a")!, project: null });
    // Apply the production client's binding to the requested displayed revision.
    const displayed = { ...entry, availability: { ...result, revision: entry.updatedAt } };
    expect(hasSessionAttention(displayed)).toBe(true);
    const openAgent = vi.fn();
    expect(openSessionById({ entries: [displayed], id: "a", serverId: "fixture-host", navigation: { openAgent, openWorkspace: vi.fn() } }).ok).toBe(true);
    expect(openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "a", serverId: "fixture-host" });
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("still treats a true null exact-ID response as absence", async () => {
    const s = await setup(); s.refresh.mockResolvedValueOnce(null);
    const result = (await s.service.probe(["a"])).a!;
    expect(result.state).toBe("missing");
    const entry = toDirectoryEntry({ agent: s.records.get("a")!, project: null });
    expect(hasSessionAttention({ ...entry, availability: { ...result, revision: entry.updatedAt } })).toBe(false);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("preserves good old sessions and unknown/probe failures; excludes only confirmed missing IDs and archives", async () => {
    const s = await setup(); s.refresh.mockImplementationOnce(async () => null).mockRejectedValueOnce(new Error("Provider/host temporarily unavailable"));
    const reads = await s.service.probe(["missing", "unknown", "a"]); expect(reads.missing?.state).toBe("missing"); expect(reads.unknown?.state).toBe("unknown"); expect(reads.a?.state).toBe("readable");
    const entry = toDirectoryEntry({ agent: agent("a"), project: null });
    expect(matchesStatus(entry, "attention")).toBe(true);
    for (const state of ["readable", "unknown"] as const) expect(hasSessionAttention({ ...entry, availability: { agentId: entry.id, state, reason: "probe", checkedAt: new Date().toISOString(), revision: entry.updatedAt } })).toBe(true);
    expect(hasSessionAttention({ ...entry, availability: { agentId: entry.id, state: "missing", reason: "missing", checkedAt: new Date().toISOString(), revision: entry.updatedAt } })).toBe(false);
    expect(hasSessionAttention({ ...entry, availability: { agentId: entry.id, state: "missing", reason: "old probe", checkedAt: "then", revision: "an earlier native revision" } })).toBe(true);
    expect(hasSessionAttention({ ...entry, archivedAt: "now" })).toBe(false);
    expect(matchesStatus({ ...entry, status: "idle", attentionReason: "permission", needsInput: true, requiresAttention: false }, "needs_input")).toBe(true);
    expect(s.send).not.toHaveBeenCalled(); expect(s.archive).not.toHaveBeenCalled();
  });
  it("only blocks opening a freshly confirmed missing ID; unknown and readable old sessions still open by ID", () => {
    const openAgent = vi.fn(), entry = toDirectoryEntry({ agent: agent("a"), project: null });
    for (const state of ["readable", "unknown", "missing"] as const) {
      const evidence = { agentId: entry.id, state, reason: "probe", checkedAt: new Date().toISOString(), revision: entry.updatedAt };
      const result = openSessionById({ entries: [{ ...entry, availability: evidence }], id: "a", serverId: "fixture-host", navigation: { openAgent, openWorkspace: vi.fn() } });
      expect(result.ok).toBe(state !== "missing");
    }
    expect(openAgent).toHaveBeenCalledTimes(2); expect(openAgent).toHaveBeenLastCalledWith({ agentId: "a", serverId: "fixture-host" });
  });
});


describe("P1 fresh native record reconciliation", () => {
  it("revalidates a missing record and records local removal without claiming daemon archive", async () => {
    const s = await setup(); s.refresh.mockResolvedValue(null);
    const review = await s.preview("archive");
    expect(review.rows[0]).toMatchObject({ id: "a", eligible: true, availability: { agentId: "a", state: "missing" } });
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "absent", availability: { localRemoval: true }, message: expect.stringContaining("no daemon archive") }); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each(["401 Unauthorized", "Timeout", "API not responding"])("keeps %s unknown without a false absence claim", async (message) => {
    const s = await setup(); s.refresh.mockRejectedValueOnce(new Error(message));
    const review = await s.preview("archive");
    expect(review.rows[0]).toMatchObject({ eligible: false, availability: { state: "unknown" } });
    expect(review.rows[0]!.reason).toContain(message); expect(s.archive).not.toHaveBeenCalled();
  });
  it("rejects a cross-ID review reply even when its record is well formed", async () => {
    const s = await setup(); s.refresh.mockResolvedValueOnce({ agent: agent("b"), project: null });
    expect((await s.preview("archive")).rows[0]).toMatchObject({ id: "a", eligible: false, availability: { state: "unknown" } });
    expect(s.archive).not.toHaveBeenCalled();
  });
  it("reports disappearance during confirmation as absent with no dispatch", async () => {
    const s = await setup(), review = await s.preview("archive"); s.records.delete("a");
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "absent", availability: { state: "missing" } }); expect(s.archive).not.toHaveBeenCalled();
  });
  it("reports already archived truthfully, and keeps a closed readable unread result", async () => {
    const s = await setup([agent("a", { status: "closed", attentionReason: "finished" })]);
    const review = await s.preview("archive"); expect(review.rows[0]!.eligible).toBe(true);
    s.records.set("a", agent("a", { status: "closed", archivedAt: "2026-10-21" }));
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "already_archived", availability: { state: "readable", archivedAt: "2026-10-21" } });
    expect(s.archive).not.toHaveBeenCalled();
  });
  it("exposes absent persistence honestly without loading history or blocking native archive", async () => {
    const s = await setup([agent("a", { persistence: null })]), original = s.client.agents.ref, history = vi.fn();
    s.client.agents.ref = (id) => ({ ...original(id), timeline: { refetch: history } });
    const review = await s.preview("archive");
    expect(review.rows[0]).toMatchObject({ eligible: true, availability: { state: "readable", providerHistory: "not-reported" } });
    expect((await s.service.execute(review.id, "a")).outcome).toBe("archived"); expect(history).not.toHaveBeenCalled();
  });
});

describe("force-close archive and read-only settlement", () => {
  it("rechecks exact placement after the final slow child inventory", async () => {
    const s = await setup(), original = s.client.agents.list; let lists = 0;
    s.client.agents.list = async options => { const result = await original(options); if (++lists === 3) s.records.set("a", agent("a", { workspaceId: "moved" })); return result; };
    const review = await s.preview("archive");
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each(["running", "initializing", "idle", "error", "closed"] as const)("archives exact %s even when activity updates during preflight", async (status) => {
    const s = await setup([agent("a", { status }), agent("b")]); const review = await s.preview("archive");
    s.records.set("a", agent("a", { status, updatedAt: "new-token-time", lastError: "401 Unauthorized" }));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("archived");
    expect(s.archive).toHaveBeenCalledExactlyOnceWith("a"); expect(s.records.get("b")?.archivedAt).toBeNull(); expect(s.send).not.toHaveBeenCalled();
  });
  it("settles a lost archive acknowledgement after reload by metadata only while retaining its fence", async () => {
    const s = await setup(), review = await s.preview("archive");
    s.archive.mockImplementationOnce(async (id) => { s.records.set(id, agent(id, { status: "closed", archivedAt: "now" })); throw new Error("Lost acknowledgement"); });
    expect((await s.service.execute(review.id, "a")).outcome).toBe("uncertain");
    const reloaded = createLifecycleService(s.client, s.root);
    expect(await reloaded.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", message: expect.stringContaining("No additional") });
    expect((await reloaded.execute(review.id, "a")).outcome).toBe("archived");
    expect(s.archive).toHaveBeenCalledTimes(1); expect((await readdir(s.root)).some((name) => name.startsWith("intent-"))).toBe(true);
  });
  it.each(["missing", "mismatch", "timeout", "still-running"])("never settles %s as archived or resends", async (state) => {
    const s = await setup(), review = await s.preview("archive"); s.archive.mockRejectedValueOnce(new Error("Lost transport"));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("uncertain");
    if (state === "missing") s.refresh.mockResolvedValue(null);
    else if (state === "mismatch") s.refresh.mockResolvedValue({ agent: agent("other", { status: "closed", archivedAt: "now" }), project: null });
    else if (state === "timeout") s.refresh.mockRejectedValue(new Error("Timeout"));
    else s.records.set("a", agent("a", { status: "running", archivedAt: "now" }));
    expect((await createLifecycleService(s.client, s.root).reconcile(review.id, "a")).outcome).toBe("uncertain");
    expect(s.archive).toHaveBeenCalledTimes(1); expect((await readdir(s.root)).some((name) => name.startsWith("intent-"))).toBe(true);
  });
  it("does not call an archived-but-still-running record successfully retired", async () => {
    const s = await setup(), review = await s.preview("archive");
    s.records.set("a", agent("a", { status: "running", archivedAt: "now" }));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.archive).not.toHaveBeenCalled();
  });
  it("retains local absence intent across reload, rechecks it, and restores readable or unknown IDs", async () => {
    const s = await setup(), review = await s.preview("archive"); s.records.delete("a");
    expect((await s.service.execute(review.id, "a")).outcome).toBe("absent");
    const reloaded = createLifecycleService(s.client, s.root);
    expect(await reloaded.dispositions(["a", "b"])).toMatchObject({ a: { state: "missing", localRemoval: true } });
    expect((await reloaded.dispositions(["a", "b"])).b).toBeUndefined();
    s.refresh.mockRejectedValueOnce(new Error("401 Unauthorized"));
    expect(await reloaded.dispositions(["a"])).toMatchObject({ a: { state: "unknown", localRemoval: false } });
    s.records.set("a", agent("a"));
    expect(await reloaded.dispositions(["a"])).toMatchObject({ a: { state: "readable", localRemoval: false } });
    expect(s.archive).not.toHaveBeenCalled(); expect(s.send).not.toHaveBeenCalled();
  });
});
