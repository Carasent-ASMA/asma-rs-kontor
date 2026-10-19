import type { PlanGraphData, PlanNode } from "./contract";

export const CARD_WIDTH = 220;
export const COLUMN_PITCH = 300;
export const ROW_GAP = 24;
export const HEADER_HEIGHT = 48;
export const MIN_TOUCH_TARGET = 44;

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
