import { Icon } from "@getpaseo/plugin/client/react-native";
import { useCallback, useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { AccessibilityInfo, Animated, AppState, Platform, type LayoutChangeEvent, type StyleProp, type ViewStyle, View } from "react-native";
import type { Colors } from "./page-controls";
import { observePulseEnvironment } from "./composer-pulse-environment";
import { composerRunningInk } from "./session-status-ink";
import { RAINBOW_FRAMES, RunningRainbowMark, rainbowFrameInterpolation, rainbowMarkSvg, runningMarkShape, type RunningMarkShape } from "./running-mark";

// One clock and one OS subscription for the visible running marks, never one timer per directory entry.
// D3-r2/D7: the clock is LINEAR and loops; it keeps running under Reduce Motion (the colour flow never
// stops), and a backgrounded or off-screen mark retains no clock at all.
let progress: Animated.Value | undefined;
let animation: Animated.CompositeAnimation | undefined;
let users = 0;
const pulseProgress = () => progress ??= new Animated.Value(0);
/** The shared clock: linear 0 -> 1, looping, about 2.7 s per revolution. No easing key — D7 forbids an
 * eased or ping-pong clock; the pulse's cosine shape comes from the sampled interpolation instead.
 * This is the ONLY path to Animated.timing's options (F4): a test asserts exactly what the clock
 * receives, so re-adding easing anywhere below the exported pins fails. */
export const pulseClockConfig = { toValue: 1, duration: 2700 };
export const pulseTimingOptions = () => ({ ...pulseClockConfig, useNativeDriver: Platform.OS !== "web", isInteraction: false });
export function retainClock() {
  if (users++ === 0) {
    animation = Animated.loop(Animated.timing(pulseProgress(), pulseTimingOptions()));
    animation.start();
  }
  let released = false;
  return () => {
    if (released) return;
    released = true;
    if (--users === 0) { animation?.stop(); animation = undefined; pulseProgress().setValue(0); }
  };
}
const listeners = new Set<() => void>();
let flowEnabled = false;   // app foreground: the shared clock runs and the colour flow animates
let reducedMotion = true;  // OS Reduce Motion: drops only the scale pulse
let stopEnvironment: (() => void) | undefined;
function subscribeMotion(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) stopEnvironment = observePulseEnvironment({
    currentState: AppState.currentState,
    readReducedMotion: () => AccessibilityInfo.isReduceMotionEnabled(),
    onReducedMotion: (notify) => { const subscription = AccessibilityInfo.addEventListener("reduceMotionChanged", notify); return () => subscription.remove(); },
    onAppState: (notify) => { const subscription = AppState.addEventListener("change", notify); return () => subscription.remove(); },
    onError: (error) => console.warn("Kontor motion preference could not be read", error),
  }, (next) => {
    flowEnabled = next.foreground;
    reducedMotion = next.reducedMotion;
    for (const notify of listeners) notify();
  });
  return () => {
    listeners.delete(listener);
    if (!listeners.size) { stopEnvironment?.(); stopEnvironment = undefined; flowEnabled = false; reducedMotion = true; }
  };
}
/** The published facts, resolved once: "10" = foreground without Reduce Motion (flow + pulse);
 * "11" = foreground with Reduce Motion (flow only); anything else = off (unknown or backgrounded). */
export const pulseMotion = (snapshot: string): { active: boolean; pulsing: boolean } =>
  ({ active: snapshot.startsWith("1"), pulsing: snapshot === "10" });
const motionSnapshot = () => `${flowEnabled ? 1 : 0}${reducedMotion ? 1 : 0}`;
/** The useSyncExternalStore contract, exported for the wiring tests (F4/F5): what a subscriber hears
 * and what the snapshot reads — including BEFORE the first subscribe, where unknown motion stays off. */
export const subscribePulseMotion = subscribeMotion;
export const pulseMotionSnapshot = () => motionSnapshot();
const noMotion = () => "00";
const noSubscription = () => () => {};

const observed = new Map<Element, (visible: boolean) => void>();
type ViewportObserver = { observe(element: Element): void; unobserve(element: Element): void; disconnect(): void };
let viewport: ViewportObserver | undefined;
function observeViewport(element: Element, notify: (visible: boolean) => void) {
  const Observer = (globalThis as { IntersectionObserver?: new (notify: (entries: { target: Element; isIntersecting: boolean }[]) => void) => ViewportObserver }).IntersectionObserver;
  if (!Observer) return () => {};
  viewport ??= new Observer((entries) => { for (const entry of entries) observed.get(entry.target)?.(entry.isIntersecting); });
  observed.set(element, notify); viewport.observe(element);
  return () => { viewport?.unobserve(element); observed.delete(element); if (!observed.size) { viewport?.disconnect(); viewport = undefined; } };
}

const absoluteFill: StyleProp<ViewStyle> = { position: "absolute", top: 0, left: 0, width: "100%", height: "100%" };

/** The breathing scale is capped at 1.12 (D2); both the component and the tests pin this constant. */
export const PULSE_SCALE_MAX = 1.12;
/** The pulse's cosine shape, sampled on the linear clock (D7): the crossfade reads t directly, the scale
 * reads the same t through this sampled cosine interpolation 1 -> 1.12 -> 1. No second clock. */
export const pulseScaleEasing = (phase: number) => (1 - Math.cos(phase * Math.PI * 2)) / 2;
export const PULSE_SAMPLES = 8;
export const pulseScaleInterpolation: { inputRange: number[]; outputRange: number[] } = {
  inputRange: Array.from({ length: PULSE_SAMPLES + 1 }, (_, step) => Number((step / PULSE_SAMPLES).toFixed(6))),
  outputRange: Array.from({ length: PULSE_SAMPLES + 1 }, (_, step) =>
    Number((1 + (PULSE_SCALE_MAX - 1) * pulseScaleEasing(step / PULSE_SAMPLES)).toFixed(6))),
};

/** K gradient frames stacked and crossfaded on the shared clock; opacity is the only animated property. */
function RainbowFlow({ colors, size, shape, clock }: { colors: Colors; size: number; shape: RunningMarkShape; clock: Animated.Value }) {
  const instance = useId();
  const svgs = useMemo(
    () => Array.from({ length: RAINBOW_FRAMES }, (_, frame) => rainbowMarkSvg(shape, colors, frame, instance)),
    [shape, colors, instance],
  );
  const opacities = useMemo(
    () => Array.from({ length: RAINBOW_FRAMES }, (_, frame) => clock.interpolate(rainbowFrameInterpolation(frame, RAINBOW_FRAMES))),
    [clock],
  );
  return <View testID="composer-running-tone" style={{ width: size, height: size }}>
    {svgs.map((svg, frame) => <Animated.View key={frame} testID={`composer-running-frame-${frame}`} style={[absoluteFill, { opacity: opacities[frame] }]}>
      <Icon icon={{ svg }} size={size} />
    </Animated.View>)}
  </View>;
}

/** The composer keeps Building2; actual session rows supply their existing running glyph. */
export function ComposerPulse({ colors, size, name = "Building2", enabled = true }: { colors: Colors; size: number; name?: string; enabled?: boolean }) {
  const shape = runningMarkShape(name);
  const [visible, setVisible] = useState(false);
  // shortcut: native host-scrolled popovers lack child viewability; adopt a public signal when one is available.
  const [inViewport, setInViewport] = useState(Platform.OS !== "web");
  const carrier = useRef<View>(null);
  useEffect(() => {
    if (Platform.OS !== "web" || !enabled || !carrier.current) return;
    setInViewport(false);
    return observeViewport(carrier.current as unknown as Element, setInViewport);
  }, [enabled]);
  const eligible = enabled && visible && inViewport;
  const motion = useSyncExternalStore(eligible ? subscribeMotion : noSubscription, eligible ? motionSnapshot : noMotion, noMotion);
  const { active, pulsing } = pulseMotion(motion);   // flow animates while foreground; only "10" adds the pulse
  const [value] = useState(pulseProgress);
  const scale = useMemo(() => value.interpolate(pulseScaleInterpolation), [value]);
  const onLayout = useCallback(({ nativeEvent: { layout } }: LayoutChangeEvent) => { setVisible(layout.width > 0 && layout.height > 0); }, []);
  const carrierStyle = useMemo(() => ({ width: size, height: size, transform: [{ scale: pulsing ? scale : 1 }] }), [size, pulsing, scale]);
  useEffect(() => active ? retainClock() : undefined, [active]);
  return <Animated.View ref={carrier} testID="composer-running-pulse" onLayout={onLayout} style={carrierStyle}>
    {shape === undefined
      ? <Icon name={name} size={size} color={composerRunningInk(colors)} />
      : active
        ? <RainbowFlow colors={colors} size={size} shape={shape} clock={value} />
        : <RunningRainbowMark colors={colors} size={size} shape={shape} />}
  </Animated.View>;
}
