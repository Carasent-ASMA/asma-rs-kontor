import {
  DEFAULT_SESSION_FILTERS,
  type SessionFilters,
} from "./session-view";
import type { SessionSort, SortDirection } from "./session-sort";

export type SessionGrouping = "hierarchy" | "placement" | "role" | "flat";
/** My work follows the Working-as profile; a person scope names one exact account; Unknown lists what cannot be evaluated. */
export type WorkScope = "mine" | "all" | "unknown" | `person:${string}`;

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
  /** Responsibility scope; absent means My work. */
  readonly work?: WorkScope;
  /** Presentation order; absent means Current order. */
  readonly sort?: SessionSort;
  readonly sortDirection?: SortDirection;
  /** Association selector disclosure; local view state only. */
  readonly associationsOpen?: boolean;
};

export function workScopeOf(view: SessionViewState): WorkScope {
  return view.work ?? "mine";
}

export type SessionTaskFilter = { readonly jira: string | null; readonly kontorId: string | null; readonly label: string };

/** A search, a state filter or a task lists matching sessions flat; scope, desks and workstreams keep the tree. */
export function isResultMode(view: SessionFilters & { readonly task?: SessionTaskFilter | null }): boolean {
  return view.query.trim().length > 0 || view.status !== "all" || !!view.task;
}

/** Count narrowing choices only. Presentation and explicit route context are separate. */
export function sessionFilterCount(view: SessionViewState): number {
  return [view.query.trim().length > 0, view.status !== "all", !!view.task,
    view.shortcut != null, view.streams != null, view.includeArchived !== DEFAULT_SESSION_FILTERS.includeArchived,
    workScopeOf(view) !== "all"].filter(Boolean).length;
}

/** Clear all filtering choices, widening to All work; sort and grouping are presentation and stay. */
export function clearSessionFilterPatch(view: SessionViewState): Partial<SessionViewState> {
  if (sessionFilterCount(view) === 0) return {};
  return { query: "", status: "all", task: null, shortcut: null, streams: null,
    includeArchived: DEFAULT_SESSION_FILTERS.includeArchived, work: "all", scrollOffset: 0, peek: null };
}

/** What a new app session shows: filters cleared, My work, Current order and the saved grouping; nothing is saved. */
export function resetSessionViewPatch(view: SessionViewState): Partial<SessionViewState> {
  return { ...clearSessionFilterPatch(view), work: "mine", sort: undefined, sortDirection: undefined, grouping: undefined, arranging: false, scrollOffset: 0, peek: null };
}

const RESULT_KEYS = ["query", "status", "task", "includeArchived", "shortcut", "streams", "grouping", "work", "sort", "sortDirection"] as const;

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
/** Working-as account per host and tracker realm; a presentation preference for this app session, never sign-in. */
const workingAs = new Map<string, string>();
let initialView: Partial<SessionViewState> = {};

function keyOf(hostId: string): string {
  return hostId.trim();
}

export function readSessionView(hostId: string): SessionViewState {
  return (
    views.get(keyOf(hostId)) ?? {
      ...DEFAULT_SESSION_FILTERS,
      ...initialView,
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
    ...initialView,
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

function profileKey(hostId: string, realm: string): string {
  return JSON.stringify([keyOf(hostId), realm]);
}

export function readWorkingAs(hostId: string, realm: string | null): string | null {
  return realm === null ? null : workingAs.get(profileKey(hostId, realm)) ?? null;
}

/** The host a view belongs to: app scopes are `sessionScopeKey` JSON arrays led by the host ID; the default scope is the host ID. */
function viewHost(key: string): string {
  try {
    const parsed: unknown = JSON.parse(key);
    if (Array.isArray(parsed) && typeof parsed[0] === "string") return keyOf(parsed[0]);
  } catch {
    // A plain host scope.
  }
  return key;
}

/**
 * A changed profile ends the peeks it could have produced: this host's views in My work or Unknown
 * responsibility. Other hosts, person scopes and All work keep theirs, and choosing the same person changes nothing.
 */
export function writeWorkingAs(hostId: string, realm: string, accountId: string | null): void {
  if (readWorkingAs(hostId, realm) === accountId) return;
  if (accountId === null) workingAs.delete(profileKey(hostId, realm));
  else workingAs.set(profileKey(hostId, realm), accountId);
  for (const [key, view] of views) {
    const work = workScopeOf(view);
    if (view.peek && viewHost(key) === keyOf(hostId) && (work === "mine" || work === "unknown")) views.set(key, { ...view, peek: null });
  }
}

/**
 * Test seam: the store lives only for this client session, never on disk. `initial` seeds views not
 * yet read, such as `{ work: "all" }` for suites that exercise the unscoped inventory.
 */
export function clearSessionViews(initial: Partial<SessionViewState> = {}): void {
  views.clear();
  expansions.clear();
  workingAs.clear();
  initialView = initial;
}
