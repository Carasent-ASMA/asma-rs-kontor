import { useMemo, useRef, useState, type ReactNode } from "react";
import { Animated, PanResponder, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import type { Colors } from "./page-controls";

/** Leftward points before the row takes a drag from the press and the list scroll; a tap or wobble stays a tap. */
export const SWIPE_CLAIM = 12;
/** Leftward points a released drag must reach to archive. */
export const SWIPE_ARCHIVE = 96;
/** Horizontal travel must be at least this multiple of vertical travel; anything steeper belongs to the list scroll. */
export const SWIPE_RATIO = 2;

type Travel = { readonly dx: number; readonly dy: number };
const leftward = ({ dx, dy }: Travel, distance: number) => dx <= -distance && -dx >= SWIPE_RATIO * Math.abs(dy);
/** Whether a move is a leftward, mostly horizontal drag the row takes over. Rightward and vertical moves never are. */
export const claimsSwipe = (travel: Travel) => leftward(travel, SWIPE_CLAIM);
/** Whether a released drag archives: far enough left and still mostly horizontal. A cancelled drag never archives. */
export const archivesOnRelease = (travel: Travel) => leftward(travel, SWIPE_ARCHIVE);

/**
 * A deliberate right-to-left swipe on a row runs `onArchive`, the same Archive flow as the row menu. The row follows the
 * finger and shows Archive behind it; a short, rightward, vertical or cancelled drag springs back and does nothing.
 * The gesture is a shortcut only: the row menu's Archive stays the accessible action.
 */
export function SwipeToArchive({ enabled, colors, onArchive, children }: { enabled: boolean; colors: Colors; onArchive: () => void; children: ReactNode }) {
  const offset = useRef(new Animated.Value(0)).current;
  const [state, setState] = useState<"idle" | "drag" | "armed">("idle");
  // The responder is made once; it reads the latest props here.
  const latest = useRef({ enabled, onArchive });
  latest.current = { enabled, onArchive };
  const responder = useMemo(() => {
    const settle = () => { setState("idle"); Animated.spring(offset, { toValue: 0, useNativeDriver: false }).start(); };
    return PanResponder.create({
      onMoveShouldSetPanResponderCapture: (_, travel) => latest.current.enabled && claimsSwipe(travel),
      onPanResponderGrant: () => setState("drag"),
      onPanResponderMove: (_, travel) => { offset.setValue(Math.min(0, travel.dx)); setState(archivesOnRelease(travel) ? "armed" : "drag"); },
      onPanResponderRelease: (_, travel) => { settle(); if (latest.current.enabled && archivesOnRelease(travel)) latest.current.onArchive(); },
      onPanResponderTerminate: settle,
      // Once taken, a horizontal drag keeps the row until release, so a scroll cannot turn it into half an archive.
      onPanResponderTerminationRequest: () => false,
    });
  }, []);
  const armed = state === "armed";
  return <View>
    {state === "idle" ? null : <View importantForAccessibility="no-hide-descendants" accessibilityElementsHidden
      style={{ position: "absolute", top: 0, right: 0, bottom: 0, left: 0, flexDirection: "row", alignItems: "center", justifyContent: "flex-end", gap: 6, paddingRight: 16,
        borderRadius: 6, backgroundColor: armed ? colors.statusWarning : colors.surface2 }}>
      <Icon name="Archive" size={16} color={armed ? colors.surface0 : colors.foregroundMuted} />
      <Text style={{ color: armed ? colors.surface0 : colors.foregroundMuted, fontSize: 12, fontWeight: "600" }}>{armed ? "Release to archive" : "Archive"}</Text>
    </View>}
    <Animated.View {...responder.panHandlers} style={{ transform: [{ translateX: offset }], backgroundColor: state === "idle" ? undefined : colors.surface0 }}>{children}</Animated.View>
  </View>;
}
