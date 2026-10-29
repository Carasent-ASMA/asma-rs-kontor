import { describe, expect, it } from "vitest";
import { attachOutcomeSchema, navigatorCapabilitiesSchema, summaryRevisionSchema, usageTotalsSchema, type SummaryRevision } from "../../paseo-plugin/shared/session-navigator";
import { createOptionsSchema, createOutcomeSchema, type CreateFeature } from "../../paseo-plugin/shared/session-create";
import { SYNTHETIC_RECORDED_TOTALS, SYNTHETIC_TOTALS } from "../src/fixtures/integration";
import type { FixtureAgent, FixtureSession } from "../src/fixtures/sessions";
import { fixtureCreate, settingsReadback } from "../src/host/session-create";
import { fixtureNavigator } from "../src/host/session-navigator";

const session = (id: string, at: string): FixtureSession => ({ project: null, agent: { id, title: `Fixture ${id}`, status: "idle", createdAt: at, updatedAt: at,
  lastUserMessageAt: at, cwd: "/fixture", workspaceId: null, provider: "codex", model: "gpt-6-astra", labels: {} } });
/** Sources a and b, with a's r1 and r2 saved across a rename and a newer source event, so their texts differ. */
async function saved() {
  const rows = [session("agt_a", "2026-10-25T10:00:00Z"), session("agt_b", "2026-10-25T10:00:00Z")];
  const nav = fixtureNavigator(() => rows);
  const first = summaryRevisionSchema.parse(await nav.invoke("sessions.summary-save", { agentId: "agt_a" }));
  Object.assign(rows[0]!.agent, { title: "Fixture agt_a, renamed", lastUserMessageAt: "2026-10-25T11:00:00Z" });
  const second = summaryRevisionSchema.parse(await nav.invoke("sessions.summary-save", { agentId: "agt_a" }));
  return { rows, nav, first, second };
}

describe("simulated navigator host", () => {
  it("saves summaries as revisions in page memory: the same source event keeps the revision, a newer one makes the next", async () => {
    const rows = [session("agt_a", "2026-10-25T10:00:00Z")];
    const { invoke } = fixtureNavigator(() => rows);
    expect(navigatorCapabilitiesSchema.parse(await invoke("sessions.navigator-capabilities", {}))).toMatchObject({ summaries: { supported: true }, usageTotals: { supported: false } });
    expect(await invoke("sessions.summary-latest", { agentIds: ["agt_a"] })).toEqual({ agt_a: null });
    const first = summaryRevisionSchema.parse(await invoke("sessions.summary-save", { agentId: "agt_a" }));
    expect(first).toMatchObject({ revision: 1, sourceAgentId: "agt_a", cutoff: "2026-10-25T10:00:00Z" });
    expect(first.text).toMatch(/^Task: SIMULATED summary of “Fixture agt_a”\. No transcript was read\./);
    expect(await invoke("sessions.summary-save", { agentId: "agt_a" })).toEqual(first);
    rows[0]!.agent.lastUserMessageAt = "2026-10-25T11:00:00Z";
    const second = summaryRevisionSchema.parse(await invoke("sessions.summary-save", { agentId: "agt_a" }));
    expect(second).toMatchObject({ summaryId: first.summaryId, revision: 2, cutoff: "2026-10-25T11:00:00Z" });
    expect(await invoke("sessions.summary-latest", { agentIds: ["agt_a"] })).toEqual({ agt_a: second });
  });

  it("keeps r1 exactly as saved after r2, whatever a caller does to the copy it read", async () => {
    const { nav, first, second } = await saved();
    expect(second.text).not.toBe(first.text);
    const read = summaryRevisionSchema.parse(await nav.invoke("sessions.summary-read", { summaryId: first.summaryId, revision: 1 }));
    expect(read).toEqual(first);
    read.text = "tampered";
    read.sections.task = "tampered";
    expect(await nav.invoke("sessions.summary-read", { summaryId: first.summaryId, revision: 1 })).toEqual(first);
    expect(Object.isFrozen(nav.resolve({ summaryId: first.summaryId, revision: 1 }).sections)).toBe(true);
  });

  it("drafts the exact revision into another seat's draft, sends nothing, and keeps the draft inspectable", async () => {
    const { nav, second } = await saved();
    expect(attachOutcomeSchema.parse(await nav.invoke("sessions.summary-attach", { summaryId: second.summaryId, revision: 2, destinationAgentId: "agt_b" })))
      .toMatchObject({ state: "drafted", destinationAgentId: "agt_b", revision: 2, message: expect.stringContaining("Nothing was sent") });
    expect(nav.inspect().drafts).toEqual({ agt_b: { summaryId: second.summaryId, revision: 2, sourceAgentId: "agt_a", text: second.text, at: expect.any(String) } });
  });

  it("never replaces a destination's draft: the same revision again changes nothing, any other is refused", async () => {
    const { nav, first, second } = await saved();
    const attach = async (revision: number) => attachOutcomeSchema.parse(await nav.invoke("sessions.summary-attach", { summaryId: first.summaryId, revision, destinationAgentId: "agt_b" }));
    await attach(1);
    const drafted = nav.inspect().drafts;
    expect(await attach(1)).toMatchObject({ state: "drafted", revision: 1, message: expect.stringContaining("already in the draft") });
    expect(await attach(2)).toMatchObject({ state: "refused", message: "SIMULATED: “Fixture agt_b” already has a draft (r1 of agt_a). Attach never replaces a draft; send or clear it there first. Nothing was drafted." });
    expect(nav.inspect().drafts).toEqual(drafted);
    expect(nav.inspect().drafts.agt_b!.text).toBe(first.text);
    expect(second.text).not.toBe(first.text);
  });

  it("refuses an unknown summary or revision before any draft, and a summary attached back to its own source", async () => {
    const { nav, first } = await saved();
    await expect(nav.invoke("sessions.summary-attach", { summaryId: first.summaryId, revision: 3, destinationAgentId: "agt_b" })).rejects.toThrow(/no saved summary .* r3; nothing was placed/);
    await expect(nav.invoke("sessions.summary-attach", { summaryId: crypto.randomUUID(), revision: 1, destinationAgentId: "agt_b" })).rejects.toThrow(/no saved summary/);
    await expect(nav.invoke("sessions.summary-attach", { summaryId: first.summaryId, revision: 1, destinationAgentId: "agt_missing" })).rejects.toThrow(/no session agt_missing/);
    expect(await nav.invoke("sessions.summary-attach", { summaryId: first.summaryId, revision: 1, destinationAgentId: "agt_a" })).toMatchObject({ state: "refused" });
    expect(nav.inspect().drafts).toEqual({});
  });

  it("fails visibly for totals it never read and for anything it does not serve", async () => {
    const { invoke } = fixtureNavigator(() => []);
    await expect(invoke("sessions.usage-totals", { agentIds: ["agt_a"] })).rejects.toThrow(/no session token totals were read/);
    await expect(invoke("sessions.summary-save", { agentId: "agt_missing" })).rejects.toThrow(/no session agt_missing/);
    await expect(invoke("sessions.takeover-review", {})).rejects.toThrow(/Unsupported fixture navigator RPC/);
  });

  it("serves SYNTHETIC totals only when a scenario supplies them, null for any session they do not list", async () => {
    const { invoke } = fixtureNavigator(() => [], SYNTHETIC_TOTALS);
    expect(navigatorCapabilitiesSchema.parse(await invoke("sessions.navigator-capabilities", {}))).toMatchObject({ usageTotals: { supported: true }, takeoverBulk: { supported: true }, takeoverRoute: { supported: true } });
    const totals = await invoke("sessions.usage-totals", { agentIds: ["agt_swe_99103", "agt_qa_99103", "agt_other"] }) as Record<string, unknown>;
    expect(usageTotalsSchema.parse(totals.agt_swe_99103)).toEqual({ input: 1_200_000, output: 84_600, source: "SYNTHETIC totals for review; not read from any host" });
    expect(totals.agt_qa_99103).toMatchObject({ output: null });
    expect(totals.agt_other).toBeNull();
  });
});

describe("simulated creation pins and settings readback", () => {
  const base = { role: "LSA" as const, start: "ad-hoc" as const, projectId: "prj_proof_adam", topic: "Fictional review", provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "fixture-mode" };
  /** A seat store that keeps what `add` stores; with `drop` it loses the listed settings, as a host that ignores them would. */
  function seats(drop = false) {
    const stored = new Map<string, FixtureAgent>();
    return { stored, read: (id: string) => stored.get(id), add: (request: { requestId: string }, title: string, workspaceId: string, features: CreateFeature[] | null) => {
      const id = `agt_fixture_create_${request.requestId}`;
      stored.set(id, { ...session(id, "2026-10-25T12:00:00Z").agent, title, workspaceId, ...(features && !drop ? { features } : {}) });
      return id;
    } };
  }

  it("pins the exact revision of the named source on the created seat, and r1 stays r1 after r2", async () => {
    const { nav, first, second } = await saved();
    const create = fixtureCreate(true, seats(), nav);
    const outcome = createOutcomeSchema.parse(await create("sessions.create", { ...base, requestId: crypto.randomUUID(), sourceAgentId: "agt_a", sourceSummary: { summaryId: first.summaryId, revision: 1 } }));
    expect(outcome).toMatchObject({ state: "verified", message: expect.stringContaining("Saved summary r1 of agt_a pinned in page memory") });
    expect(nav.pinned(outcome.agentId!)).toMatchObject({ summaryId: first.summaryId, revision: 1, text: first.text });
    expect(nav.pinned(outcome.agentId!)?.text).not.toBe(second.text);
  });

  it.each([
    ["an unknown revision", (first: SummaryRevision) => ({ sourceAgentId: "agt_a", sourceSummary: { summaryId: first.summaryId, revision: 9 } }), /no saved summary .* r9/],
    ["another source's revision", (first: SummaryRevision) => ({ sourceAgentId: "agt_b", sourceSummary: { summaryId: first.summaryId, revision: 1 } }), /belongs to agt_a, not agt_b/],
  ] as const)("refuses %s before creating anything", async (_, source, problem) => {
    const { nav, first } = await saved();
    const store = seats(), create = fixtureCreate(true, store, nav);
    const outcome = createOutcomeSchema.parse(await create("sessions.create", { ...base, requestId: crypto.randomUUID(), ...source(first) }));
    expect(outcome).toMatchObject({ state: "refused", agentId: null, message: expect.stringMatching(problem) });
    expect(store.stored.size).toBe(0);
    expect(nav.inspect().pins).toEqual({});
  });

  it("reads Speed and Plan back from the seat it created, not from the request", async () => {
    const store = seats(), create = fixtureCreate(true, store);
    const outcome = createOutcomeSchema.parse(await create("sessions.create", { ...base, requestId: crypto.randomUUID(), featureValues: { service_tier: "priority", plan_mode: true } }));
    expect(outcome).toMatchObject({ state: "verified", differences: [], message: expect.stringContaining("Settings read back: Speed Fast, Plan on.") });
    expect(store.read(outcome.agentId!)?.features?.map((feature) => [feature.id, feature.value])).toEqual([["service_tier", "priority"], ["plan_mode", true]]);
  });

  it("holds a SIMULATED pending creation that Check reads as unknown and that blocks a new one", async () => {
    const store = seats(), create = fixtureCreate(true, store, null, true);
    const pending = createOptionsSchema.parse(await create("sessions.create-options", {})).pending!;
    expect(pending).toMatchObject({ state: "unknown", agentId: null, message: expect.stringContaining("SIMULATED") });
    expect(await create("sessions.create-check", { requestId: pending.requestId })).toEqual(pending);
    expect(await create("sessions.create", { ...base, requestId: crypto.randomUUID() })).toMatchObject({ state: "refused", message: expect.stringContaining("is not settled") });
    expect(store.stored.size).toBe(0);
  });

  it("reports differences when the seat does not hold the requested settings", async () => {
    const create = fixtureCreate(true, seats(true));
    const outcome = createOutcomeSchema.parse(await create("sessions.create", { ...base, requestId: crypto.randomUUID(), featureValues: { service_tier: "priority", plan_mode: true } }));
    expect(outcome).toMatchObject({ state: "differences", differences: ["service_tier reads not reported.", "plan_mode reads not reported."] });
    expect(outcome.message).not.toContain("Settings read back");
    expect(settingsReadback(undefined, {})).toEqual({ applied: [], differences: [] });
  });
});

it("serves recorded child usage separately from unknown cumulative totals in the review fixture", async () => {
 const { invoke } = fixtureNavigator(() => [], SYNTHETIC_RECORDED_TOTALS);
 const rows = await invoke("sessions.usage-totals", { agentIds: ["agt_model_report_0"] }) as Record<string, unknown>;
 expect(usageTotalsSchema.parse(rows.agt_model_report_0)).toMatchObject({ input: null, output: null, scope: "provider-cumulative", recordedChildUsage: {
  input: 451_900_000, output: 1_700_000, responses: 2048, qualification: "Excludes parent history; completeness and billing unverified.",
 } });
});
