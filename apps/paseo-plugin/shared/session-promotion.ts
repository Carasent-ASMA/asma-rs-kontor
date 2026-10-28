import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

const id = z.string().min(1).max(200);
const digest = z.string().regex(/^[a-f0-9]{64}$/);
const epic = z.string().regex(/^ASMA-[1-9]\d*$/);

export const nativePromotionPlacementSchema = z.object({
  workspaceId: id,
  workspace: z.string().min(1),
  title: z.string().min(1),
  titleAuto: z.string().min(1),
  observedAt: z.string(),
  revision: z.string(),
}).strict();
export type NativePromotionPlacement = z.infer<typeof nativePromotionPlacementSchema>;

export const logicalPromotionPlacementSchema = z.object({
  workspace: z.literal("LSA"),
  node: z.string().min(1),
  bindingRevision: digest,
}).strict();

export const promotionStateSchema = z.enum(["promoting", "promoted", "handoff", "unknown"]);
export type PromotionState = z.infer<typeof promotionStateSchema>;

export const promotionRecordSchema = z.object({
  schemaVersion: z.literal(1),
  requestId: z.string().uuid(),
  agentId: id,
  epic,
  branch: z.enum(["promote", "handoff"]),
  state: promotionStateSchema,
  native: nativePromotionPlacementSchema,
  logical: logicalPromotionPlacementSchema,
  persona: z.object({ issued: z.string().min(1), effective: z.string().min(1) }).strict(),
  canonicalAgentId: id.nullable(),
  summarySha256: digest.nullable(),
  summaryRevision: z.string().min(1).max(200).nullable(),
  acknowledgement: z.enum(["not-required", "verified", "unknown"]),
  message: z.string(),
  createdAt: z.string(),
  updatedAt: z.string(),
}).strict();
export type PromotionRecord = z.infer<typeof promotionRecordSchema>;

export const promoteSessionInput = z.object({
  requestId: z.string().uuid(),
  agentId: id,
  epic,
  summary: z.object({ text: z.string().min(1).max(20_000), revision: z.string().min(1).max(200) }).strict().optional(),
}).strict();
export type PromoteSessionInput = z.infer<typeof promoteSessionInput>;

export const promotionOutcomeSchema = z.object({
  requestId: z.string().uuid(),
  state: z.enum(["unchanged", "promoted", "handoff", "unknown", "refused"]),
  agentId: id,
  epic,
  canonicalAgentId: id.nullable(),
  native: nativePromotionPlacementSchema.nullable(),
  logical: logicalPromotionPlacementSchema.nullable(),
  summarySha256: digest.nullable(),
  summaryRevision: z.string().min(1).max(200).nullable(),
  acknowledgement: z.enum(["not-required", "verified", "unknown"]),
  message: z.string(),
  checkedAt: z.string(),
}).strict();
export type PromotionOutcome = z.infer<typeof promotionOutcomeSchema>;

export const promotionValidationSchema = z.object({
  state: z.enum(["current", "stale", "unknown"]),
  message: z.string(),
}).strict();
export type PromotionValidation = z.infer<typeof promotionValidationSchema>;
export const promotionListEntrySchema = z.object({ record: promotionRecordSchema, validation: promotionValidationSchema }).strict();
export type PromotionListEntry = z.infer<typeof promotionListEntrySchema>;

export const promoteSession = defineRpc({ name: "sessions.promote", input: promoteSessionInput, output: promotionOutcomeSchema });
export const readPromotion = defineRpc({ name: "sessions.promotion-read", input: z.object({ agentId: id }).strict(), output: z.object({ record: promotionRecordSchema, native: nativePromotionPlacementSchema.nullable() }).strict().nullable() });
export const readPromotionList = defineRpc({ name: "sessions.promotion-list", input: z.object({}).strict(), output: z.array(promotionListEntrySchema) });
