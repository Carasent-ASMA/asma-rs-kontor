import { useSyncExternalStore } from "react";
import { isArchivedSession, type DirectoryEntry } from "./session-directory";
import type { ProfileStorage } from "./viewer-profile";

/**
 * Recent records user opens accepted by native navigation from Kontor, per host and app storage profile.
 * It keeps the last 500 distinct sessions opened, with no age limit; visits an older build trimmed are not recoverable.
 * Only opaque session IDs and times are stored; titles and places are read live from the directory.
 * shortcut: native sidebar views and other devices need a public host view-event/history API.
 */
export const RECENT_KEPT = 500;
export const RECENT_SHOWN = 8;
const LEGACY_KEY = "kontor.recent@1";
export const recentStorageKey = (hostId: string) => "kontor.recent@2 " + JSON.stringify([hostId]);

export type RecentVisit = { readonly id: string; readonly at: number };
export type RecentRecord = { readonly clearedAt: number | null; readonly visits: readonly RecentVisit[] };
/** `session`: no device store; `failed`: a write failed; `unreadable`: the stored value is kept untouched until Clear. */
export type RecentPersistence = "device" | "session" | "failed" | "unreadable";
export type RecentState = { readonly record: RecentRecord; readonly persistence: RecentPersistence };
export type RecentRow = { readonly entry: DirectoryEntry; readonly at: number; readonly reason: "Opened" };

const EMPTY: RecentRecord = { clearedAt: null, visits: [] };
let storage: ProfileStorage | null = null;
const states = new Map<string, RecentState>();
const listeners = new Set<() => void>();

/** Opens, Clear and a new device store notify; reads never do, so a render can read safely. */
export function subscribeRecent(listener: () => void): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function notify(): void {
  for (const listener of [...listeners]) listener();
}

/** This host's Recent, current after every open or Clear on this device. */
export function useRecent(hostId: string): RecentState {
  return useSyncExternalStore(subscribeRecent, () => readRecent(hostId));
}

/** The Kontor screen module sets the device store when the plugin loads; null keeps Recent for this app session. */
export function configureRecentStorage(next: ProfileStorage | null): void {
  storage = next;
  states.clear();
  notify();
}

const store = () => storage;

function parseRecord(value: unknown): RecentRecord | null {
  if (typeof value !== "string") return null;
  try {
    const parsed = JSON.parse(value) as { clearedAt?: unknown; visits?: unknown };
    const clearedAt = parsed.clearedAt ?? null;
    if ((clearedAt !== null && (typeof clearedAt !== "number" || !Number.isFinite(clearedAt))) || !Array.isArray(parsed.visits)) return null;
    const visits = parsed.visits.filter((visit): visit is RecentVisit => typeof visit?.id === "string" && typeof visit?.at === "number" && Number.isFinite(visit.at));
    if (visits.length !== parsed.visits.length) return null;
    // A repeated ID keeps its first, most recent, position, so a duplicate never takes one of the 500 kept places.
    const seen = new Set<string>();
    return {
      clearedAt,
      visits: visits.filter((visit) => {
        if (seen.has(visit.id)) return false;
        seen.add(visit.id);
        return true;
      }),
    };
  } catch {
    return null;
  }
}

function remember(hostId: string, next: RecentState): RecentState {
  states.set(hostId, next);
  return next;
}

/** Old opens and returns have no source or host. Preserve their bytes and expose IDs as saved entries only. */
export function readSavedHistory(): { readonly ids: readonly string[]; readonly unreadable: boolean } {
  try {
    const value = store()?.read(LEGACY_KEY);
    if (value === null || value === undefined) return { ids: [], unreadable: false };
    const record = parseRecord(value);
    return { ids: record ? [...new Set(record.visits.map((visit) => visit.id))] : [], unreadable: record === null };
  } catch {
    return { ids: [], unreadable: true };
  }
}

/**
 * Saved legacy IDs to list after a host's verified visits, in stored order. Their time and host are unknown, so
 * they carry neither, are never counted or ordered as visits, and an ID this host has a verified visit for is left out.
 */
export function readSavedEntries(record: RecentRecord): { readonly ids: readonly string[]; readonly unreadable: boolean } {
  const saved = readSavedHistory();
  const verified = new Set(record.visits.map((visit) => visit.id));
  return { ids: saved.ids.filter((id) => !verified.has(id)), unreadable: saved.unreadable };
}

function loadRecent(hostId: string): RecentState {
  const device = store();
  if (!device) return remember(hostId, { record: EMPTY, persistence: "session" });
  try {
    const value = device.read(recentStorageKey(hostId));
    if (value === null || value === undefined) return remember(hostId, { record: EMPTY, persistence: "device" });
    const record = parseRecord(value);
    if (record) return remember(hostId, { record, persistence: "device" });
  } catch {
    // Unreadable, like a value this version does not accept.
  }
  return remember(hostId, { record: EMPTY, persistence: "unreadable" });
}

/**
 * The stored sequence, trimmed only past the newest 500 sessions; age never removes a visit.
 * The clock argument is unused and stays only for existing callers.
 */
export function readRecent(hostId: string, _now?: number): RecentState {
  const current = states.get(hostId) ?? loadRecent(hostId);
  if (current.record.visits.length <= RECENT_KEPT) return current;
  const record = { ...current.record, visits: current.record.visits.slice(0, RECENT_KEPT) };
  return current.persistence === "unreadable" ? remember(hostId, { record, persistence: "unreadable" }) : write(hostId, record);
}

function write(hostId: string, record: RecentRecord): RecentState {
  const device = store();
  if (!device) return remember(hostId, { record, persistence: "session" });
  try {
    device.write(recentStorageKey(hostId), JSON.stringify(record));
    return remember(hostId, { record, persistence: "device" });
  } catch {
    return remember(hostId, { record, persistence: "failed" });
  }
}

/** Most recent first, one per ID, at most 500. An unreadable stored value is never overwritten by a visit. */
export function recordVisit(hostId: string, id: string, now = Date.now()): RecentState {
  const current = readRecent(hostId);
  const visits = [{ id, at: now }, ...current.record.visits.filter((visit) => visit.id !== id)].slice(0, RECENT_KEPT);
  const record = { ...current.record, visits };
  const next = current.persistence === "unreadable" ? remember(hostId, { record, persistence: "unreadable" }) : write(hostId, record);
  notify();
  return next;
}

/** Clears this host in this app profile. Legacy saved entries stay untouched. */
export function clearRecent(hostId: string, now = Date.now()): RecentState {
  const next = write(hostId, { clearedAt: now, visits: [] });
  notify();
  return next;
}

/**
 * Follow the recorded MRU sequence; wall times are shown as age, never used for open order.
 * A visited session absent from the directory is not shown, and its visit is kept,
 * so a partial read or an outage never erases it. Clear already empties the sequence.
 * The clock argument is unused and stays only for existing callers.
 */
export function recentRows(entries: readonly DirectoryEntry[], record: RecentRecord, _now?: number): RecentRow[] {
  const available = new Map(entries.map((entry) => [entry.id, entry]));
  const rows: RecentRow[] = [];
  for (const { id, at } of record.visits) {
    const entry = available.get(id);
    if (!entry || isArchivedSession(entry)) continue;
    rows.push({ entry, at, reason: "Opened" });
    available.delete(id);
  }
  return rows;
}
