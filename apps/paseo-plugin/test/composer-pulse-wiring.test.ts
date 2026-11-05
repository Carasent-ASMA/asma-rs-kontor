import { describe, expect, it, vi, beforeEach } from "vitest";
import type { ReactElement } from "react";
import type { Colors } from "../client/page-controls";

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
    View: "View",
    Value: class { interpolate() { return {}; } setValue(value: number) { void value; } },
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

// A minimal hook runtime so the real ComposerPulse component can be drawn as a plain function and its
// effects run in order, cleanups first — enough to exercise the component-level retain link
// `active ? retainClock() : undefined` in vitest (N6/X-retain).
const component = vi.hoisted(() => ({
  cells: [] as unknown[],
  cursor: 0,
  drawnEffects: [] as Array<{ fx: () => void | (() => void); deps?: readonly unknown[] }>,
  cleanups: [] as Array<void | (() => void)>,
  depsAt: [] as Array<readonly unknown[] | undefined>,
  storeUnsubscribe: null as null | (() => void),
}));
vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    useState: (init: unknown) => {
      const slot = component.cursor++;
      if (component.cells[slot] === undefined) component.cells[slot] = typeof init === "function" ? (init as () => unknown)() : init;
      const set = (value: unknown) => { component.cells[slot] = typeof value === "function" ? (value as (previous: unknown) => unknown)(component.cells[slot]) : value; };
      return [component.cells[slot], set];
    },
    useRef: (initial: unknown) => {
      const slot = component.cursor++;
      component.cells[slot] ??= { current: initial };
      return component.cells[slot];
    },
    useEffect: (fx: () => void | (() => void), deps?: readonly unknown[]) => { component.drawnEffects.push({ fx, deps }); },
    useId: () => "pulse-under-test",
    useMemo: (factory: () => unknown) => factory(),
    useCallback: (fn: unknown) => fn,
    useSyncExternalStore: (subscribe: (listener: () => void) => () => void, read: () => unknown) => {
      component.storeUnsubscribe?.();
      component.storeUnsubscribe = subscribe(() => { /* the next draw re-reads the snapshot */ });
      return read();
    },
  };
});

import { ComposerPulse, pulseMotionSnapshot, retainClock, subscribePulseMotion } from "../client/composer-pulse";

const flush = async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); };

const dark: Colors = {
  surface0: "#1a1a1e", surface1: "#222226", surface2: "#2a2a2e", border: "#3f3f46",
  foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b",
};

/** Runs the effects a draw queued, React-style: a changed dependency first runs its previous cleanup. */
const runEffects = () => {
  component.drawnEffects.forEach((effect, index) => {
    const previous = component.depsAt[index];
    const changed = previous === undefined || effect.deps === undefined || effect.deps.length !== previous.length
      || effect.deps.some((dep, position) => !Object.is(dep, previous[position]));
    if (!changed) return;
    component.cleanups[index]?.();
    component.cleanups[index] = effect.fx() ?? undefined;
    component.depsAt[index] = effect.deps;
  });
};
const drawPulse = (visible: boolean, inViewport: boolean): ReactElement => {
  component.cursor = 0;
  component.drawnEffects = [];
  component.cells[0] = visible;
  component.cells[1] = inViewport;
  return ComposerPulse({ colors: dark, size: 16 }) as ReactElement;
};

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

  it("the component retains the clock only while its mark is active, and releases through the effect cleanup (N6/X-retain)", async () => {
    // The real component, drawn through the hook harness. An INACTIVE mark (off-screen or facts off)
    // must not retain the shared clock at all — a mutant that retains regardless fails right here.
    const first = drawPulse(false, false);
    expect(first).toBeDefined();
    const clocksBefore = harness.timingConfigs.length;
    const loopsBefore = harness.loops.length;
    runEffects();
    expect(harness.timingConfigs).toHaveLength(clocksBefore);
    // Eligible mark with foreground facts: the link retains the clock exactly once.
    harness.appListeners.forEach(listener => listener("active"));
    await flush();
    drawPulse(true, true);
    runEffects();
    expect(harness.timingConfigs).toHaveLength(clocksBefore + 1);
    expect(harness.loops[loopsBefore]!.stopped).toBe(false);
    // Same active state again: the dependency did not change, so no second retain.
    drawPulse(true, true);
    runEffects();
    expect(harness.timingConfigs).toHaveLength(clocksBefore + 1);
    // Backgrounded facts deactivate the mark: the previous effect's cleanup releases the clock.
    harness.appState = "background";
    harness.appListeners.forEach(listener => listener("background"));
    await flush();
    drawPulse(true, true);
    runEffects();
    expect(harness.loops[loopsBefore]!.stopped).toBe(true);
    // Foreground again: a fresh retain starts a new loop.
    harness.appState = "active";
    harness.appListeners.forEach(listener => listener("active"));
    await flush();
    drawPulse(true, true);
    runEffects();
    expect(harness.timingConfigs).toHaveLength(clocksBefore + 2);
    expect(harness.loops[loopsBefore + 1]!.stopped).toBe(false);
  });
});
