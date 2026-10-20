import {
  type DirectoryEntry,
  type SessionStatus,
} from "./session-directory";

export const ALL_STATUS_FILTER = "all" as const;

/** One exclusive state choice; runtime and attention choices may overlap on the same session. */
export type StatusFilter =
  | typeof ALL_STATUS_FILTER
  | SessionStatus
  | "needs_input"
  | "attention"
  | "favorites";

/** Frequent choices in prototype order; Starting and Closed sit under More states. */
export const QUICK_STATUS_FILTERS: readonly StatusFilter[] = [
  ALL_STATUS_FILTER,
  "running",
  "idle",
  "error",
  "needs_input",
  "attention",
  "favorites",
];
export const MORE_STATUS_FILTERS: readonly StatusFilter[] = ["initializing", "closed"];

export type SessionFilters = {
  readonly query: string;
  readonly status: StatusFilter;
  readonly includeArchived: boolean;
  readonly advancedOpen: boolean;
};

export const DEFAULT_SESSION_FILTERS: SessionFilters = {
  query: "",
  status: ALL_STATUS_FILTER,
  includeArchived: false,
  advancedOpen: false,
};

const STATUS_FILTER_LABELS: Record<StatusFilter, string> = {
  all: "All",
  running: "Running",
  idle: "Idle",
  error: "Error",
  needs_input: "Needs input",
  attention: "Attention",
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

/**
 * Needs input follows the native pending-permission priority, Error includes an
 * error attention reason, and Attention is the native attention flag.
 */
export function matchesStatus(
  entry: DirectoryEntry,
  status: StatusFilter,
  favorites: readonly string[] = [],
): boolean {
  switch (status) {
    case ALL_STATUS_FILTER:
      return true;
    case "favorites":
      return favorites.includes(entry.id);
    case "needs_input":
      return entry.needsInput;
    case "attention":
      return entry.requiresAttention;
    case "error":
      return entry.status === "error" || entry.attentionReason === "error";
    default:
      return entry.status === status;
  }
}

export function matchesFilters(
  entry: DirectoryEntry,
  filters: SessionFilters,
  favorites: readonly string[] = [],
): boolean {
  return matchesStatus(entry, filters.status, favorites) && matchesQuery(entry, filters.query);
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
  if (filters.includeArchived) {
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
  if (entry.status === "error" || entry.attentionReason === "error") {
    return "Error";
  }
  if (!entry.requiresAttention) {
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
