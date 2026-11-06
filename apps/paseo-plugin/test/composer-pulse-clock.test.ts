// R37-r3 fix-1 (S1): the SHIPPED clock — retainClock() over pulseTimingOptions() — driven by the REAL
// react-native-web 0.21.4 Animated engine and the real Easing, with fake timers. RNW selects
// AnimatedMock when NODE_ENV is "test" at its Animated init, so NODE_ENV goes off "test" around the
// import only (dropping that override is the M-animatedmock test mutant: the engine then never
// advances and this file fails). react-native-web is a devDependency of apps/kontor-prototype and
// resolves from there; if it cannot be resolved this file FAILS loudly — it never skips.
import { expect, it, vi } from "vitest";

vi.mock("react-native", async () => {
  const { createRequire } = await import("node:module");
  const { fileURLToPath } = await import("node:url");
  const req = createRequire(fileURLToPath(new URL("../../kontor-prototype/package.json", import.meta.url) as unknown as string));
  const entry = req.resolve("react-native-web/dist/index.js");
  const previous = process.env.NODE_ENV;
  process.env.NODE_ENV = "development";
  try {
    return { ...await import(entry) };
  } finally {
    process.env.NODE_ENV = previous;
  }
});
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: () => null }));

it("retainClock drives the REAL react-native-web Animated engine: linear 0.25/0.50/0.75 at the quarters and across two wraps (D7/F4/S1)", async () => {
  // The engine ticks on requestAnimationFrame; a setTimeout shim under fake timers supplies both the
  // ticks and the fake timestamps the timing animation reads.
  (globalThis as Record<string, unknown>).requestAnimationFrame = (cb: (t: number) => void) => setTimeout(() => cb(Date.now()), 16);
  (globalThis as Record<string, unknown>).cancelAnimationFrame = (id: ReturnType<typeof setTimeout>) => clearTimeout(id);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  const RN = (await import("react-native")) as { Animated: { timing: (value: unknown, config: unknown) => { start: () => void; stop: () => void } } };
  const realTiming = RN.Animated.timing;
  let clock: { __getValue: () => number } | undefined;
  const timing = vi.spyOn(RN.Animated, "timing").mockImplementation(((value: unknown, config: unknown) => {
    clock ??= value as { __getValue: () => number };
    return realTiming(value, config);
  }) as typeof RN.Animated.timing);
  const { retainClock } = await import("../client/composer-pulse");
  const release = retainClock();
  const near = (expected: number) => expect(Math.abs(clock!.__getValue() - expected)).toBeLessThanOrEqual(0.02);
  try {
    // retainClock exactly once: one timing construction behind the spy.
    expect(timing).toHaveBeenCalledTimes(1);
    // 25%, 50%, 75% of 2700 ms; then one wrap (2700+675) and a second wrap (5400+675) both read ~0.25.
    for (const [advance, expected] of [[675, 0.25], [675, 0.5], [675, 0.75], [1350, 0.25], [2700, 0.25]] as const) {
      vi.advanceTimersByTime(advance);
      near(expected);
    }
  } finally {
    release();
    vi.useRealTimers();
    vi.restoreAllMocks();
  }
});
