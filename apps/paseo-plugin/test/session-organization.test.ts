import { describe, expect, it, vi } from "vitest";
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
  NO_LINKED_EPIC,
  OTHER_WORKSTREAM,
  sessionKey,
  UNASSIGNED_EPIC,
  UNASSIGNED_WORKSPACE,
  workspaceKindPrefix,
  sessionRolePrefix,
  organizationScope,
  leadingStates,
  NO_ROLE,
  NO_ROLE_KEY,
  ROLE_ROOT_KEY,
  sessionRole,
  type OrganizationGroup,
} from "../client/session-organization";
import { siblingKeys } from "../client/organization-order";
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


  it("places a promoted ADHOC session under its logical epic inside the ADHOC workspace", () => {
    const promoted = entry("promoted", { title: "LSA • ADHOC • Route", workspaceName: "ADHOC", workspaceId: "wks-adhoc",
      projectKey: "prj-adam", projectName: "DESK • ADAM", labels: { role: "LSA", epic: "ASMA-100", "title.auto": "LSA • ADHOC • Route" } });
    const tree = buildOrganizationTree({ entries: [promoted] });
    const adhoc = findGroup(tree, (group) => group.kind === "workspace" && group.label === "ADHOC");
    const epic = adhoc.groups.find((group) => group.kind === "epic");
    expect(epic?.label).toBe("ASMA-100");
    expect(epic?.entries.map((value) => value.id)).toContain("promoted");
  });

describe("session organization hierarchy", () => {
  it("keeps unknown-label rows in native placement, never Other work or No epic linked", () => {
    const rows = [entry("catalog", { projectKey: "prj_desk", projectName: "DESK • ADAM", workspaceId: "wks_adam", workspaceName: "ADAM" })];
    const tree = buildOrganizationTree({ entries: rows, labelsComplete: false, grouping: "hierarchy" });
    expect(tree.map((group) => group.kind)).toEqual(["project"]);
    expect(allGroups(tree).map((group) => group.label)).not.toContain(OTHER_WORKSTREAM);
    expect(allGroups(tree).map((group) => group.label)).not.toContain(NO_LINKED_EPIC);
    expect(sessionIds(flattenOrganizationTree(tree))).toEqual(["catalog"]);
  });

  it("uses the authoritative Mapping → desk → workstream precedence", () => {
    const native = entry("desk", { labels: { desk: "PR Review", workstream: "kontor" } });
    expect(organizationScope(native, {}).workstream).toBe("PR Review");
    expect(organizationScope(native, { "session:desk": { workstream: "Explicit mapping" } }).workstream).toBe("Explicit mapping");
    expect(organizationScope(entry("stream", { labels: { workstream: "kontor" } }), {}).workstream).toBe("kontor");
  });
  it("keeps unassigned workspace-kind siblings inside their native project boundary", () => {
    const tree = buildOrganizationTree({ entries: [entry("one", { projectKey: "A", projectName: "Project A", workspaceId: "a", workspaceName: "TSW • A" }), entry("two", { projectKey: "B", projectName: "Project B", workspaceId: "b", workspaceName: "TSW • B" })] });
    expect(allGroups(tree).filter((group) => group.kind === "workspace-kind")).toHaveLength(0);
    expect(allGroups(tree).filter((group) => group.kind === "epic").map((group) => [group.label, group.context])).toEqual([["No epic linked", "Project A"], ["No epic linked", "Project B"]]);
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
    expect(tree[2]?.groups[0]?.label).toBe(NO_LINKED_EPIC);
    expect(tree[2]?.groups[0]?.groups[0]?.label).toBe(UNASSIGNED_WORKSPACE);
  });

  it("keeps the no-linked-epic group key, so saved disclosure still applies", () => {
    const tree = buildOrganizationTree({ entries: [entry("loose", { workspaceId: "w", workspaceName: "main", projectKey: "P", labels: { workstream: "Kontor" } })] });
    const group = findGroup(tree, (candidate) => candidate.kind === "epic");
    expect([group.label, group.key, group.epic]).toEqual([NO_LINKED_EPIC, `workstream:kontor/project:P/epic:${UNASSIGNED_EPIC.toLocaleLowerCase()}`, null]);
    expect(sessionIds(flattenOrganizationTree(tree, { [group.key]: true, [group.groups[0]!.key]: true }, false, defaultGroupExpanded))).toEqual(["loose"]);
  });

  it("places desk work with no linked epic directly under the desk, keeping real epics", () => {
    const tree = buildOrganizationTree({
      entries: [
        entry("review", { workspaceId: "r", workspaceName: "desk-review", projectKey: "P", labels: { desk: "PR Review" } }),
        entry("review-2", { workspaceId: "r", workspaceName: "desk-review", projectKey: "P", labels: { desk: "PR Review" } }),
        entry("epic-review", { workspaceId: "e", workspaceName: "epic-review", projectKey: "P", labels: { desk: "PR Review", epic: "ASMA-1" } }),
        entry("adam", { workspaceId: "a", workspaceName: "desk-adam", projectKey: "Q", labels: { desk: "Adam" } }),
        entry("custom", { workspaceId: "c", workspaceName: "desk-custom", projectKey: "P", labels: { desk: "Kontor desk" } }),
        entry("stream", { workspaceId: "s", workspaceName: "stream-main", projectKey: "P", labels: { workstream: "Kontor desk" } }),
      ],
      workspaces: [{ id: "idle-desk", name: "admin-idle", projectKey: "P" }, { id: "idle-epic", name: "epic-idle", projectKey: "P" }, { id: "idle-other", name: "other-idle", projectKey: "P" }],
      displayMappings: {
        [workspaceMappingKey("idle-desk")]: { workstream: "PR Review" },
        [workspaceMappingKey("idle-epic")]: { workstream: "PR Review", epic: "ASMA-1" },
        [workspaceMappingKey("idle-other")]: { workstream: "Kontor" },
      },
    });
    const children = (label: string) => findGroup(tree, (group) => group.kind === "workstream" && group.label === label).groups.map((group) => [group.kind, group.label, group.parentKey, group.totalCount]);
    expect(children("PR Review")).toEqual([
      ["epic", "ASMA-1", "workstream:pr review", 1],
      // Real epics first, then the desk's workspaces, whatever their names.
      ["workspace", "admin-idle", "workstream:pr review", 0],
      ["workspace", "desk-review", "workstream:pr review", 2],
    ]);
    expect(children("Adam")).toEqual([["workspace", "desk-adam", "workstream:adam", 1]]);
    // A desk label of any name is desk work; the same name as a plain workstream keeps its epic layer.
    expect(children("Kontor desk")).toEqual([
      ["workspace", "desk-custom", "workstream:kontor desk", 1],
      ["epic", NO_LINKED_EPIC, "workstream:kontor desk", 1],
    ]);
    expect(children("Kontor")).toEqual([["epic", NO_LINKED_EPIC, "workstream:kontor", 0]]);
    // An empty desk workspace mapped to an epic stays in that epic.
    expect(findGroup(tree, (group) => group.label === "epic-idle").parentKey).toBe("workstream:pr review/project:P/epic:asma-1");
    const review = findGroup(tree, (group) => group.label === "desk-review");
    expect([review.key, review.depth, review.sessions.map((session) => session.id)]).toEqual(["workstream:pr review/workspace:id%3Ar", 1, ["review", "review-2"]]);
    expect(findGroup(tree, (group) => group.label === "PR Review").totalCount).toBe(3);
    expect(allGroups(tree).filter((group) => group.kind === "epic" && group.parentKey?.match(/pr review|adam/))).toHaveLength(1);
    const ids = sessionIds(flattenOrganizationTree(tree, {}, true));
    expect([ids.length, new Set(ids).size]).toEqual([6, 6]);
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
    expect(role.map((group) => [group.label, group.sessions.map((session) => session.id)])).toEqual([["TPM", ["tpm-a"]], ["SWE", ["swe-a", "swe-b"]]]);
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
  it("shows one Role header per role across the scope, singletons and No role included, each session once", () => {
    const entries = [
      entry("a1", { title: "SWE • one", workspaceId: "w1", workspaceName: "TSW • one", labels: { epic: "ASMA-1" } }),
      entry("b1", { title: "SWE • three", workspaceId: "w2", workspaceName: "TSW • two", projectKey: "other", labels: { workstream: "Mobile", epic: "ASMA-2" } }),
      entry("c1", { title: "QA • lonely", workspaceId: "w2", workspaceName: "TSW • two", labels: { epic: "ASMA-1" } }),
      entry("d1", { title: "Explore SWE graph layout options", labels: { desk: "Adam" } }),
      entry("e1", { title: "Notes", labels: { "kontor.role": "tpm" } }),
    ];
    const tree = buildOrganizationTree({ entries, grouping: "role" });
    expect(tree.map((group) => [group.key, group.label, group.sessions.map((session) => session.id)])).toEqual([
      [`${ROLE_ROOT_KEY}/role:TPM`, "TPM", ["e1"]],
      // Both idle: the Mobile workstream sorts before Other work.
      [`${ROLE_ROOT_KEY}/role:SWE`, "SWE", ["b1", "a1"]],
      [`${ROLE_ROOT_KEY}/role:QA`, "QA", ["c1"]],
      [NO_ROLE_KEY, NO_ROLE, ["d1"]],
    ]);
    expect(tree.every((group) => group.kind === "role" && group.parentKey === ROLE_ROOT_KEY && group.depth === 0 && group.groups.length === 0 && group.detail === null)).toBe(true);
    const members = tree.flatMap((group) => group.sessions.map((session) => session.id));
    expect([members.length, new Set(members).size, tree.reduce((sum, group) => sum + group.totalCount, 0)]).toEqual([5, 5, 5]);
    expect(sessionIds(flattenOrganizationTree(tree, {}, false, defaultGroupExpanded))).toEqual([]);
  });

  it("resolves a role from kontor.role, then role, then the title prefix, keeping a losing source as a cue", () => {
    const role = (title: string, labels: Record<string, string> = {}) => sessionRole(entry("r", { title, labels }));
    expect(role("QA • check", { "kontor.role": "AUD" })).toEqual({ code: "AUD", source: "kontor.role", conflicts: [{ source: "title", code: "QA" }] });
    expect(role("QA • check", { "kontor.role": "AUD", role: "qa" })).toEqual({ code: "AUD", source: "kontor.role", conflicts: [{ source: "role", code: "QA" }] });
    expect(role("SWE • work", { "kontor.role": " ", role: "rev_a" })).toEqual({ code: "REV A", source: "role", conflicts: [{ source: "title", code: "SWE" }] });
    expect(role("qa • work", { role: "QA" })).toEqual({ code: "QA", source: "role", conflicts: [] });
    expect(role("rev a • work")).toEqual({ code: "REV A", source: "title", conflicts: [] });
    expect(role("Explore SWE graph layout options")).toEqual({ code: null, source: null, conflicts: [] });
  });

  it("matches role codes across case, Unicode whitespace and NFC, but never merges punctuation, suffixes or other roles", () => {
    const code = (title: string, labels: Record<string, string> = {}) => sessionRole(entry("r", { title, labels })).code;
    expect(["qa • x", "QA • y", "\u00a0Qa • z", "Qa\u2003• w"].map((title) => code(title))).toEqual(["QA", "QA", "QA", "QA"]);
    expect(["REV  A • x", "rev\u00a0a • y", "Rev\tA • z"].map((title) => code(title))).toEqual(["REV A", "REV A", "REV A"]);
    expect(code("Re\u0301v • x")).toBe(code("R\u00e9v • y"));
    expect(["QA1 • x", "\uff31\uff21 • y", "AUD • z", "REV B • w", "REV-A • v", "QA. • u"].map((title) => code(title))).toEqual(["QA1", "\uff31\uff21", "AUD", "REV B", "REV-A", "QA."]);
    // Role labels read "-" and "_" as spaces, as they always have; a title prefix keeps them.
    expect([code("x", { role: "rev-a" }), code("x", { role: "rev__a" })]).toEqual(["REV A", "REV A"]);
  });

  it("reads role codes the same under a Turkish locale", () => {
    const upper = vi.spyOn(String.prototype, "toLocaleUpperCase").mockImplementation(function (this: string) { return this.replace(/i/g, "\u0130").toUpperCase(); });
    const lower = vi.spyOn(String.prototype, "toLocaleLowerCase").mockImplementation(function (this: string) { return this.replace(/I/g, "\u0131").toLowerCase(); });
    try {
      const tree = buildOrganizationTree({ entries: [entry("a", { title: "qi • x" }), entry("b", { title: "QI • y" }), entry("c", { title: "x", labels: { role: "qi" } })], grouping: "role" });
      expect(tree.map((group) => [group.label, group.totalCount])).toEqual([["QI", 3]]);
    } finally {
      upper.mockRestore();
      lower.mockRestore();
    }
  });

  it("orders Role headers by the role order, then by label, No role last, and lets a saved order win", () => {
    const entries = ["Zeta", "", "QA", "Alpha", "REV B", "LSA", "REV A", "QA1"].map((prefix, index) => entry(`s${index}`, { title: prefix ? `${prefix} • work` : "Untitled" }));
    expect(buildOrganizationTree({ entries, grouping: "role" }).map((group) => group.label)).toEqual(["LSA", "QA", "REV A", "REV B", "ALPHA", "QA1", "ZETA", NO_ROLE]);
    const saved = { [ROLE_ROOT_KEY]: [NO_ROLE_KEY, `${ROLE_ROOT_KEY}/role:ZETA`, `${ROLE_ROOT_KEY}/role:QA`] };
    expect(buildOrganizationTree({ entries, grouping: "role", siblingOrder: saved }).map((group) => group.label)).toEqual(["ZETA", "QA", "LSA", "REV A", "REV B", "ALPHA", "QA1", NO_ROLE]);
    expect(siblingKeys(buildOrganizationTree({ entries, grouping: "role" }), ROLE_ROOT_KEY)).toEqual(["LSA", "QA", "REV A", "REV B", "ALPHA", "QA1", "ZETA"].map((code) => `${ROLE_ROOT_KEY}/role:${encodeURIComponent(code)}`).concat(NO_ROLE_KEY));
  });

  it("orders sessions in a role by state, then placement and identity, so an update alone never moves one", () => {
    const make = (updated: (id: string) => string, needs: string | null = null) => [
      entry("b-idle", { title: "SWE • b", status: "idle", labels: { workstream: "Beta" } }),
      // Same state and place: the earlier-created one leads, though its ID sorts later.
      entry("a-run-1", { title: "SWE • a", status: "running", labels: { workstream: "Alpha" } }),
      entry("a-run-2", { title: "SWE • c", status: "running", labels: { workstream: "Alpha" } }),
      entry("z-run", { title: "SWE • z", status: "running", labels: { workstream: "Zulu" } }),
    ].map((value) => ({
      ...value,
      updatedAt: updated(value.id),
      createdAt: value.id === "a-run-1" ? "2026-10-18T09:30:00.000Z" : value.createdAt,
      needsInput: value.id === needs,
    }));
    const order = (entries: ReturnType<typeof make>) => buildOrganizationTree({ entries, grouping: "role" })[0]!.sessions.map((session) => session.id);
    const before = order(make(() => "2026-10-18T10:00:00.000Z"));
    expect(before).toEqual(["a-run-2", "a-run-1", "z-run", "b-idle"]);
    expect(order(make((id) => id === "z-run" ? "2026-10-18T11:00:00.000Z" : "2026-10-18T10:00:00.000Z").reverse())).toEqual(before);
    expect(order(make(() => "2026-10-18T10:00:00.000Z", "b-idle"))).toEqual(["b-idle", "a-run-2", "a-run-1", "z-run"]);
    const saved = buildOrganizationTree({ entries: make(() => "x"), grouping: "role", siblingOrder: { [`${ROLE_ROOT_KEY}/role:SWE`]: [sessionKey("z-run")] } });
    expect(saved[0]!.sessions[0]!.id).toBe("z-run");
  });

  it("closes Role headers on a first visit but keeps local role groups open, and lets saved state win", () => {
    const entries = [entry("a", { title: "SWE • a" }), entry("b", { title: "SWE • b" })];
    const [role] = buildOrganizationTree({ entries, grouping: "role" });
    expect(defaultGroupExpanded(role!)).toBe(false);
    expect(sessionIds(flattenOrganizationTree([role!], { [role!.key]: true }, false, defaultGroupExpanded))).toEqual(["a", "b"]);
    const local = findGroup(buildOrganizationTree({ entries }), (group) => group.kind === "role");
    expect([local.parentKey === ROLE_ROOT_KEY, defaultGroupExpanded(local)]).toEqual([false, true]);
  });

  it("counts a narrowed Role header as n of m and its leading states", () => {
    const entries = [
      entry("n", { title: "QA • n", pendingPermissionCount: 1, labels: { desk: "PR Review" } }),
      entry("e", { title: "QA • e", status: "error" }),
      ...["r1", "r2", "r3"].map((id) => entry(id, { title: `QA • ${id}`, status: "running", labels: { desk: "PR Review" } })),
    ];
    const [qa] = buildOrganizationTree({ entries, grouping: "role" });
    expect(leadingStates(qa!.aggregate)).toEqual([{ state: "needs_input", count: 1 }, { state: "failed", count: 1 }]);
    expect(leadingStates(qa!.aggregate, 3).map(({ state, count }) => `${state}:${count}`)).toEqual(["needs_input:1", "failed:1", "running:3"]);
    const [narrowed] = filterOrganizationTree([qa!], (session) => session.labels.desk === "PR Review");
    expect([narrowed!.visibleCount, narrowed!.totalCount, leadingStates(narrowed!.aggregate)]).toEqual([4, 5, [{ state: "needs_input", count: 1 }, { state: "running", count: 3 }]]);
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
