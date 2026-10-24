import { describe, expect, it } from "vitest";
import {
  failureAt, observeRefusal, quietAfterRefusal, recoveredAfterRefusal, refusalAsOf, refusalKind, refusalState, type ProviderRefusalRead, type RefusalObservation,
} from "../shared/provider-block";

// Synthetic refusal shaped like an OpenCode APIError: the response date is the only zoned time; the reset has no zone.
const QUOTA_REFUSAL = 'APIError: {"statusCode":429,"responseHeaders":{"date":"Tue, 20 Oct 2026 21:30:52 GMT"},"responseBody":"{\\"error\\":{\\"code\\":\\"1308\\",\\"message\\":\\"5-hour usage limit reached; reset after 2026-10-21 07:45:40\\"}}"}';
const FAILED_AT = "2026-10-20T21:30:52.000Z";

describe("refusal classifier", () => {
  // The single pattern this module replaced, kept here only to prove the split matches exactly the same text.
  const FORMER = /quota|usage.?limit|rate.?limit|exhaust|unauthori[sz]ed|authenticat|\b401\b|\b403\b|billing|credit|api.?key|credential|token.expired|permission.denied/i;
  it("matches exactly what the former lifecycle pattern matched", () => {
    const corpus = [QUOTA_REFUSAL, "Usage limit reached", "rate-limit", "Quota exhausted", "billing problem", "out of credit", "401 Unauthorized", "HTTP 403", "4013",
      "Authentication failed", "invalid api_key", "credential missing", "token expired", "permission denied", "Turn interrupted by transport failure", "", "error 4030"];
    for (const text of corpus) expect(refusalKind({ lastError: text }) !== null, text).toBe(FORMER.test(text));
  });
  it("names the kind and treats a host provider-unavailable flag as unavailable", () => {
    expect(refusalKind({ lastError: QUOTA_REFUSAL })).toBe("quota");
    expect(refusalKind({ lastError: "401 Unauthorized" })).toBe("auth");
    expect(refusalKind({ lastError: "Turn interrupted", providerUnavailable: true })).toBe("unavailable");
    expect(refusalKind({ lastError: "Turn interrupted" })).toBeNull();
  });
});

describe("failure time comes only from the refusal text", () => {
  it.each([
    ["the responseHeaders date", QUOTA_REFUSAL, FAILED_AT],
    ["a capitalised Date header", '{"responseHeaders":{"content-type":"application/json","Date":"Tue, 20 Oct 2026 21:30:52 GMT"}}', FAILED_AT],
    ["empty responseHeaders and a body error.reset.date", '{"responseHeaders":{},"responseBody":"{\\"error\\":{\\"reset\\":{\\"date\\":\\"Wed, 21 Oct 2026 07:45:40 GMT\\"}}}"}', "unknown"],
    ["empty responseHeaders and an unescaped body reset date", '{"responseHeaders":{},"responseBody":{"error":{"reset":{"date":"Wed, 21 Oct 2026 07:45:40 GMT"}}}}', "unknown"],
    ["a reset header", '{"responseHeaders":{"x-reset-date":"Wed, 21 Oct 2026 07:45:40 GMT"}}', "unknown"],
    ["a date outside responseHeaders", '{"date":"Tue, 20 Oct 2026 21:30:52 GMT"}', "unknown"],
    ["a raw header line", "HTTP/1.1 429\ndate: Tue, 20 Oct 2026 21:30:52 GMT\n", "unknown"],
    ["a body responseHeaders before an empty outer one", '{"responseBody":{"responseHeaders":{"date":"Sat, 24 Oct 2026 18:00:00 GMT"}},"responseHeaders":{},"message":"usage limit"}', "unknown"],
    ["a nested error envelope without outer headers", 'APIError: {"error":{"responseHeaders":{"date":"Tue, 20 Oct 2026 21:30:52 GMT"}}}', "unknown"],
    ["a header date that is not an HTTP date", '{"responseHeaders":{"date":"2026-10-20T21:30:52Z"}}', "unknown"],
    ["text that is not one JSON envelope", 'APIError: {"responseHeaders":{"date":"Tue, 20 Oct 2026 21:30:52 GMT"}} (retrying)', "unknown"],
    ["a reset time without a zone", "5-hour usage limit; reset after 2026-10-21 07:45:40", "unknown"],
    ["a zoned reset time", "usage limit; try again at 2026-10-21T07:45:40Z", "unknown"],
    ["a reset labelled date", "usage limit; reset date: Tue, 20 Oct 2026 21:30:52 GMT", "unknown"],
    ["no time at all", "You've hit your usage limit.", "unknown"],
  ])("%s", (_name, text, expected) => expect(failureAt(text)).toBe(expected));
  it("is unknown for missing text", () => expect(failureAt(undefined)).toBe("unknown"));
});

describe("observation binding", () => {
  const session = { provider: "codex-fixture-account", model: "gpt-6.1-sol", runtimeInfo: { model: "gpt-6.1-sol-runtime" }, status: "error", lastError: "You've hit your usage limit." };
  it("binds the read time, the route read and the verbatim text; never a host timestamp as the failure time", () => {
    expect(observeRefusal(session, "2026-10-24T10:00:00.000Z")).toEqual({ kind: "quota", failureAt: "unknown", observedAt: "2026-10-24T10:00:00.000Z",
      route: { provider: "codex-fixture-account", model: "gpt-6.1-sol-runtime" }, message: "You've hit your usage limit." });
    expect(observeRefusal({ ...session, lastError: QUOTA_REFUSAL }, "2026-10-24T10:00:00.000Z")?.failureAt).toBe(FAILED_AT);
  });
  it("observes nothing without a classified refusal", () => {
    expect(observeRefusal({ ...session, lastError: "Turn interrupted by transport failure" }, "now")).toBeNull();
    expect(observeRefusal({ ...session, status: "running", lastError: undefined }, "now")).toBeNull();
  });
});

describe("state and recovery", () => {
  const observation: RefusalObservation = { kind: "quota", failureAt: FAILED_AT, observedAt: "2026-10-24T10:00:00.000Z", route: { provider: "claude", model: "claude-opus-5-5" }, message: QUOTA_REFUSAL };
  it("is current only when this read saw it on a native error", () => {
    expect(refusalState(observation, { seen: true, readable: true, error: true, recovered: false })).toMatchObject({ read: "fresh", state: "current", recovery: "not-confirmed" });
    expect(refusalState(observation, { seen: true, readable: true, error: false, recovered: false })).toMatchObject({ read: "fresh", state: "history", recovery: "not-confirmed" });
    expect(refusalState(observation, { seen: false, readable: true, error: false, recovered: false })).toMatchObject({ read: "stale", state: "history", recovery: "not-confirmed" });
    expect(refusalState(observation, { seen: false, readable: false, error: true, recovered: false })).toMatchObject({ read: "unreadable", state: "history", recovery: "not-confirmed" });
  });
  it("confirms recovery only for a refusal this read no longer shows", () => {
    expect(refusalState(observation, { seen: false, readable: true, error: false, recovered: true }).recovery).toBe("confirmed");
    expect(refusalState(observation, { seen: true, readable: true, error: true, recovered: true }).recovery).toBe("not-confirmed");
  });

  const checkedAt = "2026-10-24T10:00:00.000Z", at = (ms: number) => Date.parse(checkedAt) + ms;
  const result: ProviderRefusalRead = { availability: { agentId: "a", state: "readable", reason: "", checkedAt, revision: "u1" },
    refusal: refusalState(observation, { seen: true, readable: true, error: true, recovered: false }) };
  it("ages a fresh read into stale history; expiry never clears or downgrades recovery", () => {
    expect(refusalAsOf(result, { id: "a", updatedAt: "u1" }, at(59_999))).toMatchObject({ read: "fresh", state: "current" });
    for (const [name, entry, now] of [["expired", { id: "a", updatedAt: "u1" }, at(60_000)], ["negative age", { id: "a", updatedAt: "u1" }, at(-1)],
      ["revision changed", { id: "a", updatedAt: "u2" }, at(1)], ["another session", { id: "b", updatedAt: "u1" }, at(1)]] as const)
      expect(refusalAsOf(result, entry, now), name).toMatchObject({ read: "stale", state: "history", recovery: "not-confirmed", failureAt: FAILED_AT, observedAt: checkedAt });
  });
  it("keeps unreadable and absent results as they are", () => {
    const unreadable = { ...result, refusal: refusalState(observation, { seen: false, readable: false, error: false, recovered: false }) };
    expect(refusalAsOf(unreadable, { id: "a", updatedAt: "u1" }, at(120_000))?.read).toBe("unreadable");
    expect(refusalAsOf({ ...result, refusal: null }, { id: "a", updatedAt: "u1" }, at(120_000))).toBeNull();
  });
  it("needs an exact idle read: closed, a retry start, an active turn or any error is not recovery", () => {
    expect(quietAfterRefusal({ provider: "claude", status: "idle" })).toBe(true);
    for (const patch of [{ status: "closed" }, { status: "running" }, { status: "initializing" }, { status: "error" }, { activeTurn: { turnId: "t" } }, { lastError: "Turn interrupted" }])
      expect(quietAfterRefusal({ provider: "claude", status: "idle", ...patch }), JSON.stringify(patch)).toBe(false);
  });
});

describe("timeline recovery (G7), bound to the stored observation", () => {
  const observedAt = "2026-10-24T10:00:00.000Z", day = 86_400_000;
  const at = (ms: number) => ({ timestamp: new Date(Date.parse(observedAt) + ms).toISOString() });
  const refused = (ms: number) => ({ item: { type: "assistant_message", text: "[System Error] You've hit your usage limit. Try again later." }, ...at(ms) });
  const user = (ms: number) => ({ item: { type: "user_message", text: "Please continue." }, ...at(ms) });
  const ok = (ms: number) => ({ item: { type: "assistant_message", text: "Done: continued the task." }, ...at(ms) });
  it.each([
    ["a completed turn after the observation", [refused(-1_000), user(1_000), ok(2_000)], true],
    ["an error item refusal, then a completed turn", [{ item: { type: "error", message: QUOTA_REFUSAL }, ...at(-1_000) }, user(1_000), ok(2_000)], true],
    ["the refusal alone", [refused(-1_000)], false],
    ["a retry start with no answer", [refused(-1_000), user(1_000)], false],
    ["a retry refused again", [refused(-1_000), user(1_000), refused(2_000)], false],
    ["a later failure without its own completed turn", [refused(-1_000), user(1_000), ok(2_000), user(3_000), { item: { type: "assistant_message", text: "[System Error] transport closed" }, ...at(4_000) }], false],
    ["a later error notification", [refused(-1_000), user(1_000), ok(2_000), { item: { type: "notification", level: "error", message: "stream failed" }, ...at(3_000) }], false],
    ["no refusal in the tail", [user(1_000), ok(2_000)], false],
    ["an older refusal answered before the observation", [refused(-day), user(-day + 1_000), ok(-day + 2_000)], false],
    ["an older answered refusal while the observed one is not shown", [refused(-day), user(-day + 1_000), ok(-day + 2_000), user(-1_000), user(1_000), ok(2_000)], false],
    ["an answer before the observation", [refused(-3_000), user(-2_000), ok(-1_000)], false],
    ["an entry without a timestamp", [refused(-1_000), { item: { type: "user_message", text: "x" } }, ok(2_000)], false],
    ["entries out of order", [refused(-1_000), ok(2_000), user(1_000), ok(3_000)], false],
    ["an empty or malformed tail", [null, {}, { item: null }, { item: { text: "no type" } }], false],
  ] as const)("%s", (_name, entries, expected) => expect(recoveredAfterRefusal(entries, { kind: "quota", observedAt })).toBe(expected));
  it("proves nothing against an unreadable observation time", () => expect(recoveredAfterRefusal([refused(-1_000), user(1_000), ok(2_000)], { kind: "quota", observedAt: "unknown" })).toBe(false));
  it("needs the stored kind: a quota refusal in the tail does not recover a stored auth refusal", () =>
    expect(recoveredAfterRefusal([refused(-1_000), user(1_000), ok(2_000)], { kind: "auth", observedAt })).toBe(false));
  it.each((["unavailable", "unknown"] as const).flatMap((kind) => ([
    ["an older quota-recovery tail after only a readiness change", [refused(-day), user(-day + 1_000), ok(-day + 2_000)], false],
    ["a retry start with no answer", [ok(-1_000), user(1_000)], false],
    ["a turn answered after the observation", [ok(-1_000), user(1_000), ok(2_000)], true],
  ] as const).map(([name, entries, expected]) => [kind, name, entries, expected] as const)))("%s, bound by time only: %s", (kind, _name, entries, expected) => expect(recoveredAfterRefusal(entries, { kind, observedAt })).toBe(expected));
});
