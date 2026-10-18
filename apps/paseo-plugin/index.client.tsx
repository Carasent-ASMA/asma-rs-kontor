import type {
  PluginClientContext,
  PluginSidebarItemProps,
} from "@getpaseo/plugin/client";
import { SidebarRow } from "@getpaseo/plugin/client/ui";
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
      label="Sessions"
      active={currentScreen?.screenId === SESSIONS_SCREEN_ID}
      onPress={() => openScreen({ screenId: SESSIONS_SCREEN_ID })}
    />
  );
}

export default function contribute(client: PluginClientContext): () => void {
  const cleanups = [
    client.addScreen({
      id: SESSIONS_SCREEN_ID,
      title: "Sessions",
      Component: SessionsScreen,
    }),
    client.addSidebarHeaderItem({
      id: SESSIONS_SIDEBAR_ID,
      title: "Sessions",
      Component: SessionsSidebarItem,
    }),
    client.addCommandCenterItem({
      id: SESSIONS_COMMAND_ID,
      title: "Open Kontor Sessions",
      icon: "List",
      keywords: ["sessions", "agents", "navigator", "kontor"],
      context: "global",
      onSelect({ openScreen }) {
        openScreen({ screenId: SESSIONS_SCREEN_ID });
      },
    }),
  ];
  return () => {
    for (const cleanup of cleanups) {
      void cleanup();
    }
  };
}
