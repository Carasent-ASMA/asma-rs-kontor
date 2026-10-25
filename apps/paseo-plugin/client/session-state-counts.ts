import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { matchesStatus, MORE_STATUS_FILTERS, QUICK_STATUS_FILTERS, readCoversMembership, type StatusFilter } from "./session-view";

export type SessionStateCount = {
  readonly text: string;
  readonly label: string;
  /** Compact ratio parts, for the fixed-width mobile state target. Exact values stay in label. */
  readonly numerator?: string;
  readonly denominator?: string;
};

/** At most three characters per half of a mobile ratio; never shrink the text to fit. */
export function compactSessionCount(value: number): string {
  if (value < 1_000) return String(value);
  const units = ["K", "M", "B", "T"];
  for (let index = 0; index < units.length; index++) {
    const scaled = value / 1_000 ** (index + 1);
    if (Math.round(scaled) < 100) return `${Math.round(scaled)}${units[index]}`;
    if (scaled < 1_000 && index + 1 < units.length) {
      const tenth = Math.round(scaled / 100);
      return `${tenth === 10 ? "1" : `.${tenth}`}${units[index + 1]}`;
    }
  }
  return "…";
}

/**
 * Both sides count canonical session IDs with the same state predicate. The caller supplies one
 * host/membership/route universe and its refinements, BEFORE the selected state or presentation.
 * An incomplete/retained read is not a total; expose its observed counts in help, never as a live ratio.
 */
export function sessionStateCounts(input: {
  universe: readonly DirectoryEntry[]; candidates: readonly DirectoryEntry[]; favorites: readonly string[];
  refined: boolean; state: DirectoryState; includeArchived: boolean; freshness: string; blocked: string | null;
}): Readonly<Record<StatusFilter, SessionStateCount>> {
  const { universe, candidates, favorites, refined, state, includeArchived, freshness, blocked } = input;
  return Object.fromEntries([...QUICK_STATUS_FILTERS, ...MORE_STATUS_FILTERS].map((status) => {
    const count = (entries: readonly DirectoryEntry[]) => new Set(entries.filter((entry) => matchesStatus(entry, status, favorites)).map((entry) => entry.id)).size;
    const matched = count(candidates), total = count(universe);
    let result: SessionStateCount;
    if (blocked !== null) result = { text: "…", label: `Matching sessions not evaluated: ${blocked} is not evaluated` };
    else if (!readCoversMembership(state, includeArchived) || state.lastUpdatedAt === null) {
      result = state.status === "error" && readCoversMembership(state, includeArchived)
        ? { text: "—", label: "Session count unavailable: this membership could not be read" }
        : { text: "…", label: "Reading sessions for this membership" };
    } else if (freshness !== "Live" || state.inventoryComplete !== true) {
      const observed = refined ? `${matched} of ${total} sessions` : `${total} sessions`;
      result = { text: "…", label: `${observed} in the ${state.inventoryComplete === true ? "retained" : "partial"} read. Total updating; ${freshness.toLocaleLowerCase()}.` };
    } else if (refined) {
      const numerator = compactSessionCount(matched), denominator = compactSessionCount(total);
      result = { text: `${numerator}/${denominator}`, numerator, denominator,
        label: `${matched} of ${total} sessions after refinements, in this host's current project and archive scope` };
    } else result = { text: compactSessionCount(total), label: `${total} sessions in this host's current project and archive scope` };
    return [status, result];
  })) as Record<StatusFilter, SessionStateCount>;
}
