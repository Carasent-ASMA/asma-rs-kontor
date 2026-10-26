import { afterEach, describe, expect, it, vi } from "vitest";
import { takeoverOutcomeSchema, takeoverReviewSchema, type TakeoverReviewInput } from "../../paseo-plugin/shared/session-lifecycle";
import { CREATE_REQUEST_LABEL, type CreateFeature, type CreateProvider } from "../../paseo-plugin/shared/session-create";
import type { FixtureAgent, FixtureSession } from "../src/fixtures/sessions";
import { PROVIDERS } from "../src/host/session-create";
import { fixtureNavigator, TAKEOVER_SIMULATED } from "../src/host/session-navigator";
import { fixtureTakeover, UNDISCLOSED_LIMIT, type TakeoverSimMode } from "../src/host/session-takeover";
import { copyText } from "../src/host/sdk-react-native";

const AT = "2026-10-25T10:00:00Z";
const seat = (id: string, patch: Partial<FixtureAgent> = {}): FixtureSession => ({ project: { projectKey: "prj_fixture", projectName: "Fixture desk", workspaceName: "LSA" },
  agent: { id, title: `LSA • ADHOC • Fictional ${id}`, status: "idle", createdAt: AT, updatedAt: AT, lastUserMessageAt: AT, cwd: "/fixture/desk", workspaceId: "wks_fixture",
    provider: "claude", model: "claude-opus-5-5", effectiveThinkingOptionId: "high", currentModeId: "default", labels: { role: "LSA" }, ...patch } });
const FAST_PLAN = { provider: "codex", model: "gpt-6.1-sol", effort: "high", featureValues: { service_tier: "priority", plan_mode: true } };
/** A Codex seat on its own provider and permission mode, with the given saved settings. */
const codexSeat = (id: string, patch: Partial<FixtureAgent> = {}) => seat(id, { provider: "codex", model: "gpt-6-astra", effectiveThinkingOptionId: "high", currentModeId: "fixture-mode", ...patch });
/** The synthetic Codex settings as a seat stores them: Speed and Plan at the given values. */
const stored = (speed: string | null, plan: boolean): CreateFeature[] => [
  { type: "select", id: "service_tier", label: "Speed", value: speed, options: [{ id: "default", label: "Normal", isDefault: true }, { id: "priority", label: "Fast", isDefault: false }] },
  { type: "toggle", id: "plan_mode", label: "Plan", value: plan }];
/** The catalog with `xhigh` removed from the named Codex models, so a seat at xhigh cannot keep its effort there. */
const withoutXhigh = (...models: string[]): CreateProvider[] => PROVIDERS.map((provider) => ({ ...provider, models: provider.models.map((model) =>
  models.includes(model.id) ? { ...model, efforts: model.efforts.filter((effort) => effort.id !== "xhigh"), defaultEffort: "high" } : model) }));

/** A lead with one worker, a running seat and a spare idle seat, plus `extra` seats, in a store that keeps what `add` stores; with `drop` it loses the stored settings. */
function host(mode: TakeoverSimMode = "success", options: { drop?: boolean; providers?: CreateProvider[]; extra?: FixtureSession[] } = {}) {
  const rows = new Map([seat("agt_lead"), seat("agt_worker", { parentAgentId: "agt_lead" }), seat("agt_busy", { status: "running" }), seat("agt_spare"), ...options.extra ?? []].map((row) => [row.agent.id, row]));
  const get = () => [...rows.values()], archives: string[] = [];
  const navigator = fixtureNavigator(get);
  const takeover = fixtureTakeover({ get, navigator, mode, providers: options.providers,
    update: (id, patch) => { archives.push(id); rows.set(id, { ...rows.get(id)!, agent: { ...rows.get(id)!.agent, ...patch } }); },
    add: (row) => { const { features: _dropped, ...agent } = row.agent; rows.set(row.agent.id, options.drop ? { ...row, agent } : row); } });
  const review = async (input: Partial<TakeoverReviewInput> & { agentId: string }) => takeoverReviewSchema.parse(await takeover.invoke("sessions.takeover-review", { bindingKey: null, ...input }));
  const apply = async (ready: { reviewId: string; revision: number; digest: string | null }) => takeoverOutcomeSchema.parse(await takeover.invoke("sessions.takeover-apply", { reviewId: ready.reviewId, revision: ready.revision, digest: ready.digest ?? "0".repeat(64) }));
  const check = async (reviewId: string) => takeoverOutcomeSchema.parse(await takeover.invoke("sessions.takeover-check", { reviewId }));
  const successors = (of = "agt_lead") => get().filter(({ agent }) => agent.labels["takeover.predecessor"] === of);
  return { rows, navigator, takeover, review, apply, check, successors, archives };
}

describe("simulated takeover host", () => {
  it("reviews Auto on the first available route of the seat's chain, and keeps title, workspace, checkout and mode", async () => {
    const retired = codexSeat("agt_retired", { model: "gpt-5-retired", effectiveThinkingOptionId: "xhigh", currentModeId: "fixture-full-access" });
    const ready = await host("success", { extra: [retired] }).review({ agentId: "agt_retired" });
    expect(ready).toMatchObject({ state: "ready", ownership: "ordinary", message: "SIMULATED: takes over on GPT-6 Astra (Auto: first available in the seat's chain). Compact context is saved before archive." });
    expect(ready.route?.considered.map((route) => [route.account, route.model, route.excluded])).toEqual([["codex", "gpt-6-astra", null]]);
    expect(ready.route?.selected).toMatchObject({ account: "codex", model: "gpt-6-astra", effort: "xhigh" });
    expect(ready.successor).toMatchObject({ title: "LSA • ADHOC • Fictional agt_retired", workspaceId: "wks_fixture", cwd: "/fixture/desk", provider: "codex", model: "gpt-6-astra", mode: "fixture-full-access" });
    expect(ready.digest).toMatch(/^[a-f0-9]{64}$/);
    expect((await host().review({ agentId: "agt_lead" })).route?.selected).toMatchObject({ account: "claude", model: "claude-opus-5-5", effort: "high" });
  });

  it("takes over on a chosen route: the successor seat stores Fast and Plan, the predecessor is archived and its worker is kept", async () => {
    const { rows, navigator, review, apply, successors, archives } = host("success", { extra: [codexSeat("agt_lead", { model: "gpt-6-astra" })] });
    const ready = await review({ agentId: "agt_lead", route: FAST_PLAN });
    expect(ready.message).toContain("(chosen for this takeover only)");
    const done = await apply(ready);
    expect(done).toMatchObject({ state: "succeeded", phase: "done", next: "none", message: "SIMULATED: continuing in a new session; the previous session is archived. Settings read back: Speed Fast, Plan on." });
    const [successor] = successors();
    expect(successors()).toHaveLength(1);
    expect(done.successor).toEqual({ requestId: ready.successor!.requestId, agentId: successor!.agent.id, title: "LSA • ADHOC • Fictional agt_lead" });
    expect(successor!.agent).toMatchObject({ title: "LSA • ADHOC • Fictional agt_lead", workspaceId: "wks_fixture", cwd: "/fixture/desk", currentModeId: "fixture-mode", provider: "codex", model: "gpt-6.1-sol", archivedAt: null });
    expect(successor!.agent.features?.map((feature) => [feature.id, feature.value])).toEqual([["service_tier", "priority"], ["plan_mode", true]]);
    expect(rows.get("agt_lead")!.agent).toMatchObject({ status: "closed", archivedAt: expect.any(String) });
    expect(rows.get("agt_worker")!.agent.archivedAt).toBeUndefined();
    expect(archives).toEqual(["agt_lead"]);
    expect(navigator.pinned(successor!.agent.id)).toMatchObject({ sourceAgentId: "agt_lead", revision: 1 });
  });

  it("reports differences when the stored seat does not hold the chosen settings, never a false readback", async () => {
    const { review, apply } = host("success", { drop: true, extra: [codexSeat("agt_lead")] });
    const done = await apply(await review({ agentId: "agt_lead", route: FAST_PLAN }));
    expect(done).toMatchObject({ state: "partial", phase: "verify", message: expect.stringContaining("service_tier reads not reported. plan_mode reads not reported.") });
    expect(done.message).not.toContain("Settings read back");
  });

  it("answers a repeated confirmation from its recorded outcome, with no second seat", async () => {
    const { review, apply, successors, archives } = host();
    const ready = await review({ agentId: "agt_lead" });
    const first = await apply(ready);
    expect(await apply(ready)).toEqual(first);
    expect(successors()).toHaveLength(1);
    expect(archives).toEqual(["agt_lead"]);
  });

  it("launch fails: archived with no successor and the original not kept; a fresh review finishes it once, without a second archive", async () => {
    const { rows, review, apply, successors, archives } = host("launch-fails");
    const partial = await apply(await review({ agentId: "agt_lead" }));
    expect(partial).toMatchObject({ state: "partial", phase: "create", next: "review", successor: { agentId: null } });
    expect(partial.message).toContain("The original was not kept.");
    expect(partial.message).not.toMatch(/original is kept/);
    expect(rows.get("agt_lead")!.agent.archivedAt).toEqual(expect.any(String));
    expect(successors()).toHaveLength(0);
    const retry = await review({ agentId: "agt_lead" });
    expect(retry).toMatchObject({ state: "ready", message: expect.stringContaining("finishes the earlier takeover") });
    expect(await apply(retry)).toMatchObject({ state: "succeeded", successor: { agentId: successors()[0]!.agent.id } });
    expect(successors()).toHaveLength(1);
    expect(archives).toEqual(["agt_lead"]);
    expect((await review({ agentId: "agt_lead" })).state).toBe("refused");
  });

  it("no model: the review is blocked, nothing can be confirmed and the original is kept", async () => {
    const { rows, review, apply, successors } = host("no-model");
    const blocked = await review({ agentId: "agt_lead" });
    expect(blocked).toMatchObject({ state: "blocked", digest: null, successor: null, message: "SIMULATED: no model in this seat's chain is available. Nothing was sent; the original is kept." });
    expect(blocked.route?.considered.every((route) => route.excluded)).toBe(true);
    expect(await apply(blocked)).toMatchObject({ state: "refused", phase: "preflight", next: "review" });
    expect(rows.get("agt_lead")!.agent.archivedAt).toBeUndefined();
    expect(successors()).toHaveLength(0);
  });

  it("unknown effect: uncertain until Check reads back the one successor its request created", async () => {
    const { review, apply, check, successors } = host("unknown-effect");
    const ready = await review({ agentId: "agt_lead" });
    expect(await apply(ready)).toMatchObject({ state: "uncertain", phase: "create", next: "check", successor: { agentId: null } });
    const [successor] = successors();
    expect(successor!.agent.labels[CREATE_REQUEST_LABEL]).toBe(ready.successor!.requestId);
    expect(await check(ready.reviewId)).toMatchObject({ state: "succeeded", next: "none", successor: { requestId: ready.successor!.requestId, agentId: successor!.agent.id } });
    expect(await apply(ready)).toMatchObject({ state: "succeeded", successor: { agentId: successor!.agent.id } });
    expect(successors()).toHaveLength(1);
  });

  it("refuses a stale revision or digest with nothing done, and a revision it no longer holds", async () => {
    const { rows, review, apply, successors } = host();
    const first = await review({ agentId: "agt_lead" });
    const second = await review({ agentId: "agt_lead", reviewId: first.reviewId, revision: 1 });
    expect(second).toMatchObject({ reviewId: first.reviewId, revision: 2, successor: { requestId: first.successor!.requestId } });
    expect(second.digest).not.toBe(first.digest);
    await expect(review({ agentId: "agt_lead", reviewId: first.reviewId, revision: 1 })).rejects.toThrow(/not the current revision/);
    expect(await apply(first)).toMatchObject({ state: "refused", phase: "preflight", next: "review" });
    expect(await apply({ ...second, digest: first.digest })).toMatchObject({ state: "refused", phase: "preflight" });
    expect(rows.get("agt_lead")!.agent.archivedAt).toBeUndefined();
    expect(successors()).toHaveLength(0);
    expect((await apply(second)).state).toBe("succeeded");
  });

  it("refuses a running seat and fails visibly for an unknown one; preflight refused archives nothing and a fresh review recovers", async () => {
    const { rows, review, apply } = host("refuse-preflight");
    expect(await review({ agentId: "agt_busy" })).toMatchObject({ state: "refused", digest: null, message: "SIMULATED: this session is running. Stop it in the session, then take it over. Nothing was sent; the original is kept." });
    await expect(review({ agentId: "agt_missing" })).rejects.toThrow("SIMULATED: no session agt_missing on this host.");
    expect(await apply(await review({ agentId: "agt_lead" }))).toMatchObject({ state: "refused", phase: "preflight", next: "review", message: expect.stringContaining("Nothing was archived; the original is kept.") });
    expect(rows.get("agt_lead")!.agent.archivedAt).toBeUndefined();
    expect((await apply(await review({ agentId: "agt_lead" }))).state).toBe("succeeded");
  });

  it("cycles outcomes across distinct seats in sequence mode", async () => {
    const { review, takeover } = host("sequence");
    expect((await review({ agentId: "agt_lead" })).state).toBe("ready");
    expect((await review({ agentId: "agt_spare" })).state).toBe("blocked");
    expect(takeover.inspect().assigned).toEqual({ agt_lead: "success", agt_spare: "no-model" });
  });
});

// R1: a permission mode is a provider's own; no mapping between providers is qualified, so a route on another provider never inherits one.
describe("simulated takeover permissions", () => {
  it("refuses Auto when the seat's only fallback is another provider, before any archive or creation", async () => {
    // The captured cohort's shape: a seat on a second Claude account at bypassPermissions whose provider lists no model, so its chain is only the Codex default.
    const { rows, review, apply, successors, archives } = host("success", { extra: [seat("agt_personal", { provider: "claude-b", currentModeId: "bypassPermissions" })] });
    const refused = await review({ agentId: "agt_personal" });
    expect(refused).toMatchObject({ state: "refused", digest: null, successor: null });
    expect(refused.message).toBe("SIMULATED: no route in this seat's chain is available and keeps its permissions. Codex cannot keep this seat's permissions (bypassPermissions on claude-b): no permission mapping between providers is qualified. Nothing was sent; the original is kept.");
    expect(refused.route?.considered.map((route) => [route.account, route.model])).toEqual([["codex", "gpt-6-astra"]]);
    expect(await apply(refused)).toMatchObject({ state: "refused", phase: "preflight" });
    expect(rows.get("agt_personal")!.agent.archivedAt).toBeUndefined();
    expect(successors("agt_personal")).toHaveLength(0);
    expect(archives).toEqual([]);
  });

  it("skips another provider for Auto and says why, with the unavailable own route beside it", async () => {
    const unavailable = PROVIDERS.map((provider) => provider.id === "claude" ? { ...provider, available: false, status: "error" as const, note: "Provider error." } : provider);
    const blocked = await host("success", { providers: unavailable }).review({ agentId: "agt_lead" });
    expect(blocked).toMatchObject({ state: "blocked", digest: null, successor: null });
    expect(blocked.route?.considered.map((route) => [route.account, route.excluded])).toEqual([["claude", "Claude Code: Provider error."],
      ["codex", "Codex cannot keep this seat's permissions (default on claude): no permission mapping between providers is qualified."]]);
  });

  it("refuses a chosen route on another provider, and a mode the seat's own provider does not list, with nothing done", async () => {
    const { rows, review, successors, archives } = host("success", { extra: [codexSeat("agt_codex", { currentModeId: "bypassPermissions" })] });
    expect(await review({ agentId: "agt_lead", route: FAST_PLAN })).toMatchObject({ state: "refused", digest: null,
      message: "SIMULATED: Codex cannot keep this seat's permissions (default on claude): no permission mapping between providers is qualified. Nothing was sent; the original is kept." });
    // The same identifier on Codex is not Claude's mode; Codex's catalog does not list it, so it is not kept.
    expect(await review({ agentId: "agt_codex" })).toMatchObject({ state: "refused", message: expect.stringContaining("Codex does not list this seat's permission mode bypassPermissions") });
    expect(await review({ agentId: "agt_codex", route: FAST_PLAN })).toMatchObject({ state: "refused", message: expect.stringContaining("does not list this seat's permission mode bypassPermissions") });
    expect([rows.get("agt_lead")!.agent.archivedAt, rows.get("agt_codex")!.agent.archivedAt]).toEqual([undefined, undefined]);
    expect([...successors(), ...successors("agt_codex")]).toHaveLength(0);
    expect(archives).toEqual([]);
  });

  // R1b: a mode the seat never reported is missing evidence, even on its own provider: Auto and a chosen route both refuse before any effect.
  it("refuses a seat whose permission mode was never reported, Auto or chosen, with nothing done", async () => {
    const { rows, review, apply, successors, archives } = host("success", { extra: [seat("agt_unknown", { currentModeId: undefined })] });
    const unreported = "Claude Code cannot keep this seat's permissions: its permission mode was not reported, so the new session's permissions cannot be confirmed.";
    const auto = await review({ agentId: "agt_unknown" });
    expect(auto).toMatchObject({ state: "refused", digest: null, successor: null });
    expect(auto.message).toBe(`SIMULATED: no route in this seat's chain is available and keeps its permissions. ${unreported} Codex cannot keep this seat's permissions (not reported on claude): no permission mapping between providers is qualified. Nothing was sent; the original is kept.`);
    expect(await review({ agentId: "agt_unknown", route: { provider: "claude", model: "claude-opus-5-5", effort: "high", featureValues: {} } }))
      .toMatchObject({ state: "refused", digest: null, message: `SIMULATED: ${unreported} Nothing was sent; the original is kept.` });
    expect(await apply(auto)).toMatchObject({ state: "refused", phase: "preflight" });
    expect([rows.get("agt_unknown")!.agent.archivedAt, successors("agt_unknown"), archives]).toEqual([undefined, [], []]);
  });

  it("keeps a listed mode on the seat's own provider and reads it back from the stored successor", async () => {
    const { review, apply, successors } = host("success", { extra: [codexSeat("agt_full", { currentModeId: "fixture-full-access" })] });
    const done = await apply(await review({ agentId: "agt_full" }));
    expect(done).toMatchObject({ state: "succeeded", phase: "done" });
    expect(successors("agt_full")[0]!.agent).toMatchObject({ provider: "codex", currentModeId: "fixture-full-access" });
    const claude = host();
    expect(await claude.apply(await claude.review({ agentId: "agt_lead" }))).toMatchObject({ state: "succeeded" });
    expect(claude.successors()[0]!.agent).toMatchObject({ provider: "claude", currentModeId: "default" });
  });
});

// R4 and R5: Auto carries each seat's saved effort and settings; a route that cannot is skipped for one that can, else its limit is stated.
describe("simulated takeover saved settings", () => {
  const features = (row: FixtureSession | undefined) => row?.agent.features?.map((feature) => [feature.id, feature.value]);

  it("carries a Fast and Plan seat's settings and effort into the successor and reads them back from it", async () => {
    const { review, apply, successors } = host("success", { extra: [codexSeat("agt_fast", { effectiveThinkingOptionId: "high", features: stored("priority", true) })] });
    const ready = await review({ agentId: "agt_fast" });
    expect(ready.limits).toEqual([TAKEOVER_SIMULATED]);
    expect(ready.successor).toMatchObject({ provider: "codex", model: "gpt-6-astra", effort: "high" });
    expect(await apply(ready)).toMatchObject({ state: "succeeded", message: "SIMULATED: continuing in a new session; the previous session is archived. Settings read back: Speed Fast, Plan on." });
    expect(features(successors("agt_fast")[0])).toEqual([["service_tier", "priority"], ["plan_mode", true]]);
    expect(successors("agt_fast")[0]!.agent.effectiveThinkingOptionId).toBe("high");
  });

  it("reads the effort from the runtime when the seat reports no other", async () => {
    const runtime = codexSeat("agt_runtime", { effectiveThinkingOptionId: undefined, runtimeInfo: { provider: "codex", sessionId: null, thinkingOptionId: "medium" } as FixtureAgent["runtimeInfo"] });
    const { review, apply, successors } = host("success", { extra: [runtime] });
    const ready = await review({ agentId: "agt_runtime" });
    expect(ready.predecessor?.effort).toBe("medium");
    expect(ready.successor?.effort).toBe("medium");
    expect(await apply(ready)).toMatchObject({ state: "succeeded" });
    expect(successors("agt_runtime")[0]!.agent.effectiveThinkingOptionId).toBe("medium");
  });

  it("keeps each seat's own settings in a mixed set, and a seat at its defaults stays at them", async () => {
    const { review, apply, successors } = host("success", { extra: [codexSeat("agt_a", { features: stored("priority", false), effectiveThinkingOptionId: "low" }),
      codexSeat("agt_b", { features: stored("default", true), effectiveThinkingOptionId: "xhigh" }), codexSeat("agt_c", { features: stored(null, false) })] });
    for (const id of ["agt_a", "agt_b", "agt_c"]) expect((await apply(await review({ agentId: id }))).state).toBe("succeeded");
    expect(["agt_a", "agt_b", "agt_c"].map((id) => [features(successors(id)[0]), successors(id)[0]!.agent.effectiveThinkingOptionId])).toEqual([
      [[["service_tier", "priority"], ["plan_mode", false]], "low"], [[["service_tier", null], ["plan_mode", true]], "xhigh"], [[["service_tier", null], ["plan_mode", false]], "high"]]);
  });

  it("skips a route that cannot keep the seat's effort when a later one keeps everything", async () => {
    const sol = codexSeat("agt_sol", { model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", features: stored(null, false) });
    const ready = await host("success", { extra: [sol], providers: withoutXhigh("gpt-6.1-sol") }).review({ agentId: "agt_sol" });
    expect(ready.route?.considered.map((route) => [route.model, route.excluded])).toEqual([["gpt-6.1-sol", "GPT-6.1 Sol does not offer the seat's effort xhigh; it would run at high."], ["gpt-6-astra", null]]);
    expect(ready).toMatchObject({ state: "ready", limits: [TAKEOVER_SIMULATED], successor: { model: "gpt-6-astra", effort: "xhigh" } });
  });

  it("states what the only possible route cannot keep, as review limits", async () => {
    const sol = codexSeat("agt_sol", { model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", features: stored(null, false) });
    const ready = await host("success", { extra: [sol], providers: withoutXhigh("gpt-6.1-sol", "gpt-6-astra") }).review({ agentId: "agt_sol" });
    expect(ready).toMatchObject({ state: "ready", successor: { model: "gpt-6.1-sol", effort: "high" } });
    expect(ready.limits).toEqual([TAKEOVER_SIMULATED, "GPT-6.1 Sol does not offer the seat's effort xhigh; it would run at high."]);
    const planned = await host("success", { extra: [seat("agt_planned", { features: stored(null, true) })] }).review({ agentId: "agt_planned" });
    expect(planned).toMatchObject({ state: "ready", successor: { provider: "claude" } });
    expect(planned.limits).toEqual([TAKEOVER_SIMULATED, "Claude Opus 5.5 does not offer the seat's Plan on; the new session would run without it."]);
  });

  // R4b: settings a seat never reported are unknown, not off; the defaults its successor would run at are a limit of that seat's own review.
  it("states the defaults a seat with unreported settings would run at, and nothing for recorded or default settings or a model without settings", async () => {
    const { review, apply, successors } = host("success", { extra: [codexSeat("agt_unknown"), codexSeat("agt_defaults", { features: stored(null, false) }), codexSeat("agt_fast", { features: stored("priority", true) })] });
    const unknown = await review({ agentId: "agt_unknown" });
    expect(unknown).toMatchObject({ state: "ready", successor: { provider: "codex", model: "gpt-6-astra" } });
    expect(unknown.limits).toEqual([TAKEOVER_SIMULATED, "This seat's settings were not reported, so the new session would run at GPT-6 Astra's defaults: Speed Normal, Plan off."]);
    expect([(await review({ agentId: "agt_defaults" })).limits, (await review({ agentId: "agt_fast" })).limits, (await review({ agentId: "agt_lead" })).limits]).toEqual([[TAKEOVER_SIMULATED], [TAKEOVER_SIMULATED], [TAKEOVER_SIMULATED]]);
    // What the review stated is what the successor stores: the model's defaults, with no settings claimed as read back.
    expect(await apply(unknown)).toMatchObject({ state: "succeeded", message: "SIMULATED: continuing in a new session; the previous session is archived." });
    expect(features(successors("agt_unknown")[0])).toEqual([["service_tier", null], ["plan_mode", false]]);
  });

  it("states a limit the operator was not shown before in limited mode, and only on a ready review", async () => {
    const { review } = host("limited", { extra: [seat("agt_personal", { provider: "claude-b", currentModeId: "bypassPermissions" })] });
    expect((await review({ agentId: "agt_lead" })).limits).toEqual([TAKEOVER_SIMULATED, UNDISCLOSED_LIMIT]);
    expect((await review({ agentId: "agt_personal" })).limits).toEqual([TAKEOVER_SIMULATED]);
  });
});

// R5b: confirming a held review re-reads what it was decided on; a material change needs a fresh review, with nothing done.
describe("simulated takeover confirmation", () => {
  const patch = (rows: Map<string, FixtureSession>, id: string, change: Partial<FixtureAgent>) => rows.set(id, { ...rows.get(id)!, agent: { ...rows.get(id)!.agent, ...change } });

  it.each([
    ["permission mode", { currentModeId: "fixture-full-access" }],
    ["saved settings", { features: stored("priority", true) }],
    ["effort", { effectiveThinkingOptionId: "low" }],
  ] as const)("refuses a held review after the seat's %s changed, with nothing done, and a fresh review proceeds with its disclosure", async (what, change) => {
    const { rows, review, apply, successors, archives } = host("limited", { extra: [codexSeat("agt_held", { features: stored(null, false) })] });
    const held = await review({ agentId: "agt_held" });
    expect(held).toMatchObject({ state: "ready", limits: [TAKEOVER_SIMULATED, UNDISCLOSED_LIMIT] });
    patch(rows, "agt_held", change);
    expect(await apply(held)).toMatchObject({ state: "refused", phase: "preflight", next: "review",
      message: `SIMULATED: preflight refused (the ${what} changed since review; review it again). Nothing was archived; the original is kept.` });
    expect([rows.get("agt_held")!.agent.archivedAt, successors("agt_held"), archives]).toEqual([undefined, [], []]);
    const fresh = await review({ agentId: "agt_held" });
    expect(fresh).toMatchObject({ state: "ready", limits: [TAKEOVER_SIMULATED, UNDISCLOSED_LIMIT] });
    expect(fresh.reviewId).not.toBe(held.reviewId);
    expect(await apply(fresh)).toMatchObject({ state: "succeeded" });
    expect(successors("agt_held")).toHaveLength(1);
  });

  it.each([
    ["title", { title: "LSA • ADHOC • Fictional renamed seat" }],
    ["workspace", { workspaceId: "wks_fixture_moved" }],
    ["checkout", { cwd: "/fixture/moved-desk" }],
  ] as const)("refuses a held review after the seat's %s changed, and a fresh review continues it under the new one", async (what, change) => {
    const { rows, review, apply, successors, archives } = host("success", { extra: [codexSeat("agt_held", { features: stored(null, false) })] });
    const held = await review({ agentId: "agt_held" });
    expect(held.state).toBe("ready");
    patch(rows, "agt_held", change);
    expect(await apply(held)).toMatchObject({ state: "refused", phase: "preflight", next: "review",
      message: `SIMULATED: preflight refused (the ${what} changed since review; review it again). Nothing was archived; the original is kept.` });
    expect([rows.get("agt_held")!.agent.archivedAt, successors("agt_held"), archives]).toEqual([undefined, [], []]);
    const fresh = await review({ agentId: "agt_held" });
    expect(fresh.successor).toMatchObject(change);
    expect(await apply(fresh)).toMatchObject({ state: "succeeded" });
    expect(successors("agt_held").map(({ agent }) => agent)).toEqual([expect.objectContaining(change)]);
  });

  it("keeps a held review across activity that leaves the seat's name, place and route as reviewed", async () => {
    const { rows, review, apply, successors } = host("success", { extra: [codexSeat("agt_held", { features: stored(null, false) })] });
    const held = await review({ agentId: "agt_held" });
    patch(rows, "agt_held", { updatedAt: "2026-10-25T11:00:00Z", lastUserMessageAt: "2026-10-25T11:00:00Z" });
    expect(await apply(held)).toMatchObject({ state: "succeeded" });
    expect(successors("agt_held")).toHaveLength(1);
  });

  it("refuses a held review whose route became unavailable, and reviews afresh what is available now", async () => {
    const providers = structuredClone(PROVIDERS);
    const { rows, review, apply, successors, archives } = host("limited", { providers, extra: [codexSeat("agt_held", { features: stored(null, false) })] });
    const held = await review({ agentId: "agt_held" });
    Object.assign(providers.find((entry) => entry.id === "codex")!, { available: false, status: "error", note: "Provider error." });
    expect(await apply(held)).toMatchObject({ state: "refused", phase: "preflight", message: "SIMULATED: preflight refused (the route and effort changed since review; review it again). Nothing was archived; the original is kept." });
    expect([rows.get("agt_held")!.agent.archivedAt, successors("agt_held"), archives]).toEqual([undefined, [], []]);
    expect(await review({ agentId: "agt_held" })).toMatchObject({ state: "blocked", digest: null });
  });
});

describe("simulated clipboard", () => {
  afterEach(() => { vi.unstubAllGlobals(); });
  it("refuses the write with clipboard=denied and accepts it otherwise", async () => {
    vi.stubGlobal("location", { search: "?clipboard=denied" });
    await expect(copyText("summary")).rejects.toThrow("SIMULATED: the clipboard refused the write");
    vi.stubGlobal("location", { search: "" });
    await expect(copyText("summary")).resolves.toBeUndefined();
  });
});
