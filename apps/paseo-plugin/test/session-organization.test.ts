import { describe, expect, it } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import {
  aggregateState,
  aggregateSummary,
  buildOrganizationTree,
  defaultGroupExpanded,
  filterOrganizationTree,
  inEpicScope,
  isDeskWork,
  workspaceMappingKey,
  flattenOrganizationTree,
  isFavoriteSession,
  OTHER_WORKSTREAM,
  sessionKey,
  UNASSIGNED_EPIC,
  UNASSIGNED_WORKSPACE,
  workspaceKindPrefix,
  sessionRolePrefix,
  organizationScope,
  type OrganizationGroup,
} from "../client/session-organization";
import { sessionEntry } from "./session-fixtures";

function entry(
  id: string,
  input: {
    title?: string;
    workspaceName?: string | null;
    workspaceId?: string | null;
    labels?: Record<string, string>;
    status?: DirectoryEntry["status"];
    requiresAttention?: boolean;
    attentionReason?: DirectoryEntry["attentionReason"];
    pendingPermissionCount?: number;
    cwd?: string;
    projectKey?: string;
    projectName?: string;
  } = {},
): DirectoryEntry {
  return sessionEntry(id, {
    title: input.title ?? `Session ${id}`,
    workspaceName: input.workspaceName ?? null,
    workspaceId: input.workspaceId ?? null,
    labels: input.labels ?? {},
    status: input.status,
    requiresAttention: input.requiresAttention,
    attentionReason: input.attentionReason,
    pendingPermissionCount: input.pendingPermissionCount,
    cwd: input.cwd,
    projectKey: input.projectKey,
    projectName: input.projectName,
  });
}

function allGroups(groups: readonly OrganizationGroup[]): OrganizationGroup[] {
  return groups.flatMap((group) => [group, ...allGroups(group.groups)]);
}

function findGroup(
  groups: readonly OrganizationGroup[],
  predicate: (group: OrganizationGroup) => boolean,
): OrganizationGroup {
  const found = allGroups(groups).find(predicate);
  if (!found) {
    throw new Error("Expected organization group was not found");
  }
  return found;
}

function sessionIds(rows: ReturnType<typeof flattenOrganizationTree>): string[] {
  return rows
    .filter((row): row is Extract<(typeof rows)[number], { kind: "session" }> => row.kind === "session")
    .map((row) => row.entry.id);
}

function fixtureTree() {
  return buildOrganizationTree({
    entries: [
      entry("swe-1", {
        title: "SWE • first",
        workspaceName: "TSW • alpha",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("swe-2", {
        title: "SWE • second",
        workspaceName: "TSW • alpha",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("tpm-1", {
        title: "TPM • lead",
        workspaceName: "TSW • alpha",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("lsa-1", {
        title: "LSA • lone",
        workspaceName: "TSW • epsilon",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("swe-csw", {
        title: "SWE • csw",
        workspaceName: "CSW • beta",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("lsa-asw", {
        title: "LSA • asw",
        workspaceName: "ASW • gamma",
        labels: { desk: "PR Review", epic: "ASMA-1" },
      }),
      entry("adam-tpm", {
        title: "TPM • adam",
        workspaceName: "TSW • delta",
        labels: { desk: "Adam", epic: "ASMA-1" },
      }),
      entry("unassigned", {
        title: "No prefix",
        workspaceName: null,
        labels: {},
        cwd: "",
      }),
    ],
  });
}

describe("session organization hierarchy", () => {
  it("uses the authoritative Mapping → desk → workstream precedence", () => {
    const native = entry("desk", { labels: { desk: "PR Review", workstream: "kontor" } });
    expect(organizationScope(native, {}).workstream).toBe("PR Review");
    expect(organizationScope(native, { "session:desk": { workstream: "Explicit mapping" } }).workstream).toBe("Explicit mapping");
    expect(organizationScope(entry("stream", { labels: { workstream: "kontor" } }), {}).workstream).toBe("kontor");
  });
  it("keeps unassigned workspace-kind siblings inside their native project boundary", () => {
    const tree = buildOrganizationTree({ entries: [entry("one", { projectKey: "A", projectName: "Project A", workspaceId: "a", workspaceName: "TSW • A" }), entry("two", { projectKey: "B", projectName: "Project B", workspaceId: "b", workspaceName: "TSW • B" })] });
    expect(allGroups(tree).filter((group) => group.kind === "workspace-kind")).toHaveLength(0);
    expect(allGroups(tree).filter((group) => group.kind === "epic").map((group) => group.label)).toEqual(["Unassigned · Project A", "Unassigned · Project B"]);
    expect(new Set(sessionIds(flattenOrganizationTree(tree)))).toEqual(new Set(["one", "two"]));
  });
  it("retains a discovered empty workspace with zero native sessions", () => {
    const tree = buildOrganizationTree({ entries: [], workspaces: [{ id: "empty", name: "TSW • empty", projectKey: "project" }] });
    const rows = flattenOrganizationTree(filterOrganizationTree(tree, () => true, () => true));
    expect(rows.some((row) => row.kind === "group" && row.group.kind === "workspace" && row.group.label === "TSW • empty" && row.group.totalCount === 0)).toBe(true);
  });
  it("searches saved group labels and keeps independent native-state filters", () => {
    const entries = [entry("run", { status: "running" }), entry("idle", { status: "idle" })];
    const tree = buildOrganizationTree({ entries, displayMappings: { "session:run": { workstream: "Clinical delivery" }, "session:idle": { workstream: "Clinical delivery" } } });
    const matches = (group: OrganizationGroup) => group.label === "Clinical delivery";
    expect(sessionIds(flattenOrganizationTree(filterOrganizationTree(tree, () => false, matches), {}, true))).toEqual(expect.arrayContaining(["run", "idle"]));
    expect(sessionIds(flattenOrganizationTree(filterOrganizationTree(tree, () => false, matches, (row) => row.status === "running"), {}, true))).toEqual(["run"]);
  });
  it("keeps all 500 native IDs exactly once with duplicate names and empty native workspaces", () => {
    const entries = Array.from({ length: 500 }, (_, index) => entry(`Native-${index}`, {
      title: index % 2 ? "SWE • duplicate" : "QA • duplicate", workspaceId: `workspace-${index % 20}`,
      workspaceName: `TSW • ${index % 20}`, labels: { epic: "ASMA-1", workstream: "Adam" },
    }));
    const tree = buildOrganizationTree({ entries, workspaces: [{ id: "empty-A", name: "TSW • same title" }, { id: "empty-a", name: "TSW • same title" }] });
    const rows = flattenOrganizationTree(filterOrganizationTree(tree, () => true, () => true));
    expect(new Set(sessionIds(rows)).size).toBe(500);
    expect(sessionIds(rows)).toHaveLength(500);
    const empty = allGroups(tree).filter((group) => group.kind === "workspace" && group.totalCount === 0);
    expect(empty).toHaveLength(2); expect(empty[0]?.key).not.toBe(empty[1]?.key);
    expect(rows.filter((row) => row.kind === "group" && row.group.totalCount === 0).length).toBeGreaterThanOrEqual(2);
  });
  it("keeps durable workstreams first and keeps unassigned work visible", () => {
    const tree = fixtureTree();
    expect(tree.map((group) => group.label)).toEqual([
      "PR Review",
      "Adam",
      OTHER_WORKSTREAM,
    ]);
    expect(tree[0]?.kind).toBe("workstream");
    expect(tree[1]?.kind).toBe("workstream");
    expect(tree[2]?.kind).toBe("workstream");
    expect(tree[2]?.groups[0]?.label).toBe(UNASSIGNED_EPIC);
    expect(tree[2]?.groups[0]?.groups[0]?.label).toBe(UNASSIGNED_WORKSPACE);
  });

  it("groups repeated workspace kinds and session roles but not singletons", () => {
    const tree = fixtureTree();
    const pr = findGroup(tree, (group) => group.label === "PR Review");
    const epic = findGroup(pr.groups, (group) => group.label === "ASMA-1");
    const kind = findGroup(epic.groups, (group) => group.label === "TSW");
    expect(kind.kind).toBe("workspace-kind");
    expect(kind.groups.map((group) => group.label).sort()).toEqual([
      "TSW • alpha",
      "TSW • epsilon",
    ]);
    expect(
      epic.groups.some(
        (group) => group.kind === "workspace-kind" && group.label === "CSW",
      ),
    ).toBe(false);
    expect(
      epic.groups.some(
        (group) => group.kind === "workspace" && group.label === "CSW • beta",
      ),
    ).toBe(true);
    const alpha = findGroup(kind.groups, (group) => group.label === "TSW • alpha");
    expect(
      alpha.groups.some((group) => group.kind === "role" && group.label === "SWE"),
    ).toBe(true);
    expect(alpha.sessions.map((session) => session.id)).toContain("tpm-1");
    expect(
      alpha.groups.some((group) => group.kind === "role" && group.label === "TPM"),
    ).toBe(false);
  });

  it("keeps every session reachable exactly once", () => {
    const tree = fixtureTree();
    const rows = flattenOrganizationTree(tree, {}, true);
    const ids = sessionIds(rows);
    expect(ids).toHaveLength(8);
    expect(new Set(ids).size).toBe(8);
    expect(new Set(ids)).toEqual(
      new Set([
        "swe-1",
        "swe-2",
        "tpm-1",
        "lsa-1",
        "swe-csw",
        "lsa-asw",
        "adam-tpm",
        "unassigned",
      ]),
    );
  });

  it("uses parent identity in kind and role group keys", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("a1", {
          title: "SWE • a1",
          workspaceName: "TSW • one",
          workspaceId: "wks_a",
          labels: { desk: "PR Review", epic: "ASMA-1" },
        }),
        entry("a2", {
          title: "SWE • a2",
          workspaceName: "TSW • one",
          workspaceId: "wks_a",
          labels: { desk: "PR Review", epic: "ASMA-1" },
        }),
        entry("b1", {
          title: "SWE • b1",
          workspaceName: "TSW • two",
          workspaceId: "wks_b",
          labels: { desk: "PR Review", epic: "ASMA-1" },
        }),
        entry("b2", {
          title: "SWE • b2",
          workspaceName: "TSW • two",
          workspaceId: "wks_b",
          labels: { desk: "PR Review", epic: "ASMA-1" },
        }),
        entry("c1", {
          title: "SWE • c1",
          workspaceName: "TSW • three",
          workspaceId: "wks_c",
          labels: { desk: "PR Review", epic: "ASMA-2" },
        }),
        entry("c2", {
          title: "SWE • c2",
          workspaceName: "TSW • three",
          workspaceId: "wks_c",
          labels: { desk: "PR Review", epic: "ASMA-2" },
        }),
        entry("d1", {
          title: "SWE • d1",
          workspaceName: "TSW • four",
          workspaceId: "wks_d",
          labels: { desk: "PR Review", epic: "ASMA-2" },
        }),
        entry("d2", {
          title: "SWE • d2",
          workspaceName: "TSW • four",
          workspaceId: "wks_d",
          labels: { desk: "PR Review", epic: "ASMA-2" },
        }),
      ],
    });
    const kind = allGroups(tree).filter((group) => group.kind === "workspace-kind");
    const roles = allGroups(tree).filter((group) => group.kind === "role");
    expect(kind).toHaveLength(2);
    expect(new Set(kind.map((group) => group.key)).size).toBe(2);
    expect(roles).toHaveLength(4);
    expect(new Set(roles.map((group) => group.key)).size).toBe(4);
  });

  it("normalizes case, whitespace and only the first separator", () => {
    expect(workspaceKindPrefix("  tsw • one  ")).toBe("tsw");
    expect(workspaceKindPrefix("TSW • one • two")).toBe("TSW");
    expect(workspaceKindPrefix("plain name")).toBeNull();
    expect(workspaceKindPrefix(" • empty")).toBeNull();
    expect(sessionRolePrefix(" lsa • one ")).toBe("lsa");
    expect(sessionRolePrefix("SWE • one • two")).toBe("SWE");

    const tree = buildOrganizationTree({
      entries: [
        entry("one", {
          title: " lsa • one ",
          workspaceName: " tsw • one ",
          labels: { desk: "QA", epic: "ASMA-3" },
        }),
        entry("one2", {
          title: "LSA • two",
          workspaceName: "TSW • one",
          labels: { desk: "QA", epic: "ASMA-3" },
        }),
        entry("two", {
          title: "TPM • two",
          workspaceName: "TSW • two",
          labels: { desk: "QA", epic: "ASMA-3" },
        }),
      ],
    });
    const kind = findGroup(tree, (group) => group.kind === "workspace-kind");
    expect(kind.label).toBe("TSW");
    const role = findGroup(tree, (group) => group.kind === "role");
    expect(role.label).toBe("LSA");
    expect(role.totalCount).toBe(2);
  });

  it("filters without rebuilding nesting, preserving total vs visible counts", () => {
    const tree = fixtureTree();
    const filtered = filterOrganizationTree(tree, (session) =>
      session.title.toLowerCase().includes("swe"),
    );
    expect(filtered).toHaveLength(1);
    const pr = filtered[0]!;
    expect(pr.visibleCount).toBe(3);
    expect(pr.totalCount).toBe(6);
    expect(allGroups(filtered).every((group) => group.visibleCount > 0)).toBe(true);
    const rows = flattenOrganizationTree(filtered, {}, true);
    expect(sessionIds(rows).sort()).toEqual(["swe-1", "swe-2", "swe-csw"]);
  });

  it("honors collapsed groups and force-expands matching descendants", () => {
    const tree = fixtureTree();
    const pr = tree[0]!;
    const collapsed = flattenOrganizationTree(tree, { [pr.key]: false });
    expect(collapsed[0]?.kind).toBe("group");
    expect(sessionIds(collapsed)).not.toContain("swe-1");
    const expandedRows = flattenOrganizationTree(tree, { [pr.key]: true });
    expect(sessionIds(expandedRows)).toHaveLength(8);
    const forced = flattenOrganizationTree(
      filterOrganizationTree(tree, (session) => session.id === "swe-2"),
      { [pr.key]: false },
      true,
    );
    expect(sessionIds(forced)).toEqual(["swe-2"]);
  });

  it("applies persisted sibling order and favorites", () => {
    const base = buildOrganizationTree({
      entries: [
        entry("c", {
          title: "c",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
        entry("a", {
          title: "a",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
        entry("b", {
          title: "b",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
      ],
    });
    const workspace = findGroup(base, (group) => group.kind === "workspace");
    const ordered = buildOrganizationTree({
      entries: [
        entry("c", {
          title: "c",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
        entry("a", {
          title: "a",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
        entry("b", {
          title: "b",
          workspaceName: "TSW • alpha",
          workspaceId: "wks_a",
          labels: { desk: "QA", epic: "ASMA-4" },
        }),
      ],
      siblingOrder: {
        [workspace.key]: [sessionKey("b"), sessionKey("c"), sessionKey("a")],
      },
      favorites: ["a"],
    });
    const orderedWorkspace = findGroup(
      ordered,
      (group) => group.kind === "workspace",
    );
    expect(orderedWorkspace.sessions.map((session) => session.id)).toEqual([
      "b",
      "c",
      "a",
    ]);
    expect(isFavoriteSession(orderedWorkspace.sessions[2]!, ["a"])).toBe(true);
    expect(isFavoriteSession(orderedWorkspace.sessions[0]!, ["a"])).toBe(false);
  });

  it("applies explicit session and workspace display mappings without title guessing", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("mapped", {
          title: "Unlabelled session",
          workspaceName: "ordinary workspace",
          workspaceId: "wks_mapped",
        }),
        entry("workspace-mapped", {
          title: "Workspace mapping",
          workspaceName: "ordinary workspace",
          workspaceId: "wks_mapped",
        }),
      ],
      displayMappings: {
        "workspace:wks_mapped": { workstream: "QA", epic: "ASMA-9" },
        "session:mapped": { workstream: "Adam", epic: "ASMA-10" },
      },
    });
    expect(tree.map((group) => group.label).sort()).toEqual(["Adam", "QA"]);
    const adam = findGroup(tree, (group) => group.label === "Adam");
    const qa = findGroup(tree, (group) => group.label === "QA");
    expect(adam.groups[0]?.label).toBe("ASMA-10");
    expect(qa.groups[0]?.label).toBe("ASMA-9");
  });

  it("supports explicit role and flat grouping choices", () => {
    const entries = [
      entry("swe-a", { title: "SWE • a", labels: { desk: "QA", epic: "ASMA-5" } }),
      entry("swe-b", { title: "SWE • b", labels: { desk: "QA", epic: "ASMA-5" } }),
      entry("tpm-a", { title: "TPM • a", labels: { desk: "QA", epic: "ASMA-5" } }),
    ];
    const role = buildOrganizationTree({ entries, grouping: "role" });
    expect(role).toHaveLength(1);
    expect(role[0]?.label).toBe("Roles");
    expect(role[0]?.groups[0]?.label).toBe("SWE");
    expect(role[0]?.sessions.map((session) => session.id)).toEqual(["tpm-a"]);
    const flat = buildOrganizationTree({ entries, grouping: "flat" });
    expect(flat[0]?.label).toBe("All sessions");
    expect(flat[0]?.sessions).toHaveLength(3);
  });

  it("prioritizes native state and attention in aggregate counts", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("input", {
          status: "error",
          pendingPermissionCount: 1,
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
        entry("failed", {
          status: "error",
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
        entry("running", {
          status: "running",
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
        entry("attention", {
          status: "idle",
          requiresAttention: true,
          attentionReason: "finished",
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
        entry("done", {
          status: "closed",
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
        entry("other", {
          status: "initializing",
          labels: { desk: "QA", epic: "ASMA-6" },
        }),
      ],
      grouping: "flat",
    });
    expect(tree[0]?.aggregate).toEqual({
      total: 6,
      needsInput: 1,
      failed: 1,
      running: 1,
      attention: 1,
      initializing: 1,
      idle: 0,
      closed: 1,
      native: { initializing: 1, idle: 1, running: 1, error: 2, closed: 1 },
    });
    expect(aggregateState(tree[0]!.aggregate)).toBe("needs_input");
    expect(aggregateSummary(tree[0]!.aggregate)).toBe(
      "1 needs input · 1 error · 1 running · 1 attention · 1 starting · 1 closed",
    );
  });
});

describe("session organization parity", () => {
  it("partitions Role groups by native placement and display scope", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("a1", { title: "SWE • one", workspaceId: "w1", workspaceName: "TSW • one", labels: { epic: "ASMA-1" } }),
        entry("a2", { title: "SWE • two", workspaceId: "w1", workspaceName: "TSW • one", labels: { epic: "ASMA-1" } }),
        entry("b1", { title: "SWE • three", workspaceId: "w2", workspaceName: "TSW • two", labels: { epic: "ASMA-1" } }),
        entry("b2", { title: "SWE • four", workspaceId: "w2", workspaceName: "TSW • two", labels: { epic: "ASMA-1" } }),
        entry("c1", { title: "QA • lonely", workspaceId: "w2", workspaceName: "TSW • two", labels: { epic: "ASMA-1" } }),
      ],
      grouping: "role",
    });
    const roles = allGroups(tree).filter((group) => group.kind === "role");
    expect(roles.map((group) => group.sessions.map((session) => session.id))).toEqual([["a1", "a2"], ["b1", "b2"]]);
    expect(new Set(roles.map((group) => group.key)).size).toBe(2);
    expect(roles.every((group) => group.parentKey === "roles")).toBe(true);
    expect(roles.map((group) => group.detail)).toEqual([
      "Other work · ASMA-1 · TSW • one",
      "Other work · ASMA-1 · TSW • two",
    ]);
    expect(tree[0]!.sessions.map((session) => session.id)).toEqual(["c1"]);
  });

  it("opens workstreams and desks but not epics or workspaces on a first visit, and lets saved or forced state win", () => {
    const tree = buildOrganizationTree({
      entries: [entry("a", { workspaceId: "w1", workspaceName: "Main", labels: { epic: "ASMA-1" } })],
    });
    const keys = (rows: ReturnType<typeof flattenOrganizationTree>) =>
      rows.map((row) => (row.kind === "group" ? `${row.group.kind}:${row.expanded}` : row.kind));
    expect(keys(flattenOrganizationTree(tree, {}, false, defaultGroupExpanded))).toEqual(["workstream:true", "epic:false"]);
    const epic = findGroup(tree, (group) => group.kind === "epic");
    expect(keys(flattenOrganizationTree(tree, { [epic.key]: true }, false, defaultGroupExpanded))).toEqual([
      "workstream:true", "epic:true", "workspace:false",
    ]);
    expect(keys(flattenOrganizationTree(tree, { [tree[0]!.key]: false }, true, defaultGroupExpanded))).toEqual([
      "workstream:true", "epic:true", "workspace:true", "session",
    ]);
    expect(defaultGroupExpanded({ ...tree[0]!, kind: "project", label: "DESK • adam" })).toBe(true);
    expect(defaultGroupExpanded({ ...tree[0]!, kind: "project", label: "asma-rs-kontor" })).toBe(false);
  });

  it("recomputes counts and the priority state from the visible sessions after filtering", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("run", { status: "running", workspaceId: "w1", workspaceName: "Main" }),
        entry("idle", { status: "idle", workspaceId: "w1", workspaceName: "Main" }),
        entry("failed", { status: "error", workspaceId: "w1", workspaceName: "Main" }),
      ],
    });
    const filtered = filterOrganizationTree(tree, (session) => session.status === "idle");
    const workspace = findGroup(filtered, (group) => group.kind === "workspace");
    expect(workspace.aggregate).toMatchObject({ total: 1, idle: 1, running: 0, failed: 0 });
    expect(aggregateState(workspace.aggregate)).toBe("idle");
    expect([workspace.unitVisible, workspace.unitTotal]).toEqual([1, 3]);
    expect(aggregateState(findGroup(tree, (group) => group.kind === "workspace").aggregate)).toBe("failed");
  });

  it("counts workspaces, not sessions, on a workspace-kind group", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("a", { workspaceId: "w1", workspaceName: "TSW • one" }),
        entry("b", { workspaceId: "w1", workspaceName: "TSW • one" }),
        entry("c", { workspaceId: "w2", workspaceName: "TSW • two" }),
      ],
    });
    const kind = findGroup(tree, (group) => group.kind === "workspace-kind");
    expect([kind.label, kind.unit, kind.unitTotal, kind.totalCount]).toEqual(["TSW", "workspaces", 2, 3]);
    const filtered = findGroup(
      filterOrganizationTree(tree, (session) => session.id === "c"),
      (group) => group.kind === "workspace-kind",
    );
    expect([filtered.unitVisible, filtered.unitTotal]).toEqual([1, 2]);
  });

  it("groups Desks & workspaces by native project with desk projects first and an empty workspace row", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("k", { projectKey: "p-kontor", projectName: "asma-rs-kontor", workspaceId: "w1", workspaceName: "TSW • one" }),
        entry("d", { projectKey: "p-desk", projectName: "DESK • adam", workspaceId: "w2", workspaceName: "Adam" }),
      ],
      grouping: "placement",
      workspaces: [{ id: "w3", name: "main", projectKey: "p-kontor", projectName: "asma-rs-kontor" }],
    });
    expect(tree.map((group) => [group.kind, group.label, group.detail])).toEqual([
      ["project", "DESK • adam", "1 workspace"],
      ["project", "asma-rs-kontor", "2 workspaces"],
    ]);
    const empty = findGroup(tree, (group) => group.label === "main");
    const rows = flattenOrganizationTree(tree, { [empty.parentKey!]: true, [empty.key]: true }, false, defaultGroupExpanded);
    expect(flattenOrganizationTree(tree, {}, false, defaultGroupExpanded).some((row) => row.kind === "empty")).toBe(false);
    expect(rows.find((row) => row.kind === "empty")).toEqual({ kind: "empty", depth: empty.depth + 1, parentKey: empty.key });
  });

  it("matches an epic scope through display mappings and recognizes desk work", () => {
    const mapped = entry("m", { workspaceId: "w1", labels: { epic: "ASMA-1" } });
    const mappings = {
      [workspaceMappingKey("w1")]: { workstream: "Adam", epic: "asma-9" },
    };
    expect(inEpicScope(mapped, mappings, "ASMA-9")).toBe(true);
    expect(inEpicScope(mapped, mappings, "ASMA-1")).toBe(false);
    expect(inEpicScope(mapped, undefined, "asma-1")).toBe(true);
    expect(isDeskWork(mapped, mappings)).toBe(true);
    expect(isDeskWork(mapped, undefined)).toBe(false);
    expect(isDeskWork(entry("label", { labels: { desk: "Kontor desk" } }), undefined)).toBe(true);
    expect(isDeskWork(entry("label", { labels: { desk: "Kontor desk" } }), { [workspaceMappingKey("w1")]: {} })).toBe(true);
    expect(
      isDeskWork(entry("mapped", { workspaceId: "w1", labels: { desk: "QA" } }), {
        [workspaceMappingKey("w1")]: { workstream: "Billing" },
      }),
    ).toBe(false);
  });
});
