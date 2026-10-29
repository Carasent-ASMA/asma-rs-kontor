import { Icon } from "@getpaseo/plugin/client/react-native";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { AccessibilityInfo, Animated, AppState, Platform, type LayoutChangeEvent, type View } from "react-native";
import type { Colors } from "./page-controls";
import { observePulseEnvironment } from "./composer-pulse-environment";
import { composerRunningInk } from "./session-status-ink";

// One clock and one OS subscription for the visible running marks, never one timer per directory entry.
let progress: Animated.Value | undefined;
let animation: Animated.CompositeAnimation | undefined;
let users = 0;
const pulseProgress = () => progress ??= new Animated.Value(0);
function retainClock() {
  if (users++ === 0) {
    animation = Animated.loop(Animated.timing(pulseProgress(), {
      toValue: 1, duration: 2700, easing: (phase) => (1 - Math.cos(phase * Math.PI * 2)) / 2,
      useNativeDriver: Platform.OS !== "web", isInteraction: false,
    }));
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
let motionEnabled = false;
let stopEnvironment: (() => void) | undefined;
function subscribeMotion(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) stopEnvironment = observePulseEnvironment({
    currentState: AppState.currentState,
    readReducedMotion: () => AccessibilityInfo.isReduceMotionEnabled(),
    onReducedMotion: (notify) => { const subscription = AccessibilityInfo.addEventListener("reduceMotionChanged", notify); return () => subscription.remove(); },
    onAppState: (notify) => { const subscription = AppState.addEventListener("change", notify); return () => subscription.remove(); },
    onError: (error) => console.warn("Kontor motion preference could not be read", error),
  }, (next) => { motionEnabled = next; for (const notify of listeners) notify(); });
  return () => {
    listeners.delete(listener);
    if (!listeners.size) { stopEnvironment?.(); stopEnvironment = undefined; motionEnabled = false; }
  };
}
const motionSnapshot = () => motionEnabled;
const noMotion = () => false;
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

/** The composer keeps Building2; actual session rows supply their existing running glyph. */
export function ComposerPulse({ colors, size, name = "Building2", enabled = true }: { colors: Colors; size: number; name?: string; enabled?: boolean }) {
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
  const allowed = useSyncExternalStore(eligible ? subscribeMotion : noSubscription, eligible ? motionSnapshot : noMotion, noMotion);
  const active = eligible && allowed;
  const [value] = useState(pulseProgress);
  const scale = useMemo(() => value.interpolate({ inputRange: [0, 1], outputRange: [1, 1.12] }), [value]);
  const onLayout = useCallback(({ nativeEvent: { layout } }: LayoutChangeEvent) => { setVisible(layout.width > 0 && layout.height > 0); }, []);
  const carrierStyle = useMemo(() => ({ width: size, height: size, transform: [{ scale: active ? scale : 1 }] }), [size, active, scale]);
  const toneStyle = useMemo(() => ({ position: "absolute" as const, top: 0, left: 0, opacity: value }), [value]);
  useEffect(() => active ? retainClock() : undefined, [active]);
  return <Animated.View ref={carrier} testID="composer-running-pulse" onLayout={onLayout} style={carrierStyle}>
    <Icon name={name} size={size} color={composerRunningInk(colors)} />
    {active && <Animated.View testID="composer-running-tone" style={toneStyle}><Icon name={name} size={size} color={colors.statusSuccess} /></Animated.View>}
  </Animated.View>;
}
