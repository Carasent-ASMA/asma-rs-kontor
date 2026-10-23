import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearRecent, configureRecentStorage, readRecent, recentRows, recordVisit, RECENT_KEPT, recentStorageKey, readSavedHistory } from "../client/recent-sessions";
import type { ProfileStorage } from "../client/viewer-profile";
import { sessionEntry } from "./session-fixtures";

const DAY = 24 * 60 * 60 * 1000;
const NOW = Date.parse("2026-10-22T12:00:00.000Z");

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  const storage: ProfileStorage = { read: (key) => values.get(key) ?? null, write: (key, value) => { if (value === null) values.delete(key); else values.set(key, value); } };
  return { storage, values };
}
const prompted = (id: string, at: number, overrides: Parameters<typeof sessionEntry>[1] = {}) => ({ ...sessionEntry(id, overrides), lastUserMessageAt: new Date(at).toISOString() });

describe("Recent visit store", () => {
  beforeEach(() => { configureRecentStorage(null); vi.spyOn(Date, "now").mockReturnValue(NOW); });
  afterEach(() => vi.restoreAllMocks());

  it("keeps one visit per session, newest first, at most 20, and only opaque IDs and times", () => {
    const { storage, values } = memory();
    configureRecentStorage(storage);
    for (let index = 0; index < RECENT_KEPT + 3; index += 1) recordVisit("host-a", `agent-${index}`, NOW + index);
    recordVisit("host-a", "agent-5", NOW + 100);
    const visits = readRecent("host-a").record.visits;
    expect(visits).toHaveLength(RECENT_KEPT);
    expect(visits[0]).toEqual({ id: "agent-5", at: NOW + 100 });
    expect(visits.filter((visit) => visit.id === "agent-5")).toHaveLength(1);
    expect(visits.map((visit) => visit.id)).not.toContain("agent-0");
    expect(Object.keys(JSON.parse(values.get(recentStorageKey("host-a"))!))).toEqual(["clearedAt", "visits"]);
    expect(readRecent("host-a").persistence).toBe("device");
  });

  it("drops visits older than 14 days when it records", () => {
    configureRecentStorage(memory().storage);
    recordVisit("host-a", "agent-old", NOW - 15 * DAY);
    recordVisit("host-a", "agent-new", NOW);
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["agent-new"]);
  });

  it("prunes expired visits from device storage on read and while the app stays open", () => {
    const { storage, values } = memory({ [recentStorageKey("host-a")]: JSON.stringify({ clearedAt: null, visits: [
      { id: "agent-new", at: NOW }, { id: "agent-old", at: NOW - 15 * DAY },
    ] }) });
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits).toEqual([{ id: "agent-new", at: NOW }]);
    expect(JSON.parse(values.get(recentStorageKey("host-a"))!).visits).toEqual([{ id: "agent-new", at: NOW }]);
    vi.mocked(Date.now).mockReturnValue(NOW + 15 * DAY);
    expect(readRecent("host-a").record.visits).toEqual([]);
    expect(JSON.parse(values.get(recentStorageKey("host-a"))!).visits).toEqual([]);
  });

  it("treats non-finite timestamps as unreadable and leaves the value untouched", () => {
    const raw = '{"clearedAt":null,"visits":[{"id":"agent-a","at":1e309}]}';
    const { storage, values } = memory({ [recentStorageKey("host-a")]: raw });
    configureRecentStorage(storage);
    expect(readRecent("host-a").persistence).toBe("unreadable");
    recordVisit("host-a", "agent-b", NOW);
    expect(values.get(recentStorageKey("host-a"))).toBe(raw);
  });

  it("reads a stored record back after a reload", () => {
    const { storage } = memory({ [recentStorageKey("host-a")]: JSON.stringify({ clearedAt: null, visits: [{ id: "agent-a", at: NOW }] }) });
    configureRecentStorage(storage);
    expect(readRecent("host-a")).toEqual({ record: { clearedAt: null, visits: [{ id: "agent-a", at: NOW }] }, persistence: "device" });
  });

  it("keeps an unreadable stored value untouched until Clear replaces it", () => {
    const { storage, values } = memory({ [recentStorageKey("host-a")]: "{not json" });
    configureRecentStorage(storage);
    expect(readRecent("host-a").persistence).toBe("unreadable");
    recordVisit("host-a", "agent-a", NOW);
    expect(values.get(recentStorageKey("host-a"))).toBe("{not json");
    expect(readRecent("host-a")).toMatchObject({ persistence: "unreadable", record: { visits: [{ id: "agent-a", at: NOW }] } });
    expect(clearRecent("host-a", NOW)).toEqual({ record: { clearedAt: NOW, visits: [] }, persistence: "device" });
    expect(JSON.parse(values.get(recentStorageKey("host-a"))!)).toEqual({ clearedAt: NOW, visits: [] });
  });

  it("refuses a record with a malformed visit rather than half-reading it", () => {
    configureRecentStorage(memory({ [recentStorageKey("host-a")]: JSON.stringify({ clearedAt: null, visits: [{ id: "agent-a", at: "yesterday" }] }) }).storage);
    expect(readRecent("host-a").persistence).toBe("unreadable");
  });

  it("says when a write failed or no device store exists", () => {
    configureRecentStorage({ read: () => null, write: () => { throw new Error("quota"); } });
    expect(recordVisit("host-a", "agent-a", NOW)).toMatchObject({ persistence: "failed", record: { visits: [{ id: "agent-a" }] } });
    configureRecentStorage(null);
    expect(recordVisit("host-a", "agent-a", NOW).persistence).toBe("session");
  });

  it("isolates hosts with identical session IDs across writes, clear, and reload", () => {
    const { storage, values } = memory();
    configureRecentStorage(storage);
    recordVisit("host-a", "same-id", NOW - 10);
    recordVisit("host-b", "same-id", NOW - 5);
    clearRecent("host-a", NOW);
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits).toEqual([]);
    expect(readRecent("host-b").record.visits).toEqual([{ id: "same-id", at: NOW - 5 }]);
    expect(values.has(recentStorageKey("host-a"))).toBe(true);
    expect(values.has(recentStorageKey("host-b"))).toBe(true);
  });

  it("keeps app storage profiles separate without using the Working as identity", () => {
    const first = memory().storage, second = memory().storage;
    configureRecentStorage(first);
    recordVisit("host-a", "agent-a", NOW);
    configureRecentStorage(second);
    expect(readRecent("host-a").record.visits).toEqual([]);
    recordVisit("host-a", "agent-b", NOW);
    configureRecentStorage(first);
    expect(readRecent("host-a").record.visits).toEqual([{ id: "agent-a", at: NOW }]);
  });

  it("preserves mixed legacy history byte-for-byte without importing its times or host", () => {
    const raw = ' {"clearedAt":null,"visits":[{"id":"legacy-a","at":1},{"id":"legacy-b","at":2}]} ';
    const { storage, values } = memory({ "kontor.recent@1": raw });
    configureRecentStorage(storage);
    expect(readSavedHistory()).toEqual({ ids: ["legacy-a", "legacy-b"], unreadable: false });
    for (const host of ["host-a", "host-b"]) {
      expect(readRecent(host).record.visits).toEqual([]);
      recordVisit(host, "new-open", NOW);
      clearRecent(host, NOW);
    }
    configureRecentStorage(storage);
    expect(readSavedHistory().ids).toEqual(["legacy-a", "legacy-b"]);
    expect(values.get("kontor.recent@1")).toBe(raw);
  });

  it("keeps unreadable legacy bytes untouched even after clearing new history", () => {
    const { storage, values } = memory({ "kontor.recent@1": "{not json" });
    configureRecentStorage(storage);
    expect(readSavedHistory()).toEqual({ ids: [], unreadable: true });
    clearRecent("host-a", NOW);
    expect(values.get("kontor.recent@1")).toBe("{not json");
  });

  it("shows an open after Clear even when its wall time is earlier or equal", () => {
    for (const at of [NOW - 1, NOW]) {
      const { storage } = memory();
      configureRecentStorage(storage);
      recordVisit("host-a", "before-clear", NOW - 10);
      clearRecent("host-a", NOW);
      recordVisit("host-a", "after-clear", at);
      configureRecentStorage(storage);
      const record = readRecent("host-a", NOW).record;
      expect(record.visits).toEqual([{ id: "after-clear", at }]);
      expect(recentRows([sessionEntry("before-clear"), sessionEntry("after-clear")], record, NOW).map((row) => row.entry.id)).toEqual(["after-clear"]);
    }
  });
});

describe("Recent rows", () => {
  const record = (visits: { id: string; at: number }[], clearedAt: number | null = null) => ({ clearedAt, visits });

  it("orders opens only; a prompt never introduces a session or changes its place", () => {
    const entries = [prompted("agent-b", NOW - 1000), prompted("agent-a", NOW - 1000), prompted("agent-c", NOW - 5000), sessionEntry("agent-d")];
    const rows = recentRows(entries, record([{ id: "agent-c", at: NOW - 10 }, { id: "agent-b", at: NOW - 9000 }]), NOW);
    expect(rows.map((row) => [row.entry.id, row.reason, row.at])).toEqual([
      ["agent-c", "Opened", NOW - 10],
      ["agent-b", "Opened", NOW - 9000],
    ]);
  });

  it("hides archived and absent sessions but keeps their visits", () => {
    const visits = record([{ id: "agent-archived", at: NOW }, { id: "agent-gone", at: NOW }, { id: "agent-live", at: NOW - 1 }]);
    const rows = recentRows([sessionEntry("agent-archived", { archivedAt: new Date(NOW).toISOString() }), sessionEntry("agent-live")], visits, NOW);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-live"]);
    expect(visits.visits).toHaveLength(3);
  });

  it("hides expired opens, regardless of prompts", () => {
    const entries = [prompted("agent-before", NOW - 2000), prompted("agent-after", NOW - 500), prompted("agent-stale", NOW - 15 * DAY)];
    const rows = recentRows(entries, record([{ id: "agent-stale", at: NOW - 15 * DAY }]), NOW);
    expect(rows).toEqual([]);
  });

  it("keeps MRU order for tied times and ignores worker, creation and change timestamps", () => {
    const entries = ["agent-b", "agent-a", "never-opened"].map((id) => ({ ...sessionEntry(id, {
      status: "running", createdAt: new Date(NOW).toISOString(), updatedAt: new Date(NOW).toISOString(),
    }), lastActivityAt: new Date(NOW).toISOString() }));
    expect(recentRows(entries, record([{ id: "agent-b", at: NOW - 100 }, { id: "agent-a", at: NOW - 100 }]), NOW).map((row) => row.entry.id)).toEqual(["agent-b", "agent-a"]);
  });
});
