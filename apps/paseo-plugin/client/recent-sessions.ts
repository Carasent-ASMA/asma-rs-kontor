import { isArchivedSession, type DirectoryEntry } from "./session-directory";
import type { ProfileStorage } from "./viewer-profile";

/**
 * Recent records user opens accepted by native navigation from Kontor, per host and app storage profile.
 * Only opaque session IDs and times are stored; titles and places are read live from the directory.
 * shortcut: native sidebar views and other devices need a public host view-event/history API.
 */
export const RECENT_KEPT = 20;
export const RECENT_SHOWN = 8;
export const RECENT_DAYS = 14;
const DAY = 24 * 60 * 60 * 1000;
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

/** The Kontor screen module sets the device store when the plugin loads; null keeps Recent for this app session. */
export function configureRecentStorage(next: ProfileStorage | null): void {
  storage = next;
  states.clear();
}

const store = () => storage;

function parseRecord(value: unknown): RecentRecord | null {
  if (typeof value !== "string") return null;
  try {
    const parsed = JSON.parse(value) as { clearedAt?: unknown; visits?: unknown };
    const clearedAt = parsed.clearedAt ?? null;
    if ((clearedAt !== null && (typeof clearedAt !== "number" || !Number.isFinite(clearedAt))) || !Array.isArray(parsed.visits)) return null;
    const visits = parsed.visits.filter((visit): visit is RecentVisit => typeof visit?.id === "string" && typeof visit?.at === "number" && Number.isFinite(visit.at));
    return visits.length === parsed.visits.length ? { clearedAt, visits } : null;
  } catch {
    return null;
  }
}

function remember(hostId: string, next: RecentState): RecentState {
  states.set(hostId, next);
  return next;
}

/** Old opens and returns have no source or host. Preserve their bytes and expose IDs as saved history only. */
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

/** Retention applies on reads too, even if no new session is opened for weeks. */
export function readRecent(hostId: string, now = Date.now()): RecentState {
  const current = states.get(hostId) ?? loadRecent(hostId);
  const visits = current.record.visits.filter((visit) => visit.at > now - RECENT_DAYS * DAY).slice(0, RECENT_KEPT);
  if (visits.length === current.record.visits.length) return current;
  const record = { ...current.record, visits };
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

/** Most recent first, one per ID, at most 20 within 14 days. An unreadable stored value is never overwritten by a visit. */
export function recordVisit(hostId: string, id: string, now = Date.now()): RecentState {
  const current = readRecent(hostId, now);
  const visits = [{ id, at: now }, ...current.record.visits.filter((visit) => visit.id !== id && visit.at > now - RECENT_DAYS * DAY)].slice(0, RECENT_KEPT);
  const record = { ...current.record, visits };
  return current.persistence === "unreadable" ? remember(hostId, { record, persistence: "unreadable" }) : write(hostId, record);
}

/** Clears this host in this app profile. Legacy saved history stays untouched. */
export function clearRecent(hostId: string, now = Date.now()): RecentState {
  return write(hostId, { clearedAt: now, visits: [] });
}

/**
 * One row per live, non-archived directory session that was opened here within the window
 * and after the last Clear; newest first, ties by ID. A visited session absent from the directory is not shown, and
 * its visit is kept, so a partial read or an outage never erases it.
 */
export function recentRows(entries: readonly DirectoryEntry[], record: RecentRecord, now: number): RecentRow[] {
  const opened = new Map(record.visits.map((visit) => [visit.id, visit.at]));
  const floor = Math.max(now - RECENT_DAYS * DAY, record.clearedAt ?? Number.NEGATIVE_INFINITY);
  const rows: RecentRow[] = [];
  for (const entry of entries) {
    if (isArchivedSession(entry)) continue;
    const visit = opened.get(entry.id);
    if (visit !== undefined && visit > floor) rows.push({ entry, at: visit, reason: "Opened" });
  }
  return rows.sort((a, b) => b.at - a.at || (a.entry.id < b.entry.id ? -1 : a.entry.id > b.entry.id ? 1 : 0));
}
