import type { DirectoryEntry } from "./session-directory";
import { parseTime } from "./session-time";

/** Current order is the existing state, saved and Role order; the others are explicit presentation choices. */
export type SessionSort = "current" | "updated" | "created" | "title";
export type SortDirection = "asc" | "desc";

export const SESSION_SORTS: readonly { readonly id: SessionSort; readonly label: string }[] = [
  { id: "current", label: "Current order" },
  { id: "updated", label: "Last changed at" },
  { id: "created", label: "Created at" },
  { id: "title", label: "Title" },
];

export function sortLabel(sort: SessionSort): string {
  return SESSION_SORTS.find(({ id }) => id === sort)!.label;
}

/** Times start newest first, titles A–Z. */
export function defaultSortDirection(sort: SessionSort): SortDirection {
  return sort === "title" ? "asc" : "desc";
}

export function sortDirectionLabel(sort: SessionSort, direction: SortDirection): string {
  if (sort === "title") return direction === "asc" ? "A–Z" : "Z–A";
  return direction === "desc" ? "Newest first" : "Oldest first";
}

function compareText(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

const titles = new Intl.Collator("en", { sensitivity: "base", numeric: true });

/**
 * Explicit order, or null for Current order. A missing or unparseable time sorts last in both directions;
 * no other time stands in for it. Equal values fall back to the exact session ID, ascending in both directions.
 */
export function sessionComparator(sort: SessionSort, direction: SortDirection): ((left: DirectoryEntry, right: DirectoryEntry) => number) | null {
  if (sort === "current") return null;
  const sign = direction === "asc" ? 1 : -1;
  if (sort === "title") {
    return (left, right) => sign * (titles.compare(left.title.normalize("NFC"), right.title.normalize("NFC")) || compareText(left.title, right.title))
      || compareText(left.id, right.id);
  }
  const field = sort === "updated" ? "updatedAt" : "createdAt";
  return (left, right) => {
    const a = parseTime(left[field]), b = parseTime(right[field]);
    if (a === null || b === null) return a === b ? compareText(left.id, right.id) : a === null ? 1 : -1;
    return sign * (a - b) || compareText(left.id, right.id);
  };
}

/** Explicit leaf positions by session ID for the organization tree; undefined keeps Current order. */
export function explicitLeafOrder(entries: readonly DirectoryEntry[], sort: SessionSort, direction: SortDirection): ReadonlyMap<string, number> | undefined {
  const compare = sessionComparator(sort, direction);
  return compare ? new Map([...entries].sort(compare).map((entry, index) => [entry.id, index])) : undefined;
}
