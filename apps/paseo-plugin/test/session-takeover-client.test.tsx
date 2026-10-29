import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TakeoverOutcome, TakeoverReview } from "../shared/session-lifecycle";

// Each test gets a fresh renderer, so one run's state never carries into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const rpc = vi.hoisted(() => ({}) as Record<string, ReturnType<typeof vi.fn>>);
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", ScrollView: "ScrollView",
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

import { outcomeResult, retryable, reviewResult, seatAction, takeoverSummary, TakeoverResults, unfinished, useTakeoverRun, type SeatResult, type TakeoverRun } from "../client/session-takeover";

type Element = { type: unknown; props: Record<string, unknown> };
/** The sheet and its footer are hook-free, so they are drawn in place and their buttons are visible. */
const DRAWN = new Set(["OperationSheet", "SheetFooter", "FooterButton"]);
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? []
  : typeof node.type === "function" && DRAWN.has(node.type.name) ? [node, ...walk((node.type as (props: object) => unknown)(node.props))] : [node, ...walk(node.props.children)];
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const DIGEST = "d".repeat(64);
const reviewId = (id: string) => `00000000-0000-4000-8000-00000000000${id}`;
const seat = (id: string, patch: Partial<SeatResult> = {}): SeatResult => ({ id, title: `Seat ${id}`, state: "queued", line: "Queued", detail: null, model: null, reviewId: null, next: "review", ...patch });
const review = (agentId: string, state: TakeoverReview["state"] = "ready", patch: Partial<TakeoverReview> = {}): TakeoverReview => ({
  reviewId: reviewId(agentId), revision: 2, agentId, checkedAt: "2026-10-26T09:00:00Z", state, message: `Fixture review ${state}.`, predecessor: null, ownership: "ordinary",
  association: { bindingKey: null, source: null, choices: [] }, route: null, limits: [], digest: state === "ready" ? DIGEST : null,
  successor: { requestId: "00000000-0000-4000-8000-0000000000f1", title: `Seat ${agentId}`, workspaceId: "wks-1", cwd: "/fixture", provider: "codex", model: "gpt-6.1-sol", effort: "high", mode: null, persona: null },
  ...patch,
});
const outcome = (agentId: string, state: TakeoverOutcome["state"], phase: TakeoverOutcome["phase"] = "done", next: TakeoverOutcome["next"] = "review"): TakeoverOutcome => ({
  reviewId: reviewId(agentId), revision: 2, agentId, state, phase, message: `Fixture outcome ${state}.`, successor: null, next, checkedAt: "2026-10-26T09:01:00Z" });
const entries = (...ids: string[]) => ids.map((id) => ({ id, title: `Seat ${id}` }));

beforeEach(() => {
  harness.fresh();
  rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId));
  rpc["sessions.takeover-apply"] = vi.fn(async (input: { reviewId: string }) => outcome(input.reviewId.slice(-1), "succeeded", "done", "none"));
  rpc["sessions.takeover-check"] = vi.fn(async (input: { reviewId: string }) => outcome(input.reviewId.slice(-1), "succeeded", "done", "none"));
});

describe("takeover results", () => {
  it("retries every unfinished seat, never a continued, working, held or hopeless one", () => {
    expect(["queued", "kept", "recovery", "unknown"].map((state) => retryable(seat("a", { state: state as SeatResult["state"] })))).toEqual([true, true, true, true]);
    expect(retryable(seat("a", { state: "done" }))).toBe(false);
    expect(retryable(seat("a", { state: "working" }))).toBe(false);
    expect(retryable(seat("a", { state: "review", held: { revision: 2, digest: DIGEST, limits: ["Extra."] } }))).toBe(false);
    expect(retryable(seat("a", { state: "kept", next: "none" }))).toBe(false);
  });

  // R6: a refusal, a chain with no available model and a chain only a chooser could settle answer the same on Retry.
  it("keeps the original on a review that is not ready, and leaves only an unread answer to Retry, saying why", () => {
    const same = "Retry remaining skips it: asking again gets the same answer.";
    expect(reviewResult(seat("a"), review("a", "blocked"))).toMatchObject({ state: "kept", line: "No available model · Original kept", detail: `Fixture review blocked. ${same}`, model: "S6.1", reviewId: reviewId("a"), next: "none" });
    expect(reviewResult(seat("a"), review("a", "refused", { successor: null }))).toMatchObject({ state: "kept", line: "Not started · Original kept", detail: `Fixture review refused. ${same}`, model: null, next: "none" });
    expect(reviewResult(seat("a"), review("a", "choose", { successor: null }))).toMatchObject({ state: "kept", next: "none",
      detail: `Fixture review choose. Choose a replacement model from the Take over model menu.` });
    expect(reviewResult(seat("a"), review("a", "unknown", { successor: null }))).toMatchObject({ state: "kept", detail: "Fixture review unknown.", next: "review" });
    for (const state of ["blocked", "refused", "choose"] as const) expect(retryable(reviewResult(seat("a"), review("a", state)))).toBe(false);
    expect(retryable(reviewResult(seat("a"), review("a", "unknown")))).toBe(true);
  });

  it.each([
    ["succeeded", "done", "done", "Continuing · Previous session archived"],
    ["partial", "create", "recovery", "Previous session archived · No new session yet · Recovery pending"],
    ["partial", "archive", "recovery", "Previous session archived · No new session yet · Recovery pending"],
    ["partial", "verify", "recovery", "Previous session archived · New session not verified · Recovery pending"],
    ["refused", "preflight", "kept", "Not started · Original kept"],
  ] as const)("words a %s outcome at %s as %s: %s", (state, phase, seatState, line) => {
    expect(outcomeResult(seat("a"), outcome("a", state, phase, "none"))).toMatchObject({ state: seatState, line, detail: expect.stringMatching(new RegExp(`^Fixture outcome ${state}\\.`)), reviewId: reviewId("a"), next: "none" });
  });

  it("explains a refusal nothing here can change, and keeps one a fresh review may change as the server said it", () => {
    expect(outcomeResult(seat("a"), outcome("a", "refused", "preflight", "none")).detail).toBe("Fixture outcome refused. Retry remaining skips it: asking again gets the same answer.");
    expect(outcomeResult(seat("a"), outcome("a", "refused", "preflight", "review")).detail).toBe("Fixture outcome refused.");
  });

  it("leaves an uncertain outcome unconfirmed with only Check next, whatever next the server named", () => {
    expect(outcomeResult(seat("a"), outcome("a", "uncertain", "create", "review"))).toMatchObject({ state: "unknown", line: "Not confirmed · Check before retrying", next: "check" });
  });

  it("summarizes in the order the states matter, skipping empty ones and counting queued with working", () => {
    const seats = [seat("q"), seat("k", { state: "kept" }), seat("u", { state: "unknown" }), seat("w", { state: "working" }), seat("d", { state: "done" }), seat("r", { state: "recovery" }), seat("k2", { state: "kept" })];
    expect(takeoverSummary(seats)).toBe("1 continued · 1 recovery pending · 1 not confirmed · 2 not started · 2 in progress");
    expect(takeoverSummary([seat("d", { state: "done" }), seat("e", { state: "done" })])).toBe("2 continued");
  });
});

describe("useTakeoverRun", () => {
  // Runs live per host beyond any one renderer, so each mount gets its own host unless a test shares one on purpose.
  let hosts = 0;
  const mount = (onFinish = vi.fn(), hostId = `host-${++hosts}`, disclosed: readonly string[] = []) => ({ onFinish, hostId, render: () => harness.current.render(() => useTakeoverRun(hostId, disclosed, onFinish)) });
  const states = (run: TakeoverRun | null) => run?.seats.map((one) => [one.id, one.state, one.next]);

  it("takes seats over one at a time, applying each with the review's own ID, revision and digest", async () => {
    const log: string[] = [];
    let active = 0, most = 0;
    const track = <T,>(name: string, answer: () => T) => async () => { active++; most = Math.max(most, active); log.push(name); await settle(); active--; return answer(); };
    rpc["sessions.takeover-review"] = vi.fn((input: { agentId: string }) => track(`review ${input.agentId}`, () => review(input.agentId))());
    rpc["sessions.takeover-apply"] = vi.fn((input: { reviewId: string }) => track(`apply ${input.reviewId.slice(-1)}`, () => outcome(input.reviewId.slice(-1), "succeeded", "done", "none"))());
    const h = mount();
    await h.render().start(entries("a", "b"), null);
    expect(log).toEqual(["review a", "apply a", "review b", "apply b"]);
    expect(most).toBe(1);
    expect(rpc["sessions.takeover-apply"]).toHaveBeenNthCalledWith(1, { reviewId: reviewId("a"), revision: 2, digest: DIGEST });
    const run = h.render().run!;
    expect(run).toMatchObject({ route: null, running: false });
    expect(run.seats.map((one) => [one.id, one.state, one.line, one.model])).toEqual([["a", "done", "Continuing · Previous session archived", "S6.1"], ["b", "done", "Continuing · Previous session archived", "S6.1"]]);
    expect(h.onFinish).toHaveBeenCalledExactlyOnceWith(run);
  });

  it("sends a chosen route with each review and leaves it out for Auto", async () => {
    const route = { provider: "codex", model: "gpt-6-sol", effort: "high", featureValues: { service_tier: "priority" } };
    await mount().render().start(entries("a"), route);
    expect(rpc["sessions.takeover-review"]).toHaveBeenLastCalledWith({ agentId: "a", bindingKey: null, route });
    harness.fresh();
    await mount().render().start(entries("b"), null);
    expect(rpc["sessions.takeover-review"]).toHaveBeenLastCalledWith({ agentId: "b", bindingKey: null });
  });

  it("keeps the original and says nothing was sent when the review fails", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async () => { throw new Error("socket closed"); });
    const h = mount();
    await h.render().start(entries("a"), null);
    expect(h.render().run!.seats[0]).toMatchObject({ state: "kept", line: "Not started · Original kept", next: "review",
      detail: "Kontor could not review this takeover (socket closed). Nothing was sent." });
    expect(rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
  });

  it("leaves a seat unconfirmed with Check next when confirming gets no reply", async () => {
    rpc["sessions.takeover-apply"] = vi.fn(async () => { throw new Error("timeout"); });
    const h = mount();
    await h.render().start(entries("a"), null);
    expect(h.render().run!.seats[0]).toMatchObject({ state: "unknown", line: "Not confirmed · Check before retrying", next: "check", reviewId: reviewId("a"), model: "S6.1",
      detail: expect.stringMatching(/^No reply after confirming \(timeout\)\. The takeover may have started/) });
  });

  it("checks an unconfirmed seat first on Retry and never reviews or confirms it again", async () => {
    rpc["sessions.takeover-apply"] = vi.fn(async () => { throw new Error("timeout"); });
    const h = mount();
    await h.render().start(entries("a"), null);
    h.render().retry(); await settle();
    expect(rpc["sessions.takeover-check"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("a") });
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    expect(h.render().run!.seats[0]).toMatchObject({ state: "done", line: "Continuing · Previous session archived" });
  });

  it("keeps a seat unconfirmed with Check next when the check itself fails", async () => {
    rpc["sessions.takeover-apply"] = vi.fn(async () => { throw new Error("timeout"); });
    rpc["sessions.takeover-check"] = vi.fn(async () => { throw new Error("store busy"); });
    const h = mount();
    await h.render().start(entries("a"), null);
    h.render().retry(); await settle();
    expect(h.render().run!.seats[0]).toMatchObject({ state: "unknown", next: "check", detail: "Kontor could not read this takeover's result (store busy)." });
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
  });

  it("retries only unfinished seats and never restarts a continued one", async () => {
    // Seat b's first review could not read something; its second is ready.
    let unread = true;
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => {
      if (input.agentId !== "b" || !unread) return review(input.agentId);
      unread = false;
      return review("b", "unknown");
    });
    const h = mount();
    await h.render().start(entries("a", "b"), null);
    expect(states(h.render().run)).toEqual([["a", "done", "none"], ["b", "kept", "review"]]);
    h.render().retry(); await settle();
    expect(rpc["sessions.takeover-review"]!.mock.calls.map(([input]) => (input as { agentId: string }).agentId)).toEqual(["a", "b", "b"]);
    expect(states(h.render().run)).toEqual([["a", "done", "none"], ["b", "done", "none"]]);
    expect(h.onFinish).toHaveBeenCalledTimes(2);
  });

  it("does not retry a seat nothing here can help", async () => {
    rpc["sessions.takeover-apply"] = vi.fn(async () => outcome("a", "refused", "preflight", "none"));
    const h = mount();
    await h.render().start(entries("a"), null);
    expect(states(h.render().run)).toEqual([["a", "kept", "none"]]);
    h.render().retry(); await settle();
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
  });

  it("leaves seats that cannot change out of Retry: no available model, a refusal and a profile choice", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, ({ a: "blocked", b: "refused", c: "choose" } as const)[input.agentId as "a" | "b" | "c"]));
    const h = mount();
    await h.render().start(entries("a", "b", "c"), null);
    expect(h.render().run!.seats.filter(retryable)).toEqual([]);
    h.render().retry(); await settle();
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(3);
  });

  it("never drops a running takeover on clear, and forgets a finished one", async () => {
    let release!: () => void;
    rpc["sessions.takeover-review"] = vi.fn((input: { agentId: string }) => new Promise<TakeoverReview>((resolve) => { release = () => resolve(review(input.agentId)); }));
    const h = mount();
    const started = h.render().start(entries("a"), null);
    h.render().clear();
    expect(h.render().run).toMatchObject({ running: true });
    release(); await started;
    expect(h.render().run).toMatchObject({ running: false });
    h.render().clear();
    expect(h.render().run).toBeNull();
  });
});

// R5: one tap confirms only what its surface showed; any other limit waits for the operator, on that exact review.
describe("useTakeoverRun disclosure", () => {
  let hosts = 0;
  const SHOWN = "SIMULATED: shown on the menu.", EXTRA = "The supervisor is not told about the new session.";
  const mount = (disclosed: readonly string[] = [SHOWN]) => { const hostId = `disclosure-${++hosts}`; return { render: () => harness.current.render(() => useTakeoverRun(hostId, disclosed)) }; };

  it("confirms at once a ready review whose every limit the surface showed", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, "ready", { limits: [SHOWN] }));
    const h = mount();
    await h.render().start(entries("a"), null);
    expect(rpc["sessions.takeover-apply"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("a"), revision: 2, digest: DIGEST });
    expect(h.render().run!.seats[0]).toMatchObject({ state: "done" });
  });

  it("never applies a ready review with a limit nobody was shown; it waits as Review needed listing that limit", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, "ready", { limits: [SHOWN, EXTRA] }));
    const h = mount();
    await h.render().start(entries("a"), null);
    expect(rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
    const held = h.render().run!.seats[0]!;
    expect(held).toMatchObject({ state: "review", line: "Review needed · Original kept", reviewId: reviewId("a"), held: { revision: 2, digest: DIGEST, limits: [EXTRA] } });
    expect(seatAction(held)).toEqual({ kind: "confirm", label: "Confirm takeover" });
    expect(takeoverSummary([held])).toBe("1 review needed");
    // Retry remaining does not run it: only the operator's confirmation does.
    h.render().retry(); await settle();
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
  });

  it("confirms exactly the held review, its ID, revision and digest, without reviewing again", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, "ready", { limits: [EXTRA] }));
    const h = mount([]);
    await h.render().start(entries("a", "b"), null);
    h.render().act("a"); await settle(); await settle();
    expect(rpc["sessions.takeover-apply"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("a"), revision: 2, digest: DIGEST });
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(2);
    expect(h.render().run!.seats.map((one) => one.state)).toEqual(["done", "review"]);
  });

  it("needs a fresh review when the held one went stale, and holds that one again rather than applying it", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, "ready", { limits: [EXTRA] }));
    rpc["sessions.takeover-apply"] = vi.fn(async () => outcome("a", "refused", "preflight", "review"));
    const h = mount([]);
    await h.render().start(entries("a"), null);
    h.render().act("a"); await settle(); await settle();
    expect(h.render().run!.seats[0]).toMatchObject({ state: "kept", next: "review", held: null });
    h.render().retry(); await settle(); await settle();
    expect(rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(2);
    expect(rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    expect(h.render().run!.seats[0]).toMatchObject({ state: "review", held: { limits: [EXTRA] } });
  });
});

// R2: closing results dismisses presentation only; unfinished seats stay with the host for Check or Finish.
describe("useTakeoverRun unfinished seats", () => {
  let hosts = 0;
  // Five seats as the sequence scenario draws them: continued, no model, archived without successor, effect unknown, refused.
  const FIVE = ["a", "b", "c", "d", "e"];
  function sequence() {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, input.agentId === "b" ? "blocked" : "ready"));
    rpc["sessions.takeover-apply"] = vi.fn(async (input: { reviewId: string }) => {
      const id = input.reviewId.slice(-1);
      if (id === "c") return outcome("c", "partial", "create", "review");
      if (id === "d") throw new Error("reply lost");
      if (id === "e") return outcome("e", "refused", "preflight", "review");
      return outcome(id, "succeeded", "done", "none");
    });
  }

  it("keeps recovery and unknown seats with their review IDs after Close, for every surface of the host", async () => {
    sequence();
    const hostId = `unfinished-${++hosts}`;
    const switcher = () => harness.current.render(() => useTakeoverRun(hostId, []));
    await switcher().start(entries(...FIVE), null);
    expect(switcher().run!.seats.map((one) => one.state)).toEqual(["done", "kept", "recovery", "unknown", "kept"]);
    switcher().clear();
    expect(switcher().run!.seats.map((one) => [one.id, one.state, one.reviewId])).toEqual([["c", "recovery", reviewId("c")], ["d", "unknown", reviewId("d")]]);
    // Another surface of the same host, with its own renderer, sees the same seats.
    harness.fresh();
    const sessions = () => harness.current.render(() => useTakeoverRun(hostId, []));
    expect(sessions().run!.seats.filter(unfinished).map((one) => [one.id, seatAction(one)?.label])).toEqual([["c", "Finish takeover"], ["d", "Check takeover"]]);
  });

  it("checks an unknown seat by its earlier review before anything else, and finishes only the seat asked, never a continued one", async () => {
    sequence();
    const hostId = `unfinished-${++hosts}`;
    const render = () => harness.current.render(() => useTakeoverRun(hostId, []));
    await render().start(entries(...FIVE), null);
    render().clear();
    vi.mocked(rpc["sessions.takeover-review"]!).mockClear(); vi.mocked(rpc["sessions.takeover-apply"]!).mockClear();
    render().act("d"); await settle(); await settle();
    expect(rpc["sessions.takeover-check"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("d") });
    expect(rpc["sessions.takeover-review"]).not.toHaveBeenCalled();
    expect(rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
    rpc["sessions.takeover-apply"] = vi.fn(async (input: { reviewId: string }) => outcome(input.reviewId.slice(-1), "succeeded", "done", "none"));
    render().act("c"); await settle(); await settle();
    expect(vi.mocked(rpc["sessions.takeover-review"]!).mock.calls.map(([input]) => (input as { agentId: string }).agentId)).toEqual(["c"]);
    expect(render().run!.seats.map((one) => [one.id, one.state])).toEqual([["c", "done"], ["d", "done"]]);
    render().clear();
    expect(render().run).toBeNull();
  });

  // R2b: a new start never replaces unresolved seats; only their own Check, Finish, Confirm or Decline resolves them.
  const BUSY = "Finish or check the unfinished takeover first.", RUNNING = "A takeover is running; wait for its results.";
  const CHOSEN = { provider: "codex", model: "gpt-6.1-sol", effort: "high", featureValues: {} };
  const reviewed = () => vi.mocked(rpc["sessions.takeover-review"]!).mock.calls.map(([input]) => (input as { agentId: string }).agentId);

  it("refuses a new single or bulk start from any surface while seats are unfinished, and lets Check and Finish continue the same run", async () => {
    sequence();
    const hostId = `unfinished-${++hosts}`;
    const sessions = () => harness.current.render(() => useTakeoverRun(hostId, []));
    await sessions().start(entries(...FIVE), null);
    sessions().clear();
    const kept = structuredClone(sessions().run);
    vi.mocked(rpc["sessions.takeover-review"]!).mockClear(); vi.mocked(rpc["sessions.takeover-apply"]!).mockClear();
    expect(await sessions().start(entries("f"), null)).toBe(BUSY);
    harness.fresh();
    const switcher = () => harness.current.render(() => useTakeoverRun(hostId, []));
    expect(await switcher().start(entries("f", "g"), CHOSEN)).toBe(BUSY);
    expect(switcher().busy).toBe(BUSY);
    expect(switcher().run).toEqual(kept);
    expect(kept!.seats.map((one) => [one.id, one.state, one.reviewId])).toEqual([["c", "recovery", reviewId("c")], ["d", "unknown", reviewId("d")]]);
    expect([rpc["sessions.takeover-review"], rpc["sessions.takeover-apply"], rpc["sessions.takeover-check"]].map((call) => call!.mock.calls.length)).toEqual([0, 0, 0]);
    switcher().act("d"); await settle(); await settle();
    expect(rpc["sessions.takeover-check"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("d") });
    rpc["sessions.takeover-apply"] = vi.fn(async (input: { reviewId: string }) => outcome(input.reviewId.slice(-1), "succeeded", "done", "none"));
    switcher().act("c"); await settle(); await settle();
    expect(reviewed()).toEqual(["c"]);
    expect(rpc["sessions.takeover-apply"]).toHaveBeenCalledExactlyOnceWith({ reviewId: reviewId("c"), revision: 2, digest: DIGEST });
    switcher().clear();
    expect(switcher().busy).toBeNull();
    expect(await switcher().start(entries("f"), null)).toBeNull();
    expect(switcher().run!.seats.map((one) => [one.id, one.state])).toEqual([["f", "done"]]);
  });

  it("lets only the first of two controllers or a stale callback start a host, and keeps another host independent", async () => {
    const releases: (() => void)[] = [];
    rpc["sessions.takeover-review"] = vi.fn((input: { agentId: string }) => new Promise<TakeoverReview>((resolve) => { releases.push(() => resolve(review(input.agentId))); }));
    const hostId = `unfinished-${++hosts}`, away = `unfinished-${++hosts}`;
    const stale = harness.current.render(() => useTakeoverRun(hostId, []));
    harness.fresh();
    const second = harness.current.render(() => useTakeoverRun(hostId, []));
    const first = stale.start(entries("a"), null);
    expect(await stale.start(entries("b"), null)).toBe(RUNNING);
    expect(await second.start(entries("b", "c"), CHOSEN)).toBe(RUNNING);
    stale.retry(); stale.act("a"); stale.clear();
    harness.fresh();
    const elsewhere = harness.current.render(() => useTakeoverRun(away, [])).start(entries("x"), null);
    expect(reviewed()).toEqual(["a", "x"]);
    for (const release of releases) release();
    expect([await first, await elsewhere]).toEqual([null, null]);
    harness.fresh();
    expect(harness.current.render(() => useTakeoverRun(hostId, [])).run!.seats.map((one) => [one.id, one.state])).toEqual([["a", "done"]]);
    harness.fresh();
    expect(harness.current.render(() => useTakeoverRun(away, [])).run!.seats.map((one) => [one.id, one.state])).toEqual([["x", "done"]]);
  });

  it("keeps a held review after Close and refuses a new start until its operator confirms or declines that review", async () => {
    rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => review(input.agentId, "ready", { limits: ["The supervisor is not told."] }));
    const hostId = `unfinished-${++hosts}`;
    const render = () => harness.current.render(() => useTakeoverRun(hostId, []));
    await render().start(entries("a"), null);
    render().clear();
    expect(render().run!.seats.map((one) => [one.id, one.state, one.reviewId, one.held?.digest])).toEqual([["a", "review", reviewId("a"), DIGEST]]);
    expect(await render().start(entries("b"), null)).toBe(BUSY);
    render().decline("a");
    expect(render().run!.seats[0]).toMatchObject({ state: "kept", line: "Declined · Original kept", reviewId: reviewId("a"), held: null, next: "none" });
    expect(rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
    expect(render().busy).toBeNull();
    expect(await render().start(entries("b"), null)).toBeNull();
    expect(reviewed()).toEqual(["a", "b"]);
  });
});

describe("TakeoverResults", () => {
  const sheet = (run: TakeoverRun) => {
    const onClose = vi.fn(), onRetry = vi.fn();
    const onSeat = vi.fn(), onDecline = vi.fn();
    return { onClose, onRetry, onSeat, onDecline, tree: walk(TakeoverResults({ run, colors: COLORS, onClose, onRetry, onSeat, onDecline })) };
  };
  const buttons = (tree: Element[]) => walk(byTestId(tree, "operation-sheet-footer")).filter((node) => node.type === "Pressable");
  const finished: TakeoverRun = { route: null, running: false, seats: [seat("a", { state: "done", line: "Continuing · Previous session archived" }), seat("b", { state: "kept", line: "Not started · Original kept" }), seat("c", { state: "kept", next: "none" })] };

  it("puts Close left and Retry remaining right, counting only seats a retry runs", () => {
    const s = sheet(finished);
    expect(buttons(s.tree).map((node) => node.props.testID)).toEqual(["takeover-results-close", "takeover-results-retry"]);
    expect(buttons(s.tree)[1]!.props).toMatchObject({ accessibilityLabel: "Retry remaining 1", disabled: false });
    (buttons(s.tree)[1]!.props.onPress as () => void)();
    (buttons(s.tree)[0]!.props.onPress as () => void)();
    expect(s.onRetry).toHaveBeenCalledTimes(1);
    expect(s.onClose).toHaveBeenCalledTimes(1);
    expect(text(s.tree)).toContain("Completed seats are not restarted.");
    expect(byTestId(s.tree, "takeover-results-summary")!.props.children).toBe("1 continued · 2 not started");
  });

  it("disables Retry remaining while seats are running and when nothing remains, saying why", () => {
    const running = buttons(sheet({ ...finished, running: true }).tree)[1]!;
    expect(running.props).toMatchObject({ disabled: true, tabIndex: 0, accessibilityLabel: expect.stringMatching(/^Retry remaining \d+\. Wait until every seat has a result\.$/) });
    const none = buttons(sheet({ ...finished, seats: [seat("a", { state: "done" })] }).tree)[1]!;
    expect(none.props).toMatchObject({ accessibilityLabel: "Retry remaining 0. Nothing is left to retry.", disabled: true });
  });

  it("gives a held, unknown or recoverable seat its own button, listing a held review's limits", () => {
    const run: TakeoverRun = { route: null, running: false, seats: [seat("r", { state: "review", reviewId: reviewId("r"), held: { revision: 2, digest: DIGEST, limits: ["The supervisor is not told."] } }),
      seat("u", { state: "unknown", reviewId: reviewId("u"), next: "check" }), seat("c", { state: "recovery", reviewId: reviewId("c"), next: "review" }), seat("d", { state: "done", next: "none" })] };
    const s = sheet(run);
    expect(["takeover-confirm-r", "takeover-check-u", "takeover-finish-c"].map((id) => byTestId(s.tree, id)?.props.accessibilityLabel)).toEqual(["Confirm takeover: Seat r", "Check takeover: Seat u", "Finish takeover: Seat c"]);
    expect(s.tree.some((node) => String(node.props.testID).endsWith("-d") && String(node.props.testID).startsWith("takeover-") && node.props.testID !== "takeover-result-d")).toBe(false);
    expect(text(walk(byTestId(s.tree, "takeover-limits-r")))).toBe("• The supervisor is not told.");
    (byTestId(s.tree, "takeover-check-u")!.props.onPress as () => void)();
    expect(s.onSeat).toHaveBeenCalledExactlyOnceWith("u");
    // A held review is declined beside its Confirm, Decline left; nothing else offers Decline.
    const row = walk(byTestId(s.tree, "takeover-result-r")).filter((node) => node.type === "Pressable").map((node) => node.props.testID);
    expect(row).toEqual(["takeover-decline-r", "takeover-confirm-r"]);
    expect(byTestId(s.tree, "takeover-decline-r")!.props.accessibilityLabel).toBe("Decline takeover: Seat r");
    expect(s.tree.filter((node) => String(node.props.testID).startsWith("takeover-decline-"))).toHaveLength(1);
    (byTestId(s.tree, "takeover-decline-r")!.props.onPress as () => void)();
    expect(s.onDecline).toHaveBeenCalledExactlyOnceWith("r");
  });

  // R6b: the reason a seat was kept, and why Retry leaves it out, is shown in full, never clipped to a few lines.
  it("shows each seat's whole reason, unclipped", () => {
    const reason = `Codex cannot keep this seat's permissions (bypassPermissions on claude-b): no permission mapping between providers is qualified. Nothing was sent; the original is kept. Retry remaining skips it: asking again gets the same answer.`;
    const s = sheet({ route: null, running: false, seats: [seat("k", { state: "kept", line: "Not started · Original kept", detail: reason, next: "none" })] });
    const detail = walk(byTestId(s.tree, "takeover-result-k")).find((node) => node.type === "Text" && node.props.children === reason)!;
    expect(detail.props.numberOfLines).toBeUndefined();
  });

  it("says Auto used each seat's chain, or that a chosen model applied to this takeover only", () => {
    expect(text(sheet(finished).tree)).toContain("Auto used each seat's chain.");
    const chosen = text(sheet({ ...finished, route: { provider: "codex", model: "gpt-6-sol", effort: "high", featureValues: {} } }).tree);
    expect(chosen).toContain("S6 was chosen for this takeover only. Defaults stay unchanged.");
    expect(chosen).not.toContain("Auto used");
  });
});
