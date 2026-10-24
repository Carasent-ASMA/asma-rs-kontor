import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureRecentStorage, readRecent, recentRows } from "../client/recent-sessions";
import { archivedByRead, openCreatedSession, openRecoveredSession, openSessionById, type SessionNavigation } from "../client/session-actions";
import type { CreateOutcome } from "../shared/session-create";
import type { LifecycleRow } from "../shared/session-lifecycle";
import { sessionEntry } from "./session-fixtures";

function navigation(): SessionNavigation & { openAgent: ReturnType<typeof vi.fn> } {
  return {
    openAgent: vi.fn(),
    openWorkspace: vi.fn(),
  } as unknown as SessionNavigation & { openAgent: ReturnType<typeof vi.fn> };
}

describe("openSessionById", () => {
  beforeEach(() => configureRecentStorage(null));

  it("opens by native ID and leaves duplicate titles distinct", () => {
    const first = sessionEntry("agent-first", { title: "Same title" });
    const second = sessionEntry("agent-second", { title: "Same title" });
    const nav = navigation();

    const result = openSessionById({
      entries: [first, second],
      id: second.id,
      serverId: "srv_host",
      navigation: nav,
    });

    expect(result).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledTimes(1);
    expect(nav.openAgent).toHaveBeenCalledWith({
      agentId: "agent-second",
      serverId: "srv_host",
    });
    expect(nav.openAgent).not.toHaveBeenCalledWith(
      expect.objectContaining({ agentId: "agent-first" }),
    );
    expect(readRecent("srv_host").record.visits.map((visit) => visit.id)).toEqual(["agent-second"]);
  });

  it("refuses a session that disappeared instead of falling back to a name", () => {
    const nav = navigation();
    const result = openSessionById({
      entries: [sessionEntry("agent-first", { title: "Same title" })],
      id: "agent-gone",
      serverId: "srv_host",
      navigation: nav,
    });

    expect(result).toMatchObject({ ok: false, reason: "missing" });
    expect(nav.openAgent).not.toHaveBeenCalled();
  });

  it("reports an older host without native navigation", () => {
    const result = openSessionById({
      entries: [sessionEntry("agent-first")],
      id: "agent-first",
      serverId: "srv_host",
      navigation: undefined,
    });

    expect(result).toMatchObject({ ok: false, reason: "navigation" });
  });

  it("returns the host failure without opening another session", () => {
    const nav = navigation();
    nav.openAgent.mockImplementation(() => {
      throw new Error("workspace is unavailable");
    });
    const result = openSessionById({
      entries: [sessionEntry("agent-first")],
      id: "agent-first",
      serverId: "srv_host",
      navigation: nav,
    });

    expect(result).toMatchObject({
      ok: false,
      reason: "failed",
      message: "workspace is unavailable",
    });
    expect(nav.openAgent).toHaveBeenCalledTimes(1);
  });

  it("records a visit only when the host opened the session", () => {
    const entries = [sessionEntry("agent-first")];
    openSessionById({ entries, id: "agent-gone", serverId: "srv_host", navigation: navigation() });
    openSessionById({ entries, id: "agent-first", serverId: "srv_host", navigation: undefined });
    const failing = navigation();
    failing.openAgent.mockImplementation(() => { throw new Error("workspace is unavailable"); });
    openSessionById({ entries, id: "agent-first", serverId: "srv_host", navigation: failing });
    expect(readRecent("srv_host").record.visits).toEqual([]);
  });

  it.each([
    { sequence: "A → B → A", opens: [{ id: "agent-a", at: 0 }, { id: "agent-b", at: 1 }, { id: "agent-a", at: 2 }], expected: [{ id: "agent-a", at: 2 }, { id: "agent-b", at: 1 }] },
    { sequence: "B → A → B with equal wall times", opens: [{ id: "agent-b", at: 0 }, { id: "agent-a", at: 0 }, { id: "agent-b", at: 0 }], expected: [{ id: "agent-b", at: 0 }, { id: "agent-a", at: 0 }] },
    { sequence: "A → B with the clock moving backwards", opens: [{ id: "agent-a", at: 0 }, { id: "agent-b", at: -1 }], expected: [{ id: "agent-b", at: -1 }, { id: "agent-a", at: 0 }] },
  ])("persists $sequence once each, newest open first, after a remount", ({ opens, expected }) => {
    const now = Date.now(), values = new Map<string, string>();
    const storage = { read: (key: string) => values.get(key), write: (key: string, value: string | null) => { if (value !== null) values.set(key, value); } };
    configureRecentStorage(storage);
    const entries = [sessionEntry("agent-a"), sessionEntry("agent-b"), { ...sessionEntry("agent-c"), lastUserMessageAt: new Date(now + 100).toISOString() }];
    const nav = navigation();
    const clock = vi.spyOn(Date, "now");
    try {
      for (const { id, at } of opens) {
        clock.mockReturnValue(now + at);
        expect(openSessionById({ entries, id, serverId: "srv_host", navigation: nav }).ok).toBe(true);
      }
      configureRecentStorage(storage);
      const record = readRecent("srv_host").record;
      expect(record.visits).toEqual(expected.map(({ id, at }) => ({ id, at: now + at })));
      expect(recentRows(entries, record, now + 100).map((row) => row.entry.id)).toEqual(expected.map(({ id }) => id));
      expect(readRecent("other-host").record.visits).toEqual([]);
    } finally {
      clock.mockRestore();
    }
  });
});

describe("openCreatedSession", () => {
  beforeEach(() => configureRecentStorage(null));
  const created = (patch: Partial<CreateOutcome> = {}): CreateOutcome => ({ requestId: "00000000-0000-4000-8000-000000000001", state: "verified", title: "LSA • ADHOC • Fixture",
    agentId: "created-exact-id", workspaceId: "wks-adam", message: "Created and verified.", differences: [], persona: "supported", checkedAt: "2026-10-23", ...patch });

  it("opens the successful readback's exact host/native ID without needing a directory entry", () => {
    const nav = navigation();
    expect(openCreatedSession({ outcome: created(), serverId: "host-a", navigation: nav })).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "created-exact-id", serverId: "host-a" });
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["created-exact-id"]);
    expect(readRecent("host-b").record.visits).toEqual([]);
  });

  it.each([
    { state: "unknown", agentId: "unconfirmed-id" },
    { state: "refused", agentId: "unconfirmed-id" },
    { state: "verified", agentId: null },
  ] as const)("never opens or records an unqualified outcome $state/$agentId", (patch) => {
    const nav = navigation();
    expect(openCreatedSession({ outcome: created(patch), serverId: "host-a", navigation: nav })).toMatchObject({ ok: false, reason: "missing" });
    expect(nav.openAgent).not.toHaveBeenCalled();
    expect(readRecent("host-a").record.visits).toEqual([]);
  });

  it("does not invent a recent visit when navigation is absent or throws", () => {
    const nav = navigation();
    nav.openAgent.mockImplementation(() => { throw new Error("host navigation failed"); });
    expect(openCreatedSession({ outcome: created(), serverId: "host-a", navigation: undefined })).toMatchObject({ ok: false, reason: "navigation" });
    expect(openCreatedSession({ outcome: created(), serverId: "host-a", navigation: nav })).toMatchObject({ ok: false, reason: "failed", message: "host navigation failed" });
    expect(readRecent("host-a").record.visits).toEqual([]);
  });

  it("shares the existing persistent A-B-A MRU path with ordinary opens", () => {
    const values = new Map<string, string>();
    const storage = { read: (key: string) => values.get(key), write: (key: string, value: string | null) => { if (value !== null) values.set(key, value); } };
    configureRecentStorage(storage);
    const nav = navigation();
    openSessionById({ entries: [sessionEntry("a")], id: "a", serverId: "host-a", navigation: nav });
    openCreatedSession({ outcome: created({ agentId: "b" }), serverId: "host-a", navigation: nav });
    openCreatedSession({ outcome: created({ agentId: "a" }), serverId: "host-a", navigation: nav });
    configureRecentStorage(storage);
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["a", "b"]);
  });
});

describe("openRecoveredSession", () => {
  beforeEach(() => configureRecentStorage(null));
  const recovered = (availability: Partial<NonNullable<LifecycleRow["availability"]>> | null = {}, id = "archived-exact-id") => ({ id, title: "SWE • DEMO-4 • Fixture", outcome: "archived", runtime: "unconfirmed",
    availability: availability && { agentId: id, state: "readable", archivedAt: "2026-10-24T08:00:00.000Z", reason: "Fixture read.", checkedAt: "2026-10-24T08:01:00.000Z", revision: "r1", ...availability } }) as LifecycleRow;

  it("opens an uncertain archive whose exact-ID read does not show it archived, after its row left the directory", () => {
    const nav = navigation();
    expect(openRecoveredSession({ row: recovered({ archivedAt: null }), id: "archived-exact-id", serverId: "host-a", navigation: nav })).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "archived-exact-id", serverId: "host-a" });
  });

  it("AR-01 never opens a session its exact-ID read shows archived: Paseo would ask to restore its workspace", () => {
    const nav = navigation();
    expect(openRecoveredSession({ row: recovered(), id: "archived-exact-id", serverId: "host-a", navigation: nav }))
      .toEqual({ ok: false, reason: "archived", message: expect.stringContaining("Refresh status settles its archive") });
    expect(nav.openAgent).not.toHaveBeenCalled(); expect(readRecent("host-a").record.visits).toEqual([]);
    expect(archivedByRead(recovered(), "archived-exact-id")).toBe(true);
    expect(archivedByRead(recovered({ archivedAt: null }), "archived-exact-id")).toBe(false);
    expect(archivedByRead(recovered({ agentId: "other-id" }), "archived-exact-id")).toBe(false);
    expect(archivedByRead(recovered({ state: "unknown" }), "archived-exact-id")).toBe(false);
  });

  it("AR-01 leaves Archived-list opening by exact ID unchanged", () => {
    const nav = navigation(), entry = sessionEntry("archived-exact-id", { archivedAt: "2026-10-24T08:00:00.000Z" });
    expect(openSessionById({ entries: [entry], id: entry.id, serverId: "host-a", navigation: nav })).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "archived-exact-id", serverId: "host-a" });
  });

  it.each([
    ["no row", null, "archived-exact-id"],
    ["another row", recovered({}, "other-id"), "archived-exact-id"],
    ["a read naming another ID", recovered({ agentId: "other-id" }), "archived-exact-id"],
    ["an unknown read", recovered({ state: "unknown" }), "archived-exact-id"],
    ["a missing session", recovered({ state: "missing" }), "archived-exact-id"],
    ["no read", recovered(null), "archived-exact-id"],
  ] as const)("never opens or records %s", (_, row, id) => {
    const nav = navigation();
    expect(openRecoveredSession({ row, id, serverId: "host-a", navigation: nav })).toMatchObject({ ok: false, reason: "missing" });
    expect(nav.openAgent).not.toHaveBeenCalled(); expect(readRecent("host-a").record.visits).toEqual([]);
  });
});
