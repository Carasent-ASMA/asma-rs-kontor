import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { describe, expect, it } from "vitest";
import { planGraphSchema, planSourceActions, validatePlanGraph, type PlanGraphData } from "../client/plan-graph/contract";

const frozen = readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8");
const sample = (): PlanGraphData => JSON.parse(frozen);

describe("canonical plan export boundary", () => {
  it("accepts the actual frozen ASMA-8371 contract without adding live state", () => {
    const graph = planGraphSchema.parse(sample());
    expect(graph.nodes).toHaveLength(19);
    expect(graph.edges).toHaveLength(4);
    expect(graph.epic.jira).toBe("ASMA-8371");
    expect(graph.source_sha256).toBe("c8d1abadfe7321fe6e95919ebf0a185bea31f39965b751b7c6dc58c17f2d89bc");
    expect(graph.graph_sha256).toBe("98c2f6de65df5b5ed410dbde8e5ae307a9eea95a113e6cfa8b5148320364c7f5");
    expect(validatePlanGraph(graph, graph)).toEqual({ ok: true, graph });
    expect(Object.keys(graph.nodes[0]!)).toEqual(["id", "title", "phase", "jira", "kontor_id", "depends_on", "wave"]);
  });

  it("accepts canonical KN-style local codes and honest null bindings", () => {
    const graph = sample();
    const remap = (id: string) => id.replace("TASK-", "KN-");
    graph.nodes.forEach((node) => { node.id = remap(node.id); node.depends_on = node.depends_on.map(remap); node.jira = null; });
    graph.edges.forEach((edge) => { edge.from = remap(edge.from); edge.to = remap(edge.to); });
    graph.waves.forEach((wave) => { wave.tasks = wave.tasks.map(remap); });
    graph.external_fences.forEach((fence) => { fence.task = remap(fence.task); });
    expect(planGraphSchema.safeParse(graph).success).toBe(true);
  });

  it.each([
    ["source", "source_mismatch", "/configured/other-plan.md"],
    ["source_sha256", "source_revision_mismatch", "a".repeat(64)],
    ["graph_sha256", "graph_revision_mismatch", "b".repeat(64)],
  ] as const)("reports exact %s mismatch without substituting a sample", (field, code, value) => {
    const graph = sample();
    const result = validatePlanGraph(graph, { ...graph, [field]: value });
    expect(result).toEqual({ ok: false, diagnostics: [{ code, path: field,
      message: `Exact ${field} differs from the requested source`, expected: value, actual: graph[field] }] });
  });

  const invalid: [string, (graph: PlanGraphData) => unknown][] = [
    ["missing source hash", (g) => ({ ...g, source_sha256: undefined })],
    ["unsupported schema", (g) => ({ ...g, schema_version: 2 })],
    ["invalid hash", (g) => ({ ...g, graph_sha256: "c8d1abad" })],
    ["source normalization", (g) => ({ ...g, source: ` ${g.source}` })],
    ["empty graph", (g) => ({ ...g, nodes: [], edges: [], waves: [], external_fences: [] })],
    ["duplicate task", (g) => ({ ...g, nodes: [...g.nodes, g.nodes[0]] })],
    ["nonlocal ID", (g) => { g.nodes[0]!.id = "ASMA-8372"; return g; }],
    ["invented Jira", (g) => { g.nodes[0]!.jira = "OTHER-8372"; return g; }],
    ["duplicate binding", (g) => { g.nodes[0]!.jira = g.epic.jira; return g; }],
    ["self dependency", (g) => { g.nodes[0]!.depends_on = [g.nodes[0]!.id]; return g; }],
    ["unknown prerequisite", (g) => { g.nodes[0]!.depends_on = ["TASK-999"]; return g; }],
    ["duplicate prerequisite", (g) => { g.nodes[2]!.depends_on.push("TASK-001"); return g; }],
    ["cycle", (g) => { g.nodes[0]!.depends_on = ["TASK-005"]; g.edges.push({ from: "TASK-005", to: "TASK-001" }); return g; }],
    ["wrong direction", (g) => { g.edges[0] = { from: "TASK-003", to: "TASK-001" }; return g; }],
    ["missing edge", (g) => ({ ...g, edges: g.edges.slice(1) })],
    ["duplicate edge", (g) => ({ ...g, edges: [...g.edges, g.edges[0]] })],
    ["invented edge", (g) => { g.edges.push({ from: "TASK-001", to: "TASK-002" }); return g; }],
    ["authored wave", (g) => { g.nodes[3]!.wave = 1; return g; }],
    ["missing wave task", (g) => { g.waves[0]!.tasks.pop(); return g; }],
    ["duplicate wave", (g) => ({ ...g, waves: [...g.waves, g.waves[0]] })],
    ["wrong wave", (g) => { g.waves[0]!.tasks.push("TASK-003"); return g; }],
    ["unknown fence", (g) => { g.external_fences[0]!.task = "TASK-999"; return g; }],
    ["empty fence owner", (g) => { g.external_fences[0]!.owner = ""; return g; }],
    ["multiline title", (g) => { g.nodes[0]!.title += "\nextra"; return g; }],
  ];
  it.each(invalid)("rejects %s with schema diagnostics", (_name, mutate) => {
    const result = validatePlanGraph(mutate(sample()), sample());
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.diagnostics.some((item) => item.code === "schema" && item.message.length > 0)).toBe(true);
  });

  it.each(["status", "readiness", "frontier", "capacity", "planned_start", "governance", "serializes_with"])("rejects invented %s fields", (field) => {
    const graph = sample();
    expect(planGraphSchema.safeParse({ ...graph, [field]: "invented" }).success).toBe(false);
    expect(planGraphSchema.safeParse({ ...graph, nodes: [{ ...graph.nodes[0], [field]: "invented" }, ...graph.nodes.slice(1)] }).success).toBe(false);
  });

  it.each(["Start", "Done"])("rejects a synthetic %s node", (id) => {
    const graph = sample(); graph.nodes.push({ ...graph.nodes[0]!, id, jira: null });
    expect(planGraphSchema.safeParse(graph).success).toBe(false);
  });

  it("offers only canonical source actions and confirmed ASMA Jira bindings", () => {
    const graph = sample();
    const node = graph.nodes[0]!;
    expect(planSourceActions(graph, { ...node, jira: "ASMA-99999" })).toEqual([
      { kind: "plan", source: graph.source, source_sha256: graph.source_sha256, taskId: node.id },
      { kind: "jira", jira: "ASMA-8372" },
    ]);
    node.jira = null;
    expect(planSourceActions(graph, node)).toHaveLength(1);
    expect(planSourceActions(graph, { ...node, id: "TASK-999" })).toEqual([]);
  });
});
