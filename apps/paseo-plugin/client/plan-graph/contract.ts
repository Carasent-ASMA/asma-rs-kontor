import { z } from "zod";

const text = z.string().min(1).refine((value) => value === value.trim() && !/[\u0000-\u001f]/.test(value), "Expected trimmed single-line text");
const sha256 = z.string().regex(/^[a-f0-9]{64}$/);
const jira = z.string().regex(/^ASMA-[1-9][0-9]*$/);
const taskId = z.string().regex(/^[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*-[0-9]+$/).refine((value) => !/^ASMA-/.test(value), "Use a local task code, not a Jira key");
const wave = z.number().int().nonnegative();

export const planNodeSchema = z.strictObject({
  id: taskId,
  title: text,
  phase: text,
  jira: jira.nullable(),
  kontor_id: text.nullable(),
  depends_on: z.array(taskId),
  wave,
});

/** Version 1 of `asma plan export`. A wave is dependency depth, never readiness. */
export const planGraphSchema = z.strictObject({
  schema_version: z.literal(1),
  source: text,
  source_sha256: sha256,
  graph_sha256: sha256,
  epic: z.strictObject({ jira: jira.nullable(), kontor_id: text.nullable() }),
  nodes: z.array(planNodeSchema).min(1).max(10000),
  edges: z.array(z.strictObject({ from: taskId, to: taskId })),
  waves: z.array(z.strictObject({ wave, tasks: z.array(taskId) })),
  external_fences: z.array(z.strictObject({
    task: taskId,
    prerequisite: text,
    owner: text,
    lift_condition: text,
  })),
}).superRefine((graph, context) => {
  const issue = (path: (string | number)[], message: string) =>
    context.addIssue({ code: "custom", path, message });
  const nodes = new Map(graph.nodes.map((node) => [node.id, node]));
  if (nodes.size !== graph.nodes.length) issue(["nodes"], "Duplicate task ID");
  for (const field of ["jira", "kontor_id"] as const) {
    const bindings = [graph.epic, ...graph.nodes].map((node) => node[field]).filter((value) => value !== null);
    if (new Set(bindings).size !== bindings.length) issue(["nodes"], `Duplicate ${field} binding`);
  }
  const expectedEdges = new Set<string>();
  for (const [index, node] of graph.nodes.entries()) {
    if (new Set(node.depends_on).size !== node.depends_on.length) {
      issue(["nodes", index, "depends_on"], "Duplicate prerequisite");
    }
    for (const prerequisite of node.depends_on) {
      if (!nodes.has(prerequisite) || prerequisite === node.id) {
        issue(["nodes", index, "depends_on"], `Invalid prerequisite ${prerequisite}`);
      }
      expectedEdges.add(`${prerequisite}:${node.id}`);
    }
  }
  const actualEdges = new Set<string>();
  for (const [index, edge] of graph.edges.entries()) {
    const key = `${edge.from}:${edge.to}`;
    if (!expectedEdges.has(key) || actualEdges.has(key)) {
      issue(["edges", index], "Edge must be a unique, directed direct prerequisite");
    }
    actualEdges.add(key);
  }
  if ([...expectedEdges].some((key) => !actualEdges.has(key))) {
    issue(["edges"], "Missing direct prerequisite edge");
  }
  // Iterative topological traversal also bounds work for large/deep graphs.
  const remaining = new Map(graph.nodes.map((node) => [node.id, node.depends_on.length]));
  const dependents = new Map<string, string[]>();
  for (const node of graph.nodes) {
    for (const prerequisite of node.depends_on) {
      const list = dependents.get(prerequisite) ?? [];
      list.push(node.id);
      dependents.set(prerequisite, list);
    }
  }
  const queue = graph.nodes.filter((node) => !node.depends_on.length).map((node) => node.id);
  const depths = new Map<string, number>();
  for (let index = 0; index < queue.length; index += 1) {
    const id = queue[index]!;
    const node = nodes.get(id)!;
    const depth = node.depends_on.reduce((max, parent) => Math.max(max, (depths.get(parent) ?? -1) + 1), 0);
    depths.set(id, depth);
    if (node.wave !== depth) issue(["nodes", graph.nodes.indexOf(node), "wave"], "Wave must equal dependency depth");
    for (const child of dependents.get(id) ?? []) {
      const count = (remaining.get(child) ?? 0) - 1;
      remaining.set(child, count);
      if (count === 0) queue.push(child);
    }
  }
  if (depths.size !== nodes.size) issue(["nodes"], "Dependencies must be an acyclic graph with known prerequisites");
  const membership = new Set<string>();
  const waveNumbers = new Set<number>();
  for (const [index, layer] of graph.waves.entries()) {
    if (waveNumbers.has(layer.wave) || !layer.tasks.length) issue(["waves", index], "Duplicate or empty wave");
    waveNumbers.add(layer.wave);
    for (const id of layer.tasks) {
      if (membership.has(id) || nodes.get(id)?.wave !== layer.wave) {
        issue(["waves", index], `Invalid wave membership for ${id}`);
      }
      membership.add(id);
    }
  }
  if (membership.size !== nodes.size) issue(["waves"], "Waves must contain every task exactly once");
  for (const [index, fence] of graph.external_fences.entries()) {
    if (!nodes.has(fence.task)) issue(["external_fences", index, "task"], "Unknown fence task");
  }
});

export type PlanGraphData = z.infer<typeof planGraphSchema>;
export type PlanNode = z.infer<typeof planNodeSchema>;
export type PlanSourceIdentity = Pick<PlanGraphData, "source" | "source_sha256" | "graph_sha256">;
export type PlanGraphDiagnostic = {
  code: "schema" | "source_mismatch" | "source_revision_mismatch" | "graph_revision_mismatch";
  path: string;
  message: string;
  expected?: string;
  actual?: string;
};

/** Compare exact exporter identity; this does not claim to hash or read the source file. */
export function validatePlanGraph(payload: unknown, expected: PlanSourceIdentity):
  | { ok: true; graph: PlanGraphData }
  | { ok: false; diagnostics: PlanGraphDiagnostic[] } {
  const parsed = planGraphSchema.safeParse(payload);
  if (!parsed.success) return {
    ok: false,
    diagnostics: parsed.error.issues.map((issue) => ({ code: "schema", path: issue.path.join("."), message: issue.message })),
  };
  const fields = [
    ["source", "source_mismatch"],
    ["source_sha256", "source_revision_mismatch"],
    ["graph_sha256", "graph_revision_mismatch"],
  ] as const;
  const diagnostics: PlanGraphDiagnostic[] = [];
  for (const [field, code] of fields) {
    if (parsed.data[field] !== expected[field]) diagnostics.push({
      code, path: field, message: `Exact ${field} differs from the requested source`,
      expected: expected[field], actual: parsed.data[field],
    });
  }
  return diagnostics.length ? { ok: false, diagnostics } : { ok: true, graph: parsed.data };
}

/** Status is a separate source observation, never inferred from this export. */
export type SourcedPlanState = {
  label: string;
  source: string;
  observedAt: string;
};

export type PlanSourceAction =
  | { kind: "plan"; source: string; source_sha256: string; taskId: string }
  | { kind: "jira"; jira: string };

export function planSourceActions(graph: PlanGraphData, node: PlanNode): PlanSourceAction[] {
  const canonical = graph.nodes.find((candidate) => candidate.id === node.id);
  if (!canonical) return [];
  const actions: PlanSourceAction[] = [{ kind: "plan", source: graph.source, source_sha256: graph.source_sha256, taskId: canonical.id }];
  if (canonical.jira && jira.safeParse(canonical.jira).success) actions.push({ kind: "jira", jira: canonical.jira });
  return actions;
}
