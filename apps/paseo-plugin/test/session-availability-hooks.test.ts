import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const hooks = vi.hoisted(() => ({ slots: [] as Array<{ value?: unknown; deps?: unknown[]; cleanup?: () => void }>, cursor: 0, queue: [] as Array<() => void>, rpc: vi.fn(),
  slot() { return this.slots[this.cursor] ?? (this.slots[this.cursor] = {}); },
}));
vi.mock("react", () => ({
  useState: (initial: unknown) => { const s = hooks.slot(); hooks.cursor++; if (!("value" in s)) s.value = typeof initial === "function" ? initial() : initial; return [s.value, (next: unknown) => { s.value = typeof next === "function" ? next(s.value) : next; }]; },
  useRef: (initial: unknown) => { const s = hooks.slot(); hooks.cursor++; return s.value ?? (s.value = { current: initial }); },
  useMemo: (fn: () => unknown) => fn(), useCallback: (fn: unknown) => fn,
  useEffect: (fn: () => (() => void) | undefined, deps: unknown[]) => { const s = hooks.slot(); hooks.cursor++; if (s.deps && deps.every((d, i) => Object.is(d, s.deps![i]))) return; s.deps = deps; hooks.queue.push(() => { s.cleanup?.(); s.cleanup = fn(); }); },
}));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => hooks.rpc }));
import { useSessionAvailability } from "../client/use-session-availability";
import { useSessionLifecycle } from "../client/use-session-lifecycle";
import { sessionEntry } from "./session-fixtures";
import { AVAILABILITY_TTL_MS, type LifecycleRow, type SessionAvailability } from "../shared/session-lifecycle";
import { hasSessionAttention } from "../client/session-directory";
function render<T>(fn: () => T) { hooks.cursor = 0; const value = fn(); for (const run of hooks.queue.splice(0)) run(); return value; }
async function settle() { for (let n = 0; n < 10; n++) await Promise.resolve(); }
const entry = sessionEntry("same-id", { requiresAttention: true, attentionReason: "finished" });
const evidence = (state: SessionAvailability["state"]): SessionAvailability => ({ agentId: entry.id, revision: entry.updatedAt, checkedAt: new Date().toISOString(), state, reason: "fictional native read" });
beforeEach(() => { hooks.slots = []; hooks.cursor = 0; hooks.queue = []; hooks.rpc.mockReset(); vi.useFakeTimers(); });
afterEach(() => { hooks.slots.forEach(s => s.cleanup?.()); vi.useRealTimers(); });
describe("P1 host/cache/action hook races", () => {
  it("discards an in-flight missing reply after switching hosts with the same ID", async () => {
    let resolveOld!: (value: unknown) => void; let host = "host-a";
    hooks.rpc.mockImplementationOnce(() => new Promise(r => { resolveOld = r; })).mockResolvedValue({ [entry.id]: evidence("readable") });
    const draw = () => render(() => useSessionAvailability(host, [entry], true)); draw(); host = "host-b"; draw(); await settle();
    resolveOld({ [entry.id]: evidence("missing") }); await settle();
    expect(draw().entries[0]!.availability?.state).toBe("readable"); expect(hasSessionAttention(draw().entries[0]!)).toBe(true);
  });
  it("invalidates a cached missing reply on explicit refresh before the next read finishes", async () => {
    hooks.rpc.mockResolvedValueOnce({ [entry.id]: evidence("missing") }).mockImplementation(() => new Promise(() => {}));
    const draw = () => render(() => useSessionAvailability("host", [entry], true)); draw(); await settle();
    expect(hasSessionAttention(draw().entries[0]!)).toBe(false); draw().refresh();
    expect(hasSessionAttention(draw().entries[0]!)).toBe(true);
  });
  it("expires unchanged native snapshots once without a provider polling loop", async () => {
    hooks.rpc.mockResolvedValue({ [entry.id]: evidence("missing") });
    const draw = () => render(() => useSessionAvailability("host", [entry], true)); draw(); await settle(); draw();
    vi.advanceTimersByTime(AVAILABILITY_TTL_MS); expect(hasSessionAttention(draw().entries[0]!)).toBe(true); expect(hooks.rpc).toHaveBeenCalledTimes(1);
  });
  it("prevents a slow old availability read overwriting a newer archive observation", async () => {
    let resolveOld!: (value: unknown) => void;
    hooks.rpc.mockImplementationOnce(() => new Promise(r => { resolveOld = r; })).mockImplementation(() => new Promise(() => {}));
    const draw = () => render(() => useSessionAvailability("host", [entry], true)); const initial = draw();
    initial.reconcile([{ id: entry.id, availability: { ...evidence("readable"), archivedAt: "2026-10-21" } } as LifecycleRow]); draw();
    resolveOld({ [entry.id]: evidence("readable") }); await settle();
    expect(draw().entries[0]!.archivedAt).toBe("2026-10-21"); expect(hasSessionAttention(draw().entries[0]!)).toBe(false);
  });
  it("propagates fresh review observations before any execute action", async () => {
    const rows = [{ id: entry.id, eligible: false, availability: evidence("missing") }];
    hooks.rpc.mockResolvedValue({ id: "fixture-review", action: "archive", rows }); const changed = vi.fn();
    const state = render(() => useSessionLifecycle("host", changed)); await state.prepare("archive", "filtered", [entry.id]);
    expect(changed).toHaveBeenCalledExactlyOnceWith(rows); expect(hooks.rpc).toHaveBeenCalledTimes(1);
  });
});
