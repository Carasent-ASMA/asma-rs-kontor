import type { DirectoryEntry } from "./session-directory";
import type { SourceRead } from "../shared/read-sources";

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
