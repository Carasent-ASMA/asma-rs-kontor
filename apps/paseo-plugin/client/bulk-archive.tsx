import { useEffect, useRef, useState, type RefObject } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon, Modal, ScrollView } from "@getpaseo/plugin/client/react-native";
import { ARCHIVE_BATCH_LIMIT, archiveSummary, unresolvedArchive, type LifecycleRow } from "../shared/session-lifecycle";
import { Action, COARSE_POINTER, Disclosure, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import type { useSessionLifecycle } from "./use-session-lifecycle";
import { RECOVERY_STATES, recoveryState } from "./archive-feedback";
import { SheetFooter } from "./operation-sheet";

type Lifecycle = ReturnType<typeof useSessionLifecycle>;
const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
const firstSentence = (text: string) => text.match(/^.*?[.!?](?=\s|$)/)?.[0] ?? text;
const ACTIVE = ["running", "initializing"];
/** "N of M" over the sessions the confirm button counted; a preflight-blocked row's request archives nothing. */
const progress = (rows: readonly LifecycleRow[]) => { const sent = rows.filter((row) => row.eligible); return `${Math.min(sent.filter((row) => row.outcome !== "pending").length + 1, sent.length)} of ${sent.length}`; };

/**
 * The exact session IDs chosen on one host. A change of filters never adds or drops one: `hidden` counts the selected
 * sessions the current view no longer shows. Only a shown session can be added; any selected one can be removed.
 */
export function useArchiveSelection(hostId: string, shown: readonly string[]) {
  const [selecting, setSelecting] = useState(false), [selected, setSelected] = useState<string[]>([]);
  useEffect(() => { setSelecting(false); setSelected([]); }, [hostId]);
  const toggle = (id: string) => {
    if (!shown.includes(id) && !selected.includes(id)) return;
    setSelecting(true);
    setSelected((ids) => ids.includes(id) ? ids.filter((value) => value !== id) : ids.length < ARCHIVE_BATCH_LIMIT ? [...ids, id] : ids);
  };
  /** Adds `ids`, every known match, when the selection stays within one batch; otherwise nothing changes. */
  const selectAll = (ids: readonly string[]) => setSelected((current) => { const next = [...new Set([...current, ...ids])]; return next.length <= ARCHIVE_BATCH_LIMIT ? next : current; });
  return { selecting, selected, hidden: selected.filter((id) => !shown.includes(id)).length, toggle, selectAll, selectShown: () => selectAll(shown),
    remove: (ids: readonly string[]) => setSelected((current) => current.filter((id) => !ids.includes(id))),
    clear: () => { setSelecting(false); setSelected([]); }, start: () => setSelecting(true) };
}

/**
 * Why a row did not archive and the next supported step, from its structured fields (children, eligibility, fence
 * reference, outcome); message wording is only ever shown, never parsed. With the review's rows, unselected children
 * are exact: idle ones can be added for one more confirmation, running ones are never added (no force-retire).
 */
export function archiveIssue(row: LifecycleRow, rows?: readonly LifecycleRow[]): { text: string; add: string[]; running: number; check: boolean } {
  const result = { text: unresolvedArchive(row) ? RECOVERY_STATES[recoveryState(row)].label : firstSentence(row.message ?? row.reason),
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

/** Whether a row's archive still owes the sessions list a line under the session. */
export function archiveFeedbackShown(row: LifecycleRow | undefined): row is LifecycleRow {
  return !!row && (row.outcome === "blocked" || unresolvedArchive(row));
}

/** One line in the sessions list; the full reason and the session's ID stay in the dialog's row details. */
export function ArchiveRowFeedback({ row, colors }: { row: LifecycleRow | undefined; colors: Colors }) {
  if (!archiveFeedbackShown(row)) return null;
  return <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
    <Icon name="CircleAlert" size={12} color={colors.statusWarning} />
    <Text numberOfLines={2} style={{ flexShrink: 1, color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{row.outcome === "blocked" ? `Archive blocked · ${archiveIssue(row).text}` : archiveIssue(row).text}</Text>
  </View>;
}

/** A batch's progress, Stop, its one announcement and what can follow it; selecting itself is board D1's header and bar. */
export function BulkArchiveToolbar({ selection, lifecycle, colors, maxHeight, announce = true }: {
  selection: ReturnType<typeof useArchiveSelection>; lifecycle: Lifecycle; colors: Colors; maxHeight?: number; announce?: boolean;
}) {
  const { batch, running, review } = lifecycle;
  // Results stay until Dismiss; deselecting blocked rows only changes this selection and never archives anything.
  const blocked = batch && !running ? (review?.rows ?? []).filter((row) => row.outcome === "blocked" && selection.selected.includes(row.id)).map((row) => row.id) : [];
  return <ScrollView testID="sessions-bulk-archive" style={{ flexGrow: 0, flexShrink: 0, maxHeight }} contentContainerStyle={{ gap: 4 }} keyboardShouldPersistTaps="handled">
    {running && batch ? <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}><Action colors={colors} label="Stop before next session" onPress={lifecycle.stop} /></View> : null}
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
 * Board D4: Cancel LEFT and Archive N RIGHT in one row. Anchored to `anchor` inside `root`, or the host's modal when a
 * surface has neither, such as the composer popover.
 */
export function BulkArchiveDialog({ lifecycle, root, anchor, rootSize, colors, compact, onClose, onFinished }: {
  lifecycle: Lifecycle; root?: RefObject<View | null>; anchor?: RefObject<View | null>; rootSize?: MenuSize; colors: Colors; compact: boolean;
  /** `archived`: every session any review in this dialog saw archived or gone, including reviews before Review again. */
  onClose: (archived: string[]) => void; onFinished: (summary: string) => void;
}) {
  const closeButton = useRef<View | null>(null), closed = useRef(false), { review, busy, running, batch } = lifecycle;
  const [details, setDetails] = useState<string | null>(null), [stopping, setStopping] = useState(false);
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, []);
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 };
  // Review again replaces the review's rows, so the sessions earlier batches archived are kept here for the caller.
  const archived = useRef(new Set<string>());
  for (const row of review?.rows ?? []) if (row.outcome === "archived" || row.outcome === "already_archived" || row.outcome === "absent") archived.current.add(row.id);
  const close = () => { closed.current = true; lifecycle.stop(); onClose([...archived.current]); };
  // The dialog states the outcome while it is open; a toast reports only a batch that finishes after it closed.
  const archive = async () => { setStopping(false); const summary = await lifecycle.run(); if (summary && closed.current) onFinished(summary); };
  const reading = busy && !batch, rows = reading ? [] : review?.rows ?? [], done = !!batch && !running;
  const ready = rows.filter((row) => sectionOf(row) === "ready"), blocked = rows.filter((row) => sectionOf(row) === "blocked");
  // Blocked and unattempted rows can be read again; an unconfirmed archive keeps its exact-ID Refresh status and is never resent.
  const again = rows.filter((row) => row.outcome === "pending" || row.outcome === "blocked" && !row.reconcileReviewId).map((row) => row.id);
  const preview = (ids: string[]) => { setDetails(null); setStopping(false); void lifecycle.prepare("archive", "filtered", ids, true); };
  const title = reading || !review ? "Archive selected sessions" : running ? "Archiving…" : done ? stopping ? "Archive stopped" : "Archive finished" : `Archive ${plural(rows.length, "session")}`;
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
        {issue.check ? <Action small colors={colors} label="Refresh status" accessibilityLabel={`Refresh archive status: ${row.title}`} disabled={busy} onPress={() => { void lifecycle.reconcile(row.id); }} /> : null}
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
  const body = <>
    <ScrollView style={{ flexShrink: 1 }} contentContainerStyle={{ padding: 12, gap: 12 }}>
      {reading ? <Text style={muted}>Reading selected sessions and managed children…</Text> : review ? <>
        {batch ? null : <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>Only the selected sessions are requested for archive.</Text>}
        {summary ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          {done ? <Icon name={attention ? "CircleAlert" : "Check"} size={16} color={attention ? colors.statusWarning : colors.statusSuccess} /> : null}
          <Text testID="bulk-archive-summary" {...(batch ? { role: "status", accessibilityLiveRegion: "polite" as const } : {})} style={{ flexShrink: 1, color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{summary}</Text>
        </View> : null}
        {batch ? null : <>
          <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>Archive stops active work and keeps history and files. There is no undo.</Text>
          {/* Material scope before consent: any selected session can gain a child after its final check, so this never waits in About. */}
          <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{`${ready.some((row) => row.children.length) ? "Children archive before their parents. " : ""}A new child session created just before a selected session archives may be archived with it, even if unselected.`}</Text>
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
        {batch ? null : <View style={{ flexDirection: "row", alignItems: "center", gap: 8, padding: 10, borderRadius: 8, backgroundColor: colors.surface2 }}>
          <Icon name="Info" size={16} color={colors.foregroundMuted} />
          <Text style={{ ...muted, flex: 1 }}>Archived sessions stay available in Kontor.</Text>
        </View>}
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
    {review && !reading ? <View style={{ paddingHorizontal: 7 }}>
      {running ? <SheetFooter colors={colors} secondary={{ testID: "bulk-archive-stop", label: "Stop before next session", onPress: () => { setStopping(true); lifecycle.stop(); } }} />
        : done ? <SheetFooter colors={colors} secondary={{ testID: "bulk-archive-close", label: "Close", onPress: close }} primary={again.length ? { testID: "bulk-archive-again", label: `Review ${again.length} again`,
          accessibilityLabel: `Review ${plural(again.length, "blocked or unattempted session")} again`, disabled: busy, onPress: () => preview(again) } : null} />
        : <SheetFooter colors={colors} secondary={{ testID: "bulk-archive-cancel", label: "Cancel", onPress: close }}
          primary={{ testID: "bulk-archive-confirm", label: `Archive ${ready.length}`, accessibilityLabel: `Archive ${plural(ready.length, "session")}`, disabled: busy || running || !ready.length, onPress: () => { void archive(); } }} />}
    </View> : null}
  </>;
  if (!root || !anchor || !rootSize) return <Modal title={title} open onOpenChange={(open) => { if (!open) close(); }}>
    <Modal.Content scrollable={false} contentContainerStyle={{ padding: 4, gap: 4 }}><View testID="bulk-archive-dialog" accessibilityLabel="Confirm bulk archive" style={{ flexShrink: 1, minHeight: 0 }}>{body}</View></Modal.Content>
  </Modal>;
  return <AnchoredMenu anchor={anchor} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }}
    onClose={close} colors={colors} label="Confirm bulk archive" dialog testID="bulk-archive-dialog"
    header={<View style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "center", gap: 8, paddingLeft: 7 }}><Text accessibilityRole="header" numberOfLines={2} style={{ flexShrink: 1, color: colors.foreground, fontSize: 16, fontWeight: "600" }}>{title}</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>}>
    {body}
  </AnchoredMenu>;
}
