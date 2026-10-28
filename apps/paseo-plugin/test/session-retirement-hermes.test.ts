import { describe, expect, it } from "vitest";
import type { useSessionRetirementProjection as Hook } from "../client/use-session-lifecycle";
import * as lifecycle from "../shared/session-lifecycle";
import * as promotion from "../shared/session-promotion";
import type { LifecycleReview, LifecycleRow, UnresolvedRef } from "../shared/session-lifecycle";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { sessionEntry } from "./session-fixtures";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";

const settle = async () => { for (let n = 0; n < 12; n++) await new Promise((resolve) => setTimeout(resolve, 0)); };
const reviewId = (id: string) => `00000000-0000-4000-8000-${Buffer.from(id).toString("hex").slice(-12).padStart(12, "0")}`;
const row = (id: string, outcome: LifecycleRow["outcome"] = "uncertain", runtime?: LifecycleRow["runtime"]): LifecycleRow =>
  ({ id, title: `Fixture ${id}`, eligible: true, outcome, message: "Fixture receipt", reason: "Fixture reason", ...(runtime ? { runtime } : {}) }) as LifecycleRow;
const review = (id: string, rows: LifecycleRow[], unresolved?: UnresolvedRef[]): LifecycleReview =>
  ({ id, createdAt: "2026-10-24T08:00:00.000Z", action: "archive", scope: "filtered", target: lifecycle.DEFAULT_RECOVERY_TARGET, rows, limitation: lifecycle.TAKEOVER_LIMIT, ...(unresolved ? { unresolved } : {}) });
const R1 = reviewId("review-one"), R2 = reviewId("review-two");
const A = "fixture-a", B = "fixture-b", C = "fixture-c", D = "fixture-d";
const refs: UnresolvedRef[] = [{ reviewId: R1, agentId: A }, { reviewId: R2, agentId: B }, { reviewId: R1, agentId: C }];
const directory = { status: "ready" as const, live: true, lastUpdatedAt: Date.parse("2026-10-24T08:01:00.000Z"), inventoryComplete: true, includeArchived: true };

function harness(host: () => string, entries: () => ReturnType<typeof sessionEntry>[], handlers: Record<string, (input: Record<string, unknown>) => Promise<unknown>>,
  extra: { readonly sources?: readonly ReadSourceSummary[]; readonly reads?: Readonly<Record<string, SourceRead>>; readonly associations?: Readonly<Record<string, readonly string[]>> } = {}) {
  const calls: Array<[string, Record<string, unknown>]> = [];
  const rpc: Record<string, (input: Record<string, unknown>) => Promise<unknown>> = {};
  for (const name of Object.keys(handlers)) rpc[name] = (input) => { calls.push([name, input]); return handlers[name]!(input); };
  rpc["sessions.promotion-list"] ??= async () => [];
  const { react, render } = deferredReact();
  const { useSessionRetirementProjection } = loadHermesScoped<{ useSessionRetirementProjection: typeof Hook }>("../client/use-session-lifecycle.ts", {
    react, "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => rpc[contract.name] }, "../shared/session-lifecycle": lifecycle, "../shared/session-promotion": promotion,
  });
  const draw = () => render(() => useSessionRetirementProjection(host(), { entries: entries(), directory, enabled: true, ...extra }));
  return { draw, calls, count: (name: string) => calls.filter(([called]) => called === name).length };
}

describe("read-only retirement projection under the iOS (Hermes) scoping of plugin bundles", () => {
  it("reads each exact review once and shows every affected session's unsettled intent", async () => {
    const h = harness(() => "host", () => [sessionEntry(A), sessionEntry(B), sessionEntry(C), sessionEntry(D)], {
      "sessions.lifecycle-review": async (input) => input.reviewId === R1 ? review(R1, [row(A), row(C)]) : input.reviewId === R2 ? review(R2, [row(B)]) : review("00000000-0000-4000-8000-0000000000aa", [], refs),
    });
    h.draw(); await settle();
    const drawn = h.draw();
    expect(h.count("sessions.lifecycle-review")).toBe(3);
    expect(h.calls.filter(([, input]) => input.reviewId === R1)).toHaveLength(1);
    expect(h.calls.filter(([, input]) => input.reviewId === R2)).toHaveLength(1);
    expect(drawn.receipts).toMatchObject({ state: "ready", complete: true, error: null });
    for (const id of [A, B, C]) expect(drawn.projections.get(id)?.intent).toMatchObject({ state: "unsettled", sessionId: id });
    expect(drawn.projections.get(D)?.intent.state).toBe("none");
  });


  it("keeps a stale promotion visible as historical evidence, never as current verified scope", async () => {
    const record = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e2", agentId: A, epic: "ASMA-100", branch: "promote", state: "promoted",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Fixture", titleAuto: "LSA • ADHOC • Fixture", observedAt: "2026-10-24T08:00:00.000Z", revision: "r1" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "b".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: A,
      summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted",
      createdAt: "2026-10-24T08:00:00.000Z", updatedAt: "2026-10-24T08:00:00.000Z",
    });
    const h = harness(() => "host", () => [sessionEntry(A)], {
      "sessions.lifecycle-review": async () => review("00000000-0000-4000-8000-0000000000aa", [], []),
      "sessions.promotion-list": async () => [{ record, validation: { state: "stale", message: "persona drift" } }],
    });
    h.draw(); await settle();
    const drawn = h.draw();
    expect(drawn.projections.get(A)?.promotion).toMatchObject({ agentId: A, epic: "ASMA-100", source: { fresh: "stale" } });
    expect(drawn.projections.get(A)?.scope.claims).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "epic", value: "ASMA-100", binding: "claimed" }),
    ]));
    expect(drawn.projections.get(A)?.conflicts).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "stale-or-mismatched-evidence" }),
    ]));
  });

  it("invalidates a current promotion when the native directory row drifts", async () => {
    const record = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e3", agentId: A, epic: "ASMA-100", branch: "promote", state: "promoted",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Fixture", titleAuto: "LSA • ADHOC • Fixture", observedAt: "2026-10-24T08:00:00.000Z", revision: "r1" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "c".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: A,
      summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted",
      createdAt: "2026-10-24T08:00:00.000Z", updatedAt: "2026-10-24T08:00:00.000Z",
    });
    let row = sessionEntry(A, { updatedAt: "r1", workspaceId: "wks-adhoc", workspaceName: "ADHOC",
      labels: { role: "LSA", persona: "kontor-lsa-adhoc@fixture" } });
    const h = harness(() => "host", () => [row], {
      "sessions.lifecycle-review": async () => review("00000000-0000-4000-8000-0000000000aa", [], []),
      "sessions.promotion-list": async () => [{ record, validation: { state: "current", message: "matches", revision: "r1" } }],
    });
    h.draw(); await settle();
    expect(h.draw().projections.get(A)?.promotion?.source.fresh).toBe("current");
    row = { ...row, updatedAt: "r2", labels: { ...row.labels, role: "TPM" } };
    const drifted = h.draw();
    expect(drifted.projections.get(A)?.promotion?.source.fresh).toBe("stale");
    expect(drifted.projections.get(A)?.scope.claims).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "epic", binding: "claimed" })]));
  });

  it("keeps a promoting promotion visible as historical evidence", async () => {
    const record = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e4", agentId: A, epic: "ASMA-100", branch: "promote", state: "promoting",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Fixture", titleAuto: "LSA • ADHOC • Fixture", observedAt: "2026-10-24T08:00:00.000Z", revision: "r1" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "d".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: null,
      summarySha256: null, summaryRevision: null, acknowledgement: "unknown", message: "pending",
      createdAt: "2026-10-24T08:00:00.000Z", updatedAt: "2026-10-24T08:00:00.000Z",
    });
    const h = harness(() => "host", () => [sessionEntry(A, { workspaceId: "wks-adhoc", workspaceName: "ADHOC",
      labels: { role: "LSA", persona: "kontor-lsa-adhoc@fixture" } })], {
      "sessions.lifecycle-review": async () => review("00000000-0000-4000-8000-0000000000ab", [], []),
      "sessions.promotion-list": async () => [{ record, validation: { state: "unknown", message: "The promotion is not a settled canonical binding.", revision: null } }],
    });
    h.draw(); await settle();
    const projection = h.draw().projections.get(A);
    expect(projection?.promotion?.source.fresh).toBe("unknown");
    expect(projection?.scope.claims).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "epic", value: "ASMA-100", binding: "claimed" })]));
  });

  it("keeps the promoted owner when handoff history names the same lead", async () => {
    const promoted = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e5", agentId: A, epic: "ASMA-100", branch: "promote", state: "promoted",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Fixture", titleAuto: "LSA • ADHOC • Fixture", observedAt: "2026-10-24T08:00:00.000Z", revision: "r1" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "e".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: A,
      summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted",
      createdAt: "2026-10-24T08:00:00.000Z", updatedAt: "2026-10-24T08:00:00.000Z",
    });
    const handoff = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e6", agentId: B, epic: "ASMA-100", branch: "handoff", state: "handoff",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Source", titleAuto: "LSA • ADHOC • Source", observedAt: "2026-10-24T08:00:00.000Z", revision: "r2" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "f".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: A,
      summarySha256: "a".repeat(64), summaryRevision: "r2", acknowledgement: "verified", message: "handoff",
      createdAt: "2026-10-24T08:01:00.000Z", updatedAt: "2026-10-24T08:01:00.000Z",
    });
    const h = harness(() => "host", () => [sessionEntry(A, { updatedAt: "r1", workspaceId: "wks-adhoc", workspaceName: "ADHOC",
      labels: { role: "LSA", persona: "kontor-lsa-adhoc@fixture" } })], {
      "sessions.lifecycle-review": async () => review("00000000-0000-4000-8000-0000000000ac", [], []),
      "sessions.promotion-list": async () => [{ record: handoff, validation: { state: "current", message: "receiver matches", revision: "r1" } },
        { record: promoted, validation: { state: "current", message: "matches", revision: "r1" } }],
    });
    h.draw(); await settle();
    const projection = h.draw().projections.get(A);
    expect(projection?.promotion).toMatchObject({ epic: "ASMA-100", source: { fresh: "current" } });
    expect(projection?.scope.claims).toEqual(expect.arrayContaining([expect.objectContaining({ kind: "epic", value: "ASMA-100", binding: "verified" })]));
  });

  it("retains known promotion history when a later list refresh fails", async () => {
    const record = promotion.promotionRecordSchema.parse({
      schemaVersion: 1, requestId: "00000000-0000-4000-8000-0000000000e7", agentId: A, epic: "ASMA-100", branch: "promote", state: "promoted",
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: "LSA • ADHOC • Fixture", titleAuto: "LSA • ADHOC • Fixture", observedAt: "2026-10-24T08:00:00.000Z", revision: "r1" },
      logical: { workspace: "LSA", node: "epic:ASMA-100", bindingRevision: "a".repeat(64) },
      persona: { issued: "kontor-lsa-adhoc@fixture", effective: "kontor-lsa-adhoc@fixture" }, canonicalAgentId: A,
      summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "promoted",
      createdAt: "2026-10-24T08:00:00.000Z", updatedAt: "2026-10-24T08:00:00.000Z",
    });
    let lists = 0;
    const h = harness(() => "host", () => [sessionEntry(A, { updatedAt: "r1", workspaceId: "wks-adhoc", workspaceName: "ADHOC",
      labels: { role: "LSA", persona: "kontor-lsa-adhoc@fixture" } })], {
      "sessions.lifecycle-review": async () => review("00000000-0000-4000-8000-0000000000ad", [], []),
      "sessions.promotion-list": async () => { lists++; if (lists > 1) throw new Error("list failed"); return [{ record, validation: { state: "current", message: "matches", revision: "r1" } }]; },
    });
    h.draw(); await settle();
    const first = h.draw();
    expect(first.projections.get(A)?.promotion?.source.fresh).toBe("current");
    first.reload(); await settle();
    const failed = h.draw().projections.get(A)?.promotion;
    expect(failed?.source.fresh).toBe("unknown");
    expect(failed?.epic).toBe("ASMA-100");
  });
  it("preserves successful receipts and only marks the failed receipt's sessions Unknown", async () => {
    let failSecond = true, failFirst = false;
    const h = harness(() => "host", () => [sessionEntry(A), sessionEntry(B), sessionEntry(D)], {
      "sessions.lifecycle-review": async (input) => {
        if (!input.reviewId) return review("00000000-0000-4000-8000-0000000000aa", [], refs.slice(0, 2));
        if (input.reviewId === R1 && failFirst) throw new Error("fixture first read failed");
        if (input.reviewId === R2 && failSecond) throw new Error("fixture second read failed");
        return input.reviewId === R1 ? review(R1, [row(A)]) : review(R2, [row(B)]);
      },
    });
    h.draw(); await settle();
    expect(h.draw().receipts.state).toBe("partial");
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(B)?.intent.state).toBe("unknown");
    expect(h.draw().projections.get(D)?.intent.state).toBe("none");
    failSecond = false; h.draw().reload(); await settle(); h.draw();
    expect(h.draw().receipts.state).toBe("ready");
    expect(h.draw().projections.get(B)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    failFirst = true; h.draw().reload(); await settle(); h.draw();
    expect(h.draw().receipts.state).toBe("partial");
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(B)?.intent.state).toBe("unsettled");
  });

  it("keeps known receipts after a list read failure and records unknown coverage", async () => {
    let failList = false;
    const h = harness(() => "host", () => [sessionEntry(A), sessionEntry(D)], {
      "sessions.lifecycle-review": async (input) => {
        if (!input.reviewId) { if (failList) throw new Error("fixture list read failed"); return review("00000000-0000-4000-8000-0000000000aa", [], [{ reviewId: R1, agentId: A }]); }
        return review(R1, [row(A)]);
      },
    });
    h.draw(); await settle(); h.draw();
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    failList = true; h.draw().reload(); await settle(); h.draw();
    expect(h.draw().receipts).toMatchObject({ state: "unknown", complete: false, error: "fixture list read failed" });
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(D)?.intent).toMatchObject({ state: "unknown", sessionId: D });
  });

  it("never publishes an old host's receipt into the current host", async () => {
    let host = "host-a";
    const pending = new Map<string, (value: LifecycleReview | null) => void>();
    const h = harness(() => host, () => host === "host-a" ? [sessionEntry(A)] : [sessionEntry(B)], {
      "sessions.lifecycle-review": (input) => new Promise((resolve) => {
        if (input.reviewId) resolve(review(input.reviewId as string, [row(input.reviewId === R1 ? A : B)]));
        else pending.set(host, resolve);
      }),
    });
    h.draw(); await settle();
    host = "host-b"; h.draw(); await settle();
    pending.get("host-b")!(review("00000000-0000-4000-8000-0000000000bb", [], [{ reviewId: R2, agentId: B }]));
    await settle(); h.draw();
    expect(h.draw().projections.get(B)?.intent.state).toBe("unsettled");
    pending.get("host-a")!(review("00000000-0000-4000-8000-0000000000aa", [], [{ reviewId: R1, agentId: A }]));
    await settle(); h.draw();
    expect(h.draw().projections.has(A)).toBe(false);
    expect(h.draw().projections.get(B)?.intent.state).toBe("unsettled");
  });

  it("derives plan and title claims with their own source time and never dispatches a lifecycle action", async () => {
    const summary: ReadSourceSummary = { id: "plan-fixture", title: "Fixture plan", kind: "plan", path: "/fixture/plan.md", workstream: null, epic: "ASMA-9", projectId: null, revisionKind: "working copy" };
    const read: SourceRead = { source: summary, status: "ready", revision: "a".repeat(64), readAt: "2026-10-24T08:02:00.000Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [] };
    const h = harness(() => "host", () => [sessionEntry(A, { title: "SWE • ASMA-9 • fixture" })], { "sessions.lifecycle-review": async () => null },
      { sources: [summary], reads: { [summary.id]: read }, associations: { [A]: [summary.id] } });
    h.draw(); await settle(); const projection = h.draw().projections.get(A)!;
    expect(projection.responsibility.claimedRole).toBe("SWE");
    expect(projection.scope.claims.map((entry) => [entry.kind, entry.value, entry.binding, entry.source.observedAt])).toContainEqual(["epic", "ASMA-9", "claimed", read.readAt]);
    expect(projection.sources.map((entry) => entry.id)).toContain(`source:${summary.id}`);
    expect(h.calls.map(([name]) => name)).toEqual(["sessions.lifecycle-review"]);
    expect(h.calls.every(([name]) => name === "sessions.lifecycle-review")).toBe(true);
    expect(h.calls.some(([name]) => /preview|execute|reconcile|resume|archive|send|timeline|provider/.test(name))).toBe(false);
  });
});

describe("projection regressions for running execution and exact receipt coverage", () => {
  it("retains an archived running snapshot with no activeTurn object and marks the archive conflict", async () => {
    const h = harness(() => "host", () => [sessionEntry("running-archived", { status: "running", activeTurn: false, archivedAt: "2026-10-24T08:00:30.000Z" })], {
      "sessions.lifecycle-review": async () => null,
    });
    h.draw(); await settle(); const projection = h.draw().projections.get("running-archived")!;
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.native).toEqual({ status: "running", archiveMembership: "archived" });
    expect(projection.conflicts.map((entry) => entry.kind)).toContain("archived-with-active-execution");
  });

  it("retains an unarchived running snapshot with no activeTurn object", async () => {
    const h = harness(() => "host", () => [sessionEntry("running-live", { status: "running", activeTurn: false })], {
      "sessions.lifecycle-review": async () => null,
    });
    h.draw(); await settle(); const projection = h.draw().projections.get("running-live")!;
    expect(projection.retirement.disposition).toBe("retain");
    expect(projection.native).toEqual({ status: "running", archiveMembership: "unarchived" });
    expect(projection.conflicts).toEqual([]);
  });

  it("keeps exact review and agent coverage when a cached review cannot be refreshed", async () => {
    const cached = reviewId("cached"), unrelated = reviewId("unrelated");
    let phase = 0, failCached = false;
    const h = harness(() => "host", () => [sessionEntry(A), sessionEntry(B), sessionEntry(C), sessionEntry(D)], {
      "sessions.lifecycle-review": async (input) => {
        if (!input.reviewId) return phase === 0
          ? review("00000000-0000-4000-8000-0000000000aa", [], [{ reviewId: cached, agentId: A }, { reviewId: unrelated, agentId: C }])
          : review("00000000-0000-4000-8000-0000000000aa", [], [{ reviewId: cached, agentId: B }, { reviewId: unrelated, agentId: C }]);
        if (input.reviewId === cached) {
          if (failCached) throw new Error("cached review read failed");
          return review(cached, [row(A), row(B, "pending")]);
        }
        return review(unrelated, [row(C)]);
      },
    });
    h.draw(); await settle(); h.draw();
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(B)?.intent.state).toBe("none");
    expect(h.draw().projections.get(C)?.intent.state).toBe("unsettled");
    phase = 1; failCached = true; h.draw().reload(); await settle(); h.draw();
    expect(h.draw().receipts.state).toBe("partial");
    expect(h.draw().projections.get(A)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(B)?.intent.state).toBe("unknown");
    expect(h.draw().projections.get(C)?.intent.state).toBe("unsettled");
    expect(h.draw().projections.get(D)?.intent.state).toBe("none");
    failCached = false; h.draw().reload(); await settle(); h.draw();
    expect(h.draw().receipts).toMatchObject({ state: "partial", complete: false });
    expect(h.draw().projections.get(B)?.intent.state).toBe("unknown");
    expect(h.draw().projections.get(C)?.intent.state).toBe("unsettled");
  });

  it("reports partial coverage when a referenced review has no matching row", async () => {
    const R = reviewId("no-row");
    const h = harness(() => "host", () => [sessionEntry(A)], {
      "sessions.lifecycle-review": async (input) => input.reviewId
        ? review(R, [])
        : review("00000000-0000-4000-8000-0000000000aa", [], [{ reviewId: R, agentId: A }]),
    });
    h.draw(); await settle(); const value = h.draw();
    expect(value.projections.get(A)?.intent.state).toBe("unknown");
    expect(value.receipts).toMatchObject({ state: "partial", complete: false });
  });
});
