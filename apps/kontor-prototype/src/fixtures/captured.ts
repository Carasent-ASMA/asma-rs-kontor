import { z } from "zod";
import sessionsInput from "captured:sessions";
import catalogInput from "captured:catalog";
import featuresInput from "captured:features";

/**
 * Operator-local data captured from supported Paseo reads and frozen at `capturedAt`. Each export is null when the
 * prototype runs without it, so the committed build and tests stay synthetic. Free text the capture may hold, such
 * as provider and model descriptions, is dropped here: unknown keys never pass these schemas.
 */
const option = z.object({ id: z.string(), label: z.string(), isDefault: z.boolean().optional() });
const catalogSchema = z.object({
  provenance: z.object({ capturedAt: z.string() }),
  providers: z.array(z.object({ id: z.string(), label: z.string(), enabled: z.boolean(), status: z.string(),
    modes: z.array(z.object({ id: z.string(), label: z.string(), colorTier: z.string().optional() })) })),
  catalogs: z.array(z.object({ provider: z.string(), models: z.array(z.object({ id: z.string(), label: z.string(), isDefault: z.boolean().nullish(),
    defaultThinkingOptionId: z.string().nullish(), thinkingOptions: z.array(option).nullish() })) })),
});
const featuresSchema = z.object({
  capturedAt: z.string(), provider: z.string(), selectedModel: z.string(),
  features: z.array(z.union([
    z.object({ type: z.literal("toggle"), id: z.string(), label: z.string(), value: z.boolean() }),
    z.object({ type: z.literal("select"), id: z.string(), label: z.string(), value: z.string().nullable(), options: z.array(option) }),
  ])),
});
const sessionsSchema = z.object({
  provenance: z.object({ capturedAt: z.string(), completeHostInventory: z.boolean(), sessionCount: z.number() }),
  agents: z.array(z.object({
    id: z.string(), title: z.string(), provider: z.string(), model: z.string().nullable(),
    thinkingOptionId: z.string().nullish(), effectiveThinkingOptionId: z.string().nullish(), currentModeId: z.string().nullish(),
    workspaceId: z.string().nullable(), cwd: z.string(), status: z.enum(["initializing", "idle", "running", "error", "closed"]),
    createdAt: z.string(), updatedAt: z.string(), lastUserMessageAt: z.string().nullish(),
    requiresAttention: z.boolean(), attentionReason: z.enum(["finished", "error", "permission"]).nullish(), attentionTimestamp: z.string().nullish(),
    activeTurn: z.object({ startedAt: z.string().nullish() }).nullish(),
    labels: z.record(z.string(), z.string()),
    runtimeInfo: z.object({ provider: z.string(), model: z.string().nullish(), thinkingOptionId: z.string().nullish(), modeId: z.string().nullish() }).nullish(),
    latestUsageReport: z.object({ inputTokens: z.number().optional(), cachedInputTokens: z.number().optional(), outputTokens: z.number().optional(),
      contextWindowMaxTokens: z.number().optional(), contextWindowUsedTokens: z.number().optional() }).nullish(),
    pendingPermissionCount: z.number(),
  })),
  workspaces: z.array(z.object({ workspaceId: z.string(), projectId: z.string(), cwd: z.string(), kind: z.string(), title: z.string() })),
});

/** An empty stub is absent; a present file that does not parse fails loudly, never as an empty capture. */
function load<T extends z.ZodType>(name: string, schema: T, input: unknown): z.infer<T> | null {
  if (input && typeof input === "object" && !Object.keys(input).length) return null;
  const parsed = schema.safeParse(input);
  if (!parsed.success) throw new Error(`Captured ${name} fixture does not match its expected shape: ${parsed.error.issues[0]?.message ?? "unknown"}`);
  return parsed.data;
}

export const capturedCatalog = load("model catalog", catalogSchema, catalogInput);
export const capturedFeatures = load("feature", featuresSchema, featuresInput);
export const capturedSessions = load("session", sessionsSchema, sessionsInput);
export type CapturedCatalog = NonNullable<typeof capturedCatalog>;
export type CapturedFeatures = NonNullable<typeof capturedFeatures>;
export type CapturedSessions = NonNullable<typeof capturedSessions>;

/** Last visited offsets, newest first, in minutes; the capture holds no visit history, so this order is simulated. */
const SIMULATED_VISITS = [0, 12, 60, 2 * 1440, 3 * 1440, 14 * 1440, 15 * 1440, 21 * 1440, 35 * 1440, 40 * 1440];

/**
 * The captured cohort as fixture rows. Kept as captured: titles, labels, states, times, models, efforts, modes and
 * the last usage report, which stays a last report and never becomes a session total. Unknown stays unknown:
 * project names (none were read), provider session IDs and raw permissions (omitted; only their count is real).
 * Last visited order and times are simulated and labelled so.
 */
export function capturedFixture(now = Date.now()) {
  if (!capturedSessions) return null;
  const workspaces = new Map(capturedSessions.workspaces.map((workspace) => [workspace.workspaceId, workspace]));
  const sessions = capturedSessions.agents.map((agent) => {
    const workspace = agent.workspaceId ? workspaces.get(agent.workspaceId) : undefined;
    return {
      agent: {
        id: agent.id, title: agent.title, status: agent.status, provider: agent.provider, model: agent.model,
        thinkingOptionId: agent.thinkingOptionId ?? undefined, effectiveThinkingOptionId: agent.effectiveThinkingOptionId ?? undefined, currentModeId: agent.currentModeId ?? undefined,
        workspaceId: agent.workspaceId, cwd: agent.cwd, createdAt: agent.createdAt, updatedAt: agent.updatedAt, lastUserMessageAt: agent.lastUserMessageAt ?? null,
        requiresAttention: agent.requiresAttention, attentionReason: agent.attentionReason ?? null, attentionTimestamp: agent.attentionTimestamp ?? null,
        activeTurn: agent.activeTurn?.startedAt ? { startedAt: agent.activeTurn.startedAt } : null,
        pendingPermissions: Array.from({ length: agent.pendingPermissionCount }, (_, index) => ({ id: `captured_permission_${index}`, kind: "omitted" })),
        archivedAt: null, labels: agent.labels,
        runtimeInfo: agent.runtimeInfo ? { sessionId: null, provider: agent.runtimeInfo.provider, model: agent.runtimeInfo.model ?? null, thinkingOptionId: agent.runtimeInfo.thinkingOptionId ?? null, modeId: agent.runtimeInfo.modeId ?? null } : undefined,
        lastUsage: agent.latestUsageReport ?? undefined,
      },
      project: workspace ? { projectKey: workspace.projectId, projectName: "", workspaceName: workspace.title } : null,
    };
  });
  const byActivity = [...sessions].sort((left, right) => (right.agent.activeTurn?.startedAt ?? right.agent.lastUserMessageAt ?? "").localeCompare(left.agent.activeTurn?.startedAt ?? left.agent.lastUserMessageAt ?? ""));
  const visits = byActivity.slice(0, SIMULATED_VISITS.length).map((session, index) => ({ id: session.agent.id, at: now - SIMULATED_VISITS[index]! * 60_000 }));
  return {
    provenance: capturedSessions.provenance, sessions, visits,
    workspaces: capturedSessions.workspaces.map((workspace) => ({ id: workspace.workspaceId, projectId: workspace.projectId, projectDisplayName: "", projectCustomName: null,
      title: workspace.title, name: workspace.title, directory: workspace.cwd, status: "active" as const })),
  };
}
