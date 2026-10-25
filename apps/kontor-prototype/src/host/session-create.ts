import { issueTopic, leadTitle, leadWorkspaceName, normalizeTopic, topicProblem, createRequestSchema, type CreateOptions, type CreateOutcome, type CreateRequest, type CreateScope, type JiraIssue } from "../../../paseo-plugin/shared/session-create";

const ISSUE: JiraIssue = { id: "100", key: "ASMA-100", summary: "Deliver the desk layout", created: "2026-10-01T10:00:00.000+0000", type: "Epic", epic: true, status: "In Development", statusCategory: "In Progress", open: true, parent: null };
const DESK = { projectId: "prj_proof_adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam" as const, cwd: "/fixture/prj_proof_adam",
  lead: { ADHOC: "wks_proof_adhoc", LSA: "wks_proof_lsa", TPM: "wks_proof_tpm" } };
const PROFILES = {
  LSA: { name: "LSA (fictional)", provider: "codex", model: "gpt-6.1-sol", modeId: "fixture-mode", effort: "xhigh" },
  TPM: { name: "TPM (fictional)", provider: "codex", model: "gpt-6.1-sol", modeId: "fixture-mode", effort: "xhigh" },
} as const;

/** Page-memory simulation of the current creation RPCs; never calls a native host. */
export function fixtureCreate(enabled: boolean, add: (request: CreateRequest, title: string, workspaceId: string) => string) {
  const outcomes = new Map<string, CreateOutcome>();
  const options = (role: "LSA" | "TPM" = "LSA"): CreateOptions => ({
    enabled, disabledReason: enabled ? null : "SIMULATED: this scenario has no Adam desk. Use Combined navigator review.",
    desks: enabled ? [DESK] : [],
    catalogProjectId: enabled ? "prj_proof_adam" : null,
    providers: [{ id: "codex", label: "Codex (simulated)", available: true, status: "ready", note: "Synthetic provider; no account or model is contacted.", persona: "supported",
      models: [{ id: "gpt-6.1-sol", label: "Fixture model", efforts: [{ id: "xhigh", label: "Extra high" }], defaultEffort: "xhigh" }],
      defaultModel: "gpt-6.1-sol", modes: [{ id: "fixture-mode", label: "Fixture permissions", dangerous: false }], defaultMode: "fixture-mode" }],
    profile: PROFILES[role],
    issues: enabled ? [ISSUE] : [],
    jiraProblem: null,
    pending: null, notes: ["SIMULATED creation changes only this page's fictional session map."],
  });
  return async (method: string, input: unknown): Promise<unknown> => {
    if (method === "sessions.create-options") return structuredClone(options((input as CreateScope | undefined)?.role));
    if (method === "sessions.create-check") {
      const result = outcomes.get((input as { requestId: string }).requestId);
      if (!result) throw new Error("Unknown fixture creation request");
      return structuredClone(result);
    }
    if (method !== "sessions.create") throw new Error(`Unsupported fixture creation RPC ${method}`);
    const request = createRequestSchema.parse(input);
    const prior = outcomes.get(request.requestId);
    if (prior) return structuredClone(prior);
    const current = options(request.role);
    const desk = current.desks.find((entry) => entry.projectId === request.projectId);
    const issue = request.start === "from-jira" ? current.issues.find((entry) => entry.key === request.issueKey?.trim().toUpperCase()) ?? null : null;
    const topic = request.start === "from-jira" && request.topic === undefined ? issueTopic(issue?.summary ?? "") : normalizeTopic(request.topic ?? "");
    const workspaceName = leadWorkspaceName(request.role, request.start);
    const workspaceId = desk?.lead[workspaceName] ?? null;
    const problem = !enabled ? current.disabledReason
      : !desk ? "SIMULATED: this scenario has no matching desk."
        : !workspaceId ? `SIMULATED: DESK • ADAM has no ${workspaceName} checkout.`
          : request.start === "from-jira" && !issue ? "SIMULATED: choose a fixture issue."
            : topicProblem(topic) ? topicProblem(topic)
              : request.provider !== "codex" || request.model !== "gpt-6.1-sol" || request.effort !== "xhigh" || request.modeId !== "fixture-mode" ? "Unsupported fixture creation options" : null;
    const title = leadTitle(request.role, issue?.key ?? null, topic);
    const result: CreateOutcome = { requestId: request.requestId, state: problem ? "refused" : "verified", title,
      agentId: problem ? null : add(request, title, workspaceId!), workspaceId: problem ? null : workspaceId,
      message: problem ?? "SIMULATED: fictional session added to this page only; no native effect or persona dispatch.",
      differences: [], persona: "supported", checkedAt: new Date().toISOString() };
    outcomes.set(request.requestId, result);
    return structuredClone(result);
  };
}
