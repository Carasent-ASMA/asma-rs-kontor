import type { PluginTheme } from "@getpaseo/plugin";
import { Text, View } from "react-native";
import { modelPresentation, sessionUsageLines, usagePresentation, type SessionUsageEntry } from "./session-usage";

type Colors = PluginTheme["colors"];

/** One secondary metadata treatment. Full IDs wrap; exact counts live in Details. */
export function SessionUsage({ entry, colors }: { entry: SessionUsageEntry; colors: Colors }) {
  return <View testID="session-usage" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 8, rowGap: 2, minWidth: 0, flexShrink: 1 }}>
    <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1 }}>{modelPresentation(entry).label}</Text>
    <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1, fontVariant: ["tabular-nums"] }}>{usagePresentation(entry).label}</Text>
  </View>;
}

export function SessionUsageLines({ entry, colors, stale }: { entry: SessionUsageEntry; colors: Colors; stale: boolean }) {
  return <View testID="session-usage-details">
    {sessionUsageLines(entry, stale).map(({ label, value }) => <View key={label} style={{ paddingHorizontal: 6, paddingVertical: 4 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value}</Text>
    </View>)}
  </View>;
}
