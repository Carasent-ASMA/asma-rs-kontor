import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import type { PaseoAgentListResult } from "@getpaseo/client";
import type { KontorPluginConfig } from "./config";
import { PASEO_DIRECT_ORCHESTRATION, hasKontorLabels, parent, seatBinding } from "./labels";
import { CREATE_REQUEST_LABEL } from "../shared/session-create";

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

const TOOL = "kontor_fleet_policy_resolve";
const routeSchema = z.object({ rung: z.object({ provider: z.string(), model: z.string(), effort: z.string().optional() }), step: z.number(), sub_step: z.number(), vendor: z.string() });
const selectionSchema = z.object({
  provenance: z.object({ policy_hash: z.string(), schema_version: z.number(), binding_key: z.string(), chain: z.string() }),
  route_health: z.unknown(), selected: routeSchema.nullable(), considered: z.array(z.object({ route: routeSchema, excluded: z.string().optional() })),
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
  considered: (RouteView & { excluded: string | null })[];
  digest: string | null;
};
const view = (route: z.infer<typeof routeSchema>): RouteView =>
  ({ account: route.rung.provider, model: route.rung.model, effort: route.rung.effort ?? null, vendor: route.vendor, step: route.step, subStep: route.sub_step });
const without = (state: "refused" | "unknown", message: string): TakeoverRoute => ({ state, message, provenance: null, selected: null, considered: [], digest: null });

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
  return {
    state: blocked ? "blocked" : "selected",
    message: blocked ? "No route in the configured chain has fresh positive evidence on this host. Nothing will be created." : "The configured chain's first route with fresh positive evidence on this host.",
    provenance: { policyHash: parsed.provenance.policy_hash, bindingKey: parsed.provenance.binding_key, chain: parsed.provenance.chain },
    selected: parsed.selected && view(parsed.selected),
    considered: parsed.considered.map(({ route, excluded }) => ({ ...view(route), excluded: excluded ?? null })),
    digest: createHash("sha256").update(JSON.stringify(receipt)).digest("hex"),
  };
}
