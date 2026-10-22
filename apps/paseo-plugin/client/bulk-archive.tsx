import { useEffect, useRef, useState, type RefObject } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { ScrollView } from "@getpaseo/plugin/client/react-native";
import { ARCHIVE_BATCH_LIMIT, unresolvedArchive, type LifecycleRow } from "../shared/session-lifecycle";
import { Action, Disclosure, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import type { useSessionLifecycle } from "./use-session-lifecycle";

type Lifecycle = ReturnType<typeof useSessionLifecycle>;

export function useArchiveSelection(hostId: string, shown: readonly string[]) {
  const [selecting, setSelecting] = useState(false), [selected, setSelected] = useState<string[]>([]);
  useEffect(() => { setSelecting(false); setSelected([]); }, [hostId]);
  const scope = JSON.stringify(shown);
  useEffect(() => { setSelected((ids) => { const kept = ids.filter((id) => shown.includes(id)); return kept.length === ids.length ? ids : kept; }); }, [scope]);
  const toggle = (id: string) => {
    if (!shown.includes(id)) return;
    setSelecting(true);
    setSelected((ids) => ids.includes(id) ? ids.filter((value) => value !== id) : ids.length < ARCHIVE_BATCH_LIMIT ? [...ids, id] : ids);
  };
  return { selecting, selected, toggle, selectShown: () => { if (shown.length <= ARCHIVE_BATCH_LIMIT) setSelected([...shown]); },
    clear: () => { setSelecting(false); setSelected([]); }, start: () => setSelecting(true) };
}

export function ArchiveSelectionCheckbox({ session, checked, disabled, pending, colors, onPress, outcome, onCheck }: {
  session: { id: string; title: string }; checked: boolean; disabled: boolean; pending: boolean; colors: Colors; onPress: () => void; outcome?: LifecycleRow; onCheck?: () => void;
}) {
  const checkbox = <Pressable testID={`sessions-select-${session.id}`} accessibilityRole="checkbox" accessibilityLabel={`Select ${session.title} (${session.id}) for archive`}
    accessibilityState={{ checked, disabled, busy: pending }} aria-checked={checked} disabled={disabled}
    {...(Platform.OS === "web" ? { onKeyDown: (event: { key: string; repeat?: boolean; preventDefault(): void; stopPropagation(): void }) => {
      if (event.key !== " " && event.key !== "Spacebar") return;
      event.preventDefault(); event.stopPropagation(); if (!event.repeat && !disabled) onPress();
    } } : {})}
    onPress={onPress} style={{ minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.5 : 1 }}>
    <Text style={{ color: colors.foreground, fontSize: 18 }}>{pending ? "…" : checked ? "☑" : "☐"}</Text>
  </Pressable>;
  return outcome && (unresolvedArchive(outcome) || outcome.reconcileReviewId) ? <View style={{ alignItems: "center" }}>{checkbox}
    <Action colors={colors} label="Check" accessibilityLabel={`Check archive outcome: ${session.title} (${session.id})`} disabled={disabled} onPress={() => onCheck?.()} />
  </View> : checkbox;
}

export function ArchiveRowFeedback({ row, colors }: { row: LifecycleRow | undefined; colors: Colors }) {
  if (!row || row.outcome === "pending" || row.outcome === "archived" && row.runtime !== "unconfirmed") return null;
  return <Text style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{`${row.outcome === "blocked" ? "Blocked" : "Archive not confirmed"}: ${row.message ?? row.reason}`}</Text>;
}

export function BulkArchiveToolbar({ selection, shownCount, disabled, lifecycle, colors, onReview, toggleRef }: {
  selection: ReturnType<typeof useArchiveSelection>; shownCount: number; disabled: boolean; lifecycle: Lifecycle; colors: Colors; onReview: () => void; toggleRef?: RefObject<View | null>;
}) {
  return <View testID="sessions-bulk-archive" style={{ gap: 4 }}>
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Action ref={toggleRef} colors={colors} label={selection.selecting ? "Cancel selection" : "Select"} disabled={disabled}
        accessibilityLabel={selection.selecting ? "Cancel archive selection" : "Select sessions for archive"} onPress={selection.selecting ? selection.clear : selection.start} />
      {selection.selecting ? <>
        <Action colors={colors} label={`Select all shown (${shownCount})`} disabled={disabled || shownCount > ARCHIVE_BATCH_LIMIT} onPress={selection.selectShown} />
        <Action colors={colors} label={`Archive selected (${selection.selected.length})`} disabled={disabled || !selection.selected.length} onPress={onReview} />
        <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{`${selection.selected.length}/${ARCHIVE_BATCH_LIMIT} selected${shownCount > ARCHIVE_BATCH_LIMIT ? ". Narrow the view to select all shown." : ""}`}</Text>
      </> : null}
      {lifecycle.running && lifecycle.batch ? <Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /> : null}
    </View>
    {lifecycle.batch ? <>
      {lifecycle.running ? <Text style={{ color: colors.foreground, fontSize: 12 }}>{`Archiving ${Math.min(lifecycle.batch.completed + 1, lifecycle.batch.total)} of ${lifecycle.batch.total}`}</Text> : null}
      <Text testID="sessions-bulk-status" role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12 }}>{lifecycle.batch.announcement}</Text>
    </> : null}
  </View>;
}

export function BulkArchiveDialog({ lifecycle, root, rootSize, colors, compact, onClose, onRun }: {
  lifecycle: Lifecycle; root: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean; onClose: () => void; onRun: () => void;
}) {
  const closeButton = useRef<View | null>(null), { review, busy, running, batch } = lifecycle;
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, []);
  const text = (value: string) => <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{value}</Text>;
  const close = () => { lifecycle.stop(); onClose(); };
  return <AnchoredMenu anchor={root} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }}
    onClose={close} colors={colors} label="Confirm bulk archive" dialog testID="bulk-archive-dialog"
    header={<View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}><Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 16, fontWeight: "600" }}>Archive selected sessions</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>}>
    <ScrollView contentContainerStyle={{ padding: 12, gap: 12 }}>
      {busy ? text("Reading selected sessions and managed children…") : review ? <>
        {text(`${review.rows.length} selected session${review.rows.length === 1 ? "" : "s"}. History and files are kept. Archive stops active work. There is no undo here.`)}
        {text("A parent can archive only when every active managed child is selected. Children archive first; a blocked or unconfirmed child keeps its parent blocked. Unselected sessions are never archived.")}
        <Disclosure colors={{ ...colors, accent: colors.foreground }} label={`Selected sessions (${review.rows.length})`}>
          {review.rows.map((row) => <View key={row.id} style={{ gap: 4, paddingVertical: 6 }}>
            {text(`${row.title} (${row.id})`)}
            {text(row.message ?? row.reason)}
            {text(row.outcome === "pending" ? row.eligible ? "Ready for confirmation" : "Blocked" : row.outcome)}
            {(unresolvedArchive(row) || row.reconcileReviewId) && !running ? <Action colors={colors} label={`Check outcome: ${row.title}`} disabled={busy} onPress={() => { void lifecycle.reconcile(row.id); }} /> : null}
          </View>)}
        </Disclosure>
        {!batch ? <Action colors={colors} label={`Archive ${review.rows.length} session${review.rows.length === 1 ? "" : "s"}`} disabled={busy || running || !review.rows.length} onPress={onRun} /> : null}
        {running ? <>{text(`Archiving ${Math.min((batch?.completed ?? 0) + 1, batch?.total ?? review.rows.length)} of ${batch?.total ?? review.rows.length}`)}<Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /></> : null}
        {batch?.summary ? text(batch.summary) : null}
      </> : null}
      {lifecycle.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{lifecycle.error}</Text> : null}
    </ScrollView>
  </AnchoredMenu>;
}
