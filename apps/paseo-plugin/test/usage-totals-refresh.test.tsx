import { beforeEach, describe, expect, it, vi } from "vitest";
const box = await vi.hoisted(async () => ({ hooks: (await import("./hermes-scoping")).deferredReact(), read: vi.fn() }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: (...args: unknown[]) => (box.hooks.react.useState as Function)(...args), useEffect: (...args: unknown[]) => (box.hooks.react.useEffect as Function)(...args) }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => box.read }));
import { useUsageTotals, usageTotalsRevision } from "../client/use-navigator";
import { sessionEntry } from "./session-fixtures";
beforeEach(async () => { box.hooks = (await import("./hermes-scoping")).deferredReact(); box.read.mockReset(); });
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
describe("vendor cumulative refresh", () => {
  it("reads again on a real usage/turn revision and discards a late previous result", async () => {
    const finish: Array<(value: unknown) => void> = [];
    box.read.mockImplementation(() => new Promise((resolve) => finish.push(resolve)));
    const draw = (revision: string) => box.hooks.render(() => useUsageTotals(true, ["a", "a"], revision));
    expect(draw("host-a:turn1").size).toBe(0); expect(box.read).toHaveBeenCalledExactlyOnceWith({ agentIds: ["a"] });
    draw("host-a:turn2"); expect(box.read).toHaveBeenCalledTimes(2);
    finish[1]!({ a: { input: 200, output: 7, source: "Fixture cumulative", scope: "provider-cumulative" } }); await settle();
    expect(draw("host-a:turn2").get("a")?.input).toBe(200);
    finish[0]!({ a: { input: 100, output: 5, source: "Old fixture" } }); await settle();
    expect(draw("host-a:turn2").get("a")?.input).toBe(200);
    draw("host-b:turn2"); expect(box.read).toHaveBeenCalledTimes(3);
  });
  it("does not refetch for unrelated streaming metadata and does refresh for usage, native identity or completed turns", () => {
    const entry = sessionEntry("a"), base = usageTotalsRevision("host-a", [entry]);
    expect(usageTotalsRevision("host-a", [{ ...entry, updatedAt: "later", title: "renamed" }])).toBe(base);
    expect(usageTotalsRevision("host-b", [entry])).not.toBe(base);
    expect(usageTotalsRevision("host-a", [{ ...entry, telemetry: { runtimeInfo: { provider: "codex", sessionId: "new-native-thread" } } }])).not.toBe(base);
    expect(usageTotalsRevision("host-a", [{ ...entry, activeTurn: true }])).not.toBe(base);
    expect(usageTotalsRevision("host-a", [{ ...entry, telemetry: { lastUsage: { inputTokens: 1 } } }])).not.toBe(base);
  });
  it("keeps disabled capabilities and failed reads unknown without a zero fallback", async () => {
    expect(box.hooks.render(() => useUsageTotals(false, ["a"])).size).toBe(0); expect(box.read).not.toHaveBeenCalled();
    box.read.mockRejectedValue(Error("Fixture unavailable"));
    box.hooks.render(() => useUsageTotals(true, ["a"])); await settle();
    expect(box.hooks.render(() => useUsageTotals(true, ["a"])).size).toBe(0);
  });
});

it("continues incomplete scans at an unchanged key and stops after completion", async () => {
 vi.useFakeTimers();
 try {
  box.read.mockResolvedValueOnce({ a: { input: null, output: null, source: "Codex cumulative usage unavailable: incomplete bounded source read." } })
   .mockResolvedValue({ a: { input: 200, output: 8, source: "Qualified fixture" } });
  const draw = () => box.hooks.render(() => useUsageTotals(true, ["a"], "same"));
  draw(); await vi.advanceTimersByTimeAsync(1000);
  expect(box.read).toHaveBeenCalledTimes(2);
  expect(draw().get("a")?.input).toBe(200);
  await vi.advanceTimersByTimeAsync(5000); expect(box.read).toHaveBeenCalledTimes(2);
 } finally { vi.useRealTimers(); }
});
it("cancels a pending continuation when capability is disabled", async () => {
 vi.useFakeTimers();
 try {
  box.read.mockResolvedValue({ a: { input: null, output: null, source: "incomplete bounded source read" } });
  box.hooks.render(() => useUsageTotals(true, ["a"]));
  await vi.advanceTimersByTimeAsync(0);
  box.hooks.render(() => useUsageTotals(false, ["a"]));
  await vi.advanceTimersByTimeAsync(2000); expect(box.read).toHaveBeenCalledTimes(1);
 } finally { vi.useRealTimers(); }
});
