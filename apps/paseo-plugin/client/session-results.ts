import type { DirectoryEntry } from "./session-directory";
import { isDeskWork, organizationScope, sessionState, workspaceLabel, type OrganizationGroup, type OrganizationState } from "./session-organization";

type Mappings = Parameters<typeof organizationScope>[1];

/**
 * Where a filtered result sits: the tree's own workstream, epic and workspace
 * labels, from display mappings and session labels only, never from the title.
 * Desks sit outside epics, so a desk result names the desk and its workspace.
 */
export type SessionProvenance = { readonly parts: readonly string[]; readonly desk: boolean; readonly workspace: string };

/** Whether the session names a Paseo workspace; the tree otherwise groups it by its working directory. */
export function inWorkspace(entry: DirectoryEntry): boolean {
  return !!(entry.workspaceId || entry.workspaceName);
}

export function sessionProvenance(entry: DirectoryEntry, mappings: Mappings): SessionProvenance {
  const scope = organizationScope(entry, mappings);
  const desk = isDeskWork(entry, mappings);
  // Say a working directory is not a workspace rather than pass it off as one.
  const workspace = inWorkspace(entry) ? workspaceLabel(entry) : entry.cwd ? `No workspace · ${entry.cwd}` : "No workspace";
  return { parts: desk ? [`${scope.workstream} desk`, workspace] : [scope.workstream, scope.epic, workspace], desk, workspace };
}

/** Every token must appear in the session's search text or its placement labels (a mapped name counts). */
export function matchesResultSearch(entry: DirectoryEntry, tokens: readonly string[], mappings: Mappings): boolean {
  if (!tokens.length) return true;
  const placement = sessionProvenance(entry, mappings).parts.join("\n").toLocaleLowerCase();
  return tokens.every((token) => entry.searchText.includes(token) || placement.includes(token));
}

/** Group keys from the root down to the group holding the session, or null when the tree does not hold it. */
export function sessionAncestors(groups: readonly OrganizationGroup[], sessionId: string): string[] | null {
  for (const group of groups) {
    if (group.sessions.some((entry) => entry.id === sessionId)) return [group.key];
    const below = sessionAncestors(group.groups, sessionId);
    if (below) return [group.key, ...below];
  }
  return null;
}

/**
 * Role from a role label only: `kontor.role` (Kontor runtime) or `role`. Both are
 * unenforced labels; a title prefix is never read as a role here.
 */
export function labelledRole(entry: DirectoryEntry): string | null {
  const value = (entry.labels["kontor.role"] ?? entry.labels.role)?.trim();
  return value ? value.replace(/[-_]+/g, " ").toLocaleUpperCase() : null;
}

/** Label keys whose exact value may name a task's Jira key. */
export const TASK_JIRA_LABELS = ["jira.issue", "task"] as const;
/** Label key whose exact value may name a task's Kontor ticket. */
export const TASK_TICKET_LABEL = "kontor.ticket";

export type TaskRef = { readonly jira: string | null; readonly kontorId: string | null };
export type TaskLink = { readonly entry: DirectoryEntry; readonly via: string };

/** Sessions whose label value equals the task's Jira key or Kontor ticket exactly; titles and search text are never read. */
export function taskSessions(entries: readonly DirectoryEntry[], task: TaskRef): TaskLink[] {
  const jira = task.jira?.trim().toLocaleUpperCase() || null;
  const ticket = task.kontorId?.trim() || null;
  const links: TaskLink[] = [];
  for (const entry of entries) {
    const key = jira ? TASK_JIRA_LABELS.find((label) => entry.labels[label]?.trim().toLocaleUpperCase() === jira) : undefined;
    if (key) { links.push({ entry, via: key }); continue; }
    if (ticket && entry.labels[TASK_TICKET_LABEL]?.trim() === ticket) links.push({ entry, via: TASK_TICKET_LABEL });
  }
  return links;
}

/** Display order only; it implies no handoff, sequence or completion. */
const ROLE_ORDER = ["LSA", "TPM", "SA", "SWE", "QA", "AUD", "UAT", "SEC", "REV A", "REV B", "JUDGE"];

export type ChainState = OrganizationState | "archived";
export type RoleLane = {
  readonly role: string | null;
  /** Non-archived sessions not replaced by another linked session. */
  readonly current: readonly DirectoryEntry[];
  /** Named as `takeover.predecessor` by another linked session, or archived. */
  readonly earlier: readonly DirectoryEntry[];
};

export function chainState(entry: DirectoryEntry): ChainState {
  return entry.archivedAt !== null ? "archived" : sessionState(entry);
}

/** Linked sessions grouped by labelled role; several sessions in one role stay separate, unlabelled ones stay last. */
export function roleLanes(links: readonly TaskLink[]): RoleLane[] {
  const replaced = new Set(links.map(({ entry }) => entry.labels["takeover.predecessor"]?.trim()).filter((id): id is string => !!id));
  const lanes = new Map<string | null, { current: DirectoryEntry[]; earlier: DirectoryEntry[] }>();
  for (const { entry } of links) {
    const role = labelledRole(entry);
    const lane = lanes.get(role) ?? { current: [], earlier: [] };
    (replaced.has(entry.id) || entry.archivedAt !== null ? lane.earlier : lane.current).push(entry);
    lanes.set(role, lane);
  }
  const rank = (role: string | null) => role === null ? ROLE_ORDER.length + 1 : ROLE_ORDER.includes(role) ? ROLE_ORDER.indexOf(role) : ROLE_ORDER.length;
  return [...lanes].map(([role, lane]) => ({ role, ...lane }))
    .sort((left, right) => rank(left.role) - rank(right.role) || (left.role ?? "").localeCompare(right.role ?? ""));
}

/** Distinct `kontor.team_run` values among the linked sessions (a label the Kontor runtime sets). */
export function teamRuns(links: readonly TaskLink[]): string[] {
  return [...new Set(links.map(({ entry }) => entry.labels["kontor.team_run"]?.trim()).filter((value): value is string => !!value))].sort();
}

export const CHAIN_STATE_LABELS: Readonly<Record<ChainState, string>> = {
  needs_input: "Needs input", failed: "Error", running: "Running", attention: "Attention",
  initializing: "Starting", idle: "Idle", closed: "Closed", archived: "Archived",
};
/** A shape per state, so a dot never relies on colour alone. */
export const CHAIN_STATE_GLYPHS: Readonly<Record<ChainState, string>> = {
  needs_input: "▲", failed: "✕", running: "●", attention: "◆", initializing: "◌", idle: "○", closed: "□", archived: "▫",
};

/** The state word, keeping the runtime apart from a reported cue: an error reported on a session that is idle now. */
export function chainLabel(entry: DirectoryEntry): string {
  const state = chainState(entry);
  return state === "failed" && entry.status !== "error" ? `Error reported, ${entry.status} now` : CHAIN_STATE_LABELS[state];
}

/** Spoken summary of a task's lanes, e.g. "SWE: Running; QA: Needs input and Idle". */
export function chainSummary(lanes: readonly RoleLane[]): string {
  return lanes.filter((lane) => lane.current.length > 0).map((lane) => `${lane.role ?? "Role not labelled"}: ${lane.current.map(chainLabel).join(" and ")}`).join("; ");
}
