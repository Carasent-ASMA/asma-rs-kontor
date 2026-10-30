import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { PaseoApiProvider, PluginRpcProvider } from "paseo-sdk/host";
import { Pressable, ScrollView, Text, View, useWindowDimensions } from "react-native-web";
import { hostNavigation, navigate, sessionScopeKey } from "../../../paseo-plugin/client/navigation-state";
import { patchSessionView } from "../../../paseo-plugin/client/session-view-store";
import { hostEvents, subscribeHostEvents, logHostEvent } from "./event-log";
import type { PluginButtonContentProps, PluginButtonIconProps } from "paseo-sdk/client";
import { contributedScreen, contributedSidebar, pillsFor, registryVersion, setOpenScreenHandler, subscribeRegistry, type HostRoute, type Pill } from "./plugin-host";
import { fakePaseo, fixtureSessions, HOST_ID, HOST_LABEL, invokeRpc, scenario, setHostStatus, simulateUpdate } from "./runtime";
import { Icon, ToastLayer } from "./sdk-react-native";
import { PluginPopover, SHEET_BELOW_WIDTH } from "./plugin-popover";
import { shellColors, themeName } from "./theme";
import { WORKSPACES } from "../fixtures/sessions";
import { PLAN_99101, PLAN_99102 } from "../fixtures/sources";

/** Assumed compact breakpoint; Paseo's own layout decision is not exported to plugins. */
const COMPACT_WIDTH = 768;
const queryClient = new QueryClient();

// Seed the plugin's own in-memory route and Sessions view, as a user would have left them.
const start = scenario.start;
if (start?.tab && start.tab !== "Sessions") {
  const read = [PLAN_99101, PLAN_99102].find((value) => value.source.id === start.sourceId);
  navigate(HOST_ID, { tab: start.tab, sourceId: read?.source.id ?? null, sourceRevision: read?.revision ?? null, epicKey: read?.graph?.epic.jira ?? null, projectId: read?.source.projectId ?? null });
  hostNavigation(HOST_ID).back = [];
}
if (start?.query || start?.status || start?.grouping) {
  patchSessionView(sessionScopeKey(HOST_ID, hostNavigation(HOST_ID).route), { query: start.query ?? "", status: start.status ?? "all", ...(start.grouping ? { grouping: start.grouping } : {}) });
}
for (const step of scenario.script ?? []) setTimeout(() => simulateUpdate(step.agentId, step.patch, `${step.note} (simulated)`), step.afterMs);
if (scenario.hostOfflineAfterMs) setTimeout(() => { setHostStatus("offline"); logHostEvent("simulated.update", "host offline (simulated)"); }, scenario.hostOfflineAfterMs);

export function HostShell() {
  const colors = shellColors();
  const { width } = useWindowDimensions();
  const compact = width < COMPACT_WIDTH;
  const [stack, setStack] = useState<HostRoute[]>([{ kind: "screen", screenId: "sessions", params: scenario.start?.sheet ? { sheet: scenario.start.sheet } : {} }]);
  const [logOpen, setLogOpen] = useState(false);
  useSyncExternalStore(subscribeRegistry, registryVersion);
  const events = useSyncExternalStore(subscribeHostEvents, () => hostEvents().length);
  useEffect(() => setOpenScreenHandler((route) => setStack((current) => [...current, route])), []);
  const push = (route: HostRoute) => setStack((current) => [...current, route]);
  const back = () => setStack((current) => current.length > 1 ? current.slice(0, -1) : current);
  const navigation = useMemo(() => ({
    openAgent: ({ agentId }: { agentId: string }) => { logHostEvent("navigation.openAgent", agentId); push({ kind: "agent", agentId }); },
    openWorkspace: ({ workspaceId }: { workspaceId: string }) => { logHostEvent("navigation.openWorkspace", workspaceId); push({ kind: "workspace", workspaceId }); },
  }), []);
  const route = stack[stack.length - 1]!;
  const theme = { colors };
  const hostProps = { theme, host: { id: HOST_ID, label: HOST_LABEL }, layout: { compact, platform: "web" as const }, navigation };

  let body;
  let title = "Kontor";
  if (route.kind === "screen") {
    const screen = contributedScreen(route.screenId);
    const Component = screen?.Component;
    body = Component ? <Component key={`${stack.length}:${JSON.stringify(route.params)}`} {...hostProps} params={route.params} /> : <Text style={{ color: colors.foreground, padding: 20 }}>Screen {route.screenId} is not contributed.</Text>;
  } else if (route.kind === "agent") {
    const found = fixtureSessions().find((value) => value.agent.id === route.agentId);
    title = found?.agent.title ?? "Session";
    body = <AgentShim agentId={route.agentId} hostProps={hostProps} onOpenWorkspace={(id) => navigation.openWorkspace({ workspaceId: id })} />;
  } else {
    const workspace = WORKSPACES.find((value) => value.id === route.workspaceId);
    title = workspace?.name ?? "Workspace";
    body = <WorkspaceShim workspaceId={route.workspaceId} onOpenAgent={(id) => navigation.openAgent({ agentId: id })} />;
  }

  const prototypeBar = <View testID="prototype-bar" style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 10, minHeight: 28, backgroundColor: themeName === "light" ? "#fff4d6" : "#3a3220", borderBottomWidth: 1, borderColor: colors.border }}>
    {/* The scenario names itself first and the line may wrap once, so a phone still shows what is simulated, such as SYNTHETIC totals. */}
    <Text numberOfLines={2} style={{ flex: 1, color: colors.foreground, fontSize: 11 }}>
      <Text style={{ fontWeight: "700" }}>PROTOTYPE</Text> · {scenario.title} · {scenario.inventory === "captured" ? "captured local data, frozen; effects simulated" : "fictional fixture data"} · no Paseo host{scenario.script ? " · simulated updates" : ""}{scenario.simulatedTeams ? " · SIMULATED team declarations" : ""}
    </Text>
    <Pressable testID="prototype-log-toggle" accessibilityRole="button" accessibilityState={{ expanded: logOpen }} onPress={() => setLogOpen(!logOpen)} style={{ minHeight: 28, justifyContent: "center", paddingHorizontal: 6 }}>
      <Text style={{ color: colors.foreground, fontSize: 11, textDecorationLine: "underline" }}>Host log ({events})</Text>
    </Pressable>
  </View>;
  const hostHeader = <View testID="host-header" style={{ flexDirection: "row", alignItems: "center", gap: 6, minHeight: 44, paddingHorizontal: 6, borderBottomWidth: 1, borderColor: colors.border, backgroundColor: colors.surface0 }}>
    {stack.length > 1 ? <Pressable testID="host-back" accessibilityRole="button" accessibilityLabel="Back (host)" onPress={back} style={{ minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center" }}>
      <Icon name="ArrowLeft" size={18} color={colors.foreground} />
    </Pressable> : null}
    <Text numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{title}</Text>
  </View>;

  return <QueryClientProvider client={queryClient}><PaseoApiProvider paseo={fakePaseo}><PluginRpcProvider invoke={invokeRpc}>
    <View style={{ height: "100%", backgroundColor: colors.surface0 }}>
      {prototypeBar}
      <View style={{ flex: 1, flexDirection: "row", minHeight: 0 }}>
        {compact ? null : <View testID="host-sidebar" style={{ width: 240, borderRightWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1, padding: 8, gap: 4 }}>
          <Text style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 10, paddingVertical: 6 }}>{HOST_LABEL} · host shim</Text>
          {contributedSidebar().map((item) => { const Item = item.Component; return <Item key={item.id} {...hostProps} currentScreen={route.kind === "screen" ? { screenId: route.screenId, params: route.params } : null} openScreen={(input: { screenId: string; params?: Record<string, string> }) => push({ kind: "screen", screenId: input.screenId, params: input.params ?? {} })} openPopover={() => undefined} />; })}
        </View>}
        <View style={{ flex: 1, minWidth: 0 }}>
          {hostHeader}
          <View style={{ flex: 1, minHeight: 0 }}>{body}</View>
        </View>
        {logOpen ? <HostLog onClose={() => setLogOpen(false)} compact={compact} /> : null}
      </View>
      <ToastLayer />
    </View>
  </PluginRpcProvider></PaseoApiProvider></QueryClientProvider>;
}

function HostLog({ onClose, compact }: { onClose: () => void; compact: boolean }) {
  const colors = shellColors();
  useSyncExternalStore(subscribeHostEvents, () => hostEvents().length);
  const rows = [...hostEvents()].reverse();
  return <View testID="prototype-log" style={compact ? { position: "absolute", left: 0, right: 0, bottom: 0, height: "45%", backgroundColor: colors.surface1, borderTopWidth: 1, borderColor: colors.border } : { width: 320, borderLeftWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1 }}>
    <View style={{ flexDirection: "row", alignItems: "center", padding: 8 }}>
      <Text style={{ flex: 1, color: colors.foreground, fontWeight: "600", fontSize: 12 }}>Host log: navigation runs in the shim; Paseo effects are mocked</Text>
      <Pressable accessibilityRole="button" accessibilityLabel="Close host log" onPress={onClose} style={{ minWidth: 44, minHeight: 32, alignItems: "center", justifyContent: "center" }}><Text style={{ color: colors.foreground }}>✕</Text></Pressable>
    </View>
    <ScrollView contentContainerStyle={{ padding: 8, gap: 6 }}>
      {rows.length ? rows.map((event) => <Text key={event.seq} style={{ color: colors.foregroundMuted, fontSize: 11, fontFamily: "monospace" }}>{event.at.slice(11, 19)} {event.kind} · {event.detail}</Text>)
        : <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Nothing yet. Opened sessions and workspaces (shown by the host shim), and mocked settings saves, links, copies and toasts appear here.</Text>}
    </ScrollView>
  </View>;
}

/**
 * The session the host opens: fixture facts, then a fixture chat whose composer carries the pills the plugin registered.
 * A pill runs its own registered behavior: an action is called, and a popover mounts its Content once, in the host's
 * popover chrome anchored to that pill, with close().
 */
function AgentShim({ agentId, hostProps, onOpenWorkspace }: { agentId: string; hostProps: Record<string, unknown>; onOpenWorkspace: (id: string) => void }) {
  const colors = shellColors();
  const [popover, setPopover] = useState<Pill | null>(null);
  const anchors = useRef(new Map<string, HTMLElement>()), anchor = useRef<HTMLElement | null>(null);
  const { width } = useWindowDimensions();
  useSyncExternalStore(subscribeRegistry, registryVersion);
  const found = fixtureSessions().find((value) => value.agent.id === agentId);
  if (!found) return <Text style={{ color: colors.foreground, padding: 20 }}>Session {agentId} is not in the fixture.</Text>;
  const { agent, project } = found;
  const press = (pill: Pill) => {
    const behavior = pill.button.behavior;
    logHostEvent("composer.pill", `${pill.id} ${behavior.kind}`);
    if (behavior.kind === "popover") { anchor.current = anchors.current.get(pill.id) ?? null; setPopover(pill); }
    else if (behavior.kind === "action") void behavior.onPress();
    else logHostEvent("composer.pill", "menu behavior is not simulated");
  };
  const open = popover && popover.button.behavior.kind === "popover" ? { pill: popover, Content: popover.button.behavior.Content } : null;
  return <View testID="host-agent-shim" style={{ flex: 1 }}><ScrollView style={{ flex: 1 }} contentContainerStyle={{ padding: 16, gap: 10 }}>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>Host shim: in Paseo this opens the native conversation. The prototype shows the session's fixture facts instead.</Text>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 16, fontWeight: "600" }}>{agent.title}</Text>
    <Text style={{ color: colors.foreground, fontSize: 13 }}>Status {agent.status}{agent.pendingPermissions?.length ? " · waiting for permission" : ""}{agent.archivedAt ? " · archived" : ""}</Text>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>ID {agent.id} · {agent.provider} {agent.model ?? ""}</Text>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>Workspace {project?.workspaceName ?? "none"} · project {project?.projectName ?? "none"}</Text>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>Labels {Object.entries(agent.labels).map(([key, value]) => `${key}=${value}`).join(", ") || "none"}</Text>
    {agent.workspaceId ? <Pressable accessibilityRole="button" onPress={() => onOpenWorkspace(agent.workspaceId!)} style={{ alignSelf: "flex-start", minHeight: 44, justifyContent: "center" }}><Text style={{ color: colors.foreground, textDecorationLine: "underline" }}>Open workspace</Text></Pressable> : null}
    <View testID="host-chat" style={{ marginTop: 12, padding: 12, borderRadius: 8, borderWidth: 1, borderStyle: "dashed", borderColor: colors.border }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>Fixture chat: in Paseo the conversation shows here. Nothing is loaded or sent in the prototype.</Text>
    </View>
  </ScrollView>
    <View testID="host-composer" style={{ gap: 6, padding: 8, borderTopWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1 }}>
      <View testID="host-composer-pills" style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
        {pillsFor(agentId).map((pill) => <Pressable key={pill.id} ref={(node: unknown) => { if (node) anchors.current.set(pill.id, node as HTMLElement); else anchors.current.delete(pill.id); }} testID={`host-pill-${pill.id}`} accessibilityRole="button" accessibilityLabel={pill.button.title}
          accessibilityState={pill.button.behavior.kind === "popover" ? { expanded: popover?.id === pill.id } : undefined} disabled={pill.button.disabled} onPress={() => press(pill)}
          style={{ flexDirection: "row", alignItems: "center", gap: 6, minHeight: 32, paddingHorizontal: 10, borderRadius: 16, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1 }}>
          <PillIcon pill={pill} hostProps={hostProps} color={colors.foreground} />
          <Text style={{ color: colors.foreground, fontSize: 12 }}>{pill.button.label ?? pill.button.title}</Text>
        </Pressable>)}
      </View>
      <View testID="host-composer-input" style={{ minHeight: 44, justifyContent: "center", paddingHorizontal: 10, borderRadius: 8, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface0 }}>
        <Text style={{ color: colors.foregroundMuted, fontSize: 13 }}>Message (host shim: not sent)</Text>
      </View>
    </View>
    {/* Paseo hands a pill's popover the compact flag of its own form factor, which also picks the sheet. */}
    {open ? <PluginPopover title={open.pill.button.title} anchor={anchor} onClose={() => setPopover(null)}>
      <open.Content {...(hostProps as object as PluginButtonContentProps)} layout={{ compact: width < SHEET_BELOW_WIDTH, platform: "web" }} context="agent" workspaceId={open.pill.workspaceId} agentId={open.pill.agentId} close={() => setPopover(null)} />
    </PluginPopover> : null}
  </View>;
}

/** A pill's icon as Paseo draws it: a registered icon name, or the plugin's own component given the SDK's icon props. */
function PillIcon({ pill, hostProps, color }: { pill: Pill; hostProps: Record<string, unknown>; color: string }) {
  const icon = pill.button.icon;
  if (typeof icon === "string") return <Icon name={icon} size={14} color={color} />;
  if (typeof icon !== "function") return null; // Paseo renders inline SVG icons; this prototype omits that form.
  const Registered = icon;
  return <Registered {...(hostProps as object as PluginButtonIconProps)} context="agent" workspaceId={pill.workspaceId} agentId={pill.agentId} size={14} color={color} />;
}

function WorkspaceShim({ workspaceId, onOpenAgent }: { workspaceId: string; onOpenAgent: (id: string) => void }) {
  const colors = shellColors();
  const workspace = WORKSPACES.find((value) => value.id === workspaceId);
  const members = fixtureSessions().filter((value) => value.agent.workspaceId === workspaceId);
  return <ScrollView testID="host-workspace-shim" contentContainerStyle={{ padding: 16, gap: 8 }}>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>Host shim: in Paseo this opens the workspace. The prototype lists its fixture sessions.</Text>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 16, fontWeight: "600" }}>{workspace?.name ?? workspaceId}</Text>
    {members.map(({ agent }) => <Pressable key={agent.id} accessibilityRole="button" onPress={() => onOpenAgent(agent.id)} style={{ minHeight: 44, justifyContent: "center", borderBottomWidth: 1, borderColor: colors.border }}>
      <Text style={{ color: colors.foreground, fontSize: 13 }}>{agent.title} <Text style={{ color: colors.foregroundMuted }}>· {agent.status}{agent.archivedAt ? " · archived" : ""}</Text></Text>
    </Pressable>)}
  </ScrollView>;
}
