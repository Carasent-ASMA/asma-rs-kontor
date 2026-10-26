import { describe, expect, it, vi } from "vitest";
import type * as Feedback from "../client/archive-feedback";
import type { useSessionLifecycle as Hook } from "../client/use-session-lifecycle";
import * as lifecycle from "../shared/session-lifecycle";
import type { LifecycleReview, LifecycleRow, UnresolvedRef } from "../shared/session-lifecycle";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";
import { configureArchiveNoticeStorage, hiddenArchivesKey, readHiddenArchives, recoveryKey, recoveryLine, recoveryState, refreshNote, writeHiddenArchives, type RecoveryItem } from "../client/archive-feedback";

const settle = async () => { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); };
function deferred<T>() { let resolve!: (value: T) => void, reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const row = (id: string, patch: Partial<LifecycleRow> = {}) => ({ id, title: `Fixture ${id}`, eligible: true, outcome: "pending", message: null, reason: "Fixture reason.", ...patch }) as LifecycleRow;
const sa: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000a1", agentId: "fixture-sa" };
const other: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000b2", agentId: "fixture-other" };
const latest = (unresolved?: UnresolvedRef[]): LifecycleReview => ({ id: "00000000-0000-4000-8000-0000000000c3", createdAt: "2026-10-22", action: "resume", scope: "filtered",
  target: lifecycle.DEFAULT_RECOVERY_TARGET, rows: [], limitation: lifecycle.TAKEOVER_LIMIT, ...(unresolved ? { unresolved } : {}) });
/** A listed ref's own archive review, as its exact-ID read returns it. */
const ownerReview = (ref: UnresolvedRef, ...rows: LifecycleRow[]): LifecycleReview => ({ ...latest(), id: ref.reviewId, action: "archive",
  rows: rows.length ? rows : [row(ref.agentId, { outcome: "archived", runtime: "unconfirmed" })] });
const seen = (id: string, patch: Partial<NonNullable<LifecycleRow["availability"]>> = {}) => ({ agentId: id, state: "readable", archivedAt: null, reason: "Fixture read.",
  checkedAt: "2026-10-24T08:00:00.000Z", revision: "r1", ...patch }) as NonNullable<LifecycleRow["availability"]>;
const ONE = "Fixture fixture-sa: archived, runtime stop not confirmed";

/** Both hooks under Hermes scoping, as the iOS app runs them; each call is one fresh mount, and `unmount` runs its effect cleanups. */
function mount(rpc: Record<string, (input: Record<string, unknown>) => Promise<unknown>>, host: () => string, changed: (rows: LifecycleRow[]) => void) {
  const { react: base, render } = deferredReact(), cleanups = new Set<() => void>();
  const react = { ...base, useEffect: (fn: () => (() => void) | void, deps: unknown[]) => base.useEffect(() => {
    const cleanup = fn(); if (!cleanup) return; cleanups.add(cleanup); return () => { cleanups.delete(cleanup); cleanup(); };
  }, deps) };
  const modules = { react, "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => rpc[contract.name] }, "../shared/session-lifecycle": lifecycle };
  const { useSessionLifecycle } = loadHermesScoped<{ useSessionLifecycle: typeof Hook }>("../client/use-session-lifecycle.ts", modules);
  const { useArchiveRecovery } = loadHermesScoped<typeof Feedback>("../client/archive-feedback.ts", modules);
  return Object.assign(() => render(() => { const state = useSessionLifecycle(host(), changed); return { lifecycle: state, recovery: useArchiveRecovery(host(), state, changed) }; }),
    { unmount: () => { for (const cleanup of [...cleanups]) cleanup(); cleanups.clear(); } });
}
function host(reads: Array<() => Promise<unknown>> = []) {
  const calls: Array<[string, Record<string, unknown>]> = [], record = (name: string) => (input: Record<string, unknown>) => { calls.push([name, input]); return handlers[name]!(input); };
  // The list read pops `reads`; each ref's exact owner-review read answers from `owners`.
  const owners = new Map<string, () => Promise<unknown>>([[sa.reviewId, async () => ownerReview(sa)], [other.reviewId, async () => ownerReview(other)]]);
  const handlers: Record<string, (input: Record<string, unknown>) => Promise<unknown>> = {
    "sessions.lifecycle-review": async (input) => input.reviewId ? (owners.get(input.reviewId as string) ?? (async () => null))() : (reads.shift() ?? (async () => latest([sa])))(),
    "sessions.lifecycle-preview": async (input) => ({ id: "00000000-0000-4000-8000-0000000000d4", action: input.action, scope: "filtered", rows: (input.agentIds as string[]).map((id) => row(id)) }),
    "sessions.lifecycle-execute-item": async (input) => row(input.agentId as string, { outcome: "archived", runtime: "closed" }),
    "sessions.lifecycle-reconcile-item": async (input) => row(input.agentId as string, { outcome: "archived", runtime: "closed" }),
  };
  const rpc = Object.fromEntries(Object.keys(handlers).map((name) => [name, record(name)]));
  return { handlers, rpc, calls, owners, count: (name: string) => calls.filter(([called]) => called === name).length,
    lists: () => calls.filter(([called, input]) => called === "sessions.lifecycle-review" && !input.reviewId).length };
}

describe("one Archive X", () => {
  it("changes the directory once, after execute, and ignores a second press while pending", async () => {
    const fake = host(), changed = vi.fn(), executing = deferred<LifecycleRow>();
    fake.handlers["sessions.lifecycle-execute-item"] = () => executing.promise;
    const draw = mount(fake.rpc, () => "host", changed);
    const first = draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]);
    await settle(); draw();
    expect(changed).not.toHaveBeenCalled();
    expect(await draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"])).toBeNull();
    const result = row("fixture-swe", { outcome: "archived", runtime: "closed" });
    executing.resolve(result);
    expect(await first).toBe(result);
    expect(changed).toHaveBeenCalledExactlyOnceWith([result]);
    expect(fake.count("sessions.lifecycle-preview")).toBe(1); expect(fake.count("sessions.lifecycle-execute-item")).toBe(1);
  });
  it("maps each result to one toast; a known close error and a lost acknowledgement read alike", () => {
    const { notifyArchive } = loadHermesScoped<typeof Feedback>("../client/archive-feedback.ts", { react: {}, "@getpaseo/plugin/client": {}, "../shared/session-lifecycle": lifecycle });
    const toast = () => ({ show: vi.fn(), error: vi.fn() });
    const unconfirmed = (reason: string) => row("x", { outcome: "archived", runtime: "unconfirmed", message: `Archived. The archive reply did not confirm completion (${reason}), so stopping its runtime is not confirmed.` });
    const cases: Array<[LifecycleRow | string | null, "show" | "error" | null, string?]> = [
      [row("x", { outcome: "archived", runtime: "closed" }), "show", "Archived Fixture x."],
      [row("x", { outcome: "archived", runtime: "not-observed" }), "show", "Archived Fixture x."],
      [row("x", { outcome: "already_archived", message: "Already archived on this host. No archive request was sent." }), "show", "Already archived on this host. No archive request was sent."],
      [unconfirmed("Runtime close failed"), "error"], [unconfirmed("Timeout"), "error"],
      [row("x", { outcome: "uncertain", message: "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request." }), "error", "Fixture x: archive result unknown."],
      [row("x", { outcome: "uncertain", availability: seen("x", { archivedAt: "2026-10-24T07:59:00.000Z" }) }), "error", "Fixture x: archived, runtime stop not confirmed."],
      [row("x", { outcome: "blocked", message: "An earlier archive owns this session." }), "error", "An earlier archive owns this session."],
      [row("x", { eligible: false }), "error", "Fixture reason."],
      ["Native inventory exceeded the bounded page walk.", "error", "Archive did not report a result: Native inventory exceeded the bounded page walk."],
      [null, null],
    ];
    const lines: string[] = [];
    for (const [result, kind, message] of cases) {
      const t = toast(); notifyArchive(t, result);
      expect(t.show.mock.calls.length + t.error.mock.calls.length).toBe(kind ? 1 : 0);
      if (kind) { const call = t[kind].mock.calls[0]![0] as string; if (message) expect(call).toBe(message); else lines.push(call.replace(/\(.*\)/, "(…)")); }
    }
    expect(lines).toHaveLength(2); expect(lines[0]).toBe(lines[1]);
  });
});

describe("persistent archive recovery", () => {
  it("keeps SA through another ID's archive and a remount", async () => {
    const fake = host(), changed = vi.fn();
    let draw = mount(fake.rpc, () => "host", changed);
    draw(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [sa], error: null, line: ONE });
    await draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]);
    draw(); await settle(); draw(); await settle();
    expect(draw().recovery.unresolved).toEqual([sa]);
    draw = mount(fake.rpc, () => "host", changed);
    draw(); await settle();
    expect(draw().recovery.unresolved).toEqual([sa]);
    expect(fake.lists()).toBeGreaterThanOrEqual(3);
  });
  it("keeps known refs beside a rejected lookup, and treats an absent field as unknown", async () => {
    const overflow = "More than 100 archives are not confirmed (101 found). The list cannot be shown exactly; no fence was changed.";
    const fake = host([async () => latest([sa]), async () => { throw new Error(overflow); }, async () => latest()]);
    const draw = mount(fake.rpc, () => "host", vi.fn());
    draw(); await settle();
    void draw().recovery.reload(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [sa], error: overflow, line: `${ONE}. ${overflow}` });
    void draw().recovery.reload(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [sa], line: `${ONE}. The host did not report its current list.` });
    const empty = mount(host([async () => { throw new Error(overflow); }]).rpc, () => "host", vi.fn());
    empty(); await settle();
    expect(empty().recovery.line).toBe(`Archive checks could not be read: ${overflow}`);
  });
  it("host B never shows host A's refs while its own read is pending", async () => {
    const slow = deferred<unknown>(), fake = host([async () => latest([sa]), () => slow.promise]);
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, vi.fn());
    draw(); await settle(); expect(draw().recovery.unresolved).toEqual([sa]);
    current = "host-b"; draw(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [], line: null });
    slow.resolve(latest([other])); await settle();
    expect(draw().recovery.unresolved).toEqual([other]);
  });
  it("AUD P2-03 an old host-A Refresh status result after A→B→A changes nothing", async () => {
    const fake = host([async () => latest([sa]), async () => latest([other]), async () => latest([sa])]), changed = vi.fn(), pending: Array<() => void> = [];
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => new Promise((resolve) => pending.push(() => resolve(row(input.agentId as string, { outcome: "archived", runtime: "closed" }))));
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, changed);
    draw(); await settle();
    const stale = draw().recovery.refresh(); await settle();
    current = "host-b"; draw(); await settle();
    current = "host-a"; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [sa] });
    pending.shift()!(); await stale; draw(); await settle();
    expect(changed).not.toHaveBeenCalled();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [sa], error: null });
    expect(fake.lists()).toBe(3);
    const fresh = draw().recovery.refresh(); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item")).toHaveLength(2);
    pending.shift()!(); await fresh;
    expect(changed).toHaveBeenCalledTimes(1);
  });
  it("QA P2-01 / AUD P2-02 an absent list on a fresh mount reads as unknown, never as none; null is none", async () => {
    const older = mount(host([async () => latest()]).rpc, () => "host", vi.fn());
    older(); await settle();
    expect(older().recovery).toMatchObject({ unresolved: [], line: "Archive checks could not be read: The host did not report its current list." });
    const fresh = mount(host([async () => latest([sa]), async () => null]).rpc, () => "host", vi.fn());
    fresh(); await settle(); expect(fresh().recovery.unresolved).toEqual([sa]);
    void fresh().recovery.reload(); await settle();
    expect(fresh().recovery).toMatchObject({ unresolved: [], error: null, line: null });
  });
  it("QA P2-02 a Refresh status pending on host A neither keeps host B checking nor lands on B", async () => {
    const fake = host([async () => latest([sa]), async () => latest([other])]), changed = vi.fn(), pending: Array<() => void> = [];
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => new Promise((resolve) => pending.push(() => resolve(row(input.agentId as string, { outcome: "archived", runtime: "closed" }))));
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, changed);
    draw(); await settle();
    const a = draw().recovery.refresh(); await settle();
    expect(draw().recovery.checking).toBe(true);
    current = "host-b"; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [other] });
    pending.shift()!(); await a; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [other], error: null });
    expect(changed).not.toHaveBeenCalled(); expect(fake.lists()).toBe(2);
    const b = draw().recovery.refresh(); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([sa, other]);
    pending.shift()!(); await b;
    expect(changed.mock.calls.map(([rows]) => rows[0].id)).toEqual([other.agentId]);
  });
  it("discards a late read from the previous host", async () => {
    const slow = deferred<unknown>(), fake = host([() => slow.promise, async () => latest([])]);
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, vi.fn());
    draw(); current = "host-b"; draw(); await settle();
    slow.resolve(latest([sa])); await settle();
    expect(draw().recovery.unresolved).toEqual([]);
  });
  it("checks each direct owner ref in order, one at a time, without an archive request", async () => {
    const fake = host([async () => latest([sa, other]), async () => latest([])]), changed = vi.fn(), pending: Array<() => void> = [];
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => new Promise((resolve) => pending.push(() => resolve(row(input.agentId as string, { outcome: "archived", runtime: "closed" }))));
    const draw = mount(fake.rpc, () => "host", changed);
    draw(); await settle();
    const checking = draw().recovery.refresh(); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([sa]);
    pending.shift()!(); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([sa, other]);
    pending.shift()!(); await checking; draw(); await settle();
    expect(changed.mock.calls.map(([rows]) => rows[0].id)).toEqual([sa.agentId, other.agentId]);
    expect(draw().recovery).toMatchObject({ unresolved: [], line: null });
    expect(fake.count("sessions.lifecycle-execute-item")).toBe(0); expect(fake.count("sessions.lifecycle-preview")).toBe(0);
  });
});

describe("Archive details", () => {
  it("QA1-ARCHIVE-B-P2-02 / AUD r4 an owner restore that fails or returns anything but SA's own review reports it and keeps no earlier review", async () => {
    const fake = host(), changed = vi.fn();
    let exact: () => Promise<unknown> = async () => { throw new Error("Fixture read failure."); };
    fake.handlers["sessions.lifecycle-review"] = async (input) => input.reviewId ? exact() : latest([sa]);
    const draw = mount(fake.rpc, () => "host", changed);
    await draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]); await settle();
    expect(draw().lifecycle.review?.rows.map((entry) => entry.id)).toEqual(["fixture-swe"]);
    expect(await draw().lifecycle.restore(sa)).toBe("Fixture read failure."); await settle();
    expect(draw().lifecycle).toMatchObject({ review: null, error: "Fixture read failure." });
    // None, another review, another review that lists SA, and the owner's review ID without SA's own row.
    for (const answer of [null, latest(), { ...latest(), rows: [row(sa.agentId)] }, { ...latest(), id: sa.reviewId, action: "archive", rows: [row("fixture-swe")] }]) {
      await draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]); await settle();
      exact = async () => answer;
      expect(await draw().lifecycle.restore(sa)).toBe("The host did not return this archive's own review."); await settle();
      expect(draw().lifecycle.review).toBeNull();
    }
    const owner = { ...latest(), id: sa.reviewId, action: "archive" as const, rows: [row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" })] };
    exact = async () => owner;
    expect(await draw().lifecycle.restore(sa)).toEqual(owner); await settle();
    expect(draw().lifecycle).toMatchObject({ review: { id: sa.reviewId }, error: null });
  });
  it("AUD r4 drops an owner read the earlier host view started, after A→B and after A→B→A", async () => {
    const fake = host(), changed = vi.fn();
    let current = "host-a", held = deferred<unknown>();
    fake.handlers["sessions.lifecycle-review"] = async (input) => input.reviewId ? held.promise : latest([sa]);
    const draw = mount(fake.rpc, () => current, changed);
    const owner = { ...latest(), id: sa.reviewId, action: "archive" as const, rows: [row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" })] };
    for (const [path, fails] of [[["host-b"], false], [["host-b", "host-a"], false], [["host-b"], true], [["host-b", "host-a"], true]] as const) {
      current = "host-a"; draw(); await settle();
      held = deferred(); const pending = draw().lifecycle.restore(sa);
      for (const next of path) { current = next; draw(); await settle(); }
      if (fails) held.reject(new Error("Fixture late failure.")); else held.resolve(owner);
      expect(await pending).toBeNull(); await settle();
      expect(draw().lifecycle).toMatchObject({ review: null, busy: false, error: null });
    }
    expect(changed).not.toHaveBeenCalled();
  });
  it("QA1-ARCHIVE-B-P2-03 / AUD P2-B01-R5 an unmount drops every late result: owner read, X, batch, row and recovery Refresh status", async () => {
    const owner = { ...latest(), id: sa.reviewId, action: "archive" as const, rows: [row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" })] };
    for (const fails of [false, true]) {
      const fake = host(), changed = vi.fn(), held = deferred<unknown>();
      fake.handlers["sessions.lifecycle-review"] = async (input) => input.reviewId ? held.promise : latest([sa]);
      const draw = mount(fake.rpc, () => "host-a", changed); draw(); await settle();
      const pending = draw().lifecycle.restore(sa);
      draw.unmount(); if (fails) held.reject(new Error("Fixture late failure.")); else held.resolve(owner);
      expect(await pending).toBeNull(); expect(changed).not.toHaveBeenCalled();
    }
    for (const step of ["preview", "execute", "execute-failure"] as const) {
      const fake = host(), changed = vi.fn(), held = deferred<unknown>(), name = step === "preview" ? "sessions.lifecycle-preview" : "sessions.lifecycle-execute-item";
      const real = fake.handlers[name]!; fake.handlers[name] = (input) => held.promise.then(() => step === "execute-failure" ? Promise.reject(new Error("Fixture late failure.")) : real(input));
      const draw = mount(fake.rpc, () => "host-a", changed); draw(); await settle();
      const pending = draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]); await settle();
      draw.unmount(); held.resolve(undefined);
      expect(await pending).toBeNull(); await settle();
      expect(changed).not.toHaveBeenCalled(); expect(fake.count("sessions.lifecycle-execute-item")).toBe(step === "preview" ? 0 : 1);
    }
    for (const step of ["run", "reconcile"] as const) {
      const fake = host(), changed = vi.fn(), held = deferred<unknown>(), name = step === "run" ? "sessions.lifecycle-execute-item" : "sessions.lifecycle-reconcile-item";
      const real = fake.handlers[name]!; fake.handlers[name] = (input) => held.promise.then(() => real(input));
      const draw = mount(fake.rpc, () => "host-a", changed); draw(); await settle();
      await draw().lifecycle.prepare("archive", "filtered", ["fixture-other"], true); await settle(); changed.mockReset();
      const pending = step === "run" ? draw().lifecycle.run() : draw().lifecycle.reconcile("fixture-other"); await settle();
      draw.unmount(); held.resolve(undefined); await pending; await settle();
      expect(changed).not.toHaveBeenCalled(); expect(fake.count(name)).toBe(1);
    }
    const fake = host(), changed = vi.fn(), held = deferred<unknown>(), real = fake.handlers["sessions.lifecycle-reconcile-item"]!;
    fake.handlers["sessions.lifecycle-review"] = async () => latest([sa, other]);
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => held.promise.then(() => real(input));
    const draw = mount(fake.rpc, () => "host-a", changed); draw(); await settle();
    const checking = draw().recovery.refresh(); await settle();
    draw.unmount(); held.resolve(undefined); await checking; await settle();
    expect(changed).not.toHaveBeenCalled(); expect(fake.count("sessions.lifecycle-reconcile-item")).toBe(1);
  });
  it("QA1-PR476-P2-01 an unmount retires the recovery lookup: a pending list or owner read sends no further read and writes nothing", async () => {
    for (const held of ["list", "owner"] as const) for (const fails of [false, true]) {
      const slow = deferred<unknown>(), changed = vi.fn(), fake = host([held === "list" ? () => slow.promise : async () => latest([sa, other])]);
      if (held === "owner") fake.owners.set(sa.reviewId, () => slow.promise);
      const draw = mount(fake.rpc, () => "host-a", changed), view = draw(); await settle();
      const before = fake.calls.map(([name, input]) => [name, { ...input }]);
      expect(before.at(-1)).toEqual(["sessions.lifecycle-review", held === "list" ? {} : { reviewId: sa.reviewId }]);
      draw.unmount(); if (fails) slow.reject(new Error("Fixture late failure.")); else slow.resolve(held === "list" ? latest([sa, other]) : ownerReview(sa)); await settle();
      // Retired controls stay inert too.
      await view.recovery.reload(); await view.recovery.refresh(); await view.recovery.refresh(sa); await settle();
      expect(fake.calls).toEqual(before); expect(changed).not.toHaveBeenCalled();
      expect(draw().recovery).toMatchObject({ items: [], error: null, listed: null, complete: false, refreshing: null });
    }
  });
});

describe("AUD P2-B01-R6 a request the earlier host view of a mounted hook started", () => {
  it("settles without touching host B: no failure, state, onChanged or lock; B starts idle and keeps its own result and failure", async () => {
    const owner = { ...latest(), id: sa.reviewId, action: "archive" as const, rows: [row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" })] };
    for (const step of ["run", "x", "restore", "reconcile"] as const) for (const path of [["host-b"], ["host-b", "host-a"]]) for (const fails of [true, false]) {
      const fake = host(), changed = vi.fn(), a = deferred<unknown>(), b = deferred<LifecycleRow>(), real = { ...fake.handlers };
      const name = step === "restore" ? "sessions.lifecycle-review" : step === "reconcile" ? "sessions.lifecycle-reconcile-item" : "sessions.lifecycle-execute-item";
      let sent = 0;
      fake.handlers[name] = (input) => {
        // Recovery reads no owner review here: only the restore under test reads one.
        if (step === "restore" && !input.reviewId) return Promise.resolve(latest([]));
        sent++; return a.promise.then(() => fails ? Promise.reject(new Error("Fixture host A failure.")) : step === "restore" ? owner : real[name]!(input));
      };
      const execute = fake.handlers["sessions.lifecycle-execute-item"]!;
      fake.handlers["sessions.lifecycle-execute-item"] = (input) => input.agentId === "fixture-b" ? b.promise : execute(input);
      let current = "host-a";
      const draw = mount(fake.rpc, () => current, changed); draw(); await settle();
      if (step === "run" || step === "reconcile") { await draw().lifecycle.prepare("archive", "filtered", ["fixture-other"], true); await settle(); }
      changed.mockReset();
      const late: Promise<unknown> = step === "run" ? draw().lifecycle.run() : step === "reconcile" ? draw().lifecycle.reconcile("fixture-other")
        : step === "x" ? draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]) : draw().lifecycle.restore(sa);
      await settle();
      for (const next of path) { current = next; draw(); await settle(); }
      expect(draw().lifecycle).toMatchObject({ busy: false, running: false, error: null, review: null });
      // B's own Archive batch holds B's lock while A's request settles.
      await draw().lifecycle.prepare("archive", "filtered", ["fixture-b"], true); await settle();
      void draw().lifecycle.run(); await settle();
      a.resolve(undefined); expect(await late ?? null).toBeNull(); await settle();
      expect(draw().lifecycle).toMatchObject({ running: true, error: null, review: { rows: [{ id: "fixture-b" }] } });
      expect(await draw().lifecycle.prepare("archive", "filtered", ["fixture-probe"])).toBeNull();
      expect(sent).toBe(1); expect(fake.count("sessions.lifecycle-preview")).toBe(step === "x" ? 2 : step === "restore" ? 1 : 2);
      if (fails) b.reject(new Error("Fixture host B failure.")); else b.resolve(row("fixture-b", { outcome: "archived", message: "Fixture." }));
      await settle();
      expect(draw().lifecycle).toMatchObject({ busy: false, running: false, error: fails ? "Fixture host B failure." : null });
      expect(changed.mock.calls.map(([rows]) => rows.map((entry: LifecycleRow) => `${entry.id}:${entry.outcome}`))).toEqual([["fixture-b:pending"], [`fixture-b:${fails ? "uncertain" : "archived"}`]]);
      draw.unmount();
    }
  });
});

describe("U1 precise archive states", () => {
  it("names each state from structured fields only; message wording never decides", () => {
    const legacy = "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.";
    expect(recoveryState(row("x", { outcome: "uncertain", message: legacy }))).toBe("unknown");
    expect(recoveryState(row("x", { outcome: "pending" }))).toBe("unknown");
    expect(recoveryState(row("x", { outcome: "uncertain", message: "Archived and stopped.", availability: seen("x") }))).toBe("unknown");
    expect(recoveryState(row("x", { outcome: "archived", runtime: "unconfirmed" }))).toBe("unconfirmed");
    expect(recoveryState(row("x", { outcome: "uncertain", message: legacy, availability: seen("x", { archivedAt: "2026-10-24T07:59:00.000Z" }) }))).toBe("unconfirmed");
    expect(recoveryState(row("x", { outcome: "uncertain", availability: seen("x", { state: "unknown", archivedAt: "2026-10-24T07:59:00.000Z" }) }))).toBe("unavailable");
    expect(recoveryState(null)).toBe("unavailable");
  });
  it("one item names its session and state, several are counted, and a failed list read stays beside them", () => {
    const item = (id: string, state: RecoveryItem["state"], title: string | null = `Fixture ${id}`): RecoveryItem =>
      ({ ref: { reviewId: sa.reviewId, agentId: id }, state, row: title === null ? null : row(id, { title }), error: null });
    expect(recoveryLine([item("a", "unknown")], null)).toBe("Fixture a: archive result unknown");
    expect(recoveryLine([item("a", "unavailable", null)], null)).toBe("An archived session: couldn't check archive");
    expect(recoveryLine([item("a", "unknown"), item("b", "unconfirmed")], null)).toBe("2 archives need attention");
    expect(recoveryLine([item("a", "unknown"), item("b", "unconfirmed")], "Fixture failure.")).toBe("2 archives need attention. Fixture failure.");
    expect(recoveryLine([], "Fixture failure.")).toBe("Archive checks could not be read: Fixture failure.");
    expect(recoveryLine([], null)).toBeNull();
  });
  it("U5 a refresh result is confirmed, changed or unchanged against the item it started from", () => {
    const before: RecoveryItem = { ref: sa, state: "unknown", row: row(sa.agentId, { outcome: "uncertain" }), error: null };
    expect(refreshNote(before, row(sa.agentId, { outcome: "archived", runtime: "not-observed" }), 1)).toEqual({ at: 1, outcome: "confirmed", title: "Fixture fixture-sa" });
    expect(refreshNote(before, row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" }), 1)).toMatchObject({ outcome: "changed", state: "unconfirmed" });
    expect(refreshNote(before, row(sa.agentId, { outcome: "uncertain" }), 1)).toMatchObject({ outcome: "unchanged", state: "unknown" });
    expect(refreshNote(undefined, row(sa.agentId, { outcome: "uncertain" }), 1)).toMatchObject({ outcome: "unchanged" });
  });
  it("reads only the listed owner reviews, once per review, and keeps blocked and settled rows out", async () => {
    const shared: UnresolvedRef = { reviewId: other.reviewId, agentId: "fixture-shared" }, blocked: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000e5", agentId: "fixture-blocked" };
    const lost: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000f6", agentId: "fixture-lost" };
    const fake = host([async () => latest([sa, other, shared, blocked, lost])]);
    fake.owners.set(sa.reviewId, async () => ownerReview(sa, row(sa.agentId, { outcome: "uncertain" })));
    fake.owners.set(other.reviewId, async () => ownerReview(other, row(other.agentId, { outcome: "archived", runtime: "unconfirmed" }), row(shared.agentId, { outcome: "archived", runtime: "not-observed" })));
    fake.owners.set(blocked.reviewId, async () => ownerReview(blocked, row(blocked.agentId, { outcome: "blocked", message: "Has an active child." })));
    fake.owners.set(lost.reviewId, async () => { throw new Error("Fixture host read failed."); });
    const draw = mount(fake.rpc, () => "host", vi.fn());
    draw(); await settle();
    const recovery = draw().recovery;
    expect(recovery.items.map((item) => [item.ref.agentId, item.state, item.error])).toEqual([["fixture-sa", "unknown", null], ["fixture-other", "unconfirmed", null], ["fixture-lost", "unavailable", "Fixture host read failed."]]);
    expect(recovery).toMatchObject({ line: "3 archives need attention", error: null, listed: "host", complete: false });
    expect(fake.calls.filter(([, input]) => input.reviewId).map(([, input]) => input.reviewId)).toEqual([sa.reviewId, other.reviewId, blocked.reviewId, lost.reviewId]);
    expect(fake.count("sessions.lifecycle-reconcile-item") + fake.count("sessions.lifecycle-execute-item") + fake.count("sessions.lifecycle-preview")).toBe(0);
  });
  it("a failed owner read keeps the item's last known state and row", async () => {
    const fake = host([async () => latest([sa]), async () => latest([sa])]), draw = mount(fake.rpc, () => "host", vi.fn());
    draw(); await settle();
    expect(draw().recovery).toMatchObject({ complete: true, items: [{ state: "unconfirmed", error: null }] });
    fake.owners.set(sa.reviewId, async () => { throw new Error("Fixture host offline."); });
    void draw().recovery.reload(); await settle();
    expect(draw().recovery).toMatchObject({ complete: false, items: [{ state: "unconfirmed", row: { id: sa.agentId }, error: "Fixture host offline." }] });
  });
});

describe("U5 Refresh status feedback", () => {
  it("reports checking, then unchanged, changed, confirmed or couldn't check per item, and never sends archive", async () => {
    const third: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000f7", agentId: "fixture-third" };
    const fake = host([async () => latest([sa, other, third]), async () => latest([sa, third]), async () => latest([sa, third])]), changed = vi.fn();
    fake.owners.set(sa.reviewId, async () => ownerReview(sa, row(sa.agentId, { outcome: "uncertain" })));
    fake.owners.set(third.reviewId, async () => ownerReview(third, row(third.agentId, { outcome: "uncertain" })));
    const replies: Record<string, Array<() => Promise<LifecycleRow>>> = {
      [sa.agentId]: [async () => row(sa.agentId, { outcome: "archived", runtime: "unconfirmed" })],
      [other.agentId]: [async () => row(other.agentId, { outcome: "archived", runtime: "not-observed" })],
      [third.agentId]: [async () => { throw new Error("Fixture host read failed."); }, async () => row(third.agentId, { outcome: "uncertain" })],
    };
    const held = deferred<void>();
    fake.handlers["sessions.lifecycle-reconcile-item"] = async (input) => { await held.promise; return replies[input.agentId as string]!.shift()!(); };
    const draw = mount(fake.rpc, () => "host", changed);
    draw(); await settle();
    const all = draw().recovery.refresh(); await settle();
    expect(draw().recovery).toMatchObject({ checking: true, refreshing: "all" });
    held.resolve(); await all; draw(); await settle();
    const { notes, items, refreshing } = draw().recovery;
    expect(refreshing).toBeNull();
    expect(notes[recoveryKey(sa)]).toMatchObject({ outcome: "changed", state: "unconfirmed" });
    expect(notes[recoveryKey(other)]).toMatchObject({ outcome: "confirmed", title: "Fixture fixture-other" });
    expect(notes[recoveryKey(third)]).toMatchObject({ outcome: "failed", error: "Fixture host read failed." });
    expect(items.map((item) => item.ref.agentId)).toEqual([sa.agentId, third.agentId]);
    expect(changed.mock.calls.map(([rows]) => rows[0].id)).toEqual([sa.agentId, other.agentId]);
    const one = draw().recovery.refresh(third);
    expect(draw().recovery.refreshing).toBe(recoveryKey(third));
    await one; draw(); await settle();
    expect(draw().recovery.notes[recoveryKey(third)]).toMatchObject({ outcome: "unchanged", state: "unknown" });
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([sa, other, third, third]);
    draw().recovery.clearNotes(); expect(draw().recovery.notes).toEqual({});
    expect(fake.count("sessions.lifecycle-execute-item") + fake.count("sessions.lifecycle-preview")).toBe(0);
  });
});

describe("U3 hidden archive notices per host", () => {
  const store = (initial: Record<string, unknown> = {}) => {
    const values = new Map(Object.entries(initial)), writes: Array<[string, string | null]> = [];
    return { values, writes, fail: false, read(key: string) { return values.get(key) ?? null; },
      write(key: string, value: string | null) { if (this.fail) throw new Error("Fixture store full."); writes.push([key, value]); if (value === null) values.delete(key); else values.set(key, value); } };
  };
  const keys = ["archive:r1:a1:unconfirmed", "archive:r2:a2:unknown"];
  it("keeps hidden keys for this app session when the device has no store", () => {
    configureArchiveNoticeStorage(null);
    expect(readHiddenArchives("host-a")).toEqual({ keys: [], persistence: "session" });
    expect(writeHiddenArchives("host-a", keys)).toEqual({ keys, persistence: "session" });
    expect(readHiddenArchives("host-a").keys).toEqual(keys); expect(readHiddenArchives("host-b").keys).toEqual([]);
  });
  it("remembers them per host on the device, across a restart, and forgets with a removal", () => {
    const device = store();
    configureArchiveNoticeStorage(device);
    expect(writeHiddenArchives(" host-a ".trim(), keys)).toEqual({ keys, persistence: "device" });
    expect(device.values.get(hiddenArchivesKey("host-a"))).toBe(JSON.stringify(keys));
    expect(writeHiddenArchives("host-a", [...keys].reverse())).toMatchObject({ persistence: "device" }); expect(device.writes).toHaveLength(1);
    configureArchiveNoticeStorage(device);
    expect(readHiddenArchives("host-a")).toEqual({ keys, persistence: "device" });
    expect(readHiddenArchives("host-b")).toEqual({ keys: [], persistence: "device" });
    writeHiddenArchives("host-a", []);
    expect(device.writes.at(-1)).toEqual([hiddenArchivesKey("host-a"), null]); expect(device.values.size).toBe(0);
  });
  it("an unreadable value hides nothing, and a failed write says it lasts this app session, then retries", () => {
    for (const value of ["not json", JSON.stringify(["source:error"]), JSON.stringify({ keys })]) {
      configureArchiveNoticeStorage(store({ [hiddenArchivesKey("host-a")]: value }));
      expect(readHiddenArchives("host-a")).toEqual({ keys: [], persistence: "unreadable" });
    }
    const device = store(); configureArchiveNoticeStorage(device); device.fail = true;
    expect(writeHiddenArchives("host-a", keys)).toEqual({ keys, persistence: "failed" });
    device.fail = false;
    expect(writeHiddenArchives("host-a", keys)).toEqual({ keys, persistence: "device" });
    configureArchiveNoticeStorage(null);
  });
});
