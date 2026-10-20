import { z } from "zod";
import type { TeamsFleetData, TeamsRosterData, TeamsSourceDiagnostic, TeamsTemplatesData } from "../shared/teams-source-snapshot";

const text = z.string().min(1).max(512);
const slug = text.regex(/^[a-zA-Z0-9_.-]+$/);
const list = z.array(text);
const version = z.number().int().positive();
const role = z.object({ role: slug, version }).strict();
const fleetSchema = z.object({
  schema_version: z.union([z.literal(1), z.literal(2)]),
  domains: z.record(slug, z.object({ provider: slug, accounts: z.array(slug).min(1), model_prefix: text.optional() }).strict()),
  models: z.record(slug, z.object({ domain: slug, id: text, vendor: slug,
    efforts: z.array(slug).min(1).optional(), vision: z.boolean(), calibrated: z.boolean() }).strict()),
  chains: z.record(slug, z.array(z.array(text.regex(/^[a-zA-Z0-9_.-]+(?:@[a-zA-Z0-9_.-]+)?$/)).min(1).max(64)).min(1).max(16)),
  bindings: z.record(text, slug),
  unavailable: z.object({ domains: z.array(slug), accounts: z.array(slug) }).strict(),
  rules: z.object({ calibration_required: list, vision_required: list, independent_of: z.record(text, text) }).strict(),
}).strict();
const rosterSchema = z.object({ schema_version: z.literal(1), version,
  role_catalog: z.object({ catalog_id: z.uuid(), version, content_hash: text.regex(/^[a-f0-9]{64}$/) }).strict(),
  seats: z.array(z.object({ role_slot_id: slug, role_code: slug,
    presence: z.enum(["required", "optional"]), ad_hoc_allowed: z.boolean() }).strict()),
}).strict();
const templateSchema = z.object({ schema_version: z.literal(1), template_id: z.uuid(), version, name: text,
  roles: z.array(z.object({ role, min_slots: z.number().int().nonnegative(), max_slots: version }).strict()),
  slots: z.array(z.object({ id: slug, role,
    model_chain: z.object({ rungs: z.array(z.object({ provider: slug, model: text,
      effort: slug.nullable().optional() }).strict()).min(1) }).strict(),
    skills: z.array(z.object({ skill: slug, version }).strict()),
    context: z.object({ template: slug, version }).strict().nullable(),
    may_evaluate: list, may_waive: list,
    waiver_policy: z.object({ authorized_roles: list, required_evidence: list }).strict().nullable(),
    autonomy: slug.optional(),
  }).strict()),
  handoffs: z.array(z.object({ from_slot: slug, to_slot: slug, after_phase: text.nullable(), required_artifacts: list }).strict()),
  max_successor_depth: z.number().int().nonnegative(), max_handoff_depth: z.number().int().nonnegative(),
}).strict();
const packSchema = z.object({ schema_version: z.literal(1), teams: z.array(templateSchema) }).strict();

const pointerPart = (value: string) => value.replaceAll("~", "~0").replaceAll("/", "~1");
const provenance = (sourceId: string, pointer: string) => ({ sourceId, pointer });
const diagnostic = (code: string, pointer: string, message: string): TeamsSourceDiagnostic => ({ code, pointer, message });

function bindingIdentity(reference: string, schemaVersion: number): Pick<TeamsFleetData["bindings"][number], "groupKind" | "groupId" | "slotId"> {
  const parts = reference.split("/");
  const [groupKind, groupId, slotId] = parts;
  const validPart = (part: string | undefined) => part !== undefined && /^[a-zA-Z0-9_.-]+$/.test(part);
  if (!validPart(groupId)) throw new Error(`Malformed binding reference: ${reference}`);
  if (groupKind === "advisor" && parts.length === 2) return { groupKind, groupId: groupId!, slotId: null };
  if (parts.length === 3 && validPart(slotId)) {
    if (groupKind === "team" || groupKind === "committee") return { groupKind, groupId: groupId!, slotId: slotId! };
    if (groupKind === "leadership" && schemaVersion === 2 && /^[a-f0-9]{64}$/.test(groupId!)) {
      return { groupKind, groupId: groupId!, slotId: slotId! };
    }
  }
  throw new Error(`Malformed or unsupported binding reference: ${reference}`);
}

/** Display projection only. The canonical Rust fleet validator still owns policy eligibility. */
export function projectTeamsFleet(raw: unknown, sourceId: string): { data: TeamsFleetData; diagnostics: TeamsSourceDiagnostic[] } {
  const input = fleetSchema.parse(raw);
  const diagnostics: TeamsSourceDiagnostic[] = [];
  const domains = Object.entries(input.domains).map(([alias, value]) => ({ alias, provider: value.provider,
    accounts: value.accounts, modelPrefix: value.model_prefix ?? null, provenance: provenance(sourceId, `/domains/${pointerPart(alias)}`) }));
  const models = Object.entries(input.models).map(([alias, value]) => ({ alias, id: value.id, domain: value.domain,
    vendor: value.vendor, supportedEfforts: value.efforts ?? null, vision: value.vision, calibrated: value.calibrated,
    provenance: provenance(sourceId, `/models/${pointerPart(alias)}`) }));
  const domainByAlias = new Map(domains.map((domain) => [domain.alias, domain]));
  const modelByAlias = new Map(models.map((model) => [model.alias, model]));
  for (const model of models) {
    if (!domainByAlias.has(model.domain)) diagnostics.push(diagnostic("unknown-domain", model.provenance.pointer, `Unknown domain ${model.domain}`));
  }
  const chains = Object.entries(input.chains).map(([name, steps]) => {
    const pointer = `/chains/${pointerPart(name)}`;
    return { name, provenance: provenance(sourceId, pointer), rungs: steps.map((alternatives, index) => ({
      index, provenance: provenance(sourceId, `${pointer}/${index}`), alternatives: alternatives.map((reference, alternativeIndex) => {
        const location = `${pointer}/${index}/${alternativeIndex}`;
        const [modelAlias = "", selectedEffort = null] = reference.split("@");
        const model = modelByAlias.get(modelAlias) ?? null;
        const domain = model ? domainByAlias.get(model.domain) ?? null : null;
        const issues: TeamsSourceDiagnostic[] = [];
        if (!model) issues.push(diagnostic("unknown-model", location, `Unknown model alias ${modelAlias}`));
        if (model && !domain) issues.push(diagnostic("unknown-domain", location, `Unknown domain ${model.domain}`));
        if (model && ((model.supportedEfforts && (!selectedEffort || !model.supportedEfforts.includes(selectedEffort))) || (!model.supportedEfforts && selectedEffort))) {
          issues.push(diagnostic("invalid-effort", location, `Effort ${selectedEffort ?? "unspecified"} does not match declared efforts for ${modelAlias}`));
        }
        diagnostics.push(...issues);
        return { reference, modelAlias, selectedEffort, model, domain, diagnostics: issues, provenance: provenance(sourceId, location) };
      }),
    })) };
  });
  const bindings = Object.entries(input.bindings).map(([reference, chainName]) => {
    const pointer = `/bindings/${pointerPart(reference)}`;
    const identity = bindingIdentity(reference, input.schema_version);
    const chainResolved = Object.hasOwn(input.chains, chainName);
    if (!chainResolved) diagnostics.push(diagnostic("unknown-chain", pointer, `Unknown chain ${chainName}`));
    return { reference, ...identity, chainName, chainResolved, provenance: provenance(sourceId, pointer) };
  });
  for (const alias of input.unavailable.domains) {
    if (!domainByAlias.has(alias)) diagnostics.push(diagnostic("unknown-domain", "/unavailable/domains", `Unknown domain ${alias}`));
  }
  const accounts = new Set(domains.flatMap((domain) => domain.accounts));
  for (const account of input.unavailable.accounts) {
    if (!accounts.has(account)) diagnostics.push(diagnostic("unknown-account", "/unavailable/accounts", `Unknown account alias ${account}`));
  }
  const ruleReferences = [...input.rules.calibration_required, ...input.rules.vision_required,
    ...Object.keys(input.rules.independent_of), ...Object.values(input.rules.independent_of)];
  for (const reference of ruleReferences) {
    if (!Object.hasOwn(input.bindings, reference)) diagnostics.push(diagnostic("unknown-binding", "/rules", `Rule names unknown binding ${reference}`));
  }
  return { data: { kind: "fleet", schemaVersion: input.schema_version, domains, models, chains, bindings,
    declaredUnavailable: input.unavailable, rules: input.rules }, diagnostics };
}

export function projectTeamsRoster(raw: unknown, sourceId: string): TeamsRosterData {
  const input = rosterSchema.parse(raw);
  if (new Set(input.seats.map((seat) => seat.role_slot_id)).size !== input.seats.length) throw new Error("Duplicate core roster slot");
  return { kind: "roster", version: input.version, roleCatalog: input.role_catalog,
    seats: input.seats.map((seat, index) => ({ ...seat, provenance: provenance(sourceId, `/seats/${index}`) })) };
}

export function projectTeamsTemplates(raw: unknown, sourceId: string): { data: TeamsTemplatesData; diagnostics: TeamsSourceDiagnostic[] } {
  const input = packSchema.parse(raw);
  const identities = input.teams.map((team) => `${team.template_id}@${team.version}`);
  if (new Set(identities).size !== identities.length) throw new Error("Duplicate team template ID/version");
  const diagnostics: TeamsSourceDiagnostic[] = [];
  const templates = input.teams.map((team, index) => {
    const pointer = `/teams/${index}`;
    const slotIds = new Set(team.slots.map((slot) => slot.id));
    if (slotIds.size !== team.slots.length) throw new Error(`Duplicate template slot at ${pointer}`);
    for (const [slotIndex, slot] of team.slots.entries()) {
      if (!team.roles.some((entry) => entry.role.role === slot.role.role && entry.role.version === slot.role.version)) {
        diagnostics.push(diagnostic("unknown-role", `${pointer}/slots/${slotIndex}`, `Role ${slot.role.role}@${slot.role.version} is not declared`));
      }
    }
    for (const [handoffIndex, handoff] of team.handoffs.entries()) {
      if (!slotIds.has(handoff.from_slot) || !slotIds.has(handoff.to_slot)) {
        diagnostics.push(diagnostic("unknown-slot", `${pointer}/handoffs/${handoffIndex}`, "Handoff names an unknown slot"));
      }
    }
    return { id: team.template_id, version: team.version, name: team.name, roles: team.roles,
      slots: team.slots.map((slot, slotIndex) => ({ id: slot.id, role: slot.role,
        defaultRungs: slot.model_chain.rungs.map((rung) => ({ ...rung, effort: rung.effort ?? null })),
        skills: slot.skills, context: slot.context, mayEvaluate: slot.may_evaluate, mayWaive: slot.may_waive,
        waiverPolicy: slot.waiver_policy, autonomy: slot.autonomy ?? null,
        provenance: provenance(sourceId, `${pointer}/slots/${slotIndex}`) })),
      handoffs: team.handoffs, maxSuccessorDepth: team.max_successor_depth, maxHandoffDepth: team.max_handoff_depth,
      provenance: provenance(sourceId, pointer) };
  });
  return { data: { kind: "templates", templates }, diagnostics };
}
