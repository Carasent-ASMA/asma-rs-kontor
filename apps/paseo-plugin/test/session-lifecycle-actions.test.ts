import { mkdtemp, open, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { createHash, randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { createLifecycleService, lifecycleRow, type LifecycleClient } from "../server/session-lifecycle";
import { DEFAULT_RECOVERY_TARGET as target, previewLifecycle, executeLifecycleItem, unresolvedArchive, type LifecycleReview } from "../shared/session-lifecycle";
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
    for (const value of [agent("a", { status: "idle" }), agent("a", { status: "running" }), agent("a", { status: "closed" })]) expect(lifecycleRow(value, [value], "resume").eligible).toBe(false);
    expect(lifecycleRow(agent("a"), [], "resume").eligible).toBe(true);
  });
  it("keeps quota/auth, waits, replaced and missing-history visible with reasons", () => {
    const cases: Partial<Agent>[] = [{ lastError: "Usage limit reached" }, { lastError: "401 Unauthorized" }, { attentionReason: "permission" }, { labels: { "takeover.successor": "new" } }, { persistence: null }];
    for (const patch of cases) { const row = lifecycleRow(agent("a", patch), [], "resume"); expect(row.eligible).toBe(false); expect(row.reason.length).toBeGreaterThan(15); }
  });
  it("blocks every managed child while allowing explicit native stop/archive of active sessions", () => {
    const a = agent("a"), child = agent("child", { status: "running", workspaceId: "different-workspace", labels: { "paseo.parent-agent-id": "a" } });
    expect(lifecycleRow(a, [a, child], "archive")).toMatchObject({ eligible: false, children: [{ id: "child" }] });
    expect(lifecycleRow(agent("a", { status: "running" }), [], "archive").eligible).toBe(true);
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
  it("keeps scope explicit, missing IDs visible and archive bounded to selected IDs", async () => {
    const s = await setup(); expect((await s.preview()).rows.map((row) => row.id)).toEqual(["a"]);
    expect((await s.preview("resume", [], "host")).rows).toHaveLength(2);
    expect((await s.preview("resume", ["missing"])).rows[0]).toMatchObject({ id: "missing", eligible: false, nativeStatus: "missing" });
    expect((await s.preview("archive", ["a", "b"])).rows.map((row) => row.id)).toEqual(["a", "b"]);
    for (const ids of [[], Array.from({ length: 51 }, (_, n) => `fixture-${n}`)]) await expect(s.preview("archive", ids)).rejects.toThrow("1–50");
    await expect(s.preview("archive", ["a"], "host")).rejects.toThrow("filtered scope");
    expect(previewLifecycle.input.safeParse({ action: "archive", scope: "filtered", agentIds: ["a"], target, arbitraryPath: "/etc/passwd" }).success).toBe(false);
    expect(executeLifecycleItem.input.safeParse({ reviewId: "../../x", agentId: "a" }).success).toBe(false);
  });
  it("holds a wrong-ID post-archive readback uncertain and preserves its no-replay barrier", async () => {
    const s = await setup(), review = await s.preview("archive"); let reads = 0;
    s.archive.mockImplementationOnce(async () => ({ archivedAt: "acknowledged-only" }));
    s.refresh.mockImplementation(async (id) => ({ agent: ++reads === 3 ? agent("other-id", { archivedAt: "now", status: "closed" }) : s.records.get(id)!, project: null }));
    expect(await s.service.execute(review.id, "a")).toMatchObject({ id: "a", outcome: "uncertain", message: expect.stringContaining("exact-ID read does not show") });
    expect(s.records.get("a")).toMatchObject({ status: "error", archivedAt: null });
    const reloaded = createLifecycleService(s.client, s.root);
    expect((await reloaded.read(review.id))?.rows[0]?.outcome).toBe("uncertain");
    expect((await reloaded.execute(review.id, "a")).outcome).toBe("uncertain");
    expect((await reloaded.execute((await s.preview("archive")).id, "a")).outcome).toBe("blocked");
    expect(s.archive).toHaveBeenCalledExactlyOnceWith("a"); expect(s.send).not.toHaveBeenCalled();
  });
});

describe("bounded bulk archive and managed children", () => {
  const family = () => [agent("parent"), agent("child", { labels: { "paseo.parent-agent-id": "parent" } }), agent("grandchild", { parentAgentId: "child" } as Partial<Agent>), agent("other")];
  it("previews without effects, orders all selected descendants first, and archives only exact selected IDs", async () => {
    const s = await setup(family()), review = await s.preview("archive", ["parent", "child", "grandchild"]);
    expect(review.rows.map((row) => row.id)).toEqual(["grandchild", "child", "parent"]);
    expect(review.rows.every((row) => row.eligible)).toBe(true); expect(s.archive).not.toHaveBeenCalled();
    for (const row of review.rows) expect(await s.service.execute(review.id, row.id)).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(s.archive.mock.calls.map(([id]) => id)).toEqual(["grandchild", "child", "parent"]);
    expect(s.records.get("other")!.archivedAt).toBeNull(); expect(s.send).not.toHaveBeenCalled();
  });
  it("blocks an unselected child, an out-of-order parent, and a newly attached child", async () => {
    const s = await setup(family()), partial = await s.preview("archive", ["parent", "other"]);
    expect(partial.rows.find((row) => row.id === "parent")!.eligible).toBe(false);
    expect(await s.service.execute(partial.id, "parent")).toMatchObject({ outcome: "blocked" });
    const full = await s.preview("archive", ["parent", "child", "grandchild"]);
    expect(await s.service.execute(full.id, "parent")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("children") });
    await s.service.execute(full.id, "grandchild");
    s.records.set("new", agent("new", { labels: { "paseo.parent-agent-id": "child" } }));
    expect(await s.service.execute(full.id, "child")).toMatchObject({ outcome: "blocked" });
    expect(s.archive.mock.calls.map(([id]) => id)).toEqual(["grandchild"]);
  });
  it("keeps an unconfirmed child's parent blocked even if archive membership was saved", async () => {
    const s = await setup(family().filter((row) => row.id !== "grandchild")), review = await s.preview("archive", ["parent", "child"]);
    s.archive.mockImplementationOnce(async (id) => { s.records.set(id, { ...s.records.get(id)!, archivedAt: "now", status: "idle" }); throw new Error("Lost acknowledgement"); });
    expect(await s.service.execute(review.id, "child")).toMatchObject({ outcome: "archived", runtime: "unconfirmed" });
    expect(await s.service.execute(review.id, "parent")).toMatchObject({ outcome: "blocked" });
    expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("accepts exactly 50 and rejects a cyclic child graph without dispatch", async () => {
    const s = await setup(Array.from({ length: 50 }, (_, n) => agent(`fixture-${n}`)));
    expect((await s.preview("archive", [...s.records.keys()])).rows).toHaveLength(50);
    const cycle = await setup([agent("a", { labels: { "paseo.parent-agent-id": "b" } }), agent("b", { labels: { "paseo.parent-agent-id": "a" } })]);
    await expect(cycle.preview("archive", ["a", "b"])).rejects.toThrow("cycle"); expect(cycle.archive).not.toHaveBeenCalled();
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
  it("rechecks exact placement after the last child inventory", async () => {
    const s = await setup(), original = s.client.agents.list; let lists = 0;
    // Walk 1 is the preview, walk 2 the execute's only child walk; the exact-ID read after it still sees the move.
    s.client.agents.list = async options => { const result = await original(options); if (++lists === 2) s.records.set("a", agent("a", { workspaceId: "moved" })); return result; };
    const review = await s.preview("archive");
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.archive).not.toHaveBeenCalled();
  });
  it("walks only active sessions, once in the preview and once in the execute, however much archived history the host holds", async () => {
    // Archived children never block (childrenOf), so archived history is never read for an archive.
    const history = Array.from({ length: 1_000 }, (_, n) => agent(`old-${n}`, { status: "closed", archivedAt: "2026-01-01", labels: { "paseo.parent-agent-id": "a" } }));
    const s = await setup([agent("a"), agent("b"), ...history]), calls: Array<{ includeArchived?: boolean; rows: number }> = [];
    s.client.agents.list = async (options) => {
      const rows = [...s.records.values()].filter((value) => options?.filter?.includeArchived || value.archivedAt == null);
      const start = Number(options?.page?.cursor ?? 0), limit = options?.page?.limit ?? 200, page = rows.slice(start, start + limit), more = start + limit < rows.length;
      calls.push({ includeArchived: options?.filter?.includeArchived, rows: page.length });
      return { requestId: "fixture", entries: page.map((value) => ({ agent: value, project: null })), pageInfo: { hasMore: more, nextCursor: more ? String(start + limit) : null, prevCursor: null } } as unknown as PaseoAgentListResult;
    };
    const review = await s.preview("archive");
    expect((await s.service.execute(review.id, "a")).outcome).toBe("archived"); expect(s.archive).toHaveBeenCalledExactlyOnceWith("a");
    expect(calls).toEqual([{ includeArchived: false, rows: 2 }, { includeArchived: false, rows: 2 }]);
  });
  it("still blocks an active child linked only by its parentAgentId field; Resume still walks archived history", async () => {
    const child = { ...agent("child", { status: "idle" }), parentAgentId: "a" } as Agent;
    const s = await setup([agent("a"), child]), original = s.client.agents.list, filters: unknown[] = [];
    s.client.agents.list = async (options) => { filters.push(options?.filter); return original(options); };
    const review = await s.preview("archive");
    expect(review.rows[0]).toMatchObject({ eligible: false, children: [{ id: "child" }] });
    expect((await s.service.execute(review.id, "a")).outcome).toBe("blocked"); expect(s.archive).not.toHaveBeenCalled();
    await s.preview("resume", ["a"], "host");
    expect(filters).toEqual([{ includeArchived: false }, { includeArchived: true }]);
  });
  it.each(["running", "initializing", "idle", "error", "closed"] as const)("archives exact %s even when activity updates during preflight", async (status) => {
    const s = await setup([agent("a", { status }), agent("b")]); const review = await s.preview("archive");
    s.records.set("a", agent("a", { status, updatedAt: "new-token-time", lastError: "401 Unauthorized" }));
    expect((await s.service.execute(review.id, "a")).outcome).toBe("archived");
    expect(s.archive).toHaveBeenCalledExactlyOnceWith("a"); expect(s.records.get("b")?.archivedAt).toBeNull(); expect(s.send).not.toHaveBeenCalled();
  });
  it("holds a lost archive acknowledgement unconfirmed, then settles it after reload by an exact-ID closed read", async () => {
    const s = await setup(), review = await s.preview("archive");
    s.archive.mockImplementationOnce(async (id) => { s.records.set(id, agent(id, { status: "closed", archivedAt: "now" })); throw new Error("Lost acknowledgement"); });
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "unconfirmed" });
    expect((await readdir(s.root)).some((name) => name.startsWith("intent-"))).toBe(true);
    const reloaded = createLifecycleService(s.client, s.root);
    expect(await reloaded.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("No additional") });
    expect((await reloaded.execute(review.id, "a")).outcome).toBe("archived");
    expect(s.archive).toHaveBeenCalledTimes(1); expect((await readdir(s.root)).some((name) => name.startsWith("intent-"))).toBe(false);
    expect((await readdir(s.root)).some((name) => name.startsWith("done-"))).toBe(true);
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

const sha = (value: string) => createHash("sha256").update(value).digest("hex");
const LEGACY = "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.";
const turn = { turnId: "fixture-turn", startedAt: "2026-10-22T00:00:00.000Z" } as Agent["activeTurn"];
const fencePath = (root: string, id: string) => join(root, `intent-${sha(id)}.json`);
const fenceBytes = (root: string, id: string) => nativeFs.readFile(fencePath(root, id), "utf8").catch(() => null);
const resultFile = (root: string, reviewId: string, id: string) => join(root, `result-${reviewId}-${sha(id)}.json`);
type Setup = Awaited<ReturnType<typeof setup>>;
/** Paseo 0.12: archivedAt is persisted before the runtime close, which may then fail or lose its reply. */
function archiveAs(s: Setup, patch: Partial<Agent>, failure?: string) {
  s.archive.mockImplementationOnce(async (id) => {
    s.records.set(id, { ...s.records.get(id)!, archivedAt: "2026-10-22", ...patch });
    if (failure) throw new Error(failure);
    return { archivedAt: "2026-10-22" };
  });
}
/** A fence and result row exactly as the pre-split success check left them after a resolved archive. */
async function legacyFence(s: Setup, review: LifecycleReview, patch: { message?: string; archivedAt?: string | null; fence?: Record<string, unknown> } = {}) {
  const row = review.rows[0]!;
  const saved = { ...row, outcome: "uncertain", message: patch.message ?? LEGACY, availability: { ...row.availability!, archivedAt: patch.archivedAt === undefined ? "2026-10-21" : patch.archivedAt } };
  await writeFile(resultFile(s.root, review.id, row.id), JSON.stringify(saved));
  const contents = JSON.stringify({ reviewId: review.id, agentId: row.id, action: "archive", phase: "uncertain", fingerprint: row.fingerprint, message: saved.message, ...patch.fence });
  await writeFile(fencePath(s.root, row.id), contents);
  s.records.set(row.id, { ...s.records.get(row.id)!, status: "idle", archivedAt: "2026-10-21" });
  return contents;
}

describe("Archive X membership, runtime and fences", () => {
  it("1 resolves with a closed read: archived/closed, fence released, revision fence kept", async () => {
    const s = await setup(), review = await s.preview("archive");
    expect(await fenceBytes(s.root, "a")).toBeNull();
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("runtime closed") });
    expect(await fenceBytes(s.root, "a")).toBeNull();
    expect((await readdir(s.root)).filter((name) => name.startsWith("done-"))).toHaveLength(1);
    expect(s.archive).toHaveBeenCalledExactlyOnceWith("a");
  });
  it("2 resolves with a cold idle read: archived/not-observed, released, never called closed", async () => {
    const s = await setup([agent("a", { status: "idle", lastError: undefined })]), review = await s.preview("archive");
    archiveAs(s, { status: "idle" });
    const row = await s.service.execute(review.id, "a");
    expect(row).toMatchObject({ outcome: "archived", runtime: "not-observed", nativeStatus: "idle" });
    expect(row.message).not.toMatch(/verified|shows its runtime closed/);
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it.each<[string, Partial<Agent>]>([["running only", { status: "running" }], ["an active turn only", { status: "idle", activeTurn: turn }]])("3-4 resolves but the read shows %s: uncertain, fence retained", async (_label, patch) => {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, patch);
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("does not show this session archived and inactive") });
    expect(JSON.parse((await fenceBytes(s.root, "a"))!)).toMatchObject({ reviewId: review.id, agentId: "a", phase: "uncertain" });
    expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("5-6 a known close-error text and a lost acknowledgement classify alike: archived/unconfirmed, fence retained", async () => {
    for (const failure of ["Runtime close failed: session.close() rejected", "Timeout waiting for the archive acknowledgement"]) {
      const s = await setup(), review = await s.preview("archive");
      archiveAs(s, { status: "idle" }, failure);
      const row = await s.service.execute(review.id, "a");
      expect(row).toMatchObject({ outcome: "archived", runtime: "unconfirmed", message: `Archived. The archive reply did not confirm completion (${failure}), so stopping its runtime is not confirmed.` });
      expect(unresolvedArchive(row)).toBe(true);
      expect(JSON.parse((await fenceBytes(s.root, "a"))!)).toMatchObject({ reviewId: review.id, agentId: "a", action: "archive", phase: "unconfirmed", fingerprint: row.fingerprint });
      expect((await readdir(s.root)).some((name) => name.startsWith("done-"))).toBe(true);
      expect(s.archive).toHaveBeenCalledTimes(1);
    }
  });
  it("7 rejects without archivedAt: uncertain, fence retained", async () => {
    const s = await setup(), review = await s.preview("archive"); s.archive.mockRejectedValueOnce(new Error("Lost transport"));
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "uncertain", message: "Archive not confirmed: Lost transport" });
    expect(JSON.parse((await fenceBytes(s.root, "a"))!)).toMatchObject({ phase: "uncertain" }); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it.each<[string, Partial<Agent>]>([
    ["archived and running", { status: "running" }], ["archived with an active turn", { status: "idle", activeTurn: turn }], ["running and not archived", { status: "running", archivedAt: null }],
  ])("8 rejects with a read %s: uncertain, the row stays", async (_label, patch) => {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, patch, "Lost acknowledgement");
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "uncertain", message: "Archive not confirmed: the session still reports activity." });
    expect(JSON.parse((await fenceBytes(s.root, "a"))!)).toMatchObject({ phase: "uncertain" }); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("9 preflight finds archived and inactive with no earlier fence: already_archived, no request, own lock released", async () => {
    const s = await setup([agent("a", { status: "idle", archivedAt: "2026-10-22" })]), review = await s.preview("archive");
    expect(review.rows[0]!.eligible).toBe(true);
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "already_archived", message: "Already archived on this host. No archive request was sent." });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each<[string, Partial<Agent>]>([["an active turn", { status: "idle", activeTurn: turn }], ["status running", { status: "running" }]])("10-11 preflight keeps archivedAt with %s blocked without a request", async (_label, patch) => {
    const s = await setup(), review = await s.preview("archive"); s.records.set("a", agent("a", { archivedAt: "2026-10-22", ...patch }));
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("runtime retirement is not confirmed") });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each(["intent", "uncertain", "unconfirmed"].flatMap((phase) => [[phase, null], [phase, "2026-10-22"]]))("12 an existing %s fence (archivedAt %s) stays untouched and names its owner", async (phase, archivedAt) => {
    const s = await setup(), owner = await s.preview("archive");
    const contents = JSON.stringify({ reviewId: owner.id, agentId: "a", action: "archive", phase, fingerprint: owner.rows[0]!.fingerprint });
    await writeFile(fencePath(s.root, "a"), contents);
    s.records.set("a", agent("a", { status: "idle", archivedAt }));
    const contender = await s.preview("archive");
    expect(await s.service.execute(contender.id, "a")).toMatchObject({ outcome: "blocked", reconcileReviewId: owner.id });
    expect(await fenceBytes(s.root, "a")).toBe(contents); expect(s.archive).not.toHaveBeenCalled();
  });
  it("13 an unconfirmed fence survives an idle read and is released only by a closed read", async () => {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    const held = await fenceBytes(s.root, "a"), reloaded = createLifecycleService(s.client, s.root);
    const idle = await reloaded.reconcile(review.id, "a");
    expect(idle).toMatchObject({ outcome: "archived", runtime: "unconfirmed" }); expect(unresolvedArchive(idle)).toBe(true);
    expect(await fenceBytes(s.root, "a")).toBe(held);
    s.records.set("a", { ...s.records.get("a")!, status: "closed" });
    expect(await reloaded.execute(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("No additional") });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect((await readdir(s.root)).some((name) => name.startsWith("done-"))).toBe(true);
    expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("14 releases a legacy predicate-miss fence with its exact owner provenance and no request", async () => {
    const s = await setup(), review = await s.preview("archive"); await legacyFence(s, review);
    expect(await createLifecycleService(s.client, s.root).reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "not-observed" });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each<[string, Parameters<typeof legacyFence>[2]]>([
    ["the text without post-dispatch archivedAt", { archivedAt: null }],
    ["archivedAt without the text", { message: "Archive outcome remains uncertain. The original no-replay fence is preserved." }],
    ["near-miss text", { message: LEGACY.replace("do not retry", "do not resend") }],
    ["a wrong review ID", { fence: { reviewId: "00000000-0000-4000-8000-000000000009" } }],
    ["a wrong agent ID", { fence: { agentId: "b" } }],
    ["a wrong action", { fence: { action: "resume" } }],
    ["a wrong fingerprint", { fence: { fingerprint: "other-revision" } }],
  ])("15 keeps a legacy fence with %s", async (_label, patch) => {
    const s = await setup(), review = await s.preview("archive"), contents = await legacyFence(s, review, patch);
    const result = await s.service.reconcile(review.id, "a");
    expect(result).toMatchObject({ outcome: "archived", runtime: "unconfirmed" });
    expect(await fenceBytes(s.root, "a")).toBe(contents); expect(s.archive).not.toHaveBeenCalled();
  });
  it("15 never releases a foreign owner's fence, even on a closed read", async () => {
    const s = await setup(), mine = await s.preview("archive"); await legacyFence(s, mine);
    const other = await s.preview("archive"), contents = JSON.stringify({ reviewId: other.id, agentId: "a", action: "archive", phase: "unconfirmed", fingerprint: other.rows[0]!.fingerprint });
    await writeFile(fencePath(s.root, "a"), contents); s.records.set("a", { ...s.records.get("a")!, status: "closed" });
    expect(await s.service.reconcile(mine.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("does not belong to this review") });
    expect(await fenceBytes(s.root, "a")).toBe(contents); expect(s.archive).not.toHaveBeenCalled();
  });
  it("15 an idle archived record with no fence history is not settled by reconcile", async () => {
    const s = await setup(), review = await s.preview("archive"); s.records.set("a", agent("a", { status: "idle", archivedAt: "2026-10-22" }));
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "pending" });
    expect((await s.service.read(review.id))?.rows[0]?.outcome).toBe("pending");
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).not.toHaveBeenCalled();
  });
  it.each(["archived/unconfirmed", "legacy uncertain"])("16 SA (%s) stays recoverable after SWE success, a later Resume and a remount, then settles by an exact-ID read", async (kind) => {
    const s = await setup([agent("sa"), agent("swe"), agent("b")]), sa = await s.preview("archive", ["sa"]);
    if (kind === "legacy uncertain") await legacyFence(s, sa);
    else { archiveAs(s, { status: "idle" }, "Lost acknowledgement"); expect(await s.service.execute(sa.id, "sa")).toMatchObject({ outcome: "archived", runtime: "unconfirmed" }); }
    const held = await fenceBytes(s.root, "sa"), swe = await s.preview("archive", ["swe"]);
    expect(await s.service.execute(swe.id, "swe")).toMatchObject({ outcome: "archived", runtime: "closed" });
    const calls = s.archive.mock.calls.length, resume = await s.preview("resume", ["b"]);
    expect(JSON.parse(await nativeFs.readFile(join(s.root, "latest.json"), "utf8"))).toEqual({ id: resume.id });
    const remounted = createLifecycleService(s.client, s.root);
    expect((await remounted.read())?.unresolved).toEqual([{ reviewId: sa.id, agentId: "sa" }]);
    // The owner's preview row is still pending; only its result file carries the outcome the lookup listed.
    expect(JSON.parse(await nativeFs.readFile(join(s.root, `${sa.id}.json`), "utf8")).rows[0].outcome).toBe("pending");
    expect(JSON.parse(await nativeFs.readFile(resultFile(s.root, sa.id, "sa"), "utf8"))).toMatchObject(kind === "legacy uncertain" ? { outcome: "uncertain" } : { outcome: "archived", runtime: "unconfirmed" });
    expect(await fenceBytes(s.root, "sa")).toBe(held);
    s.records.set("sa", { ...s.records.get("sa")!, status: "closed" });
    expect(await remounted.reconcile(sa.id, "sa")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "sa")).toBeNull();
    expect((await remounted.read())?.unresolved).toEqual([]);
    expect(s.archive).toHaveBeenCalledTimes(calls);
  });
});

describe("17 unresolved lookup identity", () => {
  async function base() {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement");
    const row = await s.service.execute(review.id, "a");
    return { s, review, row, lock: JSON.parse((await fenceBytes(s.root, "a"))!) as Record<string, unknown> };
  }
  type Base = Awaited<ReturnType<typeof base>>;
  const rewrite = (s: Setup, lock: Record<string, unknown>) => writeFile(fencePath(s.root, "a"), JSON.stringify(lock));
  it.each<[string, (c: Base) => Promise<unknown>]>([
    ["a file-name/agent hash mismatch", ({ s, lock }) => rewrite(s, { ...lock, agentId: "b" })],
    ["a missing owner review", ({ s, lock }) => rewrite(s, { ...lock, reviewId: randomUUID() })],
    ["an owner review whose internal ID differs", async ({ s, review, lock }) => {
      const alias = randomUUID(); await writeFile(join(s.root, `${alias}.json`), await nativeFs.readFile(join(s.root, `${review.id}.json`), "utf8")); await rewrite(s, { ...lock, reviewId: alias });
    }],
    ["an owner review for Resume", async ({ s, lock }) => rewrite(s, { ...lock, reviewId: (await s.preview("resume")).id })],
    ["no owner row for that ID", async ({ s, lock }) => rewrite(s, { ...lock, reviewId: (await s.preview("archive", ["b"])).id })],
    ["a fingerprint mismatch", ({ s, lock }) => rewrite(s, { ...lock, fingerprint: "other-revision" })],
    ["a phase outside the set", ({ s, lock }) => rewrite(s, { ...lock, phase: "bogus" })],
    ["a Resume-only phase on an archive fence", ({ s, lock }) => rewrite(s, { ...lock, phase: "submitted" })],
    ["AUD P2-01 a settled-phase fence whose owner result is unreadable", async ({ s, review, lock }) => { await rewrite(s, { ...lock, phase: "archived" }); vi.mocked(readFile).mockImplementation(async (...args) => {
      if (args[0] === resultFile(s.root, review.id, "a")) throw Object.assign(new Error("fixture read fault"), { code: "EACCES" });
      return nativeFs.readFile(...args);
    }); }],
    ["AUD P2-01 a settled-phase fence whose owner review is missing", ({ s, lock }) => rewrite(s, { ...lock, reviewId: randomUUID(), phase: "blocked" })],
    ["a non-preflight fence over a pending owner row", async ({ s, lock }) => rewrite(s, { ...lock, reviewId: (await s.preview("archive")).id, phase: "intent" })],
    ["malformed fence JSON", ({ s }) => writeFile(fencePath(s.root, "a"), "not json")],
    ["malformed owner result JSON", ({ s, review }) => writeFile(resultFile(s.root, review.id, "a"), "not json")],
    ["an owner result outside the row schema", ({ s, review }) => writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ id: "a", outcome: "archived" }))],
    ["an unreadable owner result", async ({ s, review }) => { vi.mocked(readFile).mockImplementation(async (...args) => {
      if (args[0] === resultFile(s.root, review.id, "a")) throw Object.assign(new Error("fixture read fault"), { code: "EACCES" });
      return nativeFs.readFile(...args);
    }); }],
  ])("rejects the no-ID read for %s and changes nothing", async (_label, mutate) => {
    const c = await base(); await mutate(c); const before = await fenceBytes(c.s.root, "a");
    await expect(c.s.service.read()).rejects.toThrow("Archive checks could not be fully read: 1 fence(s) could not be validated. No fence was changed.");
    expect(await fenceBytes(c.s.root, "a")).toBe(before); expect(c.s.archive).toHaveBeenCalledTimes(1);
  });
  it.each<[string, (c: Base) => Promise<unknown>]>([
    ["a Resume fence", ({ s, lock }) => rewrite(s, { ...lock, action: "resume" })],
    ["an in-flight preflight over its pending owner row", async ({ s, lock }) => rewrite(s, { ...lock, reviewId: (await s.preview("archive")).id, phase: "preflight" })],
    ["a settled-phase fence over a settled owner row", ({ s, review, row, lock }) => Promise.all([rewrite(s, { ...lock, phase: "archived" }), writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ ...row, runtime: "closed" }))])],
    ["a merged row settled closed", ({ s, review, row }) => writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ ...row, runtime: "closed" }))],
    ["a merged row settled not-observed", ({ s, review, row }) => writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ ...row, runtime: "not-observed" }))],
    ["a merged row already archived", ({ s, review, row }) => writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ ...row, outcome: "already_archived", runtime: undefined }))],
    ["a preview row saying uncertain whose result is settled", async ({ s, review, row }) => {
      const file = join(s.root, `${review.id}.json`), saved = JSON.parse(await nativeFs.readFile(file, "utf8"));
      saved.rows[0].outcome = "uncertain"; await writeFile(file, JSON.stringify(saved));
      await writeFile(resultFile(s.root, review.id, "a"), JSON.stringify({ ...row, runtime: "closed" }));
    }],
  ])("does not list %s and reports no error", async (_label, mutate) => {
    const c = await base(); await mutate(c); const before = await fenceBytes(c.s.root, "a");
    expect((await c.s.service.read())?.unresolved).toEqual([]);
    expect(await fenceBytes(c.s.root, "a")).toBe(before); expect(c.s.archive).toHaveBeenCalledTimes(1);
  });
});

describe("18 unresolved bounds and shape", () => {
  async function fences(count: number) {
    const s = await setup(), template = await s.preview("archive"), ids: string[] = [];
    for (let n = 0; n < count; n++) {
      const id = `fence-${n}`, reviewId = randomUUID(), row = { ...template.rows[0]!, id };
      await writeFile(join(s.root, `${reviewId}.json`), JSON.stringify({ ...template, id: reviewId, rows: [row] }));
      await writeFile(resultFile(s.root, reviewId, id), JSON.stringify({ ...row, outcome: "uncertain", message: "Fixture unknown outcome." }));
      await writeFile(fencePath(s.root, id), JSON.stringify({ reviewId, agentId: id, action: "archive", phase: "uncertain", fingerprint: row.fingerprint }));
      ids.push(id);
    }
    return { s, ids };
  }
  it("lists exactly 100", async () => {
    const { s, ids } = await fences(100), refs = (await s.service.read())!.unresolved!;
    expect(refs).toHaveLength(100); expect(new Set(refs.map((ref) => ref.agentId))).toEqual(new Set(ids));
  });
  it("rejects 101 explicitly instead of truncating", async () => {
    const { s } = await fences(101), failure = await s.service.read().catch((error: Error) => error.message);
    expect(failure).toBe("More than 100 archives are not confirmed (101 found). The list cannot be shown exactly; no fence was changed.");
    expect(failure).not.toContain("100+");
  });
  it("returns [] with no fences, omits the field from read(id) and never saves it", async () => {
    const s = await setup(), review = await s.preview("archive");
    expect((await s.service.read())?.unresolved).toEqual([]);
    expect(await s.service.read(review.id)).not.toHaveProperty("unresolved");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    expect((await s.service.read())?.unresolved).toEqual([{ reviewId: review.id, agentId: "a" }]);
    for (const name of (await readdir(s.root)).filter((entry) => entry.endsWith(".json"))) expect(await nativeFs.readFile(join(s.root, name), "utf8")).not.toContain("unresolved");
  });
  it("returns null only when no fence exists", async () => {
    const s = await setup(); expect(await s.service.read()).toBeNull();
    const review = await s.preview("archive"); archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    await nativeFs.unlink(join(s.root, "latest.json"));
    await expect(s.service.read()).rejects.toThrow("1 archive fence(s) are not confirmed, but no review pointer exists");
  });
});

describe("19 a listed ref grants no authority", () => {
  it("execute still blocks through the fence and reconcile re-checks ownership", async () => {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    expect((await s.service.read())?.unresolved).toEqual([{ reviewId: review.id, agentId: "a" }]);
    const contender = await s.preview("archive"), held = await fenceBytes(s.root, "a");
    expect(await s.service.execute(contender.id, "a")).toMatchObject({ outcome: "blocked", reconcileReviewId: review.id });
    expect(await fenceBytes(s.root, "a")).toBe(held);
    const moved = JSON.stringify({ ...JSON.parse(held!), reviewId: contender.id });
    await writeFile(fencePath(s.root, "a"), moved); s.records.set("a", { ...s.records.get("a")!, status: "closed" });
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ message: expect.stringContaining("does not belong to this review") });
    expect(await fenceBytes(s.root, "a")).toBe(moved); expect(s.archive).toHaveBeenCalledTimes(1);
  });
});

describe("20 reconcile saves before it releases, and releases only the fence it read", () => {
  async function unconfirmed() {
    const s = await setup(), review = await s.preview("archive");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    s.records.set("a", { ...s.records.get("a")!, status: "closed" });
    return { s, review, held: (await fenceBytes(s.root, "a"))! };
  }
  const savedRow = async (s: Setup, reviewId: string) => JSON.parse(await nativeFs.readFile(resultFile(s.root, reviewId, "a"), "utf8"));
  it("QA P2-03 a result write fault after a closed read keeps the fence, the row and the recovery ref", async () => {
    const { s, review, held } = await unconfirmed(), temp = `${resultFile(s.root, review.id, "a")}.`;
    let fault = true;
    vi.mocked(open).mockImplementation(async (...args) => {
      if (fault && String(args[0]).startsWith(temp)) { fault = false; throw Object.assign(new Error("fixture write fault"), { code: "EIO" }); }
      return nativeFs.open(...args);
    });
    await expect(s.service.reconcile(review.id, "a")).rejects.toThrow("fixture write fault");
    expect(await fenceBytes(s.root, "a")).toBe(held);
    expect(await savedRow(s, review.id)).toMatchObject({ outcome: "archived", runtime: "unconfirmed" });
    await s.preview("resume", ["b"]);
    const fresh = createLifecycleService(s.client, s.root);
    expect((await fresh.read())?.unresolved).toEqual([{ reviewId: review.id, agentId: "a" }]);
    expect(await fresh.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("an interrupted release leaves its owned fence behind a settled row; the next Check releases it without archiving", async () => {
    const { s, review, held } = await unconfirmed();
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    await writeFile(fencePath(s.root, "a"), held);
    expect((await s.service.read())?.unresolved).toEqual([]);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("a Check behind a settled row leaves an execute's in-flight preflight fence to that execute", async () => {
    const { s, review, held } = await unconfirmed();
    await s.service.reconcile(review.id, "a");
    const preflight = JSON.stringify({ ...JSON.parse(held), phase: "preflight", message: undefined });
    await writeFile(fencePath(s.root, "a"), preflight);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBe(preflight); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("AUD P1 a successor Resume fence placed during the exact-ID read survives, and the saved result is not overwritten", async () => {
    const { s, review, held } = await unconfirmed(), resume = await s.preview("resume", ["b"]);
    const successor = JSON.stringify({ reviewId: resume.id, agentId: "a", action: "resume", phase: "preflight", fingerprint: JSON.parse(held).fingerprint });
    s.refresh.mockImplementationOnce(async (id) => {
      // A concurrent Check released the fence and a successor took the session while this read was in flight.
      await nativeFs.unlink(fencePath(s.root, "a")); await nativeFs.writeFile(fencePath(s.root, "a"), successor);
      return { agent: s.records.get(id)!, project: null };
    });
    const saved = await savedRow(s, review.id);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("so this result was not recorded") });
    expect(await fenceBytes(s.root, "a")).toBe(successor);
    expect(await savedRow(s, review.id)).toEqual(saved);
    expect(s.archive).toHaveBeenCalledTimes(1); expect(s.send).not.toHaveBeenCalled();
  });
  it("AUD P1-01 service interleaving: a second Check settles and a Resume takes the session while the first Check reads", async () => {
    const s = await setup(), resume = await s.preview("resume", ["a"]), review = await s.preview("archive");
    archiveAs(s, { status: "idle" }, "Lost acknowledgement"); await s.service.execute(review.id, "a");
    s.records.set("a", { ...s.records.get("a")!, status: "closed" });
    let openFirst!: () => void, openResume!: () => void;
    const firstRead = new Promise<void>((resolve) => { openFirst = resolve; }), resumeRead = new Promise<void>((resolve) => { openResume = resolve; });
    const found = (id: string) => ({ agent: s.records.get(id)!, project: null });
    s.refresh.mockImplementationOnce(async (id) => { await firstRead; return found(id); })
      .mockImplementationOnce(async (id) => found(id))
      .mockImplementationOnce(async (id) => { await resumeRead; return found(id); });
    const before = s.refresh.mock.calls.length;
    const until = async (reads: number) => { for (let n = 0; n < 500 && s.refresh.mock.calls.length < before + reads; n++) await new Promise((resolve) => setTimeout(resolve, 1)); expect(s.refresh).toHaveBeenCalledTimes(before + reads); };
    const first = s.service.reconcile(review.id, "a"); await until(1);
    expect(await createLifecycleService(s.client, s.root).reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBeNull();
    const resuming = createLifecycleService(s.client, s.root).execute(resume.id, "a"); await until(3);
    const successor = await fenceBytes(s.root, "a");
    expect(JSON.parse(successor!)).toMatchObject({ reviewId: resume.id, agentId: "a", action: "resume", phase: "preflight" });
    openFirst();
    expect(await first).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBe(successor);
    openResume();
    expect(await resuming).toMatchObject({ outcome: "blocked", message: "Already archived on this host. No archive request was sent." });
    expect(await fenceBytes(s.root, "a")).toBeNull();
    expect(s.archive).toHaveBeenCalledTimes(1); expect(s.send).not.toHaveBeenCalled();
  });
  it("legacy provenance never releases while the exact-ID read still shows activity", async () => {
    const s = await setup(), review = await s.preview("archive"), held = await legacyFence(s, review);
    s.records.set("a", { ...s.records.get("a")!, status: "idle", archivedAt: "2026-10-21", activeTurn: turn });
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "uncertain" });
    expect(await fenceBytes(s.root, "a")).toBe(held); expect(s.archive).not.toHaveBeenCalled();
  });
  it("AUD P1 r2 a late archive ACK never overwrites a successor Resume fence taken after a Check released the intent", async () => {
    const s = await setup(), resume = await s.preview("resume", ["a"]), review = await s.preview("archive"), original = s.records.get("a")!;
    let ack!: () => void, readResume!: () => void;
    const acked = new Promise<void>((resolve) => { ack = resolve; }), resumeRead = new Promise<void>((resolve) => { readResume = resolve; });
    s.archive.mockImplementationOnce(async (id) => { s.records.set(id, { ...s.records.get(id)!, status: "closed", archivedAt: "2026-10-22" }); await acked; return { archivedAt: "2026-10-22" }; });
    const archiving = s.service.execute(review.id, "a");
    for (let n = 0; n < 500 && !s.archive.mock.calls.length; n++) await new Promise((resolve) => setTimeout(resolve, 1));
    expect(s.archive).toHaveBeenCalledTimes(1);
    expect(await createLifecycleService(s.client, s.root).reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(await fenceBytes(s.root, "a")).toBeNull();
    s.records.set("a", original);
    const reads = s.refresh.mock.calls.length;
    s.refresh.mockImplementationOnce(async (id) => { await resumeRead; return { agent: s.records.get(id)!, project: null }; });
    const resuming = createLifecycleService(s.client, s.root).execute(resume.id, "a");
    for (let n = 0; n < 500 && s.refresh.mock.calls.length === reads; n++) await new Promise((resolve) => setTimeout(resolve, 1));
    const successor = (await fenceBytes(s.root, "a"))!;
    expect(JSON.parse(successor)).toMatchObject({ reviewId: resume.id, agentId: "a", action: "resume", phase: "preflight", token: expect.any(String) });
    ack();
    expect(await archiving).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("so this result was not recorded") });
    expect(await fenceBytes(s.root, "a")).toBe(successor);
    expect(await savedRow(s, review.id)).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(s.archive).toHaveBeenCalledTimes(1); expect(s.send).not.toHaveBeenCalled();
    s.records.set("a", { ...original, status: "closed", archivedAt: "2026-10-22" }); readResume();
    expect(await resuming).toMatchObject({ outcome: "blocked" });
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.send).not.toHaveBeenCalled(); expect(s.archive).toHaveBeenCalledTimes(1);
  });
  it("an execute whose preflight fence changed never writes its intent or dispatches", async () => {
    const s = await setup(), review = await s.preview("archive"), other = JSON.stringify({ reviewId: randomUUID(), agentId: "a", action: "resume", phase: "preflight", fingerprint: "other", token: "fixture" });
    s.refresh.mockImplementationOnce(async (id) => { await nativeFs.writeFile(fencePath(s.root, "a"), other); return { agent: s.records.get(id)!, project: null }; });
    expect(await s.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("Another action took this session's fence before dispatch") });
    expect(await fenceBytes(s.root, "a")).toBe(other);
    expect(await nativeFs.readFile(resultFile(s.root, review.id, "a"), "utf8").catch(() => null)).toBeNull();
    expect(s.archive).not.toHaveBeenCalled();
  });
  it("a fence already released during the read reports its read unsaved, without an error or a release", async () => {
    const { s, review } = await unconfirmed(), saved = await savedRow(s, review.id);
    s.refresh.mockImplementationOnce(async (id) => { await nativeFs.unlink(fencePath(s.root, "a")); return { agent: s.records.get(id)!, project: null }; });
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed", message: expect.stringContaining("so this result was not recorded") });
    expect(await savedRow(s, review.id)).toEqual(saved);
    expect(await fenceBytes(s.root, "a")).toBeNull(); expect(s.archive).toHaveBeenCalledTimes(1);
  });
});

describe("21 a legacy dispatch receipt outlives a Check that replaced the saved row", () => {
  type Saved = Record<string, unknown> & { fingerprint: string; availability: Record<string, unknown> };
  const receiptPath = (root: string, fingerprint: string) => join(root, `done-${sha(`a:${fingerprint}`)}.json`);
  const UNCONFIRMED = "Archived, but an exact-ID read does not show its runtime closed, so stopping it is still not confirmed. The fence is kept; no additional archive request was sent.";
  const LATER: [string, Record<string, unknown>][] = [
    ["a pre-split Check's uncertain text", { outcome: "uncertain", message: "Archive outcome remains uncertain. The original no-replay fence is preserved." }],
    ["a later archived/unconfirmed Check", { outcome: "archived", runtime: "unconfirmed", message: UNCONFIRMED }],
  ];
  /** Every dispatch receipt by name and bytes: reconcile never writes, rewrites or migrates one. */
  const doneFiles = async (s: Setup) => Object.fromEntries(await Promise.all((await readdir(s.root)).filter((name) => name.startsWith("done-")).map(async (name) => [name, await nativeFs.readFile(join(s.root, name), "utf8")])));
  const fence = async (s: Setup, value: Record<string, unknown>) => { const bytes = JSON.stringify(value); await writeFile(fencePath(s.root, "a"), bytes); return bytes; };
  /** The legacy fence and its dispatch receipt as written, then the saved row as a later Check replaced it. */
  async function overwritten(s: Setup, review: LifecycleReview, receipt: ((row: Saved) => string) | null = JSON.stringify, later = LATER[1]![1]) {
    const held = await legacyFence(s, review), file = resultFile(s.root, review.id, "a"), saved = JSON.parse(await nativeFs.readFile(file, "utf8")) as Saved;
    if (receipt) await writeFile(receiptPath(s.root, saved.fingerprint), receipt(saved));
    await writeFile(file, JSON.stringify({ ...saved, ...later }));
    return { held, fingerprint: saved.fingerprint, row: await nativeFs.readFile(file, "utf8"), done: await doneFiles(s) };
  }
  /** Nothing was sent or created and every receipt is byte-identical; the fence is exactly the expected bytes. */
  async function kept(s: Setup, c: Awaited<ReturnType<typeof overwritten>>, bytes: string | null = c.held) {
    expect(await fenceBytes(s.root, "a")).toBe(bytes);
    expect(await doneFiles(s)).toEqual(c.done);
    expect(s.archive).not.toHaveBeenCalled(); expect(s.send).not.toHaveBeenCalled();
  }
  it.each(LATER)("settles after %s: archived/not-observed, own fence released, nothing sent", async (_label, later) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review, JSON.stringify, later);
    const remounted = createLifecycleService(s.client, s.root);
    expect((await remounted.read())?.unresolved).toEqual([{ reviewId: review.id, agentId: "a" }]);
    const row = await remounted.reconcile(review.id, "a");
    expect(row).toMatchObject({ outcome: "archived", runtime: "not-observed", nativeStatus: "idle", message: "Archived. The archive had returned; the host did not report its runtime as closed. No additional archive request was sent." });
    expect(unresolvedArchive(row)).toBe(false);
    expect(JSON.parse(await nativeFs.readFile(resultFile(s.root, review.id, "a"), "utf8"))).toEqual(row);
    expect((await remounted.read())?.unresolved).toEqual([]);
    await kept(s, c, null);
  });
  it.each(LATER)("keeps the fence after %s with no receipt: saved text and age alone never release", async (_label, later) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review, null, later);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "unconfirmed", message: UNCONFIRMED });
    await kept(s, c);
  });
  it.each<[string, (row: Saved) => string]>([
    ["a corrupt receipt", () => "not json"],
    ["a receipt outside the row schema", () => JSON.stringify({ id: "a", outcome: "uncertain", message: LEGACY })],
    ["a receipt naming another ID", (row) => JSON.stringify({ ...row, id: "b" })],
    ["a receipt naming another revision", (row) => JSON.stringify({ ...row, fingerprint: "other-revision" })],
    ["a receipt without its exact-ID read", (row) => JSON.stringify({ ...row, availability: undefined })],
    ["a receipt read for another ID", (row) => JSON.stringify({ ...row, availability: { ...row.availability, agentId: "b" } })],
    ["a receipt read without a state", (row) => JSON.stringify({ ...row, availability: { ...row.availability, state: undefined } })],
    ...(["unknown", "missing"] as const).map((state): [string, (row: Saved) => string] => [`a receipt read ${state} that still names archivedAt`, (row) => JSON.stringify({ ...row, availability: { ...row.availability, state } })]),
    ["a receipt without post-dispatch archivedAt", (row) => JSON.stringify({ ...row, availability: { ...row.availability, archivedAt: null } })],
    ["a receipt with near-miss text", (row) => JSON.stringify({ ...row, message: LEGACY.replace("do not retry", "do not resend") })],
    ["a receipt with generic uncertain text", (row) => JSON.stringify({ ...row, message: "Archive not confirmed: Lost transport" })],
    ["a receipt whose outcome is not uncertain", (row) => JSON.stringify({ ...row, outcome: "archived", runtime: "unconfirmed" })],
  ])("keeps the fence with %s", async (_label, receipt) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review, receipt);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "unconfirmed", message: UNCONFIRMED });
    await kept(s, c);
  });
  it.each(["EACCES", "EIO"])("keeps the fence when the receipt read fails with %s", async (code) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review), file = receiptPath(s.root, c.fingerprint);
    vi.mocked(readFile).mockImplementation(async (...args) => {
      if (args[0] === file) throw Object.assign(new Error("fixture read fault"), { code });
      return nativeFs.readFile(...args);
    });
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "unconfirmed", message: UNCONFIRMED });
    expect(vi.mocked(readFile).mock.calls.some(([path]) => path === file)).toBe(true);
    await kept(s, c);
  });
  it.each<[string, (s: Setup, review: LifecycleReview, fingerprint: string) => Promise<string | null>]>([
    ["another review's fence", async (s) => { const other = await s.preview("archive"); return fence(s, { reviewId: other.id, agentId: "a", action: "archive", phase: "unconfirmed", fingerprint: other.rows[0]!.fingerprint }); }],
    ["a fence for another revision", (s, review) => fence(s, { reviewId: review.id, agentId: "a", action: "archive", phase: "uncertain", fingerprint: "other-revision" })],
    ["a Resume fence", (s, review, fingerprint) => fence(s, { reviewId: review.id, agentId: "a", action: "resume", phase: "uncertain", fingerprint })],
    ...["intent", "unconfirmed"].map((phase): [string, (s: Setup, review: LifecycleReview, fingerprint: string) => Promise<string>] => [`an owned ${phase} fence`, (s, review, fingerprint) => fence(s, { reviewId: review.id, agentId: "a", action: "archive", phase, fingerprint, token: "fixture" })]),
    ["a corrupt fence", async (s) => { await writeFile(fencePath(s.root, "a"), "not json"); return "not json"; }],
    ["no fence", async (s) => { await nativeFs.unlink(fencePath(s.root, "a")); return null; }],
  ])("never settles or releases with %s, even with a legacy receipt", async (_label, swap) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review), bytes = await swap(s, review, c.fingerprint);
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "unconfirmed" });
    await kept(s, c, bytes);
  });
  it.each<[string, (s: Setup) => void]>([
    ["running again", (s) => s.records.set("a", { ...s.records.get("a")!, status: "running" })],
    ["an active turn", (s) => s.records.set("a", { ...s.records.get("a")!, activeTurn: turn })],
    ["not archived", (s) => s.records.set("a", { ...s.records.get("a")!, archivedAt: null })],
    ["no native record", (s) => { s.records.delete("a"); }],
    ["a read error", (s) => { s.refresh.mockRejectedValueOnce(new Error("Host unavailable")); }],
    ["a reply for another session", (s) => { s.refresh.mockResolvedValueOnce({ agent: agent("b", { status: "idle", archivedAt: "2026-10-21" }), project: null }); }],
  ])("keeps the fence when the exact-ID read now shows %s", async (_label, change) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review); change(s);
    const row = await s.service.reconcile(review.id, "a");
    expect(row).toMatchObject({ outcome: "uncertain", message: expect.stringContaining("Archive outcome remains uncertain") }); expect(unresolvedArchive(row)).toBe(true);
    await kept(s, c);
  });
  it.each<[string, (s: Setup) => Promise<string | null>]>([
    ["a successor takes the fence", (s) => fence(s, { reviewId: randomUUID(), agentId: "a", action: "resume", phase: "preflight", fingerprint: "other", token: "fixture" })],
    ["the fence is released", async (s) => { await nativeFs.unlink(fencePath(s.root, "a")); return null; }],
  ])("reports its read unsaved when %s during the read", async (_label, race) => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review);
    let bytes: string | null = null;
    s.refresh.mockImplementationOnce(async (id) => { bytes = await race(s); return { agent: s.records.get(id)!, project: null }; });
    expect(await s.service.reconcile(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "not-observed", message: expect.stringContaining("so this result was not recorded") });
    expect(await nativeFs.readFile(resultFile(s.root, review.id, "a"), "utf8")).toBe(c.row);
    await kept(s, c, bytes);
  });
  it("two concurrent Refreshes settle once and release only the fence they read", async () => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review);
    const rows = await Promise.all([s.service.reconcile(review.id, "a"), createLifecycleService(s.client, s.root).reconcile(review.id, "a")]);
    for (const row of rows) expect(row).toMatchObject({ outcome: "archived", runtime: "not-observed" });
    expect(JSON.parse(await nativeFs.readFile(resultFile(s.root, review.id, "a"), "utf8"))).toMatchObject({ outcome: "archived", runtime: "not-observed" });
    await kept(s, c, null);
  });
  it.each(["preflight", "uncertain"])("a pending row's %s fence never borrows an earlier dispatch's legacy receipt", async (phase) => {
    const s = await setup(), review = await s.preview("archive"), row = review.rows[0]!;
    await writeFile(receiptPath(s.root, row.fingerprint), JSON.stringify({ ...row, outcome: "uncertain", message: LEGACY, availability: { ...row.availability!, archivedAt: "2026-10-21" } }));
    const bytes = await fence(s, { reviewId: review.id, agentId: "a", action: "archive", phase, fingerprint: row.fingerprint, token: "fixture" });
    s.records.set("a", { ...s.records.get("a")!, status: "idle", archivedAt: "2026-10-21" });
    expect((await s.service.reconcile(review.id, "a")).runtime).not.toBe("not-observed");
    expect(await fenceBytes(s.root, "a")).toBe(bytes); expect(s.archive).not.toHaveBeenCalled(); expect(s.send).not.toHaveBeenCalled();
  });
  it("a failed save keeps the fence and the row; the next Refresh settles it, and a replay sends nothing", async () => {
    const s = await setup(), review = await s.preview("archive"), c = await overwritten(s, review), temp = `${resultFile(s.root, review.id, "a")}.`;
    let fault = true;
    vi.mocked(open).mockImplementation(async (...args) => {
      if (fault && String(args[0]).startsWith(temp)) { fault = false; throw Object.assign(new Error("fixture write fault"), { code: "EIO" }); }
      return nativeFs.open(...args);
    });
    await expect(s.service.reconcile(review.id, "a")).rejects.toThrow("fixture write fault");
    expect(await nativeFs.readFile(resultFile(s.root, review.id, "a"), "utf8")).toBe(c.row);
    await kept(s, c);
    const settled = await s.service.reconcile(review.id, "a");
    expect(settled).toMatchObject({ outcome: "archived", runtime: "not-observed" });
    expect(await s.service.reconcile(review.id, "a")).toEqual(settled);
    expect(await s.service.execute(review.id, "a")).toEqual(settled);
    await kept(s, c, null);
  });
});
