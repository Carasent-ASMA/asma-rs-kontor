import { useEffect } from "react";
import type {
  PluginClientContext,
  PluginSidebarItemProps,
} from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import { SidebarRow } from "@getpaseo/plugin/client/ui";
import { contributeReturnControl } from "./client/return-control";
import { KontorScreen } from "./client/kontor-screen";
import { contributePlanControl } from "./client/plan-control";
import { SWITCH_SHEET } from "./client/start-switch-sheet";
import {
  disposeRetainedDirectories,
  startHostDirectory,
} from "./client/use-session-directory";
import { forgetWorkspaceDirectories } from "./client/use-workspace-directory";

export const SESSIONS_SCREEN_ID = "sessions";
export const SESSIONS_SIDEBAR_ID = "sessions";
export const SESSIONS_COMMAND_ID = "sessions";
export const BACK_COMMAND_ID = "back-to-kontor";
export const SWITCH_COMMAND_ID = "start-or-switch";

function runCleanup(
  label: string,
  action: () => void | Promise<void>,
): void {
  try {
    void Promise.resolve(action()).catch((error) => {
      console.error(`Kontor ${label} cleanup failed`, error);
    });
  } catch (error) {
    console.error(`Kontor ${label} cleanup failed`, error);
  }
}

function SessionsSidebarItem({
  host,
  currentScreen,
  openScreen,
}: PluginSidebarItemProps) {
  const hostStatus = useHosts().find((candidate) => candidate.serverId === host.id)?.status ?? "offline";
  useEffect(() => {
    startHostDirectory(host.id, hostStatus);
  }, [host.id, hostStatus]);
  return (
    <SidebarRow
      icon="Building2"
      label="Kontor"
      active={currentScreen?.screenId === SESSIONS_SCREEN_ID}
      onPress={() => openScreen({ screenId: SESSIONS_SCREEN_ID })}
    />
  );
}

export default function contribute(client: PluginClientContext): () => void {
  const cleanups = [
    client.addScreen({
      id: SESSIONS_SCREEN_ID,
      title: "Kontor",
      Component: KontorScreen,
    }),
    client.addSidebarHeaderItem({
      id: SESSIONS_SIDEBAR_ID,
      title: "Kontor",
      Component: SessionsSidebarItem,
    }),
    client.addCommandCenterItem({
      id: SESSIONS_COMMAND_ID,
      title: "Open Kontor",
      icon: "Building2",
      keywords: ["sessions", "agents", "navigator", "kontor"],
      context: "global",
      onSelect({ openScreen }) {
        openScreen({ screenId: SESSIONS_SCREEN_ID });
      },
    }),
    // The composer's Kontor pill as a command, for the Command Center of an opened session.
    client.addCommandCenterItem({
      id: BACK_COMMAND_ID,
      title: "Back to Kontor",
      icon: "ArrowLeft",
      keywords: ["back", "return", "kontor", "sessions"],
      context: "agent",
      onSelect({ openScreen }) {
        openScreen({ screenId: SESSIONS_SCREEN_ID });
      },
    }),
    client.addCommandCenterItem({
      id: SWITCH_COMMAND_ID,
      title: "Start or switch Kontor session…",
      icon: "History",
      keywords: ["recent", "switch", "jump", "last", "visited", "new", "start", "lsa", "adam", "kontor", "sessions"],
      context: "global",
      onSelect({ openScreen }) {
        openScreen({ screenId: SESSIONS_SCREEN_ID, params: { sheet: SWITCH_SHEET, at: String(Date.now()) } });
      },
    }),
  ];
  const stopReturnControl = contributeReturnControl(client, SESSIONS_SCREEN_ID);
  const stopPlanControl = contributePlanControl(client, SESSIONS_SCREEN_ID);
  return () => {
    runCleanup("retained directories", () => disposeRetainedDirectories());
    runCleanup("workspace directories", forgetWorkspaceDirectories);
    runCleanup("plan control", stopPlanControl);
    runCleanup("return control", stopReturnControl);
    for (const cleanup of cleanups) {
      runCleanup("contribution", cleanup);
    }
  };
}
