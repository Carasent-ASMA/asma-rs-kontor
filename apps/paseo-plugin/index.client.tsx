import type {
  PluginClientContext,
  PluginSidebarItemProps,
} from "@getpaseo/plugin/client";
import { SidebarRow } from "@getpaseo/plugin/client/ui";
import { contributeReturnControl } from "./client/return-control";
import { SessionsScreen } from "./client/sessions-screen";

export const SESSIONS_SCREEN_ID = "sessions";
export const SESSIONS_SIDEBAR_ID = "sessions";
export const SESSIONS_COMMAND_ID = "sessions";

function SessionsSidebarItem({
  currentScreen,
  openScreen,
}: PluginSidebarItemProps) {
  return (
    <SidebarRow
      icon="List"
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
      Component: SessionsScreen,
    }),
    client.addSidebarHeaderItem({
      id: SESSIONS_SIDEBAR_ID,
      title: "Kontor",
      Component: SessionsSidebarItem,
    }),
    client.addCommandCenterItem({
      id: SESSIONS_COMMAND_ID,
      title: "Open Kontor",
      icon: "List",
      keywords: ["sessions", "agents", "navigator", "kontor"],
      context: "global",
      onSelect({ openScreen }) {
        openScreen({ screenId: SESSIONS_SCREEN_ID });
      },
    }),
  ];
  const stopReturnControl = contributeReturnControl(client, SESSIONS_SCREEN_ID);
  return () => {
    stopReturnControl();
    for (const cleanup of cleanups) {
      void cleanup();
    }
  };
}
