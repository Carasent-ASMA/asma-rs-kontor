import { describe, expect, it, vi } from "vitest";
import type * as Feedback from "../client/archive-feedback";
import type { useSessionLifecycle as Hook } from "../client/use-session-lifecycle";
import * as lifecycle from "../shared/session-lifecycle";
import type { LifecycleReview, LifecycleRow, UnresolvedRef } from "../shared/session-lifecycle";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";

const settle = async () => { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); };
function deferred<T>() { let resolve!: (value: T) => void, reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
const row = (id: string, patch: Partial<LifecycleRow> = {}) => ({ id, title: `Fixture ${id}`, eligible: true, outcome: "pending", message: null, reason: "Fixture reason.", ...patch }) as LifecycleRow;
const sa: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000a1", agentId: "fixture-sa" };
const other: UnresolvedRef = { reviewId: "00000000-0000-4000-8000-0000000000b2", agentId: "fixture-other" };
const latest = (unresolved?: UnresolvedRef[]): LifecycleReview => ({ id: "00000000-0000-4000-8000-0000000000c3", createdAt: "2026-10-22", action: "resume", scope: "filtered",
  target: lifecycle.DEFAULT_RECOVERY_TARGET, rows: [], limitation: lifecycle.TAKEOVER_LIMIT, ...(unresolved ? { unresolved } : {}) });

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
  const handlers: Record<string, (input: Record<string, unknown>) => Promise<unknown>> = {
    "sessions.lifecycle-review": async () => (reads.shift() ?? (async () => latest([sa])))(),
    "sessions.lifecycle-preview": async (input) => ({ id: "00000000-0000-4000-8000-0000000000d4", action: input.action, scope: "filtered", rows: (input.agentIds as string[]).map((id) => row(id)) }),
    "sessions.lifecycle-execute-item": async (input) => row(input.agentId as string, { outcome: "archived", runtime: "closed" }),
    "sessions.lifecycle-reconcile-item": async (input) => row(input.agentId as string, { outcome: "archived", runtime: "closed" }),
  };
  const rpc = Object.fromEntries(Object.keys(handlers).map((name) => [name, record(name)]));
  return { handlers, rpc, calls, count: (name: string) => calls.filter(([called]) => called === name).length };
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
      [row("x", { outcome: "uncertain", message: "Archive not confirmed: the session still reports activity." }), "error", "Archive not confirmed: the session still reports activity."],
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
    expect(draw().recovery).toMatchObject({ unresolved: [sa], error: null, line: "1 archive not confirmed" });
    await draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]);
    draw(); await settle(); draw(); await settle();
    expect(draw().recovery.unresolved).toEqual([sa]);
    draw = mount(fake.rpc, () => "host", changed);
    draw(); await settle();
    expect(draw().recovery.unresolved).toEqual([sa]);
    expect(fake.count("sessions.lifecycle-review")).toBeGreaterThanOrEqual(3);
  });
  it("keeps known refs beside a rejected lookup, and treats an absent field as unknown", async () => {
    const overflow = "More than 100 archives are not confirmed (101 found). The list cannot be shown exactly; no fence was changed.";
    const fake = host([async () => latest([sa]), async () => { throw new Error(overflow); }, async () => latest()]);
    const draw = mount(fake.rpc, () => "host", vi.fn());
    draw(); await settle();
    void draw().recovery.reload(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [sa], error: overflow, line: `1 archive not confirmed. ${overflow}` });
    void draw().recovery.reload(); await settle();
    expect(draw().recovery).toMatchObject({ unresolved: [sa], line: "1 archive not confirmed. The host did not report its current list." });
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
  it("AUD P2-03 an old host-A Check result after A→B→A changes nothing", async () => {
    const fake = host([async () => latest([sa]), async () => latest([other]), async () => latest([sa])]), changed = vi.fn(), pending: Array<() => void> = [];
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => new Promise((resolve) => pending.push(() => resolve(row(input.agentId as string, { outcome: "archived", runtime: "closed" }))));
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, changed);
    draw(); await settle();
    const stale = draw().recovery.check(); await settle();
    current = "host-b"; draw(); await settle();
    current = "host-a"; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [sa] });
    pending.shift()!(); await stale; draw(); await settle();
    expect(changed).not.toHaveBeenCalled();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [sa], error: null });
    expect(fake.count("sessions.lifecycle-review")).toBe(3);
    const fresh = draw().recovery.check(); await settle();
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
  it("QA P2-02 a Check pending on host A neither keeps host B checking nor lands on B", async () => {
    const fake = host([async () => latest([sa]), async () => latest([other])]), changed = vi.fn(), pending: Array<() => void> = [];
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => new Promise((resolve) => pending.push(() => resolve(row(input.agentId as string, { outcome: "archived", runtime: "closed" }))));
    let current = "host-a";
    const draw = mount(fake.rpc, () => current, changed);
    draw(); await settle();
    const a = draw().recovery.check(); await settle();
    expect(draw().recovery.checking).toBe(true);
    current = "host-b"; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [other] });
    pending.shift()!(); await a; draw(); await settle();
    expect(draw().recovery).toMatchObject({ checking: false, unresolved: [other], error: null });
    expect(changed).not.toHaveBeenCalled(); expect(fake.count("sessions.lifecycle-review")).toBe(2);
    const b = draw().recovery.check(); await settle();
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
    const checking = draw().recovery.check(); await settle();
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
  it("QA1-ARCHIVE-B-P2-03 / AUD P2-B01-R5 an unmount drops every late result: owner read, X, batch, row Check and recovery Check", async () => {
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
      await draw().lifecycle.prepare("resume", "filtered", ["fixture-other"]); await settle(); changed.mockReset();
      const pending = step === "run" ? draw().lifecycle.run() : draw().lifecycle.reconcile("fixture-other"); await settle();
      draw.unmount(); held.resolve(undefined); await pending; await settle();
      expect(changed).not.toHaveBeenCalled(); expect(fake.count(name)).toBe(1);
    }
    const fake = host(), changed = vi.fn(), held = deferred<unknown>(), real = fake.handlers["sessions.lifecycle-reconcile-item"]!;
    fake.handlers["sessions.lifecycle-review"] = async () => latest([sa, other]);
    fake.handlers["sessions.lifecycle-reconcile-item"] = (input) => held.promise.then(() => real(input));
    const draw = mount(fake.rpc, () => "host-a", changed); draw(); await settle();
    const checking = draw().recovery.check(); await settle();
    draw.unmount(); held.resolve(undefined); await checking; await settle();
    expect(changed).not.toHaveBeenCalled(); expect(fake.count("sessions.lifecycle-reconcile-item")).toBe(1);
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
        if (step === "restore" && !input.reviewId) return real[name]!(input);
        sent++; return a.promise.then(() => fails ? Promise.reject(new Error("Fixture host A failure.")) : step === "restore" ? owner : real[name]!(input));
      };
      const execute = fake.handlers["sessions.lifecycle-execute-item"]!;
      fake.handlers["sessions.lifecycle-execute-item"] = (input) => input.agentId === "fixture-b" ? b.promise : execute(input);
      let current = "host-a";
      const draw = mount(fake.rpc, () => current, changed); draw(); await settle();
      if (step === "run" || step === "reconcile") { await draw().lifecycle.prepare("resume", "filtered", ["fixture-other"]); await settle(); }
      changed.mockReset();
      const late: Promise<unknown> = step === "run" ? draw().lifecycle.run() : step === "reconcile" ? draw().lifecycle.reconcile("fixture-other")
        : step === "x" ? draw().lifecycle.prepare("archive", "filtered", ["fixture-swe"]) : draw().lifecycle.restore(sa);
      await settle();
      for (const next of path) { current = next; draw(); await settle(); }
      expect(draw().lifecycle).toMatchObject({ busy: false, running: false, error: null, review: null });
      // B's own Resume batch holds B's lock while A's request settles.
      await draw().lifecycle.prepare("resume", "filtered", ["fixture-b"]); await settle();
      void draw().lifecycle.run(); await settle();
      a.resolve(undefined); expect(await late ?? null).toBeNull(); await settle();
      expect(draw().lifecycle).toMatchObject({ running: true, error: null, review: { rows: [{ id: "fixture-b" }] } });
      expect(await draw().lifecycle.prepare("archive", "filtered", ["fixture-probe"])).toBeNull();
      expect(sent).toBe(1); expect(fake.count("sessions.lifecycle-preview")).toBe(step === "x" ? 2 : step === "restore" ? 1 : 2);
      if (fails) b.reject(new Error("Fixture host B failure.")); else b.resolve(row("fixture-b", { outcome: "submitted", message: "Fixture." }));
      await settle();
      expect(draw().lifecycle).toMatchObject({ busy: false, running: false, error: fails ? "Fixture host B failure." : null });
      expect(changed.mock.calls.map(([rows]) => rows.map((entry: LifecycleRow) => `${entry.id}:${entry.outcome}`))).toEqual([["fixture-b:pending"], ...(fails ? [] : [["fixture-b:submitted"]])]);
      draw.unmount();
    }
  });
});
