import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ View: "View", Platform: { OS: "web" } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { PULSE_SAMPLES, PULSE_SCALE_MAX, pulseClockConfig, pulseMotion, pulseScaleEasing, pulseScaleInterpolation } from "../client/composer-pulse";
import { RAINBOW_FRAMES } from "../client/running-mark";
import { observePulseEnvironment, type PulseEnvironment } from "../client/composer-pulse-environment";

describe("composer pulse breathing", () => {
  it("reads the pulse as a sampled cosine on the linear clock, capped at 1.12 (D7)", () => {
    // The clock itself is linear (no easing on the timing); the pulse's cosine shape is a piecewise-linear
    // sample table over the same clock. A seeded eased ping-pong clock (M-B) fails the linear-clock pin.
    expect(pulseClockConfig).toEqual({ toValue: 1, duration: 2700 });
    expect(Object.keys(pulseClockConfig)).not.toContain("easing");
    expect(PULSE_SCALE_MAX).toBeLessThanOrEqual(1.12);
    // D7: at least eight forward crossfade frames and a sampled cosine with at least eight samples.
    expect(RAINBOW_FRAMES).toBeGreaterThanOrEqual(8);
    expect(PULSE_SAMPLES).toBeGreaterThanOrEqual(8);
    // The facts resolve once: Reduce Motion ("11") keeps the flow and drops only the pulse (D3-r2);
    // backgrounded or unknown ("00") keeps nothing at all.
    expect(pulseMotion("10")).toEqual({ active: true, pulsing: true });
    expect(pulseMotion("11")).toEqual({ active: true, pulsing: false });
    expect(pulseMotion("00")).toEqual({ active: false, pulsing: false });
    expect(pulseScaleInterpolation.inputRange).toEqual(Array.from({ length: PULSE_SAMPLES + 1 }, (_, step) => Number((step / PULSE_SAMPLES).toFixed(6))));
    for (let step = 0; step <= PULSE_SAMPLES; step++) {
      const expected = Number((1 + (PULSE_SCALE_MAX - 1) * pulseScaleEasing(step / PULSE_SAMPLES)).toFixed(6));
      expect(pulseScaleInterpolation.outputRange[step]).toBe(expected);
    }
    expect(pulseScaleInterpolation.outputRange[PULSE_SAMPLES / 2]).toBe(PULSE_SCALE_MAX);
    let peak = 0;
    for (let step = 0; step <= 2700; step += 5) {
      const share = step / 2700;
      const upper = Math.min(PULSE_SAMPLES, Math.ceil(share * PULSE_SAMPLES));
      const lower = Math.max(0, upper - 1);
      const span = pulseScaleInterpolation.inputRange[upper]! - pulseScaleInterpolation.inputRange[lower]!;
      const blend = span === 0 ? 0 : (share - pulseScaleInterpolation.inputRange[lower]!) / span;
      const scale = pulseScaleInterpolation.outputRange[lower]! * (1 - blend) + pulseScaleInterpolation.outputRange[upper]! * blend;
      peak = Math.max(peak, scale);
    }
    expect(peak).toBeCloseTo(PULSE_SCALE_MAX, 9);
    expect(peak).toBeLessThanOrEqual(1.12);
  });
});

describe("composer pulse environment facts", () => {
  const envPort = (overrides: Partial<PulseEnvironment> = {}): PulseEnvironment => ({
    currentState: "active",
    readReducedMotion: () => Promise.resolve(false),
    onReducedMotion: () => () => undefined,
    onAppState: () => () => undefined,
    onError: () => undefined,
    ...overrides,
  });
  it("keeps the colour flow facts separate: foreground retains the clock, Reduce Motion drops only the pulse", async () => {
    // D3-r2: Reduce Motion never stops the colour flow — the published facts keep foreground and
    // reducedMotion distinct, so a mutant that gates the flow on the accessibility flag fails here.
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    let motionListener: ((reduced: boolean) => void) | null = null;
    const port = envPort({
      readReducedMotion: () => Promise.resolve(true),
      onReducedMotion: (listener) => { motionListener = listener; return () => { motionListener = null; }; },
    });
    const stop = observePulseEnvironment(port, (state) => published.push(state));
    // N2/N3: the first publish is synchronous at subscribe — foreground from AppState.currentState and
    // Reduce Motion still unknown, therefore reduced.
    expect(published).toEqual([{ foreground: true, reducedMotion: true }]);
    await Promise.resolve();
    expect(published).toEqual([{ foreground: true, reducedMotion: true }]); // the read confirms the same facts
    motionListener!(false);
    expect(published).toEqual([{ foreground: true, reducedMotion: true }, { foreground: true, reducedMotion: false }]);
    stop();
  });

  it("releases the clock facts when the app backgrounds (no clock retained at all)", async () => {
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    let stateListener: ((state: string) => void) | null = null;
    const port = envPort({ currentState: "active", onAppState: (listener) => { stateListener = listener; return () => { stateListener = null; }; } });
    const stop = observePulseEnvironment(port, (state) => published.push(state));
    await Promise.resolve();
    await Promise.resolve();
    stateListener!("background");
    expect(published).toEqual([{ foreground: true, reducedMotion: true }, { foreground: true, reducedMotion: false }, { foreground: false, reducedMotion: false }]);
    stop();
  });
});
