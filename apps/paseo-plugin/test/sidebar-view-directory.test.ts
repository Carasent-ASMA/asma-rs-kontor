import { afterEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => {
  const { deferredReact } = await import("./hermes-scoping");
  return { current: deferredReact() };
});

const model = vi.hoisted(() => ({
  hosts: [] as Array<{ serverId: string; label: string; status: "online" | "offline" }>,
  getPaseoClient: vi.fn(() => { throw new Error("No host client in this test"); }),
  directoryCalls: [] as unknown[],
}));

vi.mock("react", () => harness.current.react);
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client", () => ({
  getPaseoClient: model.getPaseoClient,
  useHosts: () => model.hosts,
  useAgents: () => [],
  useWorkspaces: () => [],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ useToast: () => ({ error: () => {}, show: () => {} }) }));
vi.mock("../client/use-session-directory", () => ({
  useSessionDirectory: (input: unknown) => {
    model.directoryCalls.push(input);
    return { state: { entries: [] } };
  },
}));
vi.mock("../client/control-help", () => ({ ControlHelpProvider: "ControlHelpProvider" }));
vi.mock("../client/start-switch-sheet", () => ({ useStartSwitch: () => ({ header: null, body: null, footer: null }) }));
vi.mock("../client/recent-sessions", () => ({ recordVisit: () => {} }));

import { KontorSidebarView } from "../client/sidebar-view";

const VIEW_PROPS = {
  theme: { colors: {} },
  layout: { compact: false },
  active: { agent: null, workspace: null },
  navigation: { openAgent: () => {}, openWorkspace: () => {}, openTerminal: () => {}, openProject: () => {} },
  dismiss: () => {},
  onOpenKontor: () => null,
} as unknown as Parameters<typeof KontorSidebarView>[0];

afterEach(async () => {
  const directory = await vi.importActual<typeof import("../client/use-session-directory")>("../client/use-session-directory");
  await directory.disposeRetainedDirectories();
});

describe("Kontor sidebar view directory ownership", () => {
  it("reads the shown host's directory without claiming this runtime's owner", () => {
    model.hosts = [{ serverId: "srv_b", label: "TEST", status: "online" }];
    model.directoryCalls = [];
    harness.current.render(() => {
      const element = KontorSidebarView(VIEW_PROPS) as unknown as {
        type: (props: unknown) => unknown;
        props: unknown;
      };
      return element.type(element.props);
    });
    expect(model.directoryCalls).toEqual([
      expect.objectContaining({ hostId: "srv_b", hostStatus: "online", claimOwner: false }),
    ]);
  });

  it("does not claim the owner when a read-only mount retains another host first", async () => {
    const directory = await vi.importActual<typeof import("../client/use-session-directory")>("../client/use-session-directory");
    const seen: number[] = [];
    const release = directory.bindHostDirectory(() => seen.push(1));

    harness.current.render(() => directory.useSessionDirectory({ hostId: "srv_b", hostStatus: "online", claimOwner: false }));
    expect(seen).toEqual([]);

    directory.startHostDirectory("srv_a");
    expect(seen.length).toBeGreaterThan(0);
    release();
  });

  it("claims the owner when a host-bearing mount finds a read-only record first", async () => {
    const directory = await vi.importActual<typeof import("../client/use-session-directory")>("../client/use-session-directory");
    const seen: unknown[] = [];
    const release = directory.bindHostDirectory((state) => seen.push(state));
    const draw = () => {
      let view!: ReturnType<typeof directory.useSessionDirectory>;
      harness.current.render(() => {
        view = directory.useSessionDirectory({ hostId: "srv_a", hostStatus: "online", claimOwner: false });
      });
      return view;
    };

    draw();
    expect(seen).toEqual([]);

    directory.startHostDirectory("srv_a");
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.at(-1)).toEqual(draw().state);
    release();
  });
});
