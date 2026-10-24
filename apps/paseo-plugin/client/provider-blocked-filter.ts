import { freshAvailability, type SessionAvailability } from "../shared/session-lifecycle";
import { isArchivedSession, type DirectoryEntry } from "./session-directory";
import { compactSessionCount, unsettledCount, type CountRead, type SessionStateCount } from "./session-state-counts";

/**
 * Exact-ID `sessions.provider-refusal` results by session ID: the fields of PR482's `ProviderRefusalRead` this filter
 * reads, or "failed" when the read threw. A missing ID was never read. The list carries no `lastError`, so these reads
 * are the only provider-block evidence; status, attention, age and titles never are.
 */
export type ProviderRefusalReads = ReadonlyMap<string, "failed" | {
  readonly availability: SessionAvailability;
  readonly refusal: { readonly state: "current" | "history"; readonly read: "fresh" | "stale" | "unreadable" } | null;
}>;

export type UnknownReason = "not-read" | "read-failed" | "read-not-current" | "unresolved";
export type ProviderBlock =
  /** A fresh read of this ID and revision saw a classified refusal on its native error (PR482 `current`). */
  | { readonly state: "blocked" }
  /** A fresh read stored no refusal. Not provider health, and never Resume eligibility. */
  | { readonly state: "none" }
  | { readonly state: "unknown"; readonly reason: UnknownReason };

/** Only a non-archived native error can carry a current refusal; every other session is outside the filter (null). */
export function providerBlock(entry: DirectoryEntry, reads: ProviderRefusalReads, now = Date.now()): ProviderBlock | null {
  if (entry.status !== "error" || isArchivedSession(entry)) return null;
  const result = reads.get(entry.id), unknown = (reason: UnknownReason) => ({ state: "unknown" as const, reason });
  if (result === undefined) return unknown("not-read");
  if (result === "failed") return unknown("read-failed");
  // As PR482 `refusalAsOf`: another ID, another revision, a negative age or expiry is not a read of this session now.
  if (!freshAvailability({ ...entry, availability: result.availability }, now)) return unknown("read-not-current");
  const { refusal } = result;
  return !refusal ? { state: "none" } : refusal.state === "current" && refusal.read === "fresh" ? { state: "blocked" } : unknown("unresolved");
}

export type ProviderBlockedView = {
  /** What choosing the filter shows, in candidate order: observed blocks only. */
  readonly sessions: readonly DirectoryEntry[];
  readonly blocked: number;
  readonly unknown: number;
  readonly none: number;
  readonly count: SessionStateCount;
  /** The error sessions not checked, counted but not listed (the screen offers Error), else null. */
  readonly notice: string | null;
  /** Why the filter shows nothing from candidates that are not empty, else null. */
  readonly empty: string | null;
};

const errorSessions = (count: number) => `${count} error session${count === 1 ? "" : "s"}`;

/**
 * One exclusive state choice over the screen's candidates (search, scope, task and assignment applied, state not). It
 * lists observed blocks only and counts unknown error sessions, never listing them as blocked or dropping them as free
 * of refusals. It only narrows the list: Resume and Take over eligibility stay the server's fenced exact read.
 */
export function providerBlockedView(input: CountRead & { candidates: readonly DirectoryEntry[]; reads: ProviderRefusalReads; now?: number }): ProviderBlockedView {
  const { candidates, reads, now = Date.now() } = input;
  const blocks = candidates.map((entry) => ({ entry, block: providerBlock(entry, reads, now) }));
  const ids = (state: ProviderBlock["state"]) => new Set(blocks.filter(({ block }) => block?.state === state).map(({ entry }) => entry.id)).size;
  const blocked = ids("blocked"), unknown = ids("unknown"), none = ids("none"), errors = blocked + unknown + none;
  const sessions = blocks.filter(({ block }) => block?.state === "blocked").map(({ entry }) => entry);
  const observed = `${blocked} provider blocked and ${unknown} not checked of ${errorSessions(errors)}`;
  const count = unsettledCount(input, observed) ?? (unknown
    ? { text: "…", label: `${observed}. A session not checked is not counted as blocked.` }
    : { text: compactSessionCount(blocked), label: `${blocked} provider blocked (observed) of ${errorSessions(errors)}, each read by exact ID within the last minute` });
  const notice = unknown ? `${errorSessions(unknown)} not checked: provider state unknown, so not listed as blocked. Error shows ${unknown === 1 ? "it" : "them"}.` : null;
  const empty = !candidates.length || sessions.length ? null
    : errors ? `No provider block observed among the ${errorSessions(errors)} here${unknown ? `; ${unknown} not checked` : ""}. This is not a provider health check.`
      : candidates.some((entry) => entry.status === "error") ? "Archived sessions are history: a refusal on one is never current."
        : "No session here is in native error, and only a native error can carry a current provider refusal.";
  return { sessions, blocked, unknown, none, count, notice, empty };
}
