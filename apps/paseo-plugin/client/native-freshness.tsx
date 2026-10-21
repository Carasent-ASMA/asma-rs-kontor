import { Text, View } from "react-native";
import type { PluginHostSummary } from "@getpaseo/plugin/client";
import type { DirectoryState } from "./session-directory";
import type { Colors } from "./page-controls";
export function nativeFreshnessLabel(input: { hostStatus: PluginHostSummary["status"]; status: DirectoryState["status"]; live: boolean }): string {
  if (input.hostStatus === "connecting") return "Reconnecting";
  if (input.hostStatus === "idle") return "Host idle";
  if (input.hostStatus === "offline") return "Host offline";
  if (input.hostStatus === "error") return "Host error";
  if (input.status === "idle" || input.status === "loading") return "Reading";
  if (input.status === "error") return "Unavailable";
  if (input.status === "stale" || !input.live) return "Stale";
  return "Live";
}
export function nativeFreshnessColor(label: string, colors: Colors): string {
  if (label === "Live") return colors.statusSuccess;
  if (["Reading", "Stale", "Reconnecting", "Host idle"].includes(label)) return colors.statusWarning;
  return colors.statusDanger;
}
export function inventoryReadLabel(state: Pick<DirectoryState, "lastUpdatedAt" | "inventoryComplete">): string {
  if (state.lastUpdatedAt === null) return "Inventory not read yet · time Unknown";
  // The timestamp records the latest successful walk, including partial reads.
  const readAt = new Date(state.lastUpdatedAt).toISOString();
  if (state.inventoryComplete === true) return `Last complete read ${readAt}`;
  if (state.inventoryComplete === false) return `Last partial inventory read ${readAt}`;
  return `Last inventory read ${readAt} · completeness Unknown`;
}
/** The host is named only to assistive technology: Paseo's own header shows it whenever there is a host to choose. */
export function NativeFreshnessPill({ state, hostStatus, hostLabel, colors }: { state: DirectoryState; hostStatus: PluginHostSummary["status"]; hostLabel?: string; colors: Colors }) {
  const label = nativeFreshnessLabel({ ...state, hostStatus });
  const color = nativeFreshnessColor(label, colors);
  return <View testID="kontor-native-freshness" accessible accessibilityRole="text" accessibilityLabel={`Native sessions${hostLabel ? ` from ${hostLabel}` : ""}: ${label}. ${inventoryReadLabel(state)}. Source freshness is reported separately.`} style={{ flexShrink: 1, minWidth: 0, maxWidth: "40%", borderWidth: 1, borderColor: color, borderRadius: 12, paddingHorizontal: 7, paddingVertical: 4 }}>
    <Text numberOfLines={2} style={{ color, fontSize: 12 }}>Native · {label}</Text>
  </View>;
}
