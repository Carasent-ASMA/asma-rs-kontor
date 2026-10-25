import { describe, expect, it } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import {
  buildOrganizationTree,
  deskProjectScope,
  deriveDeskScopes,
  filterOrganizationTree,
  flattenOrganizationTree,
  isDeskWork,
  matchesScope,
  nativeDesk,
  NO_LINKED_EPIC,
  organizationScope,
  OTHER_WORKSTREAM,
  scopeKeepsGroup,
  workspaceMappingKey,
  type OrganizationGroup,
  type OrganizationWorkspace,
} from "../client/session-organization";
import { sessionEntry } from "./session-fixtures";

// Fictional inventory: native desk projects with labelled and unlabelled members, as Paseo reports them.
const ADAM = { projectKey: "prj_adam", projectName: "DESK • ADAM" };
const REVIEW = { projectKey: "prj_review", projectName: "DESK • PR REVIEW" };
const EPIC = { projectKey: "prj_epic", projectName: "ESW • EX-100" };

function entry(id: string, place: { projectKey: string; projectName: string }, workspaceId: string, workspaceName: string, title: string, labels: Record<string, string> = {}): DirectoryEntry {
  return sessionEntry(id, { ...place, workspaceId, workspaceName, title, labels });
}

function inventory(): DirectoryEntry[] {
  return [
    // The PR desk labels its own seats; the LSA tabs opened by hand in the same workspace carry no label.
    entry("review-tpm-1", REVIEW, "wks_review_adam", "ADAM", "TPM • EX-1 • repo#1", { desk: "pr-review", "desk.role": "adam" }),
    entry("review-tpm-2", REVIEW, "wks_review_adam", "ADAM", "TPM • EX-2 • repo#2", { desk: "pr-review", "desk.role": "adam" }),
    entry("review-lsa-1", REVIEW, "wks_review_adam", "ADAM", "LSA • GENERAL"),
    entry("review-lsa-2", REVIEW, "wks_review_adam", "ADAM", "LSA • DRAFTS CHECKER"),
    // The Adam desk has no desk label at all.
    entry("adam-lsa-1", ADAM, "wks_adam", "ADAM", "LSA • RESEARCH"),
    entry("adam-lsa-2", ADAM, "wks_adam", "ADAM", "LSA • DOCS"),
    entry("adam-epic", ADAM, "wks_adam", "ADAM", "TPM • EX-300 • relaunch", { "jira.epic": "EX-300" }),
    entry("adam-declared", ADAM, "wks_adam", "ADAM", "TPM • EX-400 • navigator", { epic: "EX-400", workstream: "kontor" }),
    // Epic work outside the desks: one labelled, one unlabelled, one whose label names another epic.
    entry("epic-labelled", EPIC, "wks_epic", "TSW • EX-100 • slice", "SWE • EX-101 • slice", { "jira.epic": "EX-100" }),
    entry("epic-unlabelled", EPIC, "wks_epic", "TSW • EX-100 • slice", "QA • EX-101 • slice"),
    entry("epic-conflict", EPIC, "wks_epic_2", "TSW • EX-100 • history", "Builder · history", { "jira.epic": "EX-200", "jira.parent": "EX-100" }),
    entry("unknown-stream", EPIC, "wks_epic_2", "TSW • EX-100 • history", "SWE • history", { workstream: "unrecognised-stream" }),
  ];
}

const emptyWorkspaces: OrganizationWorkspace[] = [
  { id: "wks_review_idle", name: "RSW • EX-9 • repo#9", projectKey: REVIEW.projectKey, projectName: REVIEW.projectName },
  { id: "wks_adam_idle", name: "CSW • EX-8 • review", projectKey: ADAM.projectKey, projectName: ADAM.projectName },
  { id: "wks_epic_idle", name: "TSW • EX-100 • idle", projectKey: EPIC.projectKey, projectName: EPIC.projectName },
];

function allGroups(groups: readonly OrganizationGroup[]): OrganizationGroup[] {
  return groups.flatMap((group) => [group, ...allGroups(group.groups)]);
}

function placement(groups: readonly OrganizationGroup[]): Map<string, string[]> {
  const where = new Map<string, string[]>();
  for (const group of allGroups(groups)) for (const session of group.sessions) where.set(session.id, [...(where.get(session.id) ?? []), group.key]);
  return where;
}

function workstream(groups: readonly OrganizationGroup[], label: string): OrganizationGroup {
  const found = groups.find((group) => group.kind === "workstream" && group.label === label);
  if (!found) throw new Error(`No workstream ${label}`);
  return found;
}

function treeIds(groups: readonly OrganizationGroup[]): string[] {
  return flattenOrganizationTree(groups, {}, true).flatMap((row) => row.kind === "session" ? [row.entry.id] : []);
}

describe("desk grouping from native desk projects", () => {
  it("reads a native desk from the project name only, as the PR desk's own label slug", () => {
    expect([nativeDesk("DESK • ADAM"), nativeDesk("DESK • PR REVIEW"), nativeDesk(" desk •  Pr_Review "), nativeDesk("DESK•QA")]).toEqual(["adam", "pr-review", "pr-review", "qa"]);
    expect([nativeDesk("ESW • EX-100"), nativeDesk("ADAM"), nativeDesk("DESK •"), nativeDesk("Desktop • app"), nativeDesk(null)]).toEqual([null, null, null, null, null]);
  });

  it("puts the Adam desk and the PR desk's unlabelled LSA tabs under their desks, each session once", () => {
    const tree = buildOrganizationTree({ entries: inventory(), workspaces: emptyWorkspaces });
    expect(tree.map((group) => group.label)).toEqual([ADAM.projectName, REVIEW.projectName, "unrecognised-stream", OTHER_WORKSTREAM]);
    const where = placement(tree);
    expect([...where.values()].every((keys) => keys.length === 1)).toBe(true);
    expect(where.size).toBe(inventory().length);
    // One native workspace, one place: the labelled TPMs and the unlabelled LSAs stay together.
    expect(where.get("review-lsa-1")).toEqual(["project:prj_review/workspace:id%3Awks_review_adam/role:lsa"]);
    expect(where.get("review-tpm-1")).toEqual(["project:prj_review/workspace:id%3Awks_review_adam/role:tpm"]);
    expect(where.get("adam-lsa-1")).toEqual(["project:prj_adam/workspace:id%3Awks_adam/role:lsa"]);
    expect(tree.find((group) => group.project?.id === ADAM.projectKey)?.desk).toBe(true);
    expect(workstream(tree, OTHER_WORKSTREAM).desk).toBe(false);
  });

  it("keeps native project/workspace keys stable as unlabelled peers arrive", () => {
    const before = buildOrganizationTree({ entries: inventory().filter((session) => session.labels.desk) });
    const after = buildOrganizationTree({ entries: inventory() });
    const keys = (groups: readonly OrganizationGroup[]) => allGroups(groups).map((group) => group.key);
    expect(keys(after)).toEqual(expect.arrayContaining(keys(before)));
    expect(keys(after)).toEqual(expect.arrayContaining(["project:prj_adam", "project:prj_adam/workspace:id%3Awks_adam"]));
  });

  it("treats a missing epic and a missing workstream as separate facts", () => {
    const tree = buildOrganizationTree({ entries: inventory() });
    const where = placement(tree);
    // Native parentage stays fixed; declared epic/workstream remain associations.
    expect(where.get("adam-epic")).toEqual(["project:prj_adam/workspace:id%3Awks_adam/role:tpm"]);
    // An explicit association never splits a native desk workspace.
    expect(where.get("adam-declared")).toEqual(["project:prj_adam/workspace:id%3Awks_adam/role:tpm"]);
    // An epic without a workstream is honest Other work, not a fabricated desk.
    expect(organizationScope(inventory().find((session) => session.id === "epic-labelled")!, {})).toEqual({ workstream: OTHER_WORKSTREAM, epic: "EX-100" });
    expect(where.get("epic-unlabelled")?.[0]).toContain(`workstream:${OTHER_WORKSTREAM.toLocaleLowerCase()}/project:prj_epic/epic:unassigned`);
    expect(isDeskWork(inventory().find((session) => session.id === "epic-unlabelled")!, {})).toBe(false);
  });

  it("shows a label's epic against its native placement without relabelling either", () => {
    const tree = buildOrganizationTree({ entries: inventory() });
    const epics = workstream(tree, OTHER_WORKSTREAM).groups.filter((group) => group.kind === "epic").map((group) => group.label);
    expect(epics).toEqual(expect.arrayContaining(["EX-100", "EX-200", NO_LINKED_EPIC]));
    expect(placement(tree).get("epic-conflict")?.[0]).toContain("/project:prj_epic/epic:ex-200/");
    // An unknown workstream value is shown as declared, in its own group.
    expect(placement(tree).get("unknown-stream")?.[0]).toMatch(/^workstream:unrecognised-stream\//);
  });

  it("keeps empty desk workspaces under their native desk across association mappings", () => {
    const tree = buildOrganizationTree({ entries: inventory(), workspaces: emptyWorkspaces });
    const empty = (id: string) => allGroups(tree).find((group) => group.kind === "workspace" && group.key.endsWith(`workspace:id%3A${id}`))!;
    expect(empty("wks_review_idle").key).toBe("project:prj_review/workspace:id%3Awks_review_idle");
    expect(empty("wks_adam_idle").key).toBe("project:prj_adam/workspace:id%3Awks_adam_idle");
    expect(empty("wks_epic_idle").key).toContain("workstream:other work/project:prj_epic/epic:unassigned/");
    const mapped = buildOrganizationTree({ entries: [], workspaces: emptyWorkspaces, displayMappings: { [workspaceMappingKey("wks_adam_idle")]: { workstream: "kontor", epic: "EX-400" } } });
    expect(allGroups(mapped).find((group) => group.label === "CSW • EX-8 • review")?.key).toBe("project:prj_adam/workspace:id%3Awks_adam_idle");
  });
});

describe("desk scope contexts", () => {
  const scopes = ["PR Review", "Adam", "Desks", "Other work"] as const;
  const count = (scope: string | null) => inventory().filter((session) => matchesScope(session, scope, {})).map((session) => session.id);

  it("counts every native desk member once, including members filed under another workstream", () => {
    expect(count("Adam")).toEqual(["adam-lsa-1", "adam-lsa-2", "adam-epic", "adam-declared"]);
    expect(count("PR Review")).toEqual(["review-tpm-1", "review-tpm-2", "review-lsa-1", "review-lsa-2"]);
    expect(count("Desks")).toEqual([...count("PR Review"), ...count("Adam")]);
    expect(count("Other work")).toEqual(["epic-labelled", "epic-unlabelled", "epic-conflict", "unknown-stream"]);
    expect(count(null)).toHaveLength(inventory().length);
    // Desks and Other work split the inventory with no overlap.
    expect(new Set([...count("Desks"), ...count("Other work")]).size).toBe(inventory().length);
    // A named workstream is still a workstream filter.
    expect(count("kontor")).toEqual(["adam-declared"]);
  });

  it("still finds desk work a label or mapping files under a desk outside its project", () => {
    const labelled = sessionEntry("loose", { ...EPIC, labels: { desk: "pr-review" } });
    expect(scopes.filter((scope) => matchesScope(labelled, scope, {}))).toEqual(["PR Review", "Desks"]);
    const mapped = sessionEntry("mapped", { ...EPIC });
    expect(scopes.filter((scope) => matchesScope(mapped, scope, { "session:mapped": { workstream: "Adam" } }))).toEqual(["Adam", "Desks"]);
  });

  it("treats label- and mapping-declared Tickets sessions outside a native desk as desk work", () => {
    const labelled = sessionEntry("ticket-label", { ...EPIC, labels: { workstream: "Tickets" } });
    const mapped = sessionEntry("ticket-map", { ...EPIC });
    const mappings = { "session:ticket-map": { workstream: "Tickets" } };
    for (const [entry, values] of [[labelled, {}], [mapped, mappings]] as const) {
      expect(isDeskWork(entry, values)).toBe(true);
      expect(matchesScope(entry, "Tickets", values)).toBe(true);
      expect(matchesScope(entry, "Desks", values)).toBe(true);
      expect(matchesScope(entry, "Other work", values)).toBe(false);
    }
    const tree = buildOrganizationTree({ entries: [labelled, mapped], displayMappings: mappings });
    const tickets = tree.find((group) => group.kind === "workstream" && group.label === "Tickets")!;
    expect(tickets).toMatchObject({ desk: true });
    expect(tickets.entries.map((entry) => entry.id).sort()).toEqual(["ticket-label", "ticket-map"]);
    expect(tree.some((group) => group.label === OTHER_WORKSTREAM)).toBe(false);
  });

  it("keeps a desk context's empty workspaces discoverable, and only that desk's", () => {
    const tree = buildOrganizationTree({ entries: inventory(), workspaces: emptyWorkspaces });
    const shown = (scope: string | null) => {
      const filtered = filterOrganizationTree(tree, (session) => matchesScope(session, scope, {}), (group) => scopeKeepsGroup(group, scope), (session) => matchesScope(session, scope, {}));
      return { ids: treeIds(filtered), empty: allGroups(filtered).filter((group) => group.kind === "workspace" && group.totalCount === 0).map((group) => group.label) };
    };
    expect(shown("Adam")).toEqual({ ids: ["adam-lsa-1", "adam-lsa-2", "adam-epic", "adam-declared"], empty: ["CSW • EX-8 • review"] });
    expect(shown("PR Review").empty).toEqual(["RSW • EX-9 • repo#9"]);
    expect(shown("Desks").empty.sort()).toEqual(["CSW • EX-8 • review", "RSW • EX-9 • repo#9"]);
    expect(shown("Other work")).toEqual({ ids: expect.arrayContaining(count("Other work")), empty: [] });
    expect(shown(null).empty).toHaveLength(3);
  });
});

describe("native identity is never merged by name", () => {
  it("keeps same-named and differently cased desk projects apart by workspace, each session once", () => {
    const twin = { projectKey: "prj_adam_2", projectName: "DESK • Adam" };
    const tree = buildOrganizationTree({ entries: [entry("one", ADAM, "wks_one", "ADAM", "LSA • one"), entry("two", twin, "wks_two", "ADAM", "LSA • two")] });
    expect(tree.map((group) => group.label)).toEqual([twin.projectName, ADAM.projectName]);
    expect(tree.flatMap((group) => group.groups.map((child) => child.key)).sort()).toEqual(["project:prj_adam/workspace:id%3Awks_one", "project:prj_adam_2/workspace:id%3Awks_two"].sort());
    expect(treeIds(tree).sort()).toEqual(["one", "two"]);
  });

  it("follows a rename honestly: a project renamed out of the desk convention is Other work, still reachable once", () => {
    const renamed = { projectKey: ADAM.projectKey, projectName: "ADAM (retired)" };
    const tree = buildOrganizationTree({ entries: [entry("kept", renamed, "wks_adam", "ADAM", "LSA • kept")] });
    expect(tree.map((group) => group.label)).toEqual([OTHER_WORKSTREAM]);
    expect(treeIds(tree)).toEqual(["kept"]);
    expect(matchesScope(tree[0]!.entries[0]!, "Adam", {})).toBe(false);
  });

  it("builds each host's tree from that host's sessions only, even with the same project and workspace IDs", () => {
    const hostA = buildOrganizationTree({ entries: [entry("a-1", ADAM, "wks_adam", "ADAM", "LSA • host a")] });
    const hostB = buildOrganizationTree({ entries: [entry("b-1", ADAM, "wks_adam", "ADAM", "LSA • host b"), entry("b-2", ADAM, "wks_adam", "ADAM", "LSA • host b2")] });
    expect([treeIds(hostA), treeIds(hostB).sort()]).toEqual([["a-1"], ["b-1", "b-2"]]);
    expect(hostA[0]!.key).toBe(hostB[0]!.key);
  });
});


describe("native desk project contexts", () => {
  it("offers a desk with no workspace or session from the project registry", () => {
    const scopes = deriveDeskScopes([], [], [
      { id: "prj_empty", name: "DESK • RESEARCH" },
      { id: "prj_not_a_desk", name: "Research" },
    ]);
    expect(scopes).toEqual([{ id: deskProjectScope("prj_empty"), label: "DESK • RESEARCH", projectId: "prj_empty" }]);
  });

  it("uses current project names, keeps same-named desks distinct and retains the ID across renames", () => {
    const one = { id: ADAM.projectKey, name: ADAM.projectName };
    const two = { id: "prj_twin", name: ADAM.projectName };
    const original = deriveDeskScopes(inventory(), emptyWorkspaces, [one, two]);
    expect(original.filter((scope) => scope.label === ADAM.projectName).map((scope) => scope.projectId)).toEqual([ADAM.projectKey, "prj_twin"]);
    const renamed = deriveDeskScopes(inventory(), emptyWorkspaces, [{ ...one, name: "DESK • DOCS" }, two]);
    expect(renamed.find((scope) => scope.projectId === ADAM.projectKey)).toEqual({ id: deskProjectScope(ADAM.projectKey), label: "DESK • DOCS", projectId: ADAM.projectKey });
    const retired = deriveDeskScopes(inventory(), emptyWorkspaces, [{ ...one, name: "ADAM (retired)" }]);
    expect(retired.some((scope) => scope.projectId === ADAM.projectKey)).toBe(false);
  });

  it("filters by exact project identity, including native members filed under another workstream", () => {
    const twin = entry("twin", { projectKey: "prj_twin", projectName: ADAM.projectName }, "wks_twin", "ADAM", "LSA • twin");
    const scope = deskProjectScope(ADAM.projectKey);
    expect([...inventory(), twin].filter((session) => matchesScope(session, scope, {})).map((session) => session.id)).toEqual(["adam-lsa-1", "adam-lsa-2", "adam-epic", "adam-declared"]);
    expect(matchesScope(twin, scope, {})).toBe(false);
    expect(matchesScope(twin, deskProjectScope("prj_twin"), {})).toBe(true);
  });

  it("keeps native empty desk workspaces across display mappings without admitting a sibling workspace", () => {
    const workspaces = [...emptyWorkspaces,
      { id: "wks_twin_idle", name: "CSW • twin", projectKey: "prj_twin", projectName: ADAM.projectName },
    ];
    const mappings = { [workspaceMappingKey("wks_adam_idle")]: { workstream: "kontor", epic: "EX-400" } };
    const tree = buildOrganizationTree({ entries: inventory(), workspaces, displayMappings: mappings });
    const show = (scope: string) => filterOrganizationTree(tree,
      (session) => matchesScope(session, scope, mappings),
      (group) => scopeKeepsGroup(group, scope, mappings),
      (session) => matchesScope(session, scope, mappings));
    for (const scope of ["Adam", "Desks", deskProjectScope(ADAM.projectKey)]) {
      expect(allGroups(show(scope)).filter((group) => group.kind === "workspace" && group.totalCount === 0).map((group) => group.label)).toContain("CSW • EX-8 • review");
    }
    expect(allGroups(show(deskProjectScope(ADAM.projectKey))).filter((group) => group.kind === "workspace" && group.totalCount === 0).map((group) => group.label)).toEqual(["CSW • EX-8 • review"]);
    expect(treeIds(show(deskProjectScope(ADAM.projectKey))).sort()).toEqual(["adam-declared", "adam-epic", "adam-lsa-1", "adam-lsa-2"]);
  });

  it("honors session mapping, workspace mapping, desk and workstream declarations before the native fallback", () => {
    const session = entry("precedence", ADAM, "wks_adam", "ADAM", "LSA • precedence", { desk: "declared-desk", workstream: "declared-stream" });
    const workspace = { [workspaceMappingKey("wks_adam")]: { workstream: "workspace-stream" } };
    expect(organizationScope(session, { ...workspace, "session:precedence": { workstream: "session-stream" } }).workstream).toBe("session-stream");
    expect(organizationScope(session, workspace).workstream).toBe("workspace-stream");
    expect(organizationScope(session, {}).workstream).toBe("declared-desk");
    expect(organizationScope({ ...session, labels: { workstream: "declared-stream" } }, {}).workstream).toBe("declared-stream");
    expect(organizationScope({ ...session, labels: {} }, {}).workstream).toBe("adam");
    expect(matchesScope(session, deskProjectScope(ADAM.projectKey), workspace)).toBe(true);
  });
});
