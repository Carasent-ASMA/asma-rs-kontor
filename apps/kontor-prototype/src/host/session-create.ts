import { createChoiceProblem, createProviders, featureValuesProblem, issueTopic, leadTitle, leadWorkspaceName, normalizeTopic, topicProblem, createRequestSchema,
  type CreateFeature, type CreateOptions, type CreateOutcome, type CreateRequest, type CreateScope, type JiraIssue, type ProviderSnapshotEntry } from "../../../paseo-plugin/shared/session-create";
import { capturedCatalog, capturedFeatures, type CapturedCatalog } from "../fixtures/captured";
import type { FixtureAgent } from "../fixtures/sessions";
import type { fixtureNavigator } from "./session-navigator";
import type { SummaryRevision } from "../../../paseo-plugin/shared/session-navigator";

const ISSUE: JiraIssue = { id: "100", key: "ASMA-100", summary: "Deliver the desk layout", created: "2026-10-01T10:00:00.000+0000", type: "Epic", epic: true, status: "In Development", statusCategory: "In Progress", open: true, parent: null };
const DESK = { projectId: "prj_proof_adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam" as const, cwd: "/fixture/prj_proof_adam",
  lead: { ADHOC: "wks_proof_adhoc", LSA: "wks_proof_lsa", TPM: "wks_proof_tpm" } };
/** A host's fictional desks in the native names' order; each lists the same catalog, so any one can be chosen and created on. */
const DESKS = [DESK, ...([["review", "PR REVIEW", "pr-review"], ["tickets", "TICKETS", "tickets"]] as const).map(([key, desk, kind]) => ({ projectId: `prj_proof_${key}`, project: `DESK • ${desk}`, desk, kind,
  cwd: `/fixture/prj_proof_${key}`, lead: { ADHOC: `wks_proof_${key}_adhoc`, LSA: `wks_proof_${key}_lsa`, TPM: `wks_proof_${key}_tpm` } }))];
export const fixtureDesk = (projectId: string) => DESKS.find((desk) => desk.projectId === projectId) ?? DESK;
const PROFILES = {
  LSA: { name: "LSA (fictional)", provider: "codex", model: "gpt-6.1-sol", modeId: "fixture-mode", effort: "xhigh" },
  TPM: { name: "TPM (fictional)", provider: "codex", model: "gpt-6.1-sol", modeId: "fixture-mode", effort: "xhigh" },
} as const;

const EFFORTS = [{ id: "low", label: "Low" }, { id: "medium", label: "Medium" }, { id: "high", label: "High" }, { id: "xhigh", label: "XHigh" }];
/** Fictional providers for a build without a captured catalog: Codex flags its own default; OpenCode routes other vendors' models. */
const SYNTHETIC: ProviderSnapshotEntry[] = [
  { provider: "codex", label: "Codex", enabled: true, status: "ready", defaultModeId: "fixture-mode",
    modes: [{ id: "fixture-mode", label: "Default permissions" }, { id: "fixture-full-access", label: "Full access", colorTier: "dangerous" }],
    models: [{ id: "gpt-6-astra", label: "GPT-6 Astra", isDefault: true, defaultThinkingOptionId: "xhigh", thinkingOptions: EFFORTS },
      { id: "gpt-6.1-sol", label: "GPT-6.1 Sol", defaultThinkingOptionId: "xhigh", thinkingOptions: EFFORTS }] },
  { provider: "claude", label: "Claude Code", enabled: true, status: "ready", defaultModeId: "default",
    modes: [{ id: "default", label: "Default" }, { id: "bypassPermissions", label: "Bypass permissions", colorTier: "dangerous" }],
    models: [{ id: "claude-opus-5-5", label: "Claude Opus 5.5", isDefault: true, defaultThinkingOptionId: "high", thinkingOptions: [{ id: "high", label: "High" }, { id: "max", label: "Max" }] }] },
  { provider: "opencode", label: "OpenCode", enabled: true, status: "ready",
    models: [{ id: "deepseek/deepseek-flash", label: "DeepSeek Flash" }, { id: "zai-coding-plan/glm-5.3", label: "GLM 5.3" }] },
  { provider: "cursor", label: "Cursor", enabled: true, status: "ready", models: [{ id: "composer-2.5", label: "Composer 2.5" }] },
];
/** Codex's listed settings for a synthetic model. */
const SYNTHETIC_FEATURES: CreateFeature[] = [
  { type: "select", id: "service_tier", label: "Speed", value: null, options: [{ id: "default", label: "Normal", isDefault: true }, { id: "priority", label: "Fast", isDefault: false }] },
  { type: "toggle", id: "plan_mode", label: "Plan", value: false },
];

/** A captured catalog as the host snapshot lists it; the catalog's "available" is the snapshot's "ready". Only ids, labels and flags are read. */
const capturedEntries = (catalog: CapturedCatalog): ProviderSnapshotEntry[] => catalog.providers.map((provider) => ({
  provider: provider.id, label: provider.label, enabled: provider.enabled, status: provider.status === "available" ? "ready" : provider.status, modes: provider.modes,
  models: catalog.catalogs.find((entry) => entry.provider === provider.id)?.models.map((model) => ({ id: model.id, label: model.label, isDefault: model.isDefault,
    defaultThinkingOptionId: model.defaultThinkingOptionId, thinkingOptions: model.thinkingOptions?.map((option) => ({ id: option.id, label: option.label, isDefault: option.isDefault })) })) ?? [] }));
/** This host's catalog: the captured one when present, else the synthetic one. */
export const PROVIDERS = createProviders({ entries: capturedCatalog ? capturedEntries(capturedCatalog) : SYNTHETIC });
const CATALOG_NOTE = capturedCatalog ? `Model catalog captured at ${capturedCatalog.provenance.capturedAt}; SIMULATED creation contacts no provider.`
  : "Synthetic model catalog; SIMULATED creation contacts no provider or account.";

/** The settings the host lists for one catalog model: captured ones for the model they were captured on, synthetic Codex ones for any other Codex model, none elsewhere; null when the model is not in the catalog. */
export function listedFeatures(provider: string, model: string): CreateFeature[] | null {
  if (!PROVIDERS.find((entry) => entry.id === provider)?.models.some((entry) => entry.id === model)) return null;
  if (capturedFeatures && capturedFeatures.provider === provider && capturedFeatures.selectedModel === model)
    return capturedFeatures.features.map((feature): CreateFeature => feature.type === "toggle" ? { type: "toggle", id: feature.id, label: feature.label, value: feature.value }
      : { type: "select", id: feature.id, label: feature.label, value: feature.value, options: feature.options.map((option) => ({ id: option.id, label: option.label, isDefault: option.isDefault ?? false })) });
  return provider === "codex" ? structuredClone(SYNTHETIC_FEATURES) : [];
}

/**
 * The requested settings as the created seat holds them: "Speed Fast", "Plan on" for each one it stores as asked, and a
 * difference for each one it stores otherwise or not at all. Read from the seat, never from the request.
 */
export function settingsReadback(seat: Pick<FixtureAgent, "features"> | undefined, values: Record<string, string | boolean>): { applied: string[]; differences: string[] } {
  const applied: string[] = [], differences: string[] = [];
  for (const [key, value] of Object.entries(values)) {
    const feature = seat?.features?.find((entry) => entry.id === key);
    const shown = (stored: string | boolean | null | undefined) => feature?.type === "select" ? feature.options.find((option) => option.id === stored)?.label ?? String(stored ?? "not reported") : stored === true ? "on" : stored === false ? "off" : "not reported";
    if (feature?.value === value) applied.push(`${feature.label} ${shown(value)}`);
    else differences.push(`${feature?.label ?? key} reads ${shown(feature?.value)}.`);
  }
  return { applied, differences };
}

/** Where created seats live: `add` stores one with the settings it accepted, `read` returns it as stored. */
export type FixtureSeats = {
  add(request: CreateRequest, title: string, workspaceId: string, features: CreateFeature[] | null): string;
  read(agentId: string): FixtureAgent | undefined;
};

/**
 * Page-memory simulation of the current creation RPCs; never calls a native host. With `pending`, an earlier SIMULATED
 * creation whose effect is unknown blocks a new one; Check reads it and, as on a host that never sees it, it stays unknown.
 */
export function fixtureCreate(enabled: boolean, seats: FixtureSeats, summaries: Pick<ReturnType<typeof fixtureNavigator>, "resolve" | "pin" | "pinned"> | null = null, pending = false) {
  const outcomes = new Map<string, CreateOutcome>();
  const unsettled: CreateOutcome | null = pending ? { requestId: crypto.randomUUID(), state: "unknown", title: leadTitle("LSA", null, "SIMULATED pending creation"), agentId: null, workspaceId: null,
    message: "SIMULATED: an earlier creation was sent and its effect is not confirmed. Check reads it; nothing is resent.", differences: [], persona: "supported", checkedAt: new Date().toISOString() } : null;
  if (unsettled) outcomes.set(unsettled.requestId, unsettled);
  const options = (role: "LSA" | "TPM" = "LSA", projectId?: string): CreateOptions => ({
    enabled, disabledReason: enabled ? null : "SIMULATED: this scenario has no Adam desk. Use Combined navigator review.",
    desks: enabled ? DESKS : [],
    catalogProjectId: enabled ? fixtureDesk(projectId ?? DESK.projectId).projectId : null,
    providers: structuredClone(PROVIDERS),
    profile: PROFILES[role],
    issues: enabled ? [ISSUE] : [],
    jiraProblem: null,
    pending: unsettled, notes: ["SIMULATED creation changes only this page's in-memory session map.", CATALOG_NOTE],
  });
  const features = (input: { projectId: string; provider: string; model: string }): { features: CreateFeature[]; problem: string | null } => {
    if (!enabled || !DESKS.some((desk) => desk.projectId === input.projectId)) return { features: [], problem: "SIMULATED: that desk is not on this host." };
    const listed = listedFeatures(input.provider, input.model);
    return listed ? { features: listed, problem: null } : { features: [], problem: `SIMULATED: ${input.model} is not in this host's catalog.` };
  };
  return async (method: string, input: unknown): Promise<unknown> => {
    if (method === "sessions.create-options") return structuredClone(options((input as CreateScope | undefined)?.role, (input as CreateScope | undefined)?.projectId));
    if (method === "sessions.create-features") return features(input as { projectId: string; provider: string; model: string });
    if (method === "sessions.create-check") {
      const result = outcomes.get((input as { requestId: string }).requestId);
      if (!result) throw new Error("Unknown fixture creation request");
      return structuredClone(result);
    }
    if (method !== "sessions.create") throw new Error(`Unsupported fixture creation RPC ${method}`);
    const request = createRequestSchema.parse(input);
    const prior = outcomes.get(request.requestId);
    if (prior) return structuredClone(prior);
    const current = options(request.role, request.projectId);
    const desk = current.desks.find((entry) => entry.projectId === request.projectId);
    const issue = request.start === "from-jira" ? current.issues.find((entry) => entry.key === request.issueKey?.trim().toUpperCase()) ?? null : null;
    const topic = request.start === "from-jira" && request.topic === undefined ? issueTopic(issue?.summary ?? "") : normalizeTopic(request.topic ?? "");
    const workspaceName = leadWorkspaceName(request.role, request.start);
    const workspaceId = desk?.lead[workspaceName] ?? null;
    const values = request.featureValues ?? {};
    const listed = Object.keys(values).length ? features(request) : null;
    // The pin resolves to one exact saved revision of the named source before anything is created.
    let pin: SummaryRevision | null = null, pinProblem: string | null = null;
    if (request.sourceSummary) {
      if (!summaries) pinProblem = "SIMULATED: this host keeps no saved summaries to pin.";
      else try { pin = summaries.resolve(request.sourceSummary, request.sourceAgentId ?? null); } catch (error) { pinProblem = error instanceof Error ? error.message : String(error); }
    }
    const problem = !enabled ? current.disabledReason
      : unsettled ? `SIMULATED: an earlier creation, “${unsettled.title}”, is not settled. Check it first; nothing was created.`
      : !desk ? "SIMULATED: this scenario has no matching desk."
        : !workspaceId ? `SIMULATED: ${desk.project} has no ${workspaceName} checkout.`
          : request.start === "from-jira" && !issue ? "SIMULATED: choose a fixture issue."
            : topicProblem(topic) ? topicProblem(topic)
              : createChoiceProblem(PROVIDERS, request) ?? (listed ? listed.problem ?? featureValuesProblem(listed.features, values) : null) ?? pinProblem;
    const title = leadTitle(request.role, issue?.key ?? null, topic);
    // The seat stores each listed setting with the value it accepted; the readback below reads the seat, not this request.
    const agentId = problem ? null : seats.add(request, title, workspaceId!, listed ? listed.features.map((feature) => ({ ...feature, value: values[feature.id] ?? feature.value }) as CreateFeature) : null);
    if (agentId && pin) summaries!.pin(agentId, pin);
    const readback = agentId ? settingsReadback(seats.read(agentId), values) : { applied: [], differences: [] };
    const pinned = agentId ? summaries?.pinned(agentId) ?? null : null;
    const result: CreateOutcome = { requestId: request.requestId, state: problem ? "refused" : readback.differences.length ? "differences" : "verified", title,
      agentId, workspaceId: problem ? null : workspaceId,
      message: problem ?? ["SIMULATED: session added to this page's memory only; no native effect or persona dispatch.",
        readback.applied.length ? `Settings read back: ${readback.applied.join(", ")}.` : null,
        pinned ? `Saved summary r${pinned.revision} of ${pinned.sourceAgentId} pinned in page memory; nothing was sent.` : null].filter(Boolean).join(" "),
      differences: readback.differences, persona: "supported", checkedAt: new Date().toISOString() };
    outcomes.set(request.requestId, result);
    return structuredClone(result);
  };
}
