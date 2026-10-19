export const DESTINATIONS = ["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery", "Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"] as const;
export type Destination = (typeof DESTINATIONS)[number];
export type KontorRoute = { tab: Destination; projectId: string | null; epicKey: string | null; sourceId: string | null; originAgentId: string | null };
export const INITIAL_ROUTE: KontorRoute = { tab: "Sessions", projectId: null, epicKey: null, sourceId: null, originAgentId: null };
type HostNavigation = { route: KontorRoute; back: KontorRoute[]; queries: Partial<Record<Destination, string>>; offsets: Partial<Record<Destination, number>> };
const hosts = new Map<string, HostNavigation>();
export function hostNavigation(hostId: string): HostNavigation {
  let state = hosts.get(hostId);
  if (!state) { state = { route: { ...INITIAL_ROUTE }, back: [], queries: {}, offsets: {} }; hosts.set(hostId, state); }
  return state;
}
export function navigate(hostId: string, patch: Partial<KontorRoute>): KontorRoute {
  const state = hostNavigation(hostId);
  const next = { ...state.route, ...patch };
  // Overview always represents the entire selected host.
  if (next.tab === "Overview") { next.projectId = null; next.epicKey = null; }
  if (JSON.stringify(next) !== JSON.stringify(state.route)) {
    state.back = [...state.back.slice(-39), state.route]; state.route = next;
  }
  return state.route;
}
export function goBack(hostId: string): KontorRoute {
  const state = hostNavigation(hostId);
  state.route = state.back.pop() ?? state.route;
  return state.route;
}
export function linkedPlans(associations: Readonly<Record<string, readonly string[]>>, agentId: string, sourceIds: ReadonlySet<string>): string[] {
  return [...new Set(associations[agentId] ?? [])].filter((id) => sourceIds.has(id));
}
