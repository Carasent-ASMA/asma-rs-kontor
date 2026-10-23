import type {
  PluginClientContext,
  PluginSidebarItemProps,
} from "@getpaseo/plugin/client";
import { SidebarRow } from "@getpaseo/plugin/client/ui";
import { contributeReturnControl } from "./client/return-control";
import { KontorScreen } from "./client/kontor-screen";
import { contributePlanControl } from "./client/plan-control";
import { recordVisit } from "./client/recent-sessions";
import { SWITCH_SHEET } from "./client/start-switch-sheet";

export const SESSIONS_SCREEN_ID = "sessions";
export const SESSIONS_SIDEBAR_ID = "sessions";
export const SESSIONS_COMMAND_ID = "sessions";
export const BACK_COMMAND_ID = "back-to-kontor";
export const SWITCH_COMMAND_ID = "start-or-switch";

function SessionsSidebarItem({
  currentScreen,
  openScreen,
}: PluginSidebarItemProps) {
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
      onSelect({ openScreen, agent }) {
        recordVisit(agent.id);
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
    stopPlanControl();
    stopReturnControl();
    for (const cleanup of cleanups) {
      void cleanup();
    }
  };
}
