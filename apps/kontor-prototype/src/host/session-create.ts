import { adhocTitle, normalizeTopic, topicProblem, createRequestSchema, type CreateOptions, type CreateOutcome, type CreateRequest } from "../../../paseo-plugin/shared/session-create";

/** Page-memory simulation of the current creation RPCs; never calls a native host. */
export function fixtureCreate(enabled: boolean, add: (request: CreateRequest, title: string) => string) {
  const outcomes = new Map<string, CreateOutcome>();
  const options: CreateOptions = {
    enabled, disabledReason: enabled ? null : "SIMULATED: this scenario has no Adam desk. Use Combined navigator review.",
    desks: enabled ? [{ workspaceId: "wks_proof_adam", projectId: "prj_proof_adam", desk: "ADAM", project: "DESK • ADAM" }] : [],
    providers: [{ id: "codex", label: "Codex (simulated)", available: true, note: "Synthetic provider; no account or model is contacted.", persona: "supported",
      models: [{ id: "gpt-6.1-sol", label: "Fixture model", efforts: [{ id: "xhigh", label: "Extra high" }], defaultEffort: "xhigh" }],
      defaultModel: "gpt-6.1-sol", modes: [{ id: "fixture-mode", label: "Fixture permissions", dangerous: false }], defaultMode: "fixture-mode" }],
    profile: { name: "LSA (fictional)", provider: "codex", model: "gpt-6.1-sol", modeId: "fixture-mode", effort: "xhigh" },
    pending: null, notes: ["SIMULATED creation changes only this page's fictional session map."],
  };
  return async (method: string, input: unknown): Promise<unknown> => {
    if (method === "sessions.create-options") return structuredClone(options);
    if (method === "sessions.create-check") {
      const result = outcomes.get((input as { requestId: string }).requestId);
      if (!result) throw new Error("Unknown fixture creation request");
      return structuredClone(result);
    }
    if (method !== "sessions.create") throw new Error(`Unsupported fixture creation RPC ${method}`);
    const request = createRequestSchema.parse(input);
    const prior = outcomes.get(request.requestId);
    if (prior) return structuredClone(prior);
    const topic = normalizeTopic(request.topic), title = adhocTitle(topic);
    const problem = !enabled ? options.disabledReason : topicProblem(topic)
      ?? (request.workspaceId !== "wks_proof_adam" || request.provider !== "codex" || request.model !== "gpt-6.1-sol"
        || request.effort !== "xhigh" || request.modeId !== "fixture-mode" ? "Unsupported fixture creation options" : null);
    const result: CreateOutcome = { requestId: request.requestId, state: problem ? "refused" : "verified", title,
      agentId: problem ? null : add(request, title), workspaceId: problem ? null : request.workspaceId,
      message: problem ?? "SIMULATED: fictional session added to this page only; no native effect or persona dispatch.",
      differences: [], persona: "supported", checkedAt: new Date().toISOString() };
    outcomes.set(request.requestId, result);
    return structuredClone(result);
  };
}
