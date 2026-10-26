import { Text } from "react-native";
import type { SessionProjection } from "../shared/session-lifecycle";
import type { Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
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

export function CompactRetirementNotice({ projection, colors }: { projection: SessionProjection; colors: Colors }) {
  const notice = compactRetirementNotice(projection);
  return notice ? <Text testID={`sessions-compact-warning-${projection.sessionId}`} style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{notice}</Text> : null;
}
