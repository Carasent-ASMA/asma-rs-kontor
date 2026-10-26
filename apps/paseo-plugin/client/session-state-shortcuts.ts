import { providerBlockedView, type ProviderRefusalReads } from "./provider-blocked-filter";
import type { DirectoryState } from "./session-directory";
import { sessionStateCounts } from "./session-state-counts";
import { matchesMembership, matchesQuery, type SessionMembership, type StatusFilter } from "./session-view";
import { clearSessionFilterPatch, type SessionViewState } from "./session-view-store";

/** What the switcher shows, handed whole to Kontor's Sessions: this host, every project, one state, its search and membership. */
export type SwitchView = { readonly status: StatusFilter; readonly query: string; readonly membership: SessionMembership };
const NO_REFUSAL_READS: ProviderRefusalReads = new Map();

/**
 * Same evidence boundary as Sessions: unchecked native errors are unknown, never inferred provider blocks. A search
 * refines every count to its matches of the state's total.
 */
export function startSwitchCounts(state: DirectoryState, freshness: string, { query = "", recent = [], favorites = [], reads = NO_REFUSAL_READS }: {
  query?: string; recent?: readonly string[]; favorites?: readonly string[]; reads?: ProviderRefusalReads;
} = {}) {
  const universe = state.entries.filter((entry) => matchesMembership(entry, "sessions"));
  const candidates = query.trim() ? universe.filter((entry) => matchesQuery(entry, query)) : universe;
  const input = { state, includeArchived: false, freshness: freshness === "Live" && state.inventoryComplete !== true ? "Reading" : freshness, blocked: null };
  const providerBlocked = providerBlockedView({ ...input, candidates, reads });
  return sessionStateCounts({ ...input, universe, candidates, favorites, recent, refined: candidates !== universe, providerBlocked: providerBlocked.count });
}

/** Only transient filters change, to exactly the switcher's view. Saved presentation and the chat composer are outside this hand-off. */
export function switchViewPatch(view: SessionViewState, next: SwitchView): Partial<SessionViewState> {
  return { ...clearSessionFilterPatch(view), status: next.status, query: next.query, membership: next.membership,
    includeArchived: next.membership === "archived", work: "mine", scrollOffset: 0, peek: null };
}
