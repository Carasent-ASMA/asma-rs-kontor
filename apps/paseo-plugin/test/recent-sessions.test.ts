import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearRecent, configureRecentStorage, readRecent, recentRows, recordVisit, RECENT_KEPT } from "../client/recent-sessions";
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
    for (let index = 0; index < RECENT_KEPT + 3; index += 1) recordVisit(`agent-${index}`, NOW + index);
    recordVisit("agent-5", NOW + 100);
    const visits = readRecent().record.visits;
    expect(visits).toHaveLength(RECENT_KEPT);
    expect(visits[0]).toEqual({ id: "agent-5", at: NOW + 100 });
    expect(visits.filter((visit) => visit.id === "agent-5")).toHaveLength(1);
    expect(visits.map((visit) => visit.id)).not.toContain("agent-0");
    expect(Object.keys(JSON.parse(values.get("kontor.recent@1")!))).toEqual(["clearedAt", "visits"]);
    expect(readRecent().persistence).toBe("device");
  });

  it("drops visits older than 14 days when it records", () => {
    configureRecentStorage(memory().storage);
    recordVisit("agent-old", NOW - 15 * DAY);
    recordVisit("agent-new", NOW);
    expect(readRecent().record.visits.map((visit) => visit.id)).toEqual(["agent-new"]);
  });

  it("prunes expired visits from device storage on read and while the app stays open", () => {
    const { storage, values } = memory({ "kontor.recent@1": JSON.stringify({ clearedAt: null, visits: [
      { id: "agent-new", at: NOW }, { id: "agent-old", at: NOW - 15 * DAY },
    ] }) });
    configureRecentStorage(storage);
    expect(readRecent().record.visits).toEqual([{ id: "agent-new", at: NOW }]);
    expect(JSON.parse(values.get("kontor.recent@1")!).visits).toEqual([{ id: "agent-new", at: NOW }]);
    vi.mocked(Date.now).mockReturnValue(NOW + 15 * DAY);
    expect(readRecent().record.visits).toEqual([]);
    expect(JSON.parse(values.get("kontor.recent@1")!).visits).toEqual([]);
  });

  it("treats non-finite timestamps as unreadable and leaves the value untouched", () => {
    const raw = '{"clearedAt":null,"visits":[{"id":"agent-a","at":1e309}]}';
    const { storage, values } = memory({ "kontor.recent@1": raw });
    configureRecentStorage(storage);
    expect(readRecent().persistence).toBe("unreadable");
    recordVisit("agent-b", NOW);
    expect(values.get("kontor.recent@1")).toBe(raw);
  });

  it("reads a stored record back after a reload", () => {
    const { storage } = memory({ "kontor.recent@1": JSON.stringify({ clearedAt: null, visits: [{ id: "agent-a", at: NOW }] }) });
    configureRecentStorage(storage);
    expect(readRecent()).toEqual({ record: { clearedAt: null, visits: [{ id: "agent-a", at: NOW }] }, persistence: "device" });
  });

  it("keeps an unreadable stored value untouched until Clear replaces it", () => {
    const { storage, values } = memory({ "kontor.recent@1": "{not json" });
    configureRecentStorage(storage);
    expect(readRecent().persistence).toBe("unreadable");
    recordVisit("agent-a", NOW);
    expect(values.get("kontor.recent@1")).toBe("{not json");
    expect(readRecent()).toMatchObject({ persistence: "unreadable", record: { visits: [{ id: "agent-a", at: NOW }] } });
    expect(clearRecent(NOW)).toEqual({ record: { clearedAt: NOW, visits: [] }, persistence: "device" });
    expect(JSON.parse(values.get("kontor.recent@1")!)).toEqual({ clearedAt: NOW, visits: [] });
  });

  it("refuses a record with a malformed visit rather than half-reading it", () => {
    configureRecentStorage(memory({ "kontor.recent@1": JSON.stringify({ clearedAt: null, visits: [{ id: "agent-a", at: "yesterday" }] }) }).storage);
    expect(readRecent().persistence).toBe("unreadable");
  });

  it("says when a write failed or no device store exists", () => {
    configureRecentStorage({ read: () => null, write: () => { throw new Error("quota"); } });
    expect(recordVisit("agent-a", NOW)).toMatchObject({ persistence: "failed", record: { visits: [{ id: "agent-a" }] } });
    configureRecentStorage(null);
    expect(recordVisit("agent-a", NOW).persistence).toBe("session");
  });
});

describe("Recent rows", () => {
  const record = (visits: { id: string; at: number }[], clearedAt: number | null = null) => ({ clearedAt, visits });

  it("merges this device's opens with the host's prompts, newest reason wins, ties by ID", () => {
    const entries = [prompted("agent-b", NOW - 1000), prompted("agent-a", NOW - 1000), prompted("agent-c", NOW - 5000), sessionEntry("agent-d")];
    const rows = recentRows(entries, record([{ id: "agent-c", at: NOW - 10 }, { id: "agent-b", at: NOW - 9000 }]), NOW);
    expect(rows.map((row) => [row.entry.id, row.reason, row.at])).toEqual([
      ["agent-c", "Opened", NOW - 10],
      ["agent-a", "Prompted", NOW - 1000],
      ["agent-b", "Prompted", NOW - 1000],
    ]);
  });

  it("hides archived and absent sessions but keeps their visits", () => {
    const visits = record([{ id: "agent-archived", at: NOW }, { id: "agent-gone", at: NOW }, { id: "agent-live", at: NOW - 1 }]);
    const rows = recentRows([sessionEntry("agent-archived", { archivedAt: new Date(NOW).toISOString() }), sessionEntry("agent-live")], visits, NOW);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-live"]);
    expect(visits.visits).toHaveLength(3);
  });

  it("hides both opens and prompts at or before Clear and outside 14 days", () => {
    const entries = [prompted("agent-before", NOW - 2000), prompted("agent-after", NOW - 500), prompted("agent-stale", NOW - 15 * DAY)];
    const rows = recentRows(entries, record([{ id: "agent-before", at: NOW - 1500 }], NOW - 1000), NOW);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-after"]);
  });
});
