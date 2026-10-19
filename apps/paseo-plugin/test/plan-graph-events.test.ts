import type { PluginTheme } from "@getpaseo/plugin";
import { readFileSync } from "node:fs";
import { URL } from "node:url";
import type { ReactElement, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { planGraphSchema, type PlanGraphData } from "../client/plan-graph/contract";

const hooks = vi.hoisted(() => {
  const slots: unknown[] = [];
  let cursor = 0;
  return {
    reset() { slots.length = 0; cursor = 0; },
    render() { cursor = 0; },
    useMemo: (factory: () => unknown) => factory(),
    useSyncExternalStore: (_subscribe: unknown, read: () => unknown) => read(),
    useRef(initial: unknown) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = { current: initial };
      return slots[index];
    },
    useState(initial: unknown) {
      const index = cursor++;
      if (!(index in slots)) slots[index] = initial;
      return [slots[index], (next: unknown) => { slots[index] = next; }];
    },
  };
});
vi.mock("react", async (importOriginal) => ({ ...await importOriginal<typeof import("react")>(), ...hooks }));
vi.mock("react-native", () => ({ View: "View", Text: "Text", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView" }));

import { PlanGraph, createPlanGraphStore, createPlanTapGuard, activePlanNode, planGraphViewKey, type PlanGraphProps } from "../client/plan-graph/graph";

const theme: PluginTheme = { colors: {
  foreground: "fg", foregroundMuted: "muted", accent: "accent", accentForeground: "accent-fg",
  surface0: "surface-0", surface1: "surface-1", surface2: "surface-2", border: "border",
  statusDanger: "danger", statusSuccess: "success", statusWarning: "warning",
} };
type Element = { type: unknown; props: Record<string, unknown> };
function walk(node: unknown): Element[] {
  if (Array.isArray(node)) return node.flatMap(walk);
  if (!node || typeof node !== "object" || !("props" in node)) return [];
  const element = node as Element;
  return [element, ...walk(element.props.children)];
}
function content(node: unknown): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(content).join(" ");
  if (node && typeof node === "object" && "props" in node) return content((node as Element).props.children);
  return "";
}
function find(tree: unknown, id: string): Element {
  const element = walk(tree).find((item) => item.props.testID === id || item.props.accessibilityLabel === id);
  if (!element) throw new Error(`Element not found: ${id}`);
  return element;
}
function emit(element: Element, name: string, event?: unknown) {
  const handler = element.props[name];
  if (typeof handler !== "function") throw new Error(`Handler missing: ${name}`);
  handler(event);
}
function style(element: Element): Record<string, unknown> { return element.props.style as Record<string, unknown>; }
function sample(): PlanGraphData { return JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8")); }
function rootGraph(count: number): PlanGraphData {
  const graph = sample();
  graph.nodes = Array.from({ length: count }, (_, index) => ({
    id: `TASK-${String(index).padStart(3, "0")}`, title: `Task ${index}`, phase: "Phase 1",
    jira: null, kontor_id: null, depends_on: [], wave: 0,
  }));
  graph.waves = [{ wave: 0, tasks: graph.nodes.map((node) => node.id) }];
  graph.edges = []; graph.external_fences = [];
  return graph;
}
function setup(patch: Partial<PlanGraphProps> = {}) {
  const props: PlanGraphProps = { graph: sample(), hostId: "host-a", store: createPlanGraphStore(), theme, ...patch };
  const render = () => {
    hooks.render();
    const wrapper = PlanGraph(props) as ReactElement<PlanGraphProps, (props: PlanGraphProps) => ReactNode>;
    return wrapper.type(wrapper.props);
  };
  return { props, render, key: planGraphViewKey(props.hostId, props.graph) };
}

describe("native PlanGraph events (mocked host primitives, not device qualification)", () => {
  beforeEach(() => hooks.reset());

  it("renders only the 19 canonical cards and four directed connectors at full contrast", () => {
    const { render, props, key } = setup();
    const tree = render();
    const cards = walk(tree).filter((node) => String(node.props.testID).startsWith("plan-node-"));
    const edges = walk(tree).filter((node) => String(node.props.testID).startsWith("plan-edge-"));
    expect(cards).toHaveLength(19); expect(edges).toHaveLength(4);
    expect([...cards, ...edges].every((element) => style(element).opacity === 1)).toBe(true);
    expect(props.store.read(key).selected).toBeNull();
    expect(content(tree)).toContain("Status not supplied");
    expect(content(tree)).toContain("Host host-a");
    expect(content(tree)).toContain("c8d1abadfe73");
    expect(content(tree)).not.toMatch(/ready to start|frontier|serializes.with|governance|planned.start|capacity|In Development/i);
    expect(walk(tree).filter((node) => node.props.accessibilityRole === "button").every((node) => Number(style(node).minHeight) >= 44)).toBe(true);
  });

  it("prioritizes hover over focus over selection; temporary exit restores intentional selection", () => {
    const { render, props, key } = setup();
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-002");
    emit(find(render(), "plan-node-TASK-003"), "onFocus");
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-003 · Write the PR review skill");
    emit(find(render(), "plan-node-TASK-005"), "onHoverIn");
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-005 · Free OpenRouter");
    emit(find(render(), "plan-node-TASK-005"), "onHoverOut");
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-003 · Write the PR review skill");
    emit(find(render(), "plan-node-TASK-003"), "onBlur");
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-002 · Approve model chains");
    emit(find(render(), "Clear graph highlight"), "onPress");
    expect(props.store.read(key).selected).toBeNull();
    const restored = walk(render()).filter((node) => /plan-(node|edge)-/.test(String(node.props.testID)));
    expect(restored.every((node) => style(node).opacity === 1)).toBe(true);
  });

  it("hover/focus without selection restores full contrast on exit; accessible clear resets all channels", () => {
    const { render } = setup();
    emit(find(render(), "plan-node-TASK-003"), "onFocus");
    expect(style(find(render(), "plan-node-TASK-002")).opacity).toBe(0.35);
    emit(find(render(), "plan-node-TASK-003"), "onBlur");
    expect(style(find(render(), "plan-node-TASK-002")).opacity).toBe(1);
    emit(find(render(), "plan-node-TASK-003"), "onHoverIn");
    emit(find(render(), "plan-node-TASK-003"), "onAccessibilityAction", { nativeEvent: { actionName: "escape" } });
    expect(style(find(render(), "plan-node-TASK-002")).opacity).toBe(1);
    emit(find(render(), "plan-node-TASK-003"), "onHoverIn");
    emit(find(render(), "plan-node-TASK-003"), "onHoverOut");
    expect(style(find(render(), "plan-edge-TASK-002-TASK-004")).opacity).toBe(1);
  });

  it("shows phase, direct relationships, confirmed Jira and external fence source facts", () => {
    const { render } = setup();
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    const details = content(find(render(), "plan-graph-details"));
    expect(details).toContain("Phase: Phase 1");
    expect(details).toContain("Direct prerequisites: None");
    expect(details).toContain("Direct dependents: TASK-004");
    expect(details).toContain("Confirmed Jira: ASMA-8373");
    expect(details).toContain("Sourced state: not supplied by this export");
    expect(details).toContain("Owner: Igor");
    expect(details).toContain("Lift condition: Igor approves");
    expect(details).toContain("Plan source:");
  });

  it("displays status only when independently sourced for this host and exact revision", () => {
    const { render, props, key } = setup();
    const states = { "TASK-002": { label: "Source-reported status", source: "Jira read ASMA-8373", observedAt: "2026-10-19T10:00:00Z" } };
    props.sourcedStates = { viewKey: key, nodes: states };
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    expect(content(find(render(), "plan-graph-details"))).toContain("Source-reported status · Jira read ASMA-8373 · observed 2026-10-19T10:00:00Z");
    props.sourcedStates = { viewKey: "other-host-or-revision", nodes: states };
    expect(content(find(render(), "plan-graph-details"))).toContain("not supplied by this export");
    props.sourcedStates = { viewKey: key, nodes: { "TASK-002": { ...states["TASK-002"], observedAt: "invalid" } } };
    expect(content(find(render(), "plan-graph-details"))).toContain("not supplied by this export");
  });

  it("shows only supplied source capabilities and emits canonical identities", () => {
    const onPlan = vi.fn(); const onJira = vi.fn();
    const { props, render } = setup({ sourceActions: { plan: onPlan } });
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    expect(walk(render()).some((node) => node.props.accessibilityLabel === "Open ASMA-8373")).toBe(false);
    emit(find(render(), "Open exact plan source"), "onPress");
    expect(onPlan).toHaveBeenCalledWith({ kind: "plan", source: props.graph.source, source_sha256: props.graph.source_sha256, taskId: "TASK-002" });
    props.sourceActions = { jira: onJira };
    emit(find(render(), "Open ASMA-8373"), "onPress");
    expect(onJira).toHaveBeenCalledWith({ kind: "jira", jira: "ASMA-8373" });
  });

  it("guards touch selection against a pan, native cancellation, and scroll drag", () => {
    const { render, props, key } = setup();
    const touch = (x: number, y: number) => ({ nativeEvent: { pageX: x, pageY: y } });
    const task = () => find(render(), "plan-node-TASK-002");
    emit(task(), "onPressIn", touch(10, 10)); emit(task(), "onTouchMove", touch(30, 10)); emit(task(), "onPress");
    expect(props.store.read(key).selected).toBeNull();
    emit(task(), "onPressIn", touch(10, 10)); emit(task(), "onTouchCancel"); emit(task(), "onPress");
    expect(props.store.read(key).selected).toBeNull();
    emit(task(), "onPressIn", touch(10, 10));
    emit(walk(render()).find((node) => node.props.onScrollBeginDrag)!, "onScrollBeginDrag"); emit(task(), "onPress");
    expect(props.store.read(key).selected).toBeNull();
    emit(task(), "onPressIn", touch(10, 10)); emit(task(), "onTouchMove", touch(12, 12)); emit(task(), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-002");
  });

  it("uses focus/activation and explicit task controls for keyboard and accessibility navigation", () => {
    const { props, render, key } = setup();
    emit(find(render(), "Next task"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-001");
    emit(find(render(), "Next task"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-002");
    emit(find(render(), "Previous task"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-001");
    emit(find(render(), "plan-node-TASK-003"), "onFocus");
    emit(find(render(), "plan-node-TASK-003"), "onAccessibilityAction", { nativeEvent: { actionName: "activate" } });
    expect(props.store.read(key).selected).toBe("TASK-003");
    emit(find(render(), "Clear graph highlight"), "onPress");
    expect(props.store.read(key).selected).toBeNull();
  });

  it("keeps compact native graph interactive and supplies a selectable, directed wave-list alternate", () => {
    const { render, props, key } = setup({ compact: true, textScale: 2 });
    expect(find(render(), "plan-graph-canvas")).toBeDefined();
    expect(Number(style(find(render(), "plan-node-TASK-002")).minHeight)).toBe(320);
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(content(render())).toContain("TASK-002 → TASK-004");
    expect(props.store.read(key).selected).toBe("TASK-002");
    emit(find(render(), "plan-node-TASK-003"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-003");
    expect(content(render())).toContain("Zoom is unavailable");
    emit(find(render(), "Show graph canvas"), "onPress");
    expect(find(render(), "plan-graph-canvas")).toBeDefined();
  });

  it("preserves canvas lineage colors and opacity on every alternate edge, including diamond bypasses and clear", () => {
    const graph = sample();
    const prerequisites: string[][] = [[], ["TASK-001"], ["TASK-002"], ["TASK-002"], ["TASK-001", "TASK-002", "TASK-003", "TASK-004"], []];
    graph.nodes = prerequisites.map((depends_on, index) => ({
      id: `TASK-00${index + 1}`, title: `Task ${index + 1}`, phase: "Phase 1", jira: null, kontor_id: null,
      depends_on, wave: [0, 1, 2, 2, 3, 0][index]!,
    }));
    graph.edges = graph.nodes.flatMap((node) => node.depends_on.map((from) => ({ from, to: node.id })));
    graph.waves = [0, 1, 2, 3].map((wave) => ({ wave, tasks: graph.nodes.filter((node) => node.wave === wave).map((node) => node.id) }));
    graph.external_fences = [];
    const { render } = setup({ graph });
    emit(find(render(), "plan-node-TASK-003"), "onPress");
    const expected = graph.edges.map((edge) => {
      const element = find(render(), `plan-edge-${edge.from}-${edge.to}`);
      const segment = walk(element).find((child) => style(child)?.backgroundColor)!;
      return { edge, opacity: style(element).opacity, color: style(segment).backgroundColor };
    });
    emit(find(render(), "Fit as wave list"), "onPress");
    for (const { edge, opacity, color } of expected) {
      const element = find(render(), `plan-list-edge-${edge.from}-${edge.to}`);
      expect(style(element).opacity).toBe(opacity);
      expect(style(walk(element).find((child) => child.type === "Text")!).color).toBe(color);
    }
    expect(style(find(render(), "plan-list-edge-TASK-001-TASK-005")).opacity).toBe(0.2);
    expect(style(find(render(), "plan-list-edge-TASK-003-TASK-005")).opacity).toBe(1);
    emit(find(render(), "Clear graph highlight"), "onPress");
    for (const edge of graph.edges) {
      const element = find(render(), `plan-list-edge-${edge.from}-${edge.to}`);
      expect(style(element).opacity).toBe(1);
      expect(style(walk(element).find((child) => child.type === "Text")!).color).toBe(theme.colors.foregroundMuted);
    }
  });

  it("uses the active card foreground for prerequisite text in a monochrome alternate", () => {
    const monochrome: PluginTheme = { colors: { ...theme.colors, foreground: "#ffffff", surface1: "#111111", accent: "#ffffff", accentForeground: "#000000" } };
    const { render } = setup({ theme: monochrome });
    emit(find(render(), "Fit as wave list"), "onPress");
    emit(find(render(), "plan-node-TASK-003"), "onPress");
    const card = find(render(), "plan-node-TASK-003");
    expect(style(card).backgroundColor).toBe(monochrome.colors.accent);
    const texts = walk(card).filter((element) => element.type === "Text");
    expect(texts.some((element) => content(element).startsWith("Prerequisites:"))).toBe(true);
    expect(texts.every((element) => style(element).color === monochrome.colors.accentForeground)).toBe(true);
    emit(find(render(), "Clear graph highlight"), "onPress");
    expect(walk(find(render(), "plan-node-TASK-003")).filter((element) => element.type === "Text")
      .every((element) => style(element).color === monochrome.colors.foreground)).toBe(true);
  });

  it("preserves independent canvas and list offsets across Fit, paging, delayed scroll events and return", () => {
    const { render, props, key } = setup({ graph: rootGraph(100) });
    const scrollEvent = (y: number) => ({ nativeEvent: { contentOffset: { x: 0, y } } });
    const canvas = walk(render()).find((element) => element.props.onScroll && !element.props.horizontal)!;
    emit(canvas, "onScroll", scrollEvent(10000));
    emit(find(render(), "Fit as wave list"), "onPress");
    const list = find(render(), "plan-graph-list-scroll");
    expect(list.props.contentOffset).toEqual({ x: 0, y: 0 });
    emit(canvas, "onScroll", scrollEvent(0)); // Old canvas event after the mode changed.
    expect(props.store.read(key).y).toBe(10000);
    emit(list, "onScroll", scrollEvent(500));
    expect(props.store.read(key).listY).toBe(500);
    emit(find(render(), "Next tasks page"), "onPress");
    expect(props.store.read(key).listY).toBe(0);
    emit(list, "onScroll", scrollEvent(750)); // Old page event after paging.
    expect(props.store.read(key).listY).toBe(0);
    const nextList = find(render(), "plan-graph-list-scroll");
    emit(nextList, "onScroll", scrollEvent(450));
    emit(find(render(), "Show graph canvas"), "onPress");
    emit(nextList, "onScroll", scrollEvent(0)); // Unmounted list cannot overwrite its saved offset.
    expect(props.store.read(key)).toMatchObject({ y: 10000, listY: 450, page: 1 });
    expect(find(render(), "plan-graph-canvas-scroll").props.contentOffset).toEqual({ x: 0, y: 10000 });
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(find(render(), "plan-graph-list-scroll").props.contentOffset).toEqual({ x: 0, y: 450 });
    expect(content(render())).toContain("Page 2 of 3");
    hooks.reset();
    expect(find(render(), "plan-graph-list-scroll").props.contentOffset).toEqual({ x: 0, y: 450 });
    expect(props.store.read(key).y).toBe(10000);
  });

  it("opens Fit on the intentional selection's page and uses measured row position to reveal it", () => {
    const { render, props, key } = setup({ graph: rootGraph(100) });
    const scrollTo = vi.fn();
    for (const element of walk(render())) {
      const ref = element.props.ref as { current: unknown } | undefined;
      if (ref) ref.current = { scrollTo };
    }
    props.store.update(key, { y: 10000 });
    emit(find(render(), "plan-node-TASK-099"), "onPress");
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(content(render())).toContain("Page 3 of 3");
    expect(find(render(), "plan-node-TASK-099").props.accessibilityState).toEqual({ selected: true });
    emit(find(render(), "plan-list-row-TASK-099"), "onLayout", { nativeEvent: { layout: { y: 3900 } } });
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 3900, animated: false });
    expect(props.store.read(key)).toMatchObject({ selected: "TASK-099", page: 2, listY: 3900, y: 10000 });
    emit(find(render(), "Show graph canvas"), "onPress");
    expect(find(render(), "plan-graph-canvas-scroll").props.contentOffset).toEqual({ x: 0, y: 10000 });
  });

  it("tracks the fitted row through intrinsic many-prerequisite reflow without repeating settled scrolls", () => {
    const graph = rootGraph(100);
    const selected = graph.nodes.at(-1)!;
    selected.depends_on = graph.nodes.slice(0, 80).map((node) => node.id);
    selected.wave = 1;
    graph.edges = selected.depends_on.map((from) => ({ from, to: selected.id }));
    graph.waves = [
      { wave: 0, tasks: graph.nodes.slice(0, -1).map((node) => node.id) },
      { wave: 1, tasks: [selected.id] },
    ];
    const { render, props, key } = setup({ graph: planGraphSchema.parse(graph), compact: true });
    props.store.update(key, { x: 175, y: 10000 });
    emit(find(render(), "plan-node-TASK-099"), "onPress");
    emit(find(render(), "Fit as wave list"), "onPress");
    const scrollTo = vi.fn();
    (find(render(), "plan-graph-list-scroll").props.ref as { current: unknown }).current = { scrollTo };
    const rowLayout = (y: number) => ({ nativeEvent: { layout: { y } } });
    emit(find(render(), "plan-list-row-TASK-099"), "onLayout", rowLayout(3900));
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 3900, animated: false });
    emit(find(render(), "plan-node-TASK-099"), "onLayout", { nativeEvent: { layout: { height: 400 } } });
    expect(style(find(render(), "plan-node-TASK-099")).minHeight).toBe(400);
    emit(find(render(), "plan-list-row-TASK-099"), "onLayout", rowLayout(8460));
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 8460, animated: false });
    expect(props.store.read(key)).toMatchObject({ x: 175, y: 10000, listY: 8460, selected: "TASK-099" });
    emit(find(render(), "plan-list-row-TASK-099"), "onLayout", rowLayout(8460));
    expect(scrollTo).toHaveBeenCalledTimes(2);
    // A later intrinsic expansion must still be tracked; no timer assumes layout has settled.
    emit(find(render(), "plan-node-TASK-099"), "onLayout", { nativeEvent: { layout: { height: 480 } } });
    emit(find(render(), "plan-list-row-TASK-099"), "onLayout", rowLayout(9980));
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 9980, animated: false });
    expect(props.store.read(key).listY).toBe(9980);
  });

  it.each(["drag", "clear", "selection", "page", "canvas"])("stops Fit reflow tracking after explicit %s intent", (intent) => {
    const { render, props, key } = setup({ graph: rootGraph(100), compact: true });
    emit(find(render(), "plan-node-TASK-099"), "onPress");
    emit(find(render(), "Fit as wave list"), "onPress");
    const scrollTo = vi.fn();
    (find(render(), "plan-graph-list-scroll").props.ref as { current: unknown }).current = { scrollTo };
    const row = find(render(), "plan-list-row-TASK-099");
    emit(row, "onLayout", { nativeEvent: { layout: { y: 3900 } } });
    if (intent === "drag") emit(find(render(), "plan-graph-list-scroll"), "onScrollBeginDrag");
    if (intent === "clear") emit(find(render(), "Clear graph highlight"), "onPress");
    if (intent === "selection") {
      emit(find(render(), "plan-node-TASK-098"), "onPress");
      emit(find(render(), "plan-node-TASK-099"), "onPress");
    }
    if (intent === "page") emit(find(render(), "Previous tasks page"), "onPress");
    if (intent === "canvas") emit(find(render(), "Show graph canvas"), "onPress");
    scrollTo.mockClear();
    const savedOffset = props.store.read(key).listY;
    emit(row, "onLayout", { nativeEvent: { layout: { y: 8460 } } });
    expect(scrollTo).not.toHaveBeenCalled();
    expect(props.store.read(key).listY).toBe(savedOffset);
  });

  it("centres a selected task through native scroll refs and preserves selection/offset on return", () => {
    const { render, props, key } = setup();
    const scrollTo = vi.fn();
    for (const element of walk(render())) {
      const ref = element.props.ref as { current: unknown } | undefined;
      if (ref) ref.current = { scrollTo };
    }
    emit(find(render(), "plan-node-TASK-005"), "onPress");
    emit(find(render(), "Centre selected task"), "onPress");
    expect(scrollTo).toHaveBeenCalledTimes(2);
    expect(props.store.read(key).x).toBeGreaterThan(0);
    const saved = props.store.read(key);
    hooks.reset();
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-005");
    expect(props.store.read(key)).toEqual(saved);
  });

  it("separates host and revision state explicitly, retaining each prior view", () => {
    const { render, props, key } = setup();
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    props.hostId = "host-b"; hooks.reset();
    expect(content(render())).toContain("Host host-b");
    expect(props.store.read(planGraphViewKey(props.hostId, props.graph)).selected).toBeNull();
    props.hostId = "host-a"; props.graph = { ...props.graph, source_sha256: "f".repeat(64) }; hooks.reset();
    expect(content(render())).toContain("source ffffffffffff");
    expect(props.store.read(planGraphViewKey(props.hostId, props.graph)).selected).toBeNull();
    expect(props.store.read(key).selected).toBe("TASK-002");
  });

  it("bounds rendering for large graphs with paged, interactive canonical wave lists", () => {
    const graph = sample();
    graph.nodes = Array.from({ length: 1000 }, (_, i) => ({ id: `TASK-${i}`, title: `Task ${i}`, phase: "Phase 1", jira: null, kontor_id: null, depends_on: [], wave: 0 }));
    graph.waves = [{ wave: 0, tasks: graph.nodes.map((node) => node.id) }]; graph.edges = []; graph.external_fences = [];
    const { render, props, key } = setup({ graph, compact: true });
    const scrollTo = vi.fn();
    for (const element of walk(render())) {
      const ref = element.props.ref as { current: unknown } | undefined;
      if (ref) ref.current = { scrollTo };
    }
    expect(walk(render()).filter((node) => String(node.props.testID).startsWith("plan-node-"))).toHaveLength(40);
    expect(content(render())).toContain("Page 1 of 25");
    emit(find(render(), "Next tasks page"), "onPress");
    expect(content(render())).toContain("Page 2 of 25");
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 0, animated: false });
    const node = walk(render()).find((item) => String(item.props.testID).startsWith("plan-node-"))!;
    emit(node, "onPress");
    expect(props.store.read(key).selected).not.toBeNull();
    expect(find(render(), "plan-graph-details")).toBeDefined();
  });
});

describe("interaction state helpers", () => {
  it("keeps precedence explicit, including empty channels", () => {
    expect(activePlanNode({ hovered: "A", focused: "B", selected: "C" })).toBe("A");
    expect(activePlanNode({ hovered: null, focused: "B", selected: "C" })).toBe("B");
    expect(activePlanNode({ hovered: null, focused: null, selected: "C" })).toBe("C");
    expect(activePlanNode({ hovered: null, focused: null, selected: null })).toBeNull();
  });
  it("persists scoped views, notifies consumers, and supports teardown", () => {
    const store = createPlanGraphStore(); const listener = vi.fn(); const stop = store.subscribe(listener);
    const initial = store.read("a"); expect(store.read("a")).toBe(initial);
    store.update("a", { selected: "TASK-001", x: 100 });
    expect(listener).toHaveBeenCalledTimes(1); expect(store.read("b").selected).toBeNull();
    stop(); store.update("a", { y: 200 }); expect(listener).toHaveBeenCalledTimes(1);
    expect(store.read("a").x).toBe(100); store.clear(); expect(store.read("a")).toBe(initial);
  });
  it("never treats a drag or cancelled press as a tap and allows subsequent keyboard activation", () => {
    const guard = createPlanTapGuard(); guard.start(0, 0); guard.move(6, 6); expect(guard.canPress()).toBe(false);
    guard.keyboardFocus(); expect(guard.canPress()).toBe(true);
    guard.cancel(); expect(guard.canPress()).toBe(false);
    guard.start(0, 0); guard.move(0, 8); expect(guard.canPress()).toBe(true);
  });
});
