import type { PluginTheme } from "@getpaseo/plugin";
import { ScrollView } from "@getpaseo/plugin/client/react-native";
import { useMemo, useRef, useState, useSyncExternalStore, type ComponentRef, type ReactNode } from "react";
import { Pressable, Text, View } from "react-native";
import { planSourceActions, type PlanGraphData, type PlanSourceAction, type SourcedPlanState } from "./contract";
import { edgeEmphasis, edgeSegments, layoutPlanGraph, MIN_TOUCH_TARGET, nodeEmphasis, tracePlanGraph } from "./layout";

export type PlanGraphViewState = {
  selected: string | null;
  alternate: boolean;
  page: number;
  /** Canvas pan is independent of the paged alternate's scroll position. */
  x: number;
  y: number;
  listY: number;
};
const INITIAL_VIEW: PlanGraphViewState = { selected: null, alternate: false, page: 0, x: 0, y: 0, listY: 0 };
export const LARGE_GRAPH_THRESHOLD = 250;
export const WAVE_LIST_PAGE_SIZE = 40;

export function planGraphViewKey(hostId: string, graph: PlanGraphData): string {
  return JSON.stringify([hostId, graph.source, graph.source_sha256, graph.graph_sha256]);
}

/** Retain one store in the shell for open/return. No global host or revision cache. */
export function createPlanGraphStore() {
  const views = new Map<string, PlanGraphViewState>();
  const listeners = new Set<() => void>();
  return {
    read(key: string): PlanGraphViewState { return views.get(key) ?? INITIAL_VIEW; },
    update(key: string, patch: Partial<PlanGraphViewState>) {
      views.set(key, { ...(views.get(key) ?? INITIAL_VIEW), ...patch });
      for (const listener of listeners) listener();
    },
    subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    clear() { views.clear(); for (const listener of listeners) listener(); },
  };
}
export type PlanGraphStore = ReturnType<typeof createPlanGraphStore>;

export type PlanGraphInteraction = { hovered: string | null; focused: string | null; selected: string | null };
export function activePlanNode(interaction: PlanGraphInteraction): string | null {
  return interaction.hovered ?? interaction.focused ?? interaction.selected;
}

/** A cancelled or >8 logical-pixel gesture never selects a card. */
export function createPlanTapGuard() {
  let origin: { x: number; y: number } | null = null;
  let cancelled = false;
  return {
    start(x: number, y: number) { origin = { x, y }; cancelled = false; },
    move(x: number, y: number) {
      if (origin && Math.hypot(x - origin.x, y - origin.y) > 8) cancelled = true;
    },
    cancel() { cancelled = true; },
    canPress() { return !cancelled; },
    keyboardFocus() { origin = null; cancelled = false; },
  };
}

export type PlanGraphProps = {
  /** Already schema-validated, exact-source export; never pass a derived board graph. */
  graph: PlanGraphData;
  hostId: string;
  theme: PluginTheme;
  store: PlanGraphStore;
  compact?: boolean;
  /** Host's font scale. Intrinsic card measurements also expand all rows as needed. */
  textScale?: number;
  /** Same host/revision only. Missing observations remain explicitly unknown. */
  sourcedStates?: { viewKey: string; nodes: Readonly<Record<string, SourcedPlanState>> };
  /** Only callbacks actually supplied by the shell become visible source actions. */
  sourceActions?: Partial<Record<PlanSourceAction["kind"], (action: PlanSourceAction) => void>>;
};

/** Native component; the shell owns fetching, source validation, routing, and store lifetime. */
export function PlanGraph(props: PlanGraphProps) {
  return <PlanGraphView key={planGraphViewKey(props.hostId, props.graph)} {...props} />;
}

function PlanGraphView({ graph, hostId, theme, store, compact = false, textScale = 1, sourcedStates, sourceActions }: PlanGraphProps) {
  const key = planGraphViewKey(hostId, graph);
  const view = useSyncExternalStore(store.subscribe, () => store.read(key));
  const [hovered, setHovered] = useState<string | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const [measuredHeight, setMeasuredHeight] = useState(160);
  const [viewport, setViewport] = useState({ width: 800, height: 500 });
  const horizontal = useRef<ComponentRef<typeof ScrollView>>(null);
  const vertical = useRef<ComponentRef<typeof ScrollView>>(null);
  // Keep the Fit anchor through intrinsic reflow; explicit navigation releases it.
  const fittedListTask = useRef<{ id: string; y: number | null } | null>(null);
  const tapGuard = useRef(createPlanTapGuard()).current;
  const colors = theme.colors;
  const layout = useMemo(() => layoutPlanGraph(graph, Math.max(textScale, measuredHeight / 160)), [graph, textScale, measuredHeight]);
  const active = activePlanNode({ hovered, focused, selected: view.selected });
  const trace = useMemo(() => tracePlanGraph(graph, active), [graph, active]);
  const positions = useMemo(() => new Map(layout.nodes.map((item) => [item.node.id, item])), [layout]);
  const selectedNode = graph.nodes.find((node) => node.id === trace.active);
  const isLarge = graph.nodes.length > LARGE_GRAPH_THRESHOLD;
  const alternate = view.alternate || isLarge;
  const pageCount = Math.max(1, Math.ceil(layout.nodes.length / WAVE_LIST_PAGE_SIZE));
  const page = Math.min(view.page, pageCount - 1);
  const pageNodes = layout.nodes.slice(page * WAVE_LIST_PAGE_SIZE, (page + 1) * WAVE_LIST_PAGE_SIZE);
  const observation = (id: string) => {
    const value = sourcedStates?.viewKey === key ? sourcedStates.nodes[id] : undefined;
    if (!value?.label.trim() || !value.source.trim() || !Number.isFinite(Date.parse(value.observedAt))) return undefined;
    return value;
  };
  const label = (value: ReactNode, muted = false) => <Text style={{ color: muted ? colors.foregroundMuted : colors.foreground }}>{value}</Text>;
  const control = (title: string, onPress: () => void, disabled = false) => (
    <Pressable focusable accessibilityRole="button" accessibilityLabel={title} accessibilityState={{ disabled }} disabled={disabled}
      onPress={onPress} style={{ minWidth: MIN_TOUCH_TARGET, minHeight: MIN_TOUCH_TARGET, padding: 12,
        justifyContent: "center", borderWidth: 1, borderColor: colors.border, borderRadius: 8 }}>
      {label(title)}
    </Pressable>
  );
  function clear() {
    fittedListTask.current = null;
    setHovered(null); setFocused(null);
    store.update(key, { selected: null });
  }
  function selectTask(id: string) {
    fittedListTask.current = null;
    store.update(key, { selected: id });
  }
  function centre(id: string | null) {
    const target = id ? positions.get(id) : undefined;
    if (!target) return;
    const x = Math.max(0, target.x - (viewport.width - target.width) / 2);
    const y = Math.max(0, target.y - (viewport.height - target.height) / 2);
    store.update(key, { x, y });
    horizontal.current?.scrollTo({ x, animated: false });
    vertical.current?.scrollTo({ y, animated: false });
  }
  function navigate(step: number) {
    const index = layout.nodes.findIndex((item) => item.node.id === view.selected);
    const next = layout.nodes[Math.max(0, Math.min(layout.nodes.length - 1, index + step))];
    if (!next) return;
    setHovered(null); setFocused(null);
    const nextIndex = layout.nodes.indexOf(next);
    store.update(key, { selected: next.node.id, page: Math.floor(nextIndex / WAVE_LIST_PAGE_SIZE) });
    if (alternate) changePage(Math.floor(nextIndex / WAVE_LIST_PAGE_SIZE));
    else centre(next.node.id);
  }
  function changePage(nextPage: number) {
    fittedListTask.current = null;
    store.update(key, { page: nextPage, listY: 0 });
    vertical.current?.scrollTo({ y: 0, animated: false });
  }
  function toggleAlternate() {
    setHovered(null); setFocused(null);
    fittedListTask.current = null;
    if (alternate) {
      store.update(key, { alternate: false });
      return;
    }
    const index = layout.nodes.findIndex((item) => item.node.id === view.selected);
    if (index >= 0) {
      fittedListTask.current = { id: layout.nodes[index]!.node.id, y: null };
      store.update(key, { alternate: true, page: Math.floor(index / WAVE_LIST_PAGE_SIZE), listY: 0 });
    } else store.update(key, { alternate: true });
  }
  function isCurrentScrollView() {
    const current = store.read(key);
    return (current.alternate || isLarge) === alternate
      && (!alternate || Math.min(current.page, pageCount - 1) === page);
  }
  function edgeAppearance(edge: PlanGraphData["edges"][number]) {
    const emphasis = edgeEmphasis(trace, edge);
    return {
      color: emphasis === "full" || emphasis === "dim" ? colors.foregroundMuted : colors.accent,
      opacity: emphasis === "dim" ? 0.2 : 1,
    };
  }
  function drag() { fittedListTask.current = null; tapGuard.cancel(); setHovered(null); setFocused(null); }
  function card(item: typeof layout.nodes[number], inList: boolean) {
    const { node } = item;
    const emphasis = nodeEmphasis(trace, node.id);
    const state = observation(node.id);
    const highlighted = emphasis === "active";
    const foreground = highlighted ? colors.accentForeground : colors.foreground;
    return <Pressable focusable key={node.id} testID={`plan-node-${node.id}`} accessibilityRole="button"
      accessibilityLabel={`${node.id}: ${node.title}. ${state ? `${state.label}, source ${state.source}, observed ${state.observedAt}` : "Status not supplied"}. ${node.phase}. Wave ${node.wave}, dependency depth.`}
      accessibilityHint="Activate for direct prerequisites, dependents and source details"
      accessibilityState={{ selected: node.id === view.selected }}
      accessibilityActions={[{ name: "activate", label: "Select task" }, { name: "escape", label: "Clear graph highlight" }]}
      onAccessibilityAction={({ nativeEvent }) => {
        if (nativeEvent.actionName === "escape") clear();
        if (nativeEvent.actionName === "activate") selectTask(node.id);
      }}
      onHoverIn={() => setHovered(node.id)} onHoverOut={() => setHovered(null)}
      onFocus={() => { tapGuard.keyboardFocus(); setFocused(node.id); }} onBlur={() => setFocused(null)}
      onPressIn={({ nativeEvent }) => tapGuard.start(nativeEvent.pageX, nativeEvent.pageY)}
      onTouchMove={({ nativeEvent }) => tapGuard.move(nativeEvent.pageX, nativeEvent.pageY)}
      onTouchCancel={() => tapGuard.cancel()}
      onPress={() => { if (tapGuard.canPress()) selectTask(node.id); }}
      onLayout={({ nativeEvent }) => { if (nativeEvent.layout.height > measuredHeight + 1) setMeasuredHeight(Math.ceil(nativeEvent.layout.height)); }}
      style={{ ...(inList ? {} : { position: "absolute", left: item.x, top: item.y, width: item.width }),
        minHeight: item.height, padding: 12, gap: 8, borderRadius: 10, borderWidth: 2,
        borderColor: emphasis === "full" || emphasis === "dim" ? colors.border : colors.accent,
        backgroundColor: highlighted ? colors.accent : colors.surface1, opacity: emphasis === "dim" ? 0.35 : 1 }}>
      <Text style={{ color: foreground, fontWeight: "700" }}>{node.id} · {node.title}</Text>
      <Text style={{ color: foreground }}>{node.phase} · Depth {node.wave}</Text>
      <Text style={{ color: foreground }}>{state?.label ?? "Status not supplied"}</Text>
      {inList && <Text style={{ color: foreground }}>{`Prerequisites: ${node.depends_on.join(", ") || "None"}`}</Text>}
    </Pressable>;
  }
  const actions = selectedNode ? planSourceActions(graph, selectedNode).filter((action) => sourceActions?.[action.kind]) : [];
  return <View style={{ flex: 1, minHeight: 300, gap: compact ? 8 : 12, padding: compact ? 8 : 16, backgroundColor: colors.surface0 }}>
    {label(`Plan graph · ${graph.epic.jira ?? "Epic Jira not supplied"}`)}
    {label(`Host ${hostId} · source ${graph.source_sha256.slice(0, 12)} · graph ${graph.graph_sha256.slice(0, 12)}`, true)}
    {label("Waves show dependency depth. Status requires a separate source observation.", true)}
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
      {control("Clear graph highlight", clear)}
      {control("Previous task", () => navigate(-1), !graph.nodes.length)}
      {control("Next task", () => navigate(1), !graph.nodes.length)}
      {control("Centre selected task", () => centre(view.selected), !view.selected || alternate)}
      {control(alternate ? "Show graph canvas" : "Fit as wave list", toggleAlternate, isLarge)}
    </View>
    {label(isLarge ? "Large graph: paged wave list. Every canonical task and dependency remains available." : "Scroll to pan. Zoom is unavailable; use centre or Fit as wave list. Keyboard: Tab between controls, activate to select.", true)}
    <ScrollView key={alternate ? `list-${page}` : "canvas"} ref={vertical} nestedScrollEnabled
      testID={alternate ? "plan-graph-list-scroll" : "plan-graph-canvas-scroll"}
      contentOffset={{ x: 0, y: alternate ? view.listY : view.y }}
      style={{ flex: 1 }} onLayout={({ nativeEvent }) => setViewport(nativeEvent.layout)}
      onScrollBeginDrag={drag} scrollEventThrottle={100}
      onScroll={({ nativeEvent }) => {
        if (isCurrentScrollView()) store.update(key, alternate ? { listY: nativeEvent.contentOffset.y } : { y: nativeEvent.contentOffset.y });
      }}>
      {alternate ? <View style={{ gap: 12 }}>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
          {control("Previous tasks page", () => changePage(page - 1), page === 0)}
          {label(`Page ${page + 1} of ${pageCount}`)}
          {control("Next tasks page", () => changePage(page + 1), page + 1 === pageCount)}
        </View>
        {pageNodes.map((item) => <View key={item.node.id} testID={`plan-list-row-${item.node.id}`} style={{ gap: 8 }}
          onLayout={({ nativeEvent }) => {
            const fitted = fittedListTask.current;
            if (fitted?.id !== item.node.id || store.read(key).selected !== item.node.id || !isCurrentScrollView()) return;
            const y = nativeEvent.layout.y;
            if (fitted.y === y) return;
            fitted.y = y;
            store.update(key, { listY: y });
            vertical.current?.scrollTo({ y, animated: false });
          }}>
          {label(`Wave ${item.node.wave} · dependency depth`)}
          {card(item, true)}
          {graph.edges.filter((edge) => edge.from === item.node.id).map((edge) => {
            const { color, opacity } = edgeAppearance(edge);
            return <View key={`${edge.from}:${edge.to}`} testID={`plan-list-edge-${edge.from}-${edge.to}`} style={{ opacity }}>
              <Text style={{ color }}>{`${edge.from} → ${edge.to}`}</Text>
            </View>;
          })}
        </View>)}
      </View> : <ScrollView ref={horizontal} horizontal nestedScrollEnabled contentOffset={{ x: view.x, y: 0 }}
        onScrollBeginDrag={drag} scrollEventThrottle={100}
        onScroll={({ nativeEvent }) => { if (isCurrentScrollView()) store.update(key, { x: nativeEvent.contentOffset.x }); }}>
        <View testID="plan-graph-canvas" style={{ width: layout.width, height: layout.height }}>
          {layout.columns.map((column) => <View key={column.wave} style={{ position: "absolute", left: column.x, width: 220 }}>
            {label(`Wave ${column.wave} · depth`)}
          </View>)}
          {graph.edges.map((edge, index) => {
            const from = positions.get(edge.from)!;
            const to = positions.get(edge.to)!;
            const { color, opacity } = edgeAppearance(edge);
            return <View key={`${edge.from}:${edge.to}`} pointerEvents="none" accessible={false}
              testID={`plan-edge-${edge.from}-${edge.to}`} style={{ position: "absolute", opacity }}>
              {edgeSegments(from, to, index).map((segment, segmentIndex) => <View key={segmentIndex}
                style={{ position: "absolute", left: segment.x, top: segment.y, width: segment.width, height: segment.height, backgroundColor: color }} />)}
              <View style={{ position: "absolute", left: to.x - 8, top: to.y + to.height / 2 - 5,
                width: 0, height: 0, borderTopWidth: 6, borderBottomWidth: 6, borderLeftWidth: 8,
                borderTopColor: "transparent", borderBottomColor: "transparent", borderLeftColor: color }} />
            </View>;
          })}
          {layout.nodes.map((item) => card(item, false))}
        </View>
      </ScrollView>}
    </ScrollView>
    <ScrollView style={{ maxHeight: compact ? 220 : 320 }} accessibilityLabel="Task details">
      {selectedNode ? <View testID="plan-graph-details" style={{ gap: 8, padding: 12, backgroundColor: colors.surface1 }}>
        {label(`${selectedNode.id} · ${selectedNode.title}`)}
        {label(`Phase: ${selectedNode.phase}`)}
        {label(`Direct prerequisites: ${selectedNode.depends_on.join(", ") || "None"}`)}
        {label(`Direct dependents: ${graph.edges.filter((edge) => edge.from === selectedNode.id).map((edge) => edge.to).join(", ") || "None"}`)}
        {label(`Confirmed Jira: ${selectedNode.jira ?? "Not supplied"}`)}
        {label(observation(selectedNode.id) ? `Sourced state: ${observation(selectedNode.id)!.label} · ${observation(selectedNode.id)!.source} · observed ${observation(selectedNode.id)!.observedAt}` : "Sourced state: not supplied by this export")}
        {label(`Plan source: ${graph.source} · ${graph.source_sha256}`)}
        {label("External fences:")}
        {graph.external_fences.filter((fence) => fence.task === selectedNode.id).map((fence, index) => <View key={index}>
          {label(`${fence.prerequisite} · Owner: ${fence.owner} · Lift condition: ${fence.lift_condition}`)}
        </View>)}
        {!graph.external_fences.some((fence) => fence.task === selectedNode.id) && label("None recorded")}
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
          {actions.map((action) => <View key={action.kind}>{control(action.kind === "jira" ? `Open ${action.jira}` : "Open exact plan source", () => sourceActions?.[action.kind]?.(action))}</View>)}
        </View>
      </View> : label("Select, hover or focus a task for details. No task is selected automatically.", true)}
    </ScrollView>
  </View>;
}
