import { useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { readLifecycleReview, reconcileLifecycleItem, unresolvedArchive, type LifecycleReview, type LifecycleRow, type UnresolvedRef } from "../shared/session-lifecycle";
import type { ProfileStorage } from "./viewer-profile";

const text = (failure: unknown) => failure instanceof Error ? failure.message : String(failure);

/**
 * One status line per X (NN/g visibility of system status). The host toast owns politeness (WCAG 2.1 SC 4.1.3).
 * Membership is the success; an unconfirmed runtime is reported, never called closed.
 */
export function notifyArchive(toast: { show(message: string): void; error(message: string): void }, result: LifecycleRow | string | null) {
  if (result === null) return;
  if (typeof result === "string") toast.error(`Archive did not report a result: ${result}`);
  else if (result.outcome === "archived" && result.runtime !== "unconfirmed") toast.show(`Archived ${result.title}.`);
  else if (result.outcome === "already_archived" || result.outcome === "absent") toast.show(result.message ?? result.reason);
  // The host's own wording names internal checks; it stays in the review sheet's technical details.
  else if (unresolvedArchive(result)) toast.error(`${result.title}: ${RECOVERY_STATES[recoveryState(result)].short}.`);
  else toast.error(result.message ?? result.reason);
}

/**
 * Which confirmation an unresolved archive is missing, from its owner row's structured fields; message wording is
 * shown, never parsed. Blocked and preflight outcomes release their fence, so they stay in the bulk review.
 */
export type RecoveryState = "unknown" | "unconfirmed" | "unavailable";
export const RECOVERY_STATES: Readonly<Record<RecoveryState, { readonly label: string; readonly short: string; readonly reason: string }>> = {
  unknown: { label: "Archive result unknown", short: "archive result unknown",
    reason: "The archive request was sent, but the host has not confirmed whether the session was archived. Another archive request will not be sent while its outcome is uncertain." },
  unconfirmed: { label: "Archived · runtime stop not confirmed", short: "archived, runtime stop not confirmed",
    reason: "The host lists it in the archive but has not reported its runtime stopped, so it is not called stopped. Refresh status asks again; no archive request is sent again." },
  unavailable: { label: "Couldn't check", short: "couldn't check archive",
    reason: "The host could not be read, so this archive's state is not known. Refresh status when the host is available." },
};
export function recoveryState(row: LifecycleRow | null): RecoveryState {
  if (!row || row.availability?.state === "unknown") return "unavailable";
  // An uncertain request whose exact-ID read already shows it archived is an archive whose stop is unconfirmed.
  return row.outcome === "archived" || row.availability?.state === "readable" && !!row.availability.archivedAt ? "unconfirmed" : "unknown";
}
/** `row` is the owner's merged row; `error` is why this read of it failed, keeping the last known row and state. */
export type RecoveryItem = { readonly ref: UnresolvedRef; readonly state: RecoveryState; readonly row: LifecycleRow | null; readonly error: string | null };
/** What one Refresh status found against the item it started from. */
export type RefreshNote = { readonly at: number; readonly outcome: "unchanged" | "changed" | "confirmed" | "failed"; readonly title: string; readonly state?: RecoveryState; readonly error?: string };
export const recoveryKey = (ref: UnresolvedRef) => `${ref.reviewId}:${ref.agentId}`;
/** A notice key per exact review, session and state, so a changed state is a new notice; hidden keys of this shape persist. */
export const archiveNoticeKey = (item: RecoveryItem) => `archive:${recoveryKey(item.ref)}:${item.state}`;
/**
 * Hide notice after an unchanged Refresh status acknowledges that exact item and state. Unlike a hidden tray, it takes
 * the item out of the notice, its count and the review; it stays unresolved and returns as a new notice if its state changes.
 */
export const archiveAckKey = (item: RecoveryItem) => `archive:ack:${recoveryKey(item.ref)}:${item.state}`;
const titleOf = (item: RecoveryItem | undefined) => item?.row?.title ?? "An archived session";

/** One compact line: a single item names its session and state, several are counted. Known items stay beside a failed lookup. */
export function recoveryLine(items: readonly RecoveryItem[], error: string | null): string | null {
  const line = !items.length ? null : items.length === 1 ? `${titleOf(items[0])}: ${RECOVERY_STATES[items[0]!.state].short}` : `${items.length} archives need attention`;
  if (!line) return error ? `Archive checks could not be read: ${error}` : null;
  return error ? `${line}. ${error}` : line;
}

export function refreshNote(before: RecoveryItem | undefined, result: LifecycleRow, at = Date.now()): RefreshNote {
  if (!unresolvedArchive(result)) return { at, outcome: "confirmed", title: result.title };
  const state = recoveryState(result);
  return { at, outcome: before && state !== before.state ? "changed" : "unchanged", title: result.title, state };
}

/**
 * Hidden archive notices, remembered per host on this device (iOS user defaults, web and desktop local storage)
 * through the store Working as uses. Elsewhere, or when the store fails, hiding lasts this app session and says so.
 * Only `archive:` keys are kept; a key a complete read no longer reports is forgotten by the screen.
 */
export type HiddenPersistence = "device" | "session" | "failed" | "unreadable";
export type HiddenArchives = { readonly keys: readonly string[]; readonly persistence: HiddenPersistence };
export const hiddenArchivesKey = (hostId: string) => "kontor.archive-notices@1 " + JSON.stringify([hostId.trim()]);
let storage: ProfileStorage | null = null;
const hiddenByHost = new Map<string, HiddenArchives>();
/** The Kontor screen module sets the device store when the plugin loads; null keeps hiding for this app session. */
export function configureArchiveNoticeStorage(next: ProfileStorage | null): void {
  storage = next;
  hiddenByHost.clear();
}
export function readHiddenArchives(hostId: string): HiddenArchives {
  const known = hiddenByHost.get(hostId);
  if (known) return known;
  let loaded: HiddenArchives = { keys: [], persistence: storage ? "device" : "session" };
  try {
    const value = storage?.read(hiddenArchivesKey(hostId));
    if (value !== null && value !== undefined) {
      const keys: unknown = JSON.parse(String(value));
      loaded = Array.isArray(keys) && keys.every((key) => typeof key === "string" && key.startsWith("archive:")) ? { keys, persistence: "device" } : { keys: [], persistence: "unreadable" };
    }
  } catch {
    loaded = { keys: [], persistence: "unreadable" };
  }
  hiddenByHost.set(hostId, loaded);
  return loaded;
}
/** Writes only a change, or a retry of a failed write. A failed write keeps the keys for this app session and says so. */
export function writeHiddenArchives(hostId: string, keys: readonly string[]): HiddenArchives {
  const current = readHiddenArchives(hostId);
  if (current.persistence !== "failed" && keys.length === current.keys.length && keys.every((key) => current.keys.includes(key))) return current;
  let next: HiddenArchives = { keys, persistence: "session" };
  if (storage) {
    try { storage.write(hiddenArchivesKey(hostId), keys.length ? JSON.stringify(keys) : null); next = { keys, persistence: "device" }; }
    catch { next = { keys, persistence: "failed" }; }
  }
  hiddenByHost.set(hostId, next);
  return next;
}

/**
 * Archives whose fence is still held, from the durable fences rather than latest.json, so they survive later
 * reviews and a remount. Each listed ref is classified by its own owner review, read by its exact ID. Reads between
 * actions only; a listed ref is a hint, and Refresh status is an exact-ID read per owner review that never sends
 * archive(). No reconnect or timer refresh: a refresh runs only when pressed.
 */
export function useArchiveRecovery(hostId: string, lifecycle: { review: unknown; busy: boolean; running: boolean }, onChanged: (rows: LifecycleRow[]) => void) {
  const read = useRpc(readLifecycleReview), reconcile = useRpc(reconcileLifecycleItem);
  const [items, setItems] = useState<readonly RecoveryItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  // The ref key being refreshed, "all", or null.
  const [refreshing, setRefreshing] = useState<string | null>(null);
  const [notes, setNotes] = useState<Readonly<Record<string, RefreshNote>>>({});
  // The host whose list a read reported since mount or host switch; until then an empty list is unknown, not none.
  const [listed, setListed] = useState<string | null>(null);
  // Every listed item's owner row was read: only then may the screen forget a hidden key this read no longer reports.
  const [complete, setComplete] = useState(false);
  const [, setRevision] = useState(0);
  // A host switch starts a new run; a result from an earlier run, even for the same host ID, changes nothing.
  // `live` is false once the view's cleanup ran: retired work sends no further read and writes no state.
  const owner = useRef(hostId), run = useRef(0), loads = useRef(0), live = useRef(false), active = useRef(false), known = useRef<readonly RecoveryItem[]>([]);
  owner.current = hostId;
  const ownerItem = async (ref: UnresolvedRef, reviews: Map<string, Promise<LifecycleReview | null>>): Promise<RecoveryItem | null> => {
    try {
      if (!reviews.has(ref.reviewId)) reviews.set(ref.reviewId, read({ reviewId: ref.reviewId }));
      const review = await reviews.get(ref.reviewId)!;
      const row = review?.id === ref.reviewId && review.action === "archive" ? review.rows.find((entry) => entry.id === ref.agentId) : undefined;
      if (!row) throw new Error("The host did not return this archive's own review.");
      // Settled since the list was read: no longer recovery work.
      return unresolvedArchive(row) ? { ref, state: recoveryState(row), row, error: null } : null;
    } catch (failure) {
      const prior = known.current.find((item) => recoveryKey(item.ref) === recoveryKey(ref));
      return { ref, state: prior?.state ?? "unavailable", row: prior?.row ?? null, error: text(failure) };
    }
  };
  const load = async () => {
    const host = hostId, current = ++loads.current, stale = () => !live.current || owner.current !== host || current !== loads.current;
    if (stale()) return;
    try {
      const result = await read({});
      if (stale()) return;
      // null: no review and no fence on this host. A review without the field (older server) is unknown: keep what is known.
      if (result && !result.unresolved) { setError("The host did not report its current list."); setComplete(false); return; }
      const next: RecoveryItem[] = [], reviews = new Map<string, Promise<LifecycleReview | null>>();
      let partial = false;
      for (const ref of result?.unresolved ?? []) {
        if (stale()) return;
        const item = await ownerItem(ref, reviews);
        if (item?.error) partial = true;
        if (item) next.push(item);
      }
      if (stale()) return;
      known.current = next; setItems(next); setError(null); setListed(host); setComplete(!partial);
    } catch (failure) { if (!stale()) { setError(text(failure)); setComplete(false); } }
  };
  // An unmount ends the run and every load too: a pending list or owner read sends nothing further, and a refresh
  // loop never reconciles, loads or calls onChanged for an unmounted view.
  useEffect(() => {
    live.current = true; run.current++; active.current = false; known.current = [];
    setItems([]); setError(null); setRefreshing(null); setNotes({}); setListed(null); setComplete(false);
    return () => { live.current = false; run.current++; loads.current++; };
  }, [hostId]);
  useEffect(() => { if (!lifecycle.busy && !lifecycle.running) void load(); }, [hostId, lifecycle.review, lifecycle.busy, lifecycle.running]);
  // Hermes scopes loop bindings per function call, so a note reaches its updater only as this call's arguments.
  const note = (key: string, value: RefreshNote) => setNotes((current) => ({ ...current, [key]: value }));
  /** One exact-ID read per owner ref, in order: `only` refreshes that one item, none refreshes every listed item. */
  const refresh = async (only?: UnresolvedRef) => {
    if (active.current || !live.current) return;
    active.current = true; setRefreshing(only ? recoveryKey(only) : "all");
    const token = ++run.current, refs = only ? [only] : known.current.map((item) => item.ref);
    try {
      for (const ref of refs) {
        if (run.current !== token) break;
        const before = known.current.find((item) => recoveryKey(item.ref) === recoveryKey(ref));
        try {
          const result = await reconcile({ reviewId: ref.reviewId, agentId: ref.agentId });
          if (run.current !== token) break;
          onChanged([result]);
          note(recoveryKey(ref), refreshNote(before, result));
        } catch (failure) {
          if (run.current !== token) break;
          note(recoveryKey(ref), { at: Date.now(), outcome: "failed", title: titleOf(before), error: text(failure) });
        }
      }
    } finally {
      if (run.current === token) { await load(); if (run.current === token) { active.current = false; setRefreshing(null); } }
    }
  };
  const hidden = readHiddenArchives(hostId);
  // What still needs attention: every listed item the operator has not acknowledged. `items` keeps them all.
  const attention = items.filter((item) => !hidden.keys.includes(archiveAckKey(item)));
  return { items, attention, unresolved: items.map((item) => item.ref), error, checking: refreshing !== null, refreshing, notes, listed, complete, refresh, reload: load,
    line: recoveryLine(attention, error), hidden: hidden.keys, persistence: hidden.persistence,
    setHidden: (keys: readonly string[]) => { writeHiddenArchives(hostId, keys); setRevision((value) => value + 1); },
    clearNotes: () => setNotes({}) };
}
