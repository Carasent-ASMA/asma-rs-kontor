import type { DirectoryEntry } from "./session-directory";

export const OTHER_WORKSTREAM = "Other work";
export const UNASSIGNED_EPIC = "Unassigned";
export const UNASSIGNED_WORKSPACE = "Unassigned workspace";

export type OrganizationGroupKind =
  | "workstream"
  | "epic"
  | "workspace-kind"
  | "workspace"
  | "project"
  | "role"
  | "roles"
  | "flat";

/** Each session counts once, in the first state of STATE_PRIORITY it shows. */
export type OrganizationAggregate = {
  readonly total: number;
  readonly needsInput: number;
  readonly failed: number;
  readonly running: number;
  readonly attention: number;
  readonly initializing: number;
  readonly idle: number;
  readonly closed: number;
  readonly native: Readonly<Record<DirectoryEntry["status"], number>>;
};

export type OrganizationState =
  | "needs_input"
  | "failed"
  | "running"
  | "attention"
  | "initializing"
  | "idle"
  | "closed";

export const STATE_PRIORITY: readonly OrganizationState[] = [
  "needs_input",
  "failed",
  "running",
  "attention",
  "initializing",
  "idle",
  "closed",
];

export const STATE_LABELS: Readonly<Record<OrganizationState, string>> = {
  needs_input: "Needs input",
  failed: "Error",
  running: "Running",
  attention: "Attention",
  initializing: "Starting",
  idle: "Idle",
  closed: "Closed",
};

const STATE_COUNTS: Readonly<Record<OrganizationState, keyof Omit<OrganizationAggregate, "total" | "native">>> = {
  needs_input: "needsInput",
  failed: "failed",
  running: "running",
  attention: "attention",
  initializing: "initializing",
  idle: "idle",
  closed: "closed",
};

export type OrganizationGroup = {
  readonly key: string;
  readonly kind: OrganizationGroupKind;
  readonly label: string;
  readonly shortCode: string | null;
  readonly parentKey: string | null;
  readonly depth: number;
  readonly sessions: readonly DirectoryEntry[];
  readonly groups: readonly OrganizationGroup[];
  readonly entries: readonly DirectoryEntry[];
  readonly totalCount: number;
  readonly visibleCount: number;
  /** Summarizes the visible sessions once a filter has run. */
  readonly aggregate: OrganizationAggregate;
  /** Secondary context beside the label, such as the native placement of a Role group. */
  readonly detail: string | null;
  /** Resolved epic of an epic group; null elsewhere and for Unassigned. */
  readonly epic: string | null;
  /** Workspace-kind groups count workspaces; every other group counts sessions. */
  readonly unit: "sessions" | "workspaces";
  readonly unitTotal: number;
  readonly unitVisible: number;
};

export type OrganizationDisplayMapping = {
  readonly workstream?: string;
  readonly epic?: string;
};

export type OrganizationInput = {
  readonly entries: readonly DirectoryEntry[];
  readonly grouping?: "hierarchy" | "placement" | "role" | "flat";
  readonly favorites?: readonly string[];
  readonly siblingOrder?: Readonly<Record<string, readonly string[]>>;
  readonly displayMappings?: Readonly<Record<string, OrganizationDisplayMapping>>;
  readonly expandedGroups?: Readonly<Record<string, boolean>>;
  readonly workspaces?: readonly OrganizationWorkspace[];
};
export type OrganizationWorkspace = { readonly id: string; readonly name: string; readonly projectKey?: string | null; readonly projectName?: string | null };

export type OrganizationRow =
  | {
      readonly kind: "group";
      readonly group: OrganizationGroup;
      readonly depth: number;
      readonly expanded: boolean;
    }
  | {
      readonly kind: "session";
      readonly entry: DirectoryEntry;
      readonly depth: number;
      readonly parentKey: string;
    }
  | {
      /** An expanded workspace with no non-archived sessions says so instead of opening onto nothing. */
      readonly kind: "empty";
      readonly depth: number;
      readonly parentKey: string;
    };

const SEPARATOR = "\u2022";

function clean(value: string | null | undefined): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function segment(value: string): string {
  return encodeURIComponent(value.trim()) || "unknown";
}

export function sessionKey(id: string): string {
  return `session:${id}`;
}

export function workspaceMappingKey(workspaceId: string): string {
  return `workspace:${workspaceId}`;
}

export function sessionMappingKey(sessionId: string): string {
  return `session:${sessionId}`;
}

export function prefixBeforeSeparator(value: string | null | undefined): string | null {
  const trimmed = clean(value);
  if (trimmed === null) {
    return null;
  }
  const separator = trimmed.indexOf(SEPARATOR);
  if (separator < 0) {
    return null;
  }
  return clean(trimmed.slice(0, separator));
}

export function workspaceKindPrefix(workspaceName: string | null | undefined): string | null {
  return prefixBeforeSeparator(workspaceName);
}

export function sessionRolePrefix(title: string | null | undefined): string | null {
  return prefixBeforeSeparator(title);
}

function shortCode(prefix: string): string {
  return prefix.toLocaleUpperCase();
}

function normalizedPrefix(prefix: string): string {
  return prefix.trim().toLocaleLowerCase();
}

function mappedValue(
  mappings: OrganizationInput["displayMappings"],
  entry: DirectoryEntry,
  field: "workstream" | "epic",
): string | null {
  const session = mappings?.[sessionMappingKey(entry.id)]?.[field];
  if (clean(session) !== null) {
    return clean(session);
  }
  if (entry.workspaceId) {
    const workspace = mappings?.[workspaceMappingKey(entry.workspaceId)]?.[field];
    if (clean(workspace) !== null) {
      return clean(workspace);
    }
  }
  return null;
}

export function organizationScope(
  entry: DirectoryEntry,
  mappings: OrganizationInput["displayMappings"],
): { readonly workstream: string; readonly epic: string } {
  const workstream =
    mappedValue(mappings, entry, "workstream") ??
    clean(entry.labels.desk) ??
    clean(entry.labels.workstream) ??
    OTHER_WORKSTREAM;
  const epic =
    mappedValue(mappings, entry, "epic") ??
    clean(entry.labels.epic) ??
    clean(entry.labels["jira.epic"]) ??
    clean(entry.labels["origin.epic"]) ??
    UNASSIGNED_EPIC;
  return { workstream, epic };
}

function workspaceOf(entry: DirectoryEntry): {
  readonly key: string;
  readonly label: string;
} {
  if (entry.workspaceId) {
    return {
      key: `id:${entry.workspaceId}`,
      label:
        clean(entry.workspaceName) ??
        clean(entry.projectName) ??
        "Workspace",
    };
  }
  const name = clean(entry.workspaceName);
  if (name !== null) {
    return { key: `name:${normalizedPrefix(name)}`, label: name };
  }
  const cwd = clean(entry.cwd);
  if (cwd !== null) {
    return { key: `cwd:${cwd}`, label: cwd };
  }
  return { key: "unassigned", label: UNASSIGNED_WORKSPACE };
}

export function sessionState(entry: DirectoryEntry): OrganizationState {
  if (entry.needsInput) {
    return "needs_input";
  }
  if (entry.status === "error" || entry.attentionReason === "error") {
    return "failed";
  }
  if (entry.status === "running") {
    return "running";
  }
  if (entry.requiresAttention || entry.attentionReason !== null) {
    return "attention";
  }
  return entry.status;
}

function aggregateValues(entries: readonly DirectoryEntry[]): OrganizationAggregate {
  const counts = { needsInput: 0, failed: 0, running: 0, attention: 0, initializing: 0, idle: 0, closed: 0 };
  const native = { initializing: 0, idle: 0, running: 0, error: 0, closed: 0 };
  for (const entry of entries) {
    native[entry.status] += 1;
    counts[STATE_COUNTS[sessionState(entry)]] += 1;
  }
  return { total: entries.length, ...counts, native };
}

/** Highest-priority state among the counted sessions, or null when there are none. */
export function aggregateState(aggregate: OrganizationAggregate): OrganizationState | null {
  return STATE_PRIORITY.find((state) => aggregate[STATE_COUNTS[state]] > 0) ?? null;
}

export function aggregateSummary(aggregate: OrganizationAggregate): string {
  const parts = STATE_PRIORITY.filter((state) => aggregate[STATE_COUNTS[state]] > 0).map(
    (state) => `${aggregate[STATE_COUNTS[state]]} ${STATE_LABELS[state].toLocaleLowerCase()}`,
  );
  return parts.length > 0 ? parts.join(" · ") : "No sessions";
}

function group(input: {
  readonly key: string;
  readonly kind: OrganizationGroupKind;
  readonly label: string;
  readonly shortCode: string | null;
  readonly parentKey: string | null;
  readonly depth: number;
  readonly sessions: readonly DirectoryEntry[];
  readonly groups: readonly OrganizationGroup[];
  readonly detail?: string | null;
  readonly epic?: string | null;
}): OrganizationGroup {
  const entries = [
    ...input.groups.flatMap((child) => child.entries),
    ...input.sessions,
  ];
  const unit = input.kind === "workspace-kind" ? "workspaces" : "sessions";
  const unitTotal = unit === "workspaces" ? input.groups.length : entries.length;
  return {
    ...input,
    detail: input.detail ?? null,
    epic: input.epic ?? null,
    entries,
    totalCount: entries.length,
    visibleCount: entries.length,
    aggregate: aggregateValues(entries),
    unit,
    unitTotal,
    unitVisible: unitTotal,
  };
}

function orderIndex(
  siblingOrder: OrganizationInput["siblingOrder"],
  parentKey: string,
): Map<string, number> {
  const order = siblingOrder?.[parentKey] ?? [];
  return new Map(order.map((key, index) => [key, index]));
}

function sortSessions(
  entries: readonly DirectoryEntry[],
  parentKey: string,
  siblingOrder: OrganizationInput["siblingOrder"],
  nativeOrder: ReadonlyMap<string, number>,
): DirectoryEntry[] {
  const order = orderIndex(siblingOrder, parentKey);
  return [...entries].sort((left, right) => {
    const leftOrder = order.get(sessionKey(left.id));
    const rightOrder = order.get(sessionKey(right.id));
    if (leftOrder !== undefined || rightOrder !== undefined) {
      return (leftOrder ?? Number.MAX_SAFE_INTEGER) - (rightOrder ?? Number.MAX_SAFE_INTEGER);
    }
    return (nativeOrder.get(left.id) ?? 0) - (nativeOrder.get(right.id) ?? 0);
  });
}

function sortGroups(
  groups: readonly OrganizationGroup[],
  parentKey: string,
  siblingOrder: OrganizationInput["siblingOrder"],
  rank?: (group: OrganizationGroup) => number,
): OrganizationGroup[] {
  const order = orderIndex(siblingOrder, parentKey);
  return [...groups].sort((left, right) => {
    const leftRank = rank?.(left) ?? 0;
    const rightRank = rank?.(right) ?? 0;
    if (leftRank !== rightRank) {
      return leftRank - rightRank;
    }
    const leftOrder = order.get(left.key);
    const rightOrder = order.get(right.key);
    if (leftOrder !== undefined || rightOrder !== undefined) {
      return (leftOrder ?? Number.MAX_SAFE_INTEGER) - (rightOrder ?? Number.MAX_SAFE_INTEGER);
    }
    if (parentKey === "root") {
      const defaults = ["pr review", "pr-review", "adam", "qa"];
      const leftDefault = defaults.indexOf(left.label.toLocaleLowerCase());
      const rightDefault = defaults.indexOf(right.label.toLocaleLowerCase());
      if (leftDefault >= 0 && rightDefault >= 0) return leftDefault - rightDefault;
    }
    return left.label.localeCompare(right.label);
  });
}

export function workstreamRank(label: string): number {
  const normalized = label.trim().toLocaleLowerCase();
  if (normalized === "pr review" || normalized === "pr-review") {
    return 0;
  }
  if (normalized === "adam") {
    return 0;
  }
  if (normalized === "qa") {
    return 0;
  }
  if (normalized === OTHER_WORKSTREAM.toLocaleLowerCase()) {
    return 100;
  }
  return 50;
}

function buildRoleGroups(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly parentKey: string;
  readonly depth: number;
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  /** Keeps keys unique when several native scopes share one displayed parent. */
  readonly scopeKey?: string;
  readonly detail?: string;
}): { readonly sessions: DirectoryEntry[]; readonly groups: OrganizationGroup[] } {
  const byPrefix = new Map<string, { label: string; entries: DirectoryEntry[] }>();
  const direct: DirectoryEntry[] = [];
  for (const entry of input.entries) {
    const prefix = sessionRolePrefix(entry.title);
    if (prefix === null) {
      direct.push(entry);
      continue;
    }
    const key = normalizedPrefix(prefix);
    const existing = byPrefix.get(key);
    if (existing) {
      existing.entries.push(entry);
    } else {
      byPrefix.set(key, { label: prefix, entries: [entry] });
    }
  }
  const groups: OrganizationGroup[] = [];
  for (const [key, value] of byPrefix) {
    if (value.entries.length < 2) {
      direct.push(...value.entries);
      continue;
    }
    const code = shortCode(value.label);
    const roleKey = `${input.scopeKey ?? input.parentKey}/role:${key}`;
    groups.push(
      group({
        key: roleKey,
        kind: "role",
        label: code,
        shortCode: code,
        parentKey: input.parentKey,
        depth: input.depth,
        sessions: sortSessions(
          value.entries,
          roleKey,
          input.siblingOrder,
          input.nativeOrder,
        ),
        groups: [],
        detail: input.detail,
      }),
    );
  }
  return {
    sessions: sortSessions(direct, input.parentKey, input.siblingOrder, input.nativeOrder),
    groups: sortGroups(groups, input.parentKey, input.siblingOrder),
  };
}

function buildWorkspaceGroups(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly parentKey: string;
  readonly depth: number;
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  readonly emptyWorkspaces?: readonly OrganizationWorkspace[];
}): OrganizationGroup[] {
  const byWorkspace = new Map<string, { label: string; entries: DirectoryEntry[] }>();
  for (const workspace of input.emptyWorkspaces ?? []) byWorkspace.set(`id:${workspace.id}`, { label: workspace.name, entries: [] });
  for (const entry of input.entries) {
    const workspace = workspaceOf(entry);
    const existing = byWorkspace.get(workspace.key);
    if (existing) {
      existing.entries.push(entry);
    } else {
      byWorkspace.set(workspace.key, { label: workspace.label, entries: [entry] });
    }
  }
  return sortGroups(
    [...byWorkspace.entries()].map(([key, value]) => {
      const workspaceKey = `${input.parentKey}/workspace:${segment(key)}`;
      const roles = buildRoleGroups({
        entries: value.entries,
        parentKey: workspaceKey,
        depth: input.depth + 1,
        siblingOrder: input.siblingOrder,
        nativeOrder: input.nativeOrder,
      });
      return group({
        key: workspaceKey,
        kind: "workspace",
        label: value.label,
        shortCode: null,
        parentKey: input.parentKey,
        depth: input.depth,
        sessions: roles.sessions,
        groups: roles.groups,
      });
    }),
    input.parentKey,
    input.siblingOrder,
  );
}

/** Workspaces under one parent; a workspace-name prefix shared by two or more becomes a kind group. */
function workspaceChildren(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly parentKey: string;
  readonly depth: number;
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  readonly emptyWorkspaces?: readonly OrganizationWorkspace[];
}): OrganizationGroup[] {
  const workspaces = new Map<
    string,
    { label: string; prefix: string | null; entries: DirectoryEntry[] }
  >();
  for (const entry of input.entries) {
    const workspace = workspaceOf(entry);
    const existing = workspaces.get(workspace.key);
    if (existing) {
      existing.entries.push(entry);
    } else {
      workspaces.set(workspace.key, {
        label: workspace.label,
        prefix: workspaceKindPrefix(workspace.label),
        entries: [entry],
      });
    }
  }
  const prefixCounts = new Map<string, number>();
  for (const workspace of input.emptyWorkspaces ?? []) workspaces.set(`id:${workspace.id}`, { label: workspace.name, prefix: workspaceKindPrefix(workspace.name), entries: [] });
  for (const workspace of workspaces.values()) {
    if (workspace.prefix !== null) {
      const key = normalizedPrefix(workspace.prefix);
      prefixCounts.set(key, (prefixCounts.get(key) ?? 0) + 1);
    }
  }
  const kindGroups = new Map<
    string,
    { label: string; entries: DirectoryEntry[]; emptyWorkspaces: OrganizationWorkspace[] }
  >();
  const direct: DirectoryEntry[] = [];
  const directEmpty: OrganizationWorkspace[] = [];
  const empties = new Map((input.emptyWorkspaces ?? []).map((workspace) => [`id:${workspace.id}`, workspace]));
  for (const [workspaceKey, workspace] of workspaces) {
    if (
      workspace.prefix !== null &&
      (prefixCounts.get(normalizedPrefix(workspace.prefix)) ?? 0) >= 2
    ) {
      const key = normalizedPrefix(workspace.prefix);
      const existing = kindGroups.get(key);
      if (existing) {
        existing.entries.push(...workspace.entries);
        if (!workspace.entries.length && empties.has(workspaceKey)) existing.emptyWorkspaces.push(empties.get(workspaceKey)!);
      } else {
        kindGroups.set(key, {
          label: workspace.prefix,
          entries: [...workspace.entries],
          emptyWorkspaces: !workspace.entries.length && empties.has(workspaceKey) ? [empties.get(workspaceKey)!] : [],
        });
      }
    } else {
      direct.push(...workspace.entries);
      if (!workspace.entries.length && empties.has(workspaceKey)) directEmpty.push(empties.get(workspaceKey)!);
    }
  }
  const groups: OrganizationGroup[] = [];
  for (const [key, value] of kindGroups) {
    const code = shortCode(value.label);
    const kindKey = `${input.parentKey}/kind:${key}`;
    groups.push(
      group({
        key: kindKey,
        kind: "workspace-kind",
        label: code,
        shortCode: code,
        parentKey: input.parentKey,
        depth: input.depth,
        sessions: [],
        groups: buildWorkspaceGroups({
          entries: value.entries,
          parentKey: kindKey,
          depth: input.depth + 1,
          siblingOrder: input.siblingOrder,
          nativeOrder: input.nativeOrder,
          emptyWorkspaces: value.emptyWorkspaces,
        }),
      }),
    );
  }
  groups.push(
    ...buildWorkspaceGroups({
      entries: direct,
      parentKey: input.parentKey,
      depth: input.depth,
      siblingOrder: input.siblingOrder,
      nativeOrder: input.nativeOrder,
      emptyWorkspaces: directEmpty,
    }),
  );
  return sortGroups(groups, input.parentKey, input.siblingOrder, (value) =>
    value.label === UNASSIGNED_EPIC ? 1 : 0,
  );
}

function buildEpicGroup(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly workstreamKey: string;
  readonly epic: string;
  readonly depth: number;
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  readonly emptyWorkspaces?: readonly OrganizationWorkspace[];
  readonly projectKey?: string;
  readonly displayLabel?: string;
}): OrganizationGroup {
  const epicKey = `${input.workstreamKey}/project:${segment(input.projectKey ?? "unknown")}/epic:${segment(normalizedPrefix(input.epic))}`;
  return group({
    key: epicKey,
    kind: "epic",
    label: input.displayLabel ?? input.epic,
    shortCode: null,
    parentKey: input.workstreamKey,
    depth: input.depth,
    sessions: [],
    groups: workspaceChildren({
      entries: input.entries,
      parentKey: epicKey,
      depth: input.depth + 1,
      siblingOrder: input.siblingOrder,
      nativeOrder: input.nativeOrder,
      emptyWorkspaces: input.emptyWorkspaces,
    }),
    epic: input.epic === UNASSIGNED_EPIC ? null : input.epic,
  });
}

function buildHierarchy(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly displayMappings: OrganizationInput["displayMappings"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  readonly workspaces?: readonly OrganizationWorkspace[];
}): OrganizationGroup[] {
  const workstreams = new Map<
    string,
    {
      label: string;
      epics: Map<string, { label: string; projectKey: string; projectName: string; entries: DirectoryEntry[]; emptyWorkspaces?: OrganizationWorkspace[] }>;
    }
  >();
  for (const entry of input.entries) {
    const scope = organizationScope(entry, input.displayMappings);
    const workstreamKey = normalizedPrefix(scope.workstream);
    let workstream = workstreams.get(workstreamKey);
    if (!workstream) {
      workstream = { label: scope.workstream, epics: new Map() };
      workstreams.set(workstreamKey, workstream);
    }
    const projectKey = entry.projectKey ?? `unknown:${entry.workspaceId ?? entry.cwd ?? entry.id}`;
    const epicKey = JSON.stringify([projectKey, normalizedPrefix(scope.epic)]);
    const epic = workstream.epics.get(epicKey);
    if (epic) {
      epic.entries.push(entry);
    } else {
      workstream.epics.set(epicKey, { label: scope.epic, projectKey, projectName: entry.projectName ?? "Project unknown", entries: [entry] });
    }
  }
  const groups: OrganizationGroup[] = [];
  const occupied = new Set(input.entries.map((entry) => entry.workspaceId));
  for (const workspace of input.workspaces ?? []) {
    if (occupied.has(workspace.id)) continue;
    const mapping = input.displayMappings?.[workspaceMappingKey(workspace.id)];
    const stream = mapping?.workstream ?? OTHER_WORKSTREAM;
    const epic = mapping?.epic ?? UNASSIGNED_EPIC;
    const streamKey = normalizedPrefix(stream);
    let parent = workstreams.get(streamKey);
    if (!parent) { parent = { label: stream, epics: new Map() }; workstreams.set(streamKey, parent); }
    const projectKey = workspace.projectKey ?? `unknown:${workspace.id}`;
    const epicKey = JSON.stringify([projectKey, normalizedPrefix(epic)]);
    let target = parent.epics.get(epicKey);
    if (!target) { target = { label: epic, projectKey, projectName: workspace.projectName ?? "Project unknown", entries: [], emptyWorkspaces: [] }; parent.epics.set(epicKey, target); }
    target.emptyWorkspaces = [...(target.emptyWorkspaces ?? []), workspace];
  }
  for (const [key, workstream] of workstreams) {
    const workstreamKey = `workstream:${key}`;
    const multipleProjects = new Set([...workstream.epics.values()].map((epic) => epic.projectKey)).size > 1;
    const epics = [...workstream.epics.values()].map((epic) =>
      buildEpicGroup({
        entries: epic.entries,
        workstreamKey,
        epic: epic.label,
        depth: 1,
        siblingOrder: input.siblingOrder,
        nativeOrder: input.nativeOrder,
        emptyWorkspaces: epic.emptyWorkspaces,
        projectKey: epic.projectKey,
        displayLabel: multipleProjects ? `${epic.label} · ${epic.projectName}` : epic.label,
      }),
    );
    const assigned = epics.filter((epic) => epic.epic !== null).length;
    groups.push(
      group({
        key: workstreamKey,
        kind: "workstream",
        label: workstream.label,
        shortCode: null,
        parentKey: null,
        depth: 0,
        sessions: [],
        groups: sortGroups(epics, workstreamKey, input.siblingOrder, (epic) =>
          epic.label === UNASSIGNED_EPIC ? 1 : 0,
        ),
        detail: assigned > 0 ? `${assigned} epic${assigned === 1 ? "" : "s"}` : null,
      }),
    );
  }
  return sortGroups(groups, "root", input.siblingOrder, (value) =>
    workstreamRank(value.label),
  );
}

/**
 * Repeated role prefixes group only within one native project, workspace and
 * display scope; the groups and singletons then sit together under Roles.
 */
function buildRoleRoot(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly displayMappings: OrganizationInput["displayMappings"];
  readonly nativeOrder: ReadonlyMap<string, number>;
}): OrganizationGroup[] {
  const rootKey = "roles";
  const partitions = new Map<string, { detail: string; rank: number; entries: DirectoryEntry[] }>();
  for (const entry of input.entries) {
    const scope = organizationScope(entry, input.displayMappings);
    const workspace = workspaceOf(entry);
    const key = JSON.stringify([entry.projectKey ?? "", workspace.key, normalizedPrefix(scope.workstream), normalizedPrefix(scope.epic)]);
    const existing = partitions.get(key);
    if (existing) {
      existing.entries.push(entry);
    } else {
      partitions.set(key, {
        detail: `${scope.workstream} · ${scope.epic} · ${workspace.label}`,
        rank: workstreamRank(scope.workstream),
        entries: [entry],
      });
    }
  }
  const sessions: DirectoryEntry[] = [];
  const groups: OrganizationGroup[] = [];
  const ranks = new Map<string, number>();
  for (const [key, partition] of partitions) {
    const roles = buildRoleGroups({
      entries: partition.entries,
      parentKey: rootKey,
      scopeKey: `${rootKey}/scope:${segment(key)}`,
      depth: 1,
      siblingOrder: input.siblingOrder,
      nativeOrder: input.nativeOrder,
      detail: partition.detail,
    });
    sessions.push(...roles.sessions);
    for (const role of roles.groups) {
      ranks.set(role.key, partition.rank);
      groups.push(role);
    }
  }
  return [
    group({
      key: rootKey,
      kind: "roles",
      label: "Roles",
      shortCode: null,
      parentKey: null,
      depth: 0,
      sessions: sortSessions(sessions, rootKey, input.siblingOrder, input.nativeOrder),
      groups: sortGroups(groups, rootKey, input.siblingOrder, (role) => ranks.get(role.key) ?? 50),
    }),
  ];
}

function countWorkspaces(groups: readonly OrganizationGroup[]): number {
  return groups.reduce(
    (count, child) => count + (child.kind === "workspace" ? 1 : countWorkspaces(child.groups)),
    0,
  );
}

/** Native project → workspace kind → workspace → role, as Paseo places the sessions. */
function buildPlacement(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
  readonly workspaces?: readonly OrganizationWorkspace[];
}): OrganizationGroup[] {
  const projects = new Map<string, { label: string; entries: DirectoryEntry[]; empty: OrganizationWorkspace[] }>();
  const project = (key: string, label: string) => {
    let current = projects.get(key);
    if (!current) {
      current = { label, entries: [], empty: [] };
      projects.set(key, current);
    }
    return current;
  };
  for (const entry of input.entries) {
    project(entry.projectKey ?? `unknown:${entry.workspaceId ?? entry.cwd ?? entry.id}`, entry.projectName ?? "Project unknown").entries.push(entry);
  }
  const occupied = new Set(input.entries.map((entry) => entry.workspaceId));
  for (const workspace of input.workspaces ?? []) {
    if (!occupied.has(workspace.id)) project(workspace.projectKey ?? `unknown:${workspace.id}`, workspace.projectName ?? "Project unknown").empty.push(workspace);
  }
  const groups = [...projects].map(([key, value]) => {
    const projectKey = `project:${segment(key)}`;
    const children = workspaceChildren({
      entries: value.entries,
      parentKey: projectKey,
      depth: 1,
      siblingOrder: input.siblingOrder,
      nativeOrder: input.nativeOrder,
      emptyWorkspaces: value.empty,
    });
    const workspaces = countWorkspaces(children);
    return group({
      key: projectKey,
      kind: "project",
      label: value.label,
      shortCode: null,
      parentKey: null,
      depth: 0,
      sessions: [],
      groups: children,
      detail: `${workspaces} workspace${workspaces === 1 ? "" : "s"}`,
    });
  });
  return sortGroups(groups, "root", input.siblingOrder, (value) =>
    value.label.toLocaleUpperCase().startsWith("DESK") ? 0 : 1,
  );
}

function buildFlatRoot(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
}): OrganizationGroup[] {
  const rootKey = "flat";
  return [
    group({
      key: rootKey,
      kind: "flat",
      label: "All sessions",
      shortCode: null,
      parentKey: null,
      depth: 0,
      sessions: sortSessions(
        input.entries,
        rootKey,
        input.siblingOrder,
        input.nativeOrder,
      ),
      groups: [],
    }),
  ];
}

export function buildOrganizationTree(input: OrganizationInput): OrganizationGroup[] {
  const nativeOrder = new Map(input.entries.map((entry, index) => [entry.id, index]));
  if (input.grouping === "flat") {
    return buildFlatRoot({
      entries: input.entries,
      siblingOrder: input.siblingOrder,
      nativeOrder,
    });
  }
  if (input.grouping === "role") {
    return buildRoleRoot({
      entries: input.entries,
      siblingOrder: input.siblingOrder,
      displayMappings: input.displayMappings,
      nativeOrder,
    });
  }
  if (input.grouping === "placement") {
    return buildPlacement({
      entries: input.entries,
      siblingOrder: input.siblingOrder,
      nativeOrder,
      workspaces: input.workspaces,
    });
  }
  return buildHierarchy({
    entries: input.entries,
    siblingOrder: input.siblingOrder,
    displayMappings: input.displayMappings,
    nativeOrder,
    workspaces: input.workspaces,
  });
}

function filterGroup(
  current: OrganizationGroup,
  predicate: (entry: DirectoryEntry) => boolean,
  matchesGroup: (group: OrganizationGroup) => boolean,
  matchesDescendant: (entry: DirectoryEntry) => boolean,
  ancestorMatched = false,
): { readonly group: OrganizationGroup; readonly visible: readonly DirectoryEntry[] } | null {
  const matched = ancestorMatched || matchesGroup(current);
  const sessions = current.sessions.filter(matched ? matchesDescendant : predicate);
  const children = current.groups
    .map((child) => filterGroup(child, predicate, matchesGroup, matchesDescendant, matched))
    .filter((child): child is NonNullable<typeof child> => child !== null);
  const groups = children.map((child) => child.group);
  const visible = [...children.flatMap((child) => child.visible), ...sessions];
  if (visible.length === 0 && groups.length === 0 && !(current.kind === "workspace" && current.totalCount === 0 && matched)) {
    return null;
  }
  return {
    group: {
      ...current,
      sessions,
      groups,
      visibleCount: visible.length,
      aggregate: aggregateValues(visible),
      unitVisible: current.unit === "workspaces" ? groups.length : visible.length,
    },
    visible,
  };
}

export function filterOrganizationTree(
  groups: readonly OrganizationGroup[],
  predicate: (entry: DirectoryEntry) => boolean,
  matchesGroup: (group: OrganizationGroup) => boolean = () => false,
  matchesDescendant: (entry: DirectoryEntry) => boolean = () => true,
): OrganizationGroup[] {
  return groups
    .map((group) => filterGroup(group, predicate, matchesGroup, matchesDescendant)?.group ?? null)
    .filter((group): group is OrganizationGroup => group !== null);
}

/**
 * First-visit disclosure, following the prototype: workstreams, kind and role
 * groups open; epics and workspaces start closed; desk projects open.
 */
export function defaultGroupExpanded(group: OrganizationGroup): boolean {
  switch (group.kind) {
    case "epic":
    case "workspace":
      return false;
    case "project":
      return group.label.toLocaleUpperCase().startsWith("DESK");
    default:
      return true;
  }
}

/** Sessions a filtered tree still contains, whether or not their branch is expanded. */
export function visibleSessions(groups: readonly OrganizationGroup[]): DirectoryEntry[] {
  return groups.flatMap((group) => [...visibleSessions(group.groups), ...group.sessions]);
}

export function allGroupKeys(groups: readonly OrganizationGroup[]): string[] {
  return groups.flatMap((group) => [group.key, ...allGroupKeys(group.groups)]);
}

/**
 * Saved or local disclosure wins over the default; `forceExpand` reveals every
 * match during a search or state filter without changing what is saved.
 */
export function flattenOrganizationTree(
  groups: readonly OrganizationGroup[],
  expandedGroups: Readonly<Record<string, boolean>> = {},
  forceExpand = false,
  defaultExpanded: (group: OrganizationGroup) => boolean = () => true,
): OrganizationRow[] {
  const rows: OrganizationRow[] = [];
  function visit(current: OrganizationGroup): void {
    const expanded = forceExpand || (expandedGroups[current.key] ?? defaultExpanded(current));
    rows.push({
      kind: "group",
      group: current,
      depth: current.depth,
      expanded,
    });
    if (!expanded) {
      return;
    }
    for (const child of current.groups) {
      visit(child);
    }
    for (const entry of current.sessions) {
      rows.push({
        kind: "session",
        entry,
        depth: current.depth + 1,
        parentKey: current.key,
      });
    }
    if (current.kind === "workspace" && current.groups.length === 0 && current.sessions.length === 0) {
      rows.push({ kind: "empty", depth: current.depth + 1, parentKey: current.key });
    }
  }
  for (const group of groups) {
    visit(group);
  }
  return rows;
}

/** The display-scope resolver the tree uses, so a selected epic shows the same members. */
export function inEpicScope(
  entry: DirectoryEntry,
  mappings: OrganizationInput["displayMappings"],
  epic: string,
): boolean {
  return normalizedPrefix(organizationScope(entry, mappings).epic) === normalizedPrefix(epic);
}

/** Desk work is a desk workstream (PR Review, Adam, QA) or a native desk label without a workstream mapping. */
export function isDeskWork(
  entry: DirectoryEntry,
  mappings: OrganizationInput["displayMappings"],
): boolean {
  return (
    workstreamRank(organizationScope(entry, mappings).workstream) === 0 ||
    (mappedValue(mappings, entry, "workstream") === null && clean(entry.labels.desk) !== null)
  );
}

export function isFavoriteSession(
  entry: DirectoryEntry,
  favorites: readonly string[],
): boolean {
  return favorites.includes(entry.id);
}
