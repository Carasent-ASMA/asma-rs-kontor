import type { PluginClientContext } from "@getpaseo/plugin/client";
import { describe, expect, it, vi } from "vitest";

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
  context: "global";
  onSelect: (context: { openScreen: (input: unknown) => void }) => void;
};

describe("Kontor client entry", () => {
  it("registers the Sessions screen, native sidebar entry and native return command", () => {
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

    expect(calls.map((call) => call.kind)).toEqual(["screen", "sidebar", "command"]);

    const screen = calls[0]!.value as ScreenContribution;
    expect(screen.id).toBe(SESSIONS_SCREEN_ID);
    expect(screen.title).toBe("Sessions");
    expect(typeof screen.Component).toBe("function");

    const sidebar = calls[1]!.value as SidebarContribution;
    expect(sidebar.id).toBe(SESSIONS_SIDEBAR_ID);
    expect(sidebar.title).toBe("Sessions");
    expect(typeof sidebar.Component).toBe("function");

    const command = calls[2]!.value as CommandContribution;
    expect(command.id).toBe(SESSIONS_COMMAND_ID);
    expect(command.context).toBe("global");
    const opened: unknown[] = [];
    command.onSelect({ openScreen: (input) => opened.push(input) });
    expect(opened).toEqual([{ screenId: SESSIONS_SCREEN_ID }]);

    cleanup();
    expect(cleaned).toEqual(["screen", "sidebar", "command"]);
  });
});
