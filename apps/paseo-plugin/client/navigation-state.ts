import { readSessionView, writeSessionView, type SessionViewState } from "./session-view-store";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";

export const DESTINATIONS = ["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery", "Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"] as const;
export type Destination = (typeof DESTINATIONS)[number];
export type KontorRoute = { tab: Destination; projectId: string | null; epicKey: string | null; sourceId: string | null; originAgentId: string | null; sourceRevision: string | null };
export const INITIAL_ROUTE: KontorRoute = { tab: "Sessions", projectId: null, epicKey: null, sourceId: null, originAgentId: null, sourceRevision: null };
/** Context a tab family last showed: Overview keeps its own portfolio selection, work pages share one, Sessions opens unscoped. */
export type ContextScope = Pick<KontorRoute, "projectId" | "epicKey" | "sourceId" | "sourceRevision" | "originAgentId">;
export const NO_SCOPE: ContextScope = { projectId: null, epicKey: null, sourceId: null, sourceRevision: null, originAgentId: null };
type HistoryEntry = { route: KontorRoute; query: string; offset: number; sessions: SessionViewState; overview: ContextScope; work: ContextScope; views: Record<string, unknown> };
type HostNavigation = { route: KontorRoute; back: HistoryEntry[]; queries: Record<string, string>; offsets: Record<string, number>; overview: ContextScope; work: ContextScope; views: Record<string, unknown> };
const hosts = new Map<string, HostNavigation>();
export function hostNavigation(hostId: string): HostNavigation {
  let state = hosts.get(hostId);
  if (!state) { state = { route: { ...INITIAL_ROUTE }, back: [], queries: {}, offsets: {}, overview: { ...NO_SCOPE }, work: { ...NO_SCOPE }, views: {} }; hosts.set(hostId, state); }
  return state;
}
export function scopeOf(route: KontorRoute): ContextScope {
  return { projectId: route.projectId, epicKey: route.epicKey, sourceId: route.sourceId, sourceRevision: route.sourceRevision, originAgentId: route.originAgentId };
}
function remember(state: HostNavigation): void {
  const scope = scopeOf(state.route);
  if (state.route.tab === "Overview") {
    state.overview = scope;
    // As in the prototype, a scoped portfolio choice also becomes the work context.
    if (scope.projectId !== null || scope.epicKey !== null || scope.sourceId !== null) state.work = scope;
  } else if (state.route.tab !== "Sessions") state.work = scope;
}
/** The scope a tab press restores for its family. */
export function tabScope(hostId: string, tab: Destination): ContextScope {
  const state = hostNavigation(hostId);
  if (tab === "Sessions") return { ...NO_SCOPE };
  return { ...(tab === "Overview" ? state.overview : state.work) };
}
export function navigate(hostId: string, patch: Partial<KontorRoute>): KontorRoute {
  const state = hostNavigation(hostId);
  const next = { ...state.route, ...patch };
  if (patch.sourceId !== undefined && patch.sourceId !== state.route.sourceId && patch.sourceRevision === undefined) next.sourceRevision = null;
  if (next.sourceId === null) next.sourceRevision = null;
  if (JSON.stringify(next) !== JSON.stringify(state.route)) {
    state.back = [...state.back.slice(-39), { route: state.route, query: state.queries[routeViewKey(state.route)] ?? "", offset: state.offsets[routeViewKey(state.route)] ?? 0, sessions: { ...readSessionView(sessionScopeKey(hostId, state.route)) }, overview: state.overview, work: state.work, views: { ...state.views } }];
    state.route = next;
    remember(state);
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
    state.overview = previous.overview; state.work = previous.work; state.views = previous.views;
    writeSessionView(sessionScopeKey(hostId, previous.route), previous.sessions);
  }
  return state.route;
}
/** Page-owned view state (reader mode, disclosures, drill-down) keyed by route; Back restores the snapshot taken when leaving. */
export function readPageView<T>(hostId: string, route: KontorRoute, fallback: T): T {
  const value = hostNavigation(hostId).views[routeViewKey(route)];
  return value === undefined ? fallback : value as T;
}
export function writePageView(hostId: string, route: KontorRoute, value: unknown): void {
  hostNavigation(hostId).views[routeViewKey(route)] = value;
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
  return { query: state.queries[key] ?? "", offset: state.offsets[key] ?? 0 };
}
export function writeRouteView(hostId: string, route: KontorRoute, patch: { query?: string; offset?: number }): void {
  const state = hostNavigation(hostId);
  const key = routeViewKey(route);
  if (patch.query !== undefined) state.queries[key] = patch.query;
  if (patch.offset !== undefined) state.offsets[key] = patch.offset;
}
export function refreshSourceRevision(hostId: string, revision: string | null): KontorRoute {
  const state = hostNavigation(hostId);
  state.route = { ...state.route, sourceRevision: revision }; remember(state);
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
  state.route = { ...state.route, sourceId, sourceRevision: revision }; remember(state);
  return state.route;
}
/**
 * Header context: an exact plan, the single plan bound to the selected epic, or a truthful
 * unselected/ambiguous state. The key is shown only when it is a confirmed Jira key.
 */
export function contextHeading(route: KontorRoute, sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, allowedIds: readonly string[] | null, projectName: string | null): { title: string; key: string | null; plan: ReadSourceSummary | null; plans: number } {
  const jira = (value: string | null | undefined) => value && /^ASMA-[1-9]\d*$/.test(value) ? value : null;
  if (route.sourceId) {
    const plan = sources.find((source) => source.id === route.sourceId && source.kind === "plan") ?? null;
    return { title: plan?.title ?? "Source unavailable", key: plan ? jira(reads[plan.id]?.graph?.epic.jira ?? plan.epic) : null, plan, plans: plan ? 1 : 0 };
  }
  if (route.epicKey) {
    const plans = contextPlanSources(route, sources, reads, allowedIds);
    if (plans.length === 1) return { title: plans[0]!.title, key: jira(route.epicKey), plan: plans[0]!, plans: 1 };
    return { title: plans.length ? `Choose plan · ${plans.length} linked` : jira(route.epicKey) ? "No plan linked" : route.epicKey, key: jira(route.epicKey), plan: null, plans: plans.length };
  }
  if (route.projectId) return { title: projectName ?? "Project unavailable", key: null, plan: null, plans: 0 };
  return { title: "All projects", key: null, plan: null, plans: 0 };
}
