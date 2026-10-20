import type { PlanGraphData, PlanNode } from "./contract";

export const CARD_WIDTH = 220;
export const COLUMN_PITCH = 300;
export const ROW_GAP = 24;
export const HEADER_HEIGHT = 48;
export const MIN_TOUCH_TARGET = 44;
export const MIN_GRAPH_SCALE = 0.5;
export const MAX_GRAPH_SCALE = 2;
export const GRAPH_SCALE_STEP = 0.25;
export const COMPACT_CANVAS_MIN_HEIGHT = 320;
export const CANVAS_MIN_HEIGHT = 240;
export const CANVAS_PAGE_SHARE = 0.55;

export type GraphSize = { width: number; height: number };
export type GraphPoint = { x: number; y: number };
export type PlanGraphCamera = GraphPoint & { scale: number };

const extent = (value: number) => Number.isFinite(value) ? Math.max(0, value) : 0;
export function clampGraphScale(scale: number): number {
  return Math.max(MIN_GRAPH_SCALE, Math.min(MAX_GRAPH_SCALE, Number.isFinite(scale) ? scale : 1));
}

/**
 * Explicit canvas floor. A content-sized native ancestor cannot collapse it, and it keeps the
 * majority of the measured graph page, so panels shrink before the canvas does.
 */
export function canvasMinHeight(compact: boolean, pageHeight: number): number {
  return Math.max(compact ? COMPACT_CANVAS_MIN_HEIGHT : CANVAS_MIN_HEIGHT, Math.round(extent(pageHeight) * CANVAS_PAGE_SHARE));
}

/** Transforms do not change native layout: this envelope owns the scroll extents. */
export function graphScrollEnvelope(bounds: GraphSize, viewport: GraphSize, scale: number): GraphSize {
  return {
    width: Math.max(extent(viewport.width), extent(bounds.width) * clampGraphScale(scale)),
    height: Math.max(extent(viewport.height), extent(bounds.height) * clampGraphScale(scale)),
  };
}

export function cameraAtGraphPoint(scale: number, point: GraphPoint, bounds: GraphSize, viewport: GraphSize): PlanGraphCamera {
  const actualScale = clampGraphScale(scale);
  const envelope = graphScrollEnvelope(bounds, viewport, actualScale);
  return {
    scale: actualScale,
    x: Math.max(0, Math.min(envelope.width - extent(viewport.width), actualScale * extent(point.x) - extent(viewport.width) / 2)),
    y: Math.max(0, Math.min(envelope.height - extent(viewport.height), actualScale * extent(point.y) - extent(viewport.height) / 2)),
  };
}

/** U = sG - O. Keep the viewport-centre logical point fixed, except at clamped boundaries. */
export function zoomPlanGraph(camera: PlanGraphCamera, scale: number, bounds: GraphSize, viewport: GraphSize): PlanGraphCamera {
  const previousScale = clampGraphScale(camera.scale);
  return cameraAtGraphPoint(scale, {
    x: (extent(camera.x) + extent(viewport.width) / 2) / previousScale,
    y: (extent(camera.y) + extent(viewport.height) / 2) / previousScale,
  }, bounds, viewport);
}

export function fitPlanGraph(camera: PlanGraphCamera, bounds: GraphSize, viewport: GraphSize): PlanGraphCamera & { constrained: boolean } {
  const measurable = [bounds.width, bounds.height, viewport.width, viewport.height].every((value) => Number.isFinite(value) && value > 0);
  const fit = measurable ? Math.min(MAX_GRAPH_SCALE, viewport.width / bounds.width, viewport.height / bounds.height) : 0;
  if (fit < MIN_GRAPH_SCALE) return { ...zoomPlanGraph(camera, MIN_GRAPH_SCALE, bounds, viewport), constrained: true };
  return { scale: fit, x: 0, y: 0, constrained: false };
}

const compareIds = (a: string, b: string) => a < b ? -1 : a > b ? 1 : 0;

export type PositionedPlanNode = { node: PlanNode; x: number; y: number; width: number; height: number };
export type PlanGraphLayout = {
  nodes: PositionedPlanNode[];
  columns: { wave: number; x: number }[];
  width: number;
  height: number;
};

/** Stable ID order within canonical depth columns; input array order has no visual meaning. */
export function layoutPlanGraph(graph: PlanGraphData, textScale = 1): PlanGraphLayout {
  const scale = Number.isFinite(textScale) ? Math.max(1, textScale) : 1;
  const height = Math.ceil(160 * scale);
  const layers = [...graph.waves].sort((a, b) => a.wave - b.wave);
  const columns = layers
    .map((layer, index) => ({ wave: layer.wave, x: index * COLUMN_PITCH }));
  const nodes: PositionedPlanNode[] = [];
  const byId = new Map(graph.nodes.map((node) => [node.id, node]));
  let maxRows = 0;
  for (const [index, column] of columns.entries()) {
    const layer = layers[index]!;
    maxRows = Math.max(maxRows, layer.tasks.length);
    for (const [row, id] of [...layer.tasks].sort(compareIds).entries()) {
      nodes.push({ node: byId.get(id)!, x: column.x, y: HEADER_HEIGHT + row * (height + ROW_GAP), width: CARD_WIDTH, height });
    }
  }
  return { nodes, columns, width: columns.length * COLUMN_PITCH, height: HEADER_HEIGHT + maxRows * (height + ROW_GAP) };
}

export type PlanTrace = { active: string | null; ancestors: Set<string>; descendants: Set<string> };

export function tracePlanGraph(graph: PlanGraphData, active: string | null): PlanTrace {
  const ancestors = new Set<string>();
  const descendants = new Set<string>();
  if (!active || !graph.nodes.some((node) => node.id === active)) return { active: null, ancestors, descendants };
  const upstream = new Map<string, string[]>();
  const downstream = new Map<string, string[]>();
  for (const edge of graph.edges) {
    const parents = upstream.get(edge.to) ?? [];
    parents.push(edge.from);
    upstream.set(edge.to, parents);
    const children = downstream.get(edge.from) ?? [];
    children.push(edge.to);
    downstream.set(edge.from, children);
  }
  const walk = (adjacency: Map<string, string[]>, result: Set<string>) => {
    const pending = [...(adjacency.get(active) ?? [])];
    while (pending.length) {
      const id = pending.pop()!;
      if (id === active || result.has(id)) continue;
      result.add(id);
      pending.push(...(adjacency.get(id) ?? []));
    }
  };
  walk(upstream, ancestors);
  walk(downstream, descendants);
  return { active, ancestors, descendants };
}

export function nodeEmphasis(trace: PlanTrace, id: string): "full" | "active" | "upstream" | "downstream" | "dim" {
  if (!trace.active) return "full";
  if (trace.active === id) return "active";
  if (trace.ancestors.has(id)) return "upstream";
  if (trace.descendants.has(id)) return "downstream";
  return "dim";
}

export function edgeEmphasis(trace: PlanTrace, edge: PlanGraphData["edges"][number]): "full" | "upstream" | "downstream" | "dim" {
  if (!trace.active) return "full";
  if (trace.ancestors.has(edge.from) && (trace.ancestors.has(edge.to) || edge.to === trace.active)) return "upstream";
  if ((edge.from === trace.active || trace.descendants.has(edge.from)) && trace.descendants.has(edge.to)) return "downstream";
  return "dim";
}

/** Orthogonal native View segments, with a rightward arrowhead at the dependent. */
export function edgeSegments(from: PositionedPlanNode, to: PositionedPlanNode, lane = 0) {
  const startX = from.x + from.width;
  const startY = from.y + from.height / 2;
  const endX = to.x;
  const endY = to.y + to.height / 2;
  // Long bypass edges run in the row gap rather than through intermediate cards.
  const bypass = endX - from.x > COLUMN_PITCH;
  const routeY = bypass ? from.y + from.height + ROW_GAP / 2 : endY;
  const elbowX = startX + 16 + lane % 5 * 5;
  const finishX = endX - 14;
  return [
    { x: startX, y: startY, width: elbowX - startX, height: 2 },
    { x: elbowX, y: Math.min(startY, routeY), width: 2, height: Math.max(2, Math.abs(routeY - startY)) },
    { x: elbowX, y: routeY, width: Math.max(2, finishX - elbowX), height: 2 },
    { x: finishX, y: Math.min(routeY, endY), width: 2, height: Math.max(2, Math.abs(endY - routeY)) },
    { x: finishX, y: endY, width: endX - finishX, height: 2 },
  ];
}
