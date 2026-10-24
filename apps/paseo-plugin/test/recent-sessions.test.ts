import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/** Records each hook call's subscribe and snapshot functions, and returns the snapshot as React would on render. */
const hook = vi.hoisted(() => ({ calls: [] as { subscribe: (listener: () => void) => () => void; read: () => unknown }[] }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useSyncExternalStore: (subscribe: (listener: () => void) => () => void, read: () => unknown) => { hook.calls.push({ subscribe, read }); return read(); } }));

import { clearRecent, configureRecentStorage, readRecent, recentRows, recordVisit, RECENT_KEPT, RECENT_SHOWN, recentStorageKey, readSavedEntries, readSavedHistory, subscribeRecent, useRecent } from "../client/recent-sessions";
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

  it("keeps one visit per session, newest first, at most 500, and only opaque IDs and times", () => {
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

  it("keeps more than 8 and more than 20 opens, all in open order", () => {
    configureRecentStorage(memory().storage);
    for (let index = 0; index < 25; index += 1) recordVisit("host-a", `agent-${index}`, NOW + index);
    const visits = readRecent("host-a").record.visits;
    expect(visits.map((visit) => visit.id)).toEqual(Array.from({ length: 25 }, (_, index) => `agent-${24 - index}`));
    expect(recentRows(visits.map((visit) => sessionEntry(visit.id)), readRecent("host-a").record, NOW)).toHaveLength(25);
    expect(RECENT_SHOWN).toBe(8);
  });

  it("trims only past the 500th distinct session, and a repeat open at the boundary drops nothing", () => {
    const { storage, values } = memory();
    configureRecentStorage(storage);
    for (let index = 0; index < RECENT_KEPT; index += 1) recordVisit("host-a", `agent-${index}`, NOW + index);
    expect(readRecent("host-a").record.visits).toHaveLength(500);
    recordVisit("host-a", "agent-0", NOW + 1000);
    let ids = readRecent("host-a").record.visits.map((visit) => visit.id);
    expect(ids).toHaveLength(500);
    expect(ids[0]).toBe("agent-0");
    expect(ids.at(-1)).toBe("agent-1");
    recordVisit("host-a", "agent-new", NOW + 2000);
    ids = readRecent("host-a").record.visits.map((visit) => visit.id);
    expect(ids).toHaveLength(500);
    expect(ids.slice(0, 2)).toEqual(["agent-new", "agent-0"]);
    expect(ids).not.toContain("agent-1");
    expect(JSON.parse(values.get(recentStorageKey("host-a"))!).visits).toHaveLength(500);
  });

  it("trims a stored record longer than 500 to its newest 500 on read", () => {
    const stored = Array.from({ length: 502 }, (_, index) => ({ id: `agent-${index}`, at: NOW - index }));
    const { storage, values } = memory({ [recentStorageKey("host-a")]: JSON.stringify({ clearedAt: null, visits: stored }) });
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits).toEqual(stored.slice(0, 500));
    expect(JSON.parse(values.get(recentStorageKey("host-a"))!).visits).toEqual(stored.slice(0, 500));
  });

  it("moves a repeated open to the top without a duplicate: A, B, A lists A, B", () => {
    configureRecentStorage(memory().storage);
    recordVisit("host-a", "agent-a", NOW);
    recordVisit("host-a", "agent-b", NOW + 1);
    recordVisit("host-a", "agent-a", NOW + 2);
    expect(readRecent("host-a").record.visits).toEqual([{ id: "agent-a", at: NOW + 2 }, { id: "agent-b", at: NOW + 1 }]);
  });

  it("never removes a visit for its age, on record, on read or after a reload", () => {
    const old = { id: "agent-old", at: NOW - 400 * DAY };
    const raw = JSON.stringify({ clearedAt: null, visits: [{ id: "agent-new", at: NOW }, old] });
    const { storage, values } = memory({ [recentStorageKey("host-a")]: raw });
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits).toEqual([{ id: "agent-new", at: NOW }, old]);
    expect(values.get(recentStorageKey("host-a"))).toBe(raw);
    vi.mocked(Date.now).mockReturnValue(NOW + 400 * DAY);
    expect(readRecent("host-a").record.visits).toHaveLength(2);
    recordVisit("host-a", "agent-later", NOW + 400 * DAY);
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["agent-later", "agent-new", "agent-old"]);
    expect(recentRows([sessionEntry("agent-old")], readRecent("host-a").record, NOW + 400 * DAY).map((row) => row.at)).toEqual([old.at]);
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
    expect(readSavedEntries(readRecent("host-a").record)).toEqual({ ids: [], unreadable: true });
    recordVisit("host-a", "agent-a", NOW);
    clearRecent("host-a", NOW);
    expect(values.get("kontor.recent@1")).toBe("{not json");
  });

  it("never lets legacy entries into verified visits, their order, counts or times", () => {
    const raw = JSON.stringify({ clearedAt: null, visits: [{ id: "legacy-late", at: NOW + 9000 }, { id: "shared", at: NOW + 8000 }, { id: "legacy-old", at: 1 }] });
    const { storage, values } = memory({ "kontor.recent@1": raw });
    configureRecentStorage(storage);
    recordVisit("host-a", "agent-a", NOW - 20);
    recordVisit("host-a", "shared", NOW - 10);
    const record = readRecent("host-a").record;
    expect(record.visits).toEqual([{ id: "shared", at: NOW - 10 }, { id: "agent-a", at: NOW - 20 }]);
    const entries = ["legacy-late", "shared", "legacy-old", "agent-a"].map((id) => sessionEntry(id));
    expect(recentRows(entries, record, NOW).map((row) => [row.entry.id, row.at])).toEqual([["shared", NOW - 10], ["agent-a", NOW - 20]]);
    expect(readSavedEntries(record)).toEqual({ ids: ["legacy-late", "legacy-old"], unreadable: false });
    expect(readRecent("host-b").record.visits).toEqual([]);
    expect(readSavedEntries(readRecent("host-b").record).ids).toEqual(["legacy-late", "shared", "legacy-old"]);
    expect(values.get("kontor.recent@1")).toBe(raw);
  });

  it("notifies subscribers on open, Clear and a new device store, never on a read", () => {
    configureRecentStorage(memory().storage);
    const listener = vi.fn();
    const unsubscribe = subscribeRecent(listener);
    readRecent("host-a");
    expect(listener).not.toHaveBeenCalled();
    recordVisit("host-a", "agent-a", NOW);
    clearRecent("host-a", NOW);
    configureRecentStorage(memory().storage);
    expect(listener).toHaveBeenCalledTimes(3);
    unsubscribe();
    recordVisit("host-a", "agent-b", NOW);
    expect(listener).toHaveBeenCalledTimes(3);
  });

  it("gives the hook this host's state, unchanged by reads and replaced after an open", () => {
    configureRecentStorage(memory().storage);
    hook.calls.length = 0;
    const first = useRecent("host-a");
    const { subscribe, read } = hook.calls[0]!;
    expect(subscribe).toBe(subscribeRecent);
    expect(read()).toBe(first);
    recordVisit("host-b", "agent-b", NOW);
    expect(read()).toBe(first);
    recordVisit("host-a", "agent-a", NOW);
    expect(read()).not.toBe(first);
    expect((read() as ReturnType<typeof readRecent>).record.visits).toEqual([{ id: "agent-a", at: NOW }]);
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

  it("lists an open of any age, and prompts still never add a row", () => {
    const entries = [prompted("agent-before", NOW - 2000), prompted("agent-after", NOW - 500), prompted("agent-stale", NOW - 15 * DAY)];
    const rows = recentRows(entries, record([{ id: "agent-stale", at: NOW - 15 * DAY }]), NOW);
    expect(rows.map((row) => [row.entry.id, row.at])).toEqual([["agent-stale", NOW - 15 * DAY]]);
  });

  it("keeps MRU order for tied times and ignores worker, creation and change timestamps", () => {
    const entries = ["agent-b", "agent-a", "never-opened"].map((id) => ({ ...sessionEntry(id, {
      status: "running", createdAt: new Date(NOW).toISOString(), updatedAt: new Date(NOW).toISOString(),
    }), lastActivityAt: new Date(NOW).toISOString() }));
    expect(recentRows(entries, record([{ id: "agent-b", at: NOW - 100 }, { id: "agent-a", at: NOW - 100 }]), NOW).map((row) => row.entry.id)).toEqual(["agent-b", "agent-a"]);
  });
});
