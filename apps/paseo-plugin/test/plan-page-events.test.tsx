import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
const hooks = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(),
  useRef: (current: unknown) => { const index = hooks.cursor++; if (!(index in hooks.slots)) hooks.slots[index] = { current }; return hooks.slots[index]; },
  useState: <T,>(initial: T | (() => T)) => {
    const index = hooks.cursor++; if (!(index in hooks.slots)) hooks.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [hooks.slots[index] as T, (next: T) => { hooks.slots[index] = next; }];
  } }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { PlanReader, type PlanView } from "../client/plan-reader";
import { FormattedPlanBlock, planBlocks, tableRowAt, taskDeclaration } from "../client/plan-document";
import { AnchoredMenu } from "../client/anchored-menu";
type Props = Parameters<typeof PlanReader>[0];
type Node = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Node[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  // The highlighted table row lives inside the hook-free formatted block.
  const children = node.type === FormattedPlanBlock ? FormattedPlanBlock(node.props as Parameters<typeof FormattedPlanBlock>[0]) : node.props.children as ReactNode;
  return [node, ...elements(children)];
}
const layout = (values: object) => ({ nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 0, ...values } } });
const TABLE = "# Plan\nIntro.\n\n| Task | Title |\n| --- | --- |\n| T1 | First |\n| T2 | Second |\n\n## End\nLast line.";
const GRAPH = "# Plan\n\n```plan-graph\n{\n  \"nodes\": [\n    {\n      \"id\": \"T9\",\n      \"title\": \"Ninth\"\n    }\n  ]\n}\n```\n";
function mount(content: string, overrides: Partial<Props> = {}) {
  hooks.slots = [];
  const read: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: "/plans/a.md", projectId: null, workstream: null, epic: null, revisionKind: "working copy" }, status: "ready", revision: "hash", readAt: "now", modifiedAt: null, content, graph: null, records: null, diagnostics: [] };
  const props: Props = { colors: {} as Props["colors"], sources: [read.source], reads: { plan: read }, sourceId: "plan", sourceError: null, entries: [], query: "", initialOffset: 0,
    setQuery: vi.fn((value: string) => { props.query = value; }), onScroll: vi.fn(), onView: vi.fn(), ...overrides };
  let tree: ReactNode = null;
  const render = () => { hooks.cursor = 0; tree = PlanReader(props); };
  render();
  const all = () => elements(tree);
  const find = (test: (node: Node) => boolean) => all().find(test);
  const byId = (id: string) => find((node) => node.props.testID === id);
  const scrollTo = vi.fn(); (byId("plan-document")!.props.ref as { current: unknown }).current = { scrollTo };
  const fire = (id: string, values: object) => { (byId(id)!.props.onLayout as (event: object) => void)(layout(values)); };
  const press = (label: string) => { (find((node) => node.props.label === label || node.props.accessibilityLabel === label)!.props.onPress as () => void)(); render(); };
  return { props, all, find, byId, fire, press, render, scrollTo };
}
beforeEach(() => { hooks.slots = []; hooks.cursor = 0; });
describe("task declaration lookup", () => {
  it("finds the exact table row, heading token or plan-graph node, never a fuzzy match", () => {
    expect(taskDeclaration(TABLE, "T2")).toEqual({ line: 6, kind: "table" });
    expect(taskDeclaration(TABLE, "T")).toBeNull();
    expect(taskDeclaration("# Plan\n## T5 Ship it\nT5 in prose", "T5")).toEqual({ line: 1, kind: "heading" });
    expect(taskDeclaration("## T50 Other", "T5")).toBeNull();
    expect(taskDeclaration(GRAPH, "T9")).toEqual({ line: 6, kind: "graph" });
    expect(taskDeclaration("```json\n\"id\": \"T9\"\n```", "T9")).toBeNull();
    const table = planBlocks(TABLE).find((block) => block.kind === "table")!;
    expect([tableRowAt(table, 3), tableRowAt(table, 5), tableRowAt(table, 6), tableRowAt(table, 7)]).toEqual([null, 1, 2, null]);
  });
});
describe("Plan reader controls", () => {
  it("opens Contents as an anchored overlay, keeps exact Source and search, and offers no plan mutations", () => {
    const screen = mount(TABLE, { compact: true });
    expect(screen.find((node) => node.type === AnchoredMenu)).toBeUndefined();
    expect(screen.find((node) => node.props.label === "Contents (2)")!.props.expanded).toBe(false);
    screen.press("Contents (2)");
    const menu = screen.find((node) => node.type === AnchoredMenu)!;
    expect(menu.props).toMatchObject({ testID: "plan-contents-menu", label: "Plan contents", placement: { align: "stretch" } });
    expect(screen.all().filter((node) => typeof node.props.indent === "number").map((node) => [node.props.label, node.props.indent])).toEqual([["Plan", 0], ["End", 1]]);
    expect(screen.find((node) => node.props.label === "Contents (2)")!.props.expanded).toBe(true);
    (menu.props.onClose as () => void)(); screen.render();
    expect(screen.find((node) => node.type === AnchoredMenu)).toBeUndefined();
    screen.press("Exact Source");
    expect(screen.byId("plan-exact-source")?.props.children).toBe(TABLE);
    expect(screen.props.onView).toHaveBeenLastCalledWith({ mode: "source", target: null, revealed: false });
    (screen.find((node) => node.props.accessibilityLabel === "Search this plan")!.props.onChangeText as (value: string) => void)("End"); screen.render();
    expect(screen.props.query).toBe("End");
    expect(screen.byId("plan-exact-source")).toBeUndefined();
    expect(screen.find((node) => node.props.label === "Contents (1)")).toBeDefined();
    expect(screen.all().some((node) => node.props.children === "1 match" || (Array.isArray(node.props.children) && node.props.children.join("") === "1 match"))).toBe(true);
    expect(screen.all().some((node) => /approve|execute|edit plan/i.test(String(node.props.label)))).toBe(false);
  });
  it("offers the dependency graph only when the host wires it", () => {
    expect(mount(TABLE).find((node) => node.props.label === "Dependency graph")).toBeUndefined();
    const onGraph = vi.fn(); const screen = mount(TABLE, { onGraph });
    screen.press("Dependency graph"); expect(onGraph).toHaveBeenCalledTimes(1);
  });
});
describe("graph task target", () => {
  const target = (taskId: string, revision: string | null = "hash"): PlanView => ({ mode: "formatted", target: { taskId, revision }, revealed: false });
  it("reveals the exact table row once, just below the sticky bar", () => {
    const screen = mount(TABLE, { view: target("T2") });
    expect(screen.byId("plan-target-status")!.props.children).toBe("T2 · declared at source line 7");
    expect(screen.byId("plan-target-row")).toBeDefined();
    const block = screen.find((node) => node.type === FormattedPlanBlock && node.props.targetRow === 2)!;
    expect(block).toBeDefined();
    screen.fire("plan-reader-bar", { height: 50 });
    screen.fire("plan-line-3", { y: 100 });
    expect(screen.scrollTo).not.toHaveBeenCalled();
    (block.props.onTargetLayout as (y: number) => void)(80);
    expect(screen.scrollTo).not.toHaveBeenCalled();
    screen.fire("plan-article", { y: 200 });
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 200 + 100 + 5 + 80 - 50 - 8, animated: false }]]);
    expect(screen.props.onScroll).toHaveBeenLastCalledWith(327);
    expect(screen.props.onView).toHaveBeenLastCalledWith({ mode: "formatted", target: { taskId: "T2", revision: "hash" }, revealed: true });
    screen.fire("plan-article", { y: 210 }); screen.fire("plan-line-3", { y: 120 });
    expect(screen.scrollTo).toHaveBeenCalledTimes(1);
  });
  it("highlights a heading declaration and reveals it", () => {
    const screen = mount("# Plan\nIntro.\n## T5 Ship it\nBody", { view: target("T5") });
    expect(screen.byId("plan-line-2")!.props.style).toMatchObject({ borderLeftWidth: 3 });
    screen.fire("plan-reader-bar", { height: 40 }); screen.fire("plan-article", { y: 100 }); screen.fire("plan-line-2", { y: 60 });
    expect(screen.scrollTo).toHaveBeenCalledWith({ y: 112, animated: false });
  });
  it("reveals a plan-graph node in exact Source and keeps that mode", () => {
    const screen = mount(GRAPH, { view: target("T9") });
    expect(screen.find((node) => node.props.accessibilityLabel === "Exact Source")!.props.selected).toBe(true);
    expect(screen.find((node) => node.props.accessibilityLabel === "T9 declaration, source line 7")).toBeDefined();
    screen.fire("plan-reader-bar", { height: 50 }); screen.fire("plan-article", { y: 200 }); screen.fire("plan-source-target", { y: 300 });
    expect(screen.scrollTo).toHaveBeenCalledWith({ y: 442, animated: false });
    expect(screen.props.onView).toHaveBeenLastCalledWith({ mode: "source", target: { taskId: "T9", revision: "hash" }, revealed: true });
  });
  it("says when the task is not declared in this revision, never scrolls, and clears", () => {
    const screen = mount(TABLE, { view: target("T404") });
    expect(screen.byId("plan-target-status")!.props.children).toBe("T404 is not declared in this exact source revision (hash).");
    screen.fire("plan-reader-bar", { height: 50 }); screen.fire("plan-article", { y: 200 });
    expect(screen.scrollTo).not.toHaveBeenCalled();
    screen.press("Clear T404 highlight");
    expect(screen.props.onView).toHaveBeenLastCalledWith({ mode: "formatted", target: null, revealed: false });
    expect(screen.byId("plan-target-status")).toBeUndefined();
  });
  it("names a revision change between the graph link and this read", () => {
    const screen = mount(TABLE, { view: target("T1", "0123456789abcdef") });
    expect(screen.byId("plan-target-status")!.props.children).toBe("T1 · declared at source line 6 · linked from revision 0123456789ab, now hash");
  });
  it("lets a new task target win over the route's saved position", () => {
    const screen = mount(TABLE, { view: target("T2"), initialOffset: 500 });
    (screen.byId("plan-document")!.props.onLayout as (event: object) => void)(layout({ height: 600 }));
    (screen.byId("plan-document")!.props.onContentSizeChange as (width: number, height: number) => void)(400, 3000);
    expect(screen.scrollTo).not.toHaveBeenCalled();
    screen.fire("plan-reader-bar", { height: 50 }); screen.fire("plan-article", { y: 200 }); screen.fire("plan-line-3", { y: 100 });
    (screen.find((node) => node.type === FormattedPlanBlock && node.props.targetRow === 2)!.props.onTargetLayout as (y: number) => void)(80);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 327, animated: false }]]);
  });
  it("restores the saved position instead of revealing again after Back", () => {
    const screen = mount(TABLE, { view: { ...target("T2"), revealed: true }, initialOffset: 500 });
    screen.fire("plan-reader-bar", { height: 50 }); screen.fire("plan-article", { y: 200 }); screen.fire("plan-line-3", { y: 100 });
    expect(screen.scrollTo).not.toHaveBeenCalled();
    (screen.byId("plan-document")!.props.onLayout as (event: object) => void)(layout({ height: 600 }));
    (screen.byId("plan-document")!.props.onContentSizeChange as (width: number, height: number) => void)(400, 3000);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 500, animated: false }]]);
  });
});
