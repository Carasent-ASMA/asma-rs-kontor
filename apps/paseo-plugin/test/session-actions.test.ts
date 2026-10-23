import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureRecentStorage, readRecent } from "../client/recent-sessions";
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
    expect(readRecent().record.visits.map((visit) => visit.id)).toEqual(["agent-second"]);
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
    expect(readRecent().record.visits).toEqual([]);
  });
});
