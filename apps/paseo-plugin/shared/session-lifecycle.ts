import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const TAKEOVER_LIMIT = "Bulk takeover is not available yet. You can retry eligible sessions in place. Sessions that need a different provider or a new owner stay listed with a reason; no replacement is created.";
export const RESUME_PROMPT = "The operator requests a retry of this interrupted task in this same session. Read the existing conversation and current workspace before continuing the existing authorized work. Preserve the task scope, checkout, children, supervisor, open findings and permission boundaries. If context, authority or a dependency is missing, report the limitation and wait. Do not create a replacement session.";
const id = z.string().min(1).max(200);
export const recoveryTargetSchema = z.object({ provider: id, model: id, effort: id }).strict();
export type RecoveryTarget = z.infer<typeof recoveryTargetSchema>;
export const DEFAULT_RECOVERY_TARGET: RecoveryTarget = { provider: "codex", model: "gpt-6.1-sol", effort: "xhigh" };
export const availabilitySchema = z.object({ agentId: id, nativeStatus: z.string().optional(), archivedAt: z.string().nullable().optional(), providerHistory: z.enum(["unverified", "not-reported"]).optional(), localRemoval: z.boolean().optional(), state: z.enum(["readable", "missing", "unknown"]), reason: z.string(), checkedAt: z.string(), revision: z.string() });
export type SessionAvailability = z.infer<typeof availabilitySchema>;
export const AVAILABILITY_TTL_MS = 60_000;
/** Exact native read evidence expires; neither a timestamp nor provider failure proves absence. */
export function freshAvailability(entry: { id: string; updatedAt: string; availability?: SessionAvailability }, now = Date.now()): SessionAvailability | undefined {
  const value = entry.availability, age = value ? now - Date.parse(value.checkedAt) : NaN;
  return value?.agentId === entry.id && value.revision === entry.updatedAt && age >= 0 && age < AVAILABILITY_TTL_MS ? value : undefined;
}
export const lifecycleRowSchema = z.object({
  id, title: z.string(), workspaceId: z.string().nullable(), cwd: z.string(), provider: z.string(), model: z.string().nullable(),
  effort: z.string().nullable(), mode: z.string().nullable(), nativeStatus: z.string(), fingerprint: z.string(),
  category: z.enum(["candidate", "unknown", "excluded"]), eligible: z.boolean(), reason: z.string(),
  children: z.array(z.object({ id, title: z.string(), status: z.string(), workspaceId: z.string().nullable() })),
  availability: availabilitySchema.optional(),
  reconcileReviewId: z.string().uuid().optional(),
  outcome: z.enum(["pending", "intent", "submitted", "archived", "blocked", "uncertain", "absent", "already_archived"]), message: z.string().nullable(),
  // Archive membership and runtime retirement are separate facts: "closed" only from an exact-ID read after the
  // archive, "not-observed" when the archive returned but the read is not closed, "unconfirmed" when the reply failed.
  runtime: z.enum(["closed", "not-observed", "unconfirmed"]).optional(),
});
export type LifecycleRow = z.infer<typeof lifecycleRowSchema>;
export const ARCHIVE_BATCH_LIMIT = 50;
/** Stable child-first order; a cyclic native relationship cannot be archived safely. */
export function archiveOrder(rows: LifecycleRow[]): LifecycleRow[] {
  const byId = new Map(rows.map((row) => [row.id, row])), visiting = new Set<string>(), done = new Set<string>(), ordered: LifecycleRow[] = [];
  function visit(row: LifecycleRow) {
    if (done.has(row.id)) return;
    if (visiting.has(row.id)) throw new Error("Managed-child cycle: no archive batch can run. Review native relationships first.");
    visiting.add(row.id);
    for (const child of row.children) { const selected = byId.get(child.id); if (selected) visit(selected); }
    visiting.delete(row.id); done.add(row.id); ordered.push(row);
  }
  rows.forEach(visit);
  return ordered;
}
export function archiveSummary(rows: LifecycleRow[]): string {
  const archived = rows.filter((row) => row.outcome === "archived" && row.runtime !== "unconfirmed").length;
  const uncertain = rows.filter((row) => unresolvedArchive(row)).length;
  const blocked = rows.filter((row) => row.outcome === "blocked").length;
  const absent = rows.filter((row) => row.outcome === "absent").length;
  const existing = rows.filter((row) => row.outcome === "already_archived").length;
  const pending = rows.filter((row) => row.outcome === "pending").length;
  return [`Archived ${archived}`, uncertain ? `${uncertain} not confirmed` : null, blocked ? `${blocked} blocked` : null,
    absent ? `${absent} absent (local removal only)` : null, existing ? `${existing} already archived` : null, pending ? `${pending} not attempted` : null].filter(Boolean).join(" · ");
}
/** An archive whose fence stays held until an exact-ID read settles it. */
export const unresolvedArchive = (row: Pick<LifecycleRow, "outcome" | "runtime">) =>
  row.outcome === "intent" || row.outcome === "uncertain" || (row.outcome === "archived" && row.runtime === "unconfirmed");
export const unresolvedRefSchema = z.object({ reviewId: z.string().uuid(), agentId: id }).strict();
export type UnresolvedRef = z.infer<typeof unresolvedRefSchema>;
export const lifecycleReviewSchema = z.object({
  id: z.string().uuid(), createdAt: z.string(), action: z.enum(["resume", "archive"]), scope: z.enum(["filtered", "host"]),
  target: recoveryTargetSchema, rows: z.array(lifecycleRowSchema), limitation: z.string(),
  // Derived only by the latest read; never saved. A latest.json pointer alone would lose an earlier unresolved archive.
  unresolved: z.array(unresolvedRefSchema).max(100).optional(),
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
export const reconcileLifecycleItem = defineRpc({ name: "sessions.lifecycle-reconcile-item", input: z.object({ reviewId: z.string().uuid(), agentId: id }).strict(), output: lifecycleRowSchema });
export const readLifecycleReview = defineRpc({ name: "sessions.lifecycle-review", input: z.object({ reviewId: z.string().uuid().optional() }).strict(), output: lifecycleReviewSchema.nullable() });
export const probeSessionAvailability = defineRpc({ name: "sessions.availability", input: z.object({ agentIds: z.array(id).max(100) }).strict(),
  output: z.record(z.string(), availabilitySchema) });
export const readSessionDispositions = defineRpc({ name: "sessions.lifecycle-dispositions", input: z.object({ agentIds: z.array(id).max(100) }).strict(), output: z.record(z.string(), availabilitySchema) });
