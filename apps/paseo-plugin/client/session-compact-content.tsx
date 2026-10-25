import type { ReactNode } from "react";
import { Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { freshAvailability, type SessionProjection } from "../shared/session-lifecycle";
import type { Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { sessionState } from "./session-organization";
import { SessionStateMarker } from "./session-state-marker";
import type { RowTime } from "./session-time";
import { compactUsage } from "./session-usage-density";
import { attentionLabel, earlierErrorLabel, sessionAvailabilityLabel } from "./session-view";

export function sessionRowDetails(entry: DirectoryEntry) {
  return [
    { label: "Native status", value: entry.status },
    { label: "Archive membership", value: entry.archivedAt === null ? "Not archived" : `Archived · ${entry.archivedAt}` },
    { label: "Attention", value: attentionLabel(entry) ?? "None recorded" },
    { label: "Earlier error", value: earlierErrorLabel(entry) ?? "None recorded" },
    { label: "Availability", value: sessionAvailabilityLabel(entry) ?? "Exact availability is not established by this row; the native snapshot remains the source." },
  ];
}

/** Presentation of existing evidence only; disposition/reasons remain in Details. */
export function compactRetirementNotice(projection: SessionProjection): string | null {
  if (projection.conflicts.length) return projection.conflicts.some(conflict => conflict.kind.startsWith("archived-with-")) ? "Archived status conflict" : "Evidence conflict";
  return projection.retirement.attention ? "Needs review" : null;
}

/** Inside the existing row button: no new touch target, read, clock or subscription. */
export function SessionCompactContent({ entry, colors, title, context, time, children }: {
  entry: DirectoryEntry; colors: Colors; title?: string; context?: string; time: RowTime | null; children?: ReactNode;
}) {
  const model = compactUsage(entry);
  const missing = entry.requiresAttention && freshAvailability(entry)?.state === "missing";
  return <View testID="sessions-compact-content" style={{ flex: 1, minWidth: 0, gap: 2 }}>
    <Text testID="sessions-row-title" numberOfLines={3} style={{ color: colors.foreground, fontSize: 14, lineHeight: 20, minWidth: 0 }}>{title ?? entry.title}</Text>
    {context ? <Text testID="sessions-row-meta" numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>{context}</Text> : null}
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 8, rowGap: 2, minWidth: 0 }}>
      <SessionStateMarker state={sessionState(entry)} colors={colors} />
      {entry.archivedAt !== null ? <Text testID="sessions-row-archived" style={{ color: colors.foregroundMuted, fontSize: 11 }}>Archived</Text> : null}
      <View style={{ flexDirection: "row", alignItems: "center", gap: 4, minWidth: 0, flexShrink: 1 }}>
        <Icon name={model.modelIcon} size={12} color={colors.foregroundMuted} />
        <Text testID="sessions-row-model" numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, flexShrink: 1 }}>{model.model}</Text>
      </View>
      {time ? <Text testID="sessions-row-age" numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, flexShrink: 0 }}>{time.compact}</Text> : null}
    </View>
    {missing ? <Text style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>Native record absent</Text> : null}
    {children}
  </View>;
}

export function CompactRetirementNotice({ projection, colors }: { projection: SessionProjection; colors: Colors }) {
  const notice = compactRetirementNotice(projection);
  return notice ? <Text testID={`sessions-compact-warning-${projection.sessionId}`} style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{notice}</Text> : null;
}
