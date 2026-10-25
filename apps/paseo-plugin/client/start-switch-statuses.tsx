import { Text, View, useWindowDimensions } from "react-native";
import type { PluginHostSummary } from "@getpaseo/plugin/client";
import { QuickToolbarButton } from "./mobile-quick-toolbar";
import { QUICK_FILTER_ICONS } from "./mobile-quick-filter-options";
import { nativeFreshnessLabel } from "./native-freshness";
import type { Colors } from "./page-controls";
import type { DirectoryState } from "./session-directory";
import { STATUS_SHORTCUTS, STATUS_SHORTCUT_SCOPE, startSwitchCounts, type StateShortcut } from "./session-state-shortcuts";
import { sessionStatusInk } from "./session-status-ink";

/** Derives from the caller's existing directory; no inventory read, subscription, timer or provider probe. */
export function StartSwitchStatuses({ directory, hostStatus, colors, onSelect }: {
  directory: DirectoryState; hostStatus: PluginHostSummary["status"]; colors: Colors; onSelect(shortcut: StateShortcut): void;
}) {
  const { fontScale } = useWindowDimensions();
  const target = Math.max(44, Math.ceil(36 * fontScale));
  const freshness = nativeFreshnessLabel({ hostStatus, status: directory.status, live: directory.live });
  const counts = startSwitchCounts(directory, freshness);
  const readNote = directory.lastUpdatedAt === null ? "Counts not read yet" : directory.inventoryComplete !== true
    ? `${freshness === "Live" ? "Partial inventory" : freshness} · counts updating` : freshness !== "Live" ? `${freshness} · counts updating` : null;
  return <View testID="kontor-switch-statuses" style={{ gap: 4 }}>
    <Text testID="kontor-switch-status-scope" style={{ color: colors.foregroundMuted, fontSize: 11 }}>This host · all projects · nonarchived</Text>
    {readNote ? <Text testID="kontor-switch-status-read" style={{ color: colors.foregroundMuted, fontSize: 11 }}>{readNote}</Text> : null}
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
      {STATUS_SHORTCUTS.map(({ status, label, caption }) => <View key={status} style={{ minWidth: target, flexGrow: 1, alignItems: "center", gap: 2 }}>
        <QuickToolbarButton testID={`kontor-switch-status-${status}`} icon={QUICK_FILTER_ICONS[status]} iconColor={sessionStatusInk(status, colors)}
          colors={colors} target={target} count={counts[status]} label={`${label}, ${counts[status].label}. This host, all projects, nonarchived. Opens Sessions with this filter.`}
          onPress={() => onSelect({ status, scope: STATUS_SHORTCUT_SCOPE })} />
        <Text style={{ color: colors.foregroundMuted, fontSize: 11, textAlign: "center" }}>{caption}</Text>
      </View>)}
    </View>
  </View>;
}
