import { Text, View } from "react-native";
import type { Colors } from "./page-controls";
import { readSavedEntries, RECENT_KEPT, type RecentRecord, type RecentState } from "./recent-sessions";
import type { DirectoryEntry } from "./session-directory";
import { persistenceNote, sessionPlace, SwitchRowItem } from "./start-switch-sheet";

/** What Recent keeps against what this view shows. A kept visit missing from this host's current list stays kept. */
export function recentAccount(record: RecentRecord, known: readonly DirectoryEntry[], shown: number) {
  const ids = new Set(known.map((entry) => entry.id));
  const found = record.visits.filter((visit) => ids.has(visit.id)).length;
  return { kept: record.visits.length, shown, hidden: found - shown, absent: record.visits.length - found };
}

/** Recent never falls back to another state: an empty view says why, and the screen offers Current and All. */
export function recentEmptyMessage(record: RecentRecord): string {
  if (record.visits.length) return "None of your recent sessions is in this view.";
  if (record.clearedAt !== null) return `Recent was cleared on this device at ${new Date(record.clearedAt).toLocaleTimeString()}. Sessions you open from Kontor after that appear here.`;
  return "No recent sessions yet. Sessions you open from Kontor on this host and device appear here, most recent first.";
}

/**
 * Below the Recent list: what it keeps and where it comes from, then saved entries from an earlier version. Their visit
 * time and host are unknown, so they are listed apart, in stored order, never counted or sorted as visits; opening one
 * records a real visit. `known` is this host's current list, `candidates` what search and filters leave in it.
 */
export function RecentFooter({ recent, known, candidates, shown, colors, tall, onOpen }: {
  recent: RecentState; known: readonly DirectoryEntry[]; candidates: readonly DirectoryEntry[]; shown: number;
  colors: Colors; tall: boolean; onOpen(entry: DirectoryEntry): void;
}) {
  const { kept, hidden, absent } = recentAccount(recent.record, known, shown);
  const saved = readSavedEntries(recent.record);
  const byId = new Map(known.map((entry) => [entry.id, entry]));
  const listed = new Map(candidates.map((entry) => [entry.id, entry]));
  const savedRows = saved.ids.flatMap((id) => listed.get(id) ?? []);
  const savedAbsent = saved.ids.filter((id) => !byId.has(id));
  const savedHidden = saved.ids.length - savedRows.length - savedAbsent.length;
  const persistence = persistenceNote(recent.persistence);
  const muted = { color: colors.foregroundMuted, fontSize: 11, lineHeight: 16, paddingHorizontal: 4 } as const;
  return <View testID="sessions-recent-footer" style={{ gap: 2, paddingVertical: 12 }}>
    <Text testID="sessions-recent-account" style={muted}>
      {`Recent keeps the last ${RECENT_KEPT} sessions you opened from Kontor on this host and device, with no age limit: ${kept} kept · ${shown} shown`}
      {`${hidden ? ` · ${hidden} hidden by search, scope or filters` : ""}${absent ? ` · ${absent} not found in this host's current list` : ""}. Native sidebar opens and other devices are not included.`}
    </Text>
    {persistence ? <Text style={muted}>{persistence}</Text> : null}
    {saved.ids.length ? <View testID="sessions-recent-saved" style={{ gap: 2, marginTop: 8 }}>
      <Text accessibilityRole="header" style={{ ...muted, color: colors.foreground, fontSize: 12, fontWeight: "600" }}>Saved entries · visit time and host unknown</Text>
      <Text style={muted}>Kept from an earlier version of Kontor. They are not visits and are not counted in Recent; opening one adds it.</Text>
      {savedRows.map((entry) => <SwitchRowItem key={entry.id} role="button" row={{ entry, detail: `Saved entry · ${sessionPlace(entry)}` }} colors={colors} tall={tall} onPress={() => onOpen(entry)} />)}
      {savedHidden ? <Text style={muted}>{`${savedHidden} hidden by search, scope or filters.`}</Text> : null}
      {savedAbsent.length ? <Text testID="sessions-recent-saved-absent" selectable style={muted}>{`${savedAbsent.length} not found in this host's current list: ${savedAbsent.join(", ")}`}</Text> : null}
    </View> : null}
    {saved.unreadable ? <Text style={muted}>Saved entries from an earlier version could not be read and are left untouched.</Text> : null}
  </View>;
}
