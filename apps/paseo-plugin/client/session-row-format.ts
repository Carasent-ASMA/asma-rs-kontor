import { deskName } from "../shared/session-create";
import type { DirectoryEntry } from "./session-directory";
import { sessionRole, TASK_JIRA_LABELS, TASK_TICKET_LABEL } from "./session-organization";
import { parseTime } from "./session-time";
import { formatTokens, validTokenCount } from "./session-usage";

const MINUTE = 60_000, HOUR = 60 * MINUTE, DAY = 24 * HOUR, WEEK = 7 * DAY, MONTH = 30 * DAY, YEAR = 365 * DAY;
const AGE_UNITS = [[YEAR, "y"], [MONTH, "mo"], [WEEK, "w"], [DAY, "d"], [HOUR, "h"], [MINUTE, "m"]] as const;

/** "now", "5m", "3h", "2d", "3w", "4mo", "2y": the largest whole unit. Null when the time is unreadable or ahead of this device. */
export function compactAge(at: number | null, now: number): string | null {
  if (at === null) return null;
  const ago = now - at;
  if (ago < -MINUTE) return null;
  if (ago < MINUTE) return "now";
  const [size, unit] = AGE_UNITS.find(([size]) => ago >= size)!;
  return `${Math.floor(ago / size)}${unit}`;
}

/**
 * What a session last genuinely did, and which native field says so: the running turn's start or the last prompt.
 * Metadata `updatedAt` never counts, and neither does `attentionTimestamp`: Mark as unread raises it too, and it
 * survives later turns. Paseo records no time for a turn's end, so a session that only finished work reads as of
 * the prompt that started it.
 */
export type Activity = { readonly at: number; readonly source: "turn started" | "prompt" };

export function lastActivity(entry: Pick<DirectoryEntry, "activeTurnStartedAt" | "lastUserMessageAt">): Activity | null {
  const turn = parseTime(entry.activeTurnStartedAt), prompt = parseTime(entry.lastUserMessageAt);
  if (turn === null && prompt === null) return null;
  return turn !== null && (prompt === null || turn >= prompt) ? { at: turn, source: "turn started" } : { at: prompt!, source: "prompt" };
}

/** Newest genuine activity first; sessions with none keep their order at the end. */
export function byLastActivity<T extends Parameters<typeof lastActivity>[0]>(entries: readonly T[]): T[] {
  return entries.map((entry, index) => ({ entry, index, at: lastActivity(entry)?.at ?? null }))
    .sort((a, b) => a.at === null || b.at === null ? a.at === b.at ? a.index - b.index : a.at === null ? 1 : -1 : b.at - a.at || a.index - b.index)
    .map(({ entry }) => entry);
}

/** Qualified totals or labelled cumulative snapshots. Null means unavailable, never latest-turn usage. */
/** `source` is where a supported read says the totals come from, shown in Details. */
export type SessionTotals = { readonly input: number | null; readonly output: number | null; readonly source?: string; readonly scope?: "session" | "provider-cumulative" };

/** "↓ 124.8k" / "↑ 9.6k" with lowercase units; an unknown total is "↓ —", never 0. */
export function tokenTotals(totals: SessionTotals | null | undefined): { readonly input: string; readonly output: string; readonly spoken: string } {
  const one = (value: number | null | undefined) => validTokenCount(value) ? formatTokens(value).toLocaleLowerCase() : null;
  const input = one(totals?.input), output = one(totals?.output);
  return {
    input: `${totals?.scope === "provider-cumulative" ? "reported " : ""}↓ ${input ?? "—"}`, output: `↑ ${output ?? "—"}`,
    spoken: input === null && output === null ? "Session token totals unknown" : `${totals?.scope === "provider-cumulative" ? "Provider-reported cumulative" : "Session totals"}: ${input ?? "unknown"} input, ${output ?? "unknown"} output tokens`,
  };
}

/** Details preserve the source scope; unknown is said, never zero. */
export function totalsDetail(totals: SessionTotals | null | undefined): { readonly label: string; readonly value: string } {
  const { input, output } = tokenTotals(totals);
  return { label: totals?.scope === "provider-cumulative" ? "Provider-reported cumulative input ↓ and output ↑" : "Input ↓ and output ↑", value: totals ? `${input} ${output} · ${totals.source ?? "Source not stated"}` : "Unknown: no supported read reports this session's totals." };
}

/** The task and pull request a session's labels name: the first Jira or Kontor ticket label, and "PR 42" from `pr`. */
export function sessionWork(entry: DirectoryEntry): { readonly task: string | null; readonly pr: string | null } {
  const pr = entry.labels.pr?.trim().replace(/^#/, "");
  return { task: [...TASK_JIRA_LABELS, TASK_TICKET_LABEL].map((key) => entry.labels[key]?.trim()).find((value) => !!value) ?? null, pr: pr ? `PR ${pr}` : null };
}

/**
 * A "ROLE • SCOPE • topic" title shows its topic; its role and scope move to the smaller origin line with the labelled
 * task and pull request, the desk, the workspace and when Paseo last changed the session's metadata. Any other title
 * stays whole. The full title is still the row's spoken name and search text.
 */
export function rowOrigin(entry: DirectoryEntry, now: number): { readonly title: string; readonly origin: readonly string[] } {
  const parts = entry.title.split("•").map((part) => part.trim());
  const role = sessionRole(entry).code;
  const named = parts.length >= 3 && parts.every(Boolean);
  // "LSA • topic" names only its role, which the origin already carries.
  const roleOnly = parts.length === 2 && parts.every(Boolean) && parts[0] === role;
  const updated = compactAge(parseTime(entry.updatedAt), now);
  const scope = named ? parts[1]! : null, { task, pr } = sessionWork(entry);
  // A name already on the line, such as a task its scope names, or a workspace title segment ("TSW • ASMA-8641") naming one, is not repeated.
  const lead = [role, scope, task, pr, deskName(entry.projectName)], repeated = (part: string) => lead.some((other) => other?.toUpperCase() === part.toUpperCase());
  const workspace = entry.workspaceName?.split("•").map((segment) => segment.trim()).filter((segment) => segment && !repeated(segment)).join(" • ") || null;
  const origin = [...lead, workspace, updated ? `upd ${updated}` : null]
    .filter((part): part is string => !!part).filter((part, index, all) => all.findIndex((other) => other.toUpperCase() === part.toUpperCase()) === index);
  return { title: named ? parts.slice(2).join(" • ") : roleOnly ? parts[1]! : entry.title, origin };
}
