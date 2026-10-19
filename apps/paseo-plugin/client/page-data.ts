import type { DirectoryEntry } from "./session-directory";
import type { SourceRead } from "../shared/read-sources";
export function jiraIssueUrl(key: string): string | null {
  return /^ASMA-[1-9]\d*$/.test(key) ? `https://carasent.atlassian.net/browse/${key}` : null;
}

export function confirmedEpic(entry: DirectoryEntry): string | null {
  for (const key of ["epic", "jira.epic", "origin.epic"]) {
    const value = entry.labels[key]?.trim();
    if (value && /^ASMA-\d+$/.test(value)) return value;
  }
  return null;
}
export function overviewRows(entries: readonly DirectoryEntry[], reads: readonly SourceRead[]) {
  const keys = new Set(entries.map(confirmedEpic).filter((key): key is string => key !== null));
  for (const read of reads) {
    const key = read.graph?.epic.jira ?? read.source.epic;
    if (key && /^ASMA-\d+$/.test(key)) keys.add(key);
  }
  const rows = [...keys].sort().map((key) => ({ key, entries: entries.filter((entry) => confirmedEpic(entry) === key),
    reads: reads.filter((read) => (read.graph?.epic.jira ?? read.source.epic) === key) }));
  for (const read of reads) if (read.source.kind === "plan" && !(read.graph?.epic.jira ?? read.source.epic)) {
    rows.push({ key: `No epic linked · ${read.source.title}`, entries: [], reads: [read] });
  }
  const unassigned = entries.filter((entry) => confirmedEpic(entry) === null);
  if (unassigned.length) rows.push({ key: "Unassigned / ad-hoc", entries: unassigned, reads: [] });
  return rows;
}
export function nativeSummary(entries: readonly DirectoryEntry[]) {
  return { total: entries.length, running: entries.filter((entry) => entry.status === "running").length,
    idle: entries.filter((entry) => entry.status === "idle").length,
    closed: entries.filter((entry) => entry.status === "closed").length,
    attention: entries.filter((entry) => entry.requiresAttention).length,
    needsInput: entries.filter((entry) => entry.needsInput).length,
    errors: entries.filter((entry) => entry.status === "error").length };
}
export function inboxEntries(entries: readonly DirectoryEntry[]): DirectoryEntry[] {
  return entries.filter((entry) => entry.needsInput || entry.status === "error" || entry.requiresAttention || entry.attentionReason !== null);
}
export function searchRecords(entries: readonly DirectoryEntry[], reads: readonly SourceRead[], query: string) {
  const needle = query.trim().toLocaleLowerCase();
  return {
    sessions: entries.filter((entry) => !needle || entry.searchText.includes(needle)),
    plans: reads.filter((read) => read.source.kind === "plan" && (!needle || `${read.source.title}\n${read.content ?? ""}`.toLocaleLowerCase().includes(needle))),
    tasks: reads.flatMap((read) => (read.graph?.nodes ?? []).filter((node) => !needle || `${node.id} ${node.jira ?? ""} ${node.title}`.toLocaleLowerCase().includes(needle)).map((node) => ({ node, source: read.source }))),
  };
}

/** Historical observations join only exact Jira keys and confirmed parent keys. */
export function historicalJiraIssue(key: string | null, parent: string | null, reads: readonly SourceRead[]) {
  if (!key || !/^ASMA-[1-9]\d*$/.test(key)) return null;
  const matches = reads.flatMap((read) => {
    if (read.source.kind !== "jira" || read.status !== "ready" || !read.jiraHistory) return [];
    const epic = read.jiraHistory.epics.find((value) => value.key === (parent ?? key));
    const issue = parent ? epic?.children.find((value) => value.key === key) : epic;
    return issue ? [{ issue, sourceId: read.source.id, sourceTitle: read.source.title, observedAt: read.jiraHistory.generated }] : [];
  });
  const first = matches[0];
  if (!first || matches.some((value) => value.issue.status !== first.issue.status || value.issue.cat !== first.issue.cat)) return null;
  return { issue: first.issue, observations: matches };
}
export function historicalPlanProgress(graph: NonNullable<SourceRead["graph"]>, reads: readonly SourceRead[]) {
  const states = graph.nodes.map((node) => ({ node, evidence: graph.epic.jira ? historicalJiraIssue(node.jira, graph.epic.jira, reads) : null }));
  const known = states.filter((value) => value.evidence !== null).length;
  return { total: states.length, known, unknown: states.length - known, done: states.filter((value) => value.evidence?.issue.cat === "done").length, states };
}
