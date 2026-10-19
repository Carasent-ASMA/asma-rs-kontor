import { readSessionView, writeSessionView, type SessionViewState } from "./session-view-store";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";

export const DESTINATIONS = ["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery", "Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"] as const;
export type Destination = (typeof DESTINATIONS)[number];
export type KontorRoute = { tab: Destination; projectId: string | null; epicKey: string | null; sourceId: string | null; originAgentId: string | null; sourceRevision: string | null };
export const INITIAL_ROUTE: KontorRoute = { tab: "Sessions", projectId: null, epicKey: null, sourceId: null, originAgentId: null, sourceRevision: null };
type HistoryEntry = { route: KontorRoute; query: string; offset: number; documentOffset: number; sessions: SessionViewState };
type HostNavigation = { route: KontorRoute; back: HistoryEntry[]; queries: Record<string, string>; offsets: Record<string, number>; documentOffsets: Record<string, number> };
const hosts = new Map<string, HostNavigation>();
export function hostNavigation(hostId: string): HostNavigation {
  let state = hosts.get(hostId);
  if (!state) { state = { route: { ...INITIAL_ROUTE }, back: [], queries: {}, offsets: {}, documentOffsets: {} }; hosts.set(hostId, state); }
  return state;
}
export function navigate(hostId: string, patch: Partial<KontorRoute>): KontorRoute {
  const state = hostNavigation(hostId);
  const next = { ...state.route, ...patch };
  if (patch.sourceId !== undefined && patch.sourceId !== state.route.sourceId && patch.sourceRevision === undefined) next.sourceRevision = null;
  if (next.sourceId === null) next.sourceRevision = null;
  if (JSON.stringify(next) !== JSON.stringify(state.route)) {
    state.back = [...state.back.slice(-39), { route: state.route, query: state.queries[routeViewKey(state.route)] ?? "", offset: state.offsets[routeViewKey(state.route)] ?? 0, documentOffset: state.documentOffsets[routeViewKey(state.route)] ?? 0, sessions: { ...readSessionView(sessionScopeKey(hostId, state.route)) } }];
    state.route = next;
  }
  return state.route;
}
export function goBack(hostId: string): KontorRoute {
  const state = hostNavigation(hostId);
  const previous = state.back.pop();
  if (previous) {
    state.route = previous.route;
    state.queries[routeViewKey(previous.route)] = previous.query;
    state.offsets[routeViewKey(previous.route)] = previous.offset;
    state.documentOffsets[routeViewKey(previous.route)] = previous.documentOffset;
    writeSessionView(sessionScopeKey(hostId, previous.route), previous.sessions);
  }
  return state.route;
}
export function linkedPlans(associations: Readonly<Record<string, readonly string[]>>, agentId: string, sourceIds: ReadonlySet<string>): string[] {
  return [...new Set(associations[agentId] ?? [])].filter((id) => sourceIds.has(id));
}

export function routeViewKey(route: KontorRoute): string {
  return JSON.stringify([route.tab, route.projectId, route.epicKey, route.sourceId, route.sourceRevision, route.originAgentId]);
}
export function sessionScopeKey(hostId: string, route: KontorRoute): string {
  return JSON.stringify([hostId, "sessions", route.projectId, route.epicKey]);
}
export function readRouteView(hostId: string, route = hostNavigation(hostId).route) {
  const state = hostNavigation(hostId);
  const key = routeViewKey(route);
  return { query: state.queries[key] ?? "", offset: state.offsets[key] ?? 0, documentOffset: state.documentOffsets[key] ?? 0 };
}
export function writeRouteView(hostId: string, route: KontorRoute, patch: { query?: string; offset?: number; documentOffset?: number }): void {
  const state = hostNavigation(hostId);
  const key = routeViewKey(route);
  if (patch.query !== undefined) state.queries[key] = patch.query;
  if (patch.offset !== undefined) state.offsets[key] = patch.offset;
  if (patch.documentOffset !== undefined) state.documentOffsets[key] = patch.documentOffset;
}
export function refreshSourceRevision(hostId: string, revision: string | null): KontorRoute {
  const state = hostNavigation(hostId);
  state.route = { ...state.route, sourceRevision: revision };
  return state.route;
}
/** Only explicit epic/project metadata or the exact canonical epic binds a source. */
export function contextPlanSources(route: KontorRoute, sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, allowedIds: readonly string[] | null = null): ReadSourceSummary[] {
  return sources.filter((source) => source.kind === "plan"
    && (allowedIds === null || allowedIds.includes(source.id))
    && (route.projectId === null || source.projectId === route.projectId)
    && (route.epicKey === null || (source.epic ?? reads[source.id]?.graph?.epic.jira) === route.epicKey));
}
export function resolveContextPlan(route: KontorRoute, sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, allowedIds: readonly string[] | null = null): KontorRoute {
  if (!["Plan", "Graph", "Board"].includes(route.tab) || route.sourceId !== null || route.epicKey === null) return route;
  const plans = contextPlanSources(route, sources, reads, allowedIds);
  if (plans.length !== 1) return route;
  return { ...route, sourceId: plans[0]!.id, sourceRevision: reads[plans[0]!.id]?.revision ?? null };
}
export function refreshContextSource(hostId: string, sourceId: string, revision: string | null): KontorRoute {
  const state = hostNavigation(hostId);
  state.route = { ...state.route, sourceId, sourceRevision: revision };
  return state.route;
}
