import { Icon } from "@getpaseo/plugin/client/react-native";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AccessibilityInfo,
  Animated,
  AppState,
  Platform,
  type LayoutChangeEvent,
} from "react-native";
import type { Colors } from "./page-controls";
import { observePulseEnvironment } from "./composer-pulse-environment";
import { composerRunningInk } from "./session-status-ink";

/** Only the mounted composer mark animates; directory entries never create animation clocks. */
export function ComposerPulse({ colors, size }: { colors: Colors; size: number }) {
  const [motionEnabled, setMotionEnabled] = useState(false);
  const [visible, setVisible] = useState(false);
  const [progress] = useState(() => new Animated.Value(0));
  const active = motionEnabled && visible;
  const scale = useMemo(
    () => progress.interpolate({ inputRange: [0, 1], outputRange: [1, 1.12] }),
    [progress],
  );
  const onLayout = useCallback(({ nativeEvent: { layout } }: LayoutChangeEvent) => {
    setVisible(layout.width > 0 && layout.height > 0);
  }, []);
  const carrierStyle = useMemo(
    () => ({ width: size, height: size, transform: [{ scale: active ? scale : 1 }] }),
    [size, active, scale],
  );
  const toneStyle = useMemo(
    () => ({ position: "absolute" as const, top: 0, left: 0, opacity: progress }),
    [progress],
  );
  useEffect(
    () =>
      observePulseEnvironment(
        {
          currentState: AppState.currentState,
          readReducedMotion: () => AccessibilityInfo.isReduceMotionEnabled(),
          onReducedMotion: (listener) => {
            const subscription = AccessibilityInfo.addEventListener(
              "reduceMotionChanged",
              listener,
            );
            return () => subscription.remove();
          },
          onAppState: (listener) => {
            const subscription = AppState.addEventListener("change", listener);
            return () => subscription.remove();
          },
          onError: (error) =>
            console.warn("Kontor composer motion preference could not be read", error),
        },
        setMotionEnabled,
      ),
    [],
  );
  useEffect(() => {
    if (!active) return;
    const animation = Animated.loop(
      Animated.timing(progress, {
        toValue: 1,
        duration: 2700,
        easing: (phase) => (1 - Math.cos(phase * Math.PI * 2)) / 2,
        useNativeDriver: Platform.OS !== "web",
        isInteraction: false,
      }),
    );
    animation.start();
    return () => {
      animation.stop();
      progress.setValue(0);
    };
  }, [active, progress]);
  return (
    <Animated.View testID="composer-running-pulse" onLayout={onLayout} style={carrierStyle}>
      <Icon name="Building2" size={size} color={composerRunningInk(colors)} />
      {active && (
        <Animated.View testID="composer-running-tone" style={toneStyle}>
          <Icon name="Building2" size={size} color={colors.statusSuccess} />
        </Animated.View>
      )}
    </Animated.View>
  );
}
