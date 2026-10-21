import type { DirectoryEntry } from "./session-directory";
import { sessionEpic, TASK_JIRA_LABELS } from "./session-organization";
import type { Responsibility } from "../shared/responsibility";

type Person = Responsibility["people"][number];
type Issue = Responsibility["issues"][number];

/** Distinct relation types; creator, reporter and the executing agent are never read. */
export type MatchReason = "workstream-responsible" | "workstream-participant" | "epic-assignee" | "task-assignee";
export const MATCH_REASONS: Readonly<Record<MatchReason, string>> = {
  "workstream-responsible": "Workstream responsible",
  "workstream-participant": "Workstream participant",
  "epic-assignee": "Epic assignee",
  "task-assignee": "Task assignee",
};

/** `other` only when every relevant relation was read and none names the person; anything uncovered is unknown. */
export type SessionResponsibility =
  | { readonly status: "match"; readonly reasons: readonly MatchReason[]; readonly unknown: readonly string[] }
  | { readonly status: "other" }
  | { readonly status: "unknown"; readonly unknown: readonly string[] };

export type ResponsibilityIndex = {
  readonly projection: Responsibility;
  readonly people: ReadonlyMap<string, Person>;
  readonly issues: ReadonlyMap<string, Issue>;
  /** Null when no workstream declaration was supplied. */
  readonly workstreams: ReadonlyMap<string, { readonly responsible: ReadonlySet<string>; readonly participants: ReadonlySet<string> }> | null;
};

export function indexResponsibility(projection: Responsibility): ResponsibilityIndex {
  return {
    projection,
    people: new Map(projection.people.map((person) => [person.accountId, person])),
    issues: new Map(projection.issues.map((issue) => [issue.key, issue])),
    workstreams: projection.provenance.workstreams === null ? null : new Map(projection.workstreams.map((stream) =>
      [stream.key.toLocaleLowerCase(), { responsible: new Set(stream.responsible), participants: new Set(stream.participants) }])),
  };
}

/** Producer-declared expiry; an expired projection evaluates nobody, so it never narrows or broadens My work. */
export function responsibilityExpired(index: ResponsibilityIndex, now: number): boolean {
  return now >= Date.parse(index.projection.expiresAt);
}

/** The epic above an issue: its key, null when the read chain has none, undefined when the chain leaves the projection. */
function epicOf(issues: ResponsibilityIndex["issues"], key: string, depth = 0): string | null | undefined {
  const issue = issues.get(key);
  if (!issue || depth > 4) return undefined;
  if (issue.level === 1) return issue.key;
  return issue.level > 1 || issue.parent === null ? null : epicOf(issues, issue.parent, depth + 1);
}

const clean = (value: string | undefined) => value?.trim() || null;
// ponytail: the resolver's wording is pinned by a test in responsibility.test.ts.
const CONFLICT = "sources disagree";

/**
 * One person's relation to one session. The epic is the resolver's (labels and Jira capture, never a display
 * mapping) and must agree with the projection's parent of every exact task label; the workstream is the
 * session's own label, otherwise its epic's. Nothing is inherited from neighbours, parents or titles.
 */
export function sessionResponsibility(entry: DirectoryEntry, index: ResponsibilityIndex, accountId: string): SessionResponsibility {
  if (!index.people.has(accountId)) return { status: "unknown", unknown: ["person not in this projection"] };
  const resolved = sessionEpic(entry, undefined);
  if (resolved.note.startsWith(CONFLICT)) return { status: "unknown", unknown: [`epic ${resolved.note}`] };
  const unknown = new Set<string>();
  const reasons = new Set<MatchReason>();
  const tasks = [...new Set(TASK_JIRA_LABELS.flatMap((label) => clean(entry.labels[label])?.toUpperCase() ?? []))];
  const claims = new Set<string | null>(resolved.epic === null ? [] : [resolved.epic.trim().toUpperCase()]);
  for (const task of tasks) {
    const epic = epicOf(index.issues, task);
    if (epic === undefined) unknown.add(`${task} not in this projection`);
    else claims.add(epic);
  }
  if (claims.size > 1) return { status: "unknown", unknown: [`epic sources disagree: ${[...claims].map((claim) => claim ?? "none").join(" / ")}`] };
  const epic = [...claims][0] ?? null;
  const epicIssue = epic === null ? undefined : index.issues.get(epic);
  if (epic === null) unknown.add(resolved.note === "no epic or task label" ? "no epic or task label" : "epic unknown");
  else if (!epicIssue) unknown.add(`${epic} not in this projection`);
  else if (epicIssue.assignee === undefined) unknown.add(`${epic} assignee not read`);
  else if (epicIssue.assignee === accountId) reasons.add("epic-assignee");
  for (const task of tasks) {
    const issue = index.issues.get(task);
    if (!issue) continue;
    if (issue.assignee === undefined) unknown.add(`${task} assignee not read`);
    else if (issue.assignee === accountId) reasons.add("task-assignee");
  }
  const own = clean(entry.labels.desk) ?? clean(entry.labels.workstream);
  const streams = own !== null ? [own] : epicIssue?.workstreams;
  if (streams === undefined) {
    if (epicIssue) unknown.add(`workstreams of ${epic} not read`);
  } else {
    for (const stream of streams) {
      const declared = index.workstreams?.get(stream.toLocaleLowerCase());
      if (!declared) unknown.add(index.workstreams ? `workstream ${stream} not declared` : "workstream responsibility not declared");
      else if (declared.responsible.has(accountId)) reasons.add("workstream-responsible");
      else if (declared.participants.has(accountId)) reasons.add("workstream-participant");
    }
  }
  if (reasons.size) return { status: "match", reasons: [...reasons], unknown: [...unknown] };
  return unknown.size ? { status: "unknown", unknown: [...unknown] } : { status: "other" };
}

/**
 * Roster choices for Working as and person scope. Same-named people stay apart by status, account type and,
 * last, a stable ordinal; account IDs are never shown.
 */
export function rosterChoices(index: ResponsibilityIndex): { readonly accountId: string; readonly label: string }[] {
  const base = [...index.people.values()]
    .sort((left, right) => left.displayName.localeCompare(right.displayName) || (left.accountId < right.accountId ? -1 : 1))
    .map((person) => ({ accountId: person.accountId,
      label: `${person.displayName}${person.active ? "" : " (inactive)"}${person.accountType === "atlassian" ? "" : ` (${person.accountType})`}` }));
  return base.map((choice) => {
    const same = base.filter(({ label }) => label === choice.label);
    return same.length < 2 ? choice : { ...choice, label: `${choice.label} · ${same.indexOf(choice) + 1} of ${same.length}` };
  });
}
