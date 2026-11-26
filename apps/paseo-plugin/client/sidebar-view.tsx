import { useMemo, useState } from "react";
import { Text, View } from "react-native";
import {
  useAgents,
  useHosts,
  useWorkspaces,
  type AgentRef,
  type PluginHostSummary,
  type PluginOpenScreenInput,
  type PluginSidebarViewProps,
  type WorkspaceRef,
} from "@getpaseo/plugin/client";
import { useToast } from "@getpaseo/plugin/client/react-native";
import { ControlHelpProvider } from "./control-help";
import { handOffOpen, openCreatedSession, openSessionById, openTakeoverSuccessor, withdrawHandOff, type SessionNavigation } from "./session-actions";
import { useStartSwitch } from "./start-switch-sheet";
import { useSessionDirectory } from "./use-session-directory";

export const KONTOR_SIDEBAR_VIEW_ID = "kontor";

/**
 * Paseo's own client state supplies the refs its navigation accepts: `useAgents` and `useWorkspaces` return the refs
 * the app interns for this host, so Kontor never reconstructs a ref key by hand. The hooks exist from 0.16, and the
 * sidebar view only mounts on an app that has `addSidebarView`.
 */
export type SidebarRefs = {
  readonly agents: ReadonlyMap<string, AgentRef>;
  readonly workspaces: ReadonlyMap<string, WorkspaceRef>;
};

/** Kontor opens sessions by id; the sidebar slot navigates by ref. Paseo closes the compact sidebar itself after either call. */
export function screenNavigation(hostId: string, navigation: PluginSidebarViewProps["navigation"], refs: SidebarRefs): SessionNavigation {
  const unavailable = (entity: "session" | "workspace") => new Error(`That ${entity} is no longer listed on this host. Refresh the list and try again.`);
  return {
    openAgent: ({ agentId, serverId }) => {
      const ref = refs.agents.get(agentId);
      if (!ref || ref.hostId !== (serverId ?? hostId)) throw unavailable("session");
      navigation.openAgent(ref);
    },
    openWorkspace: ({ workspaceId, serverId }) => {
      const ref = refs.workspaces.get(workspaceId);
      if (!ref || ref.hostId !== (serverId ?? hostId)) throw unavailable("workspace");
      navigation.openWorkspace(ref);
    },
  };
}

/** Hands one request to Kontor's screen, as the composer popover does; a refused open withdraws it and says why. */
export function openKontor(openScreen: (input: PluginOpenScreenInput) => void, screenId: string, key: string): string | null {
  try { openScreen({ screenId, params: { target: key } }); return null; }
  catch (error) { withdrawHandOff(key); return error instanceof Error && error.message ? error.message : "Paseo could not open Kontor."; }
}

type OpenKontor = (key: string) => string | null;

/**
 * One host at a time: the active session's, else the active workspace's, else the first online host, else the first
 * configured one. The returned label and status let the view say which host's inventory it shows.
 */
export function sidebarHost(active: PluginSidebarViewProps["active"], hosts: readonly PluginHostSummary[]): {
  hostId: string; label: string; status: PluginHostSummary["status"];
} | null {
  const hostId = active.agent?.hostId ?? active.workspace?.hostId ?? (hosts.find((host) => host.status === "online") ?? hosts[0])?.serverId ?? null;
  if (!hostId) return null;
  const host = hosts.find((candidate) => candidate.serverId === hostId);
  return { hostId, label: host?.label ?? hostId, status: host?.status ?? "offline" };
}

export function KontorSidebarView(props: PluginSidebarViewProps & { onOpenKontor: OpenKontor }) {
  const hosts = useHosts();
  const host = sidebarHost(props.active, hosts);
  if (!host) return <Text style={{ color: props.theme.colors.foregroundMuted, fontSize: 12, padding: 12 }}>No Paseo host connected.</Text>;
  return <HostSidebar key={host.hostId} {...props} hostId={host.hostId} hostLabel={host.label} hostStatus={host.status} />;
}

/**
 * The third surface of Kontor's Start or switch body, after the screen's sheet and the composer popover: the same state
 * tiles, search, tree and list, row actions and New lead, so the sidebar adds no second session list to keep in step.
 * Unlike the popover it has native navigation, so a row opens its session directly and stays put beside the conversation.
 * The line above the body names the host, so two connected hosts never look like one inventory.
 */
function HostSidebar({ theme, layout, active, navigation, dismiss, onOpenKontor, hostId, hostLabel, hostStatus }: PluginSidebarViewProps & {
  onOpenKontor: OpenKontor; hostId: string; hostLabel: string; hostStatus: PluginHostSummary["status"];
}) {
  const directory = useSessionDirectory({ hostId, hostStatus });
  const toast = useToast();
  const [height, setHeight] = useState(0);
  const agentRefs = useAgents({ hosts: [hostId] });
  const workspaceRefs = useWorkspaces({ hosts: [hostId] });
  const refs = useMemo<SidebarRefs>(() => ({
    agents: new Map(agentRefs.map((ref) => [ref.id, ref])),
    workspaces: new Map(workspaceRefs.map((ref) => [ref.id, ref])),
  }), [agentRefs, workspaceRefs]);
  const open = useMemo(() => screenNavigation(hostId, navigation, refs), [hostId, navigation, refs]);
  const { header, body, footer } = useStartSwitch({
    sidebar: true, compact: layout.compact, colors: theme.colors, hostId, hostStatus, directory: directory.state, canOpen: true, frameHeight: height,
    currentId: active.agent?.hostId === hostId ? active.agent.id : null,
    initialScope: active.workspace?.hostId === hostId ? { workspaceId: active.workspace.id } : {},
    onOpen: (entry) => {
      const result = openSessionById({ entries: directory.state.entries, id: entry.id, serverId: hostId, navigation: open });
      if (!result.ok) toast.error(result.message);
    },
    onOpenTakeover: (outcome) => openTakeoverSuccessor({ outcome, serverId: hostId, navigation: open }),
    onOpenCreated: (outcome) => openCreatedSession({ outcome, serverId: hostId, navigation: open }),
    // The Kontor tile: the full Sessions screen with this state, search and membership; on a phone the drawer gives way to it.
    onShowView: (view) => {
      const failure = onOpenKontor(handOffOpen(hostId, { kind: "view", ...view }));
      if (failure) toast.error(failure); else dismiss();
    },
    onClose: dismiss,
  });
  return <ControlHelpProvider colors={theme.colors}>
    <View testID="kontor-sidebar-view" onLayout={(event) => setHeight(event.nativeEvent.layout.height)}
      style={{ flex: 1, minHeight: 0, gap: 4, paddingHorizontal: 8, paddingTop: 8, backgroundColor: theme.colors.surface0 }}>
      <Text testID="kontor-sidebar-host" accessibilityLabel={`Sessions on ${hostLabel} (${hostStatus})`} numberOfLines={1} ellipsizeMode="tail"
        style={{ flexShrink: 0, color: theme.colors.foregroundMuted, fontSize: 11, lineHeight: 15, paddingHorizontal: 1 }}>{`${hostLabel} · ${hostStatus}`}</Text>
      {header}{body}{footer}
    </View>
  </ControlHelpProvider>;
}
