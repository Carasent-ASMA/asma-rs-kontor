import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import type { Colors } from "../client/page-controls";
import { buildOrganizationTree, type OrganizationGroup, type OrganizationInput } from "../client/session-organization";
import { relateSplitProjects } from "../client/split-project-rows";
import { sessionEntry } from "./session-fixtures";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory() }));
vi.mock("react-native", () => ({ PanResponder: { create: () => ({ panHandlers: {} }) }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

// One native project's sessions settle different epics, so it shows a recognised row and an Epic unknown row; the
// reported ESW projects, with neutral native IDs. Only native project IDs relate rows; titles and project names never do.
const at = (id: string, project: string, labels: Record<string, string> = {}, name = `ESW • ${project}`) =>
  sessionEntry(id, { projectKey: `prj_${project}`, projectName: name, workspaceId: `wks_${id}`, workspaceName: `TSW • ${id}`, labels });
const empty = (id: string, project: string, name = `ESW • ${project}`) => ({ id, name: `TSW • ${id}`, projectKey: `prj_${project}`, projectName: name });
const shape = (tree: readonly OrganizationGroup[]) => tree.map((top) => [top.label, top.groups.filter((group) => group.kind === "epic").map((group) => [group.label, group.totalCount, group.detail] as const)] as const);
const members = (tree: readonly OrganizationGroup[]) => {
  const all = (group: OrganizationGroup): string[] => [group.key, ...group.groups.flatMap(all), ...group.sessions.map((entry) => entry.id)];
  return tree.flatMap(all).sort();
};
function related(input: Omit<OrganizationInput, "grouping">, roots?: ReadonlyMap<string, string>) {
  const built = buildOrganizationTree({ ...input, grouping: "hierarchy" });
  const tree = relateSplitProjects(built, roots);
  // Presentation only: every group key, session and count is the tree's own.
  expect(members(tree)).toEqual(members(built));
  expect(tree.flatMap((top) => top.groups.map((group) => [group.key, group.totalCount])).sort()).toEqual(built.flatMap((top) => top.groups.map((group) => [group.key, group.totalCount])).sort());
  // No two sibling rows read alike.
  for (const top of tree) expect(new Set(top.groups.map((group) => group.label)).size).toBe(top.groups.length);
  return { built, tree };
}

describe("an Epic unknown row beside its project's recognised rows", () => {
  it("follows the same project's last recognised row in one workstream and says how they relate", () => {
    const { built, tree } = related({
      entries: [at("a1", "7869", { "jira.epic": "ASMA-7869" }), at("a2", "7869", { "jira.epic": "ASMA-8001", "jira.parent": "ASMA-7869" }), at("a3", "7869"),
        at("b1", "8113", { "jira.epic": "ASMA-8113" }), at("b2", "8113"), at("b3", "8113"), at("c1", "8490")],
      workspaces: [empty("wks_7869_done", "7869")],
    });
    // As built, every Epic unknown row sorts after every recognised one.
    expect(shape(built)[0]![1].map(([label]) => label)).toEqual(["ASMA-7869 · ESW • 7869", "ASMA-8001 · ESW • 7869", "ASMA-8113 · ESW • 8113", "Epic unknown · ESW • 7869", "Epic unknown · ESW • 8113", "Epic unknown · ESW • 8490"]);
    expect(shape(tree)).toEqual([["Other work", [
      ["ASMA-7869 · ESW • 7869", 1, "1 more session of this project has no known epic"],
      ["ASMA-8001 · ESW • 7869", 1, "1 more session of this project has no known epic"],
      ["Epic unknown · ESW • 7869", 1, "1 session with no known epic · 1 workspace without sessions · same project as ASMA-7869, ASMA-8001"],
      ["ASMA-8113 · ESW • 8113", 1, "2 more sessions of this project have no known epic"],
      ["Epic unknown · ESW • 8113", 2, "2 sessions with no known epic · same project as ASMA-8113"],
      // No recognised row: it stays last and says only what it holds.
      ["Epic unknown · ESW • 8490", 1, "1 session with no known epic"],
    ]]]);
  });

  it("leaves a cross-workstream pair in place and names each from the other", () => {
    const { built, tree } = related({
      entries: [at("k1", "8108", { "asma.epic": "ASMA-8108" }), at("k2", "8108", { "asma.epic": "ASMA-8108" }), at("u1", "8108"), at("u2", "8108"), at("x1", "8501")],
      displayMappings: { "session:k1": { workstream: "mobile-app" }, "session:k2": { workstream: "mobile-app" } },
      workspaces: [empty("wks_8108_done", "8108")],
    });
    // Same rows in the same workstreams and order as built; only the details relate them.
    expect(shape(tree).map(([stream, rows]) => [stream, rows.map(([label]) => label)])).toEqual(shape(built).map(([stream, rows]) => [stream, rows.map(([label]) => label)]));
    expect(shape(tree)).toEqual([
      ["mobile-app", [["ASMA-8108", 2, "2 more sessions of this project have no known epic, in Other work"]]],
      ["Other work", [["Epic unknown · ESW • 8108", 2, "2 sessions with no known epic · 1 workspace without sessions · same project as ASMA-8108 in mobile-app"], ["Epic unknown · ESW • 8501", 1, "1 session with no known epic"]]],
    ]);
  });

  it.each([false, true])("counts the project's sessions with no known epic in each workstream that holds them (workspace without sessions elsewhere=%s)", (withEmpty) => {
    const { tree } = related({
      entries: [at("known", "a", { "jira.epic": "ASMA-1" }), at("local", "a"), at("remote1", "a"), at("remote2", "a")],
      displayMappings: { "session:known": { workstream: "alpha" }, "session:local": { workstream: "alpha" }, "session:remote1": { workstream: "beta" }, "session:remote2": { workstream: "beta" } },
      workspaces: withEmpty ? [empty("wks_a_done", "a")] : [],
    });
    // Not "3 more … in beta": one of the three is beside it in alpha. A workstream holding none of them is not named.
    expect(shape(tree)).toEqual([
      ["alpha", [["ASMA-1", 1, "3 more sessions of this project have no known epic: 1 in alpha, 2 in beta"], ["Epic unknown", 1, "1 session with no known epic · same project as ASMA-1"]]],
      ["beta", [["Epic unknown", 2, "2 sessions with no known epic · same project as ASMA-1 in alpha"]]],
      ...withEmpty ? [["Other work", [["Epic unknown", 0, "1 workspace without sessions · same project as ASMA-1 in alpha"]]] as const] : [],
    ]);
  });

  it("words an empty-only Epic unknown row as the tree does for workspaces without sessions, so either order of composition reads the same", () => {
    const input = { entries: [at("r1", "8101", { "jira.epic": "ASMA-8101" }), at("r2", "8101", { "jira.epic": "ASMA-8101" })], workspaces: [empty("wks_done_a", "8101"), empty("wks_done_b", "8101")] };
    const expected = [["Other work", [["ASMA-8101", 2, "2 more workspaces of this project without sessions"], ["Epic unknown", 0, "2 workspaces without sessions · same project as ASMA-8101"]]]];
    expect(shape(related(input).tree)).toEqual(expected);
    // The tree may already give that group its own detail; it comes first and is not repeated.
    const withTreeDetail = buildOrganizationTree({ ...input, grouping: "hierarchy" }).map((top) => ({ ...top, groups: top.groups.map((group) => group.epic === null ? { ...group, detail: "2 workspaces without sessions" } : group) }));
    expect(shape(relateSplitProjects(withTreeDetail))).toEqual(expected);
  });

  it("never relates rows by title or project name, only by native project", () => {
    // Two native projects with one name: the unlabelled one is not called the same project as the labelled one.
    const { tree } = related({ entries: [at("p1", "one", { "jira.epic": "ASMA-1" }, "shared"), at("p2", "two", {}, "shared")] });
    expect(shape(tree)).toEqual([["Other work", [["ASMA-1 · shared", 1, null], ["Epic unknown · shared", 1, "1 session with no known epic"]]]]);
  });
});

describe("sibling rows that different native projects label alike", () => {
  const kontor = ["asma-8564", "asma-8564-prdesk", "asma-8568-seat-a", "asma-8574"];
  const input = {
    entries: [...kontor.map((folder) => at(`k-${folder}`, folder, {}, "asma-rs-kontor")),
      at("qa-1", "qa-8541", { "jira.epic": "ASMA-8539" }, "qa"), at("qa-2", "qa-8548", { "jira.epic": "ASMA-8539" }, "qa"), at("qa-1-bare", "qa-8541", {}, "qa")],
    displayMappings: { "session:qa-1": { workstream: "kontor" }, "session:qa-2": { workstream: "kontor" } },
  };
  const roots = new Map([...kontor.map((folder) => [`prj_${folder}`, `/Users/someone/carasent/asma-modules/.worktrees/${folder}/asma-rs-kontor`] as const),
    ["prj_qa-8541", "/Users/someone/carasent/asma-modules/.worktrees/asma-8541/qa"], ["prj_qa-8548", "/Users/someone/carasent/asma-modules/.worktrees/asma-8548/qa"]]);
  const labels = (tree: readonly OrganizationGroup[]) => shape(tree).map(([stream, rows]) => [stream, rows.map(([label]) => label)]);

  it("names each by the one folder where their native roots differ, on every row of that project, never a path", () => {
    const { tree } = related(input, roots);
    // The unlabelled qa session's row is alone in Other work but carries its project's folder, so it relates to one row.
    expect(shape(tree)).toEqual([
      ["kontor", [["ASMA-8539 · qa (asma-8541)", 1, "1 more session of this project has no known epic, in Other work"], ["ASMA-8539 · qa (asma-8548)", 1, null]]],
      ["Other work", [...kontor.map((folder) => [`Epic unknown · asma-rs-kontor (${folder})`, 1, "1 session with no known epic"]),
        ["Epic unknown · qa (asma-8541)", 1, "1 session with no known epic · same project as ASMA-8539 in kontor"]]],
    ]);
    expect(labels(tree).flatMap(([, rows]) => rows).some((label) => /[\\/]|someone/.test(String(label)))).toBe(false);
  });

  it("falls back to an ordinal when roots are missing or cannot tell them apart", () => {
    expect(labels(related(input).tree)).toEqual([
      ["kontor", ["ASMA-8539 · qa (1 of 2)", "ASMA-8539 · qa (2 of 2)"]],
      ["Other work", [...kontor.map((_, index) => `Epic unknown · asma-rs-kontor (${index + 1} of 4)`), "Epic unknown · qa (1 of 2)"]],
    ]);
    const same = new Map([...roots].map(([id]) => [id, "/same/root"]));
    expect(labels(related(input, same).tree)[0]![1]).toEqual(["ASMA-8539 · qa (1 of 2)", "ASMA-8539 · qa (2 of 2)"]);
  });

  it("names projects that collide through one another together, so no two rows end up alike", () => {
    // A and B share ASMA-1, B and D share ASMA-2, C and D share Epic unknown; no roots.
    const { tree } = related({ entries: [at("a1", "a", { "jira.epic": "ASMA-1" }, "shared"), at("b1", "b", { "jira.epic": "ASMA-1" }, "shared"), at("b2", "b", { "jira.epic": "ASMA-2" }, "shared"),
      at("d1", "d", { "jira.epic": "ASMA-2" }, "shared"), at("c1", "c", {}, "shared"), at("d2", "d", {}, "shared")] });
    expect(shape(tree)).toEqual([["Other work", [
      ["ASMA-1 · shared (1 of 4)", 1, null], ["ASMA-1 · shared (2 of 4)", 1, null], ["ASMA-2 · shared (2 of 4)", 1, null],
      ["ASMA-2 · shared (3 of 4)", 1, "1 more session of this project has no known epic"],
      ["Epic unknown · shared (3 of 4)", 1, "1 session with no known epic · same project as ASMA-2"],
      ["Epic unknown · shared (4 of 4)", 1, "1 session with no known epic"],
    ]]]);
  });

  it("checks each named label against every sibling, including projects outside the cohort", () => {
    // a and b are both `repo`; their folders would make a read `repo (main)`, which project c is already called.
    const { tree } = related({ entries: [at("a", "a", {}, "repo"), at("b", "b", {}, "repo"), at("c", "c", {}, "repo (main)")] },
      new Map([["prj_a", "/srv/main/repo"], ["prj_b", "/srv/branch/repo"]]));
    expect(labels(tree)).toEqual([["Other work", ["Epic unknown · repo (1 of 2)", "Epic unknown · repo (2 of 2)", "Epic unknown · repo (main)"]]]);
    // If positions collide as well, the projects behind them are named together.
    const { tree: again } = related({ entries: [at("a", "a", {}, "repo"), at("b", "b", {}, "repo"), at("c", "c", {}, "repo (1 of 2)")] });
    expect(labels(again)).toEqual([["Other work", ["Epic unknown · repo (1 of 3)", "Epic unknown · repo (2 of 3)", "Epic unknown · repo (1 of 2) (3 of 3)"]]]);
  });

  const pair = (left: string, right: string) => labels(related({ entries: [at("l", "left", {}, "repo"), at("r", "right", {}, "repo")] }, new Map([["prj_left", left], ["prj_right", right]])).tree)[0]![1];
  it.each([
    ["macOS", "/Users/alice/work/repo", "/Users/bob/work/repo"],
    ["Linux", "/home/alice/work/repo", "/home/bob/work/repo"],
    ["Windows", "C:\\Users\\alice\\work\\repo", "C:\\Users\\bob\\work\\repo"],
    ["older Windows", "C:\\Documents and Settings\\alice\\work\\repo", "C:\\Documents and Settings\\bob\\work\\repo"],
    ["a home beside a folder outside any home", "/opt/work/repo", "/Users/bob/work/repo"],
    ["the root user's home", "/root/work/repo", "/srv/work/repo"],
    ["two homes", "/Users/alice", "/Users/bob"],
  ])("never names a project by a home folder or one above it, but by its position (%s)", (_, left, right) => {
    expect(pair(left, right)).toEqual(["Epic unknown · repo (1 of 2)", "Epic unknown · repo (2 of 2)"]);
  });
  it.each([
    ["macOS", "/Users/alice/asma-1/repo", "/Users/alice/asma-2/repo"],
    ["Linux", "/home/alice/src/asma-1/repo", "/home/alice/src/asma-2/repo"],
    ["Windows", "C:\\Users\\alice\\asma-1\\repo", "C:\\Users\\alice\\asma-2\\repo"],
    ["outside any home", "/srv/asma-1/repo", "/srv/asma-2/repo"],
  ])("names a project by the first folder below the home folder (%s)", (_, left, right) => {
    expect(pair(left, right)).toEqual(["Epic unknown · repo (asma-1)", "Epic unknown · repo (asma-2)"]);
  });
});

type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
const colors: Colors = {
  surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7",
  foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b",
};

describe("a group row's detail on a phone", () => {
  it.each([true, false])("shows the detail as text, up to two lines under the label when compact, and in the accessible label (compact=%s)", async (compact) => {
    const { OrganizationRowView } = await import("../client/organization-row");
    const { tree } = related({ entries: [at("k1", "8108", { "asma.epic": "ASMA-8108" }), at("u1", "8108")] });
    const group = tree[0]!.groups.find((candidate) => candidate.epic === null)!;
    const onPress = () => {};
    const row = OrganizationRowView({ row: { kind: "group", group, depth: 1, expanded: false }, colors, compact, disabled: false, arranging: false, selected: false, favorite: false,
      onFavorite: onPress, onToggle: onPress, onMove: onPress, onSelect: onPress });
    const nodes = elements(row);
    const detail = nodes.find((node) => node.type === "Text" && node.props.children === group.detail)!;
    expect(group.detail).toBe("1 session with no known epic · same project as ASMA-8108");
    expect(detail.props.numberOfLines).toBe(compact ? 2 : 1);
    expect(String(nodes.find((node) => node.type === "Pressable")!.props.accessibilityLabel)).toContain(`, ${group.detail}, 1 sessions.`);
  });
});
