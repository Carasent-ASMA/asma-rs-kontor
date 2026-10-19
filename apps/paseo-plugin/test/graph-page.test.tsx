import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ evidence: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: () => [model.evidence, (value: boolean) => { model.evidence = value; }] }));
vi.mock("react-native", () => ({ Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", ScrollView: "ScrollView", Icon: "Icon" }));
import { GraphPage, type BoardItem } from "../client/graph-page";
import { planGraphSchema } from "../client/plan-graph/contract";
import { PlanGraph, planGraphViewKey } from "../client/plan-graph/graph";
import { navigate, goBack, readRouteView, writeRouteView } from "../client/navigation-state";
import { SourceDetails } from "../client/source-details";
beforeEach(() => { model.evidence = false; });

const graph: CanonicalGraph = JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8"));
const source: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: graph.source, workstream: null, epic: graph.epic.jira, projectId: null, revisionKind: "working copy" }, status: "ready", revision: graph.source_sha256, readAt: "2026-10-19T00:00:00Z", modifiedAt: null, content: "# Plan", graph, records: null, diagnostics: [] };
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
function render(read = source, hostId = "adapter-host", mode: "Graph" | "Board" = "Graph", extraReads: Record<string, SourceRead> = {}) {
  const onPlan = vi.fn(); const onIssue = vi.fn();
  const tree = GraphPage({ mode, hostId, theme: { colors: {} } as PluginTheme, compact: true, sources: [read.source], selectedId: read.source.id, reads: { ...extraReads, [read.source.id]: read }, available: true, error: null, onSelect: vi.fn(), onPlan, onIssue });
  return { tree, onPlan, onIssue, graph: elements(tree).find((element) => element.type === PlanGraph) };
}
describe("canonical graph shell adapter", () => {
  it("renders no duplicate page header and hands provenance to the graph's single Source details disclosure", () => {
    const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
    const jira: SourceRead = { ...source, source: { ...source.source, id: "jira", title: "Historical Jira", kind: "jira", epic: null }, graph: null, jiraHistory: history };
    const closed = render(source, "adapter-host", "Graph", { jira });
    expect(elements(closed.tree).filter((node) => node.props.accessibilityRole === "header")).toEqual([]);
    expect(elements(closed.tree).some((node) => node.type === SourceDetails || node.props.label === "Source details")).toBe(false);
    const details = elements(closed.graph?.props.details as ReactNode).filter((node) => node.type === SourceDetails);
    expect(details.map((node) => [(node.props.read as SourceRead).source.id, node.props.expanded])).toEqual([["plan", true], ["jira", true]]);
    const again = render(source, "adapter-host", "Graph", { jira });
    expect(again.graph?.props.store).toBe(closed.graph?.props.store);
    expect(again.graph?.props.graph).toEqual(closed.graph?.props.graph);
    expect(again.graph?.props.theme).toEqual(closed.graph?.props.theme);
    expect(again.graph?.props).not.toHaveProperty("zoom");
  });
  it("rejects a graph from another source revision", () => {
    expect(render({ ...source, revision: "f".repeat(64) }).graph).toBeUndefined();
    expect(render({ ...source, graph: null, status: "partial" }).graph).toBeUndefined();
  });
  it("retains the host store and forwards only source-bound Plan and declared Jira actions", () => {
    const first = render(); const second = render();
    expect(first.graph?.props.store).toBe(second.graph?.props.store);
    expect(render(source, "other-host").graph?.props.store).not.toBe(first.graph?.props.store);
    const actions = first.graph?.props.sourceActions as { plan(action: object): void; jira(action: object): void };
    actions.plan({ kind: "plan", source: graph.source, source_sha256: graph.source_sha256, taskId: "TASK-001" });
    actions.plan({ kind: "plan", source: "/different.md", source_sha256: graph.source_sha256 });
    actions.plan({ kind: "plan", source: graph.source, source_sha256: "f".repeat(64) });
    actions.plan({ kind: "plan", source: graph.source, source_sha256: graph.source_sha256, taskId: graph.nodes[0]!.jira });
    actions.plan({ kind: "plan", source: graph.source, source_sha256: graph.source_sha256, taskId: graph.nodes[0]!.title });
    expect(first.onPlan.mock.calls).toEqual([["plan", "TASK-001"]]);
    actions.jira({ kind: "jira", jira: graph.nodes[0]!.jira }); actions.jira({ kind: "jira", jira: "ASMA-999999" });
    expect(first.onIssue.mock.calls).toEqual([[graph.nodes[0]!.jira]]);
  });
  it("passes exact-key historical observations through the existing graph API with the original time", () => {
    const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
    const jira: SourceRead = { ...source, source: { ...source.source, id: "jira", title: "Historical Jira", kind: "jira", epic: null }, graph: null, jiraHistory: history };
    const result = render(source, "history-host", "Graph", { jira });
    const sourced = result.graph?.props.sourcedStates as { viewKey: string; nodes: Record<string, { label: string; source: string; observedAt: string }> };
    expect(sourced.viewKey).toBe(planGraphViewKey("history-host", planGraphSchema.parse(graph)));
    expect(sourced.nodes[graph.nodes[0]!.id]).toEqual({ label: "Closed (historical)", source: "Historical Jira", observedAt: "2026-10-18T17:00+00:00" });
    expect(render(source, "history-host", "Graph", { jira: { ...jira, status: "unavailable" } }).graph?.props.sourcedStates).toBeUndefined();
  });
  it("gives Board canonical declarations without supplying invented task states", () => {
    const { tree, graph: component } = render(source, "board-host", "Board");
    const data = tree.props.data as BoardItem[];
    expect(component).toBeUndefined();
    expect(data.flatMap((item) => item.kind === "task" ? [item.node] : [])).toEqual(graph.nodes);
    expect(data.map((item) => item.kind)).toEqual(["section", ...graph.nodes.map(() => "task")]);
    expect(render().graph?.props.sourcedStates).toBeUndefined();
  });
  it("captures Board position, restores once after Plan/Back and persists the host-clamped offset", () => {
    const hostId = "aud-d549-board-return";
    const route = navigate(hostId, { tab: "Board", sourceId: "plan", sourceRevision: source.revision });
    const mount = () => GraphPage({ mode: "Board", hostId, theme: { colors: {} } as PluginTheme, compact: true, sources: [source.source], selectedId: "plan", reads: { plan: source }, available: true, error: null, onSelect: vi.fn(), onIssue: vi.fn(), initialOffset: readRouteView(hostId, route).offset, onScroll: (offset) => writeRouteView(hostId, route, { offset }), onPlan: (sourceId) => { navigate(hostId, { tab: "Plan", sourceId }); } });
    const first = mount();
    first.props.onScroll({ nativeEvent: { contentOffset: { y: 800 } } });
    const task = (first.props.data as BoardItem[]).find((value) => value.kind === "task")!;
    // Compact rows are one button: pressing the row opens the task in its plan.
    const item = first.props.renderItem({ item: task, index: 1 }) as ReactElement<Record<string, unknown>>;
    expect(String(item.props.label).startsWith("Open plan task")).toBe(true);
    (item.props.onPress as () => void)(); goBack(hostId);
    const returned = mount(); const scrollToOffset = vi.fn(); returned.props.ref.current = { scrollToOffset };
    returned.props.onScroll({ nativeEvent: { contentOffset: { y: 0 } } });
    returned.props.onContentSizeChange(400, 3000);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 800, animated: false });
    returned.props.onScroll({ nativeEvent: { contentOffset: { y: 360 } } });
    returned.props.onContentSizeChange(400, 4000);
    expect(scrollToOffset).toHaveBeenCalledTimes(1); expect(readRouteView(hostId, route).offset).toBe(360);
    navigate(hostId, { sourceRevision: "other-revision" }); expect(readRouteView(hostId).offset).toBe(0);
    expect(readRouteView("other-board-host", route).offset).toBe(0);
  });
});

/** Root-first ancestors of the first matching element, or null when it is absent. */
function ancestorsOf(node: ReactNode, match: (element: ReactElement<Record<string, unknown>>) => boolean): ReactElement<Record<string, unknown>>[] | null {
  if (Array.isArray(node)) {
    for (const child of node) { const path = ancestorsOf(child as ReactNode, match); if (path) return path; }
    return null;
  }
  if (!isValidElement<Record<string, unknown>>(node)) return null;
  if (match(node)) return [];
  const path = ancestorsOf(node.props.children as ReactNode, match);
  return path ? [node, ...path] : null;
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map((child) => text(child as ReactNode)).join(" ");
  return isValidElement<Record<string, unknown>>(node) ? text(node.props.children as ReactNode) : "";
}

describe("compact Graph mounting (iPhone 115a regression)", () => {
  const mount = (available: boolean, error: string | null) => GraphPage({ mode: "Graph", hostId: "chain-host", theme: { colors: {} } as PluginTheme, compact: true,
    sources: [source.source], selectedId: "plan", reads: { plan: source }, available, error, onSelect: vi.fn(), onPlan: vi.fn(), onIssue: vi.fn() });

  it.each([[true, null], [false, "Read failed"]] as const)("mounts the plan graph under bounded flex:1 Views only, never a ScrollView or FlatList (available %s)", (available, error) => {
    const tree = mount(available, error);
    const path = ancestorsOf(tree, (element) => element.type === PlanGraph)!;
    expect(path).not.toBeNull();
    expect(path.map((element) => element.type)).toEqual(["View"]);
    expect(path[0]!.props.style).toEqual({ flex: 1 });
    expect(elements(tree).some((element) => element.type === "ScrollView" || element.type === "FlatList")).toBe(false);
  });

  it("keeps read times, hashes and host provenance out of the Graph page outside the Source details disclosure", () => {
    const tree = mount(false, "Read failed");
    const outside = text(tree);
    const hidden = [source.readAt, graph.source_sha256.slice(0, 12), graph.graph_sha256.slice(0, 12), "chain-host", "Historical Jira:"];
    expect(hidden.filter((value) => outside.includes(value))).toEqual([]);
    expect(outside).toContain("Source service unavailable");
    const component = elements(tree).find((element) => element.type === PlanGraph)!;
    expect(text(component.props.details as ReactNode)).toContain("Historical Jira:");
  });
});
