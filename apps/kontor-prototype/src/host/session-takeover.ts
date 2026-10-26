import { applyTakeover, checkTakeover, reviewTakeover, type TakeoverOutcome, type TakeoverReview, type TakeoverReviewInput } from "../../../paseo-plugin/shared/session-lifecycle";
import { CREATE_REQUEST_LABEL, createChoiceProblem, featureDefault, featureValuesProblem, inheritedModeProblem, keptFeatureValues, settingLabel, type CreateFeature, type CreateProvider, type FeatureValues } from "../../../paseo-plugin/shared/session-create";
import { summaryRevisionSchema, type SummaryRevision } from "../../../paseo-plugin/shared/session-navigator";
import type { FixtureAgent, FixtureSession } from "../fixtures/sessions";
import { TAKEOVER_SIMULATED, type fixtureNavigator } from "./session-navigator";
import { listedFeatures, PROVIDERS, settingsReadback } from "./session-create";
import { logHostEvent } from "./event-log";

/** What a confirmed takeover of one seat does; `no-model` blocks its review instead, so there is nothing to confirm. */
export const TAKEOVER_SIMS = ["success", "no-model", "launch-fails", "unknown-effect", "refuse-preflight"] as const;
export type TakeoverSim = (typeof TAKEOVER_SIMS)[number];
/**
 * One outcome for every seat; `sequence`: each seat reviewed for the first time takes the next outcome in TAKEOVER_SIMS
 * order; `limited`: every review states a limit the operator was not shown before, so it needs their review.
 */
export type TakeoverSimMode = TakeoverSim | "sequence" | "limited";
export const isTakeoverSimMode = (value: unknown): value is TakeoverSimMode => value === "sequence" || value === "limited" || (TAKEOVER_SIMS as readonly unknown[]).includes(value);

const KEPT = "Nothing was sent; the original is kept.";
/** The extra limit `limited` states, worded as a host would state a real one. */
export const UNDISCLOSED_LIMIT = "SIMULATED: this seat's supervisor is not told about the new session; tell it yourself.";
type RouteView = NonNullable<TakeoverReview["route"]>["considered"][number];
/** A chain candidate with the saved settings it keeps and what it cannot keep. */
type Candidate = RouteView & { values: FeatureValues; limits: string[] };
/**
 * What a ready review was decided on, compared again at confirmation: the seat's title, workspace and checkout its
 * successor takes, its route and effort, the mode it keeps, the settings it carries and the limits it states.
 */
type Basis = Record<"title" | "workspace" | "checkout" | "route" | "effort" | "permission mode" | "saved settings" | "limits", string | null>;
type Stored = { review: TakeoverReview; requestId: string; values: FeatureValues; summary: SummaryRevision | null; route: TakeoverReviewInput["route"] | null; basis: Basis };

export type TakeoverDeps = {
  get(): FixtureSession[];
  update(agentId: string, patch: Partial<FixtureAgent>, note: string): void;
  /** Stores a successor seat as the host keeps it; the readback reads the stored seat, never the request. */
  add(row: FixtureSession): void;
  navigator: Pick<ReturnType<typeof fixtureNavigator>, "invoke" | "pin">;
  mode?: TakeoverSimMode;
  providers?: readonly CreateProvider[];
};

/**
 * Page-memory simulation of the three takeover RPCs. Review pins a route and issues a fresh digest per revision; apply
 * saves a compact summary through the navigator, archives the predecessor, reads that back, and only then creates one
 * successor in the same title, workspace, checkout and mode, verifies its stored seat and pins the summary on it.
 * Children are never archived. An outcome is recorded per review, so a repeated confirmation never repeats an effect.
 * A simulated failure fires on a seat's first confirmation only, so a fresh review recovers; `no-model` stays blocked.
 */
export function fixtureTakeover({ get, update, add, navigator, mode = "success", providers = PROVIDERS }: TakeoverDeps) {
  const reviews = new Map<string, Stored>(), outcomes = new Map<string, TakeoverOutcome>();
  // The simulated outcome each seat drew at its first review, seats whose one-time failure fired, and archived seats whose takeover stopped before a successor.
  const assigned = new Map<string, TakeoverSim>(), spent = new Set<string>(), partials = new Map<string, string>();
  const find = (id: string) => get().find(({ agent }) => agent.id === id) ?? null;
  const digest = () => Array.from(crypto.getRandomValues(new Uint8Array(32)), (byte) => byte.toString(16).padStart(2, "0")).join("");
  const modeOf = (agent: FixtureAgent) => agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null;
  /** The effort the seat runs with: effective, else configured, else what its runtime last reported. */
  const effortOf = (agent: FixtureAgent) => agent.effectiveThinkingOptionId ?? agent.thinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null;
  /** The settings the seat stores away from their defaults: Speed Fast or Plan on, never Normal or off. */
  const savedValues = (agent: FixtureAgent): FeatureValues => Object.fromEntries((agent.features ?? []).flatMap((feature): [string, string | boolean][] =>
    feature.type === "toggle" ? feature.value ? [[feature.id, true]] : [] : feature.value !== null && !feature.options.find((option) => option.id === feature.value)?.isDefault ? [[feature.id, feature.value]] : []));

  /** Why this seat cannot be taken over now, or null; an archived seat may only finish its own recorded partial takeover. */
  function refusal(agent: FixtureAgent): string | null {
    if (agent.archivedAt && !partials.has(agent.id)) return "SIMULATED: this session is archived; take over never revives an archived session. Nothing was sent.";
    if (agent.status === "running" || agent.status === "initializing") return `SIMULATED: this session is running. Stop it in the session, then take it over. ${KEPT}`;
    if (!agent.workspaceId) return `SIMULATED: this session has no workspace on this host. ${KEPT}`;
    return null;
  }
  /**
   * The seat's chain: its own route when the catalog lists it, then the catalog's Codex default. Each candidate keeps
   * the seat's permission mode only on the seat's own provider, and carries its saved effort and settings where the
   * model offers them; what it cannot keep is a limit, and a candidate with one is skipped while one without stays.
   */
  function chain(agent: FixtureAgent): Candidate[] {
    const codex = providers.find((entry) => entry.id === "codex"), effort = effortOf(agent), saved = savedValues(agent);
    const own = providers.find((entry) => entry.id === agent.provider)?.models.some((entry) => entry.id === agent.model) ? [{ provider: agent.provider, model: agent.model! }] : [];
    const rungs = [...own, ...codex?.defaultModel ? [{ provider: "codex", model: codex.defaultModel }] : []]
      .filter((rung, index, all) => all.findIndex((entry) => entry.provider === rung.provider && entry.model === rung.model) === index);
    const candidates = rungs.map((rung, index): Candidate => {
      const provider = providers.find((entry) => entry.id === rung.provider)!, model = provider.models.find((entry) => entry.id === rung.model)!;
      const runs = model.efforts.some((entry) => entry.id === effort) ? effort : model.defaultEffort ?? model.efforts[0]?.id ?? null;
      const { kept, dropped } = keptFeatureValues(listedFeatures(provider.id, model.id) ?? [], saved, agent.features ?? []);
      const limits = [effort && runs !== effort ? `${model.label} does not offer the seat's effort ${effort}; it would run at ${runs ?? "no effort setting"}.` : null,
        dropped.length ? `${model.label} does not offer the seat's ${dropped.join(", ")}; the new session would run without ${dropped.length === 1 ? "it" : "them"}.` : null].filter((line): line is string => line !== null);
      return { account: provider.id, model: model.id, effort: runs, vendor: provider.label, step: index + 1, subStep: 1, values: kept, limits,
        excluded: assigned.get(agent.id) === "no-model" ? "SIMULATED: no fresh positive evidence for this route." : !provider.available ? `${provider.label}: ${provider.note}`
          : inheritedModeProblem(providers, agent.provider, modeOf(agent), provider.id) };
    });
    // A candidate that keeps everything wins; one that cannot is skipped for it, with what it cannot keep as the reason.
    const whole = candidates.some((entry) => entry.excluded === null && !entry.limits.length);
    return candidates.map((entry) => ({ ...entry, excluded: entry.excluded ?? (whole && entry.limits.length ? entry.limits.join(" ") : null) }));
  }
  function assess(agent: FixtureAgent, route: TakeoverReviewInput["route"]): { state: "ready" | "blocked" | "refused"; message: string; considered: RouteView[]; selected: Omit<RouteView, "excluded"> | null; values: FeatureValues; limits: string[] } {
    const refused = refusal(agent), none = { considered: [], selected: null, values: {}, limits: [] };
    if (refused) return { state: "refused", message: refused, ...none };
    let considered: Candidate[];
    if (route) {
      // Permissions are inherited, never chosen: the seat's own mode is checked against the chosen provider before anything else.
      const provider = providers.find((entry) => entry.id === route.provider);
      const problem = inheritedModeProblem(providers, agent.provider, modeOf(agent), route.provider)
        ?? createChoiceProblem(providers.map((entry) => ({ ...entry, modes: [] })), { ...route, modeId: null })
        ?? (route.featureValues ? featureValuesProblem(listedFeatures(route.provider, route.model) ?? [], route.featureValues) : null);
      if (problem) return { state: "refused", message: `SIMULATED: ${problem} ${KEPT}`, ...none };
      considered = [{ account: route.provider, model: route.model, effort: route.effort, vendor: provider!.label, step: 1, subStep: 1, excluded: null, values: route.featureValues ?? {}, limits: [] }];
    } else considered = chain(agent);
    const views = considered.map(({ values: _values, limits: _limits, ...view }) => view), first = considered.find((entry) => entry.excluded === null);
    if (!first) {
      // Only permissions in the way: refused, since no route can keep them. Permissions and availability: blocked, with every reason.
      const permissions = considered.filter((entry) => entry.excluded === inheritedModeProblem(providers, agent.provider, modeOf(agent), entry.account)).length;
      if (!permissions) return { state: "blocked", message: `SIMULATED: no model in this seat's chain is available. ${KEPT}`, ...none, considered: views };
      return { state: permissions === considered.length ? "refused" : "blocked", ...none, considered: views,
        message: `SIMULATED: no route in this seat's chain is available and keeps its permissions. ${[...new Set(considered.map((entry) => entry.excluded))].join(" ")} ${KEPT}` };
    }
    const { excluded: _available, values, limits, ...selected } = first;
    const label = providers.find((entry) => entry.id === selected.account)?.models.find((entry) => entry.id === selected.model)?.label ?? selected.model;
    // Settings the seat never reported are unknown, not off: Auto says which defaults the new session would run at. A chosen route sets its own.
    const listed = route || agent.features ? [] : listedFeatures(selected.account, selected.model) ?? [];
    const defaults = listed.map((feature) => settingLabel(listed, feature.id, featureDefault(feature) ?? "unset")).join(", ");
    const stated = listed.length ? [...limits, `This seat's settings were not reported, so the new session would run at ${label}'s defaults: ${defaults}.`] : limits;
    const on = `${label} (${route ? "chosen for this takeover only" : "Auto: first available in the seat's chain"})`;
    return { state: "ready", considered: views, selected, values, limits: stated, message: partials.has(agent.id)
      ? `SIMULATED: finishes the earlier takeover on ${on}. The previous session is already archived, so only the new session is created.`
      : `SIMULATED: takes over on ${on}. Compact context is saved before archive.` };
  }

  function review(raw: unknown): TakeoverReview {
    const input = reviewTakeover.input.parse(raw);
    const prior = input.reviewId ? reviews.get(input.reviewId) : undefined;
    if (input.reviewId && (!prior || prior.review.agentId !== input.agentId || prior.review.revision !== input.revision)) throw new Error("SIMULATED: this is not the current revision of this review. Review the session again.");
    if (prior && outcomes.has(prior.review.reviewId)) throw new Error("SIMULATED: this review was already confirmed. Check its outcome instead.");
    const agent = find(input.agentId)?.agent;
    if (!agent) throw new Error(`SIMULATED: no session ${input.agentId} on this host.`);
    if (!assigned.has(agent.id)) assigned.set(agent.id, mode === "sequence" ? TAKEOVER_SIMS[assigned.size % TAKEOVER_SIMS.length]! : mode === "limited" ? "success" : mode);
    const found = assess(agent, input.route ?? null), ready = found.selected, requestId = prior?.requestId ?? crypto.randomUUID();
    const view = reviewTakeover.output.parse({
      reviewId: prior?.review.reviewId ?? crypto.randomUUID(), revision: (prior?.review.revision ?? 0) + 1, agentId: agent.id, checkedAt: new Date().toISOString(),
      state: found.state, message: found.message,
      predecessor: { title: agent.title, workspaceId: agent.workspaceId, cwd: agent.cwd, provider: agent.provider, model: agent.runtimeInfo?.model ?? agent.model ?? null,
        effort: effortOf(agent), mode: modeOf(agent), status: agent.status },
      ownership: "ordinary", association: { bindingKey: "fixture-chain", source: "recorded", choices: [] },
      route: found.considered.length ? { policyHash: "fixture", bindingKey: "fixture-chain", chain: "SIMULATED chain", selected: ready, considered: found.considered } : null,
      // The successor keeps the title, workspace, checkout and the permission mode its provider was checked to list; only the route is new.
      successor: ready && { requestId, title: agent.title, workspaceId: agent.workspaceId!, cwd: agent.cwd, provider: ready.account, model: ready.model, effort: ready.effort, mode: modeOf(agent), persona: null },
      limits: [TAKEOVER_SIMULATED, ...found.limits, ...ready && mode === "limited" ? [UNDISCLOSED_LIMIT] : []], digest: ready ? digest() : null,
    } satisfies TakeoverReview);
    reviews.set(view.reviewId, { review: view, requestId, values: found.values, summary: null, route: input.route ?? null, basis: basis(agent, found) });
    logHostEvent("simulated.takeover", `${agent.id}: review r${view.revision} ${view.state}`);
    return structuredClone(view);
  }

  const basis = (agent: FixtureAgent, found: ReturnType<typeof assess>): Basis => ({ title: agent.title, workspace: agent.workspaceId ?? null, checkout: agent.cwd,
    route: found.selected && `${found.selected.account}/${found.selected.model}`, effort: found.selected?.effort ?? null,
    "permission mode": modeOf(agent), "saved settings": JSON.stringify(found.values), limits: found.limits.join("\n") });
  /** What changed since this review was decided, read with the same assessment that decided it; empty when nothing material did. */
  const moved = (agent: FixtureAgent, stored: Stored) => {
    const now = basis(agent, assess(agent, stored.route));
    return (Object.keys(now) as (keyof Basis)[]).filter((key) => now[key] !== stored.basis[key]);
  };

  const outcome = (review: TakeoverReview, state: TakeoverOutcome["state"], phase: TakeoverOutcome["phase"], message: string, successor: TakeoverOutcome["successor"], next: TakeoverOutcome["next"]): TakeoverOutcome =>
    ({ reviewId: review.reviewId, revision: review.revision, agentId: review.agentId, state, phase, message, successor, next, checkedAt: new Date().toISOString() });
  function record(review: TakeoverReview, result: TakeoverOutcome): TakeoverOutcome {
    outcomes.set(review.reviewId, applyTakeover.output.parse(result));
    logHostEvent("simulated.takeover", `${review.agentId}: ${result.state} at ${result.phase}`);
    return structuredClone(result);
  }
  /** Reads the successor this review's request created back from its stored seat; null while none carries the request. */
  function settle({ review, requestId, values, summary }: Stored): TakeoverOutcome | null {
    const expected = review.successor!, seat = get().find(({ agent }) => agent.labels[CREATE_REQUEST_LABEL] === requestId)?.agent;
    if (!seat) return null;
    const named = { requestId, agentId: seat.id, title: seat.title };
    const actual = { title: seat.title, workspaceId: seat.workspaceId, cwd: seat.cwd, provider: seat.provider, model: seat.model, effort: seat.effectiveThinkingOptionId ?? null, mode: seat.currentModeId ?? null };
    const readback = settingsReadback(seat, values);
    // A mode that does not read back is never a match, even against one that was not reported.
    const differences = [...(Object.keys(actual) as (keyof typeof actual)[]).filter((key) => actual[key] !== expected[key] || (key === "mode" && actual.mode === null)).map((key) => `${key} reads ${actual[key] ?? "not reported"}.`), ...readback.differences];
    if (differences.length) return outcome(review, "partial", "verify", `SIMULATED: the new session was created, but its stored seat reads back differently, so it was not released: ${differences.join(" ")} The previous session is archived; the original was not kept.`, named, "none");
    if (summary) navigator.pin(seat.id, summary);
    partials.delete(review.agentId);
    logHostEvent("simulated.takeover", `${seat.id}: read back as requested; holds the task of ${review.agentId}`);
    return outcome(review, "succeeded", "done", ["SIMULATED: continuing in a new session; the previous session is archived.", readback.applied.length ? `Settings read back: ${readback.applied.join(", ")}.` : null].filter(Boolean).join(" "), named, "none");
  }

  async function apply(raw: unknown): Promise<TakeoverOutcome> {
    const input = applyTakeover.input.parse(raw), stored = reviews.get(input.reviewId);
    if (!stored) throw new Error(`SIMULATED: no takeover review ${input.reviewId} on this host. Review the session again.`);
    const { review } = stored, agentId = review.agentId;
    // A confirmation that does not match the current ready review is refused unrecorded, so it cannot spend the review.
    if (input.revision !== review.revision || input.digest !== review.digest) return outcome(review, "refused", "preflight", "SIMULATED: this confirmation does not match the current ready review. Nothing was done; the original is kept.", null, "review");
    const done = outcomes.get(review.reviewId);
    if (done) return structuredClone(done);
    const sim = spent.has(agentId) ? "success" : assigned.get(agentId) ?? "success";
    const fail = (reason: TakeoverSim) => { if (sim !== reason) return false; spent.add(agentId); return true; };
    const current = find(agentId), successor = review.successor!, named = { requestId: stored.requestId, agentId: null, title: successor.title };
    const stale = current && !refusal(current.agent) ? moved(current.agent, stored) : [];
    const changed = !current ? "the session is no longer on this host" : refusal(current.agent) ? "the session changed since review"
      : stale.length ? `the ${stale.join(stale.length === 2 ? " and " : ", ")} changed since review; review it again` : null;
    if (changed || fail("refuse-preflight")) return record(review, outcome(review, "refused", "preflight", `SIMULATED: preflight refused (${changed ?? "the route evidence expired since review"}). Nothing was archived; the original is kept.`, null, "review"));
    stored.summary = summaryRevisionSchema.parse(await navigator.invoke("sessions.summary-save", { agentId }));
    logHostEvent("simulated.takeover", `${agentId}: compact summary r${stored.summary.revision} saved before archive`);
    if (!current!.agent.archivedAt) update(agentId, { archivedAt: new Date().toISOString(), status: "closed" }, "SIMULATED takeover archive; history and files kept");
    if (!find(agentId)?.agent.archivedAt) throw new Error(`SIMULATED: ${agentId} did not read back archived; no new session was created.`);
    logHostEvent("simulated.takeover", `${agentId}: archive read back; retired before any new session`);
    if (fail("launch-fails")) {
      partials.set(agentId, review.reviewId);
      return record(review, outcome(review, "partial", "create", "SIMULATED: the previous session is archived but the new session did not start. The seat is unoccupied; recovery is pending. The original was not kept.", named, "review"));
    }
    const predecessor = current!, at = new Date().toISOString(), listed = listedFeatures(successor.provider, successor.model);
    add({ project: predecessor.project, agent: {
      id: `agt_fixture_takeover_${stored.requestId}`, title: successor.title, status: "idle", createdAt: at, updatedAt: at, lastUserMessageAt: null,
      cwd: successor.cwd, workspaceId: successor.workspaceId, provider: successor.provider, model: successor.model,
      effectiveThinkingOptionId: successor.effort ?? undefined, currentModeId: successor.mode ?? undefined, parentAgentId: predecessor.agent.parentAgentId ?? null,
      requiresAttention: false, attentionReason: null, pendingPermissions: [], archivedAt: null,
      labels: { ...predecessor.agent.labels, [CREATE_REQUEST_LABEL]: stored.requestId, "takeover.predecessor": agentId },
      ...(listed ? { features: listed.map((feature) => ({ ...feature, value: stored.values[feature.id] ?? feature.value }) as CreateFeature) } : {}),
    } });
    logHostEvent("simulated.takeover", `agt_fixture_takeover_${stored.requestId}: created for ${agentId} in page memory`);
    if (fail("unknown-effect")) return record(review, outcome(review, "uncertain", "create", "SIMULATED: the new session's creation reply was lost, so its effect is not confirmed. Check reads it; nothing is resent.", named, "check"));
    return record(review, settle(stored) ?? outcome(review, "uncertain", "create", "SIMULATED: no session from this request reads back yet. Check reads it; nothing is resent.", named, "check"));
  }

  /** Reads a recorded outcome; an unconfirmed creation is settled from the stored seat by its request, never created again. */
  function check(raw: unknown): TakeoverOutcome {
    const { reviewId } = checkTakeover.input.parse(raw), stored = reviews.get(reviewId), done = outcomes.get(reviewId);
    if (!stored || !done) throw new Error(`SIMULATED: no confirmed takeover of review ${reviewId} on this host.`);
    if (done.next !== "check") return structuredClone(done);
    const settled = settle(stored);
    return settled ? record(stored.review, settled) : { ...structuredClone(done), checkedAt: new Date().toISOString() };
  }

  return {
    async invoke(method: string, input: unknown): Promise<unknown> {
      if (method === reviewTakeover.name) return review(input);
      if (method === applyTakeover.name) return apply(input);
      if (method === checkTakeover.name) return check(input);
      throw new Error(`Unsupported fixture takeover RPC ${method}`);
    },
    /** Reviews, recorded outcomes, drawn outcomes and unfinished partial takeovers in page memory, for review and tests. */
    inspect: () => structuredClone({ reviews: [...reviews.values()].map((entry) => entry.review), outcomes: Object.fromEntries(outcomes), assigned: Object.fromEntries(assigned), partials: Object.fromEntries(partials) }),
  };
}
