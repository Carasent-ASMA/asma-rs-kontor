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

/** The read a count is taken over. */
export type CountRead = { state: DirectoryState; includeArchived: boolean; freshness: string; blocked: string | null };

/** What a count shows while its read is not a live, complete total, else null. `observed` names what the read saw. */
export function unsettledCount({ state, includeArchived, freshness, blocked }: CountRead, observed: string): SessionStateCount | null {
  if (blocked !== null) return { text: "…", label: `Matching sessions not evaluated: ${blocked} is not evaluated` };
  if (!readCoversMembership(state, includeArchived) || state.lastUpdatedAt === null) {
    return state.status === "error" && readCoversMembership(state, includeArchived)
      ? { text: "—", label: "Session count unavailable: this membership could not be read" }
      : { text: "…", label: "Reading sessions for this membership" };
  }
  if (freshness !== "Live" || state.inventoryComplete !== true) {
    return { text: "…", label: `${observed} in the ${state.inventoryComplete === true ? "retained" : "partial"} read. Total updating; ${freshness.toLocaleLowerCase()}.` };
  }
  return null;
}

/**
 * Both sides count canonical session IDs with the same state predicate. The caller supplies one
 * host/membership/route universe and its refinements, BEFORE the selected state or presentation.
 * An incomplete/retained read is not a total; expose its observed counts in help, never as a live ratio.
 */
export function sessionStateCounts(input: CountRead & {
  universe: readonly DirectoryEntry[]; candidates: readonly DirectoryEntry[]; favorites: readonly string[]; refined: boolean;
  /** From providerBlockedView: exact-ID reads, never the state predicate. */
  providerBlocked: SessionStateCount;
}): Readonly<Record<StatusFilter, SessionStateCount>> {
  const { universe, candidates, favorites, refined } = input;
  return Object.fromEntries([...QUICK_STATUS_FILTERS, ...MORE_STATUS_FILTERS].map((status) => {
    if (status === "provider_blocked") return [status, input.providerBlocked];
    const count = (entries: readonly DirectoryEntry[]) => new Set(entries.filter((entry) => matchesStatus(entry, status, favorites)).map((entry) => entry.id)).size;
    const matched = count(candidates), total = count(universe);
    const numerator = compactSessionCount(matched), denominator = compactSessionCount(total);
    const result = unsettledCount(input, refined ? `${matched} of ${total} sessions` : `${total} sessions`)
      ?? (refined ? { text: `${numerator}/${denominator}`, numerator, denominator,
        label: `${matched} of ${total} sessions after refinements, in this host's current project and archive scope` }
        : { text: compactSessionCount(total), label: `${total} sessions in this host's current project and archive scope` });
    return [status, result];
  })) as Record<StatusFilter, SessionStateCount>;
}
