import { isArchivedSession, type DirectoryEntry } from "./session-directory";
import { parseTime } from "./session-time";
import type { ProfileStorage } from "./viewer-profile";

/**
 * Recent: sessions opened through Kontor on this device, merged with the host's `lastUserMessageAt` ("prompted",
 * from any device). Paseo keeps no visit history a plugin can read, so Kontor records its own. Only opaque session
 * IDs and times are stored; titles and places are read live from the directory.
 */
export const RECENT_KEPT = 20;
export const RECENT_SHOWN = 8;
export const RECENT_DAYS = 14;
const DAY = 24 * 60 * 60 * 1000;
const KEY = "kontor.recent@1";

export type RecentVisit = { readonly id: string; readonly at: number };
export type RecentRecord = { readonly clearedAt: number | null; readonly visits: readonly RecentVisit[] };
/** `session`: no device store; `failed`: a write failed; `unreadable`: the stored value is kept untouched until Clear. */
export type RecentPersistence = "device" | "session" | "failed" | "unreadable";
export type RecentState = { readonly record: RecentRecord; readonly persistence: RecentPersistence };
export type RecentRow = { readonly entry: DirectoryEntry; readonly at: number; readonly reason: "Opened" | "Prompted" };

const EMPTY: RecentRecord = { clearedAt: null, visits: [] };
let storage: ProfileStorage | null = null;
let state: RecentState | undefined;

/** The Kontor screen module sets the device store when the plugin loads; null keeps Recent for this app session. */
export function configureRecentStorage(next: ProfileStorage | null): void {
  storage = next;
  state = undefined;
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

function loadRecent(): RecentState {
  const device = store();
  if (!device) return (state = { record: EMPTY, persistence: "session" });
  try {
    const value = device.read(KEY);
    if (value === null || value === undefined) return (state = { record: EMPTY, persistence: "device" });
    const record = parseRecord(value);
    if (record) return (state = { record, persistence: "device" });
  } catch {
    // Unreadable, like a value this version does not accept.
  }
  return (state = { record: EMPTY, persistence: "unreadable" });
}

/** Retention applies on reads too, even if no new session is opened for weeks. */
export function readRecent(now = Date.now()): RecentState {
  const current = state ?? loadRecent();
  const visits = current.record.visits.filter((visit) => visit.at > now - RECENT_DAYS * DAY).slice(0, RECENT_KEPT);
  if (visits.length === current.record.visits.length) return current;
  const record = { ...current.record, visits };
  return current.persistence === "unreadable" ? (state = { record, persistence: "unreadable" }) : write(record);
}

function write(record: RecentRecord): RecentState {
  const device = store();
  if (!device) return (state = { record, persistence: "session" });
  try {
    device.write(KEY, JSON.stringify(record));
    return (state = { record, persistence: "device" });
  } catch {
    return (state = { record, persistence: "failed" });
  }
}

/** Most recent first, one per ID, at most 20 within 14 days. An unreadable stored value is never overwritten by a visit. */
export function recordVisit(id: string, now = Date.now()): RecentState {
  const current = readRecent(now);
  const visits = [{ id, at: now }, ...current.record.visits.filter((visit) => visit.id !== id && visit.at > now - RECENT_DAYS * DAY)].slice(0, RECENT_KEPT);
  const record = { ...current.record, visits };
  return current.persistence === "unreadable" ? (state = { record, persistence: "unreadable" }) : write(record);
}

/** Empties the visits and hides prompts up to now, on this device only. Replaces an unreadable value. */
export function clearRecent(now = Date.now()): RecentState {
  return write({ clearedAt: now, visits: [] });
}

/**
 * One row per live, non-archived directory session that was opened here or prompted on the host within the window
 * and after the last Clear; newest first, ties by ID. A visited session absent from the directory is not shown, and
 * its visit is kept, so a partial read or an outage never erases it.
 */
export function recentRows(entries: readonly DirectoryEntry[], record: RecentRecord, now: number): RecentRow[] {
  const opened = new Map(record.visits.map((visit) => [visit.id, visit.at]));
  const floor = Math.max(now - RECENT_DAYS * DAY, record.clearedAt ?? Number.NEGATIVE_INFINITY);
  const rows: RecentRow[] = [];
  for (const entry of entries) {
    if (isArchivedSession(entry)) continue;
    const visit = opened.get(entry.id) ?? null, prompt = parseTime(entry.lastUserMessageAt);
    const candidates: RecentRow[] = [];
    if (visit !== null && visit > floor) candidates.push({ entry, at: visit, reason: "Opened" });
    if (prompt !== null && prompt > floor) candidates.push({ entry, at: prompt, reason: "Prompted" });
    const newest = candidates.sort((a, b) => b.at - a.at)[0];
    if (newest) rows.push(newest);
  }
  return rows.sort((a, b) => b.at - a.at || (a.entry.id < b.entry.id ? -1 : a.entry.id > b.entry.id ? 1 : 0));
}
