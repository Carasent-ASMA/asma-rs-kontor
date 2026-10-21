import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const TAKEOVER_LIMIT = "Bulk takeover is not available yet. You can retry eligible sessions in place. Sessions that need a different provider or a new owner stay listed with a reason; no replacement is created.";
export const RESUME_PROMPT = "The operator requests a retry of this interrupted task in this same session. Read the existing conversation and current workspace before continuing the existing authorized work. Preserve the task scope, checkout, children, supervisor, open findings and permission boundaries. If context, authority or a dependency is missing, report the limitation and wait. Do not create a replacement session.";
const id = z.string().min(1).max(200);
export const recoveryTargetSchema = z.object({ provider: id, model: id, effort: id }).strict();
export type RecoveryTarget = z.infer<typeof recoveryTargetSchema>;
export const DEFAULT_RECOVERY_TARGET: RecoveryTarget = { provider: "codex", model: "gpt-6.1-sol", effort: "xhigh" };
export const availabilitySchema = z.object({ state: z.enum(["readable", "missing", "unknown"]), reason: z.string(), checkedAt: z.string(), revision: z.string() });
export type SessionAvailability = z.infer<typeof availabilitySchema>;
export const lifecycleRowSchema = z.object({
  id, title: z.string(), workspaceId: z.string().nullable(), cwd: z.string(), provider: z.string(), model: z.string().nullable(),
  effort: z.string().nullable(), mode: z.string().nullable(), nativeStatus: z.string(), fingerprint: z.string(),
  category: z.enum(["candidate", "unknown", "excluded"]), eligible: z.boolean(), reason: z.string(),
  children: z.array(z.object({ id, title: z.string(), status: z.string(), workspaceId: z.string().nullable() })),
  outcome: z.enum(["pending", "intent", "submitted", "archived", "blocked", "uncertain"]), message: z.string().nullable(),
});
export type LifecycleRow = z.infer<typeof lifecycleRowSchema>;
export const lifecycleReviewSchema = z.object({
  id: z.string().uuid(), createdAt: z.string(), action: z.enum(["resume", "archive"]), scope: z.enum(["filtered", "host"]),
  target: recoveryTargetSchema, rows: z.array(lifecycleRowSchema), limitation: z.string(),
});
export type LifecycleReview = z.infer<typeof lifecycleReviewSchema>;
export const lifecycleCapabilities = defineRpc({ name: "sessions.lifecycle-capabilities", input: z.object({}).strict(), output: z.object({
  providers: z.array(z.object({ id: z.string(), status: z.string(), models: z.array(z.object({ id: z.string(), efforts: z.array(z.string()) })) })),
  limitation: z.string(),
}) });
export const previewLifecycle = defineRpc({ name: "sessions.lifecycle-preview", input: z.object({
  action: z.enum(["resume", "archive"]), scope: z.enum(["filtered", "host"]), agentIds: z.array(id).max(10_000), target: recoveryTargetSchema,
}).strict(), output: lifecycleReviewSchema });
export const executeLifecycleItem = defineRpc({ name: "sessions.lifecycle-execute-item", input: z.object({ reviewId: z.string().uuid(), agentId: id }).strict(), output: lifecycleRowSchema });
export const readLifecycleReview = defineRpc({ name: "sessions.lifecycle-review", input: z.object({ reviewId: z.string().uuid().optional() }).strict(), output: lifecycleReviewSchema.nullable() });
export const probeSessionAvailability = defineRpc({ name: "sessions.availability", input: z.object({ agentIds: z.array(id).max(100) }).strict(),
  output: z.record(z.string(), availabilitySchema) });
