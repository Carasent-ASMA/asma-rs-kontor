import { hasSessionAttention, type DirectoryEntry, type JiraCapture } from "./session-directory";
import { sessionEpic } from "./session-organization";
import type { SourceRead } from "../shared/read-sources";
export function jiraIssueUrl(key: string): string | null {
  return /^ASMA-[1-9]\d*$/.test(key) ? `https://carasent.atlassian.net/browse/${key}` : null;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const ISO_STAMP = /\b(\d{4})-(\d{2})-(\d{2})(?:T[\d:.]+(?:Z|[+-]\d{2}:?\d{2})?)?\b/g;
/** Visible text only: ISO stamps become their own calendar day ("19 Oct"); exact values stay in details and labels. */
export function shortDates(text: string): string {
  return text.replace(ISO_STAMP, (stamp, _year: string, month: string, day: string) => MONTHS[Number(month) - 1] ? `${Number(day)} ${MONTHS[Number(month) - 1]}` : stamp);
}
/** A plugin read time as local "22:52" on the reading day, otherwise the local day ("18 Oct"); unparseable values stay as given. */
export function readTime(value: string | number, now: number = Date.now()): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return String(value);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  return `${date.getDate()} ${MONTHS[date.getMonth()]}`;
}

/** The session's epic as a Jira key, the same one the Sessions tree files it under; null is unknown. */
export function confirmedEpic(entry: DirectoryEntry): string | null {
  const epic = sessionEpic(entry, undefined).epic?.toUpperCase() ?? null;
  return epic !== null && /^ASMA-[1-9]\d*$/.test(epic) ? epic : null;
}
const NO_JIRA_CAPTURE: JiraCapture = { epics: new Map(), observed: [] };
/** Each captured key's epic across the ready historical Jira captures; one stable value while none is ready. */
export function jiraCapture(reads: readonly SourceRead[]): JiraCapture {
  const ready = reads.flatMap((read) => read.source.kind === "jira" && read.status === "ready" && read.jiraHistory ? [read.jiraHistory] : []);
  if (!ready.length) return NO_JIRA_CAPTURE;
  const epics = new Map<string, string | null>();
  for (const history of ready) for (const epic of history.epics) for (const issue of [epic, ...epic.children]) {
    const known = epics.get(issue.key);
    epics.set(issue.key, known === undefined || known === epic.key ? epic.key : null);
  }
  return { epics, observed: ready.map((history) => history.generated) };
}
/** Entries carrying the capture; the cache, one per capture, keeps an unchanged entry's identity across updates. */
export function withJiraCapture(entries: readonly DirectoryEntry[], jira: JiraCapture, cache: WeakMap<DirectoryEntry, DirectoryEntry>): DirectoryEntry[] {
  return entries.map((entry) => {
    let linked = cache.get(entry);
    if (!linked) cache.set(entry, linked = { ...entry, jira });
    return linked;
  });
}
export function nativeSummary(entries: readonly DirectoryEntry[]) {
  return { total: entries.length, running: entries.filter((entry) => entry.status === "running").length,
    idle: entries.filter((entry) => entry.status === "idle").length,
    closed: entries.filter((entry) => entry.status === "closed").length,
    attention: entries.filter(hasSessionAttention).length,
    needsInput: entries.filter((entry) => entry.needsInput).length,
    errors: entries.filter((entry) => entry.status === "error").length };
}
export function recordedDecisions(reads: readonly SourceRead[]) {
  const sources = reads.filter((read) => read.source.kind === "decisions");
  const ready = sources.filter((read) => read.status === "ready" && read.records !== null);
  return { open: ready.length ? ready.reduce((sum, read) => sum + read.records!.decisions.filter((record) => record.state === "open").length, 0) : null,
    unknown: ready.length ? ready.reduce((sum, read) => sum + read.records!.decisions.filter((record) => record.state === "unknown").length, 0) : null,
    captured: ready.length, configured: sources.length, complete: sources.length > 0 && ready.length === sources.length };
}
export type PortfolioFilter = "all" | "active" | "review" | "decisions" | "completed" | "delivered";
/** One Overview row: an epic with its exact plans, native sessions and dated historical records, or a plan with no epic. */
export type PortfolioRow = {
  id: string; epicKey: string | null; title: string; plans: SourceRead[]; sessions: ReturnType<typeof nativeSummary>;
  lifecycle: { status: string; done: boolean; observedAt: string } | null;
  tasks: { total: number; done: number; excluded: number; observedAt: string } | null;
  decisions: number | null; delivery: number | null;
};
const WONT_DO = /^won[’']?t do$/i;
const sourceEpic = (read: SourceRead) => read.graph?.epic.jira ?? read.source.epic;
/**
 * Epics come from confirmed session labels and source metadata; the historical Jira selection adds
 * its epics only to an unscoped portfolio. Sessions without an epic stay in the inventory count.
 */
export function portfolioRows(entries: readonly DirectoryEntry[], reads: readonly SourceRead[], unscoped: boolean): PortfolioRow[] {
  const keys = new Set(entries.map(confirmedEpic).filter((key): key is string => key !== null));
  for (const read of reads) {
    const key = read.source.kind === "jira" ? null : sourceEpic(read);
    if (key && /^ASMA-\d+$/.test(key)) keys.add(key);
    if (unscoped && read.status === "ready") for (const epic of read.jiraHistory?.epics ?? []) keys.add(epic.key);
  }
  const rows = [...keys].sort().map((key): PortfolioRow => {
    const own = reads.filter((read) => read.source.kind !== "jira" && sourceEpic(read) === key);
    const plans = own.filter((read) => read.source.kind === "plan");
    const history = historicalJiraIssue(key, null, reads);
    const children = history ? reads.find((read) => read.source.id === history.observations[0]!.sourceId)?.jiraHistory?.epics.find((epic) => epic.key === key)?.children ?? null : null;
    const counted = children?.filter((child) => !WONT_DO.test(child.status)) ?? [];
    const decisions = recordedDecisions(own);
    const delivery = own.filter((read) => read.source.kind === "delivery" && read.status === "ready" && read.records);
    const observedAt = history?.observations.map((value) => value.observedAt).join(" / ") ?? "";
    return { id: key, epicKey: key, title: plans.length === 1 ? `${key} · ${plans[0]!.source.title}` : key, plans,
      sessions: nativeSummary(entries.filter((entry) => confirmedEpic(entry) === key)),
      lifecycle: history ? { status: history.issue.status, done: history.issue.cat === "done", observedAt } : null,
      tasks: children ? { total: counted.length, done: counted.filter((child) => child.cat === "done").length, excluded: children.length - counted.length, observedAt } : null,
      decisions: decisions.complete ? decisions.open : null,
      delivery: delivery.length ? delivery.reduce((sum, read) => sum + read.records!.delivery.length, 0) : null };
  });
  for (const read of reads) if (read.source.kind === "plan" && !sourceEpic(read)) {
    rows.push({ id: `plan:${read.source.id}`, epicKey: null, title: read.source.title, plans: [read], sessions: nativeSummary([]), lifecycle: null, tasks: null, decisions: null, delivery: null });
  }
  return rows;
}
/** Decisions and delivery without a captured record stay in their filters as Unknown, as in the prototype. */
export function portfolioFilter(row: PortfolioRow, filter: PortfolioFilter): boolean {
  if (filter === "active") return row.sessions.running > 0;
  if (filter === "review") return row.sessions.attention + row.sessions.needsInput + row.sessions.errors > 0;
  if (filter === "decisions") return row.decisions === null || row.decisions > 0;
  if (filter === "completed") return row.lifecycle?.done === true;
  // Delivery confirmation is never inferred from recorded pull requests or checks.
  if (filter === "delivered") return true;
  return true;
}
export function inboxEntries(entries: readonly DirectoryEntry[]): DirectoryEntry[] {
  return entries.filter((entry) => entry.needsInput || entry.status === "error" || hasSessionAttention(entry));
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
