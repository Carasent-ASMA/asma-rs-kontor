import { Text, View } from "react-native";
import type { DirectoryState } from "./session-directory";
import type { Colors } from "./page-controls";
export function nativeFreshnessLabel(input: { hostOnline: boolean; status: DirectoryState["status"]; live: boolean }): string {
  if (!input.hostOnline) return "Host offline";
  if (input.status === "idle" || input.status === "loading") return "Reading";
  if (input.status === "error") return "Unavailable";
  if (input.status === "stale" || !input.live) return "Stale";
  return "Live";
}
export function nativeFreshnessColor(label: string, colors: Colors): string {
  if (label === "Live") return colors.statusSuccess;
  if (label === "Reading" || label === "Stale") return colors.statusWarning;
  return colors.statusDanger;
}
export function NativeFreshnessPill({ state, hostOnline, colors }: { state: DirectoryState; hostOnline: boolean; colors: Colors }) {
  const label = nativeFreshnessLabel({ ...state, hostOnline });
  const color = nativeFreshnessColor(label, colors);
  return <View accessible accessibilityRole="text" accessibilityLabel={`Native sessions: ${label}. Last complete read ${state.lastUpdatedAt === null ? "Unknown" : new Date(state.lastUpdatedAt).toISOString()}`} style={{ borderWidth: 1, borderColor: color, borderRadius: 12, paddingHorizontal: 8, paddingVertical: 5 }}>
    <Text style={{ color, fontSize: 12 }}>{label}</Text>
  </View>;
}
