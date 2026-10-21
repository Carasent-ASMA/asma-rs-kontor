import { describe, expect, it } from "vitest";
import {
  allGroupKeys, buildOrganizationTree, deskProjectScope, filterOrganizationTree, flattenOrganizationTree,
  matchesScope, organizationWorkspaceCount, scopeKeepsGroup, type OrganizationGroup,
} from "../client/session-organization";
import { matchesResultSearch, sessionAncestors, sessionProvenance } from "../client/session-results";
import { NATIVE_DESK_PROJECTS, NATIVE_DESK_WORKSPACES, nativeDeskSessions } from "./fixtures/native-desks";

const entries = nativeDeskSessions();
const input = { entries, projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES };
const walk = (groups: readonly OrganizationGroup[]): OrganizationGroup[] => groups.flatMap((group) => [group, ...walk(group.groups)]);

describe("native desk structure", () => {
  it.each(["hierarchy", "placement"] as const)("keeps real workspace children in %s regardless of conflicting associations", (grouping) => {
    const tree = buildOrganizationTree({ ...input, grouping, displayMappings: { "session:desk-epic-a": { workstream: "mapped", epic: "ASMA-99999" }, "workspace:w_idle": { workstream: "other", epic: "ASMA-99999" } } });
    const desk = tree.find((group) => group.key === "project:prj_desk")!;
    expect(desk.label).toBe("DESK • ADAM"); expect(desk.detail).toBe("5 workspaces");
    expect(desk.groups.every((group) => group.kind === "workspace")).toBe(true);
    expect(desk.groups.map((group) => group.label)).toEqual(expect.arrayContaining(["Active plan format • ASMA-99101", "CSW • ASMA-99102 • review", "ADAM", "ASW • ASMA-99103 • advice"]));
    expect(desk.groups.filter((group) => group.label === "ADAM")).toHaveLength(2);
    const workspace = desk.groups.find((group) => group.key === "project:prj_desk/workspace:id%3Aw_adam")!;
    expect(workspace.entries.map((entry) => entry.id).sort()).toEqual(["desk-epic-a", "desk-epic-b", "desk-epic-c", "desk-epic-d", "desk-unlabelled"]);
    expect(walk([desk]).some((group) => group.kind === "epic" || group.kind === "workstream" || group.kind === "workspace-kind")).toBe(false);
    const placed = walk(tree).flatMap((group) => group.sessions.map((entry) => entry.id));
    expect(placed.length).toBe(entries.length); expect(new Set(placed).size).toBe(entries.length);
    expect(desk.totalCount).toBe(8);
    expect(organizationWorkspaceCount([desk])).toBe(5);
    expect(tree.find((group) => group.key === "project:prj_twin")!.entries.map((entry) => entry.id)).toEqual(["twin"]);
  });

  it("retains an empty native desk and its explicit zero-workspace state through scope filtering", () => {
    const tree = buildOrganizationTree(input);
    const empty = tree.find((group) => group.key === "project:prj_empty")!;
    expect(empty.detail).toBe("0 workspaces"); expect(empty.totalCount).toBe(0);
    const scope = deskProjectScope("prj_empty");
    const filtered = filterOrganizationTree(tree, (entry) => matchesScope(entry, scope, {}), (group) => scopeKeepsGroup(group, scope), (entry) => matchesScope(entry, scope, {}));
    expect(filtered.map((group) => group.key)).toEqual([empty.key]);
    expect(flattenOrganizationTree(filtered, {}, true).find((row) => row.kind === "empty")).toMatchObject({ parentKey: empty.key, entity: "project" });
  });

  it("keeps association searches in one native workspace and Show in tree returns its native ancestors", () => {
    const tree = buildOrganizationTree(input);
    const mappings = { "session:desk-epic-a": { epic: "ASMA-99999", workstream: "Mapped association" } };
    const entry = entries.find((entry) => entry.id === "desk-epic-a")!;
    expect(matchesResultSearch(entry, ["mapped", "asma-99999"], mappings)).toBe(true);
    expect(sessionAncestors(tree, entry.id)?.slice(0, 2)).toEqual(["project:prj_desk", "project:prj_desk/workspace:id%3Aw_adam"]);
    expect(sessionProvenance(entry, mappings)).toMatchObject({ parts: ["DESK • ADAM", "ADAM"], desk: true, nativeDesk: { id: "prj_desk", workspaceId: "w_adam" } });
    const scope = deskProjectScope("prj_desk");
    expect(entries.filter((entry) => matchesScope(entry, scope, mappings))).toHaveLength(8);
  });

  it("preserves native keys, order and disclosure through project and workspace renames", () => {
    const renamed = {
      entries: entries.map((entry) => entry.projectKey === "prj_desk" ? { ...entry, projectName: "DESK • DOCS", workspaceName: entry.workspaceId === "w_adam" ? "Renamed workspace" : entry.workspaceName } : entry),
      projects: NATIVE_DESK_PROJECTS.map((project) => project.id === "prj_desk" ? { ...project, name: "DESK • DOCS" } : project),
      workspaces: NATIVE_DESK_WORKSPACES.map((workspace) => workspace.projectKey === "prj_desk" ? { ...workspace, projectName: "DESK • DOCS", name: workspace.id === "w_adam" ? "Renamed workspace" : workspace.name } : workspace),
    };
    const order = ["project:prj_desk/workspace:id%3Aw_idle", "project:prj_desk/workspace:id%3Aw_adam"];
    const tree = buildOrganizationTree({ ...renamed, siblingOrder: { "project:prj_desk": order } });
    expect(allGroupKeys(tree).sort()).toEqual(allGroupKeys(buildOrganizationTree(input)).sort());
    const desk = tree.find((group) => group.project?.id === "prj_desk")!;
    expect(desk.label).toBe("DESK • DOCS");
    expect(desk.groups.slice(0, 2).map((group) => group.key)).toEqual(order);
    expect(flattenOrganizationTree([desk], { [desk.key]: false })[0]).toMatchObject({ expanded: false });
    expect(sessionProvenance(renamed.entries[0]!, {}).parts).toEqual(["DESK • DOCS", "Renamed workspace"]);
    expect(matchesScope(renamed.entries[0]!, deskProjectScope("prj_desk"), {})).toBe(true);
  });

  it("keeps desk shortcuts native while separate workstream filters explore conflicting declarations", () => {
    const conflict = entries.find((entry) => entry.id === "desk-epic-d")!;
    expect(matchesScope(conflict, "Adam", {})).toBe(true);
    expect(matchesScope(conflict, "PR Review", {})).toBe(false);
    expect(matchesScope(conflict, "pr-review", {})).toBe(true); // explicit workstream exploration
    const tree = buildOrganizationTree(input);
    const review = filterOrganizationTree(tree, (entry) => matchesScope(entry, "PR Review", {}), (group) => scopeKeepsGroup(group, "PR Review"), (entry) => matchesScope(entry, "PR Review", {}));
    expect(review.flatMap((group) => group.entries.map((entry) => entry.id))).toEqual(["review"]);
  });

  it("preserves non-desk workstream/epic behavior and existing native placement keys", () => {
    const control = entries.filter((entry) => entry.projectKey === "prj_control");
    const oldControl = buildOrganizationTree({ entries: control });
    const mixed = buildOrganizationTree(input);
    expect(mixed.find((group) => group.label === "control-stream")).toEqual(oldControl[0]);
    expect(allGroupKeys(mixed)).toContain("project:prj_desk/workspace:id%3Aw_adam");
    const saved = { "project:prj_desk": false, "workstream:adam/project:prj_desk/epic:asma-99101": true };
    expect(flattenOrganizationTree(mixed, saved).find((row) => row.kind === "group" && row.group.key === "project:prj_desk")).toMatchObject({ expanded: false });
    expect(saved["workstream:adam/project:prj_desk/epic:asma-99101"]).toBe(true);
  });
});
