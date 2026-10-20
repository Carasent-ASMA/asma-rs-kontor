import type { PluginClientContext } from "@getpaseo/plugin/client";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { contributeReturnControl, stopReturnControl } = vi.hoisted(() => {
  const stopReturnControl = vi.fn();
  return {
    stopReturnControl,
    contributeReturnControl: vi.fn(() => stopReturnControl),
  };
});

vi.mock("../client/return-control", () => ({ contributeReturnControl }));

vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  Pressable: () => null,
  Text: () => null,
  View: () => null,
}));

vi.mock("@getpaseo/plugin/client", () => ({
  useHosts: () => [],
  getPaseoClient: () => {
    throw new Error("No host in the client-entry test");
  },
}));

vi.mock("@getpaseo/plugin/client/ui", () => ({
  SidebarRow: () => null,
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: () => null,
  TextInput: () => null,
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

import contribute, {
  BACK_COMMAND_ID,
  SESSIONS_COMMAND_ID,
  SESSIONS_SCREEN_ID,
  SESSIONS_SIDEBAR_ID,
} from "../index.client";

type ScreenContribution = {
  id: string;
  title: string;
  Component: unknown;
};

type SidebarContribution = {
  id: string;
  title: string;
  Component: unknown;
};

type CommandContribution = {
  id: string;
  title: string;
  context: "global" | "agent";
  icon: string;
  keywords?: readonly string[];
  onSelect: (context: { openScreen: (input: unknown) => void }) => void;
};

describe("Kontor client entry", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("registers the Kontor screen, native sidebar entry, Open Kontor and Back to Kontor commands", () => {
    const calls: Array<{
      kind: "screen" | "sidebar" | "command";
      value: ScreenContribution | SidebarContribution | CommandContribution;
    }> = [];
    const cleaned: string[] = [];
    const client = {
      addScreen(value: ScreenContribution) {
        calls.push({ kind: "screen", value });
        return () => {
          cleaned.push("screen");
        };
      },
      addSidebarHeaderItem(value: SidebarContribution) {
        calls.push({ kind: "sidebar", value });
        return () => {
          cleaned.push("sidebar");
        };
      },
      addCommandCenterItem(value: CommandContribution) {
        calls.push({ kind: "command", value });
        return () => {
          cleaned.push("command");
        };
      },
    } as unknown as PluginClientContext;

    const cleanup = contribute(client);

    expect(calls.map((call) => call.kind)).toEqual(["screen", "sidebar", "command", "command"]);

    const screen = calls[0]!.value as ScreenContribution;
    expect(screen.id).toBe(SESSIONS_SCREEN_ID);
    expect(screen.title).toBe("Kontor");
    expect(typeof screen.Component).toBe("function");

    const sidebar = calls[1]!.value as SidebarContribution;
    expect(sidebar.id).toBe(SESSIONS_SIDEBAR_ID);
    expect(sidebar.title).toBe("Kontor");
    expect(typeof sidebar.Component).toBe("function");
    const row = (sidebar.Component as (props: object) => { props: { icon: string } })({});
    expect(row.props.icon).toBe("Building2");

    const command = calls[2]!.value as CommandContribution;
    expect(command.id).toBe(SESSIONS_COMMAND_ID);
    expect(command.title).toBe("Open Kontor");
    expect(command.context).toBe("global");
    expect(command.icon).toBe("Building2");
    const opened: unknown[] = [];
    command.onSelect({ openScreen: (input) => opened.push(input) });
    expect(opened).toEqual([{ screenId: SESSIONS_SCREEN_ID }]);

    const backCommand = calls[3]!.value as CommandContribution;
    expect(backCommand.id).toBe(BACK_COMMAND_ID);
    expect(backCommand.id).not.toBe(SESSIONS_COMMAND_ID);
    expect(backCommand.title).toBe("Back to Kontor");
    expect(backCommand.context).toBe("agent");
    expect(backCommand.icon).toBe("ArrowLeft");
    expect(backCommand.keywords).toEqual(expect.arrayContaining(["back", "kontor"]));
    const returned: unknown[] = [];
    backCommand.onSelect({ openScreen: (input) => returned.push(input) });
    expect(returned).toEqual([{ screenId: SESSIONS_SCREEN_ID }]);

    expect(contributeReturnControl).toHaveBeenCalledTimes(1);
    expect(contributeReturnControl).toHaveBeenCalledWith(client, SESSIONS_SCREEN_ID);

    cleanup();
    expect(stopReturnControl).toHaveBeenCalledTimes(1);
    expect(cleaned).toEqual(["screen", "sidebar", "command", "command"]);
  });
});
