import { useEffect, useRef, useState } from "react";
import { Text, View } from "react-native";
import { KEEP_CHILDREN_UNAVAILABLE, type LifecycleReview, type LifecycleRow } from "../shared/session-lifecycle";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import { Action, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import type { useSessionLifecycle } from "./use-session-lifecycle";

const ACTIVE = ["running", "initializing"];
const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
export const BRANCH_CHANGED = "The child sessions changed since this review. Nothing was archived. Check the updated list and choose again.";
const READING = "Reading this session's child sessions on the host…";
const UNREAD = "The host did not list this session's child sessions, so nothing can be archived here. Close this and archive the session again.";
const OFFLINE = "The host is offline. Nothing can be archived until it reconnects.";
const INTERRUPTED = "The host's availability changed while Archive children too was checking these sessions. Nothing was archived.";

/** The reviewed parent's descendants top-down for reading, each with its depth; the review itself stays child-first. */
export function branchChildren(review: LifecycleReview, parentId: string): { row: LifecycleRow; depth: number }[] {
  const byId = new Map(review.rows.map((row) => [row.id, row])), out: { row: LifecycleRow; depth: number }[] = [];
  const visit = (id: string, depth: number) => {
    for (const child of byId.get(id)?.children ?? []) {
      const row = byId.get(child.id);
      if (row && !out.some((entry) => entry.row === row)) { out.push({ row, depth }); visit(row.id, depth + 1); }
    }
  };
  visit(parentId, 1);
  return out;
}

/** Why Archive children too cannot archive this review, or null when every reviewed session is ready. */
export function branchProblem(review: LifecycleReview, parentId: string): string | null {
  const parent = review.rows.find((row) => row.id === parentId);
  if (!parent) return "This session was not in the host's review. Nothing was archived.";
  if (!branchChildren(review, parentId).length) return "The host now reports no active child sessions. Close this and archive the session again.";
  const blocked = review.rows.find((row) => row.outcome !== "pending" || !row.eligible);
  return blocked ? `${blocked.title} cannot be archived: ${blocked.message ?? blocked.reason} Nothing was archived.` : null;
}

/** Whether two reviews differ in any session, its native identity or its child set. Status is not identity. */
export function branchChanged(before: LifecycleReview, after: LifecycleReview): boolean {
  const shape = (review: LifecycleReview) => JSON.stringify(review.rows.map((row) => [row.id, row.fingerprint, row.children.map((child) => child.id).sort()]).sort());
  return shape(before) !== shape(after);
}

/**
 * The choice for a parent whose host preview found active children. Archive children too re-reads the exact branch and
 * refuses any change before the existing batch archives it child-first. Both choices show from the first frame: while the
 * branch is loading, unread or the host is offline, Archive children too stays disabled, and Keep children alive is always
 * visible and unavailable with the host's reason. Cancel, Escape or the host's close changes nothing.
 *
 * Confirming ends the displayed review: it stays only as the snapshot the fresh read must match. A failed fresh read
 * therefore leaves both choices unavailable until the session is archived again, and a changed one is shown for a new
 * choice. A fresh read hands over only in the view it started in: dismissal or any host availability change ends it.
 */
export function ArchiveChildrenDialog({ entry, lifecycle, colors, online, onClose, onArchive }: {
  entry: DirectoryEntry; lifecycle: ReturnType<typeof useSessionLifecycle>; colors: Colors; online: boolean;
  onClose: () => void;
  /** `review` is the current reviewed branch: the caller runs it with `lifecycle.run`. */
  onArchive: (review: LifecycleReview) => void;
}) {
  const [reviewed, setReviewed] = useState<LifecycleReview | null>(null), [refusal, setRefusal] = useState<string | null>(null);
  const closed = useRef(false), view = useRef({ online, generation: 0 });
  if (view.current.online !== online) view.current = { online, generation: view.current.generation + 1 };
  const read = (then: (review: LifecycleReview) => void) => lifecycle.branch(entry.id).then((result) => {
    if (closed.current) return;
    if (result === null) setRefusal("Another archive action is running. Close this and try again when it finishes.");
    else if (typeof result !== "string") then(result);
  });
  useEffect(() => { void read(setReviewed); }, []);
  const close = () => { closed.current = true; onClose(); };
  const confirm = () => {
    if (!reviewed || problem) return;
    const snapshot = reviewed, started = view.current.generation;
    setReviewed(null); setRefusal(null);
    return read((fresh) => {
      if (view.current.generation !== started) { setRefusal(INTERRUPTED); return; }
      const change = branchChanged(snapshot, fresh) ? BRANCH_CHANGED : branchProblem(fresh, entry.id);
      if (change) { setReviewed(fresh); setRefusal(change); return; }
      closed.current = true; onArchive(fresh);
    });
  };
  const children = reviewed ? branchChildren(reviewed, entry.id) : [];
  const running = children.filter(({ row }) => ACTIVE.includes(row.nativeStatus));
  const problem = !online ? OFFLINE : reviewed ? branchProblem(reviewed, entry.id) : lifecycle.error || refusal ? UNREAD : READING;
  const keep = reviewed?.keepChildren?.reason ?? KEEP_CHILDREN_UNAVAILABLE;
  const text = { color: colors.foreground, fontSize: 13, lineHeight: 19 }, muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 };
  const alert = { color: colors.statusWarning, fontSize: 12, lineHeight: 17 };
  return <OperationSheet testID="archive-children-dialog" title="Archive a session with children" subtitle={entry.title} colors={colors} onClose={close}
    footer={<SheetFooter colors={colors} secondary={{ testID: "archive-children-cancel", label: "Cancel", onPress: close }}
      primary={{ testID: "archive-children-confirm", label: "Archive children too", hint: problem, disabled: !!problem || lifecycle.busy || lifecycle.running, onPress: () => { void confirm(); } }} />}>
    {problem === READING ? <Text role="status" style={muted}>{READING}</Text> : null}
    {reviewed ? <>
      <Text style={text}>{`${plural(children.length, "active child session")} belong to this session. Choose what happens to them.`}</Text>
      <View role="list" style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, overflow: "hidden" }}>
        {children.map(({ row, depth }, index) => <View key={row.id} role="listitem" testID={`archive-children-row-${row.id}`}
          style={{ paddingVertical: 6, paddingRight: 10, paddingLeft: 12 * depth, gap: 2, borderTopWidth: index ? 1 : 0, borderTopColor: colors.border }}>
          <Text numberOfLines={2} style={text}>{row.title}</Text>
          <Text style={ACTIVE.includes(row.nativeStatus) ? alert : muted}>{depth > 1 ? `${row.nativeStatus} · nested` : row.nativeStatus}</Text>
        </View>)}
      </View>
      {running.length ? <Text testID="archive-children-running" style={alert}>{`Running now: ${running.map(({ row }) => row.title).join(", ")}. Archive stops their active work.`}</Text> : null}
      <Text style={text}>Archive children too archives these sessions first, the deepest first, then this session. Archive stops any active work and keeps history and files. There is no undo.</Text>
      <Text style={muted}>A new child session started just before this session archives may be archived with it by the host, even though it is not listed here.</Text>
    </> : null}
    <View style={{ gap: 4 }}>
      <Action colors={colors} label="Keep children alive" accessibilityLabel={`Keep children alive, unavailable. ${keep}`} disabled onPress={() => undefined} />
      <Text testID="archive-children-keep-reason" style={muted}>{keep}</Text>
    </View>
    {problem && problem !== READING ? <Text style={alert}>{problem}</Text> : null}
    {refusal ? <Text testID="archive-children-refusal" accessibilityRole="alert" style={alert}>{refusal}</Text> : null}
    {lifecycle.error ? <Text accessibilityRole="alert" style={alert}>{lifecycle.error}</Text> : null}
  </OperationSheet>;
}
