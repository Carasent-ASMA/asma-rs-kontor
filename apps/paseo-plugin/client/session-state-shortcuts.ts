import { providerBlockedView, type ProviderRefusalReads } from "./provider-blocked-filter";
import type { DirectoryState } from "./session-directory";
import { sessionStateCounts } from "./session-state-counts";
import { matchesMembership } from "./session-view";
import { clearSessionFilterPatch, type SessionViewState } from "./session-view-store";

export const STATUS_SHORTCUTS = [
  { status: "running", label: "Running", caption: "Running" },
  { status: "needs_input", label: "Questions / Needs input", caption: "Questions" },
  { status: "provider_blocked", label: "Provider blocked", caption: "Blocked" },
  { status: "error", label: "Errors", caption: "Errors" },
] as const;
export type StateShortcutStatus = typeof STATUS_SHORTCUTS[number]["status"];
export const STATUS_SHORTCUT_SCOPE = { projectId: null, membership: "sessions", refinements: "none" } as const;
export type StateShortcut = { readonly status: StateShortcutStatus; readonly scope: typeof STATUS_SHORTCUT_SCOPE };
const NO_REFUSAL_READS: ProviderRefusalReads = new Map();

/** Same evidence boundary as Sessions: unchecked native errors are unknown, never inferred provider blocks. */
export function startSwitchCounts(state: DirectoryState, freshness: string, reads = NO_REFUSAL_READS) {
  const candidates = state.entries.filter((entry) => matchesMembership(entry, "sessions"));
  const input = { state, includeArchived: false, freshness: freshness === "Live" && state.inventoryComplete !== true ? "Reading" : freshness, blocked: null };
  const providerBlocked = providerBlockedView({ ...input, candidates, reads });
  return sessionStateCounts({ ...input, universe: candidates, candidates, favorites: [], refined: false, providerBlocked: providerBlocked.count });
}

/** Only transient filters change. Saved presentation and the chat composer are outside this hand-off. */
export function stateShortcutPatch(view: SessionViewState, shortcut: StateShortcut): Partial<SessionViewState> {
  return { ...clearSessionFilterPatch(view), status: shortcut.status, query: "", membership: shortcut.scope.membership,
    includeArchived: false, work: "mine", scrollOffset: 0, peek: null };
}
