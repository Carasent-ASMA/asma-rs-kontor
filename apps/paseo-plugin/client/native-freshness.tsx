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
export function NativeFreshnessPill({ state, hostStatus, colors }: { state: DirectoryState; hostStatus: PluginHostSummary["status"]; colors: Colors }) {
  const label = nativeFreshnessLabel({ ...state, hostStatus });
  const color = nativeFreshnessColor(label, colors);
  return <View accessible accessibilityRole="text" accessibilityLabel={`Native sessions: ${label}. Last complete read ${state.lastUpdatedAt === null ? "Unknown" : new Date(state.lastUpdatedAt).toISOString()}. Source freshness is reported separately.`} style={{ borderWidth: 1, borderColor: color, borderRadius: 12, paddingHorizontal: 8, paddingVertical: 5 }}>
    <Text numberOfLines={1} style={{ color, fontSize: 12 }}>Native · {label}</Text>
  </View>;
}
