import {
  SESSION_STATUSES,
  type DirectoryEntry,
  type SessionStatus,
} from "./session-directory";

export const ALL_STATUS_FILTER = "all" as const;

export type StatusFilter = typeof ALL_STATUS_FILTER | SessionStatus;

export const STATUS_FILTERS: readonly StatusFilter[] = [
  ALL_STATUS_FILTER,
  ...SESSION_STATUSES,
];

export type SessionFilters = {
  readonly query: string;
  readonly status: StatusFilter;
  readonly attentionOnly: boolean;
  readonly includeArchived: boolean;
  readonly advancedOpen: boolean;
};

export const DEFAULT_SESSION_FILTERS: SessionFilters = {
  query: "",
  status: ALL_STATUS_FILTER,
  attentionOnly: false,
  includeArchived: false,
  advancedOpen: false,
};

export function statusFilterLabel(status: StatusFilter): string {
  if (status === ALL_STATUS_FILTER) {
    return "All";
  }
  return status.charAt(0).toLocaleUpperCase() + status.slice(1);
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
 * The attention filter follows the native needs-input priority: a pending
 * permission must be included even when `requiresAttention` is false.
 */
export function needsAttention(entry: DirectoryEntry): boolean {
  return entry.needsInput || entry.requiresAttention;
}

export function matchesFilters(entry: DirectoryEntry, filters: SessionFilters): boolean {
  if (filters.status !== ALL_STATUS_FILTER && entry.status !== filters.status) {
    return false;
  }
  if (filters.attentionOnly && !needsAttention(entry)) {
    return false;
  }
  return matchesQuery(entry, filters.query);
}

export function filterDirectoryEntries(
  entries: readonly DirectoryEntry[],
  filters: SessionFilters,
): DirectoryEntry[] {
  return entries.filter((entry) => matchesFilters(entry, filters));
}

/** Counts the advanced choices, not the search text or the disclosure state. */
export function activeFilterCount(filters: SessionFilters): number {
  let count = 0;
  if (filters.status !== ALL_STATUS_FILTER) {
    count += 1;
  }
  if (filters.attentionOnly) {
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
  if (!entry.requiresAttention) {
    return null;
  }
  switch (entry.attentionReason) {
    case "finished":
      return "Finished";
    case "error":
      return "Error";
    case "permission":
      return "Permission";
    default:
      return "Attention";
  }
}

export function archivedLabel(entry: DirectoryEntry): string | null {
  return entry.archivedAt === null ? null : "Archived";
}
