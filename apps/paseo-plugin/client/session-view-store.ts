import {
  DEFAULT_SESSION_FILTERS,
  type SessionFilters,
} from "./session-view";
import type { SessionSort, SortDirection } from "./session-sort";
import { loadWorkingAs, saveWorkingAs, type ProfileStorage, type RememberedWorkingAs } from "./viewer-profile";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationPresentation, type OrganizationSettings } from "../shared/organization";
import type { EpicScope } from "./session-scope";

export type SessionGrouping = "hierarchy" | "placement" | "role" | "flat";
export type SessionLayout = NonNullable<OrganizationSettings["layout"]>;
/** My work is every native session on this host; All work also lists empty workspaces and plan-only choices. */
export type WorkScope = "mine" | "all";
/**
 * An explicit filter by Jira responsibility, Any assignment by default: the Working-as person, one exact
 * account, or sessions whose relation to the Working-as person is unknown. Never part of My work.
 */
export type AssignmentScope = "any" | "me" | "unknown" | `person:${string}`;

export type SessionViewState = SessionFilters & {
  readonly scrollOffset: number;
  readonly shortcut?: string | null;
  /** Last grouping, retained while Flat is visible; saved when settings are writable. */
  readonly grouping?: SessionGrouping;
  readonly layout?: SessionLayout;
  readonly epic?: EpicScope | null;
  readonly scopeOpen?: "desk" | "epic" | null;
  /** Selected workstreams by normalized label; absent means all. */
  readonly streams?: readonly string[] | null;
  readonly arranging?: boolean;
  /** Compact toolbar disclosure; local view state only, never saved to settings. */
  readonly toolbarOpen?: boolean;
  /** Legend disclosure for this local view; never saved to organization settings. */
  readonly legendOpen?: boolean;
  /** Sessions whose task labels equal a Board task's keys exactly; set from Board, cleared like a filter. */
  readonly task?: SessionTaskFilter | null;
  /** Grouped offset kept while Flat shows, restored when Grouped returns. */
  readonly treeOffset?: number;
  /** A result shown in its place in the tree; disclosure opened for it is never saved. */
  readonly peek?: { readonly sessionId: string; readonly resultsOffset: number } | null;
  /** Work scope; absent means My work. */
  readonly work?: WorkScope;
  /** Assignment filter; absent means Any assignment. */
  readonly assignment?: AssignmentScope;
  /** Presentation order; absent means Current order. */
  readonly sort?: SessionSort;
  readonly sortDirection?: SortDirection;
  /** Association selector disclosure; local view state only. */
  readonly associationsOpen?: boolean;
};

export function workScopeOf(view: SessionViewState): WorkScope {
  return view.work ?? "mine";
}

export function assignmentOf(view: SessionViewState): AssignmentScope {
  return view.assignment ?? "any";
}

export type SessionTaskFilter = { readonly jira: string | null; readonly kontorId: string | null; readonly label: string };

export function sessionPresentation(view: Pick<SessionViewState, "layout" | "grouping">, preferences: Pick<OrganizationSettings, "layout" | "grouping"> = DEFAULT_ORGANIZATION_SETTINGS) {
  const saved = organizationPresentation(preferences);
  return {
    layout: view.layout ?? (view.grouping === undefined ? saved.layout : view.grouping === "flat" ? "flat" : "grouped"),
    grouping: view.grouping === "flat" ? "hierarchy" : view.grouping ?? saved.grouping,
  };
}

/** Layout alone chooses the flat list; filtering never changes presentation. */
export function isResultMode(view: Pick<SessionViewState, "query" | "status" | "task" | "layout" | "grouping">, preferences: Pick<OrganizationSettings, "layout" | "grouping"> = DEFAULT_ORGANIZATION_SETTINGS): boolean {
  return sessionPresentation(view, preferences).layout === "flat";
}

/** Count narrowing choices only. Presentation and explicit route context are separate. */
export function sessionFilterCount(view: SessionViewState): number {
  return [view.query.trim().length > 0, view.status !== "all", !!view.task,
    view.shortcut != null, view.streams != null, view.epic != null, view.membership === "archived",
    workScopeOf(view) !== "all", assignmentOf(view) !== "any"].filter(Boolean).length;
}

/** Clear all filtering choices, widening to All work and Any assignment; sort, grouping and Working as stay. */
export function clearSessionFilterPatch(view: SessionViewState): Partial<SessionViewState> {
  if (sessionFilterCount(view) === 0) return {};
  return { query: "", status: "all", task: null, shortcut: null, streams: null, epic: null,
    includeArchived: DEFAULT_SESSION_FILTERS.includeArchived, membership: DEFAULT_SESSION_FILTERS.membership, work: "all", assignment: "any", scrollOffset: 0, peek: null };
}

/** What a new app session shows: filters cleared, My work, Any assignment, Current order and the saved grouping; Working as stays. */
export function resetSessionViewPatch(view: SessionViewState): Partial<SessionViewState> {
  return { ...clearSessionFilterPatch(view), status: DEFAULT_SESSION_FILTERS.status, work: "mine", assignment: "any", sort: undefined, sortDirection: undefined, grouping: undefined, layout: undefined, scopeOpen: null, arranging: false, scrollOffset: 0, peek: null };
}

export function clearSessionScopePatch(): Partial<SessionViewState> {
  return { shortcut: null, streams: null, epic: null, scrollOffset: 0, peek: null };
}

const RESULT_KEYS = ["query", "status", "task", "includeArchived", "membership", "shortcut", "streams", "epic", "grouping", "layout", "work", "assignment", "sort", "sortDirection"] as const;

/**
 * The patch to apply for a view change. Entering results keeps the tree offset
 * (the screen passes it measured below its own toolbar); leaving them restores
 * it; changing what the results contain ends a peek.
 */
export function sessionViewTransition(current: SessionViewState, patch: Partial<SessionViewState>, treeAnchor = current.scrollOffset, preferences: Pick<OrganizationSettings, "layout" | "grouping"> = DEFAULT_ORGANIZATION_SETTINGS): Partial<SessionViewState> {
  const next = { ...current, ...patch };
  const was = isResultMode(current, preferences);
  const is = isResultMode(next, preferences);
  if (!was && is) return { ...patch, treeOffset: Math.max(0, treeAnchor), peek: null, scrollOffset: patch.scrollOffset ?? 0 };
  if (was && !is) return { ...patch, treeOffset: undefined, peek: null, scrollOffset: current.treeOffset ?? patch.scrollOffset ?? 0 };
  return RESULT_KEYS.some((key) => key in patch && patch[key] !== current[key]) ? { ...patch, peek: null } : patch;
}

const views = new Map<string, SessionViewState>();
const expansions = new Map<string, Readonly<Record<string, boolean>>>();
const NO_EXPANSION: Readonly<Record<string, boolean>> = Object.freeze({});
/** Working as per host and tracker realm, read from the device store once per app session; a viewing preference, never sign-in. */
const workingAs = new Map<string, RememberedWorkingAs>();
let profileStorage: ProfileStorage | null = null;
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

/** Where Working as is remembered: the screen sets this device's store, or none, before the first read. */
export function configureWorkingAsStorage(storage: ProfileStorage | null): void {
  profileStorage = storage;
  workingAs.clear();
}

/** The choice and where it lives, read synchronously from the store the first time, so it is there before the first render. */
export function readRememberedWorkingAs(hostId: string, realm: string): RememberedWorkingAs {
  const key = profileKey(hostId, realm);
  const known = workingAs.get(key);
  if (known) return known;
  const loaded = loadWorkingAs(profileStorage, hostId, realm);
  workingAs.set(key, loaded);
  return loaded;
}

export function readWorkingAs(hostId: string, realm: string | null): string | null {
  return realm === null ? null : readRememberedWorkingAs(hostId, realm).accountId;
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
 * Remember or, with null, forget Working as for this host and realm. A changed profile ends the peeks it could
 * have produced: this host's views filtered to Assigned to me or Unknown assignment. Other hosts, a named person,
 * Any assignment and My work keep theirs. The same person changes nothing, except retrying a store that failed.
 */
export function writeWorkingAs(hostId: string, realm: string, accountId: string | null): void {
  const current = readRememberedWorkingAs(hostId, realm);
  if (current.accountId === accountId && current.persistence !== "failed") return;
  workingAs.set(profileKey(hostId, realm), saveWorkingAs(profileStorage, hostId, realm, accountId));
  if (current.accountId === accountId) return;
  for (const [key, view] of views) {
    const assignment = assignmentOf(view);
    if (view.peek && viewHost(key) === keyOf(hostId) && (assignment === "me" || assignment === "unknown")) views.set(key, { ...view, peek: null });
  }
}

/**
 * Test seam: views live only for this client session, never on disk; this forgets what the session read of
 * Working as, never the device store. `initial` seeds views not yet read, such as `{ work: "all" }`.
 */
export function clearSessionViews(initial: Partial<SessionViewState> = {}): void {
  views.clear();
  expansions.clear();
  workingAs.clear();
  initialView = initial;
}
