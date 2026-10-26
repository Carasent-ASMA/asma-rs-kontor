import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { readFile } from "node:fs/promises";
import { TAKEOVER_PREDECESSOR_LABEL, createLifecycleService, createTakeoverService, lifecycleRevision, readOperatorFleetDiscovery, readTakeoverContinuity, type TakeoverDeps } from "../server/session-lifecycle";
import * as readSources from "../server/read-sources";
import * as teamsAdapter from "../server/teams-source-adapter";
import { createSessionCreateService, personaVersion, reissuePersona } from "../server/session-create";
import { configuredAccountCatalogFromFleet, resolverInstant, routeHealthFromProviderSnapshot, type ConfiguredAccountCatalog, type ResolverRun, type RouteHealth } from "../server/session-takeover";
import { TAKEOVER_PROMPT, applyTakeover, reviewTakeover, type TakeoverReview } from "../shared/session-lifecycle";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
const KEY = "advisor/route-review";
const SUMMARY = "Summary: the parser is fixed; the migration is next.";
const hash = (value: string) => createHash("sha256").update(value).digest("hex");

function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "claude", model: "opus", effectiveThinkingOptionId: "high", cwd: "/fixtures/wks-1", workspaceId: "wks-1",
    title: "Fix the parser", status: "error", lastError: "quota", createdAt: "2026-10-25", updatedAt: "2026-10-25T08:00:00Z", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "default", availableModes: [], pendingPermissions: [], persistence: null, labels: { role: "SWE", epic: "ASMA-1", note: "not carried" },
    requiresAttention: false, attentionReason: null, attentionTimestamp: null, archivedAt: null, ...patch } as Agent;
}
const WORKSPACE = { id: "wks-1", projectId: "prj-1", projectDisplayName: "DESK • ADAM", projectRootPath: "/fixtures", projectKind: "directory", workspaceKind: "local_checkout",
  name: "TSW", title: null, workspaceDirectory: "/fixtures/wks-1", archivingAt: null, status: "done", statusEnteredAt: null, activityAt: null, scripts: [] } as PaseoWorkspace;
const SNAPSHOT = { requestId: "fixture", generatedAt: "2026-10-25T08:59:00.000Z", entries: [
  { provider: "claude-a", label: "Claude A", enabled: true, status: "ready", defaultModeId: "default",
    models: [{ provider: "claude-a", id: "claude-opus-5", label: "Opus 5", thinkingOptions: [{ id: "xhigh", label: "X-High" }] }],
    modes: [{ id: "default", label: "Default", colorTier: "safe" }] },
] };
const health = (mode: string | null): RouteHealth => ({ as_of: "2026-10-25T09:00:00Z", max_age_seconds: 60, ...(mode ? { mode } : {}),
  observations: [{ account: "claude-a", model: "claude-opus-5", effort: "xhigh", ...(mode ? { mode } : {}), state: "available", observed_at: "2026-10-25T08:59:30Z", revision: "r1" }] });
const route = { rung: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" }, step: 1, sub_step: 1, vendor: "anthropic" };

const dirs: string[] = [];
afterEach(async () => { vi.restoreAllMocks(); for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

async function host({ predecessor = agent("agent-1"), others = [] as Agent[], archiveStatus = "closed", loseCreateReply = false, choices = [{ bindingKey: KEY, chain: "claude-first" }], workspace = { ...WORKSPACE }, targetProvider = "claude-a" } = {}) {
  const tmp = await mkdtemp(join(tmpdir(), "kontor-takeover-test-")); dirs.push(tmp);
  const root = join(tmp, "lifecycle"), createRoot = join(tmp, "create");
  const agents = new Map([predecessor, ...others].map((value) => [value.id, value]));
  const timelines = new Map<string, { item: Record<string, unknown> }[]>([[predecessor.id, [{ item: { type: "assistant_message", text: SUMMARY } }]]]);
  const effects = { archive: [] as string[], create: [] as Record<string, unknown>[], send: [] as { id: string; text: string; messageId?: string }[], resolve: [] as string[][] };
  let created = 0;
  const issuedAt = resolverInstant(Date.now());
  const client = {
    workspaces: {
      list: async () => ({ requestId: "fixture", entries: [workspace], pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } }),
      ref: (id: string) => ({
        refresh: async () => id === workspace.id ? workspace : null,
        agents: { create: async (input: { idempotencyKey: string; title: string; labels: Record<string, string>; config: { provider: string; modeId?: string; thinkingOptionId?: string }; prompt?: string }) => {
          effects.create.push(input);
          const [provider, model] = input.config.provider.split("/");
          const successor = agent(`successor-${++created}`, { title: input.title, labels: input.labels, provider, model, cwd: workspace.workspaceDirectory!, workspaceId: id,
            currentModeId: input.config.modeId, effectiveThinkingOptionId: input.config.thinkingOptionId, status: "idle", lastError: undefined });
          agents.set(successor.id, successor);
          timelines.set(successor.id, input.prompt ? [{ item: { type: "user_message", text: input.prompt } }] : []);
          if (loseCreateReply) throw new Error("connection reset");
          return { id: successor.id };
        } },
      }),
      create: async () => { throw new Error("not used"); },
    },
    agents: {
      list: async (options?: { filter?: { labels?: Record<string, string>; includeArchived?: boolean } }) => ({ requestId: "fixture", pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
        entries: [...agents.values()].filter((value) => (options?.filter?.includeArchived !== false || !value.archivedAt)
          && Object.entries(options?.filter?.labels ?? {}).every(([key, label]) => value.labels?.[key] === label)).map((value) => ({ agent: value, project: null })) }),
      ref: (id: string) => ({
        refresh: async () => { const value = agents.get(id); return value ? { agent: value, project: null } : null; },
        archive: async () => { effects.archive.push(id); agents.set(id, { ...agents.get(id)!, archivedAt: "2026-10-25T09:01:00Z", status: archiveStatus as Agent["status"] }); return { archivedAt: "2026-10-25T09:01:00Z" }; },
        send: async (text: string, options?: { messageId?: string }) => { effects.send.push({ id, text, messageId: options?.messageId }); timelines.get(id)!.push({ item: { type: "user_message", text, messageId: options?.messageId } }); },
        timeline: { refetch: async () => ({ entries: timelines.get(id) ?? [] }) },
      }),
    },
    providers: { snapshot: async () => ({ ...SNAPSHOT, entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: targetProvider, models: entry.models.map((model) => ({ ...model, provider: targetProvider })) })) }) },
    config: { get: async () => ({ requestId: "fixture", config: {} }) },
  };
  const run: ResolverRun = async (_command, args) => {
    effects.resolve.push(args);
    const key = args.find((arg) => arg.startsWith("--binding-key="))!.slice("--binding-key=".length);
    const evidence = JSON.parse(args.find((arg) => arg.startsWith("--route-health="))!.slice("--route-health=".length));
    const selected = { ...route, rung: { ...route.rung, provider: targetProvider } };
    return JSON.stringify({ tool: "kontor_fleet_policy_resolve", status: 200, body: { considered: [{ route: selected }], eligibility: { excluded_vendors: [], unavailable_accounts: [] }, excluded_by_policy: [],
      provenance: { binding_key: key, chain: "claude-first", policy_hash: "a".repeat(64), schema_version: 2 }, route_health: evidence, selected, selected_validity: { observed_at: evidence.observations[0]?.observed_at ?? issuedAt, valid_until: resolverInstant(Date.parse(evidence.observations[0]?.observed_at ?? issuedAt) + evidence.max_age_seconds * 1000) } } });
  };
  const create = createSessionCreateService(client as never, createRoot, join(tmp, "create.json"));
  const observedAt = new Date().toISOString();
  const fleetCatalog: ConfiguredAccountCatalog = configuredAccountCatalogFromFleet([{ accounts: [targetProvider] }], "test-catalog-rev")!;
  const deps: TakeoverDeps = { config: { cliCommand: "/opt/kontor", stateRoot: "/srv/kontor-state", placementRoots: [] }, create,
    operatorFleetDiscovery: async () => ({ choices, catalog: fleetCatalog }),
    evidence: ({ mode }) => ({ ...health(mode), as_of: resolverInstant(Date.now()), max_age_seconds: 3600, observations: health(mode).observations.map((entry) => ({ ...entry, observed_at: issuedAt, account: targetProvider })) }), run, settleMs: 0,
    continuity: async (agentId, requestId) => ({ agentId, requestId, revision: "continuity-1", observedAt, surface: "direct", wakes: "clear", owner: null }),
  };
  const service = createTakeoverService(client as never, root, deps);
  /** A review with the offered profile chosen. */
  const ready = async (agentId = predecessor.id) => {
    const first = await service.review({ agentId, bindingKey: null });
    return first.state === "choose" ? service.review({ agentId, reviewId: first.reviewId, revision: first.revision, bindingKey: KEY }) : first;
  };
  const fence = () => readdir(root).then((names) => names.filter((name) => name.startsWith("intent-")));
  return { service, deps, agents, timelines, effects, root, createRoot, client, workspace, ready, fence, apply: (review: TakeoverReview) => service.apply({ reviewId: review.reviewId, revision: review.revision, digest: review.digest! }) };
}

describe("direct takeover continuity", () => {
  it("issues a bounded direct receipt with wakes unknown when schedules are unreadable", async () => {
    const h = await host({ predecessor: agent("agent-1", { provider: "claude-a", status: "error", lastError: "429 quota" }) });
    const requestId = "11111111-1111-4111-8111-111111111111";
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
    expect(receipt).toMatchObject({ agentId: "agent-1", requestId, surface: "direct", wakes: "unknown", owner: null });
    const evidence = JSON.parse(await readFile(join(h.root, `takeover-evidence-${hash(`agent-1:${requestId}`)}.json`), "utf8"));
    expect(evidence.sessionRefusal?.kind).toBe("quota");
    const blocked = await host();
    blocked.deps.continuity = (agentId, id) => readTakeoverContinuity(blocked.client as never, blocked.root, agentId, id);
    expect((await blocked.ready()).state).toBe("unknown");
  });

  it("QA1-PR515-P2-01 production reader review then apply survives +1s observation refresh", async () => {
    vi.useFakeTimers({ now: Date.parse("2026-10-25T09:00:00.000Z") });
    try {
      const h = await host();
      (h.client as { schedules?: { forAgent: () => Promise<{ entries: unknown[] }> } }).schedules = { forAgent: async () => ({ entries: [] }) };
      h.deps.continuity = (agentId, id) => readTakeoverContinuity(h.client as never, h.root, agentId, id);
      const review = await h.ready();
      expect(review.state).toBe("ready");
      vi.advanceTimersByTime(1_100);
      const applied = await h.apply(review);
      expect(applied.state).toBe("succeeded");
      expect(h.effects.archive).toEqual(["agent-1"]);
      expect(h.effects.create).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("refuses apply when continuity semantics drift between review and apply", async () => {
    vi.useFakeTimers({ now: Date.parse("2026-10-25T09:00:00.000Z") });
    try {
      const h = await host();
      let scheduleEntries: unknown[] = [];
      (h.client as { schedules?: { forAgent: () => Promise<{ entries: unknown[] }> } }).schedules = { forAgent: async () => ({ entries: scheduleEntries }) };
      h.deps.continuity = (agentId, id) => readTakeoverContinuity(h.client as never, h.root, agentId, id);
      const review = await h.ready();
      expect(review.state).toBe("ready");
      scheduleEntries = [{ id: "hb-1" }];
      const applied = await h.apply(review);
      expect(applied.state).toBe("refused");
      expect(h.effects.archive).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("refuses apply when continuity observation is stale at apply", async () => {
    vi.useFakeTimers({ now: Date.parse("2026-10-25T09:00:00.000Z") });
    try {
      const h = await host();
      (h.client as { schedules?: { forAgent: () => Promise<{ entries: unknown[] }> } }).schedules = { forAgent: async () => ({ entries: [] }) };
      let continuityReads = 0;
      h.deps.continuity = async (agentId, id) => {
        const receipt = await readTakeoverContinuity(h.client as never, h.root, agentId, id);
        if (!receipt) return null;
        continuityReads++;
        if (continuityReads > 1) return { ...receipt, observedAt: "2026-10-25T08:58:00.000Z" };
        return receipt;
      };
      const review = await h.ready();
      expect(review.state).toBe("ready");
      vi.advanceTimersByTime(1_100);
      const applied = await h.apply(review);
      expect(applied.state).toBe("refused");
      expect(h.effects.archive).toHaveLength(0);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("takeover review", () => {
  it("accepts only the closed review and confirmation inputs, never caller-built evidence or commands", () => {
    expect(reviewTakeover.input.safeParse({ agentId: "agent-1", bindingKey: null }).success).toBe(true);
    expect(reviewTakeover.input.safeParse({ agentId: "agent-1", bindingKey: null, revision: 1 }).success).toBe(false);
    for (const extra of [{ evidence: health("default") }, { command: "/arbitrary" }, { cwd: "/another" }, { selected: route }, { owner: "another" }])
      expect(reviewTakeover.input.safeParse({ agentId: "agent-1", bindingKey: KEY, ...extra }).success).toBe(false);
    const confirmation = { reviewId: "00000000-0000-4000-8000-000000000000", revision: 1, digest: "a".repeat(64) };
    expect(applyTakeover.input.safeParse(confirmation).success).toBe(true);
    expect(applyTakeover.input.safeParse({ ...confirmation, requestId: confirmation.reviewId }).success).toBe(false);
    expect(applyTakeover.input.safeParse({ ...confirmation, digest: "guessed" }).success).toBe(false);
  });
  it("calls operator fleet discovery once per takeover assess", async () => {
    let reads = 0;
    const h = await host();
    const discovery = h.deps.operatorFleetDiscovery;
    h.deps.operatorFleetDiscovery = async () => { reads++; return discovery(); };
    await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(reads).toBe(1);
  });

  it("readOperatorFleetDiscovery uses one Teams adapter read for profiles and account catalog", async () => {
    const read = vi.fn(async () => ({
      status: "ready" as const, sha256: "fleet-sha", readAt: "2026-10-25T09:00:00.000Z",
      data: { kind: "fleet" as const, bindings: [{ chainResolved: true, reference: KEY, chainName: "claude-first" }], domains: [{ accounts: ["claude-a"] }] },
      diagnostics: [] as const,
    }));
    const catalogSpy = vi.spyOn(readSources, "sourceCatalog").mockReturnValue({ sources: [{ id: "operator", title: "operator", kind: "teams", path: "/fixture/operator-fleet.yml", workstream: null, epic: null, projectId: null, revisionKind: "working copy" }], diagnostics: [] });
    const adapterSpy = vi.spyOn(teamsAdapter, "createTeamsSourceAdapter").mockReturnValue({ read } as never);
    try {
      const result = await readOperatorFleetDiscovery();
      expect(read).toHaveBeenCalledTimes(1);
      expect(read).toHaveBeenCalledWith({ sourceId: "operator" });
      expect(result.choices).toEqual([{ bindingKey: KEY, chain: "claude-first" }]);
      expect(result.catalog.revision).toBe("fleet-sha");
    } finally {
      catalogSpy.mockRestore();
      adapterSpy.mockRestore();
    }
  });

  it("offers the server's profiles, never infers a chain, and keeps one successor request across revisions", async () => {
    const h = await host();
    // Role, epic and title are on the session; none of them chooses a chain.
    const first = await h.service.review({ agentId: "agent-1", bindingKey: null });
    expect(first).toMatchObject({ revision: 1, state: "choose", digest: null, successor: null, ownership: "ordinary",
      association: { bindingKey: null, source: null, choices: [{ bindingKey: KEY, chain: "claude-first" }] } });
    expect(h.effects.resolve).toHaveLength(0);
    const unoffered = await h.service.review({ agentId: "agent-1", reviewId: first.reviewId, revision: 1, bindingKey: "advisor/elsewhere" });
    expect(unoffered).toMatchObject({ revision: 2, state: "refused", digest: null });
    expect(h.effects.resolve).toHaveLength(0);
    const chosen = await h.service.review({ agentId: "agent-1", reviewId: first.reviewId, revision: 2, bindingKey: KEY });
    expect(chosen).toMatchObject({ revision: 3, state: "ready", association: { bindingKey: KEY, source: "chosen" },
      route: { bindingKey: KEY, chain: "claude-first", selected: { account: "claude-a", model: "claude-opus-5", effort: "xhigh" } },
      successor: { title: "Fix the parser", workspaceId: "wks-1", cwd: "/fixtures/wks-1", provider: "claude-a", model: "claude-opus-5", effort: "xhigh", mode: "default", persona: null } });
    expect(chosen.digest).toMatch(/^[a-f0-9]{64}$/);
    // Refreshing keeps the choice and the one request; the digest is reissued.
    const refreshed = await h.service.review({ agentId: "agent-1", reviewId: first.reviewId, revision: 3, bindingKey: null });
    expect(refreshed).toMatchObject({ revision: 4, state: "ready", successor: { requestId: chosen.successor!.requestId } });
    expect(refreshed.digest).not.toBe(chosen.digest);
    await expect(h.service.review({ agentId: "agent-1", reviewId: first.reviewId, revision: 3, bindingKey: null })).rejects.toThrow("not the current review");
  });

  it("refuses governed, unverifiable and unready sessions with one short reason, before any resolver read", async () => {
    const cases: [Partial<Agent>, string][] = [
      [{ labels: { "kontor.seat_binding_id": "seat-1" } }, "Kontor-governed seat"],
      [{ labels: { orchestration: "paseo-direct" } }, "names no owning session"],
      [{ archivedAt: "2026-10-25T08:30:00Z", status: "closed" }, "Already archived"],
      [{ pendingPermissions: [{ id: "p1" }] as Agent["pendingPermissions"], status: "idle" }, "Waiting for permission"],
      [{ status: "running" }, "Running or starting"],
      [{ cwd: "/fixtures/wks-1/sub" }, "outside its workspace directory"],
      [{ labels: { orchestration: "paseo-direct", "paseo.parent-agent-id": "gone" } }, "owning session is not active"],
      [{ labels: { "create.request": "0f1e2d3c-4b5a-4968-8776-655443322110" } }, "creation record"],
    ];
    for (const [patch, reason] of cases) {
      const h = await host({ predecessor: agent("agent-1", patch) });
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state, reason).toBe("refused");
      expect(review.message, reason).toContain(reason);
      expect(review.digest).toBeNull();
      expect(h.effects.resolve, reason).toHaveLength(0);
    }
    const child = await host({ others: [agent("child-1", { status: "idle", labels: { "paseo.parent-agent-id": "agent-1" } })] });
    expect((await child.service.review({ agentId: "agent-1", bindingKey: KEY })).message).toContain("active child sessions");
    // A governed seat with its labels removed still sits in a Kontor placement.
    const stripped = await host();
    stripped.deps.config = { ...stripped.deps.config, placementRoots: ["/fixtures"] };
    expect((await stripped.service.review({ agentId: "agent-1", bindingKey: KEY })).message).toContain("Kontor placement");
    const none = await host({ choices: [] });
    expect(await none.service.review({ agentId: "agent-1", bindingKey: null })).toMatchObject({ state: "refused", message: expect.stringContaining("No configured profile") });
  });

  it("states a block, never a route, when no positive route evidence exists", async () => {
    const h = await host();
    h.deps.evidence = ({ mode }) => ({ ...health(mode), observations: [] });
    h.deps.run = async (_command, args) => JSON.stringify({ tool: "kontor_fleet_policy_resolve", status: 409, body: { code: "placement_blocked", message: "no route", selection: {
      considered: [{ route, excluded: "route_unobserved" }], provenance: { binding_key: KEY, chain: "claude-first", policy_hash: "a".repeat(64), schema_version: 2 },
      route_health: JSON.parse(args.at(-1)!.slice("--route-health=".length)), selected: null } } });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review).toMatchObject({ state: "blocked", digest: null, successor: null, route: { selected: null, considered: [{ excluded: "route_unobserved" }] } });
    expect(review.limits.join(" ")).toContain("No host source of positive route health");
  });

  it("blocks the configured chain when the predecessor's current quota lastError matches the snapshot account", async () => {
    const h = await host({ predecessor: agent("agent-1", { provider: "claude-a", status: "error", lastError: "429 usage limit exceeded" }) });
    let seenCwd = "";
    const catalog = configuredAccountCatalogFromFleet([{ accounts: ["claude-a"] }], "test-catalog-rev")!;
    h.deps.evidence = ({ mode, cwd, catalog: cat }) => {
      seenCwd = cwd;
      const asOf = Date.now();
      return routeHealthFromProviderSnapshot({ ...SNAPSHOT, generatedAt: new Date(asOf).toISOString() }, mode, cat ?? catalog, asOf);
    };
    h.deps.run = async (_command, args) => JSON.stringify({ tool: "kontor_fleet_policy_resolve", status: 409, body: { code: "placement_blocked", message: "no route", selection: {
      considered: [{ route, excluded: "route_unavailable_now" }], provenance: { binding_key: KEY, chain: "claude-first", policy_hash: "a".repeat(64), schema_version: 2 },
      route_health: JSON.parse(args.at(-1)!.slice("--route-health=".length)), selected: null } } });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(seenCwd).toBe("/fixtures/wks-1");
    expect(review.state).toBe("blocked");
    expect(review.route?.considered?.[0]?.excluded).toBe("route_unavailable_now");
  });

  it("reads positive route evidence from the host provider snapshot at the session checkout", async () => {
    const h = await host();
    let seenCwd = "";
    h.deps.evidence = ({ mode, cwd, catalog }) => {
      seenCwd = cwd;
      const asOf = Date.now();
      return routeHealthFromProviderSnapshot({ ...SNAPSHOT, generatedAt: new Date(asOf).toISOString() }, mode, catalog, asOf);
    };
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(seenCwd).toBe("/fixtures/wks-1");
    expect(review.limits.join(" ")).not.toContain("No host source of positive route health");
    expect(review.state).toBe("ready");
    expect(h.effects.resolve[0]?.find((arg) => arg.startsWith("--route-health="))).toContain("\"state\":\"available\"");
  });

  it("apply succeeds when only the provider snapshot requestId changes but semantic evidence is unchanged", async () => {
    const h = await host();
    const asOf = Date.parse("2026-10-25T09:00:00.000Z");
    vi.setSystemTime(asOf);
    h.deps.continuity = async (agentId, requestId) => ({ agentId, requestId, revision: "continuity-1", observedAt: new Date(asOf).toISOString(), surface: "direct", wakes: "clear", owner: null });
    let requestId = "transport-req-1";
    h.deps.evidence = ({ mode, cwd, catalog }) => routeHealthFromProviderSnapshot({ ...SNAPSHOT, requestId, generatedAt: "2026-10-25T08:59:00.000Z", entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: "claude-a" })) }, mode, catalog, asOf);
    const review = await h.ready();
    expect(review.state).toBe("ready");
    requestId = "transport-req-2";
    vi.setSystemTime(asOf + 1000);
    const result = await h.apply(review);
    expect(result.state).toBe("succeeded");
    expect([h.effects.archive.length, h.effects.create.length]).toEqual([1, 1]);
    vi.useRealTimers();
  });

  it("apply revalidates the same quota refusal when only the observation clock advances within freshness", async () => {
    const h = await host({ predecessor: agent("agent-1", { provider: "claude-a", model: "claude-opus-5", status: "error", lastError: "429 usage limit exceeded" }) });
    const asOf = Date.parse("2026-10-25T09:00:00.000Z");
    vi.setSystemTime(asOf);
    h.deps.evidence = ({ mode, catalog }) => routeHealthFromProviderSnapshot({ ...SNAPSHOT, requestId: "req-stable", generatedAt: "2026-10-25T08:59:00.000Z" }, mode, catalog, asOf);
    h.deps.run = async (_command, args) => JSON.stringify({ tool: "kontor_fleet_policy_resolve", status: 409, body: { code: "placement_blocked", message: "no route", selection: {
      considered: [{ route, excluded: "route_unavailable_now" }], provenance: { binding_key: KEY, chain: "claude-first", policy_hash: "a".repeat(64), schema_version: 2 },
      route_health: JSON.parse(args.at(-1)!.slice("--route-health=".length)), selected: null } } });
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("blocked");
    vi.setSystemTime(asOf + 1000);
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("blocked");
    vi.useRealTimers();
  });

  it("refuses to retire without fresh, exact owner and wake proof", async () => {
    const h = await host(), good = h.deps.continuity!;
    for (const patch of [null, { agentId: "another" }, { requestId: "00000000-0000-4000-8000-000000000000" }, { observedAt: "2000-01-01T00:00:00.000Z" }, { surface: "unknown" }, { wakes: "unknown" }, { owner: { agentId: "owner", revision: "r1", bindingRevision: "b1" } }]) {
      h.deps.continuity = async (...args) => patch ? { ...(await good(...args))!, ...patch } as never : null;
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(["unknown", "refused"]).toContain(review.state);
      expect(review.digest).toBeNull();
    }
    delete h.deps.continuity;
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "unknown", digest: null, message: expect.stringContaining("No qualified owner and wake-continuity receipt") });
    expect([h.effects.archive.length, h.effects.create.length]).toEqual([0, 0]);
  });

  it("refuses a label-stripped governed session even outside the known placement roots", async () => {
    const h = await host(), good = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await good(...args))!, surface: "governed" });
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", digest: null, message: expect.stringContaining("governed lifecycle writer") });
    expect([h.effects.archive.length, h.effects.create.length]).toEqual([0, 0]);
  });

  it("requires an orchestrated owner's receipt and binding revision, and preserves a native-only supervisor", async () => {
    const owner = agent("owner-1", { status: "idle", title: "Own the lifecycle" });
    const h = await host({ predecessor: agent("agent-1", { labels: { orchestration: "paseo-direct" }, parentAgentId: "owner-1" } as Partial<Agent>), others: [owner] });
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", message: expect.stringContaining("owner and binding revision") });
    const good = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await good(...args))!, owner: { agentId: owner.id, revision: lifecycleRevision(owner), bindingRevision: "owner-binding-1" } });
    const ready = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(ready.state).toBe("ready");
    expect(await h.apply(ready)).toMatchObject({ state: "succeeded" });
    expect(h.agents.get("successor-1")!.labels).toMatchObject({ orchestration: "paseo-direct", "paseo.parent-agent-id": owner.id });
  });

  it("serializes revisions of one review across service instances", async () => {
    const h = await host(), review = await h.ready();
    const second = createTakeoverService(h.client as never, h.root, h.deps);
    const input = { agentId: "agent-1", reviewId: review.reviewId, revision: review.revision, bindingKey: KEY };
    const answers = await Promise.allSettled([h.service.review(input), second.review(input)]);
    expect(answers.map((answer) => answer.status)).toEqual(["fulfilled", "rejected"]);
    expect(h.effects.archive).toEqual([]);
  });

  it("does not accept stale history or an unavailable creation preflight as retirement evidence", async () => {
    const stale = await host(), ref = stale.client.agents.ref;
    stale.client.agents.ref = (id) => ({ ...ref(id), timeline: { refetch: async () => ({ entries: stale.timelines.get(id) ?? [], error: "unavailable" }) } });
    expect(await stale.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "unknown", digest: null, message: expect.stringContaining("summary") });
    const off = await host();
    off.deps.create = { ...off.deps.create, preflight: async () => ({ problem: "Creation is off." }) };
    expect(await off.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", digest: null, message: "Creation is off." });
    expect([...stale.effects.archive, ...off.effects.archive]).toEqual([]);
  });

  it("keeps an unreadable permission posture and a wrong workspace identity unconfirmable", async () => {
    const missing = await host({ predecessor: agent("agent-1", { currentModeId: undefined }) });
    expect(await missing.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "unknown", digest: null, message: expect.stringContaining("permission mode") });
    const wrong = await host(), ref = wrong.client.workspaces.ref;
    wrong.client.workspaces.ref = (id) => ({ ...ref(id), refresh: async () => ({ ...WORKSPACE, id: "another-workspace" }) });
    expect(await wrong.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", digest: null, message: expect.stringContaining("exact workspace") });
    expect([...missing.effects.archive, ...wrong.effects.archive]).toEqual([]);
  });
});

describe("takeover apply", () => {
  it("archives once, creates the one reviewed request, verifies and names the successor, and answers a repeat from its record", async () => {
    const h = await host();
    const review = await h.ready();
    expect(review).toMatchObject({ state: "ready", digest: expect.any(String), successor: { requestId: expect.any(String) } });
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "succeeded", phase: "done", next: "none", successor: { requestId: review.successor!.requestId, agentId: "successor-1", title: "Fix the parser" } });
    expect(result.message).toContain("successor-1");
    expect(h.effects.archive).toEqual(["agent-1"]);
    expect(h.effects.create).toHaveLength(1);
    expect(h.effects.create[0]).toMatchObject({ idempotencyKey: review.successor!.requestId, title: "Fix the parser",
      config: { provider: "claude-a/claude-opus-5", modeId: "default", thinkingOptionId: "xhigh" },
      labels: { role: "SWE", epic: "ASMA-1", "create.request": review.successor!.requestId, [TAKEOVER_PREDECESSOR_LABEL]: "agent-1" } });
    expect(h.effects.create[0]!.labels).not.toHaveProperty("note");
    expect(h.effects.create[0]!.config).not.toHaveProperty("systemPrompt");
    expect(String(h.effects.create[0]!.prompt)).toContain(SUMMARY);
    expect(h.effects.send).toEqual([{ id: "successor-1", text: TAKEOVER_PROMPT, messageId: `kontor-takeover-${review.reviewId}` }]);
    expect(await h.fence()).toEqual([]);
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
    // The successor's chain is recorded for its exact ID.
    expect((await h.service.review({ agentId: "successor-1", bindingKey: null })).association).toMatchObject({ bindingKey: KEY, source: "recorded" });
  });

  it("refuses a wrong revision or digest without spending the review, and refuses drift after the review", async () => {
    const h = await host();
    const review = await h.ready();
    const visible = hash(JSON.stringify({ ...review, digest: null }));
    for (const input of [{ ...review, digest: visible }, { ...review, digest: "0".repeat(64) }, { ...review, revision: review.revision - 1 }])
      expect(await h.apply(input)).toMatchObject({ state: "refused", next: "review" });
    expect(await h.fence()).toEqual([]);
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, updatedAt: "2026-10-25T09:05:00Z" });
    const drift = await h.apply(review);
    expect(drift).toMatchObject({ state: "refused", phase: "preflight", next: "review", successor: null });
    expect(drift.message).toContain("changed since review");
    expect([h.effects.archive.length, h.effects.create.length]).toEqual([0, 0]);
    expect(await h.apply(review)).toEqual(drift);
    expect(await h.fence()).toEqual([]);
  });

  it("pins the owner binding, evidence identity, policy, deadline, summary and title rather than only the chosen route", async () => {
    for (const field of ["binding", "evidence", "policy", "deadline", "summary", "title"]) {
      const h = await host(), review = await h.ready();
      if (field === "binding") {
        const prior = h.deps.continuity!;
        h.deps.continuity = async (...args) => ({ ...(await prior(...args))!, revision: "continuity-2" });
      }
      if (field === "evidence") {
        const prior = h.deps.evidence;
        h.deps.evidence = (input) => Promise.resolve(prior(input)).then((resolved) => ({ ...resolved, observations: resolved.observations.map((entry) => ({ ...entry, state: "unknown" as const })) }));
      }
      if (field === "deadline") {
        const prior = h.deps.evidence!;
        h.deps.evidence = (input) => Promise.resolve(prior(input)).then((resolved) => ({ ...resolved, max_age_seconds: resolved.max_age_seconds + 60 }));
      }
      if (field === "policy") {
        const prior = h.deps.run!;
        h.deps.run = async (...args) => (await prior(...args)).replace("a".repeat(64), "b".repeat(64));
      }
      if (field === "summary") h.timelines.get("agent-1")!.push({ item: { type: "assistant_message", text: "Different handoff." } });
      if (field === "title") h.agents.set("agent-1", { ...h.agents.get("agent-1")!, title: "Another task" });
      expect(await h.apply(review), field).toMatchObject({ state: "refused", next: "review" });
      expect([h.effects.archive.length, h.effects.create.length], field).toEqual([0, 0]);
    }
  });

  it("pins the complete verified creation record and refuses missing or mismatched provenance", async () => {
    const requestId = "0f1e2d3c-4b5a-4968-8776-655443322110";
    const predecessor = agent("agent-1", { labels: { "create.request": requestId } });
    const h = await host({ predecessor });
    const record = { request: { requestId, title: predecessor.title!, cwd: predecessor.cwd, workspaceId: predecessor.workspaceId!, labels: predecessor.labels!, provider: predecessor.provider, model: predecessor.model!, effort: "high", modeId: "default", at: "2026-10-25T08:00:00Z" },
      outcome: { requestId, state: "verified" as const, title: predecessor.title!, agentId: predecessor.id, workspaceId: predecessor.workspaceId!, message: "Created and verified", differences: [], persona: "supported" as const, checkedAt: "2026-10-25T08:00:00Z" } };
    h.deps.create = { ...h.deps.create, record: async () => record };
    const review = await h.ready();
    expect(review).toMatchObject({ state: "ready", ownership: "created" });
    record.outcome.message = "Record changed";
    expect(await h.apply(review)).toMatchObject({ state: "refused", next: "review" });
    record.request.cwd = "/another-checkout";
    expect(await h.service.review({ agentId: predecessor.id, bindingKey: KEY })).toMatchObject({ state: "refused", message: expect.stringContaining("no longer matches") });
    expect(h.effects.archive).toEqual([]);
  });

  it("catches a child attached during the creation preflight before retirement", async () => {
    const h = await host(), preflight = h.deps.create.preflight;
    h.deps.create = { ...h.deps.create, preflight: async (...args) => {
      const answer = await preflight(...args);
      h.agents.set("child", agent("child", { labels: { "paseo.parent-agent-id": "agent-1" } }));
      return answer;
    } };
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", message: expect.stringContaining("child appeared") });
    expect(h.effects.archive).toEqual([]);
  });

  it("refuses a forged confirmation even after the valid operation has finished", async () => {
    const h = await host(), review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(await h.apply({ ...review, digest: "0".repeat(64) })).toMatchObject({ state: "refused" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("does not release a successor whose exact route, supervisor, persona, checkout or summary differs", async () => {
    for (const difference of ["route", "supervisor", "persona", "checkout", "summary"]) {
      const h = await host(), ref = h.client.agents.ref;
      let reads = 0;
      h.client.agents.ref = (id) => {
        const handle = ref(id);
        return { ...handle, refresh: async () => {
          const found = await handle.refresh();
          if (id === "successor-1" && found && ++reads > 1) {
            if (difference === "route") found.agent.model = "another-model";
            if (difference === "supervisor") found.agent.labels = { ...found.agent.labels, "paseo.parent-agent-id": "another-owner" };
            if (difference === "persona") found.agent.labels = { ...found.agent.labels, persona: "another-persona" };
            if (difference === "checkout") found.agent.cwd = "/another-checkout";
            if (difference === "summary") h.timelines.set(id, []);
          }
          return found;
        } };
      };
      expect(await h.apply(await h.ready()), difference).toMatchObject({ state: "partial", phase: "verify", next: "none" });
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length], difference).toEqual([1, 1, 0]);
    }
  });

  it("shares the per-session fence with Archive and Resume", async () => {
    const h = await host();
    const review = await h.ready();
    await writeFile(join(h.root, `intent-${hash("agent-1")}.json`), JSON.stringify({ reviewId: "00000000-0000-4000-8000-000000000000", agentId: "agent-1", action: "archive", phase: "intent", fingerprint: "x" }));
    expect(await h.apply(review)).toMatchObject({ state: "refused", message: expect.stringContaining("Another action holds this session") });
    expect(h.effects.archive).toEqual([]);
  });

  it("calls an archive without a closed runtime partial, creates nothing, and settles it only by reads", async () => {
    const h = await host({ archiveStatus: "idle" });
    const review = await h.ready();
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "partial", phase: "archive", next: "check", successor: null });
    expect(h.effects.create).toEqual([]);
    // The takeover fence stays, and Archive's own recovery list reads past it.
    expect(await h.fence()).toHaveLength(1);
    expect(await createLifecycleService(h.client as never, h.root).read()).toBeNull();
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, status: "closed" });
    expect(await h.service.check(review.reviewId)).toMatchObject({ state: "partial", phase: "archive", next: "none", message: expect.stringContaining("no successor was created") });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("reconciles a lost creation reply on the same request and never creates or releases again", async () => {
    const h = await host({ loseCreateReply: true });
    const review = await h.ready();
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "uncertain", phase: "create", next: "check" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", phase: "verify", next: "none", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(checked.message).toContain("its task was not released");
    expect(await h.service.check(review.reviewId)).toEqual(checked);
    expect(await h.apply(review)).toEqual(checked);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 0]);
  });

  it("reconciles a lost task-release reply by the same message ID and rechecks the successor", async () => {
    for (const drift of [false, true]) {
      const h = await host(), ref = h.client.agents.ref;
      h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, send: async (...args) => { await handle.send(...args); throw new Error("lost reply"); } }; };
      const review = await h.ready();
      expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
      h.agents.set("successor-1", { ...h.agents.get("successor-1")!, status: "running" });
      if (drift) h.agents.set("successor-1", { ...h.agents.get("successor-1")!, model: "another-model" });
      expect(await h.service.check(review.reviewId)).toMatchObject({ state: drift ? "uncertain" : "succeeded" });
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
    }
  });
});

describe("PR506 review rework", () => {
  it.each(["preflight", "retirement", "creation intent"])("AUD-8641-CP2-01 refuses checkout drift at %s with no successor dispatch", async (stage) => {
    const h = await host(), review = await h.ready();
    expect(review.state).toBe("ready");
    if (stage === "preflight") {
      const preflight = h.deps.create.preflight;
      h.deps.create = { ...h.deps.create, preflight: async (...args) => {
        const result = await preflight(...args);
        h.workspace.workspaceDirectory = "/fixtures/changed-checkout";
        return result;
      } };
    } else if (stage === "retirement") {
      const ref = h.client.agents.ref;
      h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, archive: async () => {
        const result = await handle.archive();
        h.workspace.workspaceDirectory = "/fixtures/changed-checkout";
        return result;
      } }; };
    } else {
      // The workspace moves while the durable creation intent is written, after validation read A.
      const intent = join(h.createRoot, `${review.successor!.requestId}.json`);
      Object.defineProperty(h.workspace, "workspaceDirectory", { get: () => existsSync(intent) ? "/fixtures/changed-checkout" : WORKSPACE.workspaceDirectory });
    }
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: stage === "preflight" ? "refused" : "partial", phase: stage === "preflight" ? "preflight" : "create" });
    expect(result.message).toContain("expected checkout");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([stage === "preflight" ? 0 : 1, 0, 0]);
    expect(await h.apply(review)).toEqual(result);
  });

  it.each(["claude-a", "claude"])("AUD-8641-CP2-02 requires qualified role-card delivery for a created lead on %s", async (targetProvider) => {
    const requestId = "0f1e2d3c-4b5a-4968-8776-655443322110", title = "LSA • ASMA-100 • Route review";
    const predecessor = agent("agent-1", { title, labels: { role: "LSA", epic: "ASMA-100", "title.auto": title, persona: personaVersion("lsa-bound", true), "create.request": requestId } });
    const h = await host({ predecessor, workspace: { ...WORKSPACE, name: "LSA" }, targetProvider });
    h.deps.create = { ...h.deps.create, record: async () => ({
      request: { requestId, title, cwd: predecessor.cwd, workspaceId: predecessor.workspaceId!, labels: predecessor.labels!, provider: predecessor.provider, model: predecessor.model!, effort: "high", modeId: "default", at: "2026-10-25T08:00:00Z" },
      outcome: { requestId, state: "verified", title, agentId: predecessor.id, workspaceId: predecessor.workspaceId!, message: "Verified", differences: [], persona: "supported", checkedAt: "2026-10-25T08:00:00Z" },
    }) };
    const review = await h.ready();
    if (targetProvider === "claude-a") {
      expect(review).toMatchObject({ state: "refused", digest: null, message: expect.stringContaining("Instruction delivery and acceptance are not qualified") });
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
    } else {
      expect(review.state).toBe("ready");
      expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
      expect(h.effects.create[0]!.config).toMatchObject({ systemPrompt: expect.stringContaining("Scope: epic ASMA-100.") });
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
    }
  });

  it.each([
    { entry: 0, wait: 59_000, state: "succeeded" },
    { entry: 0, wait: 61_001, state: "refused" },
    { entry: 61_001, wait: 0, state: "refused" },
  ])("QA1-PR506-P2-01 checks continuity at retirement (entry=$entry, preflight=$wait)", async ({ entry, wait, state }) => {
    const h = await host(), issued = Date.now();
    let now = issued;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const receipt = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await receipt(...args))!, observedAt: new Date(issued).toISOString() });
    const review = await h.ready();
    expect(review.state).toBe("ready");
    now += entry;
    const preflight = h.deps.create.preflight;
    h.deps.create = { ...h.deps.create, preflight: async (...args) => { const result = await preflight(...args); now += wait; return result; } };
    const result = await h.apply(review);
    expect(result).toMatchObject({ state });
    if (state === "refused") expect(result.message).toContain("stale");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual(state === "succeeded" ? [1, 1, 1] : [0, 0, 0]);
    expect(await h.apply(review)).toEqual(result);
  });

  it.each(["native read", "workspace read"])("QA1-PR506-P2-01 refuses expiry during the final %s before archive", async (stage) => {
    const h = await host(), issued = Date.now();
    let now = issued, nativeReads = 0;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const receipt = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await receipt(...args))!, observedAt: new Date(issued).toISOString() });
    const review = await h.ready(), ref = h.client.agents.ref, workspace = h.client.workspaces.ref;
    h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, refresh: async () => {
      const found = await handle.refresh();
      if (id === "agent-1" && ++nativeReads === 3 && stage === "native read") now += 61_001;
      return found;
    } }; };
    h.client.workspaces.ref = (id) => ({ ...workspace(id), refresh: async () => {
      const found = await workspace(id).refresh();
      if (nativeReads >= 3 && stage === "workspace read") now += 61_001;
      return found;
    } });
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "refused", message: expect.stringContaining("stale") });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("QA1-PR506-P2-01 preserves lost-effect reconciliation after continuity expires", async () => {
    const h = await host({ loseCreateReply: true }), issued = Date.now();
    let now = issued;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "create", next: "check" });
    now += 61_001;
    const result = await h.service.check(review.reviewId);
    expect(result).toMatchObject({ state: "partial", phase: "verify", next: "none", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 0]);
  });
});

describe("LSA-8641-CP2-01 sealed effect validity", () => {
  it.each([
    { stage: "preflight", wait: -1, counts: [0, 0, 0], state: "refused" },
    { stage: "preflight", wait: 500, counts: [1, 1, 1], state: "succeeded" },
    { stage: "preflight", wait: 1000, counts: [1, 1, 1], state: "succeeded" },
    { stage: "preflight", wait: 1001, counts: [0, 0, 0], state: "refused" },
    { stage: "preflight", wait: 2001, counts: [0, 0, 0], state: "refused" },
    { stage: "archive workspace", wait: 2001, counts: [0, 0, 0], state: "refused" },
    { stage: "retirement", wait: 2001, counts: [1, 0, 0], state: "partial" },
    { stage: "native create workspace", wait: 2001, counts: [1, 0, 0], state: "partial" },
    { stage: "release readback", wait: 2001, counts: [1, 1, 0], state: "partial" },
  ])("checks the selected deadline after $wait ms in $stage", async ({ stage, wait, counts, state }) => {
    const issued = Math.floor(Date.now() / 1000) * 1000;
    let now = issued, nativeReads = 0, createWorkspaceReads = 0;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const h = await host(), receipt = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await receipt(...args))!, observedAt: new Date(issued).toISOString() });
    h.deps.evidence = ({ mode }) => ({ ...health(mode), as_of: resolverInstant(now), max_age_seconds: 1,
      observations: health(mode).observations.map((entry) => ({ ...entry, observed_at: resolverInstant(issued) })) });
    const run = h.deps.run!;
    h.deps.run = async (...args) => {
      const evidence = JSON.parse(args[1].find((arg) => arg.startsWith("--route-health="))!.slice("--route-health=".length));
      expect(Date.parse(evidence.as_of) - Date.parse(evidence.observations[0].observed_at)).toBeLessThanOrEqual(1000);
      return run(...args);
    };
    const review = await h.ready();
    expect(review.state).toBe("ready");
    const preflight = h.deps.create.preflight, ref = h.client.agents.ref, workspace = h.client.workspaces.ref;
    h.deps.create = { ...h.deps.create, preflight: async (...args) => {
      const result = await preflight(...args);
      if (stage === "preflight") now += wait;
      return result;
    } };
    h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle,
      refresh: async () => { const found = await handle.refresh(); if (id === "agent-1") nativeReads++; return found; },
      archive: async () => { const result = await handle.archive(); if (stage === "retirement") now += wait; return result; },
      timeline: { refetch: async () => { const page = await handle.timeline.refetch(); if (id === "successor-1" && stage === "release readback") now += wait; return page; } },
    }; };
    h.client.workspaces.ref = (id) => ({ ...workspace(id), refresh: async () => {
      const found = await workspace(id).refresh();
      if (stage === "archive workspace" && nativeReads === 3 && !h.effects.archive.length) now += wait;
      if (h.effects.archive.length && ++createWorkspaceReads === 2 && stage === "native create workspace") now += wait;
      return found;
    } });
    const result = await h.apply(review), resolves = h.effects.resolve.length;
    expect(result).toMatchObject({ state });
    if (state !== "succeeded") expect(result.message).toContain("health evidence expired");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual(counts);
    if (stage === "retirement") expect(await h.deps.create.record(review.successor!.requestId)).toBeNull();
    expect(await h.apply(review)).toEqual(result);
    if (result.next === "none") expect(await h.service.check(review.reviewId)).toEqual(result);
    expect(h.effects.resolve).toHaveLength(resolves);
    expect(new Set(h.effects.create.map((input) => input.idempotencyKey)).size).toBeLessThanOrEqual(1);
  });

  it.each(["retirement", "native create workspace", "release readback"])("keeps the continuity bound at %s while route health remains fresh", async (stage) => {
    const issued = Date.now();
    let now = issued, workspaceReads = 0;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const h = await host(), receipt = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await receipt(...args))!, observedAt: new Date(issued).toISOString() });
    const review = await h.ready(), ref = h.client.agents.ref, workspace = h.client.workspaces.ref;
    h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle,
      archive: async () => { const result = await handle.archive(); if (stage === "retirement") now += 61_001; return result; },
      timeline: { refetch: async () => { const result = await handle.timeline.refetch(); if (id === "successor-1" && stage === "release readback") now += 61_001; return result; } },
    }; };
    h.client.workspaces.ref = (id) => ({ ...workspace(id), refresh: async () => {
      const result = await workspace(id).refresh();
      if (h.effects.archive.length && ++workspaceReads === 2 && stage === "native create workspace") now += 61_001;
      return result;
    } });
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "partial", message: expect.stringContaining("continuity receipt is stale") });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, stage === "release readback" ? 1 : 0, 0]);
    expect(await h.apply(review)).toEqual(result);
  });

  it.each(["create", "release"])("reconciles a lost %s reply after route expiry without another effect", async (stage) => {
    const issued = Math.floor(Date.now() / 1000) * 1000;
    let now = issued;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const h = await host({ loseCreateReply: stage === "create" }), receipt = h.deps.continuity!;
    h.deps.continuity = async (...args) => ({ ...(await receipt(...args))!, observedAt: new Date(issued).toISOString() });
    h.deps.evidence = ({ mode }) => ({ ...health(mode), as_of: resolverInstant(now), max_age_seconds: 1,
      observations: health(mode).observations.map((entry) => ({ ...entry, observed_at: resolverInstant(issued) })) });
    if (stage === "release") {
      const ref = h.client.agents.ref;
      h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, send: async (...args) => { await handle.send(...args); throw new Error("lost reply"); } }; };
    }
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: stage, next: "check" });
    now += 2001;
    const result = await h.service.check(review.reviewId);
    expect(result).toMatchObject({ state: stage === "create" ? "partial" : "succeeded", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, stage === "create" ? 0 : 1]);
    expect(h.effects.resolve).toHaveLength(2);
  });
});

describe("persona issued again for a successor", () => {
  it("rebuilds a New-session lead's persona from its created title and refuses anything it cannot rebuild unchanged", () => {
    const labels = { persona: personaVersion("lsa-bound", true), "title.auto": "LSA • ASMA-100 • Route review" };
    const place = { desk: "DESK • ADAM", workspace: "LSA", workspaceId: "wks-lsa" };
    const issued = reissuePersona(labels, "claude-a", place);
    expect(issued).toMatchObject({ supported: false, version: personaVersion("lsa-bound", false) });
    expect("text" in issued && issued.text).toContain("Scope: epic ASMA-100.");
    expect(reissuePersona(labels, "claude", place)).toMatchObject({ supported: true, version: personaVersion("lsa-bound", true) });
    for (const [patch, where] of [[{ persona: "kontor-lsa-bound@000000000000" }, place], [{}, { ...place, workspace: "TPM" }], [{ "title.auto": "LSA • ADHOC • Route review" }, place]] as const)
      expect(reissuePersona({ ...labels, ...patch }, "claude", where)).toHaveProperty("refused");
  });
});
