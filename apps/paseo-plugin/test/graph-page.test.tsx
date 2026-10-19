import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ evidence: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: () => [model.evidence, (value: boolean) => { model.evidence = value; }] }));
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", ScrollView: "ScrollView", Icon: "Icon" }));
import { GraphPage } from "../client/graph-page";
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
  it("keeps provenance compact until one disclosure and passes the reviewed graph props unchanged", () => {
    const closed = render();
    expect(elements(closed.tree).some((node) => node.type === SourceDetails)).toBe(false);
    const action = elements(closed.tree).find((node) => node.props.label === "Source details")!;
    (action.props.onPress as () => void)(); const opened = render();
    const details = elements(opened.tree).find((node) => node.type === SourceDetails)!;
    expect(details.props.expanded).toBe(true);
    expect(opened.graph?.props.store).toBe(closed.graph?.props.store);
    expect(opened.graph?.props.graph).toEqual(closed.graph?.props.graph);
    expect(opened.graph?.props.theme).toEqual(closed.graph?.props.theme);
    expect(opened.graph?.props).not.toHaveProperty("zoom");
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
    expect(component).toBeUndefined(); expect(tree.props.data).toEqual(graph.nodes);
    expect(render().graph?.props.sourcedStates).toBeUndefined();
  });
  it("captures Board position, restores once after Plan/Back and persists the host-clamped offset", () => {
    const hostId = "aud-d549-board-return";
    const route = navigate(hostId, { tab: "Board", sourceId: "plan", sourceRevision: source.revision });
    const mount = () => GraphPage({ mode: "Board", hostId, theme: { colors: {} } as PluginTheme, compact: true, sources: [source.source], selectedId: "plan", reads: { plan: source }, available: true, error: null, onSelect: vi.fn(), onIssue: vi.fn(), initialOffset: readRouteView(hostId, route).offset, onScroll: (offset) => writeRouteView(hostId, route, { offset }), onPlan: (sourceId) => { navigate(hostId, { tab: "Plan", sourceId }); } });
    const first = mount();
    first.props.onScroll({ nativeEvent: { contentOffset: { y: 800 } } });
    const item = first.props.renderItem({ item: graph.nodes[0]!, index: 0 });
    const open = elements(item).find((element) => String(element.props.label).startsWith("Open plan task"))!;
    (open.props.onPress as () => void)(); goBack(hostId);
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
