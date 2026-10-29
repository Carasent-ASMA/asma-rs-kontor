import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

const id = z.string().min(1).max(200);

/**
 * What a host can do for the navigator, answered by its own server. An unsupported capability names why; a supported
 * one may state `limits` up front, which every control offering it shows before it starts.
 */
export const capabilitySchema = z.union([
  z.object({ supported: z.literal(true), limits: z.array(z.string()).max(10).optional() }).strict(),
  z.object({ supported: z.literal(false), reason: z.string() }).strict(),
]);
export type Capability = z.infer<typeof capabilitySchema>;

export const navigatorCapabilitiesSchema = z.object({
  /** Whole-session input/output token totals for rows. A latest report never stands in for one. */
  usageTotals: capabilitySchema,
  /** Durable saved summary revisions with a source and cutoff. */
  summaries: capabilitySchema,
  /** A summary placed in another seat's draft for review: never sent, never an ownership change. */
  draftAttach: capabilitySchema,
  /** Take over of one seat on its own chain, confirmed at once when its review states no limit beyond these `limits`. */
  takeover: capabilitySchema,
  /** Take over of several selected seats, each its own review, confirmation and result. */
  takeoverBulk: capabilitySchema,
  /** A model chosen for one takeover instead of the seat's own chain. */
  takeoverRoute: capabilitySchema,
}).strict();
export type NavigatorCapabilities = z.infer<typeof navigatorCapabilitiesSchema>;
export type NavigatorCapability = keyof NavigatorCapabilities;

export const readNavigatorCapabilities = defineRpc({ name: "sessions.navigator-capabilities", input: z.object({}).strict(), output: navigatorCapabilitiesSchema });

/** Qualified session totals or an explicitly labelled provider cumulative snapshot; null is unavailable, never zero. */
export const usageTotalsSchema = z.object({ input: z.number().int().nonnegative().nullable(), output: z.number().int().nonnegative().nullable(), source: z.string(), scope: z.enum(["session", "provider-cumulative"]).optional() }).strict();
export type UsageTotals = z.infer<typeof usageTotalsSchema>;
export const readUsageTotals = defineRpc({ name: "sessions.usage-totals", input: z.object({ agentIds: z.array(id).max(200) }).strict(),
  output: z.record(z.string(), usageTotalsSchema.nullable()) });

/** A saved compact summary. Saving again after the source changed makes a new revision; a revision never changes. */
export const summaryRevisionSchema = z.object({
  summaryId: z.string().uuid(), revision: z.number().int().positive(),
  sourceAgentId: id, sourceTitle: z.string(),
  /** The last source event this revision covers. */
  cutoff: z.string(), savedAt: z.string(),
  sections: z.object({ task: z.string(), decisions: z.string(), currentState: z.string(), nextStep: z.string() }).strict(),
  /** The text Copy places on the clipboard. */
  text: z.string(),
}).strict();
export type SummaryRevision = z.infer<typeof summaryRevisionSchema>;
export const summaryPinSchema = z.object({ summaryId: z.string().uuid(), revision: z.number().int().positive() }).strict();
export type SummaryPin = z.infer<typeof summaryPinSchema>;

export const saveSummary = defineRpc({ name: "sessions.summary-save", input: z.object({ agentId: id }).strict(), output: summaryRevisionSchema });
export const readSummary = defineRpc({ name: "sessions.summary-read", input: summaryPinSchema, output: summaryRevisionSchema });
export const latestSummaries = defineRpc({ name: "sessions.summary-latest", input: z.object({ agentIds: z.array(id).max(200) }).strict(),
  output: z.record(z.string(), summaryRevisionSchema.nullable()) });

/** drafted: the destination's draft holds the summary for its operator to review. Nothing is sent either way. */
export const attachOutcomeSchema = z.object({ state: z.enum(["drafted", "refused", "unknown"]), message: z.string(), destinationAgentId: id, summaryId: z.string().uuid(), revision: z.number().int().positive() }).strict();
export type AttachOutcome = z.infer<typeof attachOutcomeSchema>;
export const attachSummary = defineRpc({ name: "sessions.summary-attach", input: summaryPinSchema.extend({ destinationAgentId: id }).strict(), output: attachOutcomeSchema });

/** The answers a host without these owners gives; each names what is missing, so no control pretends otherwise. */
export const UNSUPPORTED = {
  usageTotals: { supported: false, reason: "This host reports only each session's latest usage, not totals since it started." },
  summaries: { supported: false, reason: "This host has no saved summary store; Kontor cannot save or pin a summary revision here." },
  draftAttach: { supported: false, reason: "This Paseo app gives plugins no way to place a draft in another session for review." },
  takeover: { supported: false, reason: "Take over is not qualified on this host: no owner here creates and verifies the new session, so nothing would continue the task." },
  takeoverBulk: { supported: false, reason: "Take over of several sessions is not qualified on this host; take over one session at a time." },
  takeoverRoute: { supported: false, reason: "This host takes over only on the seat's configured chain; choosing a model for one takeover is not supported." },
} as const satisfies NavigatorCapabilities;
