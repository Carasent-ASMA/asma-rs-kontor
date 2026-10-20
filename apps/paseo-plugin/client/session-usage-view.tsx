import type { PluginTheme } from "@getpaseo/plugin";
import { Text, View } from "react-native";
import { sessionUsageLines, type SessionUsageEntry } from "./session-usage";

import { Icon } from "@getpaseo/plugin/client/react-native";
import { compactUsage, SESSION_METADATA_HELP } from "./session-usage-density";
import { useControlHelp } from "./control-help";

type Colors = PluginTheme["colors"];

/** One secondary metadata treatment. Full IDs wrap; exact counts live in Details. */
export function SessionUsage({ entry, colors }: { entry: SessionUsageEntry; colors: Colors }) {
  const compact = compactUsage(entry);
  const hint = useControlHelp(compact.help);
  return <View ref={hint.bindings.ref} onPointerEnter={hint.bindings.onHoverIn} onPointerLeave={hint.bindings.onHoverOut} testID="session-usage"
    style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 8, rowGap: 2, minWidth: 0, flexShrink: 1 }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, minWidth: 0, flexShrink: 1 }}>
      <Icon name={compact.modelIcon} size={12} color={colors.foregroundMuted} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1 }}>{compact.model}</Text>
    </View>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, minWidth: 0, flexShrink: 1 }}>
      <Icon name="ReceiptText" size={12} color={colors.foregroundMuted} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1, fontVariant: ["tabular-nums"] }}>{compact.tokens}</Text>
    </View>
  </View>;
}

export function SessionUsageLines({ entry, colors, stale }: { entry: SessionUsageEntry; colors: Colors; stale: boolean }) {
  return <View testID="session-usage-details">
    <Text selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18, padding: 6 }}>{SESSION_METADATA_HELP}</Text>
    {sessionUsageLines(entry, stale).map(({ label, value }) => <View key={label} style={{ paddingHorizontal: 6, paddingVertical: 4 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value}</Text>
    </View>)}
  </View>;
}
