import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import type { PaseoAgentListResult } from "@getpaseo/client";
import type { KontorPluginConfig } from "./config";
import { PASEO_DIRECT_ORCHESTRATION, hasKontorLabels, parent, seatBinding } from "./labels";
import { observeRefusal, type RefusalAgent, type RefusalObservation } from "../shared/provider-block";
import { CREATE_REQUEST_LABEL, featureValuesSchema, type FeatureValues } from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/**
 * The one ASMA-8641 takeover producer has two adapters, and every session has at most one of them. The native/direct
 * adapter is this plugin acting through the host SDK on the paired user's confirmation. The governed adapter is the
 * kontord lifecycle writer, unchanged and never reached from here. Only protected ownership evidence decides:
 * Kontor seat labels, the Paseo-direct orchestration label with its native owner, and the session-create-v1 request.
 * Title, role, model and an absent label are never evidence, and a conflicting or unverifiable claim has no adapter.
 */
export type TakeoverAdapter =
  /** No governance claim: the confirmed native operation is the ownership record. It grants no workflow role. */
  | { adapter: "direct"; basis: "ordinary"; owner: null; createRequest: null }
  /** Created by New session; its session-create-v1 request is the provenance. */
  | { adapter: "direct"; basis: "created"; owner: null; createRequest: string }
  /** A Paseo-direct seat and the native session that owns it; a takeover successor also names its creation request. */
  | { adapter: "direct"; basis: "orchestrated"; owner: string; createRequest: string | null }
  /** A kontord seat: only the governed lifecycle writer takes it over. */
  | { adapter: "governed"; seat: string }
  | { adapter: "refused"; reason: string };

const CONFLICT = "Ownership conflicts: this session carries both Kontor seat labels and direct-session labels. Nothing is taken over until its owner resolves that.";
export const GOVERNED_ELSEWHERE = "This is a Kontor-governed seat. Only its governed lifecycle writer can take it over; this screen performs no effect on it.";
const MALFORMED_REQUEST = "This session's creation request label is malformed, so its provenance cannot be verified.";
const refused = (reason: string): TakeoverAdapter => ({ adapter: "refused", reason });
const uuid = z.string().uuid();

export function takeoverAdapter(agent: Pick<Agent, "labels">): TakeoverAdapter {
  const labels = agent.labels ?? {}, orchestration = labels.orchestration, created = labels[CREATE_REQUEST_LABEL];
  if (hasKontorLabels(labels)) {
    if (orchestration !== undefined || created !== undefined) return refused(CONFLICT);
    // A recognisable governed claim that cannot be verified is never relabelled direct.
    const { seat } = seatBinding(labels);
    return seat ? { adapter: "governed", seat } : refused("This session carries Kontor labels but no seat identity, so its governed ownership cannot be verified. It is not treated as a direct session.");
  }
  if (orchestration !== undefined) {
    if (orchestration !== PASEO_DIRECT_ORCHESTRATION) return refused(`The orchestration label "${orchestration}" is not one this screen can verify an owner for.`);
    const owner = parent(agent);
    if (!owner) return refused("This Paseo-direct seat names no owning session, so its ownership cannot be verified.");
    if (created !== undefined && !uuid.safeParse(created).success) return refused(MALFORMED_REQUEST);
    return { adapter: "direct", basis: "orchestrated", owner, createRequest: created ?? null };
  }
  if (created !== undefined) return uuid.safeParse(created).success ? { adapter: "direct", basis: "created", owner: null, createRequest: created } : refused(MALFORMED_REQUEST);
  return { adapter: "direct", basis: "ordinary", owner: null, createRequest: null };
}

const SEGMENT = "[A-Za-z0-9][A-Za-z0-9._-]{0,127}";
/** A key a fleet policy binds; whether it is bound is the selector's answer, never this pattern's. */
export const BINDING_KEY = new RegExp(`^(?:(?:team|committee)/${SEGMENT}/${SEGMENT}|advisor/${SEGMENT}|leadership/[0-9a-f]{64}/${SEGMENT})$`);

/**
 * What a takeover review may ask the resolver for: the exact session and the configured binding associated with it.
 * Never an executable, path, tier, argument, environment, policy or route evidence: those are the server's.
 */
export const takeoverRouteInputSchema = z.object({ agentId: z.string().min(1).max(200), bindingKey: z.string().regex(BINDING_KEY).nullable() }).strict();
export type TakeoverRouteInput = z.infer<typeof takeoverRouteInputSchema>;

/** The resolver's positive route health, in its wire shape. The server builds it from host reads; a caller never supplies it. */
const instant = z.string().regex(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
export const routeHealthSchema = z.object({
  as_of: instant,
  max_age_seconds: z.number().int().min(1).max(3600),
  mode: z.string().min(1).optional(),
  observations: z.array(z.object({
    account: z.string().min(1), model: z.string().min(1), effort: z.enum(["off", "low", "medium", "high", "xhigh", "max", "ultra", "ultracode"]).optional(),
    mode: z.string().min(1).optional(), state: z.enum(["available", "unavailable", "unknown"]), observed_at: instant, revision: z.string().min(1),
  }).strict()).max(256),
}).strict();
export type RouteHealth = z.infer<typeof routeHealthSchema>;
/** Whole seconds in the resolver's canonical UTC form; flooring keeps every observation at or before its as_of. */
export const resolverInstant = (ms: number) => new Date(Math.floor(ms / 1000) * 1000).toISOString().replace(".000Z", "Z");

const ROUTE_EFFORTS = new Set(["off", "low", "medium", "high", "xhigh", "max", "ultra", "ultracode"]);
/** How long a host providers snapshot may admit a route; matches the takeover review preflight window. */
export const ROUTE_HEALTH_MAX_AGE_SECONDS = 300;

/** One host `providers.snapshot` read, without paid probes; the same surface session creation uses. */
export type ProviderRouteSnapshot = {
  requestId: string;
  generatedAt: string;
  entries: readonly {
    provider: string;
    enabled?: boolean;
    status?: string;
    models?: readonly { id: string; isSelectable?: boolean; thinkingOptions?: readonly { id: string }[] }[];
    modes?: readonly { id: string }[];
  }[];
};

function routeObservationState(entry: ProviderRouteSnapshot["entries"][number], mode: string | null): RouteHealth["observations"][number]["state"] {
  if (entry.enabled === false) return "unavailable";
  if (entry.enabled !== true) return "unknown";
  if (entry.status === "ready") {
    if (mode) {
      if (!entry.modes?.length) return "unknown";
      if (!entry.modes.some((candidate) => candidate.id === mode)) return "unknown";
    }
    return "available";
  }
  if (entry.status === "loading" || entry.status === "unknown") return "unknown";
  if (entry.status === "error" || entry.status === "unavailable") return "unavailable";
  return "unknown";
}

/** Configured native account aliases from operator fleet `domains.accounts`; not activation or availability proof. */
export type ConfiguredAccountCatalog = { accounts: ReadonlySet<string>; revision: string };

/** Builds the account-alias allowlist from trusted operator fleet domains; conflicting duplicates fail closed. */
export function configuredAccountCatalogFromFleet(domains: readonly { accounts: readonly string[] }[], revision: string): ConfiguredAccountCatalog | null {
  const list = domains.flatMap((domain) => domain.accounts);
  if (!list.length || new Set(list).size !== list.length) return null;
  return { accounts: new Set(list), revision };
}

function qualifiedSnapshotTiming(snapshot: ProviderRouteSnapshot, asOfMs: number): { observedAt: string; revision: string } | null {
  const requestId = snapshot.requestId.trim();
  if (!requestId) return null;
  const generatedMs = Date.parse(snapshot.generatedAt);
  if (!Number.isFinite(generatedMs) || generatedMs > asOfMs) return null;
  const observedAt = resolverInstant(generatedMs);
  if (Date.parse(observedAt) > asOfMs) return null;
  return { observedAt, revision: `${requestId}@${snapshot.generatedAt}` };
}

/** Maps the host provider snapshot into bounded per-route observations for the fleet selector. */
export function routeHealthFromProviderSnapshot(snapshot: ProviderRouteSnapshot, mode: string | null, catalog: ConfiguredAccountCatalog | null, asOfMs = Date.now()): RouteHealth {
  const as_of = resolverInstant(asOfMs);
  const timing = qualifiedSnapshotTiming(snapshot, asOfMs);
  const observedAt = timing?.observedAt ?? as_of;
  const revision = timing?.revision ?? "unqualified-snapshot";
  const snapshotQualified = timing !== null;
  const observations: RouteHealth["observations"] = [];
  for (const entry of snapshot.entries) {
    const accountProven = catalog !== null && catalog.accounts.has(entry.provider);
    const readiness = routeObservationState(entry, mode);
    const state: RouteHealth["observations"][number]["state"] = !snapshotQualified || !accountProven ? "unknown"
      : readiness === "available" ? "available" : readiness;
    const models = (entry.models ?? []).filter((model) => model.isSelectable !== false);
    for (const model of models) {
      type Effort = NonNullable<RouteHealth["observations"][number]["effort"]>;
      const efforts = (model.thinkingOptions ?? []).flatMap((option) => ROUTE_EFFORTS.has(option.id) ? [option.id as Effort] : []);
      const variants: (Effort | undefined)[] = efforts.length ? efforts : [undefined];
      for (const effort of variants) {
        if (observations.length >= 256) break;
        observations.push({
          account: entry.provider, model: model.id, ...(effort ? { effort } : {}), ...(mode ? { mode } : {}),
          state, observed_at: observedAt, revision,
        });
      }
    }
  }
  return { as_of, max_age_seconds: ROUTE_HEALTH_MAX_AGE_SECONDS, ...(mode ? { mode } : {}), observations };
}

function observationMatchesRefusal(observation: RouteHealth["observations"][number], refusal: RefusalObservation, mode: string | null): boolean {
  if (observation.account !== refusal.route.provider) return false;
  if (mode && observation.mode !== mode) return false;
  if (refusal.route.model && observation.model !== refusal.route.model) return false;
  return true;
}

/**
 * Canonical session-local refusal on the predecessor already read in review: exact account/model correlation only.
 * Not quota-window proof, timeline recovery, or persisted refusal memory.
 */
export function sessionLocalRefusalForTakeover(agent: RefusalAgent & { model?: string | null; runtimeInfo?: { model?: string | null } | null }, mode: string | null, observedAt: string): RefusalObservation | null {
  const refusal = observeRefusal(agent, observedAt);
  if (!refusal) return null;
  const agentModel = agent.runtimeInfo?.model ?? agent.model ?? null;
  if (agentModel && refusal.route.model && agentModel !== refusal.route.model) return null;
  if (mode && refusal.route.model && agentModel && agentModel !== refusal.route.model) return null;
  return refusal;
}

/**
 * Known correlated refusal wins over generic snapshot readiness; it never clears from snapshot alone.
 * Not quota-window proof; no reads beyond the agent snapshot already used in review.
 */
export function joinSessionLocalRefusalToRouteHealth(health: RouteHealth, refusal: RefusalObservation | null, mode: string | null = health.mode ?? null): RouteHealth {
  if (!refusal) return health;
  const refusalState: RouteHealth["observations"][number]["state"] = refusal.kind === "unknown" ? "unknown" : "unavailable";
  const tag = `session-refusal:${refusal.kind}@${refusal.observedAt}`;
  let changed = false;
  const observations = health.observations.map((observation) => {
    if (!observationMatchesRefusal(observation, refusal, mode)) return observation;
    changed = true;
    const state: RouteHealth["observations"][number]["state"] = observation.state === "unavailable" || refusalState === "unavailable" ? "unavailable" : refusalState;
    return { ...observation, state, revision: `${observation.revision}+${tag}` };
  });
  return changed ? { ...health, observations } : health;
}

/** Semantic route/refusal facts for review/apply pinning; transport requestId, revision and observation clocks are excluded. */
export function routeEvidenceSemanticPayload(health: RouteHealth, refusal: RefusalObservation | null, catalogRevision: string | null) {
  return {
    catalogRevision,
    maxAgeSeconds: health.max_age_seconds,
    mode: health.mode ?? null,
    observations: health.observations.map(({ account, model, effort, mode, state }) =>
      ({ account, model, effort: effort ?? null, mode: mode ?? null, state })),
    refusal: refusal ? { kind: refusal.kind, failureAt: refusal.failureAt, route: refusal.route, message: refusal.message } : null,
  };
}

/** True when every observation timestamp is at or before asOf and within max_age_seconds. */
export function routeEvidenceFreshEnough(health: RouteHealth, asOfMs = Date.now()): boolean {
  const ceiling = health.max_age_seconds * 1000;
  for (const observation of health.observations) {
    const observed = Date.parse(observation.observed_at);
    if (!Number.isFinite(observed) || observed > asOfMs || asOfMs - observed > ceiling) return false;
  }
  return true;
}

/** Pins semantic evidence identity and freshness policy, not transport requestId or raw observation clocks. */
export function routeEvidenceDigest(health: RouteHealth, refusal: RefusalObservation | null, catalogRevision: string | null): string {
  return createHash("sha256").update(JSON.stringify(routeEvidenceSemanticPayload(health, refusal, catalogRevision))).digest("hex");
}

const takeoverSpecPin = (spec: { effectValidity?: unknown }) => {
  const { effectValidity, ...rest } = spec;
  return rest;
};

/** Continuity facts apply pins; observation time is freshness-only at assess/apply. */
export type TakeoverContinuitySemantics = Pick<
  import("./session-lifecycle.js").TakeoverContinuity,
  "agentId" | "requestId" | "revision" | "surface" | "wakes" | "wakeHost" | "owner"
>;
export function takeoverContinuitySemantics(receipt: TakeoverContinuitySemantics): TakeoverContinuitySemantics {
  return { agentId: receipt.agentId, requestId: receipt.requestId, revision: receipt.revision, surface: receipt.surface, wakes: receipt.wakes, wakeHost: receipt.wakeHost ?? null, owner: receipt.owner };
}
export function takeoverContinuitySemanticsMatch(left: TakeoverContinuitySemantics, right: TakeoverContinuitySemantics): boolean {
  return isDeepStrictEqual(takeoverContinuitySemantics(left), takeoverContinuitySemantics(right));
}

/** Apply recomputes pins; transport-only evidence refresh is allowed when semantic identity and freshness hold. */
export function takeoverPinnedMatches(reviewed: {
  predecessor: string; owner: TakeoverAdapter; ownerRevision: string | null; created: string | null; bindingKey: string;
  route: Pick<TakeoverRoute, "provenance" | "selected" | "considered" | "validity">; evidence: string; continuity: unknown;
  spec: { effectValidity?: unknown }; sourceSettings?: string | null;
}, current: {
  predecessor: string; owner: TakeoverAdapter; ownerRevision: string | null; created: string | null; bindingKey: string;
  route: Pick<TakeoverRoute, "provenance" | "selected" | "considered" | "validity">; evidence: string; continuity: unknown;
  spec: { effectValidity?: unknown }; sourceSettings?: string | null;
}): boolean {
  if (reviewed.predecessor !== current.predecessor || reviewed.bindingKey !== current.bindingKey) return false;
  if (reviewed.created !== current.created || reviewed.ownerRevision !== current.ownerRevision) return false;
  if ((reviewed.sourceSettings ?? null) !== (current.sourceSettings ?? null)) return false;
  if (!isDeepStrictEqual(reviewed.owner, current.owner)) return false;
  if (!takeoverContinuitySemanticsMatch(reviewed.continuity as TakeoverContinuitySemantics, current.continuity as TakeoverContinuitySemantics)) return false;
  if (!isDeepStrictEqual(reviewed.route.provenance, current.route.provenance)) return false;
  if (!isDeepStrictEqual(reviewed.route.selected, current.route.selected)) return false;
  if (!isDeepStrictEqual(reviewed.route.considered, current.route.considered)) return false;
  if (reviewed.evidence !== current.evidence) return false;
  return isDeepStrictEqual(takeoverSpecPin(reviewed.spec), takeoverSpecPin(current.spec));
}

/**
 * The source's listed model settings as create values. The snapshot exposes only these settings; it does not expose
 * MCP servers or provider-native options, so those are never guessed. A select with no current value is omitted, and
 * the result is validated against the create contract before it can be pinned.
 */
export function sourceFeatureValues(agent: Pick<Agent, "features">): { values: FeatureValues | null; problem: string | null } {
  const values: FeatureValues = {};
  for (const feature of agent.features ?? []) {
    if (feature.type === "toggle") values[feature.id] = feature.value;
    else if (feature.value !== null) values[feature.id] = feature.value;
  }
  const parsed = featureValuesSchema.safeParse(values);
  if (!parsed.success) return { values: null, problem: "This session's listed model settings do not match the host's create contract, so its configuration cannot be carried." };
  return { values: Object.keys(parsed.data).length ? parsed.data : null, problem: null };
}

const TOOL = "kontor_fleet_policy_resolve";
const validitySchema = z.object({ observed_at: instant, valid_until: instant }).strict();
const routeSchema = z.object({ rung: z.object({ provider: z.string(), model: z.string(), effort: z.string().optional() }), step: z.number(), sub_step: z.number(), vendor: z.string() });
const selectionSchema = z.object({
  provenance: z.object({ policy_hash: z.string(), schema_version: z.number(), binding_key: z.string(), chain: z.string() }),
  route_health: z.unknown(), selected: routeSchema.nullable(), considered: z.array(z.object({ route: routeSchema, excluded: z.string().optional() })),
  selected_validity: validitySchema.optional(),
});
const documentSchema = z.union([
  z.object({ tool: z.literal(TOOL), status: z.literal(200), body: selectionSchema }),
  z.object({ tool: z.literal(TOOL), status: z.literal(409), body: z.object({ code: z.literal("placement_blocked"), selection: selectionSchema }) }),
  z.object({ tool: z.literal(TOOL), code: z.string(), rule: z.string(), dispatched: z.literal(false) }),
]);

type RouteView = { account: string; model: string; effort: string | null; vendor: string; step: number; subStep: number };
/**
 * The minimum a review shows: the chain's provenance, the chosen route and why each route was passed over. No policy
 * document, account secret, provider home or vendor message. `digest` pins the selection receipt exactly as printed.
 */
export type TakeoverRoute = {
  state: "selected" | "blocked" | "refused" | "unknown";
  message: string;
  provenance: { policyHash: string; bindingKey: string; chain: string } | null;
  selected: RouteView | null;
  validity: { observedAt: string; validUntil: string } | null;
  considered: (RouteView & { excluded: string | null })[];
  digest: string | null;
};
const view = (route: z.infer<typeof routeSchema>): RouteView =>
  ({ account: route.rung.provider, model: route.rung.model, effort: route.rung.effort ?? null, vendor: route.vendor, step: route.step, subStep: route.sub_step });
const without = (state: "refused" | "unknown", message: string): TakeoverRoute => ({ state, message, provenance: null, selected: null, validity: null, considered: [], digest: null });

/** A model the operator picked for this one takeover; Auto uses the configured chain instead. */
export const explicitTakeoverRouteSchema = z.object({
  provider: z.string().min(1).max(200), model: z.string().min(1).max(200), effort: z.string().min(1).max(200).nullable(),
  featureValues: featureValuesSchema.optional(),
}).strict();
export type ExplicitTakeoverRoute = z.infer<typeof explicitTakeoverRouteSchema>;
export type ExplicitTakeoverSelection = { route: TakeoverRoute; featureValues?: ExplicitTakeoverRoute["featureValues"] };

/**
 * Validates an explicit model selection against the same host provider snapshot session creation reads. The source's
 * own permission mode must be one the provider lists; no read-only or other invented mode is substituted. A missing or
 * unlisted mode is a concrete host-capability answer, not a role or state prohibition.
 */
export function explicitTakeoverRoute(health: RouteHealth, choice: ExplicitTakeoverRoute, mode: string, nowMs = Date.now()): ExplicitTakeoverSelection | { problem: string } {
  const routeName = `${choice.provider}/${choice.model}`;
  const observations = health.observations.filter((observation) => observation.account === choice.provider && observation.model === choice.model
    && (choice.effort === null || observation.effort === choice.effort));
  if (!observations.length) return { problem: `The host does not offer ${routeName}${choice.effort ? ` at ${choice.effort}` : ""} on this checkout and permission mode. Nothing is chosen.` };
  if (health.mode && health.mode !== mode) return { problem: `The host route evidence names permission mode ${health.mode}, not this session's ${mode}. Nothing is chosen.` };
  const available = observations.find((observation) => observation.state === "available");
  if (!available) return { problem: `The host lists ${routeName}${choice.effort ? ` at ${choice.effort}` : ""} but reports it unavailable or unobserved (${[...new Set(observations.map((entry) => entry.state))].join(", ")}). Nothing is chosen.` };
  const observed = Date.parse(available.observed_at), end = observed + health.max_age_seconds * 1000;
  // The observation's own clock, not the resolver's as_of, decides whether this picked route is fresh.
  if (!Number.isFinite(observed) || observed > nowMs) return { problem: "The host route observation for the selected model is dated in the future or has no valid time. Nothing is chosen." };
  if (nowMs - observed > health.max_age_seconds * 1000) return { problem: `The host route observation for ${routeName} is ${Math.round((nowMs - observed) / 1000)} s old, beyond its ${health.max_age_seconds} s window. Nothing is chosen.` };
  if (!available.revision.trim()) return { problem: "The host route observation for the selected model has no revision. Nothing is chosen." };
  const selected: RouteView = { account: choice.provider, model: choice.model, effort: choice.effort, vendor: choice.provider, step: 0, subStep: 0 };
  // Semantic identity only: a transport requestId or a later observation clock does not change which route was picked.
  const semantic = { kind: "explicit", account: selected.account, model: selected.model, effort: selected.effort, mode };
  const digest = createHash("sha256").update(JSON.stringify(semantic)).digest("hex");
  const provenance = { policyHash: digest, bindingKey: "explicit", chain: `selected ${routeName}` };
  return {
    route: {
      state: "selected",
      message: `Takes over on the selected ${routeName}${choice.effort ? ` at ${choice.effort}` : ""}, using this session's permission mode ${mode}.`,
      provenance, selected,
      validity: { observedAt: resolverInstant(observed), validUntil: resolverInstant(end) },
      considered: observations.filter((observation) => observation !== available).map((observation) => ({
        account: observation.account, model: observation.model, effort: observation.effort ?? null, vendor: observation.account, step: 0, subStep: 0,
        excluded: `route_${observation.state}`,
      })),
      digest,
    },
    ...(choice.featureValues && Object.keys(choice.featureValues).length ? { featureValues: choice.featureValues } : {}),
  };
}


/** Runs the command and answers its one document, whatever exit code carries it; a signal, spawn or size failure rejects. */
export type ResolverRun = (command: string, args: string[]) => Promise<string>;
const RESOLVER_BUDGET_MS = 10_000;
const RESOLVER_OUTPUT_LIMIT = 256 * 1024;
export const runResolver: ResolverRun = (command, args) => new Promise((settle, reject) => {
  execFile(command, args, { env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" }, timeout: RESOLVER_BUDGET_MS, killSignal: "SIGKILL", maxBuffer: RESOLVER_OUTPUT_LIMIT, encoding: "utf8" },
    (error, stdout) => error && typeof error.code !== "number" ? reject(error) : settle(stdout));
});

/**
 * The plugin's one Operator read: the local `kontor fleet-policy-resolve` for one direct session's configured binding,
 * under host route evidence. Only the confirmed native takeover's review and apply call it. The command, state root
 * and argument vector are fixed here; `--tier operator` declares the local gate's tier for this child alone and is
 * not identity proof. No daemon, credential, network or activation write is involved.
 */
export async function resolveTakeoverRoute(config: Pick<KontorPluginConfig, "cliCommand" | "stateRoot">, owner: TakeoverAdapter, input: unknown, health: RouteHealth, run: ResolverRun = runResolver): Promise<TakeoverRoute> {
  if (owner.adapter === "governed") return without("refused", GOVERNED_ELSEWHERE);
  if (owner.adapter === "refused") return without("refused", owner.reason);
  const request = takeoverRouteInputSchema.safeParse(input);
  if (!request.success) return without("refused", "The takeover request names something a takeover cannot take. Nothing was resolved.");
  const key = request.data.bindingKey;
  if (!key) return without("refused", "No configured chain is associated with this session. Choose an existing configured profile for it in the review; a title, role or model is never used to infer one.");
  const evidence = routeHealthSchema.safeParse(health);
  if (!evidence.success) return without("refused", "The host's route evidence is malformed. Nothing was resolved.");
  const args = ["--state-root", config.stateRoot, "--tier", "operator", "fleet-policy-resolve", `--binding-key=${key}`, `--route-health=${JSON.stringify(evidence.data)}`];
  let raw: unknown;
  try { raw = JSON.parse(await run(config.cliCommand, args)); }
  catch (error) { return without("unknown", `The local fleet selection could not be read: ${errorText(error)}. No route is chosen.`); }
  const document = documentSchema.safeParse(raw);
  if (!document.success) return without("unknown", "The local fleet selection answered with an unrecognised document. No route is chosen.");
  if (!("status" in document.data)) return without("refused", `The local fleet selection refused: ${document.data.code} (${document.data.rule}).`);
  const blocked = document.data.status === 409, parsed = document.data.status === 409 ? document.data.body.selection : document.data.body;
  // The digest covers the receipt as printed, eligibility and policy exclusions included, not this parsed view.
  const body = (raw as { body: { selection?: unknown } }).body, receipt = blocked ? body.selection : body;
  // The receipt must be the answer to this request: its binding and the exact evidence sent.
  if (parsed.provenance.binding_key !== key || !isDeepStrictEqual(parsed.route_health, evidence.data)) return without("unknown", "The local fleet selection answered for another request. No route is chosen.");
  if (!blocked && !parsed.selected) return without("unknown", "The local fleet selection reported success without a route. No route is chosen.");
  const validity = parsed.selected_validity;
  if (!blocked && (!validity || !Number.isFinite(Date.parse(validity.observed_at)) || !Number.isFinite(Date.parse(validity.valid_until))
    || Date.parse(validity.valid_until) < Date.parse(validity.observed_at))) return without("unknown", "The local fleet selection did not bind a valid evidence deadline. No route is chosen.");
  return {
    state: blocked ? "blocked" : "selected",
    message: blocked ? "No route in the configured chain has fresh positive evidence on this host. Nothing will be created." : "The configured chain's first route with fresh positive evidence on this host.",
    provenance: { policyHash: parsed.provenance.policy_hash, bindingKey: parsed.provenance.binding_key, chain: parsed.provenance.chain },
    selected: parsed.selected && view(parsed.selected),
    validity: !blocked && validity ? { observedAt: validity.observed_at, validUntil: validity.valid_until } : null,
    considered: parsed.considered.map(({ route, excluded }) => ({ ...view(route), excluded: excluded ?? null })),
    digest: createHash("sha256").update(JSON.stringify(receipt)).digest("hex"),
  };
}
