import { View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { QUICK_FILTER_ICONS } from "./mobile-quick-filter-options";
import type { Colors } from "./page-controls";
import type { OrganizationState } from "./session-organization";
import type { StatusFilter } from "./session-view";
import { sessionStatusInk } from "./session-status-ink";

/**
 * The exact quick-filter icon for a state. The row already
 * names its status; this adds no focus target or repeated screen-reader speech.
 * Labeled status filters teach the same shapes, without another permanent legend.
 */
export function SessionStatusIcon({ status, colors, size = 16 }: { status: StatusFilter; colors: Colors; size?: number }) {
  return <View testID="session-state-marker" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" aria-hidden
    style={{ width: size, height: Math.max(18, size), flexShrink: 0, alignItems: "center", justifyContent: "center" }}>
    <Icon name={QUICK_FILTER_ICONS[status]} color={sessionStatusInk(status, colors)} size={size} />
  </View>;
}

export function SessionStateMarker({ state, colors, size }: { state: OrganizationState; colors: Colors; size?: number }) {
  return <SessionStatusIcon status={state === "failed" ? "error" : state} colors={colors} size={size} />;
}
