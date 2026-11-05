import { describe, expect, it, vi, beforeEach } from "vitest";

// One sequential scenario per module load: composer-pulse holds module state (the shared clock and the
// environment subscription), so the wiring is exercised in order inside one fresh module.
const harness = vi.hoisted(() => ({
  appState: "active" as string,
  appListeners: new Set<(state: string) => void>(),
  motionListeners: new Set<(reduced: boolean) => void>(),
  reduceMotion: false,
  readRejects: false,
  timingConfigs: [] as Array<Record<string, unknown>>,
  loops: [] as Array<{ stopped: boolean }>,
}));
vi.mock("react-native", () => ({
  View: "View",
  Platform: { OS: "web" },
  AppState: {
    get currentState() { return harness.appState; },
    addEventListener: (_event: string, listener: (state: string) => void) => {
      harness.appListeners.add(listener);
      return { remove: () => harness.appListeners.delete(listener) };
    },
  },
  AccessibilityInfo: {
    isReduceMotionEnabled: () => (harness.readRejects ? Promise.reject(new Error("bridge unavailable")) : Promise.resolve(harness.reduceMotion)),
    addEventListener: (_event: string, listener: (reduced: boolean) => void) => {
      harness.motionListeners.add(listener);
      return { remove: () => harness.motionListeners.delete(listener) };
    },
  },
  Animated: {
    Value: class { setValue(value: number) { void value; } },
    timing: (_value: unknown, config: Record<string, unknown>) => { harness.timingConfigs.push(config); return {}; },
    loop: (animation: unknown) => {
      void animation;
      const loop = { stopped: false, start() { /* running */ }, stop() { this.stopped = true; } };
      harness.loops.push(loop);
      return loop;
    },
  },
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { pulseMotionSnapshot, retainClock, subscribePulseMotion } from "../client/composer-pulse";

const flush = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };
const unsubscribe = () => harness.appListeners.forEach(listener => listener === undefined ? undefined : undefined);

beforeEach(() => {
  harness.appState = "active";
  harness.reduceMotion = false;
  harness.readRejects = false;
});

describe("composer pulse wiring: environment facts to the shared clock (F4/F5/N2/N3)", () => {
  it("keeps unknown motion off, answers synchronously on subscribe, and releases the clock in the background", async () => {
    // F5/M13: BEFORE anything subscribes, motion is unknown and stays OFF — a module default of
    // flowEnabled=true would flip the snapshot on ("11") here and start the clock without any OS fact.
    expect(pulseMotionSnapshot().startsWith("1")).toBe(false);
    // N2/N3: the first subscriber gets the synchronous foreground fact at once (no static flash):
    // foreground from AppState.currentState, Reduce Motion still unknown and therefore treated reduced.
    const unsubscribeMotion = subscribePulseMotion(() => undefined);
    expect(pulseMotionSnapshot()).toBe("11");
    await flush();
    // The read has answered: foreground without Reduce Motion.
    expect(pulseMotionSnapshot()).toBe("10");
    // F4: the clock receives exactly the linear config — no easing key, 2700 ms, toValue 1, web driver off.
    const release = retainClock();
    expect(harness.timingConfigs).toHaveLength(1);
    expect(harness.timingConfigs[0]).toEqual({ toValue: 1, duration: 2700, useNativeDriver: false, isInteraction: false });
    expect(Object.keys(harness.timingConfigs[0]!)).not.toContain("easing");
    expect(harness.loops).toHaveLength(1);
    expect(harness.loops[0]!.stopped).toBe(false);
    // F5/M7: backgrounded means the facts flip off and the clock is released — nothing keeps looping.
    harness.appState = "background";
    harness.appListeners.forEach(listener => listener("background"));
    await flush();
    expect(pulseMotionSnapshot()).toBe("00");
    release();
    expect(harness.loops[0]!.stopped).toBe(true);
    // Back to active: the snapshot returns to flow+pulse and a fresh retain starts the clock again.
    harness.appState = "active";
    harness.appListeners.forEach(listener => listener("active"));
    await flush();
    expect(pulseMotionSnapshot()).toBe("10");
    const releaseAgain = retainClock();
    expect(harness.timingConfigs).toHaveLength(2);
    expect(harness.loops).toHaveLength(2);
    expect(harness.loops[1]!.stopped).toBe(false);
    releaseAgain();
    unsubscribeMotion();
  });

  it("keeps the colour flow on when the Reduce Motion read rejects, and pulses only once known (N2)", async () => {
    // The previous scenario unsubscribed, so the module reset; a rejected read must not leave the flow
    // static: foreground + unknown/rejected Reduce Motion behaves as reduced (snapshot "11").
    harness.readRejects = true;
    const unsubscribeMotion = subscribePulseMotion(() => undefined);
    await flush();
    expect(pulseMotionSnapshot()).toBe("11");
    // Once the OS answers (no rejection), the real value lands: reduced here, so still "11".
    harness.readRejects = false;
    harness.reduceMotion = true;
    harness.motionListeners.forEach(listener => listener(true));
    expect(pulseMotionSnapshot()).toBe("11");
    unsubscribeMotion();
    // The reset puts motion back to unknown-off.
    expect(pulseMotionSnapshot().startsWith("1")).toBe(false);
  });
});
