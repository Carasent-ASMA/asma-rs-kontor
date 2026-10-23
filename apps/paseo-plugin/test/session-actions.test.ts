import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureRecentStorage, readRecent, recentRows } from "../client/recent-sessions";
import { openSessionById, type SessionNavigation } from "../client/session-actions";
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

  it("persists A → B → A once each, newest open first, after a remount", () => {
    const now = Date.now(), values = new Map<string, string>();
    const storage = { read: (key: string) => values.get(key), write: (key: string, value: string | null) => { if (value !== null) values.set(key, value); } };
    configureRecentStorage(storage);
    const entries = [sessionEntry("agent-a"), sessionEntry("agent-b"), { ...sessionEntry("agent-c"), lastUserMessageAt: new Date(now + 100).toISOString() }];
    const nav = navigation();
    const clock = vi.spyOn(Date, "now");
    try {
      for (const [index, id] of ["agent-a", "agent-b", "agent-a"].entries()) {
        clock.mockReturnValue(now + index);
        expect(openSessionById({ entries, id, serverId: "srv_host", navigation: nav }).ok).toBe(true);
      }
      configureRecentStorage(storage);
      const record = readRecent("srv_host").record;
      expect(record.visits).toEqual([{ id: "agent-a", at: now + 2 }, { id: "agent-b", at: now + 1 }]);
      expect(recentRows(entries, record, now + 100).map((row) => row.entry.id)).toEqual(["agent-a", "agent-b"]);
      expect(readRecent("other-host").record.visits).toEqual([]);
    } finally {
      clock.mockRestore();
    }
  });
});
