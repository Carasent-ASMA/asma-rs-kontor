import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { describe, expect, it } from "vitest";
import { planGraphSchema, type PlanGraphData } from "../client/plan-graph/contract";
import { canvasMinHeight, CARD_WIDTH, COLUMN_PITCH, edgeEmphasis, edgeSegments, layoutPlanGraph, nodeEmphasis, tracePlanGraph } from "../client/plan-graph/layout";

const sample = (): PlanGraphData => JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8"));

export function diamondGraph(): PlanGraphData {
  const graph = sample();
  const deps = [[], ["TASK-001"], ["TASK-002"], ["TASK-002"], ["TASK-001", "TASK-002", "TASK-003", "TASK-004"], []];
  graph.nodes = deps.map((depends_on, index) => ({
    id: `TASK-00${index + 1}`, title: `Task ${index + 1}`, phase: "Phase 1", jira: null, kontor_id: null,
    depends_on, wave: [0, 1, 2, 2, 3, 0][index]!,
  }));
  graph.edges = graph.nodes.flatMap((node) => node.depends_on.map((from) => ({ from, to: node.id })));
  graph.waves = [0, 1, 2, 3].map((wave) => ({ wave, tasks: graph.nodes.filter((node) => node.wave === wave).map((node) => node.id) }));
  graph.external_fences = [];
  return planGraphSchema.parse(graph);
}

describe("canvas floor", () => {
  it("never lets the canvas collapse and keeps the majority of a measured page", () => {
    expect([0, Number.NaN, -50, Number.POSITIVE_INFINITY, 500].map((height) => canvasMinHeight(true, height))).toEqual([320, 320, 320, 320, 320]);
    expect([0, Number.NaN, 300].map((height) => canvasMinHeight(false, height))).toEqual([240, 240, 240]);
    // 390x844 with the 51px context row and the bottom tab bar leaves roughly 620-680px.
    for (const height of [620, 664, 680, 1000]) {
      expect(canvasMinHeight(true, height)).toBe(Math.round(height * 0.55));
      expect(canvasMinHeight(true, height) / height).toBeGreaterThan(0.5);
    }
  });
});

describe("deterministic native plan layout and directed traces", () => {
  it("lays out every canonical node, with depth columns and stable ID ordering", () => {
    const graph = sample();
    const layout = layoutPlanGraph(graph);
    graph.nodes.reverse(); graph.edges.reverse(); graph.waves.reverse(); graph.waves.forEach((wave) => wave.tasks.reverse());
    expect(layoutPlanGraph(graph)).toEqual(layout);
    expect(layout.nodes).toHaveLength(19);
    for (const item of layout.nodes) {
      expect(item.x).toBe(item.node.wave * COLUMN_PITCH);
      expect(item.width).toBe(CARD_WIDTH);
    }
    expect(layout.nodes.filter((item) => item.node.wave === 0).map((item) => item.node.id)).toEqual([...graph.waves.find((w) => w.wave === 0)!.tasks].sort());
  });

  it("starts at full contrast and restores all cards and edges when cleared", () => {
    const graph = sample();
    for (const active of [null, "missing"]) {
      const trace = tracePlanGraph(graph, active);
      expect(graph.nodes.map((node) => nodeEmphasis(trace, node.id))).toEqual(Array(19).fill("full"));
      expect(graph.edges.map((edge) => edgeEmphasis(trace, edge))).toEqual(Array(4).fill("full"));
    }
  });

  it("separates ancestors and descendants, excluding a diamond sibling and bypass edges", () => {
    const graph = diamondGraph();
    const trace = tracePlanGraph(graph, "TASK-003");
    expect([...trace.ancestors].sort()).toEqual(["TASK-001", "TASK-002"]);
    expect([...trace.descendants]).toEqual(["TASK-005"]);
    expect(nodeEmphasis(trace, "TASK-004")).toBe("dim");
    expect(nodeEmphasis(trace, "TASK-006")).toBe("dim");
    expect(nodeEmphasis(trace, "TASK-003")).toBe("active");
    const expected = ["upstream", "upstream", "dim", "dim", "dim", "downstream", "dim"];
    expect(graph.edges.map((edge) => edgeEmphasis(trace, edge))).toEqual(expected);
  });

  it("does not trace edges in reverse or invent serializes-with, Start or Done", () => {
    const graph = sample();
    const root = tracePlanGraph(graph, "TASK-001");
    const leaf = tracePlanGraph(graph, "TASK-005");
    expect(root.ancestors.size).toBe(0);
    expect(leaf.descendants.size).toBe(0);
    expect(root.descendants.has("TASK-002")).toBe(false);
    expect(layoutPlanGraph(graph).nodes.map((item) => item.node.id).sort()).toEqual(graph.nodes.map((node) => node.id).sort());
    expect(graph.edges.every((edge) => graph.nodes.find((node) => node.id === edge.to)!.depends_on.includes(edge.from))).toBe(true);
  });

  it("expands rows for text scale and produces directed connector geometry", () => {
    const graph = sample();
    const normal = layoutPlanGraph(graph);
    const large = layoutPlanGraph(graph, 2);
    expect(large.height).toBeGreaterThan(normal.height);
    for (const edge of graph.edges) {
      const from = large.nodes.find((item) => item.node.id === edge.from)!;
      const to = large.nodes.find((item) => item.node.id === edge.to)!;
      const segments = edgeSegments(from, to);
      expect(from.x).toBeLessThan(to.x);
      expect(segments[0]!.x).toBe(from.x + from.width);
      const end = segments.at(-1)!;
      expect(end.x + end.width).toBe(to.x);
      expect(end.y).toBe(to.y + to.height / 2);
      expect(segments.every((segment) => segment.width > 0 && segment.height > 0)).toBe(true);
    }
  });

  it("handles a deep graph iteratively, without synthesizing tasks", () => {
    const graph = sample();
    graph.nodes = Array.from({ length: 1500 }, (_, i) => ({ id: `TASK-${i}`, title: `Task ${i}`, phase: "Phase 1", jira: null, kontor_id: null, depends_on: i ? [`TASK-${i - 1}`] : [], wave: i }));
    graph.edges = graph.nodes.slice(1).map((node, i) => ({ from: graph.nodes[i]!.id, to: node.id }));
    graph.waves = graph.nodes.map((node) => ({ wave: node.wave, tasks: [node.id] }));
    graph.external_fences = [];
    expect(planGraphSchema.safeParse(graph).success).toBe(true);
    expect(tracePlanGraph(graph, "TASK-1499").ancestors.size).toBe(1499);
    expect(layoutPlanGraph(graph).nodes).toHaveLength(1500);
  });
});
