import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";
import { sourceSummarySchema } from "./read-sources";

/** ASMA-8550 v1. Host formats refer only to IDs in the existing source catalog.
 * No snapshot attests activation, eligibility, account availability or a running model.
 * Array order is source order; alternatives in one rung are never serial fallbacks.
 */
export const teamsSourceFormatSchema = z.enum([
  "fleet-authoring", "fleet-operator", "core-roster", "team-templates",
]);
export type TeamsSourceFormat = z.infer<typeof teamsSourceFormatSchema>;
const provenanceSchema = z.object({ sourceId: z.string(), pointer: z.string() });
const diagnosticSchema = z.object({ code: z.string(), pointer: z.string(), message: z.string() });
const domainSchema = z.object({
  alias: z.string(), provider: z.string(), accounts: z.array(z.string()),
  modelPrefix: z.string().nullable(), provenance: provenanceSchema,
});
const modelSchema = z.object({
  alias: z.string(), id: z.string(), domain: z.string(), vendor: z.string(),
  supportedEfforts: z.array(z.string()).nullable(), vision: z.boolean(), calibrated: z.boolean(),
  provenance: provenanceSchema,
});
const alternativeSchema = z.object({
  reference: z.string(), modelAlias: z.string(), selectedEffort: z.string().nullable(),
  model: modelSchema.nullable(), domain: domainSchema.nullable(),
  diagnostics: z.array(diagnosticSchema), provenance: provenanceSchema,
});
const chainSchema = z.object({
  name: z.string(), rungs: z.array(z.object({
    index: z.number().int(), alternatives: z.array(alternativeSchema), provenance: provenanceSchema,
  })), provenance: provenanceSchema,
});
const bindingSchema = z.object({
  reference: z.string(), groupKind: z.enum(["team", "leadership", "committee", "advisor"]),
  groupId: z.string(), slotId: z.string().nullable(), chainName: z.string(),
  chainResolved: z.boolean(), provenance: provenanceSchema,
});
export const fleetSourceDataSchema = z.object({
  kind: z.literal("fleet"), schemaVersion: z.union([z.literal(1), z.literal(2)]),
  domains: z.array(domainSchema), models: z.array(modelSchema), chains: z.array(chainSchema),
  bindings: z.array(bindingSchema),
  declaredUnavailable: z.object({ domains: z.array(z.string()), accounts: z.array(z.string()) }),
  rules: z.object({
    calibration_required: z.array(z.string()), vision_required: z.array(z.string()),
    independent_of: z.record(z.string(), z.string()),
  }),
});
const roleSchema = z.object({ role: z.string(), version: z.number().int().positive() });
const templateSlotSchema = z.object({
  id: z.string(), role: roleSchema,
  // These are ordered template rungs, not configured fleet alternatives or observed sessions.
  defaultRungs: z.array(z.object({ provider: z.string(), model: z.string(), effort: z.string().nullable() })),
  skills: z.array(z.object({ skill: z.string(), version: z.number().int().positive() })),
  context: z.object({ template: z.string(), version: z.number().int().positive() }).nullable(),
  mayEvaluate: z.array(z.string()), mayWaive: z.array(z.string()),
  waiverPolicy: z.object({ authorized_roles: z.array(z.string()), required_evidence: z.array(z.string()) }).nullable(),
  autonomy: z.string().nullable(), provenance: provenanceSchema,
});
const templateSchema = z.object({
  id: z.string(), version: z.number().int().positive(), name: z.string(),
  roles: z.array(z.object({ role: roleSchema, min_slots: z.number().int(), max_slots: z.number().int() })),
  slots: z.array(templateSlotSchema),
  handoffs: z.array(z.object({ from_slot: z.string(), to_slot: z.string(),
    after_phase: z.string().nullable(), required_artifacts: z.array(z.string()) })),
  maxSuccessorDepth: z.number().int(), maxHandoffDepth: z.number().int(), provenance: provenanceSchema,
});
export const templatesSourceDataSchema = z.object({ kind: z.literal("templates"), templates: z.array(templateSchema) });
export const rosterSourceDataSchema = z.object({
  kind: z.literal("roster"), version: z.number().int().positive(),
  roleCatalog: z.object({ catalog_id: z.string(), version: z.number().int().positive(), content_hash: z.string() }),
  seats: z.array(z.object({ role_slot_id: z.string(), role_code: z.string(), presence: z.string(),
    ad_hoc_allowed: z.boolean(), provenance: provenanceSchema })),
});
export const teamsSourceSnapshotSchema = z.object({
  contractVersion: z.literal(1), source: sourceSummarySchema, format: teamsSourceFormatSchema,
  status: z.enum(["ready", "partial", "unavailable"]),
  sha256: z.string().nullable(), readAt: z.string(), modifiedAt: z.string().nullable(),
  activation: z.literal("not-attested"),
  configurationState: z.enum(["unactivated-authoring", "operator-file-snapshot", "core-roster-snapshot", "bundled-template-defaults"]),
  data: z.discriminatedUnion("kind", [fleetSourceDataSchema, templatesSourceDataSchema, rosterSourceDataSchema]).nullable(),
  diagnostics: z.array(diagnosticSchema),
});
export type TeamsSourceSnapshot = z.infer<typeof teamsSourceSnapshotSchema>;
export type TeamsSourceDiagnostic = z.infer<typeof diagnosticSchema>;
export type TeamsFleetData = z.infer<typeof fleetSourceDataSchema>;
export type TeamsTemplatesData = z.infer<typeof templatesSourceDataSchema>;
export type TeamsRosterData = z.infer<typeof rosterSourceDataSchema>;

export const teamsSourceRequestSchema = z.object({ sourceId: z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/) }).strict();
export const readTeamsSource = defineRpc({ name: "teams.sources.read", input: teamsSourceRequestSchema, output: teamsSourceSnapshotSchema });

/** Native observations come from A's native session reader. No configuration join is inferred. */
export type ObservedTeamsSession = Readonly<{
  nativeSessionId: string;
  provider: string | null;
  model: string | null;
  effort: string | null;
  observedAt: string;
}>;
export type TeamsSourceView = Readonly<{
  configuredSources: readonly TeamsSourceSnapshot[];
  observedSessions: readonly ObservedTeamsSession[];
  metadata: readonly TeamsBindingMetadata[];
}>;
export type TeamsBindingMetadata = Readonly<{
  sourceId: string;
  bindingReference: string;
  state: "unresolved";
  reason: string;
  // All exact ID+slot matches, preserving versions. A binding declares no version.
  templateCandidates: readonly Readonly<{ sourceId: string; templateId: string; version: number; slotId: string }>[];
}>;

/** Explicit candidates only: never select a latest template or guess a roster/title/role.
 * A roster seat's slot alone does not identify a leadership roster hash.
 */
export function composeTeamsSourceView(
  configuredSources: readonly TeamsSourceSnapshot[], observedSessions: readonly ObservedTeamsSession[],
): TeamsSourceView {
  const metadata: TeamsBindingMetadata[] = [];
  for (const source of configuredSources) {
    if (source.data?.kind !== "fleet") continue;
    for (const binding of source.data.bindings) {
      const templateCandidates: TeamsBindingMetadata["templateCandidates"][number][] = [];
      let templateIdFound = false;
      if (binding.groupKind === "team" && binding.slotId !== null) {
        for (const templates of configuredSources) {
          if (templates.data?.kind !== "templates") continue;
          for (const template of templates.data.templates) {
            if (template.id !== binding.groupId) continue;
            templateIdFound = true;
            if (template.slots.some((slot) => slot.id === binding.slotId)) {
              templateCandidates.push({ sourceId: templates.source.id, templateId: template.id,
                version: template.version, slotId: binding.slotId });
            }
          }
        }
      }
      let reason = "No authorized definition identifies this group and slot.";
      if (templateIdFound && !templateCandidates.length) reason = `Unknown slot ${binding.slotId} in the available versions of template ${binding.groupId}.`;
      if (templateCandidates.length) reason = "Binding does not pin a template version; exact ID/slot candidates only.";
      if (binding.groupKind === "leadership") reason = "Core roster does not declare the binding's roster identity; slot alone is insufficient.";
      metadata.push({ sourceId: source.source.id, bindingReference: binding.reference, state: "unresolved", reason, templateCandidates });
    }
  }
  return { configuredSources, observedSessions, metadata };
}
