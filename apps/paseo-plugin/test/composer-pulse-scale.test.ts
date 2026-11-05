import { describe, expect, it, vi } from "vitest";
import { Animated } from "react-native";
vi.mock("react-native", async () => {
  // A timing engine faithful where the linear-clock pin needs it: it ticks on setTimeout against
  // fake Date.now, and reproduces RN's easing contract — `config.easing ?? easeInOut()` (RN 0.81.5
  // TimingAnimation.js:75; RNW 0.21.4 TimingAnimation.js:32). The fallback here is quad-in-out, not
  // RN's exact bezier: it only has to be clearly NOT linear, which is what the pin asserts; the real
  // curve is covered by the G-RENDER-1-r3 browser gate on real react-native-web.
  const easeInOutQuad = (t: number) => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2);
  class Value {
    private listeners = new Map<number, (event: { value: number }) => void>();
    private nextId = 1;
    constructor(public value: number) {}
    setValue(value: number) { this.value = value; for (const listener of this.listeners.values()) listener({ value }); }
    addListener(listener: (event: { value: number }) => void) { const id = this.nextId++; this.listeners.set(id, listener); listener({ value: this.value }); return id; }
    removeListener(id: number) { this.listeners.delete(id); }
    interpolate() { return {}; }
  }
  return {
    View: "View",
    Platform: { OS: "web" },
    Easing: { linear: (t: number) => t, quad: (t: number) => t * t, ease: (t: number) => t * t * t, inOut: (easing: (t: number) => number) => easing },
    Animated: {
      View: "View",
      Value,
      timing(value: Value, config: { duration: number; easing?: (t: number) => number }) {
        let timer: ReturnType<typeof setTimeout> | undefined;
        let stopped = false;
        return {
          value,
          start(onEnd?: (result: { finished: boolean }) => void) {
            const easing = config.easing ?? easeInOutQuad; // the fallback under test (TimingAnimation.js:75)
            const startedAt = Date.now();
            const tick = () => {
              if (stopped) return;
              const t = Math.min((Date.now() - startedAt) / config.duration, 1);
              value.setValue(easing(t));
              if (t >= 1) onEnd?.({ finished: true });
              else timer = setTimeout(tick, 16);
            };
            timer = setTimeout(tick, 16);
          },
          stop() { stopped = true; if (timer) clearTimeout(timer); },
        };
      },
      loop(animation: { value: Value; start: (onEnd?: (result: { finished: boolean }) => void) => void; stop: () => void }) {
        let stopped = false;
        const startValue = animation.value.value;
        const run = () => {
          if (stopped) return;
          animation.start(({ finished }) => {
            if (finished && !stopped) { animation.value.setValue(startValue); run(); }
          });
        };
        return {
          start: run,
          stop() { stopped = true; },
        };
      },
    },
  };
});
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { PULSE_SAMPLES, PULSE_SCALE_MAX, pulseMotion, pulseScaleEasing, pulseScaleInterpolation, pulseTimingOptions } from "../client/composer-pulse";
import { RAINBOW_FRAMES } from "../client/running-mark";
import { observePulseEnvironment, type PulseEnvironment } from "../client/composer-pulse-environment";

describe("composer pulse breathing", () => {
  it("drives the shared clock loop over pulseTimingOptions and reads LINEAR progress at 25/50/75% and across the wrap (D7/F4)", () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    // The same construction as retainClock (composer-pulse.tsx:24): a loop over the real config.
    const value = new Animated.Value(0);
    const animation = Animated.loop(Animated.timing(value, pulseTimingOptions()));
    animation.start();
    let progress = 0;
    const id = value.addListener(({ value: v }) => { progress = v; });
    const near = (expected: number) => {
      expect(Math.abs(progress - expected)).toBeLessThanOrEqual(0.02);
    };
    try {
      // 2700 ms / 4: one quarter per 675 ms of fake time, ±0.02 (the tick grid is 16 ms).
      vi.advanceTimersByTime(675); near(0.25);
      vi.advanceTimersByTime(675); near(0.50);
      vi.advanceTimersByTime(675); near(0.75);
      // The same after one wrap: at 2700 + 675 ms the loop has restarted from 0 and reads ~0.25 again.
      vi.advanceTimersByTime(1350); near(0.25);
    } finally {
      animation.stop();
      value.removeListener(id);
      vi.useRealTimers();
    }
  });
  it("reads the pulse as a sampled cosine on the linear clock, capped at 1.12 (D7)", () => {
    // The pulse's cosine shape is a piecewise-linear sample table over the same clock. A seeded eased
    // clock (M-B) or the RN ease-in-out fallback (M-easing-drop) fails the behaviour test above.
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
