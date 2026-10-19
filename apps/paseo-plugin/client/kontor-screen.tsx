import { useCallback, useEffect, useMemo, useState } from "react";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import { Modal, useToast } from "@getpaseo/plugin/client/react-native";
import { KeyboardAvoidingView, Linking, SafeAreaView, ScrollView, View } from "react-native";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { SessionsScreen } from "./sessions-screen";
import { useOrganizationSettings } from "./use-organization-settings";
import { useReadSources } from "./use-read-sources";
import { useSessionDirectory } from "./use-session-directory";
import { readSessionView } from "./session-view-store";
import { DESTINATIONS, goBack, hostNavigation, navigate, readRouteView, refreshSourceRevision, routeViewKey, sessionScopeKey, writeRouteView, type Destination, type KontorRoute } from "./navigation-state";
import { Action, Copy, type Colors } from "./page-controls";
import { ReadOnlyPage } from "./read-only-pages";
import { OrganizationSettingsPage } from "./organization-settings-page";
import { openSessionById } from "./session-actions";
import { confirmedEpic, jiraIssueUrl } from "./page-data";
import { linkedPlans } from "./navigation-state";
import { useWorkspaceDirectory } from "./use-workspace-directory";
import { GraphPage } from "./graph-page";

const PRIMARY_MOBILE: Destination[] = ["Sessions", "Overview", "Plan", "Board"];
const PRIMARY_DESKTOP: Destination[] = ["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery"];
const ICONS: Partial<Record<Destination, string>> = { Sessions: "List", Overview: "LayoutDashboard", Plan: "BookOpen", Board: "Columns3", Graph: "GitBranch", Delivery: "GitPullRequest" };
function appearanceColors(colors: Colors, appearance: string): Colors {
  if (appearance === "system") return colors;
  if (appearance === "dark") return { ...colors, foreground: "#ecedf0", foregroundMuted: "#a8adb7", surface0: "#15171c", surface1: "#1f222a", surface2: "#2e333e", border: "#414754", accent: "#9cbfff" };
  return { ...colors, foreground: "#20242c", foregroundMuted: "#5e6570", surface0: "#ffffff", surface1: "#f5f6f8", surface2: "#e6ebf2", border: "#c7cdd7", accent: "#245bba" };
}
export function KontorScreen(props: PluginScreenProps) {
  // Remount all host-local hooks on a host switch; routes remain host-scoped.
  return <HostScreen key={props.host.id} {...props} />;
}
function HostScreen(props: PluginScreenProps) {
  const { host, layout } = props;
  const hosts = useHosts();
  const hostStatus = hosts.find((candidate) => candidate.serverId === host.id)?.status ?? "offline";
  const organization = useOrganizationSettings();
  const saved = organization.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const colors = appearanceColors(props.theme.colors, saved.appearance);
  const [route, setRoute] = useState(() => hostNavigation(host.id).route);
  const [sheet, setSheet] = useState<"more" | "context" | null>(null);
  const [query, setQuery] = useState(() => readRouteView(host.id, route).query);
  const [includeArchived, setIncludeArchived] = useState(() => readSessionView(sessionScopeKey(host.id, route)).includeArchived);
  const directory = useSessionDirectory({ hostId: host.id, hostStatus, includeArchived });
  const sourceState = useReadSources(host.id, hostStatus === "online");
  const workspaces = useWorkspaceDirectory(host.id, hostStatus === "online");
  const presentation = sourceState.error && !sourceState.available
    ? { ...organization, state: { ...organization.state, status: "error" as const, error: "Plugin source service unavailable; organization preferences are not current." } }
    : organization;
  const toast = useToast();
  const visit = useCallback((patch: Partial<KontorRoute>, queryOverride?: string) => {
    const next = navigate(host.id, patch);
    if (queryOverride !== undefined) writeRouteView(host.id, next, { query: queryOverride, offset: 0 });
    setRoute({ ...next }); setQuery(readRouteView(host.id, next).query); setSheet(null);
  }, [host.id]);
  useEffect(() => {
    if (props.params.tab === "Plan") visit({ tab: "Plan", projectId: null, epicKey: null, sourceId: props.params.sourceId ?? null, originAgentId: props.params.agentId ?? null });
  }, [props.params.tab, props.params.sourceId, props.params.agentId, visit]);
  useEffect(() => {
    const revision = route.sourceId ? sourceState.reads[route.sourceId]?.revision ?? null : null;
    if (revision !== route.sourceRevision) {
      const next = refreshSourceRevision(host.id, revision);
      setRoute(next); setQuery(readRouteView(host.id, next).query);
    }
  }, [host.id, route.sourceId, route.sourceRevision, sourceState.reads]);
  const projects = useMemo(() => {
    const names = new Map<string, string>();
    for (const entry of directory.state.entries) if (entry.projectKey) names.set(entry.projectKey, entry.projectName ?? entry.projectKey);
    for (const workspace of workspaces.entries) names.set(workspace.projectId, workspace.projectCustomName ?? workspace.projectDisplayName);
    return [...names];
  }, [directory.state.entries, workspaces.entries]);
  const entries = useMemo(() => directory.state.entries.filter((entry) => (route.projectId === null || entry.projectKey === route.projectId) && (route.epicKey === null || confirmedEpic(entry) === route.epicKey)), [directory.state.entries, route.projectId, route.epicKey]);
  const sourcesInContext = sourceState.sources.filter((source) => (route.projectId === null || source.projectId === route.projectId) && (route.epicKey === null || (sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic) === route.epicKey));
  const associationIds = route.originAgentId ? linkedPlans(saved.planAssociations, route.originAgentId, new Set(sourcesInContext.map((source) => source.id))) : null;
  const sources = route.tab === "Plan" && associationIds ? sourcesInContext.filter((source) => associationIds.includes(source.id)) : sourcesInContext;
  const reads = Object.fromEntries(Object.entries(sourceState.reads).filter(([id]) => sources.some((source) => source.id === id)));
  const epics = [...new Set([...directory.state.entries.map(confirmedEpic), ...sourceState.sources.map((source) => sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic)].filter((key): key is string => key !== null))].sort();
  const projectLabel = route.epicKey ?? (route.projectId === null ? "All projects" : projects.find(([id]) => id === route.projectId)?.[1] ?? "Project unavailable");
  const primary = layout.compact ? PRIMARY_MOBILE : PRIMARY_DESKTOP;
  const setSearch = (value: string) => { writeRouteView(host.id, route, { query: value, offset: 0 }); setQuery(value); };
  const open = (entry: (typeof entries)[number]) => {
    const result = openSessionById({ entries: directory.state.entries, id: entry.id, serverId: host.id, navigation: props.navigation });
    if (!result.ok) toast.error(result.message);
  };
  const navigationItems = <>
      {primary.map((tab) => <Action key={tab} tab stacked={layout.compact} colors={colors} icon={ICONS[tab]} label={tab} selected={route.tab === tab} onPress={() => visit({ tab })} />)}
      <Action tab stacked={layout.compact} colors={colors} icon="Ellipsis" label="More" selected={!primary.includes(route.tab)} onPress={() => setSheet("more")} />
    </>;
  const navigation = <View accessibilityRole="tablist" style={{ flexDirection: "row", borderTopWidth: layout.compact ? 1 : 0, borderBottomWidth: layout.compact ? 0 : 1, borderColor: colors.border, backgroundColor: colors.surface0, paddingBottom: layout.compact ? 8 : 0 }}>
    {layout.compact ? navigationItems : <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ flexGrow: 1, justifyContent: "space-around" }}>{navigationItems}</ScrollView>}
  </View>;
  let page;
  if (route.tab === "Sessions") page = <SessionsScreen key={sessionScopeKey(host.id, route)} viewScope={sessionScopeKey(host.id, route)} {...props} theme={{ ...props.theme, colors }} directory={directory} organization={presentation} projectId={route.projectId} epicKey={route.epicKey} onIncludeArchived={setIncludeArchived}
    workspaces={workspaces.entries.filter((workspace) => (route.projectId === null || workspace.projectId === route.projectId) && (route.epicKey === null || saved.displayMappings[`workspace:${workspace.id}`]?.epic === route.epicKey)).map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name, projectKey: workspace.projectId, projectName: workspace.projectCustomName ?? workspace.projectDisplayName }))}
    workspaceNote={workspaces.error ?? `Workspace snapshot: ${workspaces.readAt ?? "Reading…"}`} />;
  else if (route.tab === "Settings") page = <OrganizationSettingsPage colors={colors} state={presentation.state} entries={directory.state.entries} workspaces={workspaces.entries.map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name }))} sources={sourceState.sources} reads={sourceState.reads} sourceError={sourceState.error} refresh={() => { void sourceState.refresh(); }} />;
  else if (route.tab === "Board" || route.tab === "Graph") page = <GraphPage mode={route.tab} hostId={host.id} theme={{ ...props.theme, colors }} compact={layout.compact} sources={sources} selectedId={route.sourceId} reads={reads} available={sourceState.available} error={sourceState.error}
    onSelect={(sourceId) => visit({ sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null, originAgentId: null })} onPlan={(sourceId, taskId) => visit({ tab: "Plan", sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null, originAgentId: null }, taskId ?? "")}
    onIssue={(key) => { const url = jiraIssueUrl(key); if (url) void Linking.openURL(url).catch(() => toast.error("This device could not open the verified Jira link.")); }} />;
  else page = <ReadOnlyPage key={routeViewKey(route)} tab={route.tab} colors={colors} entries={entries} directory={directory.state} sources={sources} reads={reads} sourceId={route.sourceId}
    sourceError={sourceState.error} query={query} setQuery={setSearch} onSource={(sourceId) => visit({ tab: "Plan", sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null, originAgentId: route.tab === "Plan" ? route.originAgentId : null })} onOpen={open} onTab={(tab) => visit({ tab, originAgentId: null })}
    onEpicTab={(tab, epicKey, sourceId) => visit({ tab, epicKey, sourceId, sourceRevision: sourceId ? sourceState.reads[sourceId]?.revision ?? null : null, originAgentId: null })}
    initialDocumentOffset={readRouteView(host.id, route).documentOffset} onDocumentScroll={(documentOffset) => { writeRouteView(host.id, route, { documentOffset }); }}
    initialOffset={readRouteView(host.id, route).offset} onScroll={(offset) => { writeRouteView(host.id, route, { offset }); }} />;
  return <SafeAreaView style={{ flex: 1, backgroundColor: colors.surface0 }}><KeyboardAvoidingView behavior={layout.platform === "ios" ? "padding" : undefined} style={{ flex: 1 }}>
    {!layout.compact ? navigation : null}
    <View style={{ flexDirection: "row", alignItems: "center", borderBottomWidth: 1, borderColor: colors.border, minHeight: 44 }}>
      <Action label="‹ Back" colors={colors} disabled={hostNavigation(host.id).back.length === 0} onPress={() => { const next = goBack(host.id); setRoute({ ...next }); setQuery(readRouteView(host.id, next).query); }} />
      <View style={{ flex: 1, minWidth: 0 }}><Action label={`${projectLabel}${route.sourceId ? ` › ${sourceState.sources.find((source) => source.id === route.sourceId)?.title ?? "Source unavailable"}` : ""} ▾`} colors={colors} onPress={() => setSheet("context")} /></View>
      {route.originAgentId && props.navigation ? <Action label="Return to conversation" colors={colors} disabled={!directory.state.entries.some((entry) => entry.id === route.originAgentId)} onPress={() => { const entry = directory.state.entries.find((candidate) => candidate.id === route.originAgentId); if (entry) open(entry); }} /> : null}
    </View>
    <View style={{ flex: 1 }}>{page}</View>
    {layout.compact ? navigation : null}
    <Modal title={sheet === "more" ? "More" : "Project context"} open={sheet !== null} onOpenChange={(open) => { if (!open) setSheet(null); }}>
      <Modal.Content scrollable={false} contentContainerStyle={{ padding: 12, gap: 8 }}>
          <ScrollView keyboardShouldPersistTaps="handled" style={{ flex: 1 }}>
            {sheet === "more" ? DESTINATIONS.filter((tab) => !primary.includes(tab)).map((tab) => <Action key={tab} label={tab} colors={colors} selected={route.tab === tab} onPress={() => visit({ tab })} />) : <>
              {route.tab === "Overview" ? <Copy colors={colors}>Overview always shows All projects. Choose another destination to scope its records.</Copy> : null}
              <Action colors={colors} label="All projects" selected={route.projectId === null && route.epicKey === null} onPress={() => visit({ projectId: null, epicKey: null, sourceId: null, originAgentId: null })} />
              {projects.map(([id, name]) => <Action key={id} colors={colors} label={name} disabled={route.tab === "Overview"} selected={route.projectId === id} onPress={() => visit({ projectId: id, epicKey: null, sourceId: null, originAgentId: null })} />)}
              {epics.map((epicKey) => <Action key={epicKey} colors={colors} label={epicKey} disabled={route.tab === "Overview"} selected={route.epicKey === epicKey} onPress={() => visit({ projectId: null, epicKey, sourceId: null, originAgentId: null })} />)}
            </>}
          </ScrollView>
          <Action label="Close" colors={colors} onPress={() => setSheet(null)} />
      </Modal.Content>
    </Modal>
  </KeyboardAvoidingView></SafeAreaView>;
}
