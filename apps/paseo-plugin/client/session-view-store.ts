import {
  DEFAULT_SESSION_FILTERS,
  type SessionFilters,
} from "./session-view";

export type SessionGrouping = "hierarchy" | "placement" | "role" | "flat";

export type SessionViewState = SessionFilters & {
  readonly scrollOffset: number;
  readonly shortcut?: string | null;
  /** Local choice; Workstreams, Role and Flat are also saved when settings are writable. */
  readonly grouping?: SessionGrouping;
  /** Selected workstreams by normalized label; absent means all. */
  readonly streams?: readonly string[] | null;
  readonly arranging?: boolean;
  /** Compact toolbar disclosure; local view state only, never saved to settings. */
  readonly toolbarOpen?: boolean;
  /** Legend disclosure for this local view; never saved to organization settings. */
  readonly legendOpen?: boolean;
  /** Sessions whose task labels equal a Board task's keys exactly; set from Board, cleared like a filter. */
  readonly task?: SessionTaskFilter | null;
  /** Tree offset kept while results show, restored when the last result filter clears. */
  readonly treeOffset?: number;
  /** A result shown in its place in the tree; disclosure opened for it is never saved. */
  readonly peek?: { readonly sessionId: string; readonly resultsOffset: number } | null;
};

export type SessionTaskFilter = { readonly jira: string | null; readonly kontorId: string | null; readonly label: string };

/** A search, a state filter or a task lists matching sessions flat; scope, desks and workstreams keep the tree. */
export function isResultMode(view: SessionFilters & { readonly task?: SessionTaskFilter | null }): boolean {
  return view.query.trim().length > 0 || view.status !== "all" || !!view.task;
}

const RESULT_KEYS = ["query", "status", "task", "includeArchived", "shortcut", "streams", "grouping"] as const;

/**
 * The patch to apply for a view change. Entering results keeps the tree offset
 * (the screen passes it measured below its own toolbar); leaving them restores
 * it; changing what the results contain ends a peek.
 */
export function sessionViewTransition(current: SessionViewState, patch: Partial<SessionViewState>, treeAnchor = current.scrollOffset): Partial<SessionViewState> {
  const next = { ...current, ...patch };
  const was = isResultMode(current);
  const is = isResultMode(next);
  if (!was && is) return { ...patch, treeOffset: Math.max(0, treeAnchor), peek: null, scrollOffset: patch.scrollOffset ?? 0 };
  if (was && !is) return { ...patch, treeOffset: undefined, peek: null, scrollOffset: current.treeOffset ?? patch.scrollOffset ?? 0 };
  return RESULT_KEYS.some((key) => key in patch && patch[key] !== current[key]) ? { ...patch, peek: null } : patch;
}

const views = new Map<string, SessionViewState>();
const expansions = new Map<string, Readonly<Record<string, boolean>>>();
const NO_EXPANSION: Readonly<Record<string, boolean>> = Object.freeze({});

function keyOf(hostId: string): string {
  return hostId.trim();
}

export function readSessionView(hostId: string): SessionViewState {
  return (
    views.get(keyOf(hostId)) ?? {
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    }
  );
}

export function writeSessionView(hostId: string, next: SessionViewState): void {
  views.set(keyOf(hostId), next);
}

export function patchSessionView(
  hostId: string,
  patch: Partial<SessionViewState>,
): SessionViewState {
  const next: SessionViewState = { ...readSessionView(hostId), ...patch };
  writeSessionView(hostId, next);
  return next;
}

export function writeScrollOffset(hostId: string, scrollOffset: number): void {
  patchSessionView(hostId, {
    scrollOffset: Number.isFinite(scrollOffset) && scrollOffset > 0 ? scrollOffset : 0,
  });
}

export function resetSessionView(hostId: string): SessionViewState {
  const next: SessionViewState = {
    ...DEFAULT_SESSION_FILTERS,
    scrollOffset: 0,
  };
  writeSessionView(hostId, next);
  return next;
}

/**
 * Disclosure the user chose in this client session. It applies at once and
 * keeps cached rows browseable while saved settings are unavailable or saving.
 */
export function readLocalExpansion(hostId: string): Readonly<Record<string, boolean>> {
  return expansions.get(keyOf(hostId)) ?? NO_EXPANSION;
}

export function patchLocalExpansion(
  hostId: string,
  patch: Readonly<Record<string, boolean>>,
): Readonly<Record<string, boolean>> {
  const next = { ...readLocalExpansion(hostId), ...patch };
  expansions.set(keyOf(hostId), next);
  return next;
}

/** Test seam: the store lives only for this client session, never on disk. */
export function clearSessionViews(): void {
  views.clear();
  expansions.clear();
}
