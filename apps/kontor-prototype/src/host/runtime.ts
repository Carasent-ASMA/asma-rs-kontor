import type { PaseoApi } from "@getpaseo/client";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema, type OrganizationSettings } from "../../../paseo-plugin/shared/organization";
import { DEFAULT_KEYBOARD_SETTINGS, keyboardSettingsSchema, type KeyboardSettings } from "../../../paseo-plugin/shared/keyboard";
import { baseSessions, deskSessions, DESK_PROJECTS, DESK_WORKSPACES, roleSessions, scrollSessions, timeSessions, WORKSPACES, type FixtureSession } from "../fixtures/sessions";
import { modelUsageSessions } from "../fixtures/model-usage";
import { lifecycleSessions } from "../fixtures/lifecycle";
import { recoverySessions } from "../fixtures/recovery";
import { fixtureLifecycle } from "./session-lifecycle";
import { fixtureCreate } from "./session-create";
import { integrationSessions } from "../fixtures/integration";
import { SOURCE_READS } from "../fixtures/sources";
import { responsibilityFixture } from "../fixtures/responsibility";
import { SCENARIOS, type Scenario } from "../scenarios";
import { logHostEvent } from "./event-log";

/**
 * The fixture host: one fictional Paseo server whose sessions, workspaces,
 * configured sources and plugin settings live in this page's memory. Nothing
 * here opens a network connection; a reload resets everything.
 */
export const HOST_ID = "srv_prototype";
export const HOST_LABEL = "Prototype host";

const params = new URLSearchParams(window.location.search);
export const scenario: Scenario = SCENARIOS.find((value) => value.id === params.get("scenario")) ?? SCENARIOS[0]!;

type HostStatus = "idle" | "connecting" | "online" | "offline" | "error";
let hostStatus: HostStatus = "online";
const hostListeners = new Set<() => void>();
export function currentHostStatus(): HostStatus { return hostStatus; }
export function setHostStatus(next: HostStatus): void {
  hostStatus = next;
  for (const listener of hostListeners) listener();
}
export function subscribeHost(listener: () => void): () => void {
  hostListeners.add(listener);
  return () => hostListeners.delete(listener);
}

// Sessions ------------------------------------------------------------------

const sessions = new Map<string, FixtureSession>();
const inventory = { none: () => [], large: scrollSessions, roles: roleSessions, times: timeSessions, base: baseSessions, usage: modelUsageSessions, desks: deskSessions, lifecycle: lifecycleSessions, integration: integrationSessions, recovery: recoverySessions }[scenario.inventory];
for (const value of inventory()) sessions.set(value.agent.id, value);

type Observer = { snapshot?: (value: unknown) => void; update?: (message: unknown) => void; error?: (error: unknown) => void };
const observers = new Set<Observer>();

export function fixtureSessions(): FixtureSession[] {
  return [...sessions.values()];
}

/** Applies a simulated native change and streams it like an agent_update. */
export function simulateUpdate(agentId: string, patch: Partial<FixtureSession["agent"]>, note: string): void {
  const current = sessions.get(agentId);
  if (!current) return;
  const next: FixtureSession = { ...current, agent: { ...current.agent, ...patch, updatedAt: new Date().toISOString() } };
  sessions.set(agentId, next);
  logHostEvent("simulated.update", `${agentId}: ${note}`);
  for (const observer of observers) observer.update?.({ type: "agent_update", payload: { kind: "upsert", agent: next.agent, project: next.project } });
}

function listAgents(options: { filter?: { includeArchived?: boolean }; page?: { limit?: number; cursor?: string }; subscribe?: object } = {}) {
  if (hostStatus !== "online") return Promise.reject(new Error("Prototype host is offline (simulated)."));
  const includeArchived = options.filter?.includeArchived === true;
  const all = [...sessions.values()]
    .filter((value) => includeArchived || !value.agent.archivedAt)
    .sort((left, right) => right.agent.updatedAt.localeCompare(left.agent.updatedAt));
  const limit = options.page?.limit ?? 200;
  const start = Number(options.page?.cursor ?? 0);
  const page = all.slice(start, start + limit);
  const hasMore = start + limit < all.length;
  const result: Record<string, unknown> = {
    requestId: `req_${Date.now()}`,
    entries: page.map((value) => ({ agent: value.agent, project: value.project })),
    pageInfo: { hasMore, nextCursor: hasMore ? String(start + limit) : null },
  };
  if (options.subscribe) {
    let observer: Observer | null = null;
    result.subscriptionId = `sub_${Date.now()}`;
    result.subscription = {
      subscribe(next: Observer) {
        observer = next; observers.add(next);
        queueMicrotask(() => next.snapshot?.(result));
        return () => { if (observer) observers.delete(observer); };
      },
      release() { if (observer) observers.delete(observer); return Promise.resolve(); },
    };
  }
  return Promise.resolve(result);
}

function listWorkspaces(options: { page?: { limit?: number; cursor?: string } } = {}) {
  if (hostStatus !== "online") return Promise.reject(new Error("Prototype host is offline (simulated)."));
  const limit = options.page?.limit ?? 200;
  const start = Number(options.page?.cursor ?? 0);
  const rows = scenario.inventory === "none" ? [] : scenario.inventory === "desks" ? DESK_WORKSPACES : scenario.inventory === "integration" ? [...WORKSPACES, ...DESK_WORKSPACES] : WORKSPACES;
  const hasMore = start + limit < rows.length;
  return Promise.resolve({ requestId: `req_${Date.now()}`, entries: rows.slice(start, start + limit), pageInfo: { hasMore, nextCursor: hasMore ? String(start + limit) : null } });
}

/** Only the native reads the plugin performs; any other call is logged and refused. */
function unsupported(path: string): never {
  logHostEvent("paseo.unsupported", path);
  throw new Error(`${path} is not available in the review prototype.`);
}
function refusingProxy(path: string, known: Record<string, unknown>): unknown {
  return new Proxy(known, {
    get(target, property) {
      if (typeof property !== "string") return undefined;
      if (property in target) return target[property];
      if (property === "then") return undefined;
      return () => unsupported(`${path}.${property}`);
    },
  });
}
export const fakePaseo = refusingProxy("paseo", {
  projects: refusingProxy("paseo.projects", { list: async () => {
    if (hostStatus !== "online") throw new Error("Prototype host is offline (simulated).");
    const base = [...new Map(WORKSPACES.map(workspace => [workspace.projectId, { projectId: workspace.projectId,
      projectKey: workspace.projectId, projectDisplayName: workspace.projectDisplayName, projectCustomName: workspace.projectCustomName,
      projectRootPath: `/fixture/${workspace.projectId}`, projectKind: "non_git", projectCustomIconRevision: null, projectIconRevision: "automatic:none:v1" }])).values()];
    return { requestId: "fixture-projects", projects: scenario.inventory === "none" ? [] : scenario.inventory === "desks" ? DESK_PROJECTS : scenario.inventory === "integration" ? [...base, ...DESK_PROJECTS] : base };
  } }),
  agents: refusingProxy("paseo.agents", { list: listAgents }),
  workspaces: refusingProxy("paseo.workspaces", { list: listWorkspaces }),
}) as PaseoApi;

// Configured sources (plugin RPC) --------------------------------------------

const lifecycleRpc = fixtureLifecycle(fixtureSessions, simulateUpdate);
const createRpc = fixtureCreate(scenario.inventory === "integration" || scenario.inventory === "desks", (request, title, workspaceId) => {
  const id = `agt_fixture_create_${request.requestId}`;
  const seed = baseSessions()[0]!;
  const workspaceName = request.start === "ad-hoc" ? "ADHOC" : request.role;
  const row: FixtureSession = { ...seed, agent: { ...seed.agent, id, title, status: "idle", requiresAttention: false, attentionReason: null,
    workspaceId, cwd: "/fixture/prj_proof_adam", provider: request.provider, model: request.model,
    effectiveThinkingOptionId: request.effort ?? undefined, currentModeId: request.modeId ?? undefined, archivedAt: null,
    labels: { role: request.role, scope: request.issueKey ?? "ADHOC", "create.request": request.requestId }, createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() },
    project: { projectKey: "prj_proof_adam", projectName: "DESK • ADAM", workspaceName } };
  sessions.set(id, row);
  for (const observer of observers) observer.update?.({ type: "agent_update", payload: { kind: "upsert", agent: row.agent, project: row.project } });
  logHostEvent("simulated.update", `Created ${id} in fixture memory only`);
  return id;
});
export async function invokeRpc(method: string, input: unknown): Promise<unknown> {
  await new Promise((resolve) => setTimeout(resolve, 60));
  if (hostStatus !== "online") throw new Error("Prototype host is offline (simulated).");
  if (method === "sessions.create-options" || method === "sessions.create" || method === "sessions.create-check") return createRpc(method, input);
  if (method.startsWith("sessions.")) return lifecycleRpc(method, input);
  const reads = scenario.sources ? SOURCE_READS : [];
  const catalog = { sources: reads.map((read) => read.source), diagnostics: [], registry: { path: "fixtures/workstreams.json", revision: "fixture-registry-1", workstreams: { kontor: { name: "Kontor" } }, desks: { review: { name: "Review desk" } }, diagnostics: [] } };
  switch (method) {
    case "sources.list":
      return catalog;
    case "sources.summaries":
      logHostEvent("sources.summaries", `${reads.length} metadata-only fixture reads`);
      return { ...catalog, reads: Object.fromEntries(reads.map(read => [read.source.id, { ...read, content: null, graph: null, records: null }])) };
    case "responsibility.read":
      return responsibilityFixture();
    case "kontor.status":
      return { observation: "ready", composition: "composed", checkedAt: new Date().toISOString() };
    case "sources.read": {
      const id = (input as { sourceId: string }).sourceId;
      const read = reads.find((value) => value.source.id === id);
      if (!read) throw new Error(`Unknown fixture source ${id}`);
      logHostEvent("sources.read", id);
      return { ...read, readAt: new Date().toISOString() };
    }
    case "sources.associations":
      return { status: "ready", associations: { agt_swe_99103: ["plan-99101"] }, sources: reads.map((read) => read.source), error: null };
    default:
      logHostEvent("paseo.unsupported", `RPC ${method}`);
      throw new Error(`RPC ${method} is not served by the review prototype.`);
  }
}

// Plugin settings --------------------------------------------------------------

type SettingsListener = () => void;
let settingsValues: OrganizationSettings = organizationSettingsSchema.parse({ ...DEFAULT_ORGANIZATION_SETTINGS, ...scenario.settings });
let settingsRevision = 1;
let saving = false;
const settingsListeners = new Set<SettingsListener>();
export function settingsSnapshot() {
  return { values: settingsValues, revision: `rev-${settingsRevision}`, saving };
}
export function subscribeSettings(listener: SettingsListener): () => void {
  settingsListeners.add(listener);
  return () => settingsListeners.delete(listener);
}
/** Stands in for the host's settings store: validates, bumps the revision, and logs every save. */
export async function saveSettings(values: unknown, revision: string): Promise<boolean> {
  if (revision !== `rev-${settingsRevision}`) return false;
  const parsed = organizationSettingsSchema.safeParse(values);
  if (!parsed.success) return false;
  const changed = Object.keys(parsed.data).filter((key) => JSON.stringify(parsed.data[key as keyof OrganizationSettings]) !== JSON.stringify(settingsValues[key as keyof OrganizationSettings]));
  saving = true; for (const listener of settingsListeners) listener();
  await new Promise((resolve) => setTimeout(resolve, 40));
  settingsValues = parsed.data; settingsRevision += 1; saving = false;
  logHostEvent("settings.save", `rev-${settingsRevision} · changed ${changed.join(", ") || "nothing"}${changed.includes("expandedGroups") ? ` · expandedGroups ${JSON.stringify(parsed.data.expandedGroups)}` : ""}`);
  for (const listener of settingsListeners) listener();
  return true;
}

// The keyboard document is a second settings file, as on the host: saving one leaves the other alone.
let keyboardValues: KeyboardSettings = keyboardSettingsSchema.parse({ ...DEFAULT_KEYBOARD_SETTINGS, ...scenario.keyboard });
let keyboardRevision = 1;
let keyboardSaving = false;
const keyboardListeners = new Set<SettingsListener>();
export function keyboardSnapshot() {
  return { values: keyboardValues, revision: `rev-${keyboardRevision}`, saving: keyboardSaving };
}
export function subscribeKeyboard(listener: SettingsListener): () => void {
  keyboardListeners.add(listener);
  return () => keyboardListeners.delete(listener);
}
export async function saveKeyboard(values: unknown, revision: string): Promise<boolean> {
  if (revision !== `rev-${keyboardRevision}`) return false;
  const parsed = keyboardSettingsSchema.safeParse(values);
  if (!parsed.success) return false;
  keyboardSaving = true; for (const listener of keyboardListeners) listener();
  await new Promise((resolve) => setTimeout(resolve, 40));
  keyboardValues = parsed.data; keyboardRevision += 1; keyboardSaving = false;
  logHostEvent("keyboard.save", `rev-${keyboardRevision} · backShortcut ${parsed.data.backShortcut}`);
  for (const listener of keyboardListeners) listener();
  return true;
}


/** Disposable fault injection for the availability prototype; never exported by the plugin. */
Object.assign(window, { __kontorP1Fixtures: {
  availability(id: string, state: "readable" | "missing" | "unknown") {
    if (!id.startsWith("agt_recovery_")) throw new Error("Fictional recovery IDs only");
    const row = sessions.get(id); if (!row) throw new Error("Fixture not found");
    sessions.set(id, { ...row, agent: { ...row.agent, labels: { ...row.agent.labels, "fixture.availability": state } } });
  },
  read(id: string) { const a = sessions.get(id)?.agent; return a ? { id: a.id, archivedAt: a.archivedAt, status: a.status, persistence: a.persistence } : null; },
  settle(id: string) {
    if (id !== "agt_recovery_archive_failure") throw new Error("Fictional close-failure ID only");
    simulateUpdate(id, { status: "closed" }, "SIMULATED runtime retirement observed");
  },
} });
