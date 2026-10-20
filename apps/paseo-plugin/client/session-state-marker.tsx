import { Text, View } from "react-native";
import { stateColor } from "./organization-row";
import type { Colors } from "./page-controls";
import type { OrganizationState } from "./session-organization";
import { CHAIN_STATE_GLYPHS } from "./session-results";

function luminance(color: string): number {
  const hex = color.length === 4 ? color.slice(1).split("").map(value => value + value).join("") : color.slice(1);
  const value = Number.parseInt(hex, 16);
  return [16, 8, 0].reduce((sum, shift, index) => {
    const channel = ((value >> shift) & 255) / 255;
    const linear = channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    return sum + linear * [0.2126, 0.7152, 0.0722][index]!;
  }, 0);
}

/** Keep the shape readable on normal, raised and hovered rows in the active theme. */
function markerColor(state: OrganizationState, colors: Colors): string {
  const ink = stateColor(state, colors);
  const foreground = luminance(ink);
  const readable = [colors.surface0, colors.surface1, colors.surface2].every(surface => {
    const background = luminance(surface);
    return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05) >= 3;
  });
  return readable ? ink : colors.foreground;
}

/**
 * A stable shape per state, shared with the Board's session chain. The row already
 * names its status; this adds no focus target or repeated screen-reader speech.
 * Labeled status filters teach the same shapes, without another permanent legend.
 */
export function SessionStateMarker({ state, colors }: { state: OrganizationState; colors: Colors }) {
  return <View testID="session-state-marker" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" aria-hidden
    style={{ width: 16, height: 18, flexShrink: 0, alignItems: "center", justifyContent: "center" }}>
    <Text style={{ color: markerColor(state, colors), fontSize: 15, lineHeight: 18, fontWeight: "700", textAlign: "center" }}>{CHAIN_STATE_GLYPHS[state]}</Text>
  </View>;
}
