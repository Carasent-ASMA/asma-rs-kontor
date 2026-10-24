import { freshAvailability } from "../shared/session-lifecycle";
import {
  hasEarlierError,
  hasSessionAttention,
  isArchivedSession,
  type DirectoryEntry,
  type SessionStatus,
} from "./session-directory";

export const ALL_STATUS_FILTER = "all" as const;

/** One exclusive state choice; runtime and attention choices may overlap on the same session. */
export type StatusFilter =
  | "current"
  | typeof ALL_STATUS_FILTER
  | SessionStatus
  | "needs_input"
  | "attention"
  | "earlier_error"
  | "favorites";

/** Frequent choices; runtime history and less common states sit under More states. */
export const QUICK_STATUS_FILTERS: readonly StatusFilter[] = [
  "current",
  ALL_STATUS_FILTER,
  "running",
  "idle",
  "error",
  "needs_input",
  "attention",
  "favorites",
];
export const MORE_STATUS_FILTERS: readonly StatusFilter[] = ["initializing", "closed", "earlier_error"];

export type SessionMembership = "sessions" | "archived";

export type SessionFilters = {
  readonly query: string;
  readonly status: StatusFilter;
  /** Native inventory query scope only; never a union of ordinary sessions and archives. */
  readonly includeArchived: boolean;
  /** Absent in older views means ordinary, nonarchived sessions. */
  readonly membership?: SessionMembership;
  readonly advancedOpen: boolean;
};

export const DEFAULT_SESSION_FILTERS: SessionFilters = {
  query: "",
  status: "current",
  includeArchived: false,
  membership: "sessions",
  advancedOpen: false,
};

const STATUS_FILTER_LABELS: Record<StatusFilter, string> = {
  current: "Current",
  all: "All",
  running: "Running",
  idle: "Idle",
  error: "Error",
  needs_input: "Needs input",
  attention: "Attention",
  earlier_error: "Earlier errors",
  favorites: "★ Favorites",
  initializing: "Starting",
  closed: "Closed",
};

export function statusFilterLabel(status: StatusFilter): string {
  return STATUS_FILTER_LABELS[status];
}

function queryTokens(query: string): string[] {
  return query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter((token) => token.length > 0);
}

/** Every search token must match the full original title or its workspace context. */
export function matchesQuery(entry: DirectoryEntry, query: string): boolean {
  const tokens = queryTokens(query);
  if (tokens.length === 0) {
    return true;
  }
  return tokens.every((token) => entry.searchText.includes(token));
}

/** Archive membership is chosen before state, search, scope, assignment or presentation order. */
export function matchesMembership(entry: DirectoryEntry, membership: SessionMembership = "sessions"): boolean {
  return isArchivedSession(entry) === (membership === "archived");
}

/**
 * Error follows the current native lifecycle. Earlier errors remain separately
 * reachable without inflating current Error or Attention counts.
 */
export function matchesStatus(
  entry: DirectoryEntry,
  status: StatusFilter,
  favorites: readonly string[] = [],
): boolean {
  switch (status) {
    case "current":
      // Age and provider-history uncertainty never prove a session broken.
      return !isArchivedSession(entry) && freshAvailability(entry)?.state !== "missing" && (entry.needsInput || entry.status !== "closed" && entry.status !== "error");
    case ALL_STATUS_FILTER:
      return true;
    case "favorites":
      return favorites.includes(entry.id);
    case "needs_input":
      return entry.needsInput;
    case "attention":
      return hasSessionAttention(entry);
    case "error":
      return entry.status === "error";
    case "earlier_error":
      return hasEarlierError(entry);
    default:
      return entry.status === status;
  }
}

export function matchesFilters(
  entry: DirectoryEntry,
  filters: SessionFilters,
  favorites: readonly string[] = [],
): boolean {
  return matchesMembership(entry, filters.membership) && matchesStatus(entry, filters.status, favorites) && matchesQuery(entry, filters.query);
}

export function filterDirectoryEntries(
  entries: readonly DirectoryEntry[],
  filters: SessionFilters,
  favorites: readonly string[] = [],
): DirectoryEntry[] {
  return entries.filter((entry) => matchesFilters(entry, filters, favorites));
}

/** Counts the advanced choices, not the search text or the disclosure state. */
export function activeFilterCount(filters: SessionFilters): number {
  let count = 0;
  if (filters.status !== ALL_STATUS_FILTER) {
    count += 1;
  }
  if (filters.membership === "archived") {
    count += 1;
  }
  return count;
}

export function hasActiveView(filters: SessionFilters): boolean {
  return activeFilterCount(filters) > 0 || filters.query.trim().length > 0;
}

export function attentionLabel(entry: DirectoryEntry): string | null {
  if (entry.needsInput) {
    return "Needs input";
  }
  if (entry.status === "error") {
    return "Error";
  }
  if (!hasSessionAttention(entry)) {
    return null;
  }
  switch (entry.attentionReason) {
    case "finished":
      return "Finished";
    case "permission":
      return "Permission";
    default:
      return "Attention";
  }
}

export function archivedLabel(entry: DirectoryEntry): string | null {
  return entry.archivedAt === null ? null : "Archived";
}

export function earlierErrorLabel(entry: DirectoryEntry): string | null {
  return hasEarlierError(entry) ? `Earlier error · now ${entry.status}` : null;
}

export function sessionAvailabilityLabel(entry: DirectoryEntry): string | null {
  if (!entry.requiresAttention) return null;
  const evidence = freshAvailability(entry);
  if (evidence?.state === "missing") return "Native record absent · check availability to retry";
  if (evidence?.archivedAt) return "Already archived";
  if (evidence?.state === "readable") return `${entry.attentionReason === "finished" ? "Unread result · " : ""}Native record found · provider history ${evidence.providerHistory === "not-reported" ? "not reported" : "unverified"}`;
  return "Availability unknown";
}

/** Native execution and pending interaction stay obligations even after archive membership. */
export function hasNativeActiveTurn(entry: Pick<DirectoryEntry, "activeTurn" | "activeTurnStartedAt">): boolean {
  return entry.activeTurn === true || entry.activeTurnStartedAt !== null && entry.activeTurnStartedAt !== undefined;
}

export type ArchivedConflict = { readonly entry: DirectoryEntry; readonly reasons: readonly string[] };

/** An archived session with live execution or a pending interaction remains addressable and is never revived. */
export function archivedConflictReasons(entry: DirectoryEntry): readonly string[] {
  if (!isArchivedSession(entry)) return [];
  const reasons: string[] = [];
  if (entry.status === "running" || hasNativeActiveTurn(entry)) reasons.push("active native execution");
  if (entry.pendingPermissionCount > 0 || entry.attentionReason === "permission") reasons.push("pending interaction");
  return reasons;
}

export function isArchivedConflict(entry: DirectoryEntry): boolean {
  return archivedConflictReasons(entry).length > 0;
}

/** Evaluate the full raw inventory before membership, search, status or scope filtering. */
export function archivedConflicts(entries: readonly DirectoryEntry[]): readonly ArchivedConflict[] {
  return entries.flatMap((entry) => {
    const reasons = archivedConflictReasons(entry);
    return reasons.length ? [{ entry, reasons }] : [];
  });
}

export function archivedConflictIds(entries: readonly DirectoryEntry[]): ReadonlySet<string> {
  return new Set(archivedConflicts(entries).map((conflict) => conflict.entry.id));
}

/**
 * Ordinary filter matches plus every archived conflict, so a known obligation stays reachable when the normal
 * list excludes archives, a status/search/scope filter hides it, or the result is otherwise empty.
 */
export function filterDirectoryEntriesWithArchivedConflicts(entries: readonly DirectoryEntry[], filters: SessionFilters, favorites: readonly string[] = []): DirectoryEntry[] {
  const matching = filterDirectoryEntries(entries, filters, favorites), seen = new Set(matching.map((entry) => entry.id));
  for (const conflict of archivedConflicts(entries)) if (!seen.has(conflict.entry.id)) { seen.add(conflict.entry.id); matching.push(conflict.entry); }
  return matching;
}
