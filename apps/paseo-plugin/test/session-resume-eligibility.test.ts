import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contributeSessionLifecycle, createLifecycleService, lifecycleRow, type LifecycleClient } from "../server/session-lifecycle";
import { DEFAULT_RECOVERY_TARGET as target, RESUME_PROMPT } from "../shared/session-lifecycle";
import type { ProviderRefusalRead } from "../shared/provider-block";

// Deterministic interleaving points: one runs between a guarded release's compare and its delete; the other fails the
// next read of a stored refusal with an I/O error.
const fsHook = vi.hoisted(() => ({ beforeUnlink: undefined as undefined | (() => Promise<void>), failRefusalRead: false }));
vi.mock("node:fs/promises", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs/promises")>();
  const readFile = (async (...args: Parameters<typeof actual.readFile>) => {
    if (fsHook.failRefusalRead && String(args[0]).includes("refusal-")) { fsHook.failRefusalRead = false; throw Object.assign(new Error("EIO: fixture i/o error"), { code: "EIO" }); }
    return actual.readFile(...args);
  }) as typeof actual.readFile;
  return { ...actual, readFile, unlink: async (path: Parameters<typeof actual.unlink>[0]) => { const hook = fsHook.beforeUnlink; fsHook.beforeUnlink = undefined; await hook?.(); return actual.unlink(path); } };
});

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
  return { service, root, records, unreadable, hooks, send, archive, snapshot, timeline, update: (id: string, patch: Partial<Agent>) => records.set(id, { ...records.get(id)!, ...patch }),
    preview: (action: "resume" | "archive", ids = ["a"]) => service.preview({ action, scope: "filtered", agentIds: ids, target }) };
}

describe("Resume retries the same session on its own route", () => {
  it.each(Object.entries(routes))("%s route: eligible under the default target, sent once to the same ID, nothing created", async (_name, route) => {
    // Only the session's own provider is ready; the target's provider is absent from this host.
    const h = await host([agent("a", route)], [ready(route.provider)]), review = await h.preview("resume");
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
async function refusedHost(others: Agent[] = []) {
  const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA }), ...others], [ready("codex-fixture-account"), ready("claude")]);
  return { h, first: await h.service.providerRefusal("a") };
}
afterEach(() => { fsHook.beforeUnlink = undefined; fsHook.failRefusalRead = false; vi.useRealTimers(); });
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

  it("a repeated read keeps one record at the latest observation; a confirmed recovery reads once, then nothing", async () => {
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
    expect(await refusalFiles(h.root)).toEqual([]);
    h.update("a", transport);
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(true);
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

  it("a refusal written between a release's compare and its delete survives, and Resume stays closed", async () => {
    const { h, first } = await refusedHost();
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(first.refusal!.observedAt));
    let newer: Promise<ProviderRefusalRead> | undefined;
    fsHook.beforeUnlink = async () => {
      h.update("a", { status: "error", lastError: QUOTA, updatedAt: "2026-10-24T10:00:04Z" });
      newer = h.service.providerRefusal("a");
      await new Promise((resolve) => setTimeout(resolve, 50));
    };
    await h.service.providerRefusal("a");
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
    let finish!: () => void;
    h.hooks.refresh = () => new Promise((resolve) => { finish = resolve; });
    const pending = h.service.providerRefusal("a");
    await sleep(5);
    h.update("a", { lastError: "401 Unauthorized", updatedAt: "2026-10-24T10:00:04Z" });
    const newer = await h.service.providerRefusal("a");
    await sleep(5);
    finish();
    const older = await pending;
    expect(older.refusal).toMatchObject({ kind: "quota", read: "fresh" });
    expect(Date.parse(older.refusal!.observedAt)).toBeGreaterThanOrEqual(Date.parse(newer.refusal!.observedAt));
    h.update("a", transport);
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ read: "stale", kind: "auth", message: "401 Unauthorized", failureAt: "unknown", observedAt: older.refusal!.observedAt });
    expect(await refusalFiles(h.root)).toHaveLength(1);
  });

  it("an older read still in flight never restores a refusal a newer read released (AUD-R2-01b)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]);
    let finish!: () => void;
    h.hooks.refresh = () => new Promise((resolve) => { finish = resolve; });
    const pending = h.service.providerRefusal("a");
    await sleep(5);
    const newer = await h.service.providerRefusal("a");
    h.update("a", quiet);
    h.timeline.mockResolvedValue(recoveredTail(newer.refusal!.observedAt));
    expect((await h.service.providerRefusal("a")).refusal).toMatchObject({ recovery: "confirmed" });
    finish();
    expect((await pending).refusal).toMatchObject({ kind: "quota", read: "fresh" });
    expect(await refusalFiles(h.root)).toEqual([]);
    expect((await h.service.providerRefusal("a")).refusal).toBeNull();
  });

  it("an I/O error comparing with stored evidence fails the write; an older read never replaces it (AUD-R2-01c)", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], lastError: QUOTA })], [ready("codex-fixture-account")]), path = refusalFile(h.root, "a");
    let finish!: () => void;
    h.hooks.refresh = () => new Promise((resolve) => { finish = resolve; });
    const pending = h.service.providerRefusal("a");
    await sleep(5);
    h.update("a", { lastError: "401 Unauthorized", updatedAt: "2026-10-24T10:00:04Z" });
    await h.service.providerRefusal("a");
    const stored = await readFile(path, "utf8");
    fsHook.failRefusalRead = true;
    finish();
    await expect(pending).rejects.toThrow("cannot be read (EIO");
    expect(await readFile(path, "utf8")).toBe(stored);
    fsHook.failRefusalRead = true;
    const review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ eligible: false });
    expect(await readFile(path, "utf8")).toBe(stored);
    expect(h.send).not.toHaveBeenCalled();
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

  it.each([
    ["corrupt", (path: string) => writeFile(path, "{not json"), async (path: string) => (await readFile(path, "utf8")) === "{not json"],
    ["unreadable", (path: string) => mkdir(path), async (path: string) => (await stat(path)).isDirectory()],
  ] as const)("%s stored evidence closes only that session's Resume and is left unchanged", async (_name, damage, intact) => {
    const h = await host([agent("a", routes["account-suffixed"]), agent("b", routes["account-suffixed"])], [ready("codex-fixture-account")]), path = refusalFile(h.root, "a");
    await damage(path);
    await expect(h.service.providerRefusal("a")).rejects.toThrow("Resume stays closed");
    const review = await h.preview("resume", ["a", "b"]);
    expect(review.rows.map((row) => [row.id, row.eligible])).toEqual([["a", false], ["b", true]]);
    expect(review.rows[0]?.reason).toContain("cannot be read");
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked" });
    expect(h.send).not.toHaveBeenCalled();
    expect(await intact(path)).toBe(true);
    // A refusal seen now is not permission to replace evidence that cannot be read (AUD-R2-01).
    h.update("a", { lastError: QUOTA });
    await expect(h.service.providerRefusal("a")).rejects.toThrow("cannot be read");
    expect(await intact(path)).toBe(true);
  });

  it("a failed re-read keeps the refusal as unreadable history; with no observation it is unknown, not clear", async () => {
    const { h } = await refusedHost([agent("b", routes["account-suffixed"])]);
    h.unreadable.add("a"); h.unreadable.add("b");
    expect(await h.service.providerRefusal("a")).toMatchObject({ availability: { state: "unknown" }, refusal: { read: "unreadable", state: "history", recovery: "not-confirmed" } });
    expect(await h.service.providerRefusal("b")).toMatchObject({ availability: { state: "unknown" }, refusal: null });
    expect((await h.preview("resume")).rows[0]?.eligible).toBe(false);
  });

  it("no global inference: another session on the same route shows nothing and can Resume", async () => {
    const { h } = await refusedHost([agent("b", routes["account-suffixed"]), agent("c", { ...routes["account-suffixed"], status: "running", lastError: undefined })]);
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
