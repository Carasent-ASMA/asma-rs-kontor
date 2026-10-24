import { z } from "zod";

export const sessionReadTargetSchema = z.object({
  hostId: z.string().min(1),
  id: z.string().min(1),
  updatedAt: z.string(),
  attentionTimestamp: z.string().nullable(),
});
export type SessionReadTarget = z.infer<typeof sessionReadTargetSchema>;

export const nativeReadAgentSchema = z.object({
  id: z.string(),
  updatedAt: z.string(),
  requiresAttention: z.boolean(),
  attentionReason: z.enum(["finished", "error", "permission"]).nullable().optional(),
  attentionTimestamp: z.string().nullable().optional(),
  pendingPermissions: z.array(z.unknown()),
});

export const nativeReadResultSchema = z.object({
  requestId: z.string(),
  agentId: z.string(),
  outcome: z.enum(["seen", "already_seen", "changed", "needs_input", "missing", "error"]),
  agent: nativeReadAgentSchema.nullable(),
  error: z.string().nullable(),
});

export const sessionReadOutcomeSchema = z.enum([
  "seen", "already_seen", "new_unread", "changed", "needs_input", "missing", "unknown", "unsupported", "not_sent",
]);
export type SessionReadOutcome = z.infer<typeof sessionReadOutcomeSchema>;
export type SessionReadResult = {
  readonly hostId: string;
  readonly agentId: string;
  readonly outcome: SessionReadOutcome;
  readonly message: string;
};

export const SESSION_READ_MESSAGES: Record<SessionReadOutcome, string> = {
  seen: "Seen confirmed by Paseo.",
  already_seen: "Paseo already reports this session seen.",
  new_unread: "A new unread result remains; the acknowledgement did not clear it.",
  changed: "The session changed. Its newer result was left unread; refresh before retrying.",
  needs_input: "Still needs input. Marking seen does not answer permission requests.",
  missing: "Paseo returned no native record for this exact ID on this host. Nothing was marked seen.",
  unknown: "Read acknowledgement was not confirmed. Refresh before retrying.",
  unsupported: "This Paseo host needs metadata-only read acknowledgement support before it can mark sessions seen.",
  not_sent: "Stopped before this session was acknowledged.",
};
