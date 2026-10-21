import type { PluginTheme } from "@getpaseo/plugin";
import { useState } from "react";
import { Text, View } from "react-native";
import type { SessionUsageEntry } from "./session-usage";

import { Icon } from "@getpaseo/plugin/client/react-native";
import { compactUsage } from "./session-usage-density";
import { useControlHelp } from "./control-help";
import { Action } from "./page-controls";
import { SessionDetailSection } from "./session-detail-section";
import { sessionMetadataSections, sessionReportNotes } from "./session-metadata-content";

type Colors = PluginTheme["colors"];

/** One secondary metadata treatment. Full IDs wrap; exact counts live in Details. */
export function SessionUsage({ entry, colors }: { entry: SessionUsageEntry; colors: Colors }) {
  const compact = compactUsage(entry);
  const modelHint = useControlHelp(compact.modelHelp);
  const usageHint = useControlHelp(compact.usageHelp);
  return <View testID="session-usage"
    style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 8, rowGap: 2, minWidth: 0, flexShrink: 1 }}>
    <View ref={modelHint.bindings.ref} onPointerEnter={modelHint.bindings.onHoverIn} onPointerLeave={modelHint.bindings.onHoverOut} testID="session-model-help" style={{ flexDirection: "row", alignItems: "center", gap: 4, minWidth: 0, flexShrink: 1 }}>
      <Icon name={compact.modelIcon} size={12} color={colors.foregroundMuted} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1 }}>{compact.model}</Text>
    </View>
    <View ref={usageHint.bindings.ref} onPointerEnter={usageHint.bindings.onHoverIn} onPointerLeave={usageHint.bindings.onHoverOut} testID="session-counter-help" style={{ flexDirection: "row", alignItems: "center", gap: 4, minWidth: 0, flexShrink: 1 }}>
      <Icon name="ReceiptText" size={12} color={colors.foregroundMuted} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1, fontVariant: ["tabular-nums"] }}>{compact.tokens}</Text>
    </View>
  </View>;
}

export function SessionUsageLines({ entry, colors, stale }: { entry: SessionUsageEntry; colors: Colors; stale: boolean }) {
  return <View testID="session-usage-details">
    {sessionMetadataSections(entry, stale).map(section => <SessionDetailSection key={section.title} {...section} colors={colors} />)}
  </View>;
}

/** One optional explanation, reached by the same Details control on pointer, keyboard and touch. */
export function SessionMetadataGuide({ entry, colors }: { entry: SessionUsageEntry; colors: Colors }) {
  const [expanded, setExpanded] = useState(false);
  return <View style={{ paddingHorizontal: 6, paddingVertical: 8, gap: 8 }}>
    <Action label="Model and usage guide" expanded={expanded} colors={colors} onPress={() => setExpanded(value => !value)} />
    {expanded ? <View testID="session-metadata-guide" style={{ gap: 10 }}>
      {[
        { icon: "SlidersHorizontal", text: "Selected model configuration; execution is not confirmed." },
        { icon: "Radio", text: "Last reported runtime model; it may precede a switch." },
        { icon: "CircleHelp", text: "No model supplied." },
        { icon: "ReceiptText", text: "Reported input ↓ and output ↑ counters, not session totals." },
      ].map(({ icon, text }) => <View key={icon} style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
        <View importantForAccessibility="no" accessibilityElementsHidden><Icon name={icon} size={16} color={colors.foregroundMuted} /></View>
        <Text style={{ flex: 1, color: colors.foreground, fontSize: 12, lineHeight: 18 }}>{text}</Text>
      </View>)}
      <Text style={{ color: colors.foreground, fontSize: 12, lineHeight: 18 }}>— means not reported. 0 is an explicit reported zero. Known model families use short names; custom IDs keep their exact spelling.</Text>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>About this report</Text>
      {sessionReportNotes(entry).map(note => <Text key={note} selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{note}</Text>)}
    </View> : null}
  </View>;
}
