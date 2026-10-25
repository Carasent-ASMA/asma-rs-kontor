import type { PluginTheme } from "@getpaseo/plugin";
import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { createElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { planGraphSchema, type PlanGraphData } from "../client/plan-graph/contract";

const hooks = vi.hoisted(() => {
  const slots: unknown[] = [];
  let cursor = 0;
  let effects: (() => void)[] = [];
  return {
    reset() {
      for (const slot of slots) {
        if (slot && typeof slot === "object" && "cleanup" in slot && typeof slot.cleanup === "function") slot.cleanup();
      }
      slots.length = 0; cursor = 0; effects = [];
    },
    render() { cursor = 0; effects = []; },
    flushEffects() { const queue = effects; effects = []; queue.forEach((effect) => effect()); return queue.length > 0; },
    useLayoutEffect(effect: () => void | (() => void), deps: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index] as { deps: readonly unknown[]; cleanup?: () => void } | undefined;
      if (!previous || previous.deps.length !== deps.length || previous.deps.some((value, i) => !Object.is(value, deps[i]))) {
        const slot: { deps: readonly unknown[]; cleanup?: () => void } = { deps };
        slots[index] = slot;
        effects.push(() => { previous?.cleanup?.(); const cleanup = effect(); if (cleanup) slot.cleanup = cleanup; });
      }
    },
    useMemo(factory: () => unknown, deps: readonly unknown[]) {
      const index = cursor++;
      const previous = slots[index] as { deps: readonly unknown[]; value: unknown } | undefined;
      if (previous && previous.deps.length === deps.length && previous.deps.every((value, i) => Object.is(value, deps[i]))) return previous.value;
      const value = factory(); slots[index] = { deps, value }; return value;
    },
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
vi.mock("react", async (importOriginal) => ({ ...await importOriginal<typeof import("react")>(), useContext: () => null, ...hooks }));
vi.mock("react-native", () => ({ ActivityIndicator: "ActivityIndicator", View: "View", Text: "Text", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));

import { Action } from "../client/page-controls";
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
  if (element.type === Action) return walk(Action(element.props as Parameters<typeof Action>[0]));
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
function emit(element: Element, name: string, ...events: unknown[]) {
  const handler = element.props[name];
  if (typeof handler !== "function") throw new Error(`Handler missing: ${name}`);
  handler(...events);
}
function style(element: Element): Record<string, unknown> { const value = element.props.style; return (typeof value === "function" ? value({ pressed: false }) : value) as Record<string, unknown>; }
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
    let tree: ReactNode = null;
    let iterations = 0;
    do {
      hooks.render();
      const wrapper = PlanGraph(props) as ReactElement<PlanGraphProps, (props: PlanGraphProps) => ReactNode>;
      tree = wrapper.type(wrapper.props);
      if (++iterations > 15) throw new Error("Graph layout effects did not settle");
    } while (hooks.flushEffects());
    return tree;
  };
  return { props, render, key: planGraphViewKey(props.hostId, props.graph) };
}

function settleCanvas(tree: ReactNode) {
  const envelope = style(find(tree, "plan-graph-scroll-envelope"));
  emit(find(tree, "plan-graph-canvas-scroll"), "onContentSizeChange", 0, envelope.height);
  emit(walk(tree).find((element) => element.props.horizontal)!, "onContentSizeChange", envelope.width, envelope.height);
}

/** Secondary controls sit behind the retained More disclosure (GRAPH-01). */
function openMore(render: () => ReactNode) {
  const toggle = find(render(), "More graph controls");
  if (!(toggle.props.accessibilityState as { expanded?: boolean }).expanded) emit(toggle, "onPress");
}
function openDetails(render: () => ReactNode) {
  const toggle = find(render(), "Source details");
  if (!(toggle.props.accessibilityState as { expanded?: boolean }).expanded) emit(toggle, "onPress");
}

function canvasRefs(tree: ReactNode) {
  const scrollTo = vi.fn();
  for (const element of walk(tree)) {
    const ref = element.props.ref as { current: unknown } | undefined;
    if (ref) ref.current = { scrollTo };
  }
  return scrollTo;
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
    expect(content(tree)).not.toContain("Host host-a");
    expect(content(tree)).not.toContain("c8d1abadfe73");
    openDetails(render);
    expect(content(find(render(), "plan-graph-source-details"))).toContain("Host host-a");
    expect(content(find(render(), "plan-graph-source-details"))).toContain("c8d1abadfe73");
    expect(content(tree)).not.toMatch(/ready to start|frontier|serializes.with|governance|planned.start|capacity|In Development/i);
    expect(walk(tree).filter((node) => node.props.accessibilityRole === "button").every((node) => Number(style(node).minHeight) >= 44)).toBe(true);
  });

  it("prioritizes hover over focus over selection; temporary exit restores intentional selection", () => {
    const { render, props, key } = setup();
    openMore(render);
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
    expect(details).toContain("Owner: Test Owner");
    expect(details).toContain("Lift condition: Test Owner approves");
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
    openMore(render);
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
    openMore(render);
    expect(find(render(), "plan-graph-canvas")).toBeDefined();
    expect(Number(style(find(render(), "plan-node-TASK-002")).minHeight)).toBe(320);
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(content(render())).toContain("TASK-002 → TASK-004");
    expect(props.store.read(key).selected).toBe("TASK-002");
    emit(find(render(), "plan-node-TASK-003"), "onPress");
    expect(props.store.read(key).selected).toBe("TASK-003");
    expect(content(render())).toContain("Wave list is unscaled");
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
    openMore(render);
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
    openMore(render);
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
    openMore(render);
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
    openMore(render);
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
    expect(find(render(), "plan-node-TASK-099").props["aria-pressed"]).toBe(true);
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
    openMore(render);
    emit(find(render(), "plan-graph-canvas-scroll"), "onLayout", { nativeEvent: { layout: { width: 400, height: 500 } } });
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
    openMore(render);
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
    openMore(render);
    const scrollTo = vi.fn();
    for (const element of walk(render())) {
      const ref = element.props.ref as { current: unknown } | undefined;
      if (ref) ref.current = { scrollTo };
    }
    emit(find(render(), "plan-node-TASK-005"), "onPress");
    emit(find(render(), "Centre selected task"), "onPress");
    const envelope = style(find(render(), "plan-graph-scroll-envelope"));
    emit(find(render(), "plan-graph-canvas-scroll"), "onContentSizeChange", 800, envelope.height);
    emit(walk(render()).find((element) => element.props.horizontal)!, "onContentSizeChange", envelope.width, envelope.height);
    expect(scrollTo).toHaveBeenCalledTimes(2);
    expect(props.store.read(key).x).toBeGreaterThan(0);
    const saved = props.store.read(key);
    hooks.reset();
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-005");
    expect(props.store.read(key)).toEqual(saved);
  });

  it("zooms about the viewport centre in quarter steps with scaled scroll envelopes and unscaled controls", () => {
    const { render, props, key } = setup({ compact: true });
    props.store.update(key, { x: 200, y: 1000 });
    emit(find(render(), "Zoom in"), "onPress");
    expect(props.store.read(key)).toMatchObject({ scale: 1.25, x: 350, y: 1312.5 });
    expect(style(find(render(), "plan-graph-canvas"))).toMatchObject({ transformOrigin: "left top", transform: [{ scale: 1.25 }] });
    expect(style(find(render(), "plan-graph-scroll-envelope"))).toMatchObject({ width: 1500, height: 3740 });
    expect(content(find(render(), "plan-graph-zoom-status"))).toContain("125%");
    expect(style(find(render(), "Zoom in"))).toMatchObject({ minWidth: 44, minHeight: 44 });
    expect(style(find(render(), "Zoom in")).transform).toBeUndefined();
    emit(find(render(), "Zoom out"), "onPress");
    expect(props.store.read(key)).toMatchObject({ scale: 1, x: 200, y: 1000 });
    for (let index = 0; index < 8; index += 1) emit(find(render(), "Zoom in"), "onPress");
    expect(props.store.read(key).scale).toBe(2);
    expect(find(render(), "Zoom in").props.disabled).toBe(true);
    for (let index = 0; index < 10; index += 1) emit(find(render(), "Zoom out"), "onPress");
    expect(props.store.read(key).scale).toBe(0.5);
    expect(find(render(), "Zoom out").props.disabled).toBe(true);
    expect(Number(style(find(render(), "plan-node-TASK-001")).minHeight) * 0.5).toBeGreaterThanOrEqual(44);
  });

  it("waits for both new content extents and rejects stale scroll/size callbacks across zoom and reset", () => {
    const { render, props, key } = setup();
    openMore(render);
    props.store.update(key, { x: 200, y: 1000 });
    const oldVertical = find(render(), "plan-graph-canvas-scroll");
    const oldHorizontal = walk(render()).find((element) => element.props.horizontal)!;
    emit(find(render(), "Zoom in"), "onPress");
    const tree = render(); const scrollTo = canvasRefs(tree);
    emit(oldVertical, "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    emit(oldHorizontal, "onScroll", { nativeEvent: { contentOffset: { x: 0 } } });
    emit(find(tree, "plan-graph-canvas-scroll"), "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    expect(props.store.read(key)).toMatchObject({ x: 350, y: 1312.5 });
    emit(find(tree, "plan-graph-canvas-scroll"), "onContentSizeChange", 800, 2992);
    emit(oldHorizontal, "onContentSizeChange", 1500, 3740);
    expect(scrollTo).not.toHaveBeenCalled();
    emit(find(tree, "plan-graph-canvas-scroll"), "onContentSizeChange", 800, 3740);
    expect(scrollTo).not.toHaveBeenCalled();
    emit(walk(tree).find((element) => element.props.horizontal)!, "onContentSizeChange", 1500, 3740);
    expect(scrollTo.mock.calls).toEqual([[{ x: 350, animated: false }], [{ y: 1312.5, animated: false }]]);
    emit(find(render(), "Reset zoom to 100%"), "onPress"); render();
    emit(oldVertical, "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    expect(props.store.read(key)).toMatchObject({ scale: 1, x: 200, y: 1000 });
  });

  it("retires callbacks even when a viewport resize returns to identical geometry", () => {
    const { render, props, key } = setup();
    props.store.update(key, { y: 1000 });
    const retired = find(render(), "plan-graph-canvas-scroll");
    emit(retired, "onLayout", { nativeEvent: { layout: { width: 700, height: 500 } } });
    emit(find(render(), "plan-graph-canvas-scroll"), "onLayout", { nativeEvent: { layout: { width: 800, height: 500 } } });
    render();
    emit(retired, "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    expect(props.store.read(key).y).toBe(1000);
  });

  it("scopes zoom to host/revision and retires unmounted scroll handlers", () => {
    const { render, props, key } = setup();
    props.store.update(key, { x: 200, y: 1000 });
    emit(find(render(), "Zoom in"), "onPress");
    const retired = find(render(), "plan-graph-canvas-scroll");
    const saved = props.store.read(key);
    props.hostId = "host-b"; hooks.reset();
    expect(find(render(), "plan-graph-zoom-status").props.accessibilityLabel).toContain("100%");
    expect(content(find(render(), "plan-graph-zoom-status"))).toBe("");
    emit(retired, "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    expect(props.store.read(key)).toEqual(saved);
    props.hostId = "host-a"; props.graph = { ...props.graph, graph_sha256: "f".repeat(64) }; hooks.reset();
    expect(find(render(), "plan-graph-zoom-status").props.accessibilityLabel).toContain("100%");
    expect(props.store.read(key).scale).toBe(1.25);
  });

  it("resets around intentional selection, preserving temporary interaction, sourced details and canonical actions", () => {
    const openJira = vi.fn();
    const { render, props, key } = setup({ sourceActions: { jira: openJira } });
    openMore(render);
    const canonical = JSON.stringify(props.graph);
    props.sourcedStates = { viewKey: key, nodes: { "TASK-005": { label: "Captured state", source: "Independent read", observedAt: "2026-10-19T10:00:00Z" } } };
    emit(find(render(), "plan-node-TASK-005"), "onPress");
    emit(find(render(), "plan-node-TASK-003"), "onFocus");
    emit(find(render(), "plan-node-TASK-002"), "onHoverIn");
    emit(find(render(), "Zoom in"), "onPress");
    emit(find(render(), "Reset zoom to 100%"), "onPress");
    expect(props.store.read(key)).toMatchObject({ scale: 1, x: 400, y: 0, selected: "TASK-005" });
    expect(content(find(render(), "plan-graph-details"))).toContain("TASK-002");
    emit(find(render(), "plan-node-TASK-002"), "onHoverOut");
    emit(find(render(), "plan-node-TASK-003"), "onBlur");
    expect(content(find(render(), "plan-graph-details"))).toContain("Captured state · Independent read");
    emit(find(render(), "Open ASMA-8376"), "onPress");
    expect(openJira).toHaveBeenCalledWith({ kind: "jira", jira: "ASMA-8376" });
    expect(JSON.stringify(props.graph)).toBe(canonical);
  });

  it("applies true Fit graph separately from the unscaled wave-list alternate", () => {
    const { render, props, key } = setup({ graph: rootGraph(1) });
    openMore(render);
    emit(find(render(), "plan-graph-canvas-scroll"), "onLayout", { nativeEvent: { layout: { width: 225, height: 174 } } });
    emit(find(render(), "Fit graph"), "onPress");
    expect(props.store.read(key)).toMatchObject({ scale: 0.75, x: 0, y: 0, fit: true, alternate: false });
    expect(content(find(render(), "plan-graph-zoom-status"))).toContain("Full graph fits");
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(props.store.read(key).scale).toBe(0.75);
    expect(find(render(), "Zoom in").props.disabled).toBe(true);
    expect(find(render(), "Fit graph").props.disabled).toBe(true);
    expect(style(find(render(), "plan-node-TASK-000")).transform).toBeUndefined();
    expect(content(render())).toContain("Wave list is unscaled");
  });

  it("reports a constrained 50% fit, leaves scroll available and recomputes fit after viewport and intrinsic reflow", () => {
    const { render, props, key } = setup();
    emit(find(render(), "Fit graph"), "onPress");
    expect(props.store.read(key)).toMatchObject({ scale: 0.5, fit: true });
    expect(content(find(render(), "plan-graph-zoom-status"))).toContain("Constrained fit at 50%");
    expect(style(find(render(), "plan-graph-scroll-envelope")).height).toBe(1496);
    emit(find(render(), "plan-graph-canvas-scroll"), "onLayout", { nativeEvent: { layout: { width: 1200, height: 2992 } } });
    render();
    expect(props.store.read(key).scale).toBe(1);
    expect(content(find(render(), "plan-graph-zoom-status"))).toContain("Full graph fits");
    emit(find(render(), "plan-node-TASK-001"), "onLayout", { nativeEvent: { layout: { height: 400 } } });
    render();
    expect(props.store.read(key).scale).toBe(0.5);
    expect(content(find(render(), "plan-graph-zoom-status"))).toContain("Constrained fit");
    emit(find(render(), "plan-graph-canvas-scroll"), "onScrollBeginDrag");
    emit(find(render(), "plan-graph-canvas-scroll"), "onScroll", { nativeEvent: { contentOffset: { y: 200 } } });
    expect(props.store.read(key)).toMatchObject({ y: 200, fit: false });
  });

  it("preserves canvas camera and list scrolling independently through paging and open/return", () => {
    const { render, props, key } = setup({ graph: rootGraph(100) });
    openMore(render);
    props.store.update(key, { y: 10000 });
    emit(find(render(), "Zoom out"), "onPress");
    const camera = { scale: props.store.read(key).scale, x: props.store.read(key).x, y: props.store.read(key).y };
    emit(find(render(), "Fit as wave list"), "onPress");
    emit(find(render(), "Next tasks page"), "onPress");
    emit(find(render(), "plan-graph-list-scroll"), "onScroll", { nativeEvent: { contentOffset: { y: 450 } } });
    emit(find(render(), "Show graph canvas"), "onPress");
    expect(props.store.read(key)).toMatchObject({ ...camera, listY: 450, page: 1 });
    hooks.reset();
    expect(style(find(render(), "plan-graph-canvas")).transform).toEqual([{ scale: 0.75 }]);
    expect(props.store.read(key)).toMatchObject(camera);
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(find(render(), "plan-graph-list-scroll").props.contentOffset).toEqual({ x: 0, y: 450 });
  });

  it("keeps minimum-size controls, scaled hit areas and unscaled text measurement/tap guards in compact mode", () => {
    const { render, props, key } = setup({ compact: true, textScale: 2 });
    emit(find(render(), "Zoom out"), "onPress"); emit(find(render(), "Zoom out"), "onPress");
    emit(find(render(), "plan-node-TASK-001"), "onLayout", { nativeEvent: { layout: { height: 400 } } });
    expect(style(find(render(), "plan-node-TASK-001")).minHeight).toBe(400);
    expect(style(find(render(), "plan-graph-canvas")).transform).toEqual([{ scale: 0.5 }]);
    expect(walk(render()).filter((element) => element.props.accessibilityRole === "button").every((element) => Number(style(element).minHeight) >= 44)).toBe(true);
    emit(find(render(), "plan-node-TASK-001"), "onPressIn", { nativeEvent: { pageX: 10, pageY: 10 } });
    emit(find(render(), "plan-node-TASK-001"), "onTouchMove", { nativeEvent: { pageX: 20, pageY: 10 } });
    emit(find(render(), "plan-node-TASK-001"), "onPress");
    expect(props.store.read(key).selected).toBeNull();
  });

  it("re-centres the reset selection after intrinsic expansion and ignores retired bounds callbacks", () => {
    const { render, props, key } = setup();
    openMore(render);
    emit(find(render(), "plan-node-TASK-022"), "onPress");
    emit(find(render(), "Reset zoom to 100%"), "onPress");
    expect(props.store.read(key).y).toBe(2492);
    const old = find(render(), "plan-graph-canvas-scroll");
    emit(find(render(), "plan-node-TASK-022"), "onLayout", { nativeEvent: { layout: { height: 400 } } }); render();
    emit(old, "onScroll", { nativeEvent: { contentOffset: { y: 0 } } });
    expect(props.store.read(key).y).toBe(6332);
    const tree = render(); const scrollTo = canvasRefs(tree); settleCanvas(tree);
    expect(scrollTo).toHaveBeenLastCalledWith({ y: 6332, animated: false });
  });

  it.each([249, 250, 251])("preserves the large-graph policy at %s nodes", (count) => {
    const { render } = setup({ graph: rootGraph(count) });
    expect(find(render(), "Zoom in").props.disabled).toBe(count > 250);
    const cards = walk(render()).filter((element) => String(element.props.testID).startsWith("plan-node-"));
    expect(cards).toHaveLength(count > 250 ? 40 : count);
  });

  it("separates host and revision state explicitly, retaining each prior view", () => {
    const { render, props, key } = setup();
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    props.hostId = "host-b"; hooks.reset();
    expect(content(render())).not.toContain("Host host-b");
    openDetails(render);
    expect(content(render())).toContain("Host host-b");
    expect(props.store.read(planGraphViewKey(props.hostId, props.graph)).selected).toBeNull();
    props.hostId = "host-a"; props.graph = { ...props.graph, source_sha256: "f".repeat(64) }; hooks.reset();
    openDetails(render);
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

describe("graph density and disclosure semantics (GRAPH-01, A11Y-01)", () => {
  beforeEach(() => hooks.reset());
  const isButton = (node: Element) => node.props.accessibilityRole === "button";
  const labelOf = (node: Element) => node.props.accessibilityLabel;
  const SECONDARY = ["Clear graph highlight", "Previous task", "Next task", "Centre selected task", "Reset zoom to 100%", "Fit as wave list"];

  it("shows one graph title with only zoom out, Fit and zoom in grouped beside it, plus More and Source details", () => {
    const { render } = setup();
    const tree = render();
    expect(walk(tree).filter((node) => node.props.accessibilityRole === "header").map(content)).toEqual(["Plan dependencies"]);
    const group = find(tree, "Graph zoom");
    expect(group.props.role).toBe("group");
    expect(walk(group).filter(isButton).map(labelOf)).toEqual(["Zoom out", "Fit graph", "Zoom in"]);
    expect(walk(tree).filter((node) => isButton(node) && !String(node.props.testID).startsWith("plan-node-")).map(labelOf))
      .toEqual(["Zoom out", "Fit graph", "Zoom in", "More graph controls", "Source details"]);
  });

  it("hides secondary selection and wave controls until More is expanded and retains that disclosure with the view", () => {
    const { render, props, key } = setup();
    const visible = () => SECONDARY.filter((label) => walk(render()).some((node) => labelOf(node) === label));
    expect(visible()).toEqual([]);
    expect(find(render(), "More graph controls").props.accessibilityState).toEqual({ disabled: false, expanded: false });
    expect(find(render(), "More graph controls").props["aria-expanded"]).toBe(false);
    emit(find(render(), "More graph controls"), "onPress");
    expect(find(render(), "More graph controls").props.accessibilityState).toEqual({ disabled: false, expanded: true });
    expect(find(render(), "More graph controls").props["aria-expanded"]).toBe(true);
    expect(walk(find(render(), "plan-graph-more-controls")).filter(isButton).map(labelOf)).toEqual(SECONDARY);
    hooks.reset();
    expect(visible()).toEqual(SECONDARY);
    expect(props.store.read(key).controls).toBe(true);
    emit(find(render(), "More graph controls"), "onPress");
    expect(visible()).toEqual([]);
  });

  it("offers Copy JSON under More only when the shell wires copying, and copies the canonical graph", () => {
    const onCopy = vi.fn();
    const { render, props } = setup({ onCopy });
    emit(find(render(), "More graph controls"), "onPress");
    expect(walk(find(render(), "plan-graph-more-controls")).filter(isButton).map(labelOf)).toEqual([...SECONDARY, "Copy the canonical graph JSON"]);
    emit(find(render(), "Copy the canonical graph JSON"), "onPress");
    expect(onCopy).toHaveBeenCalledExactlyOnceWith(JSON.stringify(props.graph, null, 2), "Graph JSON");
    expect(JSON.parse(onCopy.mock.calls[0]![0] as string)).toEqual(props.graph);
  });

  it("keeps host, hashes, instructions and shell provenance inside one Source details disclosure", () => {
    const { render, props } = setup({ details: createElement("Text", null, "Shell provenance read 2026-10-19T00:00:00Z") });
    const hidden = ["Host host-a", props.graph.source_sha256, props.graph.graph_sha256, "Scroll to pan", "Shell provenance", "Status requires a separate source observation"];
    const closed = content(render());
    expect(hidden.filter((text) => closed.includes(text))).toEqual([]);
    const toggles = walk(render()).filter((node) => labelOf(node) === "Source details");
    expect(toggles).toHaveLength(1);
    expect(toggles[0]!.props.accessibilityState).toEqual({ disabled: false, expanded: false });
    emit(toggles[0]!, "onPress");
    expect(find(render(), "Source details").props.accessibilityState).toEqual({ disabled: false, expanded: true });
    const panel = find(render(), "plan-graph-source-details");
    expect(hidden.filter((text) => !content(panel).includes(text))).toEqual([]);
    expect(style(panel)).toMatchObject({ maxHeight: 260, flexGrow: 0 });
    expect(content(panel)).not.toMatch(/pinch|gesture/i);
  });

  it("exposes expanded state on every disclosure trigger and on no plain control", () => {
    const { render } = setup();
    openMore(render); openDetails(render);
    const disclosures = walk(render()).filter((node) => isButton(node) && "expanded" in (node.props.accessibilityState as object));
    expect(disclosures.map(labelOf)).toEqual(["More graph controls", "Source details"]);
    expect(disclosures.every((node) => (node.props.accessibilityState as { expanded: boolean }).expanded)).toBe(true);
  });

  it("gives the viewport the remaining height and bounds the inspector only while a task is active", () => {
    const { render } = setup({ compact: true });
    expect(style(find(render(), "plan-graph-canvas-scroll")).flex).toBe(1);
    expect(walk(render()).some((node) => labelOf(node) === "Task details")).toBe(false);
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    const inspector = find(render(), "Task details");
    // Shrinkable down to zero: the inspector yields before the canvas floor does.
    expect(style(inspector)).toMatchObject({ maxHeight: 200, flexGrow: 0, flexShrink: 1, minHeight: 0 });
    expect(content(inspector)).toContain("External fences:");
    expect(content(inspector)).not.toContain(sample().source_sha256);
  });

  it("keeps wide text outside Source details to the counts, with the epic, depth note and hint inside it", () => {
    const { render } = setup();
    const moved = ["No task is selected automatically", "depth, not readiness", "ASMA-8371", "Canvas zoom"];
    const outside = () => content(walkOutside(render(), (node) => node.props.testID === "plan-graph-source-details").filter((node) => node.type === "Text").map((node) => node.props.children));
    expect(outside()).toContain("19 tasks · 4 dependencies");
    expect(moved.filter((text) => outside().includes(text))).toEqual([]);
    const counts = walk(render()).find((node) => node.type === "Text" && node.props.children === "19 tasks · 4 dependencies")!;
    expect(counts.props.accessibilityLabel).toBe("ASMA-8371 · 19 tasks · 4 dependencies · depth, not readiness");
    openDetails(render);
    expect(moved.slice(0, 3).filter((text) => !content(find(render(), "plan-graph-source-details")).includes(text))).toEqual([]);
  });

  it("shows no zoom status for the untouched camera, then shows and announces every camera change, including back to 100%", () => {
    const { render } = setup();
    const status = () => find(render(), "plan-graph-zoom-status");
    expect(content(status())).toBe("");
    expect(status().props.accessibilityLabel).toBe("Canvas zoom 100%");
    expect(status().props.accessibilityLiveRegion).toBe("polite");
    emit(find(render(), "Zoom in"), "onPress");
    expect(content(status())).toBe("Canvas zoom 125%");
    emit(find(render(), "Zoom out"), "onPress");
    expect(content(status())).toBe("Canvas zoom 100%");
  });
});

/** Elements outside every excluded subtree; an excluded element is skipped with its descendants. */
function walkOutside(node: unknown, exclude: (element: Element) => boolean): Element[] {
  if (Array.isArray(node)) return node.flatMap((child) => walkOutside(child, exclude));
  if (!node || typeof node !== "object" || !("props" in node)) return [];
  const element = node as Element;
  return exclude(element) ? [] : [element, ...walkOutside(element.props.children, exclude)];
}
/** Root-first ancestors of the first matching element, or null when it is absent. */
function ancestorsOf(node: unknown, match: (element: Element) => boolean): Element[] | null {
  if (Array.isArray(node)) {
    for (const child of node) { const path = ancestorsOf(child, match); if (path) return path; }
    return null;
  }
  if (!node || typeof node !== "object" || !("props" in node)) return null;
  const element = node as Element;
  if (match(element)) return [];
  const path = ancestorsOf(element.props.children, match);
  return path ? [element, ...path] : null;
}

describe("compact graph-first layout (iPhone 115a regression)", () => {
  beforeEach(() => hooks.reset());
  const isViewport = (element: Element) => element.props.testID === "plan-graph-canvas-scroll" || element.props.testID === "plan-graph-list-scroll";
  const layoutRoot = (render: () => ReactNode, height: number) => { emit(find(render(), "plan-graph"), "onLayout", { nativeEvent: { layout: { width: 390, height } } }); };

  it.each([false, true])("mounts the canvas and the wave list as direct children of the bounded flex:1 root, never inside a ScrollView or FlatList (compact %s)", (compact) => {
    const { render } = setup({ compact });
    openMore(render); openDetails(render);
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    for (const mode of ["canvas", "list"]) {
      const tree = render();
      const path = ancestorsOf(tree, isViewport)!;
      expect(path).not.toBeNull();
      expect(path.map((element) => element.type)).toEqual(["View"]);
      expect(path[0]!.props.testID).toBe("plan-graph");
      expect(style(path[0]!)).toMatchObject({ flex: 1 });
      expect(find(tree, mode === "canvas" ? "plan-graph-canvas-scroll" : "plan-graph-list-scroll")).toBeDefined();
      if (mode === "canvas") emit(find(render(), "Fit as wave list"), "onPress");
    }
  });

  it("gives the compact canvas an explicit floor of max(320, 55% of the measured graph page)", () => {
    const { render } = setup({ compact: true });
    const floor = () => style(walk(render()).find(isViewport)!).minHeight;
    expect(floor()).toBe(320);
    // 390x844 iPhone: about 664px remain under the 51px context row and above the bottom tabs.
    layoutRoot(render, 664);
    expect(floor()).toBe(365);
    expect(Number(floor()) / 664).toBeGreaterThan(0.5);
    // Opening every panel keeps the floor; the panels are shrinkable and yield instead.
    openMore(render); openDetails(render);
    emit(find(render(), "plan-node-TASK-002"), "onPress");
    expect(floor()).toBe(365);
    for (const id of ["Task details", "plan-graph-source-details"]) expect(style(find(render(), id))).toMatchObject({ flexShrink: 1, minHeight: 0 });
    emit(find(render(), "Fit as wave list"), "onPress");
    expect(floor()).toBe(365);
    layoutRoot(render, Number.NaN);
    expect(floor()).toBe(365);
    layoutRoot(render, Number.POSITIVE_INFINITY);
    expect(floor()).toBe(320);
    layoutRoot(render, 800);
    expect(floor()).toBe(440);
    layoutRoot(render, 300);
    expect(floor()).toBe(320);
  });

  it("keeps a smaller explicit canvas floor on wide layouts", () => {
    const { render } = setup();
    expect(style(find(render(), "plan-graph-canvas-scroll")).minHeight).toBe(240);
    layoutRoot(render, 1000);
    expect(style(find(render(), "plan-graph-canvas-scroll")).minHeight).toBe(550);
  });

  it("renders one compact title row: the title, zoom out, Fit, zoom in and More, and no other text", () => {
    const { render } = setup({ compact: true });
    const row = find(render(), "plan-graph-title-row");
    expect(style(row)).toMatchObject({ flexDirection: "row", flexWrap: "nowrap" });
    expect(walk(row).filter((node) => node.props.accessibilityRole === "button").map((node) => node.props.accessibilityLabel))
      .toEqual(["Zoom out", "Fit graph", "Zoom in", "More graph controls"]);
    const texts = walkOutside(row, (node) => node.props.accessibilityRole === "button").filter((node) => node.type === "Text");
    expect(texts.map((node) => [node.props.accessibilityRole, node.props.numberOfLines, content(node)])).toEqual([["header", 1, "Plan dependencies"]]);
  });

  it("renders no instruction, hash, host, summary or read-time text outside Source details on compact", () => {
    const graph = sample();
    const observedAt = "2026-10-18T17:00+00:00";
    const { render, props } = setup({
      graph, compact: true,
      details: createElement("Text", null, "Shell provenance read 2026-10-19T00:00:00Z"),
      sourcedStates: { viewKey: planGraphViewKey("host-a", graph), nodes: { "TASK-001": { label: "Closed (historical)", source: "Historical Jira", observedAt } } },
    });
    const hidden = ["Host host-a", graph.source_sha256.slice(0, 12), graph.graph_sha256.slice(0, 12), "Scroll to pan", "Keyboard:",
      "No task is selected automatically", "depth, not readiness", "Status requires a separate source observation", "Sourced states",
      observedAt, "Shell provenance", "2026-10-19T00:00:00Z", graph.source];
    const inPanel = (element: Element) => element.props.testID === "plan-graph-source-details";
    const outside = () => content(walkOutside(render(), inPanel).filter((node) => node.type === "Text").map((node) => node.props.children));
    expect(hidden.filter((text) => outside().includes(text))).toEqual([]);
    openDetails(render);
    expect(hidden.filter((text) => outside().includes(text))).toEqual([]);
    expect(hidden.filter((text) => !content(find(render(), "plan-graph-source-details")).includes(text))).toEqual([]);
    expect(props.store.read(planGraphViewKey("host-a", graph)).details).toBe(true);
  });

  it("keeps compact text outside the canvas to the title, button captions and a one-line zoom status", () => {
    const { render } = setup({ compact: true });
    const chrome = (tree: ReactNode) => walkOutside(tree, (node) => isViewport(node) || node.props.testID === "plan-graph-source-details"
      || node.props.accessibilityRole === "button").filter((node) => node.type === "Text");
    const zoom = find(render(), "plan-graph-zoom-status");
    expect(zoom.props.numberOfLines).toBe(1);
    expect(chrome(render()).map((node) => node.props.testID ?? node.props.accessibilityRole)).toEqual(["header", "plan-graph-zoom-status"]);
    openDetails(render);
    expect(chrome(render()).map((node) => node.props.testID ?? node.props.accessibilityRole)).toEqual(["header", "plan-graph-zoom-status"]);
  });

  it("moves the large-graph note into Source details on compact", () => {
    const { render } = setup({ graph: rootGraph(251), compact: true });
    const note = "Large graph: paged wave list";
    expect(content(render())).not.toContain(note);
    openDetails(render);
    expect(content(walkOutside(render(), (node) => node.props.testID === "plan-graph-source-details").filter((node) => node.type === "Text").map((node) => node.props.children))).not.toContain(note);
    expect(content(find(render(), "plan-graph-source-details"))).toContain(note);
    hooks.reset();
    const wide = setup({ graph: rootGraph(251) });
    expect(content(wide.render())).toContain(note);
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
