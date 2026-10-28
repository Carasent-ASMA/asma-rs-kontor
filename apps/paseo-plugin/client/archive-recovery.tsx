import { useEffect, useRef, type RefObject } from "react";
import { Text, View } from "react-native";
import { Icon, ScrollView } from "@getpaseo/plugin/client/react-native";
import { Action, Disclosure, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { RECOVERY_STATES, recoveryKey, type RecoveryItem, type RefreshNote, type useArchiveRecovery } from "./archive-feedback";
import { archivedByRead } from "./session-actions";
import { ageInWords, parseTime } from "./session-time";

type Recovery = ReturnType<typeof useArchiveRecovery>;

/** What the last Refresh status found, so an unchanged result never reads as a no-op. */
export function noteLine(note: RefreshNote, now: number): string {
  const checked = `Checked ${ageInWords(note.at, now)}`;
  if (note.outcome === "confirmed") return `${note.title}: confirmed archived · notice cleared`;
  if (note.outcome === "failed") return `Couldn't check: ${note.error ?? "no reply"}`;
  return note.outcome === "changed" && note.state ? `${checked} · now ${RECOVERY_STATES[note.state].label}` : `${checked} · unchanged`;
}

/** The facts the last exact-ID read observed, with its time; never inferred from age. */
export function observedFacts(item: RecoveryItem, now: number): string {
  const availability = item.row?.availability, checked = parseTime(availability?.checkedAt), archived = parseTime(availability?.archivedAt);
  return [checked === null ? "Not checked yet" : `Last checked ${ageInWords(checked, now)}`, archived === null ? null : `archived ${ageInWords(archived, now)}`,
    availability?.nativeStatus ? `host reports ${availability.nativeStatus}` : null].filter((fact) => fact !== null).join(" · ");
}

/**
 * Every unresolved archive in one dialog (NN/g error messages: name the problem in plain words, next to its recovery).
 * Refresh status reads the host by the exact session ID and never sends another archive request; it is the only
 * recovery. Open session is offered only while that read does not show the session archived. Archive membership and
 * the runtime stop are separate facts. An unchanged Refresh of an archived session can be acknowledged by hiding its
 * notice, which takes it out of this review until its state changes. IDs and the full host messages wait in Technical details.
 */
export function ArchiveRecoverySheet({ recovery, root, rootSize, colors, compact, canRefresh, hostOnline, onOpen, onHide, onClose }: {
  recovery: Recovery; root: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean;
  canRefresh: boolean; hostOnline: boolean; onOpen: (item: RecoveryItem) => void; onHide: (item: RecoveryItem) => void; onClose: () => void;
}) {
  const closeButton = useRef<View | null>(null);
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, []);
  const now = Date.now(), muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }, count = recovery.items.length;
  const listed = recovery.items.map((item) => recoveryKey(item.ref));
  const cleared = Object.entries(recovery.notes).filter(([key, note]) => note.outcome === "confirmed" && !listed.includes(key)).map(([key, note]) => [key, noteLine(note, now)] as const);
  const title = !count ? "No archives need attention" : count === 1 ? "Archive needs attention" : `${count} archives need attention`;
  const item = (entry: RecoveryItem, index: number) => {
    const key = recoveryKey(entry.ref), copy = RECOVERY_STATES[entry.state], note = recovery.notes[key], name = entry.row?.title ?? "Unknown session";
    const refreshing = recovery.refreshing === key || recovery.refreshing === "all";
    // Opening an archived session asks Paseo to restore its workspace, which can fail and is not needed to settle the archive.
    const archived = archivedByRead(entry.row, entry.ref.agentId);
    // Refresh status found nothing new for a session already in the archive: nothing here can change it.
    const acknowledge = archived && entry.state === "unconfirmed" && note?.outcome === "unchanged";
    const fact = (icon: string, ink: string, label: string) => <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
      <Icon name={icon} size={14} color={ink} />
      <Text style={{ flexShrink: 1, color: ink, fontSize: 13, fontWeight: "600" }}>{label}</Text>
    </View>;
    return <View key={key} role="listitem" testID={`archive-recovery-item-${entry.ref.agentId}`}
      style={{ paddingVertical: 8, paddingHorizontal: 12, gap: 4, borderTopWidth: index ? 1 : 0, borderTopColor: colors.border, backgroundColor: colors.surface0 }}>
      <Text numberOfLines={2} style={{ color: colors.foreground, fontSize: 14, lineHeight: 19, fontWeight: "600" }}>{name}</Text>
      {entry.state === "unconfirmed" ? <>
        {fact("Check", colors.foreground, "Archived")}
        {fact("CircleAlert", colors.statusWarning, "Runtime stop not confirmed")}
      </> : fact("CircleAlert", colors.statusWarning, copy.label)}
      <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{copy.reason}</Text>
      <Text style={muted}>{observedFacts(entry, now)}</Text>
      {entry.error ? <Text style={muted}>{`Its latest record could not be read: ${entry.error}`}</Text> : null}
      {note ? <Text testID={`archive-recovery-note-${entry.ref.agentId}`} role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{noteLine(note, now)}</Text> : null}
      {acknowledge ? <Text testID={`archive-recovery-ack-${entry.ref.agentId}`} style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>
        Still archived, and the host still does not report its runtime stopped. Refreshing will not change that; you can hide this notice. It returns if its state changes.</Text> : null}
      {archived ? <Text style={muted}>Not opened here: opening an archived session asks Paseo to restore its workspace, which this archive does not need. The Archived list still opens it.</Text> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        <Action colors={colors} label={refreshing ? "Checking…" : "Refresh status"} accessibilityLabel={`Refresh archive status: ${name}`} disabled={!canRefresh}
          onPress={() => { if (canRefresh) return recovery.refresh(entry.ref); }} />
        {archived ? null : <Action colors={colors} label="Open session" accessibilityLabel={`Open session: ${name}`} onPress={() => onOpen(entry)} />}
        {acknowledge ? <Action colors={colors} label="Hide notice" accessibilityLabel={`Hide archive notice: ${name}`} onPress={() => onHide(entry)} /> : null}
      </View>
      <Disclosure small colors={colors} label="Technical details" accessibilityLabel={`Technical details for ${name}`}>
        <View style={{ gap: 2, paddingTop: 2 }}>
          <Text selectable style={muted}>{`Session ID ${entry.ref.agentId}`}</Text>
          <Text selectable style={muted}>{`Review ID ${entry.ref.reviewId}`}</Text>
          {entry.row?.message ? <Text selectable style={muted}>{entry.row.message}</Text> : null}
          {entry.row?.availability ? <Text selectable style={muted}>{`${entry.row.availability.reason} Checked ${entry.row.availability.checkedAt}.`}</Text> : null}
        </View>
      </Disclosure>
    </View>;
  };
  // Anchored to the screen like the lifecycle dialog: a hidden tray or a cleared notice leaves no trigger to measure.
  return <AnchoredMenu anchor={root} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }}
    onClose={onClose} colors={colors} label="Review archive" dialog testID="archive-recovery-sheet"
    header={<View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8, paddingLeft: 7 }}>
      <Text accessibilityRole="header" numberOfLines={2} style={{ flexShrink: 1, color: colors.foreground, fontSize: 16, fontWeight: "600" }}>{title}</Text>
      <Action ref={closeButton} colors={colors} label="Close" onPress={onClose} />
    </View>}>
    <ScrollView style={{ flexShrink: 1 }} contentContainerStyle={{ padding: 12, gap: 12 }}>
      {!hostOnline ? <Text style={{ color: colors.statusWarning, fontSize: 13, lineHeight: 19 }}>The host is offline. Refresh status works again when it reconnects.</Text> : null}
      {cleared.map(([key, line]) => <Text key={key} role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{line}</Text>)}
      {count ? <View role="list" style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, overflow: "hidden" }}>{recovery.items.map(item)}</View> : null}
      {recovery.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{`Archive checks could not be read: ${recovery.error}`}</Text> : null}
      <Text style={muted}>Refresh status asks the host about this exact session. It never sends another archive request; a confirmed archive clears its notice.</Text>
      {recovery.persistence === "device" ? null : <Text style={muted}>Hidden archive notices are kept for this app session only on this device.</Text>}
    </ScrollView>
  </AnchoredMenu>;
}
