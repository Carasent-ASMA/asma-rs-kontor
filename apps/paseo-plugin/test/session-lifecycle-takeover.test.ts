import { randomUUID } from "node:crypto";
import { execFile as nativeExecFile } from "node:child_process";
import { promisify as nativePromisify } from "node:util";
import { mkdir as makeDirectory } from "node:fs/promises";
import type { PaseoApi } from "@getpaseo/client";
import { createProceduralTakeover, takeoverCheckout, takeoverHash } from "../server/procedural-takeover";
import { takeoverModeSchema, type ProceduralAdmission } from "../shared/procedural-takeover";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdtemp, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { readFile } from "node:fs/promises";
import { TAKEOVER_PREDECESSOR_LABEL, createLifecycleService, createTakeoverService, lifecycleRevision, readOperatorFleetDiscovery, readTakeoverContinuity, type TakeoverDeps } from "../server/session-lifecycle";
import { discoverCanonical } from "../server/session-canonical";
import { promotionRecordSchema } from "../shared/session-promotion";
import * as readSources from "../server/read-sources";
import * as teamsAdapter from "../server/teams-source-adapter";
import { createStateRoot, createSessionCreateService, personaVersion, reissuePersona } from "../server/session-create";
import { createSessionSummaryService } from "../server/session-summaries";
import { stockSummaryGenerator } from "../server/stock-summary-generator";
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
const FEATURES: NonNullable<Agent["features"]> = [
  { type: "toggle", id: "plan", label: "Plan", value: true },
  { type: "select", id: "speed", label: "Speed", value: "fast", options: [
    { id: "fast", label: "Fast", isDefault: true }, { id: "slow", label: "Slow", isDefault: false },
  ] },
];
const withPlan = (value: boolean): NonNullable<Agent["features"]> =>
  FEATURES.map((feature) => feature.id === "plan" ? { ...feature, value } : feature) as NonNullable<Agent["features"]>;
const health = (mode: string | null): RouteHealth => ({ as_of: "2026-10-25T09:00:00Z", max_age_seconds: 60, ...(mode ? { mode } : {}),
  observations: [{ account: "claude-a", model: "claude-opus-5", effort: "xhigh", ...(mode ? { mode } : {}), state: "available", observed_at: "2026-10-25T08:59:30Z", revision: "r1" }] });
const route = { rung: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" }, step: 1, sub_step: 1, vendor: "anthropic" };

const dirs: string[] = [];
afterEach(async () => { vi.unstubAllEnvs(); vi.restoreAllMocks(); for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

async function host({ predecessor = agent("agent-1"), others = [] as Agent[], archiveStatus = "closed", loseCreateReply = false, hideRelease = false, failFirstSend = false, choices = [{ bindingKey: KEY, chain: "claude-first" }], workspace = { ...WORKSPACE }, targetProvider = "claude-a", features = [] as NonNullable<Agent["features"]> } = {}) {
  const tmp = await mkdtemp(join(tmpdir(), "kontor-takeover-test-")); dirs.push(tmp);
  const root = join(tmp, "lifecycle"), createRoot = join(tmp, "create");
  const agents = new Map([predecessor, ...others].map((value) => [value.id, value]));
  const timelines = new Map<string, { item: Record<string, unknown> }[]>([[predecessor.id, [{ item: { type: "assistant_message", text: SUMMARY } }]]]);
  const effects = { archive: [] as string[], create: [] as Record<string, unknown>[], send: [] as { id: string; text: string; messageId?: string }[], resolve: [] as string[][], detach: [] as string[] };
  let created = 0, releaseVisible = !hideRelease, sendCalls = 0;
  const issuedAt = resolverInstant(Date.now());
  const client = {
    workspaces: {
      list: async () => ({ requestId: "fixture", entries: [workspace], pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } }),
      ref: (id: string) => ({
        refresh: async () => id === workspace.id ? workspace : null,
        agents: { create: async (input: { idempotencyKey: string; title: string; labels: Record<string, string>; config: { provider: string; modeId?: string; thinkingOptionId?: string; featureValues?: Record<string, string | boolean> }; prompt?: string }) => {
          effects.create.push(input);
          const [provider, model] = input.config.provider.split("/");
          const successor = agent(`successor-${++created}`, { title: input.title, labels: input.labels, provider, model, cwd: workspace.workspaceDirectory!, workspaceId: id,
            currentModeId: input.config.modeId, effectiveThinkingOptionId: input.config.thinkingOptionId, status: "idle", lastError: undefined,
            features: features.map((feature) => ({ ...feature, value: input.config.featureValues?.[feature.id] ?? feature.value })) as NonNullable<Agent["features"]> });
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
        send: async (text: string, options?: { messageId?: string }) => { if (failFirstSend && ++sendCalls === 1) throw new Error("process died before dispatch"); effects.send.push({ id, text, messageId: options?.messageId }); if (releaseVisible) timelines.get(id)!.push({ item: { type: "user_message", text, messageId: options?.messageId } }); },
        detach: async () => { effects.detach.push(id); const value = agents.get(id); if (value?.labels) { const { ["paseo.parent-agent-id"]: _parent, ...rest } = value.labels; value.labels = rest; } },
        timeline: { refetch: async () => ({ entries: timelines.get(id) ?? [] }) },
      }),
    },
    providers: {
      snapshot: async () => ({ ...SNAPSHOT, entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: targetProvider, models: entry.models.map((model) => ({ ...model, provider: targetProvider })) })) }),
      listFeatures: async () => ({ features, error: null }),
    },
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
  const summaryRoot = join(tmp, "summaries");
  const summaries = createSessionSummaryService(client as never, summaryRoot, {
    generate: async (agentId: string) => {
      const page = await client.agents.ref(agentId).timeline.refetch() as { entries?: { item: { type: string; text?: string } }[]; error?: string | null };
      if (page.error) throw new Error("The source history could not be captured consistently.");
      const text = [...(page.entries ?? [])].reverse().find((entry) => entry.item.type === "assistant_message" && entry.item.text?.trim())?.item.text?.trim() ?? SUMMARY;
      return { sections: { task: text, decisions: "Keep the verified decisions.", currentState: "The task is at its captured checkpoint.", nextStep: "Continue the captured next action." }, cutoff: "2026-10-25T08:00:00Z" };
    },
  });
  const create = createSessionCreateService(client as never, createRoot, join(tmp, "create.json"), undefined, undefined, summaryRoot);
  const observedAt = new Date().toISOString();
  const fleetCatalog: ConfiguredAccountCatalog = configuredAccountCatalogFromFleet([{ accounts: [targetProvider] }], "test-catalog-rev")!;
  const deps: TakeoverDeps = { config: { cliCommand: "/opt/kontor", stateRoot: "/srv/kontor-state", placementRoots: [] }, create, promotionRoot: createRoot,
    summaries, snapshot: async () => ({ ...SNAPSHOT, requestId: "snapshot-1", generatedAt: new Date().toISOString(), entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: targetProvider, models: entry.models.map((model) => ({ ...model, provider: targetProvider })) })) }),
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
  const showRelease = () => { releaseVisible = true; const last = effects.send.at(-1); if (last) timelines.get(last.id)!.push({ item: { type: "user_message", text: last.text, messageId: last.messageId } }); };
  return { service, deps, agents, timelines, effects, root, createRoot, client, workspace, ready, fence, showRelease, apply: (review: TakeoverReview) => service.apply({ reviewId: review.reviewId, revision: review.revision, digest: review.digest! }) };
}


  it("moves a promoted canonical binding to the verified takeover successor", async () => {
    const title = "LSA • ADHOC • Route review";
    const predecessor = agent("agent-1", { title, status: "error", labels: { role: "LSA", "title.auto": title, persona: personaVersion("lsa-ad-hoc", true), note: "not carried" } });
    const h = await host({ predecessor, workspace: { ...WORKSPACE, name: "ADHOC", title: null }, targetProvider: "claude" });
    await (async () => {
      const { mkdir } = await import("node:fs/promises");
      await mkdir(join(h.createRoot, "promotion-v1"), { recursive: true });
      const native = { workspaceId: "wks-1", workspace: "ADHOC", title, titleAuto: title, observedAt: "2026-10-25T08:00:00Z", revision: "r1" };
      const record = promotionRecordSchema.parse({ schemaVersion: 1, requestId: "11111111-1111-4111-8111-111111111111", agentId: predecessor.id,
        epic: "ASMA-1", branch: "promote", state: "promoted", native,
        logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "a".repeat(64) },
        persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
        summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted",
        createdAt: "2026-10-25T08:00:00Z", updatedAt: "2026-10-25T08:00:00Z" });
      await writeFile(join(h.createRoot, "promotion-v1", `${record.requestId}.json`), JSON.stringify(record));
      for (const [index, source] of ["agent-b", "agent-c"].entries()) {
        const handoff = promotionRecordSchema.parse({
          schemaVersion: 1, requestId: `2222222${index}-2222-4222-8222-22222222222${index}`, agentId: source, epic: "ASMA-1", branch: "handoff", state: "handoff",
          native, logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "b".repeat(64) },
          persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
          summarySha256: "b".repeat(64), summaryRevision: "r1", acknowledgement: "verified", message: "handoff",
          createdAt: "2026-10-25T08:01:00Z", updatedAt: `2026-10-25T08:0${index + 1}:00Z`,
        });
        await writeFile(join(h.createRoot, "promotion-v1", `${handoff.requestId}.json`), JSON.stringify(handoff));
      }
    })();
    const review = await h.ready();
    expect(review.state).toBe("ready");
    const result = await h.apply(review);
    expect(result.state).toBe("succeeded");
    const successor = [...h.agents.values()].find((entry) => entry.id !== predecessor.id)!;
    expect(await discoverCanonical(h.client as never, h.createRoot, "ASMA-1")).toMatchObject({ kind: "one", agent: { id: successor.id } });
    expect(successor.labels?.role).toBe("LSA");
    expect(successor.labels?.epic).toBe("ASMA-1");
    expect(h.effects.create[0]?.prompt).toContain("Kontor recovered canonical epic binding: ASMA-1");
  });



  it("blocks takeover when a later handoff history names a different epic", async () => {
    const title = "LSA • ASMA-1 • Route review";
    const predecessor = agent("agent-1", { title, labels: { role: "LSA", epic: "ASMA-1", "title.auto": title, persona: personaVersion("lsa-ad-hoc", true) } });
    const h = await host({ predecessor, workspace: { ...WORKSPACE, name: "ADHOC", title: null }, targetProvider: "claude" });
    const { mkdir } = await import("node:fs/promises");
    await mkdir(join(h.createRoot, "promotion-v1"), { recursive: true });
    const native = { workspaceId: "wks-1", workspace: "ADHOC", title, titleAuto: title, observedAt: "2026-10-25T08:00:00Z", revision: "r1" };
    const promoted = promotionRecordSchema.parse({ schemaVersion: 1, requestId: "33333333-3333-4333-8333-333333333331", agentId: predecessor.id,
      epic: "ASMA-1", branch: "promote", state: "promoted", native, logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "a".repeat(64) },
      persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
      summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted", createdAt: "2026-10-25T08:00:00Z", updatedAt: "2026-10-25T08:00:00Z" });
    const cross = promotionRecordSchema.parse({ schemaVersion: 1, requestId: "33333333-3333-4333-8333-333333333332", agentId: "agent-cross",
      epic: "ASMA-2", branch: "handoff", state: "handoff", native, logical: { workspace: "LSA", node: "epic:ASMA-2", bindingRevision: "b".repeat(64) },
      persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
      summarySha256: "b".repeat(64), summaryRevision: "r2", acknowledgement: "verified", message: "handoff", createdAt: "2026-10-25T08:01:00Z", updatedAt: "2026-10-25T08:01:00Z" });
    await writeFile(join(h.createRoot, "promotion-v1", `${promoted.requestId}.json`), JSON.stringify(promoted));
    await writeFile(join(h.createRoot, "promotion-v1", `${cross.requestId}.json`), JSON.stringify(cross));
    const review = await h.ready();
    expect(review.state).toBe("unknown");
    expect(review.message).toContain("different epic");
    expect(h.effects.archive).toHaveLength(0);
    expect(h.effects.create).toHaveLength(0);
  });

  it("does not infer a canonical scope from a missing native epic label", async () => {
    const title = "LSA • ASMA-1 • Route review";
    const predecessor = agent("agent-1", { title, labels: { role: "LSA", "title.auto": title, persona: personaVersion("lsa-ad-hoc", true) } });
    const h = await host({ predecessor, workspace: { ...WORKSPACE, name: "LSA", title: null }, targetProvider: "claude" });
    const { mkdir } = await import("node:fs/promises");
    await mkdir(join(h.createRoot, "promotion-v1"), { recursive: true });
    const native = { workspaceId: "wks-1", workspace: "LSA", title, titleAuto: title, observedAt: "2026-10-25T08:00:00Z", revision: "r1" };
    const handoff = promotionRecordSchema.parse({ schemaVersion: 1, requestId: "44444444-4444-4444-8444-444444444441", agentId: "agent-source",
      epic: "ASMA-2", branch: "handoff", state: "handoff", native, logical: { workspace: "LSA", node: "epic:ASMA-2", bindingRevision: "b".repeat(64) },
      persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
      summarySha256: "b".repeat(64), summaryRevision: "r2", acknowledgement: "verified", message: "handoff", createdAt: "2026-10-25T08:01:00Z", updatedAt: "2026-10-25T08:01:00Z" });
    await writeFile(join(h.createRoot, "promotion-v1", `${handoff.requestId}.json`), JSON.stringify(handoff));
    const review = await h.ready();
    expect(review.state).toBe("unknown");
    expect(review.message).toContain("native owner");
    expect(h.effects.archive).toHaveLength(0);
    expect(h.effects.create).toHaveLength(0);
  });

  it("blocks takeover when the promoted native workspace or persona has drifted", async () => {
    for (const drift of ["workspace", "persona"] as const) {
      const title = "LSA • ASMA-1 • Route review";
      const predecessor = agent("agent-1", { title, workspaceId: drift === "workspace" ? "wks-2" : "wks-1",
        labels: { role: "LSA", epic: "ASMA-1", "title.auto": title, persona: drift === "persona" ? "other-persona" : personaVersion("lsa-ad-hoc", true) } });
      const workspace = drift === "workspace" ? { ...WORKSPACE, id: "wks-2", workspaceDirectory: "/fixtures/wks-2", name: "ADHOC", title: null } : { ...WORKSPACE, name: "ADHOC", title: null };
      const h = await host({ predecessor, workspace, targetProvider: "claude" });
      const { mkdir } = await import("node:fs/promises");
      await mkdir(join(h.createRoot, "promotion-v1"), { recursive: true });
      const native = { workspaceId: "wks-1", workspace: "ADHOC", title, titleAuto: title, observedAt: "2026-10-25T08:00:00Z", revision: "r1" };
      const promoted = promotionRecordSchema.parse({ schemaVersion: 1, requestId: `5555555${drift === "workspace" ? "0" : "1"}-5555-4555-8555-555555555555`, agentId: predecessor.id,
        epic: "ASMA-1", branch: "promote", state: "promoted", native, logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "a".repeat(64) },
        persona: { issued: personaVersion("lsa-ad-hoc", true), effective: personaVersion("lsa-ad-hoc", true) }, canonicalAgentId: predecessor.id,
        summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted", createdAt: "2026-10-25T08:00:00Z", updatedAt: "2026-10-25T08:00:00Z" });
      await writeFile(join(h.createRoot, "promotion-v1", `${promoted.requestId}.json`), JSON.stringify(promoted));
      const review = await h.ready();
      expect(review.state).toBe("unknown");
      expect(h.effects.archive).toHaveLength(0);
      expect(h.effects.create).toHaveLength(0);
    }
  });
  it("does not retire a session whose promotion store is unreadable or unsettled", async () => {
    for (const corrupt of [true, false]) {
      const h = await host();
      const { mkdir } = await import("node:fs/promises");
      await mkdir(join(h.createRoot, "promotion-v1"), { recursive: true });
      if (corrupt) await writeFile(join(h.createRoot, "promotion-v1", "broken.json"), "{");
      else await writeFile(join(h.createRoot, "promotion-v1", "11111111-1111-4111-8111-111111111111.json"), JSON.stringify({
        schemaVersion: 1, requestId: "11111111-1111-4111-8111-111111111111", agentId: "agent-1", epic: "ASMA-1", branch: "promote", state: "promoting",
        native: { workspaceId: "wks-1", workspace: "TSW", title: "Fix the parser", titleAuto: "Fix the parser", observedAt: "2026-10-25T08:00:00Z", revision: "r1" },
        logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "a".repeat(64) },
        persona: { issued: "p", effective: "p" }, canonicalAgentId: null, summarySha256: null, summaryRevision: null,
        acknowledgement: "unknown", message: "pending", createdAt: "2026-10-25T08:00:00Z", updatedAt: "2026-10-25T08:00:00Z",
      }));
      const review = await h.ready();
      expect(review.state).toBe("unknown");
      expect(h.effects.archive).toHaveLength(0);
      expect(h.effects.create).toHaveLength(0);
    }
  });

describe("direct takeover continuity", () => {
  type Host = Awaited<ReturnType<typeof host>>;
  const productionContinuity = (h: Host, schedules?: { forAgent: (agentId: string) => Promise<{ entries: unknown[] }> }, completeSchedules?: NonNullable<TakeoverDeps["completeSchedules"]>) => {
    if (schedules) (h.client as { schedules?: typeof schedules }).schedules = schedules;
    h.deps.completeSchedules = completeSchedules;
    h.deps.continuity = (agentId, id) => readTakeoverContinuity(h.client as never, h.root, agentId, id, { completeSchedules });
    return h;
  };
  const noRetirementEffects = (h: Host) => {
    expect(h.effects.archive).toHaveLength(0);
    expect(h.effects.create).toHaveLength(0);
  };

  describe("AUD schedule wake read shape", () => {
    const requestId = "33333333-3333-4333-8333-333333333333";

    it("qualified empty array entries yields wakes clear on the production reader", async () => {
      const h = productionContinuity(await host(), { forAgent: async () => ({ entries: [] }) });
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt?.wakes).toBe("clear");
    });

    it("non-empty schedule entries yields wakes unknown and blocks review without retirement", async () => {
      const h = productionContinuity(await host(), { forAgent: async () => ({ entries: [{ id: "hb-1", agentId: "agent-1" }] }) });
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt?.wakes).toBe("unknown");
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state).toBe("unknown");
      expect(review.message).toMatch(/source-linked scheduled wakes/i);
      expect(review.digest).toBeNull();
      noRetirementEffects(h);
    });

    it.each([
      ["object entries", { entries: {} }],
      ["string entries", { entries: "" }],
      ["missing entries", {}],
    ] as const)("malformed schedules.forAgent (%s) yields unknown and blocks retirement", async (_label, page) => {
      const h = productionContinuity(await host(), { forAgent: async () => page as { entries: unknown[] } });
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt?.wakes).toBe("unknown");
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state).toBe("unknown");
      noRetirementEffects(h);
    });

    it("null schedules.forAgent page yields unknown and blocks retirement", async () => {
      const h = productionContinuity(await host(), { forAgent: async () => null as unknown as { entries: unknown[] } });
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt?.wakes).toBe("unknown");
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state).toBe("unknown");
      noRetirementEffects(h);
    });

    it("schedules.forAgent throw yields unknown and blocks retirement", async () => {
      const h = productionContinuity(await host(), { forAgent: async () => { throw new Error("unreadable"); } });
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt?.wakes).toBe("unknown");
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state).toBe("unknown");
      noRetirementEffects(h);
    });
  });

  describe("AUD-8641-S3-02 unsupported global wake reader", () => {
    it("production reader records wakes unknown when schedules.forAgent is absent", async () => {
      const h = await host({ predecessor: agent("agent-1", { provider: "claude-a", status: "error", lastError: "429 quota" }) });
      const requestId = "11111111-1111-4111-8111-111111111111";
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
      expect(receipt).toMatchObject({ agentId: "agent-1", requestId, surface: "direct", wakes: "unknown", owner: null });
      const evidence = JSON.parse(await readFile(join(h.root, `takeover-evidence-${hash(`agent-1:${requestId}`)}.json`), "utf8"));
      expect(evidence.sessionRefusal?.kind).toBe("quota");
    });

    it("createTakeoverService assess blocks retirement with unknown and zero host effects", async () => {
      const h = productionContinuity(await host());
      const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
      expect(review.state).toBe("unknown");
      expect(review.message).toMatch(/scheduled-wake reader/i);
      expect(review.digest).toBeNull();
      noRetirementEffects(h);
    });
  });

  describe("AUD-8641-S3-01 continuity semantic pin", () => {
    it("production reader review then apply survives +1ms observation refresh on the service path", async () => {
      vi.useFakeTimers({ now: Date.parse("2026-10-25T09:00:00.000Z") });
      try {
        const h = productionContinuity(await host(), { forAgent: async () => ({ entries: [] }) });
        const review = await h.ready();
        expect(review.state).toBe("ready");
        expect(review.digest).toMatch(/^[a-f0-9]{64}$/);
        vi.advanceTimersByTime(1);
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
        let scheduleEntries: unknown[] = [];
        const h = productionContinuity(await host(), { forAgent: async () => ({ entries: scheduleEntries }) });
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
        let continuityReads = 0;
        const h = productionContinuity(await host(), { forAgent: async () => ({ entries: [] }) });
        const reader = h.deps.continuity!;
        h.deps.continuity = async (agentId, id) => {
          const receipt = await reader(agentId, id);
          if (!receipt) return null;
          continuityReads++;
          if (continuityReads > 1) return { ...receipt, observedAt: "2026-10-25T08:58:00.000Z" };
          return receipt;
        };
        const review = await h.ready();
        expect(review.state).toBe("ready");
        vi.advanceTimersByTime(1);
        const applied = await h.apply(review);
        expect(applied.state).toBe("refused");
        expect(h.effects.archive).toHaveLength(0);
      } finally {
        vi.useRealTimers();
      }
    });
  });

  it("settles an archived direct source without a schedules read", async () => {
    const predecessor = agent("agent-1", { archivedAt: "2026-10-25T08:30:00Z", status: "closed" });
    const h = await host({ predecessor });
    h.deps.continuity = (agentId, requestId) => readTakeoverContinuity(h.client as never, h.root, agentId, requestId);
    const requestId = "44444444-4444-4444-8444-444444444444";
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
    expect(receipt).toMatchObject({ surface: "direct", wakes: "clear" });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state).toBe("ready");
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 1, 1]);
  });

  const complete = (schedules: { target: { type: "agent" | "self"; agentId: string } | { type: "new-agent" }; status: string }[], hostId = "host-1") =>
    async () => ({ complete: true as const, hostId, schedules });

  it("settles a live source from an empty complete host inventory", async () => {
    const h = productionContinuity(await host(), undefined, complete([]));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "77777777-7777-4777-8777-777777777777", { completeSchedules: h.deps.completeSchedules });
    expect(receipt).toMatchObject({ surface: "direct", wakes: "clear", wakeHost: "host-1" });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state, review.message).toBe("ready");
  });

  it("settles a live source when the complete inventory contains only unrelated rows", async () => {
    const h = productionContinuity(await host(), undefined, complete([
      { target: { type: "agent", agentId: "other-agent" }, status: "active" },
      { target: { type: "new-agent" }, status: "paused" },
    ]));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "88888888-8888-4888-8888-888888888888", { completeSchedules: h.deps.completeSchedules });
    expect(receipt).toMatchObject({ wakes: "clear", wakeHost: "host-1" });
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("ready");
  });

  it("does not turn a partial or unreadable complete inventory into clear", async () => {
    for (const read of [
      { complete: false as const, gap: "The complete inventory is partial." },
      { complete: false as const, gap: "The complete inventory is unreadable." },
    ]) {
      const h = productionContinuity(await host(), undefined, async () => read);
      const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "99999999-9999-4999-8999-999999999999", { completeSchedules: h.deps.completeSchedules });
      expect(receipt?.wakes).toBe("unknown");
      expect(receipt?.wakeGap).toBe(read.gap);
      expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("unknown");
    }
  });

  it("blocks an ongoing exact source-linked scheduled duty without dropping it", async () => {
    const h = productionContinuity(await host(), undefined, complete([{ target: { type: "agent", agentId: "agent-1" }, status: "paused" }]));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", { completeSchedules: h.deps.completeSchedules });
    expect(receipt?.wakes).toBe("unknown");
    expect(receipt?.wakeGap).toContain("ongoing source-linked scheduled duty");
    expect(receipt?.wakeGap).toContain("atomic retarget");
    expect(receipt?.wakeGap).toContain("in-flight run settlement");
    expect(receipt?.wakeGap).toContain("successor ownership readback");
    expect(receipt?.wakeGap).toContain("retirement is blocked");
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("unknown");
  });

  it.each(["", " ", "\t"])("does not turn an empty or whitespace complete-inventory target into clear (%j)", async (agentId) => {
    const h = productionContinuity(await host(), undefined, complete([{ target: { type: "agent", agentId }, status: "active" }]));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "cccccccc-cccc-4ccc-8ccc-cccccccccccc", { completeSchedules: h.deps.completeSchedules });
    expect(receipt?.wakes).toBe("unknown");
    expect(receipt?.wakeGap).toContain("empty, whitespace or missing target identity");
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("unknown");
  });

  it("requires an exact daemon host identity for a complete inventory", async () => {
    const h = productionContinuity(await host(), undefined, complete([], ""));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", { completeSchedules: h.deps.completeSchedules });
    expect(receipt?.wakes).toBe("unknown");
    expect(receipt?.wakeGap).toContain("exact daemon host identity");
    expect((await h.service.review({ agentId: "agent-1", bindingKey: KEY })).state).toBe("unknown");
  });

  it("preserves a visible managed child by detaching it, never archiving it, with a qualified clear schedule read", async () => {
    const child = agent("child-1", { labels: { "paseo.parent-agent-id": "agent-1" }, status: "idle", title: "Child task" });
    const h = productionContinuity(await host({ others: [child] }), { forAgent: async () => ({ entries: [] }) });
    const requestId = "22222222-2222-4222-8222-222222222222";
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", requestId);
    expect(receipt?.wakes).toBe("clear");
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state).toBe("ready");
    expect(review.limits.join(" ")).toMatch(/detach/i);
    const applied = await h.apply(review);
    expect(applied.state, applied.message).toBe("succeeded");
    expect(h.agents.get("child-1")!.labels?.["paseo.parent-agent-id"]).toBeUndefined();
    expect(h.agents.get("child-1")!.archivedAt).toBeNull();
    expect(h.effects.archive).toEqual(["agent-1"]);
    expect(h.effects.create).toHaveLength(1);
  });

  it("refuses before stop when the complete schedule host changes after review", async () => {
    let reads = 0;
    const reader = async () => ({ complete: true as const, hostId: reads++ === 0 ? "host-A" : "host-B", schedules: [] });
    const h = productionContinuity(await host({ predecessor: agent("agent-1", { status: "running" }) }), undefined, reader);
    const stop = vi.fn(async () => ({ ok: true as const }));
    h.deps.publicCli = { stop };
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state, review.message).toBe("ready");
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "refused", next: "review" });
    expect(stop).not.toHaveBeenCalled();
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("refuses after stop when the complete schedule host changes before final capture", async () => {
    let reads = 0;
    const reader = async () => ({ complete: true as const, hostId: reads++ < 2 ? "host-A" : "host-B", schedules: [] });
    const h = productionContinuity(await host({ predecessor: agent("agent-1", { status: "running" }) }), undefined, reader);
    const stop = vi.fn(async (id: string) => { h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); return { ok: true as const }; });
    h.deps.publicCli = { stop };
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state, review.message).toBe("ready");
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "refused", phase: "stop", next: "review" });
    expect(stop).toHaveBeenCalledTimes(1);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("stops and completes a running source when the complete schedule host stays pinned", async () => {
    const h = productionContinuity(await host({ predecessor: agent("agent-1", { status: "running" }) }), undefined, complete([], "host-A"));
    const stop = vi.fn(async (id: string) => { h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); return { ok: true as const }; });
    h.deps.publicCli = { stop };
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state, review.message).toBe("ready");
    const applied = await h.apply(review);
    expect(applied.state, applied.message).toBe("succeeded");
    expect(stop).toHaveBeenCalledTimes(1);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("childful live source with no schedule reader is unknown, never preserved", async () => {
    const child = agent("child-1", { labels: { "paseo.parent-agent-id": "agent-1" }, status: "idle", title: "Child task" });
    const h = productionContinuity(await host({ others: [child] }));
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "55555555-5555-4555-8555-555555555555");
    expect(receipt?.wakes).toBe("unknown");
    expect(receipt?.wakeGap).toContain("No qualified complete scheduled-wake reader");
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state).toBe("unknown");
    expect(review.digest).toBeNull();
    noRetirementEffects(h);
  });

  it("childful live source with active source-linked wakes is unknown, never preserved", async () => {
    const child = agent("child-1", { labels: { "paseo.parent-agent-id": "agent-1" }, status: "idle", title: "Child task" });
    const h = productionContinuity(await host({ others: [child] }), { forAgent: async () => ({ entries: [{ id: "hb-1", agentId: "agent-1" }] }) });
    const receipt = await readTakeoverContinuity(h.client as never, h.root, "agent-1", "66666666-6666-4666-8666-666666666666");
    expect(receipt?.wakes).toBe("unknown");
    expect(receipt?.wakeGap).toContain("source-linked scheduled wakes");
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state).toBe("unknown");
    noRetirementEffects(h);
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
      [{ pendingPermissions: [{ id: "p1" }] as Agent["pendingPermissions"], status: "idle" }, "Waiting for permission"],
      [{ status: "running" }, "public Paseo CLI stop adapter"],
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
    const child = await host({ others: [agent("child-1", { status: "idle", title: "Child task", labels: { "paseo.parent-agent-id": "agent-1" } })] });
    const childReview = await child.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(childReview.state, childReview.message).toBe("ready");
    expect(childReview.limits.join(" ")).toMatch(/detach/i);
    // A governed seat with its labels removed still sits in a Kontor placement.
    const stripped = await host();
    stripped.deps.config = { ...stripped.deps.config, placementRoots: ["/fixtures"] };
    expect((await stripped.service.review({ agentId: "agent-1", bindingKey: KEY })).message).toContain("Kontor placement");
    const none = await host({ choices: [] });
    expect(await none.service.review({ agentId: "agent-1", bindingKey: null })).toMatchObject({ state: "refused", message: expect.stringContaining("No configured profile") });
  });

  it("carries the source's listed model settings to the one successor", async () => {
    const predecessor = agent("agent-1", { features: FEATURES });
    const h = await host({ predecessor, features: FEATURES });
    const review = await h.ready();
    expect(review.state, review.message).toBe("ready");
    expect(review.limits.join(" ")).toContain("MCP server or provider-option configuration");
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]?.config).toMatchObject({ featureValues: { plan: true, speed: "fast" } });
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("refuses source settings changed before apply preflight", async () => {
    const h = await host({ predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    expect(review.state, review.message).toBe("ready");
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: withPlan(false) });
    expect(await h.apply(review)).toMatchObject({ state: "refused", next: "review" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("refuses source settings changed during apply preflight with zero effects", async () => {
    const h = await host({ predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    expect(review.state, review.message).toBe("ready");
    const listFeatures = h.client.providers.listFeatures;
    h.client.providers.listFeatures = async () => {
      h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: withPlan(false) });
      return listFeatures();
    };
    expect(await h.apply(review)).toMatchObject({ state: "refused", next: "review" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("refuses source settings changed during the stopped source's final assessment", async () => {
    const h = await host({ predecessor: agent("agent-1", { features: FEATURES, status: "running" }), features: FEATURES });
    h.deps.publicCli = { stop: async (id) => { h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); return { ok: true }; } };
    const review = await h.ready();
    expect(review.state, review.message).toBe("ready");
    const listFeatures = h.client.providers.listFeatures;
    let calls = 0;
    h.client.providers.listFeatures = async () => {
      if (++calls === 2) h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: withPlan(false) });
      return listFeatures();
    };
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "stop" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("keeps the source settings when an explicit route names no replacement values", async () => {
    const predecessor = agent("agent-1", { features: FEATURES });
    const h = await host({ predecessor, features: FEATURES });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: null,
      route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" } });
    expect(review.state, review.message).toBe("ready");
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]?.config).toMatchObject({ featureValues: { plan: true, speed: "fast" } });
  });

  it("keeps an explicit replacement value separate from source setting drift", async () => {
    const predecessor = agent("agent-1", { features: FEATURES });
    const h = await host({ predecessor, features: FEATURES });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: null,
      route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh", featureValues: { plan: false } } });
    expect(review.state, review.message).toBe("ready");
    const listFeatures = h.client.providers.listFeatures;
    h.client.providers.listFeatures = async () => {
      h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: withPlan(false) });
      return listFeatures();
    };
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]?.config).toMatchObject({ featureValues: { plan: false } });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("refuses rather than dropping a source setting the target model does not offer", async () => {
    const predecessor = agent("agent-1", { features: FEATURES });
    const h = await host({ predecessor, features: [FEATURES[0]!] });
    const review = await h.ready();
    expect(review.state, review.message).toBe("refused");
    expect(review.message).toContain("speed");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("names both external writers and the missing handoff fields for a governed seat", async () => {
    const h = await host({ predecessor: agent("agent-1", { labels: { "kontor.seat_binding_id": "seat-1" } }) });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: KEY });
    expect(review.state).toBe("refused");
    expect(review.message).toContain("Core Team route apply");
    expect(review.message).toContain("delivery seat replacement");
    expect(review.message).toContain("handoff summary");
    expect(review.message).toContain("bootstrap acknowledgement");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
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
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", digest: null, message: expect.stringContaining("authority writer") });
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
    expect(await h.service.review({ agentId: "agent-1", bindingKey: KEY })).toMatchObject({ state: "refused", message: expect.stringContaining("Managed children changed") });
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

  it("reconciles a lost creation reply on the same request and Check delivers the one continuation", async () => {
    const h = await host({ loseCreateReply: true });
    const review = await h.ready();
    const result = await h.apply(review);
    expect(result).toMatchObject({ state: "uncertain", phase: "create", next: "check" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "succeeded", phase: "done", next: "none", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(await h.service.check(review.reviewId)).toEqual(checked);
    expect(await h.apply(review)).toEqual(checked);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("reconciles a lost task-release reply by the same message ID and rechecks the successor", async () => {
    for (const drift of [false, true]) {
      const h = await host(), ref = h.client.agents.ref;
      h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, send: async (...args) => { await handle.send(...args); throw new Error("lost reply"); } }; };
      const review = await h.ready();
      expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
      h.agents.set("successor-1", { ...h.agents.get("successor-1")!, status: "running" });
      if (drift) h.agents.set("successor-1", { ...h.agents.get("successor-1")!, model: "another-model" });
      expect(await h.service.check(review.reviewId)).toMatchObject({ state: drift ? "partial" : "succeeded" });
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
      expect(await h.service.check(review.reviewId)).toEqual(await h.service.check(review.reviewId));
      expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
    }
  });
});

describe("universal takeover operation", () => {
  it("uses one operation and create request ID and records it on the successor", async () => {
    const h = await host(), review = await h.ready();
    expect(review.successor!.requestId).toBe(review.reviewId);
    expect(await h.apply(review)).toMatchObject({ state: "succeeded", reviewId: review.reviewId });
    expect(h.effects.create[0]!.idempotencyKey).toBe(review.reviewId);
    expect((h.effects.create[0]!.labels as Record<string, string>)["create.request"]).toBe(review.reviewId);
  });

  it("takes over an archived source from its history without reviving or prompting it", async () => {
    const predecessor = agent("agent-1", { archivedAt: "2026-10-25T08:30:00Z", status: "closed" });
    const h = await host({ predecessor });
    const review = await h.ready();
    expect(review).toMatchObject({ state: "ready", predecessor: { status: "closed" } });
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.archive).toEqual([]);
    expect(h.effects.create).toHaveLength(1);
    expect(h.effects.send).toHaveLength(1);
    expect(h.effects.send[0]!.id).toBe("successor-1");
    expect(h.agents.get("agent-1")!.archivedAt).toBe("2026-10-25T08:30:00Z");
  });

  it.each(["LSA", "TPM", "SWE", "QA", "JUDGE", null] as const)("takes over an ordinary session with role %s unchanged", async (role) => {
    const predecessor = agent("agent-1", { labels: { ...(role ? { role } : {}), note: "not carried" } });
    const h = await host({ predecessor });
    const review = await h.ready();
    expect(review.state).toBe("ready");
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]!.labels).toMatchObject(role ? { role } : {});
    if (!role) expect(h.effects.create[0]!.labels).not.toHaveProperty("role");
  });

  it.each(["claude", "codex", "gemini", "opencode"])("captures a %s source independently of the replacement provider", async (provider) => {
    const h = await host({ predecessor: agent("agent-1", { provider }) });
    const review = await h.ready();
    expect(review).toMatchObject({ state: "ready", predecessor: { provider } });
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]!.config).toMatchObject({ provider: "claude-a/claude-opus-5", modeId: "default" });
  });

  it.each(["claude-a", "codex-a"])("takes over on an explicitly picked %s route without the configured-chain resolver", async (targetProvider) => {
    const h = await host({ targetProvider });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: null, route: { provider: targetProvider, model: "claude-opus-5", effort: "xhigh" } });
    expect(review).toMatchObject({ state: "ready", successor: { provider: targetProvider, model: "claude-opus-5", effort: "xhigh", mode: "default" } });
    expect(h.effects.resolve).toHaveLength(0);
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect(h.effects.create[0]!.config).toMatchObject({ provider: `${targetProvider}/claude-opus-5`, modeId: "default", thinkingOptionId: "xhigh" });
  });

  it("rejects a stale provider observation for an explicitly picked route", async () => {
    const h = await host(), now = Date.parse("2026-10-25T09:10:00.000Z");
    vi.useFakeTimers({ now });
    try {
      h.deps.snapshot = async () => ({ ...SNAPSHOT, requestId: "stale", generatedAt: new Date(now - 301_000).toISOString(),
        entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: "claude-a" })) });
      const review = await h.service.review({ agentId: "agent-1", bindingKey: null, route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" } });
      expect(review).toMatchObject({ state: "refused", digest: null });
      expect(review.message).toMatch(/old|window/i);
      expect(h.effects.resolve).toHaveLength(0);
    } finally { vi.useRealTimers(); }
  });

  it("rejects a future or unqualified provider snapshot for an explicitly picked route", async () => {
    const h = await host(), now = Date.parse("2026-10-25T09:10:00.000Z");
    vi.useFakeTimers({ now });
    try {
      h.deps.snapshot = async () => ({ ...SNAPSHOT, requestId: "future", generatedAt: new Date(now + 60_000).toISOString(),
        entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: "claude-a" })) });
      const review = await h.service.review({ agentId: "agent-1", bindingKey: null, route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" } });
      expect(review).toMatchObject({ state: "refused", digest: null });
      expect(review.message).toMatch(/unavailable or unobserved|future/i);
    } finally { vi.useRealTimers(); }
  });

  it("keeps semantic identity across a fresh transport request and a clock-only change", async () => {
    const h = await host();
    let request = 0;
    h.deps.snapshot = async () => ({ ...SNAPSHOT, requestId: `transport-${++request}`, generatedAt: new Date().toISOString(),
      entries: SNAPSHOT.entries.map((entry) => ({ ...entry, provider: "claude-a" })) });
    const review = await h.service.review({ agentId: "agent-1", bindingKey: null, route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" } });
    expect(review).toMatchObject({ state: "ready" });
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    expect([h.effects.archive.length, h.effects.create.length]).toEqual([1, 1]);
  });

  it("accepts a clock-only refresh of the same explicitly picked observation", async () => {
    const t0 = Date.parse("2026-10-25T09:00:00.000Z");
    vi.useFakeTimers({ now: t0 });
    try {
      const h = await host();
      const review = await h.service.review({ agentId: "agent-1", bindingKey: null, route: { provider: "claude-a", model: "claude-opus-5", effort: "xhigh" } });
      expect(review).toMatchObject({ state: "ready" });
      vi.advanceTimersByTime(1000);
      expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    } finally { vi.useRealTimers(); }
  });

  it("reports the exact missing host stop primitive instead of a running-state prohibition", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) });
    const review = await h.ready();
    expect(review).toMatchObject({ state: "refused", digest: null });
    expect(review.message).toContain("public Paseo CLI stop adapter");
    expect(review.message).toContain("Nothing was retired");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("re-checks predecessor retirement at the shared delivery boundary after creation", async () => {
    const h = await host();
    const createSeat = h.deps.create.createSeat;
    let created = false, readsAfterCreate = 0;
    h.deps.create = { ...h.deps.create, createSeat: async (...args) => { const outcome = await createSeat(...args); created = true; return outcome; } };
    const ref = h.client.agents.ref;
    h.client.agents.ref = (id) => {
      const handle = ref(id);
      if (id !== "agent-1") return handle;
      return { ...handle, refresh: async () => {
        if (created && ++readsAfterCreate === 2) h.agents.set("agent-1", { ...h.agents.get("agent-1")!, archivedAt: null, status: "running" });
        return handle.refresh();
      } };
    };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "partial", next: "none", successor: { agentId: "successor-1" } });
    expect(applied.message).toContain("not archived any more");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 0]);
    expect(await readFile(join(h.root, `takeover-successor-${hash("agent-1")}.json`), "utf8")).toContain("successor-1");
  });

  it("stops a running source before archive and takes the final summary after the stop", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) }), order: string[] = [];
    h.deps.publicCli = {
      stop: async (id) => { order.push(`stop:${id}`); h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); h.timelines.get(id)!.push({ item: { type: "assistant_message", text: "Final summary after the stop." } }); return { ok: true }; }
    };
    const ref = h.client.agents.ref;
    h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, archive: async () => { order.push(`archive:${id}`); return handle.archive(); } }; };
    const review = await h.ready();
    expect(review.state).toBe("ready");
    expect(review.limits.join(" ")).toMatch(/Apply stops the exact source/);
    const applied = await h.apply(review);
    expect(applied.state, applied.message).toBe("succeeded");
    expect(order).toEqual(["stop:agent-1", "archive:agent-1"]);
    expect(h.effects.create).toHaveLength(1);
    expect(String(h.effects.create[0]!.prompt)).toContain("Final summary after the stop.");
  });

  it("refuses a running source when the public CLI is unavailable, with no archive or create", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) });
    h.deps.publicCli = { stop: async () => ({ ok: false, unavailable: true, message: "The configured public Paseo CLI paseo could not be found." }) };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "refused", phase: "stop", next: "review" });
    expect(applied.message).toContain("could not be found");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("refuses a running source that remains active after the stop, with no archive or create", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) });
    h.deps.publicCli = { stop: async () => ({ ok: true }) };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "refused", phase: "stop", next: "review" });
    expect(applied.message).toContain("still running");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("refuses a stopped source whose title changed during the stop, with no archive or create", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) });
    h.deps.publicCli = { stop: async (id) => { h.agents.set(id, { ...h.agents.get(id)!, status: "idle", title: "Changed title" }); return { ok: true }; } };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "refused", phase: "stop", next: "review" });
    expect(applied.message).toContain("title");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("Check reconciles a lost stop reply without archive, create or send", async () => {
    const h = await host({ predecessor: agent("agent-1", { status: "running" }) });
    h.deps.publicCli = { stop: async (id) => { h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); throw new Error("stop reply lost"); } };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "uncertain", phase: "stop", next: "check" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "refused", phase: "stop", next: "review" });
    expect(checked.message).toContain("stop is complete");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([0, 0, 0]);
  });

  it("Check preserves an observed archive and fence when source settings drift", async () => {
    const h = await host({ archiveStatus: "idle", predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    expect(h.agents.get("agent-1")!.archivedAt).not.toBeNull();
    expect(await h.fence()).toHaveLength(1);
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: withPlan(false) });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    expect(checked.message).toContain("model settings changed");
    expect(checked.message).not.toContain("Nothing was archived or created");
    expect(await h.fence()).toHaveLength(1);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("Check keeps an observed archive and fence without settings drift", async () => {
    const h = await host({ archiveStatus: "idle", predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    expect(checked.message).not.toContain("model settings changed");
    expect(await h.fence()).toHaveLength(1);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("Check preserves an observed archive and fence when source settings become unreadable", async () => {
    const h = await host({ archiveStatus: "idle", predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, features: [{ type: "toggle", id: "", label: "Plan", value: true }] as NonNullable<Agent["features"]> });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    expect(checked.message).toContain("model settings");
    expect(checked.message).not.toContain("Nothing was archived or created");
    expect(await h.fence()).toHaveLength(1);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("Check reports a settled retirement without creating a successor", async () => {
    const h = await host({ archiveStatus: "idle", predecessor: agent("agent-1", { features: FEATURES }), features: FEATURES });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "partial", phase: "archive", next: "check" });
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, status: "closed" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", phase: "archive", next: "none" });
    expect(checked.message).toContain("retired");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("reviews a running source without stopping or saving, then stops before the actual stock final summary", async () => {
    const source = agent("agent-1", { status: "running", currentModeId: "default" }) as Agent & { persistence: { sessionId: string } };
    source.persistence = { provider: "claude", sessionId: "source-session" };
    const h = await host({ predecessor: source });
    h.agents.set("agent-1", source);
    h.timelines.set("agent-1", [
      { item: { type: "user_message", text: "Do the work", clientMessageId: "source-user" }, seqStart: 1, seqEnd: 1, timestamp: "2026-10-25T08:00:00Z" },
      { item: { type: "assistant_message", text: "The parser is fixed; the migration is next." }, seqStart: 2, seqEnd: 2, timestamp: "2026-10-25T08:01:00Z" },
    ] as never);
    const sourcePage = () => ({ agent: h.agents.get("agent-1"), agentId: "agent-1", entries: h.timelines.get("agent-1") ?? [],
      epoch: "source-epoch", gap: null, staleCursor: null, hasOlder: false, hasNewer: false });
    const helperSections = JSON.stringify({ task: "Keep the parser fix.", decisions: "Keep the migration.", currentState: "The source is stopped.", nextStep: "Continue the migration." });
    const order: string[] = [];
    const ref = h.client.agents.ref;
    h.client.agents.ref = (id: string) => {
      const handle = ref(id);
      return { ...handle,
        archive: async () => { if (id === "agent-1") order.push("archive"); return handle.archive(); },
        timeline: { refetch: async (options?: { projection?: string }) => options?.projection === "canonical"
          ? (id === "agent-1" ? sourcePage() : { agent: h.agents.get(id), agentId: id, entries: h.timelines.get(id) ?? [], epoch: "helper-epoch", gap: null, staleCursor: null, hasOlder: false, hasNewer: false }) as never
          : handle.timeline.refetch() },
        waitForFinish: async () => {
          const entries = h.timelines.get(id) ?? [];
          entries.push({ item: { type: "assistant_message", text: helperSections }, seqStart: 2, seqEnd: 2, timestamp: "2026-10-25T08:02:00Z" } as never);
          h.timelines.set(id, entries);
          return { status: "idle" as const, final: null, error: null, lastMessage: helperSections };
        },
      } as typeof handle;
    };
    (h.client as unknown as { agents: { create: (input: Record<string, unknown> & { agentId: string; config: { provider: string }; prompt?: string; clientMessageId: string; title: string; cwd: string; labels: Record<string, string> }) => Promise<unknown> } }).agents.create = async (input) => {
      const [provider, model] = input.config.provider.split("/");
      const helper = agent(input.agentId, { title: input.title, labels: input.labels, provider: provider!, model: model!, cwd: input.cwd, workspaceId: "wks-1", status: "idle", currentModeId: "default", persistence: { sessionId: `helper-${input.agentId}` } } as Partial<Agent>);
      h.agents.set(helper.id, helper);
      h.timelines.set(helper.id, [{ item: { type: "user_message", text: input.prompt ?? "", clientMessageId: input.clientMessageId }, seqStart: 1, seqEnd: 1, timestamp: "2026-10-25T08:01:30Z" } as never]);
      return { id: helper.id, ...h.client.agents.ref(helper.id) };
    };
    const summaryRoot = join(h.root, "..", "summaries");
    const summaries = createSessionSummaryService(h.client as never, summaryRoot, stockSummaryGenerator(h.client as never, summaryRoot));
    h.deps.summaries = summaries;
    const saveOriginal = summaries.save.bind(summaries);
    const save = vi.spyOn(summaries, "save").mockImplementation(async (id) => { order.push("save"); return saveOriginal(id); });
    const stop = vi.fn(async (id: string) => { order.push("stop"); h.agents.set(id, { ...h.agents.get(id)!, status: "idle" }); return { ok: true as const }; });
    h.deps.publicCli = { stop };
    const review = await h.ready();
    expect(review.state, review.message).toBe("ready");
    expect(stop).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    const applied = await h.apply(review);
    expect(applied.state, applied.message).toBe("succeeded");
    expect(stop).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(order).toEqual(["stop", "save", "archive"]);
    expect(h.effects.archive.filter((id) => id === "agent-1")).toEqual(["agent-1"]);
    expect(h.effects.archive).toHaveLength(2);
    expect(h.effects.create).toHaveLength(1);
    expect(String(h.effects.create[0]!.prompt)).toContain("Keep the parser fix.");
  });

  it("settles a crash before the release send with exactly one delivery and no second successor", async () => {
    const h = await host({ failFirstSend: true });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
    expect([h.effects.create.length, h.effects.send.length]).toEqual([1, 0]);
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "succeeded", successor: { agentId: "successor-1" } });
    expect([h.effects.create.length, h.effects.send.length]).toEqual([1, 1]);
    expect(await h.service.check(review.reviewId)).toEqual(checked);
    expect(h.effects.send).toHaveLength(1);
  });

  it.each(["absent", "foreign"] as const)("does not send or create from Check when this operation's fence is %s", async (kind) => {
    const h = await host({ loseCreateReply: true });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "create", next: "check" });
    const fence = join(h.root, `intent-${hash("agent-1")}.json`), operation = join(h.root, `takeover-operation-${review.reviewId}.json`);
    if (kind === "absent") await rm(fence, { force: true });
    else await writeFile(fence, JSON.stringify({ reviewId: randomUUID(), agentId: "agent-1", action: "archive", phase: "intent", fingerprint: "x", token: randomUUID() }));
    const before = await readFile(operation, "utf8");
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "uncertain", next: "check" });
    expect(checked.message).toContain("fence");
    expect([h.effects.create.length, h.effects.send.length]).toEqual([1, 0]);
    expect(await readFile(operation, "utf8")).toBe(before);
  });

  it("returns the delivered operation on repeated Check without another send", async () => {
    const h = await host();
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "succeeded" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "succeeded", next: "none" });
    expect(await h.service.check(review.reviewId)).toEqual(checked);
    expect(h.effects.send).toHaveLength(1);
  });

  it("refuses a fresh takeover after success with the exact recorded successor and no new effect", async () => {
    const h = await host();
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "succeeded", successor: { agentId: "successor-1" } });
    const refused = await h.service.review({ agentId: "agent-1", bindingKey: null });
    expect(refused).toMatchObject({ state: "refused" });
    expect(refused.message).toContain("successor-1");
    expect(refused.message).toContain("does not create another successor");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("refuses a fresh takeover after a crash out of create before release without a second successor", async () => {
    const h = await host({ failFirstSend: true });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
    const refused = await h.service.review({ agentId: "agent-1", bindingKey: null });
    expect(refused).toMatchObject({ state: "refused" });
    expect(refused.message).toContain("successor-1");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 0]);
  });

  it("fails closed on a corrupt successor marker even when a lineage label exists", async () => {
    const h = await host();
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "succeeded", successor: { agentId: "successor-1" } });
    await writeFile(join(h.root, `takeover-successor-${hash("agent-1")}.json`), "{not-json");
    const unknown = await h.service.review({ agentId: "agent-1", bindingKey: null });
    expect(unknown).toMatchObject({ state: "unknown" });
    expect(unknown.message).toContain("successor record could not be read");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("uses the successor lineage label when no marker was written, before any summary or route", async () => {
    const h = await host();
    h.agents.set("successor-1", agent("successor-1", { labels: { [TAKEOVER_PREDECESSOR_LABEL]: "agent-1" } }));
    const refused = await h.service.review({ agentId: "agent-1", bindingKey: null });
    expect(refused).toMatchObject({ state: "refused" });
    expect(refused.message).toContain("successor-1");
    expect(refused.message).toContain(TAKEOVER_PREDECESSOR_LABEL);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length, h.effects.resolve.length]).toEqual([0, 0, 0, 0]);
  });

  it("does not release a verified successor when the source was revived before Check", async () => {
    const h = await host({ loseCreateReply: true });
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "create", next: "check" });
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, archivedAt: null, status: "idle" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", next: "none", successor: { agentId: "successor-1" } });
    expect(checked.message).toContain("not archived any more");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 0]);
  });

  it("re-checks the predecessor inside the creation core and creates no successor after revival", async () => {
    const h = await host(), original = h.deps.create.createSeat;
    h.deps.create = { ...h.deps.create, createSeat: async (requestId, build) => {
      h.agents.set("agent-1", { ...h.agents.get("agent-1")!, archivedAt: null, status: "idle" });
      return original(requestId, build);
    } };
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "partial", next: "none", successor: { agentId: null } });
    expect(applied.message).toContain("not archived any more");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 0, 0]);
  });

  it("does not release the continuation when the source was revived before Check at release", async () => {
    const h = await host(), ref = h.client.agents.ref;
    h.client.agents.ref = (id) => { const handle = ref(id); return { ...handle, send: async (...args) => { await handle.send(...args); throw new Error("lost reply"); } }; };
    const review = await h.ready();
    expect(await h.apply(review)).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
    h.agents.set("agent-1", { ...h.agents.get("agent-1")!, archivedAt: null, status: "idle" });
    const checked = await h.service.check(review.reviewId);
    expect(checked).toMatchObject({ state: "partial", next: "none" });
    expect(checked.message).toContain("not archived any more");
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
  });

  it("does not equate queued delivery with observed continuation", async () => {
    const h = await host({ hideRelease: true });
    const review = await h.ready();
    const applied = await h.apply(review);
    expect(applied).toMatchObject({ state: "uncertain", phase: "release", next: "check" });
    expect(applied.message).toContain("not continuation");
    h.showRelease();
    expect(await h.service.check(review.reviewId)).toMatchObject({ state: "succeeded", phase: "done" });
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
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
    expect(result).toMatchObject({ state: "succeeded", phase: "done", next: "none", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
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
      if (stage === "archive workspace" && nativeReads >= 3 && !h.effects.archive.length) now += wait;
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
    expect(result).toMatchObject({ state: "succeeded", successor: { requestId: review.successor!.requestId, agentId: "successor-1" } });
    expect(await h.apply(review)).toEqual(result);
    expect([h.effects.archive.length, h.effects.create.length, h.effects.send.length]).toEqual([1, 1, 1]);
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


// Contract fixtures exercise real immutable files and checkout reads; no native-provider qualification is claimed.
async function proceduralFixture(fault?: "archive" | "create" | "bootstrap" | "continuation" | "wrong-response" | "detach" | "session-replaced" | "source-revived" | "wrong-config" | "wrong-labels", workflow: "free" | "kontor" = "free") {
  const root = await mkdtemp(join(tmpdir(), "kontor-procedural-")); dirs.push(root);
  vi.stubEnv("PASEO_HOME", root);
  const cwd = join(root, "checkout"); await makeDirectory(cwd);
  const git = nativePromisify(nativeExecFile);
  await git("git", ["-C", cwd, "init", "-b", "fixture"]);
  await git("git", ["-C", cwd, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "--allow-empty", "-m", "fixture"]);
  const sourceId = randomUUID(), operationId = randomUUID(), coordinatorId = workflow === "kontor" ? randomUUID() : null;
  const source = agent(sourceId, { cwd, workspaceId: "wks-procedural", labels: { role: "SWE", ...(coordinatorId ? { "paseo.parent-agent-id": coordinatorId, orchestration: "paseo-direct" } : {}) }, persistence: { provider: "claude", sessionId: "source-thread" } });
  const agents = new Map<string, Agent>([[sourceId, source]]), effects: string[] = [], timeline: unknown[] = [];
  if (coordinatorId) agents.set(coordinatorId, agent(coordinatorId, { status: "idle", cwd, labels: { orchestration: "paseo-direct" } }));
  const config = { provider: "codex", model: "fixture-model", effort: "low", mode: "full-access" };
  const mode = { workflow, orchestration: "paseo" } as const;
  const ref = async (name: string, value: unknown) => { const path = join(root, name), text = JSON.stringify(value); await writeFile(path, text); return { path, sha256: takeoverHash(text) }; };
  const sourceProof = await ref("schedule-source.json", { sourceId, status: "paused" });
  const admission: ProceduralAdmission = { version: 1, operationId, sourceId, hostId: "fixture-host", mode, coordinatorId,
    sourceRevision: lifecycleRevision(source), workspaceId: "wks-procedural", cwd, projectKey: "FIXTURE", title: source.title!, expiresAt: new Date(Date.now() + 300_000).toISOString(), config,
    checkout: await takeoverCheckout(cwd), children: [], consent: "procedural-no-host-fence",
    handoff: await ref("handoff.json", { operationId, sourceId, scope: "fixture", criteria: ["preserve work"], completeAvailableContext: true, authorizedNextAction: "Inspect the fixture" }),
    authority: await ref("authority.json", { operationId, sourceId, coordinatorId, mode, config }),
    plan: await ref("plan.json", { operationId, sourceId, mode }), history: [await ref("history.json", { sourceId, history: "complete fixture context" })],
    accounting: await ref("accounting.json", { operationId, sourceId, mode, reviewedBy: "fixture-operator", observedAt: new Date().toISOString(), expiresAt: new Date(Date.now() + 300_000).toISOString(), coverage: "owner-reviewed-not-host-certified", schedules: [{ id: "old-heartbeat", status: "paused", runningRunIds: [], source: sourceProof }], workers: [], reporting: { ownerId: coordinatorId ?? "fixture-operator", deadline: new Date(Date.now() + 300_000).toISOString() }, accountPrincipal: "unverified", changedCondition: "Operator reviewed a changed provider route" }) };
  const client = {
    agents: {
      list: async () => ({ entries: [...agents.values()].filter(value => !value.archivedAt).map(agent => ({ agent })), pageInfo: { hasMore: false } }),
      ref: (id: string) => ({
        refresh: async () => agents.has(id) ? { agent: agents.get(id), project: { projectKey: "FIXTURE" } } : null,
        archive: async () => { effects.push("archive"); Object.assign(source, { archivedAt: new Date().toISOString(), status: "closed" }); if (fault === "archive") throw new Error("lost archive reply"); },
        detach: async () => { effects.push("detach"); delete agents.get(id)!.labels!["paseo.parent-agent-id"]; if (fault === "detach") throw new Error("lost detach reply"); },
        timeline: { refetch: async () => ({ agent: agents.get(id), entries: timeline, error: null }) },
        run: async (prompt: string, options: { messageId: string }) => {
          expect(source.archivedAt).toBeTruthy(); expect(source.status).toBe("closed");
          const stage = options.messageId.includes("bootstrap") ? "bootstrap" : "continuation";
          effects.push(stage);
          const ack = JSON.parse(prompt.split("reply ONLY this exact JSON: ")[1]!.split(". Otherwise")[0]!);
          if (fault === "wrong-response") ack.operationId = randomUUID();
          const text = JSON.stringify(ack), turnId = `turn-${stage}`;
          timeline.push({ provider: "codex", turnId, seqStart: timeline.length + 1, seqEnd: timeline.length + 1, item: { type: "user_message", clientMessageId: options.messageId } });
          timeline.push({ provider: "codex", turnId, seqStart: timeline.length + 1, seqEnd: timeline.length + 1, item: { type: "assistant_message", text } });
          if (fault === "source-revived" && stage === "bootstrap") Object.assign(source, { archivedAt: null, status: "idle" });
          if (fault === "session-replaced" && stage === "continuation") agents.get(id)!.persistence!.sessionId = "replacement-thread";
          if (fault === stage) throw new Error("lost native reply");
          return { status: "idle", final: agents.get(id), lastMessage: text, error: null };
        },
      }),
    },
    workspaces: { ref: () => ({ refresh: async () => ({ workspaceDirectory: cwd }), agents: { create: async (input: { agentId: string; title: string; parent?: string; labels: Record<string, string> }) => {
      expect(source.archivedAt).toBeTruthy(); effects.push("create");
      agents.set(input.agentId, agent(input.agentId, { cwd, workspaceId: admission.workspaceId, title: input.title, status: "idle", provider: config.provider, model: config.model,
        labels: { ...input.labels, ...(input.parent ? { "paseo.parent-agent-id": input.parent } : {}) }, currentModeId: config.mode, effectiveThinkingOptionId: config.effort, runtimeInfo: { provider: "codex", sessionId: "successor-thread", model: config.model }, persistence: { provider: "codex", sessionId: "successor-thread" } }));
      if (fault === "wrong-config") agents.get(input.agentId)!.currentModeId = "another-mode";
      if (fault === "wrong-labels") agents.get(input.agentId)!.labels!.role = "ANOTHER-ROLE";
      if (fault === "create") throw new Error("lost creation reply"); return { id: input.agentId };
    } } }) },
    providers: { snapshot: async () => ({ entries: [{ provider: config.provider, enabled: true, status: "ready", models: [{ id: config.model, thinkingOptions: [{ id: config.effort }] }], modes: [{ id: config.mode }] }] }) },
  } as unknown as PaseoApi;
  const store = join(root, "operations"), service = createProceduralTakeover(client, store, "fixture-host");
  return { root, client, service, admission, ref, effects, agents, source, store };
}

describe("procedural stock takeover", () => {
  it("keeps all three explicit workflow/orchestration modes and rejects free+kontor", () => {
    for (const mode of [{ workflow: "kontor", orchestration: "kontor" }, { workflow: "kontor", orchestration: "paseo" }, { workflow: "free", orchestration: "paseo" }]) expect(takeoverModeSchema.safeParse(mode).success).toBe(true);
    expect(takeoverModeSchema.safeParse({ workflow: "free", orchestration: "kontor" }).success).toBe(false);
  });
  it("retires before task effects, reserves exact IDs and deduplicates across reconstructed services", async () => {
    const f = await proceduralFixture(); const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect(f.effects).toEqual([]);
    const result = await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash);
    expect(result.phase).toBe("active"); expect(result.successorId).toBe(prepared.receipt.successorId);
    expect(result.evidence).toHaveLength(2);
    expect((await f.service.prepare(await f.ref("admission.json", f.admission))).receipt).toEqual(result); expect(f.effects).toEqual(["archive", "create", "bootstrap", "continuation"]);
    expect(await createProceduralTakeover(f.client, f.store, "fixture-host").execute(result.operationId, result.admissionHash)).toEqual(result);
    expect(f.effects).toHaveLength(4); expect(f.agents.size).toBe(2);
  });
  it.each(["archive", "create", "bootstrap", "continuation"] as const)("holds a lost %s reply and never resends after reconstruction", async fault => {
    const f = await proceduralFixture(fault); const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    const result = await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash); expect(result.phase).toBe("held");
    const before = [...f.effects], restarted = createProceduralTakeover(f.client, f.store, "fixture-host");
    await restarted.execute(result.operationId, result.admissionHash); const checked = await restarted.check(result.operationId);
    expect(f.effects).toEqual(before); if (fault === "continuation") { expect(checked.message).toContain("correlated continuation response"); expect(checked.phase).toBe("active"); expect((await restarted.read(result.operationId)).phase).toBe("active"); }
  });
  it("refuses governed ownership, stale source and changed prepared evidence without runtime effects", async () => {
    const f = await proceduralFixture(); f.source.labels = { "kontor.seat_binding_id": "governed-seat" };
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow("Governed"); expect(f.effects).toEqual([]);
    f.source.labels = { role: "SWE" }; const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    f.source.status = "idle"; expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held"); expect(f.effects).toEqual([]);
  });
  it("does not promote an unrelated response to continuation evidence", async () => {
    const f = await proceduralFixture("wrong-response"); const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    const result = await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash); expect(result.phase).toBe("held"); expect(f.effects).not.toContain("continuation");
  });
  it("runs Kontor/Paseo only with its exact existing coordinator and refuses governed mode", async () => {
    const f = await proceduralFixture(undefined, "kontor"), prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("active");
    expect(parentOfProcedural(f.agents.get(prepared.receipt.successorId)!)).toBe(f.admission.coordinatorId);
    const governed = await proceduralFixture(); governed.admission.mode = { workflow: "kontor", orchestration: "kontor" };
    await expect(governed.service.prepare(await governed.ref("admission.json", governed.admission))).rejects.toThrow("Governed transitions");
    expect(governed.effects).toEqual([]);
  });
  it.each([undefined, "detach"] as const)("preserves exact children and never archives after an unknown detach (%s)", async fault => {
    const f = await proceduralFixture(fault), childId = randomUUID();
    f.agents.set(childId, agent(childId, { labels: { "paseo.parent-agent-id": f.admission.sourceId }, status: "running" }));
    f.admission.children = [childId];
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    const result = await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash);
    expect(parentOfProcedural(f.agents.get(childId)!)).toBeNull();
    if (fault) { expect(result.phase).toBe("held"); expect(f.effects).toEqual(["detach"]); }
    else { expect(result.phase).toBe("active"); expect(f.agents.get(childId)?.archivedAt).toBeNull(); expect(f.effects[0]).toBe("detach"); }
  });
  it("one immutable execution claim prevents concurrent duplicate runtime effects", async () => {
    const f = await proceduralFixture(), prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    await Promise.all([f.service.execute(f.admission.operationId, prepared.receipt.admissionHash), createProceduralTakeover(f.client, f.store, "fixture-host").execute(f.admission.operationId, prepared.receipt.admissionHash)]);
    expect(f.effects).toEqual(["archive", "create", "bootstrap", "continuation"]);
  });
  it.each(["session-replaced", "source-revived", "wrong-config", "wrong-labels"] as const)("holds observed runtime/configuration drift (%s)", async fault => {
    const f = await proceduralFixture(fault), prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    const result = await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash);
    expect(result.phase).toBe("held"); const before = [...f.effects];
    expect((await f.service.check(result.operationId)).phase).toBe("held"); expect(f.effects).toEqual(before);
    if (fault !== "session-replaced") expect(f.effects).not.toContain("continuation");
  });
  it.each(["checkout", "authority"] as const)("rejects changed prepared %s before retirement", async field => {
    const f = await proceduralFixture(), prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    await writeFile(field === "checkout" ? join(f.admission.cwd, "new-work.txt") : f.admission.authority.path, "changed");
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held"); expect(f.effects).toEqual([]);
  });
  it("preserves the existing operator-off switch and rejects another Kontor lifecycle claim", async () => {
    const f = await proceduralFixture();
    const switchPath = join(f.root, "plugin-config", "kontor", "create.json"); await makeDirectory(join(f.root, "plugin-config", "kontor"), { recursive: true });
    await writeFile(switchPath, JSON.stringify({ create: "off" }));
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow("switched off");
    await rm(switchPath);
    const claimRoot = join(f.root, "session-lifecycle-v1"); await makeDirectory(claimRoot);
    await writeFile(join(claimRoot, `intent-${takeoverHash(f.admission.sourceId)}.json`), JSON.stringify({ action: "archive" }));
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow("Another Kontor lifecycle action");
    expect(f.effects).toEqual([]);
  });
  it.each(["kontor", "unknown-orchestrator"])("refuses conflicting orchestration claims without a direct fallback (%s)", async orchestration => {
    const f = await proceduralFixture(); f.source.labels = { orchestration };
    f.admission.sourceRevision = lifecycleRevision(f.source);
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow("Governed or conflicting ownership");
    expect(f.effects).toEqual([]);
  });
  it("admits only one operation per exact source and refuses changed operation contents", async () => {
    const f = await proceduralFixture(); await f.service.prepare(await f.ref("admission.json", f.admission));
    await expect(f.service.prepare(await f.ref("changed.json", { ...f.admission, title: "changed" }))).rejects.toThrow();
    const operationId = randomUUID(), second = { ...f.admission, operationId };
    for (const key of ["handoff", "authority", "plan", "accounting"] as const) second[key] = await f.ref(`second-${key}.json`, { ...JSON.parse(await readFile(f.admission[key].path, "utf8")), operationId });
    await expect(f.service.prepare(await f.ref("second.json", second))).rejects.toThrow("Another operation already owns this source");
    expect(f.effects).toEqual([]);
  });
});

function parentOfProcedural(value: Agent) { return value.labels?.["paseo.parent-agent-id"] ?? null; }


describe("R11 source review regressions", () => {
  it.each(["stateRoot", "placementRoots"] as const)("rejects configured %s governed placement before timeline hydration", async kind => {
    const f = await proceduralFixture();
    vi.stubEnv(kind === "stateRoot" ? "KONTOR_PLUGIN_STATE_ROOT" : "KONTOR_PLUGIN_PLACEMENT_ROOTS", f.root);
    const previous = f.client.agents.ref, historyReads: string[] = [];
    f.client.agents.ref = (id) => { const handle = previous(id); return { ...handle, timeline: { ...handle.timeline, refetch: async (...args) => { historyReads.push(handle.id); return handle.timeline.refetch(...args); } } }; };
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow("governed ownership cannot be ruled out");
    expect(f.effects).toEqual([]); expect(historyReads).toEqual([]);
  });
  it("keeps ordinary free checkout outside configured roots valid and rechecks placement before effects", async () => {
    const f = await proceduralFixture();
    vi.stubEnv("KONTOR_PLUGIN_STATE_ROOT", join(f.root, "other-state"));
    vi.stubEnv("KONTOR_PLUGIN_PLACEMENT_ROOTS", join(f.root, "other-placement"));
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect(prepared.receipt.phase).toBe("prepared");
    vi.stubEnv("KONTOR_PLUGIN_PLACEMENT_ROOTS", f.root);
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held");
    expect(f.effects).toEqual([]);
  });
  it("allows ordinary free takeover outside both configured governed root categories", async () => {
    const f = await proceduralFixture();
    vi.stubEnv("KONTOR_PLUGIN_STATE_ROOT", join(f.root, "other-state"));
    vi.stubEnv("KONTOR_PLUGIN_PLACEMENT_ROOTS", join(f.root, "other-placement"));
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("active");
  });
  it("reconciles lost continuation reply and settles only preserved child claims without resend", async () => {
    const f = await proceduralFixture("continuation"), childId = randomUUID();
    f.agents.set(childId, agent(childId, { labels: { "paseo.parent-agent-id": f.admission.sourceId }, status: "running" })); f.admission.children = [childId];
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held");
    const childClaim = join(f.root, "session-lifecycle-v1", `intent-${takeoverHash(childId)}.json`), sourceClaim = join(f.root, "session-lifecycle-v1", `intent-${takeoverHash(f.admission.sourceId)}.json`);
    expect(existsSync(childClaim)).toBe(true); const sourceBytes = await readFile(sourceClaim, "utf8"), before = [...f.effects];
    const checked = await createProceduralTakeover(f.client, f.store, "fixture-host").check(f.admission.operationId);
    expect(checked.phase).toBe("active"); expect(existsSync(childClaim)).toBe(false);
    expect(await readFile(sourceClaim, "utf8")).toBe(sourceBytes); expect(f.effects).toEqual(before);
  });
  it("serializes concurrent correlated checks without deleting a later foreign claim", async () => {
    const f = await proceduralFixture("continuation"), childId = randomUUID();
    f.agents.set(childId, agent(childId, { labels: { "paseo.parent-agent-id": f.admission.sourceId }, status: "running" })); f.admission.children = [childId];
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash);
    const childClaim = join(f.root, "session-lifecycle-v1", `intent-${takeoverHash(childId)}.json`), original = JSON.parse(await readFile(childClaim, "utf8")), before = [...f.effects];
    const services = [f.service, createProceduralTakeover(f.client, f.store, "fixture-host")];
    expect((await Promise.all(services.map(service => service.check(f.admission.operationId)))).map(result => result.phase)).toEqual(["active", "active"]);
    expect(existsSync(childClaim)).toBe(false);
    const foreign = JSON.stringify({ ...original, reviewId: randomUUID(), token: randomUUID() }); await writeFile(childClaim, foreign);
    expect((await services[0]!.check(f.admission.operationId)).phase).toBe("active");
    expect(await readFile(childClaim, "utf8")).toBe(foreign); expect(f.effects).toEqual(before);
  });
  it("settles an interrupted active cleanup idempotently after reconstruction", async () => {
    const f = await proceduralFixture(), childId = randomUUID();
    f.agents.set(childId, agent(childId, { labels: { "paseo.parent-agent-id": f.admission.sourceId }, status: "running" })); f.admission.children = [childId];
    const childClaim = join(f.root, "session-lifecycle-v1", `intent-${takeoverHash(childId)}.json`); let ownedClaim = "";
    const previous = f.client.agents.ref;
    f.client.agents.ref = (id) => { const handle = previous(id); return { ...handle, run: async (...args) => { const result = await handle.run(...args); if (id !== f.admission.sourceId) ownedClaim = await readFile(childClaim, "utf8"); return result; } }; };
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("active");
    // Model the durable checkpoint after active was saved but before child-claim unlink completed.
    await writeFile(childClaim, ownedClaim); const before = [...f.effects];
    const restarted = createProceduralTakeover(f.client, f.store, "fixture-host");
    expect((await restarted.check(f.admission.operationId)).phase).toBe("active"); expect(existsSync(childClaim)).toBe(false);
    expect((await restarted.check(f.admission.operationId)).phase).toBe("active"); expect(f.effects).toEqual(before);
  });
  it.each(["foreign", "foreign-token", "uncertain"] as const)("preserves a %s child claim during correlated reconciliation", async kind => {
    const f = await proceduralFixture("continuation"), childId = randomUUID();
    f.agents.set(childId, agent(childId, { labels: { "paseo.parent-agent-id": f.admission.sourceId }, status: "running" })); f.admission.children = [childId];
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held");
    const childClaim = join(f.root, "session-lifecycle-v1", `intent-${takeoverHash(childId)}.json`);
    const original = JSON.parse(await readFile(childClaim, "utf8"));
    const changed = kind === "foreign" ? JSON.stringify({ ...original, reviewId: randomUUID(), fingerprint: takeoverHash("foreign-operation"), token: randomUUID() }) : kind === "foreign-token" ? JSON.stringify({ ...original, token: randomUUID() }) : "{";
    await writeFile(childClaim, changed); const before = [...f.effects];
    const checked = await createProceduralTakeover(f.client, f.store, "fixture-host").check(f.admission.operationId);
    expect(checked.phase).toBe(kind === "uncertain" ? "held" : "active");
    expect(await readFile(childClaim, "utf8")).toBe(changed); expect(f.effects).toEqual(before);
  });
});

async function reviewLeadFixture(promoted = false, state: "promoted" | "promoting" | "unknown" = "promoted") {
  const f = await proceduralFixture(undefined, "kontor");
  const title = promoted ? "LSA • ADHOC • Fixture" : "LSA • ASMA-1 • Fixture";
  const persona = personaVersion(promoted ? "lsa-ad-hoc" : "lsa-bound", true);
  f.source.title = title;
  f.source.labels = { ...f.source.labels, role: "LSA", persona, "title.auto": title, ...(promoted ? {} : { epic: "ASMA-1" }) };
  f.admission.title = title;
  f.admission.sourceRevision = lifecycleRevision(f.source);
  const ref = f.client.workspaces.ref.bind(f.client.workspaces);
  f.client.workspaces.ref = ((id: string) => {
    const handle = ref(id);
    return { ...handle, refresh: async () => ({ ...await handle.refresh(), id, name: promoted ? "ADHOC" : "LSA", title: null, archivingAt: null }) };
  }) as typeof f.client.workspaces.ref;
  const root = createStateRoot();
  if (promoted) {
    const stamp = new Date().toISOString();
    const record = promotionRecordSchema.parse({ schemaVersion: 1, requestId: randomUUID(), agentId: f.source.id, epic: "ASMA-1", branch: "promote", state,
      native: { workspaceId: f.admission.workspaceId, workspace: "ADHOC", title, titleAuto: title, observedAt: stamp, revision: f.source.updatedAt },
      logical: { workspace: "LSA", node: "epic:ASMA-1", bindingRevision: "a".repeat(64) }, persona: { issued: persona, effective: persona },
      canonicalAgentId: f.source.id, summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "Synthetic review fixture", createdAt: stamp, updatedAt: stamp });
    await makeDirectory(join(root, "promotion-v1"), { recursive: true });
    await writeFile(join(root, "promotion-v1", `${record.requestId}.json`), JSON.stringify(record));
  }
  return { ...f, canonicalRoot: root };
}
describe("R11 canonical refusal regressions", () => {
  it("holds a promotion appearing after prepare across restart without effects", async () => {
    const f = await reviewLeadFixture(true, "promoting"), root = join(f.canonicalRoot, "promotion-v1");
    const name = (await readdir(root))[0]!, record = await readFile(join(root, name), "utf8");
    await rm(root, { recursive: true }); delete f.source.labels!.role;
    f.admission.sourceRevision = lifecycleRevision(f.source);
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    await makeDirectory(root, { recursive: true }); await writeFile(join(root, name), record);
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held");
    expect((await createProceduralTakeover(f.client, f.store, "fixture-host").check(f.admission.operationId)).phase).toBe("held");
    expect(f.effects).toEqual([]);
  });

  it.each([false, true])("refuses canonical lead before effects (promoted=%s)", async (promoted) => {
    const f = await reviewLeadFixture(promoted);
    expect((await discoverCanonical(f.client, f.canonicalRoot, "ASMA-1")).kind).toBe("one");
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow(/[Cc]anonical|[Pp]romotion/);
    expect(f.effects).toEqual([]);
  });
  it.each(["promoting", "unknown", "promoted"] as const)("refuses promotion %s without LSA label", async (state) => {
    const f = await reviewLeadFixture(true, state);
    delete f.source.labels!.role; f.admission.sourceRevision = lifecycleRevision(f.source);
    await expect(f.service.prepare(await f.ref("admission.json", f.admission))).rejects.toThrow(/[Pp]romotion/);
    expect(f.effects).toEqual([]);
  });
  it.each(["unreadable", "malformed", "role"])("holds ownership drift after prepare: %s", async (drift) => {
    const f = await proceduralFixture();
    const prepared = await f.service.prepare(await f.ref("admission.json", f.admission));
    const root = join(createStateRoot(), "promotion-v1");
    if (drift === "role") f.source.labels!.role = "LSA";
    else if (drift === "unreadable") { await makeDirectory(createStateRoot(), { recursive: true }); await writeFile(root, "unreadable store"); }
    else { await makeDirectory(root, { recursive: true }); await writeFile(join(root, "bad.json"), "invalid promotion record"); }
    expect((await f.service.execute(f.admission.operationId, prepared.receipt.admissionHash)).phase).toBe("held");
    expect(f.effects).toEqual([]);
    expect((await createProceduralTakeover(f.client, f.store, "fixture-host").check(f.admission.operationId)).phase).toBe("held");
    expect(f.effects).toEqual([]);
  });
});

describe("R11 selected prepared import", () => {
  it("rejects a wrong selected row before history or operation claim", async () => {
    const f = await proceduralFixture();
    await expect(f.service.importSelected(await f.ref("admission.json", f.admission), randomUUID())).rejects.toThrow("selected session");
    expect(f.effects).toEqual([]);
    expect((await f.service.importSelected(await f.ref("admission.json", f.admission), f.source.id)).canExecute).toBe(true);
  });
  it("rejects unavailable and invalid SHA references", async () => {
    const f = await proceduralFixture(), ref = await f.ref("admission.json", f.admission);
    await expect(f.service.importSelected({ ...ref, sha256: "0".repeat(64) }, f.source.id)).rejects.toThrow("changed");
    await expect(f.service.importSelected({ ...ref, path: join(f.store, "absent") }, f.source.id)).rejects.toThrow();
    expect(f.effects).toEqual([]);
  });
  it("does not offer confirmation after an interrupted execution claim", async () => {
    const f = await proceduralFixture(), ref = await f.ref("admission.json", f.admission);
    const prepared = await f.service.importSelected(ref, f.source.id);
    await writeFile(join(f.store, "operations", f.admission.operationId, "execution.claim"), "unknown");
    expect((await createProceduralTakeover(f.client, f.store, "fixture-host").importSelected(ref, f.source.id)).canExecute).toBe(false);
    expect(prepared.receipt.phase).toBe("prepared"); expect(f.effects).toEqual([]);
  });
  it("retains canonical refusal through selected import", async () => {
    const f = await reviewLeadFixture(true, "promoting");
    await expect(f.service.importSelected(await f.ref("admission.json", f.admission), f.source.id)).rejects.toThrow("Promotion");
    expect(f.effects).toEqual([]);
  });
});
