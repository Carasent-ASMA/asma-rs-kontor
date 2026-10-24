import { useEffect, useRef, useState, type RefObject } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon, ScrollView } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
import { ARCHIVE_BATCH_LIMIT, archiveSummary, unresolvedArchive, type LifecycleRow } from "../shared/session-lifecycle";
import { Action, COARSE_POINTER, Disclosure, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import type { useSessionLifecycle } from "./use-session-lifecycle";

type Lifecycle = ReturnType<typeof useSessionLifecycle>;
const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
const firstSentence = (text: string) => text.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? text;
const ACTIVE = ["running", "initializing"];
/** "N of M" over the sessions the confirm button counted; a preflight-blocked row's request archives nothing. */
const progress = (rows: readonly LifecycleRow[]) => { const sent = rows.filter((row) => row.eligible); return `${Math.min(sent.filter((row) => row.outcome !== "pending").length + 1, sent.length)} of ${sent.length}`; };

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
    remove: (ids: readonly string[]) => setSelected((current) => current.filter((id) => !ids.includes(id))),
    clear: () => { setSelecting(false); setSelected([]); }, start: () => setSelecting(true) };
}

/**
 * Why a row did not archive and the next supported step, from its structured fields (children, eligibility, fence
 * reference, outcome); message wording is only ever shown, never parsed. With the review's rows, unselected children
 * are exact: idle ones can be added for one more confirmation, running ones are never added (no force-retire).
 */
export function archiveIssue(row: LifecycleRow, rows?: readonly LifecycleRow[]): { text: string; add: string[]; running: number; check: boolean } {
  const result = { text: row.outcome === "archived" && row.runtime === "unconfirmed" ? "Archived, but stopping its runtime is not confirmed" : firstSentence(row.message ?? row.reason),
    add: [] as string[], running: 0, check: !!row.reconcileReviewId || unresolvedArchive(row) };
  if (result.check || !row.children.length) return result;
  if (!row.eligible) {
    const missing = rows ? row.children.filter((child) => !rows.some((entry) => entry.id === child.id)) : row.children;
    if (!missing.length) return result;
    const running = missing.filter((child) => ACTIVE.includes(child.status)).length;
    return { ...result, running, add: rows ? missing.filter((child) => !ACTIVE.includes(child.status)).map((child) => child.id) : [],
      text: rows ? `${plural(missing.length, "active child", "active children")} not selected${running ? ` · ${running} running` : ""}` : `Has ${plural(missing.length, "active child", "active children")}` };
  }
  const waiting = rows?.some((entry) => row.children.some((child) => child.id === entry.id) && !(entry.outcome === "already_archived" || entry.outcome === "archived" && entry.runtime !== "unconfirmed"));
  return waiting && row.outcome === "blocked" ? { ...result, text: "Waits for its selected children to archive" } : result;
}

/** A drawn box in a 44pt target. Fill plus a check or minus mark the state, so colour is never the only cue. */
function Checkbox({ testID, label, hint, checked, disabled, pending = false, colors, onPress }: {
  testID: string; label: string; hint?: string; checked: boolean | "mixed"; disabled: boolean; pending?: boolean; colors: Colors; onPress: () => void;
}) {
  const on = checked !== false;
  return <Pressable testID={testID} accessibilityRole="checkbox" accessibilityLabel={label} accessibilityHint={hint}
    accessibilityState={{ checked, disabled, busy: pending }} aria-checked={checked} disabled={disabled}
    {...(Platform.OS === "web" ? { onKeyDown: (event: { key: string; repeat?: boolean; preventDefault(): void; stopPropagation(): void }) => {
      if (event.key !== " " && event.key !== "Spacebar") return;
      event.preventDefault(); event.stopPropagation(); if (!event.repeat && !disabled) onPress();
    } } : {})}
    onPress={onPress} style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center",
      borderRadius: 8, borderWidth: 2, borderColor: focused ? colors.foreground : "transparent", opacity: disabled ? 0.4 : 1 })}>
    <View testID={`${testID}-box`} style={{ width: 22, height: 22, borderRadius: 6, borderWidth: 2, alignItems: "center", justifyContent: "center",
      borderColor: on ? colors.foreground : colors.foregroundMuted, backgroundColor: on ? colors.foreground : undefined }}>
      {pending ? <Icon name="Loader" size={14} color={on ? colors.surface0 : colors.foregroundMuted} /> : on ? <Icon name={checked === "mixed" ? "Minus" : "Check"} size={16} color={colors.surface0} /> : null}
    </View>
  </Pressable>;
}

function PrimaryButton({ label, accessibilityLabel, disabled, colors, onPress, ref }: { label: string; accessibilityLabel?: string; disabled: boolean; colors: Colors; onPress: () => void; ref?: RefObject<View | null> }) {
  return <Pressable ref={ref} accessibilityRole="button" accessibilityLabel={accessibilityLabel ?? label} accessibilityState={{ disabled }} aria-disabled={disabled} disabled={disabled} onPress={onPress}
    style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: COARSE_POINTER ? 44 : 34, minWidth: 44, paddingHorizontal: 14, borderRadius: 8, alignItems: "center", justifyContent: "center",
      borderWidth: 2, borderColor: focused ? colors.foreground : "transparent", backgroundColor: disabled ? colors.surface2 : colors.accent })}>
    <Text numberOfLines={1} style={{ color: disabled ? colors.foregroundMuted : colors.accentForeground, fontSize: 13, fontWeight: "600" }}>{label}</Text>
  </Pressable>;
}

export function ArchiveSelectionCheckbox({ session, checked, disabled, pending, colors, onPress, outcome, onCheck }: {
  session: { id: string; title: string }; checked: boolean; disabled: boolean; pending: boolean; colors: Colors; onPress: () => void; outcome?: LifecycleRow; onCheck?: () => void;
}) {
  const checkbox = <Checkbox testID={`sessions-select-${session.id}`} label={`Select ${session.title} (${session.id}) for archive`} checked={checked} disabled={disabled} pending={pending} colors={colors} onPress={onPress} />;
  return outcome && (unresolvedArchive(outcome) || outcome.reconcileReviewId) ? <View style={{ alignItems: "center" }}>{checkbox}
    <Action colors={colors} label="Check" accessibilityLabel={`Check archive outcome: ${session.title} (${session.id})`} disabled={disabled} onPress={() => onCheck?.()} />
  </View> : checkbox;
}

/** One line in the sessions list; the full reason and the session's ID stay in the dialog's row details. */
export function ArchiveRowFeedback({ row, colors }: { row: LifecycleRow | undefined; colors: Colors }) {
  if (!row || !(row.outcome === "blocked" || unresolvedArchive(row))) return null;
  return <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
    <Icon name="CircleAlert" size={12} color={colors.statusWarning} />
    <Text numberOfLines={2} style={{ flexShrink: 1, color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{`${row.outcome === "blocked" ? "Archive blocked" : "Archive not confirmed"} · ${archiveIssue(row).text}`}</Text>
  </View>;
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
  const { batch, running, review } = lifecycle, count = selection.selected.length, all = shownCount > 0 && count === shownCount;
  // Results stay until Dismiss; deselecting blocked rows only changes this selection and never archives anything.
  const blocked = batch && !running ? (review?.rows ?? []).filter((row) => row.outcome === "blocked" && selection.selected.includes(row.id)).map((row) => row.id) : [];
  return <ScrollView testID="sessions-bulk-archive" style={{ flexGrow: 0, flexShrink: 0, maxHeight }} contentContainerStyle={{ gap: 4 }} keyboardShouldPersistTaps="handled">
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
      {selection.selecting ? <>
        <Checkbox testID="sessions-select-all" colors={colors} checked={all ? true : count ? "mixed" : false} disabled={disabled || !shownCount || (!all && shownCount > ARCHIVE_BATCH_LIMIT)}
          label={all ? "Clear selection" : `Select all ${plural(shownCount, "shown session")}`}
          hint={shownCount > ARCHIVE_BATCH_LIMIT ? `More than ${ARCHIVE_BATCH_LIMIT} shown: narrow the view or select sessions one by one.` : undefined}
          onPress={() => all ? selection.remove(selection.selected) : selection.selectShown()} />
        <Text style={{ color: colors.foreground, fontSize: 13, fontWeight: "600", fontVariant: ["tabular-nums"] }}>{`${count} selected`}</Text>
        {shownCount > ARCHIVE_BATCH_LIMIT || count >= ARCHIVE_BATCH_LIMIT ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{`Max ${ARCHIVE_BATCH_LIMIT} per batch`}</Text> : null}
        <View style={{ flexGrow: 1 }} />
        <PrimaryButton ref={reviewRef} colors={colors} label={`Archive ${count}…`} accessibilityLabel={`Review archive of ${plural(count, "selected session")}`} disabled={disabled || !count} onPress={onReview} />
      </> : null}
      {running && batch ? <Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /> : null}
    </View>
    {batch ? <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
      {running ? <Text style={{ color: colors.foreground, fontSize: 12 }}>{`Archiving ${progress(review?.rows ?? [])}`}</Text> : null}
      {announce ? <Text testID="sessions-bulk-status" role="status" accessibilityLiveRegion="polite" style={{ flexShrink: 1, color: colors.foreground, fontSize: 12 }}>{batch.announcement}</Text> : null}
      {announce && blocked.length ? <Action small colors={colors} label="Deselect blocked" accessibilityLabel={`Deselect ${plural(blocked.length, "blocked session")}`} onPress={() => selection.remove(blocked)} /> : null}
      {announce && !running ? <Action small colors={colors} label="Dismiss" accessibilityLabel="Dismiss archive results" onPress={lifecycle.clear} /> : null}
    </View> : null}
  </ScrollView>;
}

type Section = "blocked" | "unconfirmed" | "ready" | "archived" | "already" | "absent";
const SECTIONS: readonly Section[] = ["blocked", "unconfirmed", "ready", "archived", "already", "absent"];
const sectionOf = (row: LifecycleRow): Section => unresolvedArchive(row) ? "unconfirmed" : row.outcome === "blocked" || row.outcome === "pending" && !row.eligible ? "blocked"
  : row.outcome === "pending" ? "ready" : row.outcome === "archived" ? "archived" : row.outcome === "already_archived" ? "already" : row.outcome === "absent" ? "absent" : "unconfirmed";

/**
 * Review, then one explicit confirmation (HIG alerts: short title, one consequence, verb buttons, Cancel). Each status
 * is a section heading once; rows lead with the title and one reason, and IDs and full messages wait in row details.
 */
export function BulkArchiveDialog({ lifecycle, root, anchor, rootSize, colors, compact, onClose, onFinished }: {
  lifecycle: Lifecycle; root: RefObject<View | null>; anchor: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean; onClose: () => void; onFinished: (summary: string) => void;
}) {
  const closeButton = useRef<View | null>(null), closed = useRef(false), { review, busy, running, batch } = lifecycle;
  const [details, setDetails] = useState<string | null>(null), [stopping, setStopping] = useState(false);
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, []);
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 };
  const close = () => { closed.current = true; lifecycle.stop(); onClose(); };
  // The dialog states the outcome while it is open; a toast reports only a batch that finishes after it closed.
  const archive = async () => { setStopping(false); const summary = await lifecycle.run(); if (summary && closed.current) onFinished(summary); };
  const reading = busy && !batch, rows = reading ? [] : review?.rows ?? [], done = !!batch && !running;
  const ready = rows.filter((row) => sectionOf(row) === "ready"), blocked = rows.filter((row) => sectionOf(row) === "blocked");
  // Blocked and unattempted rows can be read again; an unconfirmed archive keeps its exact-ID Check and is never resent.
  const again = rows.filter((row) => row.outcome === "pending" || row.outcome === "blocked" && !row.reconcileReviewId).map((row) => row.id);
  const preview = (ids: string[]) => { setDetails(null); setStopping(false); void lifecycle.prepare("archive", "filtered", ids, true); };
  const title = reading || !review ? "Archive selected sessions" : running ? "Archiving…" : done ? stopping ? "Archive stopped" : "Archive finished" : `Archive ${plural(rows.length, "session")}?`;
  const summary = running && batch ? `${stopping ? "Stopping after session" : "Session"} ${progress(rows)}`
    : done ? archiveSummary(rows) : blocked.length ? `${ready.length} ready · ${blocked.length} blocked` : null;
  const attention = rows.some((row) => ["blocked", "unconfirmed"].includes(sectionOf(row)));
  const item = (row: LifecycleRow, index: number) => {
    const section = sectionOf(row), issue = section === "blocked" || section === "unconfirmed" ? archiveIssue(row, rows) : null, open = details === row.id;
    const note = issue?.text ?? (section === "ready" ? running && batch?.currentId === row.id ? "Archiving…" : row.children.length ? `Archives after its ${plural(row.children.length, "child", "children")}` : null
      : section === "archived" && row.runtime === "not-observed" ? "Runtime not reported closed" : null);
    const overCap = !!issue && again.length + issue.add.length > ARCHIVE_BATCH_LIMIT;
    return <View key={row.id} role="listitem" testID={`bulk-archive-row-${row.id}`} style={{ paddingVertical: 6, paddingLeft: 12, paddingRight: 2, gap: 4, borderTopWidth: index ? 1 : 0, borderTopColor: colors.border, backgroundColor: colors.surface0 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
          <Text numberOfLines={2} style={{ color: colors.foreground, fontSize: 14, lineHeight: 19 }}>{row.title}</Text>
          {note ? <Text style={muted}>{note}</Text> : null}
        </View>
        <Pressable accessibilityRole="button" accessibilityLabel={`Details for ${row.title}`} accessibilityState={{ expanded: open }} aria-expanded={open} onPress={() => setDetails(open ? null : row.id)}
          style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ width: 44, height: 44, alignItems: "center", justifyContent: "center", borderRadius: 8, borderWidth: 2, borderColor: focused ? colors.foreground : "transparent" })}>
          <Icon name="Info" size={16} color={colors.foregroundMuted} />
        </Pressable>
      </View>
      {issue && !running && (issue.add.length || issue.check) ? <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        {issue.add.length && !overCap ? <Action small colors={colors} label={`Add ${plural(issue.add.length, "idle child", "idle children")}`} accessibilityLabel={`Add ${plural(issue.add.length, "idle child", "idle children")} of ${row.title} and review again`}
          disabled={busy} onPress={() => preview([...again, ...issue.add])} /> : null}
        {issue.check ? <Action small colors={colors} label="Check" accessibilityLabel={`Check outcome: ${row.title}`} disabled={busy} onPress={() => { void lifecycle.reconcile(row.id); }} /> : null}
      </View> : null}
      {issue && overCap ? <Text style={muted}>{`Adding its children would exceed the ${ARCHIVE_BATCH_LIMIT}-session batch cap.`}</Text> : null}
      {issue?.running ? <Text style={muted}>Finish running children in their own workflow, then review again.</Text> : null}
      {open ? <View style={{ gap: 2, paddingRight: 10, paddingBottom: 4 }}>
        <Text selectable style={muted}>{`ID ${row.id}`}</Text>
        <Text selectable style={muted}>{row.message ?? row.reason}</Text>
        {row.children.map((child) => <Text key={child.id} selectable style={muted}>{`Child: ${child.title} · ${child.status} · ${child.id}`}</Text>)}
      </View> : null}
    </View>;
  };
  const list = (entries: LifecycleRow[]) => <View role="list" style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, overflow: "hidden" }}>{entries.map(item)}</View>;
  const LABELS: Record<Section, string> = { blocked: "Blocked", unconfirmed: "Not confirmed", ready: done ? "Not attempted" : running ? "Queued" : "Ready",
    archived: "Archived", already: "Already archived", absent: "Not found on this host" };
  return <AnchoredMenu anchor={anchor} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }}
    onClose={close} colors={colors} label="Confirm bulk archive" dialog testID="bulk-archive-dialog"
    header={<View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8, paddingLeft: 7 }}><Text accessibilityRole="header" numberOfLines={2} style={{ flexShrink: 1, color: colors.foreground, fontSize: 16, fontWeight: "600" }}>{title}</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>}>
    <ScrollView style={{ flexShrink: 1 }} contentContainerStyle={{ padding: 12, gap: 12 }}>
      {reading ? <Text style={muted}>Reading selected sessions and managed children…</Text> : review ? <>
        {summary ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          {done ? <Icon name={attention ? "CircleAlert" : "Check"} size={16} color={attention ? colors.statusWarning : colors.statusSuccess} /> : null}
          <Text testID="bulk-archive-summary" {...(batch ? { role: "status", accessibilityLiveRegion: "polite" as const } : {})} style={{ flexShrink: 1, color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{summary}</Text>
        </View> : null}
        {batch ? null : <>
          <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>Archive stops active work and keeps history and files. There is no undo.</Text>
          {ready.some((row) => row.children.length) ? <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>Children archive before their parents. A child started during the run may be archived with its parent.</Text> : null}
        </>}
        {SECTIONS.map((section) => {
          const entries = rows.filter((row) => sectionOf(row) === section);
          if (!entries.length) return null;
          const heading = `${LABELS[section]} · ${entries.length}`;
          if (section === "archived" && batch) return <Disclosure key={section} small colors={colors} label={heading}>{list(entries)}</Disclosure>;
          const warn = section === "blocked" || section === "unconfirmed";
          return <View key={section} style={{ gap: 6 }}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
              {warn ? <Icon name="CircleAlert" size={14} color={colors.statusWarning} /> : null}
              <Text accessibilityRole="header" style={{ color: warn ? colors.statusWarning : colors.foregroundMuted, fontSize: 12, fontWeight: "600" }}>{heading}</Text>
            </View>
            {list(entries)}
          </View>;
        })}
        <Disclosure small colors={colors} label="About archive">
          <View style={{ gap: 6, paddingTop: 2 }}>
            <Text style={muted}>Only selected session IDs are directly requested for archive. At preflight, a parent is blocked unless every active managed child is selected. Children archive first; a blocked or unconfirmed child keeps its parent blocked.</Text>
            <Text style={muted}>The child check and parent archive are not atomic: a new child created between the final check and parent archive may be archived by the host with its parent, even if unselected.</Text>
            <Text style={muted}>Running children are never added for you, and blocked sessions stay in the list unarchived. Add idle children, finish running ones in their own workflow, or deselect, then review again.</Text>
          </View>
        </Disclosure>
      </> : null}
      {lifecycle.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{lifecycle.error}</Text> : null}
    </ScrollView>
    {review && !reading ? <View style={{ flexDirection: "row", flexWrap: "wrap", justifyContent: "flex-end", alignItems: "center", gap: 8, paddingHorizontal: 7, paddingTop: 6, borderTopWidth: 1, borderTopColor: colors.border }}>
      {running ? <Action colors={colors} label="Stop before next session" onPress={() => { setStopping(true); lifecycle.stop(); }} />
        : done ? <>
          {again.length ? <Action colors={colors} label={`Review ${again.length} again`} accessibilityLabel={`Review ${plural(again.length, "blocked or unattempted session")} again`} disabled={busy} onPress={() => preview(again)} /> : null}
          <PrimaryButton colors={colors} label="Done" disabled={false} onPress={close} />
        </> : <>
          <Action colors={colors} label="Cancel" onPress={close} />
          <PrimaryButton colors={colors} label={`Archive ${plural(ready.length, "session")}`} disabled={busy || running || !ready.length} onPress={() => { void archive(); }} />
        </>}
    </View> : null}
  </AnchoredMenu>;
}
