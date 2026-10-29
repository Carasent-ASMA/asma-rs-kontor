import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const takeoverModeSchema = z.union([
  z.object({ workflow: z.literal("kontor"), orchestration: z.literal("kontor") }).strict(),
  z.object({ workflow: z.literal("kontor"), orchestration: z.literal("paseo") }).strict(),
  z.object({ workflow: z.literal("free"), orchestration: z.literal("paseo") }).strict(),
]);
const id = z.string().uuid();
export const evidenceRefSchema = z
  .object({ path: z.string().min(1), sha256: z.string().regex(/^[a-f0-9]{64}$/) })
  .strict();
export const proceduralAdmissionSchema = z
  .object({
    version: z.literal(1),
    operationId: id,
    sourceId: id,
    hostId: z.string().min(1),
    mode: takeoverModeSchema,
    coordinatorId: id.nullable(),
    sourceRevision: z.string().length(64),
    workspaceId: z.string().min(1),
    cwd: z.string().min(1),
    projectKey: z.string().min(1),
    title: z.string().min(1),
    expiresAt: z.iso.datetime(),
    config: z
      .object({
        provider: z.string().min(1),
        model: z.string().min(1),
        effort: z.string().min(1),
        mode: z.string().min(1),
      })
      .strict(),
    checkout: z
      .object({
        path: z.string().min(1),
        branch: z.string().min(1),
        head: z.string().regex(/^[a-f0-9]{40}$/),
        wip: z.string().length(64),
      })
      .strict(),
    // These are owner-reviewed evidence, never a claim that Paseo certifies complete schedules or native workers.
    handoff: evidenceRefSchema,
    history: z.array(evidenceRefSchema).min(1),
    authority: evidenceRefSchema,
    plan: evidenceRefSchema,
    accounting: evidenceRefSchema,
    children: z.array(id),
    consent: z.literal("procedural-no-host-fence"),
  })
  .strict();
export type ProceduralAdmission = z.infer<typeof proceduralAdmissionSchema>;
export const proceduralReceiptSchema = z
  .object({
    version: z.literal(1),
    operationId: id,
    sourceId: id,
    successorId: id,
    admissionHash: z.string().length(64),
    revision: z.number().int().nonnegative(),
    phase: z.enum([
      "prepared",
      "preserving",
      "retiring",
      "retired",
      "creating",
      "created",
      "bootstrapping",
      "verified",
      "activating",
      "active",
      "held",
    ]),
    uncertainPhase: z.string().nullable(),
    message: z.string(),
    admission: proceduralAdmissionSchema,
    identityLabels: z.record(z.string(), z.string()),
    nativeHistory: evidenceRefSchema,
    sourcePersistence: z.string().nullable(),
    successorPersistence: z.string().nullable(),
    evidence: z.array(z.string()),
  })
  .strict();
export type ProceduralReceipt = z.infer<typeof proceduralReceiptSchema>;
export const PROCEDURAL_LIMITS = [
  "Plugin-local deduplication only; other clients and background senders are not fenced.",
  "Schedule, background-work and account-route completeness are owner-reviewed, not certified by the stock plugin API.",
  "Continuation is confirmed by an exact correlated successor response; a lost response stays unknown without replay.",
  "Public timeline capture is a bounded tail; complete available history and authority are owner-prepared references, not provider-private context migration.",
  "No schedule identity/history transfer or automatic predecessor revival.",
];
export const prepareProceduralTakeover = defineRpc({
  name: "sessions.procedural-takeover.prepare",
  input: z.object({ admission: evidenceRefSchema }).strict(),
  output: z.object({ receipt: proceduralReceiptSchema, limits: z.array(z.string()) }).strict(),
});
export const executeProceduralTakeover = defineRpc({
  name: "sessions.procedural-takeover.execute",
  input: z.object({ operationId: id, admissionHash: z.string().length(64) }).strict(),
  output: proceduralReceiptSchema,
});
export const checkProceduralTakeover = defineRpc({
  name: "sessions.procedural-takeover.check",
  input: z.object({ operationId: id }).strict(),
  output: proceduralReceiptSchema,
});

export const readPreparedTakeoverCapability = defineRpc({
  name: "sessions.procedural-takeover.capability",
  input: z.object({}).strict(),
  output: z.object({ supported: z.literal(true), limits: z.array(z.string()) }).strict(),
});
export const importPreparedTakeover = defineRpc({
  name: "sessions.procedural-takeover.import",
  input: z.object({ sourceId: id, admission: evidenceRefSchema }).strict(),
  output: z
    .object({
      receipt: proceduralReceiptSchema,
      canExecute: z.boolean(),
      limits: z.array(z.string()),
    })
    .strict(),
});
