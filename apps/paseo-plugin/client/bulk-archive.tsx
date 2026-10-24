import { useEffect, useRef, useState, type RefObject } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon, ScrollView } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
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

export function ArchiveSelectionToggle({ selection, disabled, colors, toggleRef }: {
  selection: ReturnType<typeof useArchiveSelection>; disabled: boolean; colors: Colors; toggleRef?: RefObject<View | null>;
}) {
  const hint = selection.selecting ? "Cancel selection. Clear selected sessions without archiving." : "Select sessions. Choose sessions for archive, then review and confirm.";
  const help = useActionHelp(hint, toggleRef);
  return <Pressable {...help.bindings} testID="sessions-selection-toggle" accessibilityRole="button"
    accessibilityLabel={selection.selecting ? "Cancel archive selection" : "Select sessions for archive"} accessibilityHint={hint}
    accessibilityState={{ selected: selection.selecting, disabled }} aria-pressed={selection.selecting} disabled={disabled}
    onPress={() => { help.dismiss(); if (selection.selecting) selection.clear(); else selection.start(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minWidth: 44, minHeight: 44, paddingHorizontal: 8, paddingVertical: 6,
      alignItems: "center", justifyContent: "center", borderRadius: 6, borderWidth: 1, borderColor: focused ? colors.foreground : "transparent",
      backgroundColor: pressed || selection.selecting ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 })}>
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name={selection.selecting ? "X" : "ListChecks"} size={18} color={colors.foreground} /></View>
  </Pressable>;
}

export function BulkArchiveToolbar({ selection, shownCount, disabled, lifecycle, colors, onReview, reviewRef, maxHeight, announce = true }: {
  selection: ReturnType<typeof useArchiveSelection>; shownCount: number; disabled: boolean; lifecycle: Lifecycle; colors: Colors; onReview: () => void; reviewRef?: RefObject<View | null>; maxHeight?: number; announce?: boolean;
}) {
  return <ScrollView testID="sessions-bulk-archive" style={{ flexGrow: 0, flexShrink: 0, maxHeight }} contentContainerStyle={{ gap: 4 }} keyboardShouldPersistTaps="handled">
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      {selection.selecting ? <>
        <Action colors={colors} label={`Select all shown (${shownCount})`} disabled={disabled || shownCount > ARCHIVE_BATCH_LIMIT} onPress={selection.selectShown} />
        <Action ref={reviewRef} colors={colors} label={`Archive selected (${selection.selected.length})`} disabled={disabled || !selection.selected.length} onPress={onReview} />
        <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{`${selection.selected.length}/${ARCHIVE_BATCH_LIMIT} selected${shownCount > ARCHIVE_BATCH_LIMIT ? ". Narrow the view to select all shown." : ""}`}</Text>
      </> : null}
      {lifecycle.running && lifecycle.batch ? <Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /> : null}
    </View>
    {lifecycle.batch ? <>
      {lifecycle.running ? <Text style={{ color: colors.foreground, fontSize: 12 }}>{`Archiving ${Math.min(lifecycle.batch.completed + 1, lifecycle.batch.total)} of ${lifecycle.batch.total}`}</Text> : null}
      {announce ? <Text testID="sessions-bulk-status" role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12 }}>{lifecycle.batch.announcement}</Text> : null}
    </> : null}
  </ScrollView>;
}

export function BulkArchiveDialog({ lifecycle, root, anchor, rootSize, colors, compact, onClose, onFinished }: {
  lifecycle: Lifecycle; root: RefObject<View | null>; anchor: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean; onClose: () => void; onFinished: (summary: string) => void;
}) {
  const closeButton = useRef<View | null>(null), { review, busy, running, batch } = lifecycle;
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, []);
  const text = (value: string) => <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{value}</Text>;
  const close = () => { lifecycle.stop(); onClose(); };
  return <AnchoredMenu anchor={anchor} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }}
    onClose={close} colors={colors} label="Confirm bulk archive" dialog testID="bulk-archive-dialog"
    header={<View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center" }}><Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 16, fontWeight: "600" }}>Archive selected sessions</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>}>
    <ScrollView contentContainerStyle={{ padding: 12, gap: 12 }}>
      {busy ? text("Reading selected sessions and managed children…") : review ? <>
        {text(`${review.rows.length} selected session${review.rows.length === 1 ? "" : "s"}. History and files are kept. Archive stops active work. There is no undo here.`)}
        {text("At preflight, a parent is blocked unless every active managed child is selected. Children archive first; a blocked or unconfirmed child keeps its parent blocked.")}
        {text("Only selected session IDs are directly requested for archive. The child check and parent archive are not atomic: a new child created between the final check and parent archive may be archived by the host with its parent, even if unselected.")}
        <Disclosure colors={{ ...colors, accent: colors.foreground }} label={`Selected sessions (${review.rows.length})`}>
          {review.rows.map((row) => <View key={row.id} style={{ gap: 4, paddingVertical: 6 }}>
            {text(`${row.title} (${row.id})`)}
            {text(row.message ?? row.reason)}
            {text(row.outcome === "pending" ? row.eligible ? "Ready for confirmation" : "Blocked" : row.outcome)}
            {(unresolvedArchive(row) || row.reconcileReviewId) && !running ? <Action colors={colors} label={`Check outcome: ${row.title}`} disabled={busy} onPress={() => { void lifecycle.reconcile(row.id); }} /> : null}
          </View>)}
        </Disclosure>
        {!batch ? <Action colors={colors} label={`Archive ${review.rows.length} session${review.rows.length === 1 ? "" : "s"}`} disabled={busy || running || !review.rows.length} onPress={async () => { const summary = await lifecycle.run(); if (summary) onFinished(summary); }} /> : null}
        {running ? <>{text(`Archiving ${Math.min((batch?.completed ?? 0) + 1, batch?.total ?? review.rows.length)} of ${batch?.total ?? review.rows.length}`)}<Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /></> : null}
        {batch ? <Text testID="sessions-bulk-status" role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12 }}>{batch.announcement}</Text> : null}
      </> : null}
      {lifecycle.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{lifecycle.error}</Text> : null}
    </ScrollView>
  </AnchoredMenu>;
}
