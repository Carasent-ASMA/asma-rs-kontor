import type { DirectoryEntry } from "./session-directory";

export const OTHER_WORKSTREAM = "Other work";
export const UNASSIGNED_EPIC = "Unassigned";
export const UNASSIGNED_WORKSPACE = "Unassigned workspace";

export type OrganizationGroupKind =
  | "workstream"
  | "epic"
  | "workspace-kind"
  | "workspace"
  | "role"
  | "roles"
  | "flat";

export type OrganizationAggregate = {
  readonly total: number;
  readonly needsInput: number;
  readonly failed: number;
  readonly running: number;
  readonly attention: number;
  readonly done: number;
  readonly other: number;
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
  readonly aggregate: OrganizationAggregate;
};

export type OrganizationDisplayMapping = {
  readonly workstream?: string;
  readonly epic?: string;
};

export type OrganizationInput = {
  readonly entries: readonly DirectoryEntry[];
  readonly grouping?: "hierarchy" | "role" | "flat";
  readonly favorites?: readonly string[];
  readonly siblingOrder?: Readonly<Record<string, readonly string[]>>;
  readonly displayMappings?: Readonly<Record<string, OrganizationDisplayMapping>>;
  readonly expandedGroups?: Readonly<Record<string, boolean>>;
};

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
  return encodeURIComponent(value.trim().toLocaleLowerCase()) || "unknown";
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

function scopeOf(
  entry: DirectoryEntry,
  mappings: OrganizationInput["displayMappings"],
): { readonly workstream: string; readonly epic: string } {
  const workstream =
    mappedValue(mappings, entry, "workstream") ??
    clean(entry.labels.workstream) ??
    clean(entry.labels.desk) ??
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

function aggregateValues(entries: readonly DirectoryEntry[]): OrganizationAggregate {
  let needsInput = 0;
  let failed = 0;
  let running = 0;
  let attention = 0;
  let done = 0;
  let other = 0;
  for (const entry of entries) {
    if (entry.needsInput) {
      needsInput += 1;
    } else if (entry.status === "error" || entry.attentionReason === "error") {
      failed += 1;
    } else if (entry.status === "running") {
      running += 1;
    } else if (entry.requiresAttention || entry.attentionReason !== null) {
      attention += 1;
    } else if (entry.status === "closed") {
      done += 1;
    } else {
      other += 1;
    }
  }
  return {
    total: entries.length,
    needsInput,
    failed,
    running,
    attention,
    done,
    other,
  };
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
}): OrganizationGroup {
  const entries = [
    ...input.groups.flatMap((child) => child.entries),
    ...input.sessions,
  ];
  return {
    ...input,
    entries,
    totalCount: entries.length,
    visibleCount: entries.length,
    aggregate: aggregateValues(entries),
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
    return left.label.localeCompare(right.label);
  });
}

function workstreamRank(label: string): number {
  const normalized = label.trim().toLocaleLowerCase();
  if (normalized === "pr review" || normalized === "pr-review") {
    return 0;
  }
  if (normalized === "adam") {
    return 1;
  }
  if (normalized === "qa") {
    return 2;
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
    groups.push(
      group({
        key: `${input.parentKey}/role:${key}`,
        kind: "role",
        label: code,
        shortCode: code,
        parentKey: input.parentKey,
        depth: input.depth,
        sessions: sortSessions(
          value.entries,
          `${input.parentKey}/role:${key}`,
          input.siblingOrder,
          input.nativeOrder,
        ),
        groups: [],
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
}): OrganizationGroup[] {
  const byWorkspace = new Map<string, { label: string; entries: DirectoryEntry[] }>();
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

function buildEpicGroup(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly workstreamKey: string;
  readonly epic: string;
  readonly depth: number;
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
}): OrganizationGroup {
  const epicKey = `${input.workstreamKey}/epic:${segment(input.epic)}`;
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
  for (const workspace of workspaces.values()) {
    if (workspace.prefix !== null) {
      const key = normalizedPrefix(workspace.prefix);
      prefixCounts.set(key, (prefixCounts.get(key) ?? 0) + 1);
    }
  }
  const kindGroups = new Map<
    string,
    { label: string; entries: DirectoryEntry[] }
  >();
  const direct: DirectoryEntry[] = [];
  for (const workspace of workspaces.values()) {
    if (
      workspace.prefix !== null &&
      (prefixCounts.get(normalizedPrefix(workspace.prefix)) ?? 0) >= 2
    ) {
      const key = normalizedPrefix(workspace.prefix);
      const existing = kindGroups.get(key);
      if (existing) {
        existing.entries.push(...workspace.entries);
      } else {
        kindGroups.set(key, {
          label: workspace.prefix,
          entries: [...workspace.entries],
        });
      }
    } else {
      direct.push(...workspace.entries);
    }
  }
  const groups: OrganizationGroup[] = [];
  for (const [key, value] of kindGroups) {
    const code = shortCode(value.label);
    const kindKey = `${epicKey}/kind:${key}`;
    groups.push(
      group({
        key: kindKey,
        kind: "workspace-kind",
        label: code,
        shortCode: code,
        parentKey: epicKey,
        depth: input.depth + 1,
        sessions: [],
        groups: buildWorkspaceGroups({
          entries: value.entries,
          parentKey: kindKey,
          depth: input.depth + 2,
          siblingOrder: input.siblingOrder,
          nativeOrder: input.nativeOrder,
        }),
      }),
    );
  }
  groups.push(
    ...buildWorkspaceGroups({
      entries: direct,
      parentKey: epicKey,
      depth: input.depth + 1,
      siblingOrder: input.siblingOrder,
      nativeOrder: input.nativeOrder,
    }),
  );
  return group({
    key: epicKey,
    kind: "epic",
    label: input.epic,
    shortCode: null,
    parentKey: input.workstreamKey,
    depth: input.depth,
    sessions: [],
    groups: sortGroups(groups, epicKey, input.siblingOrder, (value) =>
      value.label === UNASSIGNED_EPIC ? 1 : 0,
    ),
  });
}

function buildHierarchy(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly displayMappings: OrganizationInput["displayMappings"];
  readonly nativeOrder: ReadonlyMap<string, number>;
}): OrganizationGroup[] {
  const workstreams = new Map<
    string,
    {
      label: string;
      epics: Map<string, { label: string; entries: DirectoryEntry[] }>;
    }
  >();
  for (const entry of input.entries) {
    const scope = scopeOf(entry, input.displayMappings);
    const workstreamKey = normalizedPrefix(scope.workstream);
    let workstream = workstreams.get(workstreamKey);
    if (!workstream) {
      workstream = { label: scope.workstream, epics: new Map() };
      workstreams.set(workstreamKey, workstream);
    }
    const epicKey = normalizedPrefix(scope.epic);
    const epic = workstream.epics.get(epicKey);
    if (epic) {
      epic.entries.push(entry);
    } else {
      workstream.epics.set(epicKey, { label: scope.epic, entries: [entry] });
    }
  }
  const groups: OrganizationGroup[] = [];
  for (const [key, workstream] of workstreams) {
    const workstreamKey = `workstream:${key}`;
    const epics = [...workstream.epics.values()].map((epic) =>
      buildEpicGroup({
        entries: epic.entries,
        workstreamKey,
        epic: epic.label,
        depth: 1,
        siblingOrder: input.siblingOrder,
        nativeOrder: input.nativeOrder,
      }),
    );
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
      }),
    );
  }
  return sortGroups(groups, "root", input.siblingOrder, (value) =>
    workstreamRank(value.label),
  );
}

function buildRoleRoot(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly siblingOrder: OrganizationInput["siblingOrder"];
  readonly nativeOrder: ReadonlyMap<string, number>;
}): OrganizationGroup[] {
  const rootKey = "roles";
  const roles = buildRoleGroups({
    entries: input.entries,
    parentKey: rootKey,
    depth: 1,
    siblingOrder: input.siblingOrder,
    nativeOrder: input.nativeOrder,
  });
  return [
    group({
      key: rootKey,
      kind: "roles",
      label: "Roles",
      shortCode: null,
      parentKey: null,
      depth: 0,
      sessions: roles.sessions,
      groups: roles.groups,
    }),
  ];
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
      nativeOrder,
    });
  }
  return buildHierarchy({
    entries: input.entries,
    siblingOrder: input.siblingOrder,
    displayMappings: input.displayMappings,
    nativeOrder,
  });
}

function filterGroup(
  current: OrganizationGroup,
  predicate: (entry: DirectoryEntry) => boolean,
): OrganizationGroup | null {
  const sessions = current.sessions.filter(predicate);
  const groups = current.groups
    .map((child) => filterGroup(child, predicate))
    .filter((child): child is OrganizationGroup => child !== null);
  const visibleCount =
    sessions.length + groups.reduce((count, child) => count + child.visibleCount, 0);
  if (visibleCount === 0) {
    return null;
  }
  return { ...current, sessions, groups, visibleCount };
}

export function filterOrganizationTree(
  groups: readonly OrganizationGroup[],
  predicate: (entry: DirectoryEntry) => boolean,
): OrganizationGroup[] {
  return groups
    .map((group) => filterGroup(group, predicate))
    .filter((group): group is OrganizationGroup => group !== null);
}

export function flattenOrganizationTree(
  groups: readonly OrganizationGroup[],
  expandedGroups: Readonly<Record<string, boolean>> = {},
  forceExpand = false,
): OrganizationRow[] {
  const rows: OrganizationRow[] = [];
  function visit(current: OrganizationGroup): void {
    const expanded = forceExpand || expandedGroups[current.key] !== false;
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
  }
  for (const group of groups) {
    visit(group);
  }
  return rows;
}

export function isFavoriteSession(
  entry: DirectoryEntry,
  favorites: readonly string[],
): boolean {
  return favorites.includes(entry.id);
}
