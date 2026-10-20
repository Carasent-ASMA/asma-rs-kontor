import type { PaseoApi } from "@getpaseo/client";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema, type OrganizationSettings } from "../../../paseo-plugin/shared/organization";
import { baseSessions, roleSessions, scrollSessions, WORKSPACES, type FixtureSession } from "../fixtures/sessions";
import { SOURCE_READS } from "../fixtures/sources";
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
const inventory = { none: () => [], large: scrollSessions, roles: roleSessions, base: baseSessions }[scenario.inventory];
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
  const rows = scenario.inventory === "none" ? [] : WORKSPACES;
  const hasMore = start + limit < rows.length;
  return Promise.resolve({ requestId: `req_${Date.now()}`, entries: rows.slice(start, start + limit), pageInfo: { hasMore, nextCursor: hasMore ? String(start + limit) : null } });
}

/** Only the two reads the plugin performs; any other call is logged and refused. */
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
  agents: refusingProxy("paseo.agents", { list: listAgents }),
  workspaces: refusingProxy("paseo.workspaces", { list: listWorkspaces }),
}) as PaseoApi;

// Configured sources (plugin RPC) --------------------------------------------

export async function invokeRpc(method: string, input: unknown): Promise<unknown> {
  await new Promise((resolve) => setTimeout(resolve, 60));
  if (hostStatus !== "online") throw new Error("Prototype host is offline (simulated).");
  const reads = scenario.sources ? SOURCE_READS : [];
  switch (method) {
    case "sources.list":
      return { sources: reads.map((read) => read.source), diagnostics: [] };
    case "sources.read": {
      const id = (input as { sourceId: string }).sourceId;
      const read = reads.find((value) => value.source.id === id);
      if (!read) throw new Error(`Unknown fixture source ${id}`);
      return { ...read, readAt: new Date().toISOString() };
    }
    case "sources.associations":
      return { status: "ready", associations: { agt_swe_99103: ["plan-99101"] }, sources: reads.map((read) => read.source), error: null };
    default:
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
