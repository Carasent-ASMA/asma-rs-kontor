import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contributeSessionLifecycle, createLifecycleService, lifecycleRow, type LifecycleClient } from "../server/session-lifecycle";
import { DEFAULT_RECOVERY_TARGET as target, RESUME_PROMPT } from "../shared/session-lifecycle";
import type { ProviderRefusalRead } from "../shared/provider-block";

// Deterministic faults and one interleaving point for refusal files: a fault fails the next matching reads, writes,
// renames or unlinks; `beforeRename` runs before each rename of a refusal file.
type Fault = { op: "readFile" | "writeFile" | "rename" | "unlink"; match: RegExp; code: string; times: number };
const fsHook = vi.hoisted(() => ({ faults: [] as Fault[], beforeRename: undefined as undefined | ((from: string) => Promise<void>) }));
vi.mock("node:fs/promises", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs/promises")>();
  const failing = <T,>(name: Fault["op"], call: T): T => (async (...args: unknown[]) => {
    const path = String(args[0]), fault = fsHook.faults.find((entry) => entry.op === name && entry.times > 0 && entry.match.test(path));
    if (fault) { fault.times--; throw Object.assign(new Error(`${fault.code}: fixture i/o error (${name})`), { code: fault.code }); }
    if (name === "rename" && path.includes("refusal-")) await fsHook.beforeRename?.(path);
    return (call as (...values: unknown[]) => Promise<unknown>)(...args);
  }) as T;
  return { ...actual, readFile: failing("readFile", actual.readFile), writeFile: failing("writeFile", actual.writeFile), rename: failing("rename", actual.rename), unlink: failing("unlink", actual.unlink) };
});
const fault = (op: Fault["op"], times = 1, match = /refusal-/, code = "EIO") => { fsHook.faults.push({ op, match, code, times }); };

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Entry = Awaited<ReturnType<LifecycleClient["providers"]["snapshot"]>>["entries"][number];
// Synthetic routes; the review target stays the default codex / gpt-6.1-sol / xhigh, which none of them use.
const routes = {
  "non-default": { provider: "claude", model: "claude-opus-5-5", effectiveThinkingOptionId: "high" },
  "account-suffixed": { provider: "codex-fixture-account", model: "gpt-6.1-sol", effectiveThinkingOptionId: "high" },
  "JSON": { provider: "fixture-harness", model: '["fixture-account","deepseek-flash"]', effectiveThinkingOptionId: null },
  "runtime-suffixed": { provider: "claude", model: "claude-opus-5-5", effectiveThinkingOptionId: "high", runtimeInfo: { provider: "claude", sessionId: null, model: "claude-opus-5-5[1m]" } },
} satisfies Record<string, Partial<Agent>>;
function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "codex", model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", cwd: "/fixtures/checkout", workspaceId: "workspace-a",
    title: `Fixture ${id}`, status: "error", lastError: "Turn interrupted by transport failure", createdAt: "2001-01-01", updatedAt: "2001-01-01", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "full-access", availableModes: [], pendingPermissions: [], persistence: { provider: "codex", sessionId: `history-${id}` }, labels: {},
    requiresAttention: true, attentionReason: "error", attentionTimestamp: "2001-01-01", archivedAt: null, ...patch };
}
const ready = (provider: string): Entry => ({ provider, enabled: true, status: "ready" });
const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });
type Timeline = { entries: readonly unknown[]; error?: string | null };
async function host(initial: Agent[], entries: Entry[]) {
  const root = await mkdtemp(join(tmpdir(), "kontor-resume-test-")); dirs.push(root);
  const records = new Map(initial.map((value) => [value.id, value])), unreadable = new Set<string>();
  // One deterministic interleaving point: runs after an exact read took its record, before the read returns. A hook
  // that returns a record makes the read return that instead (a record that changed while the read was slow).
  const hooks: { refresh?: () => Promise<Agent | void> } = {};
  const timeline = vi.fn(async (_options?: { direction?: "tail"; limit?: number }): Promise<Timeline> => ({ entries: [{ type: "user_message", text: "Fictional task context" }], error: null }));
  const send = vi.fn(async (_id: string, _text: string, _options?: unknown) => {}), archive = vi.fn(async (id: string) => {
    records.set(id, { ...records.get(id)!, status: "closed", archivedAt: "2026-10-24" }); return { archivedAt: "2026-10-24" };
  });
  const snapshot = vi.fn<LifecycleClient["providers"]["snapshot"]>(async () => ({ requestId: "fixture", generatedAt: "now", entries }));
  const client: LifecycleClient = { agents: {
    list: async () => ({ requestId: "fixture", entries: [...records.values()].map((value) => ({ agent: value, project: { projectKey: "fixture", projectName: "Fixture", checkout: { worktreeRoot: null, cwd: "/fixtures/checkout", isGit: false as const, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false as const, mainRepoRoot: null } } })), pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } }),
    ref: (id) => ({ refresh: async () => {
      if (unreadable.has(id)) throw new Error("fixture transport closed");
      const taken = records.get(id), hook = hooks.refresh; hooks.refresh = undefined;
      const value = (await hook?.()) ?? taken;
      return value ? { agent: value, project: null } : null;
    },
      send: (text, options) => send(id, text, options), archive: () => archive(id), timeline: { refetch: timeline } }),
  }, providers: { snapshot } };
  const service = createLifecycleService(client, root);
  return { service, client, root, records, unreadable, hooks, send, archive, snapshot, timeline, update: (id: string, patch: Partial<Agent>) => records.set(id, { ...records.get(id)!, ...patch }),
    preview: (action: "resume" | "archive", ids = ["a"]) => service.preview({ action, scope: "filtered", agentIds: ids, target }) };
}

describe("Resume retries the same session on its own route", () => {
  it.each(Object.entries(routes))("%s route: eligible under the default target, sent once to the same ID, nothing created", async (_name, route) => {
    // Only the session's own provider is ready; the target's provider is absent from this host.
    const h = await host([agent("a", route)], [ready(route.provider)]);
    await readClean(h);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ id: "a", eligible: true, provider: route.provider });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    expect(h.send).toHaveBeenCalledTimes(1);
    expect(h.send).toHaveBeenCalledWith("a", RESUME_PROMPT, expect.objectContaining({ messageId: expect.stringMatching(new RegExp(`^kontor-resume-${review.id}-`)) }));
    expect(h.archive).not.toHaveBeenCalled();
    expect([...h.records.keys()]).toEqual(["a"]);
    expect(h.records.get("a")).toMatchObject(route);
  });

  it.each([
    ["unavailable", { provider: "claude", enabled: true, status: "unavailable" }],
    ["disabled", { provider: "claude", enabled: false, status: "ready" }],
  ] as const)("checks the session's own provider, not the target's: %s blocks with nothing sent", async (_name, own) => {
    const h = await host([agent("a", routes["non-default"])], [{ ...ready(target.provider), models: [{ provider: target.provider, id: target.model, label: "Sol", thinkingOptions: [{ id: target.effort, label: "Extra high" }] }] }, own]);
    await readClean(h);
    const review = await h.preview("resume");
    expect(review.rows[0]?.eligible).toBe(true);
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("provider claude is not ready") });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("keeps every other Resume exclusion on a non-default route", () => {
    const cases: Partial<Agent>[] = [{ lastError: "5-hour usage limit reached" }, { providerUnavailable: true }, { lastError: "401 Unauthorized" },
      { status: "idle" }, { status: "running" }, { status: "closed" }, { archivedAt: "2026-10-20" }, { labels: { "takeover.successor": "next" } },
      { attentionReason: "permission" }, { currentModeId: null }, { persistence: null }, { cwd: "" }];
    for (const patch of cases) expect(lifecycleRow(agent("a", { ...routes["account-suffixed"], ...patch }), [], "resume").eligible, JSON.stringify(patch)).toBe(false);
  });
});

describe("Archive is unaffected by the Resume route change", () => {
  it("archives a session on a non-default route without consulting providers or sending", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], status: "idle" })], []), review = await h.preview("archive");
    expect(review.rows[0]).toMatchObject({ eligible: true });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(h.archive).toHaveBeenCalledTimes(1);
    expect(h.send).not.toHaveBeenCalled();
    expect(h.snapshot).not.toHaveBeenCalled();
  });
});

// Synthetic OpenCode-shaped refusal: the response header date is its only failure time; the reset (already passed) has no zone.
const QUOTA = 'APIError: {"statusCode":429,"responseHeaders":{"date":"Tue, 20 Oct 2026 21:30:52 GMT"},"responseBody":"{\\"error\\":{\\"code\\":\\"1308\\",\\"message\\":\\"5-hour usage limit reached; reset after 2026-10-21 07:45:40\\"}}"}';
const day = 86_400_000;
const refusedItem = { type: "assistant_message", text: "[System Error] You've hit your usage limit." }, retryItem = { type: "user_message", text: "Please continue." }, answeredItem = { type: "assistant_message", text: "Continued the task." };
/** A timeline entry at a host time relative to the stored observation. */
const entry = (item: object, observedAt: string, ms: number) => ({ item, timestamp: new Date(Date.parse(observedAt) + ms).toISOString() });
const recoveredTail = (observedAt: string): Timeline => ({ entries: [entry(refusedItem, observedAt, -1_000), entry(retryItem, observedAt, 1_000), entry(answeredItem, observedAt, 2_000)], error: null });
const quiet: Partial<Agent> = { status: "idle", lastError: undefined, activeTurn: null, attentionReason: null, requiresAttention: false, updatedAt: "2026-10-24T10:00:02Z" };
const transport: Partial<Agent> = { status: "error", lastError: "Turn interrupted by transport failure", activeTurn: null, updatedAt: "2026-10-24T10:00:03Z" };
const refusalFile = (root: string, id: string) => join(root, `refusal-${createHash("sha256").update(id).digest("hex")}.json`);
const refusalFiles = async (root: string) => (await readdir(root)).filter((name) => name.startsWith("refusal-"));
/** The durable refusal snapshot of one ID. */
const recordOf = async (root: string, id: string) => JSON.parse(await readFile(refusalFile(root, id), "utf8"));
const authItem = { type: "assistant_message", text: "[System Error] 401 Unauthorized" };
/** Holds the next exact read after it took its record, until `finish`; `reached` resolves once it has taken it. */
function holdNextRead(h: Awaited<ReturnType<typeof host>>) {
  let finish!: () => void, engaged!: () => void;
  const reached = new Promise<void>((resolve) => { engaged = resolve; });
  h.hooks.refresh = () => { engaged(); return new Promise((resolve) => { finish = resolve; }); };
  return { reached, finish: () => finish() };
}
/** Holds the next recovery's final exact read until `finish`, so another read can run while that release is pending. */
function holdFinalRead(h: Awaited<ReturnType<typeof host>>, tail: Timeline) {
  let finish!: () => void, engaged!: () => void;
  const reached = new Promise<void>((resolve) => { engaged = resolve; });
  h.timeline.mockImplementation(async () => { h.hooks.refresh = () => { engaged(); return new Promise((resolve) => { finish = resolve; }); }; return tail; });
  return { reached, finish: () => finish() };
}
/** The same host after a worker restart: a fresh module, so nothing this worker held in memory survives. */
async function restarted(h: Awaited<ReturnType<typeof host>>) {
  vi.resetModules();
  return (await import("../server/session-lifecycle")).createLifecycleService(h.client, h.root);
}
/** A durable clean read of each ID while it was quiet, then its fixture state again: a Resume needs a stored clean. */
async function readClean(h: Awaited<ReturnType<typeof host>>, ids = ["a"]) {
  for (const id of ids) {
    const was = h.records.get(id)!;
    h.update(id, quiet);
    expect((await h.service.providerRefusal(id)).refusal).toBeNull();
    h.records.set(id, was);
  }
}
async function refusedHost(others: Agent[] = []) {
  const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA }), ...others], [ready("codex-fixture-account"), ready("claude")]);
  return { h, first: await h.service.providerRefusal("a") };
}
afterEach(() => { fsHook.faults = []; fsHook.beforeRename = undefined; vi.useRealTimers(); });
const resumeReview = (service: ReturnType<typeof createLifecycleService>, ids = ["a"]) => service.preview({ action: "resume", scope: "filtered", agentIds: ids, target });
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

describe("Provider refusal on an exact read (R1)", () => {
  it("current: binds the header failure time, the read time and the route read; no timeline read; Resume stays closed", async () => {
    const { h, first } = await refusedHost();
    expect(first.refusal).toEqual({ kind: "quota", failureAt: "2026-10-20T21:30:52.000Z", observedAt: expect.any(String),
      route: { provider: "codex-fixture-account", model: "gpt-6.1-sol" }, message: QUOTA, read: "fresh", state: "current", recovery: "not-confirmed" });
    expect((await h.preview("resume")).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("Quota/auth") });
    expect(h.timeline).not.toHaveBeenCalled();
    expect(Date.parse(first.refusal!.observedAt)).toBeGreaterThanOrEqual(Date.parse(first.refusal!.observedAt));
  });

  it("a body reset date with empty responseHeaders is never the failure time", async () => {
    const reset = '{"responseHeaders":{},"responseBody":"{\\"error\\":{\\"code\\":\\"1308\\",\\"message\\":\\"usage limit\\",\\"reset\\":{\\"date\\":\\"Wed, 21 Oct 2026 07:45:40 GMT\\"}}}"}';
    const h = await host([agent("a", { lastError: reset })], []);
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind: "quota", failureAt: "unknown" });
  });

  it.each([["closed", { status: "closed" }], ["archived", { status: "idle", archivedAt: "2026-10-20T21:31:50Z" }]] as const)("history: a %s session's refusal, native status unchanged", async (_name, patch) => {
    const h = await host([agent("a", { lastError: QUOTA, attentionReason: null, requiresAttention: false, ...patch })], []), read = await h.service.providerRefusal("a");
    expect(read.refusal).toMatchObject({ read: "fresh", state: "history", recovery: "not-confirmed", failureAt: "2026-10-20T21:30:52.000Z" });
    expect(read.availability.nativeStatus).toBe(patch.status);
  });

  it("a repeated read keeps one record at the latest observation; a confirmed recovery reads once, is recorded, then nothing", async () => {
    const { h, first } = await refusedHost();
    await new Promise((resolve) => setTimeout(resolve, 5));
    const second = await h.service.providerRefusal("a");
    expect(second.refusal).toMatchObject({ state: "current", failureAt: first.refusal!.failureAt });
    expect(Date.parse(second.refusal!.observedAt)).toBeGreaterThan(Date.parse(first.refusal!.observedAt));
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(second.refusal!.observedAt));
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "history", recovery: "confirmed" });
    expect(h.timeline).toHaveBeenCalledWith({ direction: "tail", limit: 50 });
    expect((await h.service.providerRefusal("a")).refusal).toBeNull();
    expect(await refusalFiles(h.root)).toHaveLength(1);
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "clean", refusal: null, recovered: { kind: "quota" }, open: [], clean: true });
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(true);
  });

  it("a recovery whose record cannot be written confirms nothing: its proof is durable before anything is released", async () => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    // From the final read on, neither the record nor the hold can be written.
    h.timeline.mockImplementation(async () => { h.hooks.refresh = async () => { fault("writeFile", 4); }; return recoveredTail(first.refusal!.observedAt); });
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind: "quota", recovery: "not-confirmed" });
    h.update("a", transport);
    for (const service of [h.service, await restarted(h)]) {
      const review = await resumeReview(service);
      expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
      expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    }
    expect(h.send).not.toHaveBeenCalled();
  });

  it("absence of lastError at a retry start is not recovery; a different later failure still cannot Resume", async () => {
    const { h, first } = await refusedHost();
    h.update("a", { status: "running", lastError: undefined, activeTurn: { turnId: "retry", startedAt: null }, updatedAt: "2026-10-24T10:00:01Z" });
    h.timeline.mockResolvedValue(recoveredTail(first.refusal!.observedAt));
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ read: "stale", state: "history", recovery: "not-confirmed", observedAt: first.refusal!.observedAt });
    expect(h.timeline).not.toHaveBeenCalled();
    h.update("a", transport);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("closed after a refusal is not recovery, even with a completed turn in the tail", async () => {
    const { h, first } = await refusedHost();
    h.update("a", { ...quiet, status: "closed" });
    h.timeline.mockResolvedValue(recoveredTail(first.refusal!.observedAt));
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "history", recovery: "not-confirmed" });
    expect(h.timeline).not.toHaveBeenCalled();
    expect(await refusalFiles(h.root)).toHaveLength(1);
  });

  it.each([
    ["a passed reset time and a ready provider without a completed turn", (o: string) => ({ entries: [entry(refusedItem, o, -1_000), entry(retryItem, o, 1_000)], error: null })],
    ["an older refusal answered before the stored observation", (o: string) => ({ entries: [entry(refusedItem, o, -day), entry(retryItem, o, -day + 1_000), entry(answeredItem, o, -day + 2_000)], error: null })],
    ["an older answered refusal while the stored one is not shown", (o: string) => ({ entries: [entry(refusedItem, o, -day), entry(retryItem, o, -day + 1_000), entry(answeredItem, o, -day + 2_000), entry(retryItem, o, -1_000), entry(retryItem, o, 1_000), entry(answeredItem, o, 2_000)], error: null })],
    ["a failed timeline read (G7)", (): Timeline => { throw new Error("fixture timeline closed"); }],
    ["a timeline error (G7)", () => ({ entries: [], error: "history unavailable" })],
    ["a refusal outside the bounded tail (G7)", (o: string) => ({ entries: [entry(retryItem, o, 1_000), entry(answeredItem, o, 2_000)], error: null })],
    ["a tail without host timestamps", () => ({ entries: [{ item: refusedItem }, { item: retryItem }, { item: answeredItem }], error: null })],
  ] as const)("not recovery: %s", async (_name, reply) => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    h.timeline.mockImplementation(async () => reply(first.refusal!.observedAt));
    for (let read = 0; read < 2; read++) expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ read: "stale", state: "history", recovery: "not-confirmed" });
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
  });

  it("a refusal read that starts while a recovery is being recorded is kept after it, and Resume stays closed", async () => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(first.refusal!.observedAt));
    let newer: Promise<ProviderRefusalRead> | undefined;
    fsHook.beforeRename = async (from) => {
      if (!(await readFile(from, "utf8")).includes('"state":"recovery"')) return;
      fsHook.beforeRename = undefined;
      h.update("a", { status: "error", lastError: QUOTA, updatedAt: "2026-10-24T10:00:04Z" });
      newer = h.service.providerRefusal("a");
      await sleep(50);
    };
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    const second = await newer!;
    expect(second.refusal).toMatchObject({ state: "current" });
    h.update("a", transport);
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ read: "stale", recovery: "not-confirmed", observedAt: second.refusal!.observedAt });
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(false);
    expect(h.send).not.toHaveBeenCalled();
  });

  it.each([["", false], [" in the same millisecond", true]] as const)("overlapping reads%s: the later-started read names the refusal; the binding keeps the latest completion (AUD-01b, AUD-R2-01a)", async (_name, frozen) => {
    if (frozen) { vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(new Date("2026-10-24T10:00:00.000Z")); }
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    const slow = holdNextRead(h), pending = h.service.providerRefusal("a");
    await slow.reached;
    h.update("a", { lastError: "401 Unauthorized", updatedAt: "2026-10-24T10:00:04Z" });
    const newer = await h.service.providerRefusal("a");
    await sleep(5);
    slow.finish();
    const older = await pending;
    expect(older.refusal).toMatchObject({ kind: "quota", read: "fresh" });
    expect(Date.parse(older.refusal!.observedAt)).toBeGreaterThanOrEqual(Date.parse(newer.refusal!.observedAt));
    h.update("a", transport);
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ read: "stale", kind: "auth", message: "401 Unauthorized", failureAt: "unknown", observedAt: older.refusal!.observedAt });
    expect(await refusalFiles(h.root)).toHaveLength(1);
  });

  it("an older read still in flight never restores a refusal a newer read released (AUD-R2-01b)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    const slow = holdNextRead(h), pending = h.service.providerRefusal("a");
    await slow.reached;
    const newer = await h.service.providerRefusal("a");
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(newer.refusal!.observedAt));
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    slow.finish();
    expect((await pending).refusal).toMatchObject({ kind: "quota", read: "fresh" });
    expect(await recordOf(h.root, "a")).toMatchObject({ refusal: null, recovered: { kind: "quota" } });
    expect((await h.service.providerRefusal("a")).refusal).toBeNull();
  });

  it("an older read never renames the refusal; evidence that cannot be read fails closed and is left unchanged (AUD-R2-01c)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]), path = refusalFile(h.root, "a");
    const slow = holdNextRead(h), pending = h.service.providerRefusal("a");
    await slow.reached;
    h.update("a", { lastError: "401 Unauthorized", updatedAt: "2026-10-24T10:00:04Z" });
    await h.service.providerRefusal("a");
    slow.finish();
    expect((await pending).refusal).toMatchObject({ kind: "quota", read: "fresh" });
    const stored = await readFile(path, "utf8");
    expect(JSON.parse(stored).refusal).toMatchObject({ kind: "auth" });
    const service = await restarted(h);
    fault("readFile");
    await expect(service.providerRefusal("a")).rejects.toThrow("cannot be read (EIO");
    expect(await readFile(path, "utf8")).toBe(stored);
    fault("readFile");
    expect((await resumeReview(service)).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("cannot be read") });
    expect(await readFile(path, "utf8")).toBe(stored);
    expect(h.send).not.toHaveBeenCalled();
  });

  it("a newer read that rewrites identical bytes after a clock rollback stops an older pending release (QA F7)", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    const at = Date.parse("2026-10-24T10:00:00.000Z");
    vi.setSystemTime(at);
    const { h, first } = await refusedHost(), before = (await recordOf(h.root, "a")).refusal;
    vi.setSystemTime(at + 5_000);
    h.update("a", quiet);
    const final = holdFinalRead(h, recoveredTail(first.refusal!.observedAt)), pending = h.service.providerRefusal("a");
    await final.reached;
    vi.setSystemTime(at);
    h.update("a", { status: "error", lastError: QUOTA, updatedAt: "2026-10-24T10:00:04Z" });
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "current" });
    // Identical evidence, named by a newer read: only its sequence differs.
    const named = (await recordOf(h.root, "a")).refusal;
    expect({ ...named, seq: 0 }).toEqual({ ...before, seq: 0 });
    expect(named.seq).toBeGreaterThan(before.seq);
    final.finish();
    expect((await pending).refusal).toMatchObject({ recovery: "not-confirmed" });
    expect((await recordOf(h.root, "a")).refusal).toEqual(named);
    h.update("a", transport);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("a newer refusal whose recording fails still stops an older pending release (QA F8)", async () => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    const final = holdFinalRead(h, recoveredTail(first.refusal!.observedAt)), pending = h.service.providerRefusal("a");
    await final.reached;
    h.update("a", { status: "error", lastError: "401 Unauthorized", updatedAt: "2026-10-24T10:00:04Z" });
    h.hooks.refresh = async () => { fault("writeFile", 2); };
    await expect(h.service.providerRefusal("a")).rejects.toThrow("could not be recorded (EIO");
    final.finish();
    expect((await pending).refusal).toMatchObject({ recovery: "not-confirmed" });
    // This worker kept it, so its next record carries it.
    expect((await recordOf(h.root, "a")).refusal).toMatchObject({ kind: "auth" });
    h.update("a", transport);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("a newer unavailable refusal that neither the record nor the hold kept is kept by this worker: a readiness change and the old tail cannot clear it (AUD-R3-01)", async () => {
    const { h, first } = await refusedHost(), o = first.refusal!.observedAt;
    const tail: Timeline = { entries: [entry(refusedItem, o, -1_000), entry(retryItem, o, 1), entry(answeredItem, o, 2)], error: null };
    await sleep(10);
    h.update("a", { ...quiet, providerUnavailable: true });
    h.hooks.refresh = async () => { fault("writeFile", 2); };
    await expect(h.service.providerRefusal("a")).rejects.toThrow("could not be recorded");
    // On disk, the read's attempt is still open: a restart now could not read this as clear.
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "attempt", refusal: { kind: "quota" }, open: [{ seq: expect.any(Number) }] });
    h.update("a", { ...quiet, providerUnavailable: false });
    h.timeline.mockResolvedValue(tail);
    const later = await createLifecycleService(h.client, h.root).providerRefusal("a");
    expect(later.refusal).toMatchObject({ kind: "unavailable", recovery: "not-confirmed" });
    expect(Date.parse(later.refusal!.observedAt)).toBeGreaterThan(Date.parse(o));
    h.update("a", transport);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("a refusal whose record publication fails is kept by the hold across a restart; its temp goes then, and only recovery after it releases it (AUD-R3-01, AUD-R4-04)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { fault("rename"); };
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind: "quota", state: "current" });
    expect((await refusalFiles(h.root)).sort()).toEqual([basename(refusalFile(h.root, "a")), `${basename(refusalFile(h.root, "a"))}.held`]);
    h.update("a", transport);
    for (const service of [createLifecycleService(h.client, h.root), await restarted(h)]) {
      const review = await resumeReview(service);
      expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(quota) has no confirmed recovery") });
      expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    }
    expect(h.send).not.toHaveBeenCalled();
    const service = await restarted(h);
    h.update("a", quiet);
    const held = (await service.providerRefusal("a")).refusal;
    expect(held).toMatchObject({ kind: "quota", read: "stale", recovery: "not-confirmed" });
    h.timeline.mockResolvedValue(recoveredTail(held!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    expect(await refusalFiles(h.root)).toEqual([basename(refusalFile(h.root, "a"))]);
    h.update("a", transport);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(true);
  });

  it("F9: a refusal neither the record nor the hold kept leaves its read attempt on disk; after a restart a transport error cannot Resume, and only a later successful turn resolves it", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { fault("writeFile", 2); };
    await expect(h.service.providerRefusal("a")).rejects.toThrow("could not be recorded");
    expect(await recordOf(h.root, "a")).toMatchObject({ agentId: "a", state: "attempt", refusal: null, open: [{ seq: 1, revision: 0, start: expect.any(String) }] });
    h.update("a", transport);
    const service = await restarted(h), review = await resumeReview(service);
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(unknown) has no confirmed recovery") });
    expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(false);
    expect(h.send).not.toHaveBeenCalled();
    // The error cleared without a turn: quiet alone resolves nothing. A turn answered after the binding does.
    h.update("a", quiet);
    const held = (await service.providerRefusal("a")).refusal;
    expect(held).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    h.timeline.mockResolvedValue(recoveredTail(held!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "unknown", recovery: "confirmed" });
    h.update("a", transport);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(true);
  });

  it("F9: a complete temp that neither the record nor the hold took is read back after a restart; it goes once a record covers it, and Resume waits for recovery after it (AUD-R4-04)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { fault("rename"); fault("writeFile", 1, /\.held$/, "EACCES"); };
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind: "quota", state: "current" });
    const temps = (await refusalFiles(h.root)).filter((name) => name.endsWith(".tmp"));
    expect(temps).toHaveLength(1);
    expect(JSON.parse(await readFile(join(h.root, temps[0]!), "utf8"))).toMatchObject({ state: "refusal", refusal: { kind: "quota" } });
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "attempt", refusal: null });
    h.update("a", transport);
    const service = await restarted(h), review = await resumeReview(service);
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(quota) has no confirmed recovery") });
    expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
    expect(await refusalFiles(h.root)).toEqual([basename(refusalFile(h.root, "a"))]);
    expect((await recordOf(h.root, "a")).refusal).toMatchObject({ kind: "quota" });
    h.update("a", quiet);
    const held = (await service.providerRefusal("a")).refusal;
    h.timeline.mockResolvedValue(recoveredTail(held!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
  });

  it("a publication that fails removes none of the evidence it would have replaced (AUD-R4-04)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { fault("rename"); fault("writeFile", 1, /\.held$/, "EACCES"); };
    await h.service.providerRefusal("a");
    const temp = (await refusalFiles(h.root)).find((name) => name.endsWith(".tmp"))!;
    h.update("a", transport);
    fault("writeFile", 1_000);
    expect((await resumeReview(await restarted(h))).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("so the exact read was not made") });
    expect(await refusalFiles(h.root)).toContain(temp);
    fsHook.faults = [];
    expect((await resumeReview(await restarted(h))).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(quota) has no confirmed recovery") });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("F9 limit: when no refusal memory can be written, no Resume-path read is made and Resume stays closed, before and after a restart", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]), read = vi.fn(async () => {});
    fault("writeFile", 1_000);
    h.hooks.refresh = read;
    await expect(h.service.providerRefusal("a")).rejects.toThrow("cannot be written (EIO");
    h.update("a", transport);
    for (const service of [h.service, await restarted(h)]) {
      const review = await resumeReview(service);
      expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("so the exact read was not made") });
      expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    }
    expect(read).not.toHaveBeenCalled();
    expect(h.send).not.toHaveBeenCalled();
    expect(await refusalFiles(h.root)).toEqual([]);
  });

  it("a newer read whose identical evidence fails to record again still stops an older pending release (QA F7, F8)", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-24T10:00:00.000Z"));
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { fault("rename"); };
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "current" });
    h.update("a", quiet);
    const final = holdFinalRead(h, recoveredTail("2026-10-24T10:00:00.000Z")), pending = h.service.providerRefusal("a");
    await final.reached;
    h.update("a", { status: "error", lastError: QUOTA, updatedAt: "2026-10-24T10:00:04Z" });
    h.hooks.refresh = async () => { fault("writeFile", 2); };
    await expect(h.service.providerRefusal("a")).rejects.toThrow("could not be recorded (EIO");
    final.finish();
    expect((await pending).refusal).toMatchObject({ recovery: "not-confirmed" });
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
  });

  it("an older read whose write fails still moves this worker's binding to its completion", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    const slow = holdNextRead(h), older = h.service.providerRefusal("a");
    await slow.reached;
    const stored = (await h.service.providerRefusal("a")).refusal!.observedAt;
    await sleep(10);
    fault("writeFile", 2);
    slow.finish();
    await expect(older).rejects.toThrow("could not be recorded (EIO");
    h.update("a", quiet);
    h.timeline.mockResolvedValue({ entries: [entry(refusedItem, stored, -1_000), entry(retryItem, stored, 1), entry(answeredItem, stored, 2)], error: null });
    const read = (await h.service.providerRefusal("a")).refusal;
    expect(read).toMatchObject({ kind: "quota", recovery: "not-confirmed" });
    expect(Date.parse(read!.observedAt)).toBeGreaterThan(Date.parse(stored) + 2);
  });

  it("after a restart, an older read whose recording failed still binds: a turn before the restart confirms nothing (AUD-R4-03)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    const slow = holdNextRead(h), older = h.service.providerRefusal("a");
    await slow.reached;
    const stored = (await h.service.providerRefusal("a")).refusal!.observedAt;
    await sleep(10);
    fault("writeFile", 2);
    slow.finish();
    await expect(older).rejects.toThrow("could not be recorded");
    expect(await recordOf(h.root, "a")).toMatchObject({ refusal: { kind: "quota", observedAt: stored }, open: [{ seq: 1 }] });
    await sleep(5);
    const service = await restarted(h);
    h.update("a", quiet);
    h.timeline.mockResolvedValue({ entries: [entry(refusedItem, stored, -1_000), entry(retryItem, stored, 1), entry(answeredItem, stored, 2)], error: null });
    const read = (await service.providerRefusal("a")).refusal;
    expect(read).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    expect(Date.parse(read!.observedAt)).toBeGreaterThan(Date.parse(stored) + 2);
    h.update("a", transport);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(false);
    // A turn answered after that binding is a genuine recovery.
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(read!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it.each([
    ["quota", { status: "error", lastError: QUOTA }],
    ["unavailable", { status: "idle", lastError: undefined, providerUnavailable: true }],
  ] as const)("a stale held %s beside a newer recorded auth (a crash before the hold went) never names the kind after a restart (QA F10, AUD-R4-02)", async (kind, first) => {
    const h = await host([agent("a", { ...routes["account-suffixed"], ...first })], [ready("codex-fixture-account")]), held = `${refusalFile(h.root, "a")}.held`;
    h.hooks.refresh = async () => { fault("rename"); };
    await h.service.providerRefusal("a");
    fault("unlink", 1_000, /\.held$/);
    h.update("a", { status: "error", lastError: "401 Unauthorized", providerUnavailable: false, updatedAt: "2026-10-24T10:00:04Z" });
    await h.service.providerRefusal("a");
    expect(JSON.parse(await readFile(held, "utf8"))).toMatchObject({ refusal: { kind } });
    expect((await recordOf(h.root, "a")).refusal).toMatchObject({ kind: "auth" });
    fsHook.faults = [];
    const service = await restarted(h);
    h.update("a", quiet);
    const stored = (await service.providerRefusal("a")).refusal!;
    expect(stored).toMatchObject({ kind: "auth", recovery: "not-confirmed" });
    expect(await refusalFiles(h.root)).toEqual([basename(refusalFile(h.root, "a"))]);
    // An uncorrelated tail: neither the stale kind nor the unavailable exception applies to auth.
    h.timeline.mockResolvedValue({ entries: [entry(retryItem, stored.observedAt, 1_000), entry(answeredItem, stored.observedAt, 2_000)], error: null });
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "auth", recovery: "not-confirmed" });
    expect((await recordOf(h.root, "a")).refusal).toMatchObject({ kind: "auth" });
    h.update("a", transport);
    const review = await resumeReview(service);
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(auth) has no confirmed recovery") });
    expect(await service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
    // A genuine later auth recovery is not blocked by the stale kind.
    h.update("a", quiet);
    h.timeline.mockResolvedValue({ entries: [entry(authItem, stored.observedAt, -1_000), entry(retryItem, stored.observedAt, 1_000), entry(answeredItem, stored.observedAt, 2_000)], error: null });
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "auth", recovery: "confirmed" });
  });

  it("an older read that moves the binding later while a release is pending stops that release", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]), path = refusalFile(h.root, "a");
    const slow = holdNextRead(h), older = h.service.providerRefusal("a");
    await slow.reached;
    const stored = await h.service.providerRefusal("a");
    h.update("a", quiet);
    const final = holdFinalRead(h, recoveredTail(stored.refusal!.observedAt)), pending = h.service.providerRefusal("a");
    await final.reached;
    await sleep(5);
    slow.finish();
    expect(Date.parse((await older).refusal!.observedAt)).toBeGreaterThan(Date.parse(stored.refusal!.observedAt));
    const moved = JSON.parse(await readFile(path, "utf8")).refusal;
    final.finish();
    expect((await pending).refusal).toMatchObject({ recovery: "not-confirmed" });
    expect((await recordOf(h.root, "a")).refusal).toEqual(moved);
    expect(moved).toMatchObject({ kind: "quota", observedAt: (await older).refusal!.observedAt });
  });

  it.each([
    ["a quota refusal", { status: "error", lastError: QUOTA, updatedAt: "2026-10-24T10:00:01Z" }],
    ["provider unavailable", { ...quiet, providerUnavailable: true, updatedAt: "2026-10-24T10:00:01Z" }],
  ] as const)("a slow read of %s binds recovery after it returned: turns before the record was read prove nothing (AUD-R2-05, QA F6)", async (_name, refusal) => {
    const h = await host([agent("a", { ...routes["account-suffixed"], ...quiet })], [ready("codex-fixture-account")]);
    h.hooks.refresh = async () => { await sleep(20); h.update("a", refusal); return h.records.get("a"); };
    const first = await h.service.providerRefusal("a"), started = first.availability.checkedAt;
    expect(first.refusal).not.toBeNull();
    expect(Date.parse(first.refusal!.observedAt)).toBeGreaterThan(Date.parse(started) + 2);
    h.update("a", { ...quiet, providerUnavailable: false });
    h.timeline.mockResolvedValue({ entries: [entry(refusedItem, started, -1_000), entry(retryItem, started, 1), entry(answeredItem, started, 2)], error: null });
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "history", recovery: "not-confirmed" });
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(false);
  });

  it.each([
    ["an auth refusal", { status: "error", lastError: "401 Unauthorized" }, "auth"],
    ["provider unavailable", { providerUnavailable: true }, "unavailable"],
  ] as const)("the final exact read's %s is stored and reported; the old tail cannot clear it once that read clears (AUD-R2-02, QA F5)", async (_name, refusal, kind) => {
    const { h, first } = await refusedHost(), o = first.refusal!.observedAt;
    const tail: Timeline = { entries: [entry(refusedItem, o, -1_000), entry(retryItem, o, 1), entry(answeredItem, o, 2)], error: null };
    await sleep(10);
    h.update("a", quiet);
    h.timeline.mockImplementation(async () => { h.update("a", { ...refusal, updatedAt: "2026-10-24T10:00:05Z" }); return tail; });
    const seen = (await h.service.providerRefusal("a")).refusal;
    expect(seen).toMatchObject({ kind, state: "history", recovery: "not-confirmed" });
    expect(Date.parse(seen!.observedAt)).toBeGreaterThan(Date.parse(o));
    h.update("a", { ...quiet, providerUnavailable: false, updatedAt: "2026-10-24T10:00:06Z" });
    h.timeline.mockResolvedValue(tail);
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind, observedAt: seen!.observedAt, recovery: "not-confirmed" });
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.update("a", transport);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
  });

  it.each([
    ["starts a turn", (h: Awaited<ReturnType<typeof host>>) => h.update("a", { status: "running", activeTurn: { turnId: "next", startedAt: null }, updatedAt: "2026-10-24T10:00:05Z" })],
    ["fails again", (h: Awaited<ReturnType<typeof host>>) => h.update("a", { ...transport, updatedAt: "2026-10-24T10:00:05Z" })],
    ["loses its provider", (h: Awaited<ReturnType<typeof host>>) => h.update("a", { providerUnavailable: true, updatedAt: "2026-10-24T10:00:05Z" })],
    ["closes", (h: Awaited<ReturnType<typeof host>>) => h.update("a", { status: "closed", updatedAt: "2026-10-24T10:00:05Z" })],
    ["cannot be read again", (h: Awaited<ReturnType<typeof host>>) => h.unreadable.add("a")],
    ["goes missing", (h: Awaited<ReturnType<typeof host>>) => h.records.delete("a")],
    ["comes back as another ID", (h: Awaited<ReturnType<typeof host>>) => h.records.set("a", { ...h.records.get("a")!, id: "z" })],
  ] as const)("a session that %s during the timeline read is not recovered (AUD-02)", async (_name, move) => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    h.timeline.mockImplementation(async () => { move(h); return recoveredTail(first.refusal!.observedAt); });
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ state: "history", recovery: "not-confirmed" });
    expect(h.timeline).toHaveBeenCalledTimes(1);
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.unreadable.delete("a"); h.records.set("a", agent("a", { ...routes["account-suffixed"], ...transport }));
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(false);
  });

  it("an unavailable observation is not recovered by a readiness change and an older quota-recovery tail (AUD-05b)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: undefined, providerUnavailable: true })], [ready("codex-fixture-account")]);
    const first = await h.service.providerRefusal("a");
    expect(first.refusal).toMatchObject({ kind: "unavailable", message: null });
    h.update("a", { ...quiet, providerUnavailable: false });
    const o = first.refusal!.observedAt;
    h.timeline.mockResolvedValue({ entries: [entry(refusedItem, o, -day), entry(retryItem, o, -day + 1_000), entry(answeredItem, o, -day + 2_000)], error: null });
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ kind: "unavailable", state: "history", recovery: "not-confirmed" });
    expect(await refusalFiles(h.root)).toHaveLength(1);
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("no confirmed recovery") });
  });

  it("unreadable stored evidence closes only that session's Resume and is left unchanged", async () => {
    const h = await host([agent("a", routes["account-suffixed"]), agent("b", routes["account-suffixed"])], [ready("codex-fixture-account")]), path = refusalFile(h.root, "a");
    await readClean(h, ["b"]);
    await mkdir(path);
    await expect(h.service.providerRefusal("a")).rejects.toThrow("Resume stays closed");
    const review = await h.preview("resume", ["a", "b"]);
    expect(review.rows.map((row) => [row.id, row.eligible])).toEqual([["a", false], ["b", true]]);
    expect(review.rows[0]?.reason).toContain("cannot be read");
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
    expect((await stat(path)).isDirectory()).toBe(true);
    // A refusal seen now is not permission to replace evidence that cannot be read (AUD-R2-01).
    h.update("a", { lastError: QUOTA });
    await expect(h.service.providerRefusal("a")).rejects.toThrow("cannot be read");
    expect((await stat(path)).isDirectory()).toBe(true);
  });

  const otherSession = JSON.stringify({ agentId: "z", state: "refusal", seq: 9, open: [], clean: true, cleared: 0, recovered: null,
    refusal: { kind: "quota", failureAt: "unknown", observedAt: "2026-10-24T10:00:00.000Z", route: { provider: "codex", model: null }, message: null, seq: 9 } });
  const oldFormat = JSON.stringify({ agentId: "a", kind: "quota", failureAt: "unknown", observedAt: "2026-10-24T10:00:00.000Z", route: { provider: "codex", model: null }, message: null });
  it.each([
    ["a corrupt record", "", "{not json"],
    ["an empty held record", ".held", ""],
    ["a partial held record", ".held", '{"agentId":"a","state":"refusal","seq":'],
    ["another session's held record", ".held", otherSession],
    ["an old-format held record", ".held", oldFormat],
    ["a partial temp", ".0b6f.tmp", '{"agentId":"a"'],
  ] as const)("%s fails closed for that session only, is recorded as unresolved before it goes, and only a later successful turn resolves it", async (_name, suffix, bytes) => {
    const h = await host([agent("a", routes["account-suffixed"]), agent("b", routes["account-suffixed"])], [ready("codex-fixture-account")]);
    await readClean(h, ["b"]);
    await writeFile(`${refusalFile(h.root, "a")}${suffix}`, bytes);
    const review = await h.preview("resume", ["a", "b"]);
    expect(review.rows.map((row) => [row.id, row.eligible])).toEqual([["a", false], ["b", true]]);
    expect(review.rows[0]?.reason).toContain("(unknown) has no confirmed recovery");
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "refusal", refusal: { kind: "unknown", observedAt: expect.any(String) } });
    expect((await refusalFiles(h.root)).filter((name) => name.startsWith(basename(refusalFile(h.root, "a"))))).toEqual([basename(refusalFile(h.root, "a"))]);
    const service = await restarted(h);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(false);
    h.update("a", quiet);
    const held = (await service.providerRefusal("a")).refusal;
    expect(held).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    h.timeline.mockResolvedValue(recoveredTail(held!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    h.update("a", transport);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(true);
  });

  it("a session first read in a native error with no clean stored is unresolved: Resume stays closed, across a restart, until a later successful turn", async () => {
    const h = await host([agent("a", routes["account-suffixed"])], [ready("codex-fixture-account")]);
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(unknown) has no confirmed recovery") });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "refusal", clean: false, refusal: { kind: "unknown" } });
    const service = await restarted(h);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(false);
    // An unclassified lastError outside a native error is unresolved too.
    h.update("a", { ...quiet, lastError: "fixture stream reset" });
    const held = (await service.providerRefusal("a")).refusal;
    expect(held).toMatchObject({ kind: "unknown", state: "history", recovery: "not-confirmed" });
    // The error cleared without a turn, and a turn before the binding, prove nothing.
    h.update("a", quiet);
    h.timeline.mockResolvedValue({ entries: [entry(retryItem, held!.observedAt, -2_000), entry(answeredItem, held!.observedAt, -1_000)], error: null });
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    h.update("a", transport);
    expect((await resumeReview(service)).rows[0]?.eligible).toBe(false);
    expect(h.send).not.toHaveBeenCalled();
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(held!.observedAt));
    expect((await service.providerRefusal("a")).refusal).toMatchObject({ kind: "unknown", recovery: "confirmed" });
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "recovery", clean: true, refusal: null });
    h.update("a", transport);
    const resumed = await resumeReview(service);
    expect(resumed.rows[0]?.eligible).toBe(true);
    expect(await service.execute(resumed.id, "a")).toMatchObject({ outcome: "submitted" });
  });

  it("only a durable clean read counts: a session read quiet first can Resume a later failure; a clean that could not be written cannot", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], ...quiet }), agent("b", { ...routes["account-suffixed"], ...quiet })], [ready("codex-fixture-account")]);
    expect((await h.service.providerRefusal("a")).refusal).toBeNull();
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "clean", clean: true, refusal: null, open: [] });
    h.hooks.refresh = async () => { fault("writeFile", 2); };
    expect((await h.service.providerRefusal("b")).refusal).toMatchObject({ kind: "unknown", recovery: "not-confirmed" });
    expect(await recordOf(h.root, "b")).toMatchObject({ state: "attempt", clean: false });
    h.update("a", transport); h.update("b", transport);
    for (const service of [h.service, await restarted(h)]) {
      const review = await resumeReview(service, ["a", "b"]);
      expect(review.rows.map((row) => [row.id, row.eligible])).toEqual([["a", true], ["b", false]]);
    }
  });

  it.each([
    ["the read's outcome could not be recorded", async (h: Awaited<ReturnType<typeof host>>) => {
      h.hooks.refresh = async () => { fault("writeFile", 2); };
      await expect(h.service.providerRefusal("a")).rejects.toThrow("could not be recorded");
    }],
    ["the worker stopped during the read", async (h: Awaited<ReturnType<typeof host>>) => {
      const slow = holdNextRead(h);
      void h.service.providerRefusal("a");
      await slow.reached;
    }],
  ] as const)("a durable attempt whose refusal was not recorded (%s) keeps Resume closed after a restart, even once the error cleared without a turn", async (_name, lose) => {
    const h = await host([agent("a", routes["account-suffixed"])], [ready("codex-fixture-account")]);
    await readClean(h);
    h.update("a", { lastError: QUOTA, updatedAt: "2026-10-24T10:00:01Z" });
    await lose(h);
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "attempt", clean: true, refusal: null, open: [{ seq: expect.any(Number) }] });
    const service = await restarted(h);
    h.update("a", quiet);
    const held = (await service.providerRefusal("a")).refusal;
    expect(held).toMatchObject({ kind: "unknown", read: "stale", recovery: "not-confirmed" });
    h.update("a", transport);
    for (const next of [service, await restarted(h)]) {
      const review = await resumeReview(next);
      expect(review.rows[0]).toMatchObject({ eligible: false, reason: expect.stringContaining("(unknown) has no confirmed recovery") });
      expect(await next.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    }
    expect(h.send).not.toHaveBeenCalled();
    const last = await restarted(h);
    h.update("a", quiet);
    const bound = (await last.providerRefusal("a")).refusal!;
    h.timeline.mockResolvedValue(recoveredTail(bound.observedAt));
    expect((await last.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    h.update("a", transport);
    expect((await resumeReview(last)).rows[0]?.eligible).toBe(true);
  });

  it("every Resume-path exact read starts only after its attempt is on disk; Archive and probes write no refusal memory", async () => {
    const h = await host([agent("a", routes["account-suffixed"]), agent("b", { ...routes["account-suffixed"], status: "idle" })], [ready("codex-fixture-account")]);
    await readClean(h);
    const ref = h.client.agents.ref.bind(h.client.agents), open: number[] = [];
    h.client.agents.ref = (id) => {
      const value = ref(id);
      return { ...value, refresh: async () => { if (id === "a") open.push((await recordOf(h.root, "a")).open.length); return value.refresh(); } };
    };
    const review = await h.preview("resume");
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    await h.service.providerRefusal("a");
    expect(open).toEqual([1, 1, 1, 1]);
    expect(await recordOf(h.root, "a")).toMatchObject({ state: "clean", open: [] });
    await h.service.probe(["b"]);
    const archive = await h.preview("archive", ["b"]);
    expect(await h.service.execute(archive.id, "b")).toMatchObject({ outcome: "archived" });
    expect((await refusalFiles(h.root)).filter((name) => name.startsWith(basename(refusalFile(h.root, "b"))))).toEqual([]);
  });

  it("a failed re-read keeps the refusal as unreadable history; with no observation it is unknown, not clear", async () => {
    const { h } = await refusedHost([agent("b", routes["account-suffixed"])]);
    h.unreadable.add("a"); h.unreadable.add("b");
    expect(await h.service.providerRefusal("a")).toMatchObject({ availability: { state: "unknown" }, refusal: { kind: "quota", read: "unreadable", state: "history", recovery: "not-confirmed" } });
    expect(await h.service.providerRefusal("b")).toMatchObject({ availability: { state: "unknown" }, refusal: { kind: "unknown", read: "unreadable", state: "history", recovery: "not-confirmed" } });
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(false);
  });

  it("no global inference: another session on the same route shows nothing and can Resume", async () => {
    const { h } = await refusedHost([agent("b", routes["account-suffixed"]), agent("c", { ...routes["account-suffixed"], status: "running", lastError: undefined })]);
    await readClean(h, ["b"]);
    expect((await h.service.providerRefusal("b")).refusal).toBeNull();
    expect((await h.service.providerRefusal("c")).refusal).toBeNull();
    expect((await h.preview("resume", ["b"])).rows[0]?.eligible).toBe(true);
  });

  it("archive neither reads nor writes refusal evidence", async () => {
    const h = await host([agent("a", { lastError: QUOTA })], []), review = await h.preview("archive");
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "archived" });
    expect(await refusalFiles(h.root)).toEqual([]);
    expect(h.timeline).not.toHaveBeenCalled();
  });

  it("registers the read-only refusal RPC with the other lifecycle RPCs", () => {
    const names: string[] = [];
    contributeSessionLifecycle({ handle: (contract: { name: string }) => { names.push(contract.name); } } as unknown as PluginServerContext);
    expect(names).toEqual(expect.arrayContaining(["sessions.lifecycle-execute-item", "sessions.provider-refusal"]));
  });
});
