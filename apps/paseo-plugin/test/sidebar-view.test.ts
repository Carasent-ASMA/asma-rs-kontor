import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => [], useAgents: () => [], useWorkspaces: () => [] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ useToast: () => ({ error: () => {}, show: () => {} }) }));
vi.mock("../client/control-help", () => ({ ControlHelpProvider: "ControlHelpProvider" }));
vi.mock("../client/start-switch-sheet", () => ({ useStartSwitch: () => ({ header: null, body: null, footer: null }) }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: { entries: [] } }) }));
vi.mock("../client/recent-sessions", () => ({ recordVisit: () => {} }));

import { handOffOpen, openHandOff } from "../client/session-actions";
import { openKontor, screenNavigation, sidebarHost } from "../client/sidebar-view";

const NAVIGATION = () => ({ openAgent: vi.fn(), openWorkspace: vi.fn(), openTerminal: vi.fn(), openProject: vi.fn() });
const REF = <Kind extends string>(kind: Kind, hostId: string, id: string) => ({ kind, key: JSON.stringify([hostId, id]), hostId, id });
const HOSTS = [
  { serverId: "srv_a", label: "MAIN", status: "offline" as const },
  { serverId: "srv_b", label: "TEST", status: "online" as const },
];
const REFS = {
  agents: new Map([["agt_1", REF("agent", "srv_a", "agt_1")], ["agt_2", REF("agent", "srv_b", "agt_2")]]),
  workspaces: new Map([["wks_1", REF("workspace", "srv_a", "wks_1")]]),
};

describe("Kontor sidebar view", () => {
  it("navigates with the refs Paseo's own client state supplies, never a rebuilt key", () => {
    const navigation = NAVIGATION();
    const open = screenNavigation("srv_a", navigation, REFS);
    open.openAgent({ agentId: "agt_1" });
    open.openAgent({ agentId: "agt_2", serverId: "srv_b" });
    open.openWorkspace({ workspaceId: "wks_1" });
    expect(navigation.openAgent.mock.calls.map(([ref]) => ref)).toEqual([REFS.agents.get("agt_1"), REFS.agents.get("agt_2")]);
    expect(navigation.openWorkspace).toHaveBeenCalledWith(REFS.workspaces.get("wks_1"));
  });

  it("refuses a row whose ref Paseo no longer lists, without navigating", () => {
    const navigation = NAVIGATION();
    const open = screenNavigation("srv_a", navigation, REFS);
    expect(() => open.openAgent({ agentId: "agt_gone" })).toThrow("no longer listed on this host");
    expect(() => open.openAgent({ agentId: "agt_2" })).toThrow("no longer listed on this host");
    expect(() => open.openWorkspace({ workspaceId: "wks_gone" })).toThrow("no longer listed on this host");
    expect(navigation.openAgent).not.toHaveBeenCalled();
    expect(navigation.openWorkspace).not.toHaveBeenCalled();
  });

  it("lists the active session's host first, then the active workspace's, then the first online host", () => {
    expect(sidebarHost({ agent: REF("agent", "srv_a", "agt_1"), workspace: null }, HOSTS)).toEqual({ hostId: "srv_a", label: "MAIN", status: "offline" });
    expect(sidebarHost({ agent: null, workspace: REF("workspace", "srv_a", "wks_1") }, HOSTS))
      .toEqual({ hostId: "srv_a", label: "MAIN", status: "offline" });
    expect(sidebarHost({ agent: null, workspace: null }, HOSTS)).toEqual({ hostId: "srv_b", label: "TEST", status: "online" });
    expect(sidebarHost({ agent: null, workspace: null }, [{ ...HOSTS[0]! }, { ...HOSTS[1]!, status: "offline" }]))
      .toEqual({ hostId: "srv_a", label: "MAIN", status: "offline" });
    expect(sidebarHost({ agent: null, workspace: null }, [])).toBeNull();
  });

  it("names a connected host it has no summary for by its id, so the line never claims the wrong host", () => {
    expect(sidebarHost({ agent: REF("agent", "srv_c", "agt_1"), workspace: null }, HOSTS)).toEqual({ hostId: "srv_c", label: "srv_c", status: "offline" });
  });

  it("hands a view to Kontor's screen by its one-shot key", () => {
    const openScreen = vi.fn();
    expect(openKontor(openScreen, "sessions", "key-1")).toBeNull();
    expect(openScreen).toHaveBeenCalledWith({ screenId: "sessions", params: { target: "key-1" } });
  });

  it("withdraws the hand-off and reports why when Paseo refuses to open Kontor", () => {
    const key = handOffOpen("srv_a", { kind: "session", agentId: "agt_1" });
    expect(openKontor(() => { throw new Error("screen gone"); }, "sessions", key)).toBe("screen gone");
    expect(openKontor(() => { throw new Error(""); }, "sessions", "other")).toBe("Paseo could not open Kontor.");
    // The withdrawn request opens nothing later: the screen finds no hand-off under that key.
    expect(openHandOff({ hostId: "srv_a", key, entries: [], navigation: undefined, onView: () => {} })).toMatchObject({ ok: false, reason: "stale" });
  });
});
