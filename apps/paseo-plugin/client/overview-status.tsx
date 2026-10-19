import { Text, View } from "react-native";
import type { SourceRead } from "../shared/read-sources";
import { historicalPlanProgress, nativeSummary } from "./page-data";
import { Copy, type Colors } from "./page-controls";

export function NativeSummaryIndicators({ counts, colors }: { counts: ReturnType<typeof nativeSummary>; colors: Colors }) {
  const indicators = [
    ["sessions", counts.total, colors.foreground], ["running", counts.running, colors.statusSuccess],
    ["idle", counts.idle, colors.foregroundMuted], ["closed", counts.closed, colors.foregroundMuted],
    ["native attention", counts.attention, colors.statusWarning], ["needs input", counts.needsInput, colors.statusWarning],
    ["native errors", counts.errors, colors.statusDanger],
  ] as const;
  return <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
    {indicators.map(([label, count, color]) => <View key={label} accessible accessibilityRole="text" accessibilityLabel={`Native snapshot: ${count} ${label}`} style={{ flexDirection: "row", alignItems: "center", gap: 5, paddingVertical: 4, paddingHorizontal: 7, borderRadius: 8, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface2 }}>
      <View importantForAccessibility="no" accessibilityElementsHidden style={{ width: 6, height: 6, borderRadius: 3, backgroundColor: count ? color : colors.foregroundMuted }} />
      <Text style={{ color: colors.foreground, fontSize: 12 }}>{count} {label}</Text>
    </View>)}
  </View>;
}

export function PlanTaskStatus({ plan, reads, colors }: { plan: SourceRead; reads: readonly SourceRead[]; colors: Colors }) {
  const graph = plan.status === "ready" ? plan.graph : null;
  const progress = graph ? historicalPlanProgress(graph, reads) : null;
  const total = progress?.total ?? 0;
  const known = progress?.known ?? 0;
  const done = progress?.done ?? 0;
  const other = known - done;
  const unknown = total - known;
  const captureTimes = [...new Set(reads.flatMap((read) => read.source.kind === "jira" && read.status === "ready" && read.jiraHistory?.epics.some((epic) => epic.key === graph?.epic.jira) ? [read.jiraHistory.generated] : []))];
  const counts = known > 0
    ? `Observed done ${done}/${total} · other observed ${other}/${total} · Unknown ${unknown}/${total} · plan coverage ${known}/${total}`
    : total > 0 ? `Task status Unknown / Not captured · plan coverage 0/${total} · ${total} tasks Unknown`
      : `Task status Unknown / Not captured · authored total ${progress ? "0" : "Not captured"} · coverage Not captured`;
  const capture = `Historical capture ${captureTimes.join(" / ") || "Not captured"} · not live`;
  const segments = [
    { key: "observed-done", count: done, color: colors.statusSuccess },
    { key: "other-observed", count: other, color: colors.accent },
    { key: "unknown", count: total ? unknown : 1, color: colors.surface2 },
  ];
  return <View style={{ gap: 5 }}>
    <Copy colors={colors}>{plan.source.title} · historical task status{total ? ` · ${total} authored tasks` : ""}</Copy>
    <View accessible accessibilityRole="image" accessibilityLabel={`${plan.source.title}: historical plan task-status distribution`}
      accessibilityValue={{ text: `${counts}. ${capture}. Current completion: Unknown.` }}
      style={{ flexDirection: "row", height: 12, borderRadius: 5, overflow: "hidden", borderWidth: 1, borderColor: colors.border }}>
      {segments.filter((segment) => segment.count > 0).map((segment) => <View key={segment.key} testID={`task-status-${segment.key}`} importantForAccessibility="no" accessibilityElementsHidden
        style={{ width: `${100 * segment.count / (total || 1)}%`, backgroundColor: segment.color, ...(segment.key === "unknown" ? { borderWidth: 1, borderStyle: "dashed", borderColor: colors.foregroundMuted } : {}) }} />)}
    </View>
    <Copy colors={colors}>{counts}</Copy>
    <Copy colors={colors} muted>{capture} · current completion Unknown</Copy>
  </View>;
}
