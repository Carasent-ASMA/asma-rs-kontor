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
import { Action, type Colors } from "./page-controls";
import { ReadOnlyPage } from "./read-only-pages";
import { OrganizationSettingsPage } from "./organization-settings-page";
import { openSessionById } from "./session-actions";
import { confirmedEpic, jiraIssueUrl } from "./page-data";
import { linkedPlans } from "./navigation-state";
import { useWorkspaceDirectory } from "./use-workspace-directory";
import { GraphPage } from "./graph-page";
import { contextPlanSources, resolveContextPlan, refreshContextSource } from "./navigation-state";
import { NativeFreshnessPill } from "./native-freshness";
import { resolvePlanLink } from "./plan-links";

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
    if (routeViewKey(hostNavigation(host.id).route) !== routeViewKey(route)) return;
    const resolved = resolveContextPlan(route, sourceState.sources, sourceState.reads, route.originAgentId ? saved.planAssociations[route.originAgentId] ?? [] : null);
    if (resolved.sourceId && resolved.sourceId !== route.sourceId) {
      const next = refreshContextSource(host.id, resolved.sourceId, resolved.sourceRevision);
      setRoute(next); setQuery(readRouteView(host.id, next).query); return;
    }
    const revision = route.sourceId ? sourceState.reads[route.sourceId]?.revision ?? null : null;
    if (revision !== route.sourceRevision) {
      const next = refreshSourceRevision(host.id, revision);
      setRoute(next); setQuery(readRouteView(host.id, next).query);
    }
  }, [host.id, route, sourceState.sources, sourceState.reads, saved.planAssociations]);
  const projects = useMemo(() => {
    const names = new Map<string, string>();
    for (const entry of directory.state.entries) if (entry.projectKey) names.set(entry.projectKey, entry.projectName ?? entry.projectKey);
    for (const workspace of workspaces.entries) names.set(workspace.projectId, workspace.projectCustomName ?? workspace.projectDisplayName);
    return [...names];
  }, [directory.state.entries, workspaces.entries]);
  const selectedPlan = route.sourceId ? sourceState.sources.find((source) => source.id === route.sourceId && source.kind === "plan") : undefined;
  const scopedEpic = selectedPlan ? sourceState.reads[selectedPlan.id]?.graph?.epic.jira ?? selectedPlan.epic : route.epicKey;
  const entries = useMemo(() => directory.state.entries.filter((entry) => (route.projectId === null || entry.projectKey === route.projectId) && (scopedEpic === null || confirmedEpic(entry) === scopedEpic) && !(route.tab === "Overview" && route.sourceId && !scopedEpic)), [directory.state.entries, route.projectId, scopedEpic, route.tab, route.sourceId]);
  const sourcesInContext = sourceState.sources.filter((source) => source.kind === "jira"
    ? source.projectId === null || route.projectId === null || source.projectId === route.projectId
    : (route.projectId === null || source.projectId === route.projectId) && (scopedEpic === null || (sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic) === scopedEpic) && (route.tab !== "Overview" || route.sourceId === null || source.id === route.sourceId || (source.kind !== "plan" && scopedEpic !== null)));
  const associationIds = route.originAgentId ? linkedPlans(saved.planAssociations, route.originAgentId, new Set(sourceState.sources.filter((source) => source.kind === "plan").map((source) => source.id))) : null;
  const linkedContext = ["Plan", "Graph", "Board"].includes(route.tab) && associationIds !== null;
  const sources = linkedContext ? sourcesInContext.filter((source) => associationIds!.includes(source.id)) : sourcesInContext;
  const reads = Object.fromEntries(Object.entries(sourceState.reads).filter(([id]) => sources.some((source) => source.id === id)));
  const epics = [...new Set([...directory.state.entries.map(confirmedEpic), ...sourceState.sources.map((source) => sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic)].filter((key): key is string => key !== null))].sort();
  const projectLabel = route.epicKey ?? (route.projectId === null ? "All projects" : projects.find(([id]) => id === route.projectId)?.[1] ?? "Project unavailable");
  const pickerPlans = contextPlanSources(linkedContext ? { ...route, epicKey: null, projectId: null } : route, sourceState.sources, sourceState.reads, linkedContext ? associationIds : null);
  const visitContext = (patch: Partial<KontorRoute>, queryOverride?: string) => {
    const candidate = { ...hostNavigation(host.id).route, ...patch };
    const resolved = resolveContextPlan(candidate, sourceState.sources, sourceState.reads, candidate.originAgentId ? saved.planAssociations[candidate.originAgentId] ?? [] : null);
    visit({ ...patch, sourceId: resolved.sourceId, sourceRevision: resolved.sourceRevision }, queryOverride);
  };
  const selectSource = (sourceId: string, tab?: Destination) => {
    const source = sourceState.sources.find((candidate) => candidate.id === sourceId && candidate.kind === "plan");
    if (!source) return;
    visit({ ...(tab ? { tab } : {}), sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null,
      projectId: source.projectId, epicKey: sourceState.reads[sourceId]?.graph?.epic.jira ?? source.epic,
      originAgentId: route.tab === "Plan" ? route.originAgentId : null });
  };
  const primary = layout.compact ? PRIMARY_MOBILE : PRIMARY_DESKTOP;
  const setSearch = (value: string) => { writeRouteView(host.id, route, { query: value, offset: 0 }); setQuery(value); };
  const open = (entry: (typeof entries)[number]) => {
    const result = openSessionById({ entries: directory.state.entries, id: entry.id, serverId: host.id, navigation: props.navigation });
    if (!result.ok) toast.error(result.message);
  };
  const openDocumentLink = (target: string) => {
    if (!selectedPlan) { toast.error("The selected plan source is unavailable."); return; }
    const linkedSources = associationIds ? sourceState.sources.filter((source) => associationIds.includes(source.id)) : sourceState.sources;
    const link = resolvePlanLink(target, selectedPlan, linkedSources, directory.state.entries);
    if (link.kind === "source") selectSource(link.id, "Plan");
    else if (link.kind === "session") { const entry = directory.state.entries.find((entry) => entry.id === link.id); if (entry) open(entry); }
    else if (link.kind === "url") void Linking.openURL(link.url).catch(() => toast.error("This device could not open the source link."));
    else toast.error(link.reason);
  };
  const navigationItems = <>
      {primary.map((tab) => <Action key={tab} tab stacked={layout.compact} colors={colors} icon={ICONS[tab]} label={tab} selected={route.tab === tab} onPress={() => visitContext({ tab })} />)}
      <Action tab stacked={layout.compact} colors={colors} icon="Ellipsis" label="More" selected={!primary.includes(route.tab)} onPress={() => setSheet("more")} />
    </>;
  const navigation = <View accessibilityRole="tablist" style={{ flexDirection: "row", borderTopWidth: layout.compact ? 1 : 0, borderBottomWidth: layout.compact ? 0 : 1, borderColor: colors.border, backgroundColor: colors.surface0, paddingBottom: layout.compact ? 8 : 0 }}>
    {layout.compact ? navigationItems : <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ flexGrow: 1, justifyContent: "space-around" }}>{navigationItems}</ScrollView>}
  </View>;
  let page;
  if (route.tab === "Sessions") page = <SessionsScreen key={sessionScopeKey(host.id, route)} viewScope={sessionScopeKey(host.id, route)} {...props} theme={{ ...props.theme, colors }} directory={directory} organization={presentation} projectId={route.projectId} epicKey={route.epicKey} onIncludeArchived={setIncludeArchived}
    workspaces={workspaces.entries.filter((workspace) => (route.projectId === null || workspace.projectId === route.projectId) && (route.epicKey === null || saved.displayMappings[`workspace:${workspace.id}`]?.epic === route.epicKey)).map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name, projectKey: workspace.projectId, projectName: workspace.projectCustomName ?? workspace.projectDisplayName }))}
    workspaceNote={workspaces.error ?? `Workspace snapshot: ${workspaces.readAt ?? "Reading…"}`} />;
  else if (route.tab === "Settings") page = <OrganizationSettingsPage colors={colors} state={presentation.state} entries={directory.state.entries} workspaces={workspaces.entries.map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name }))} sources={sourceState.sources} reads={sourceState.reads} teamsSnapshots={sourceState.teamsSnapshots} teamsErrors={sourceState.teamsErrors} sourcesAvailable={sourceState.available} sourceError={sourceState.error} refresh={() => { void sourceState.refresh(); }} />;
  else if (route.tab === "Board" || route.tab === "Graph") page = <GraphPage key={routeViewKey(route)} mode={route.tab} hostId={host.id} theme={{ ...props.theme, colors }} compact={layout.compact} sources={sources} selectedId={route.sourceId} reads={reads} available={sourceState.available} error={sourceState.error}
    initialOffset={readRouteView(host.id, route).offset} onScroll={(offset) => writeRouteView(host.id, route, { offset })}
    onChooseSource={() => setSheet("context")}
    onSelect={selectSource} onPlan={(sourceId, taskId) => visit({ tab: "Plan", sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null, originAgentId: route.originAgentId }, taskId ?? "")}
    onIssue={(key) => { const url = jiraIssueUrl(key); if (url) void Linking.openURL(url).catch(() => toast.error("This device could not open the verified Jira link.")); }} />;
  else page = <ReadOnlyPage key={routeViewKey(route)} compact={layout.compact} onChooseSource={() => setSheet("context")} onDocumentLink={openDocumentLink} contextLabel={route.sourceId ? selectedPlan?.title ?? "Source unavailable" : projectLabel} tab={route.tab} colors={colors} entries={entries} directory={directory.state} sources={sources} reads={reads} sourceId={route.sourceId}
    teamsSources={sourceState.sources.filter((source) => source.kind === "teams")} teamsJsonReads={Object.fromEntries(Object.entries(sourceState.reads).filter(([, read]) => read.source.kind === "teams"))}
    teamsSnapshots={sourceState.teamsSnapshots} teamsErrors={sourceState.teamsErrors} teamsBindingDiagnostics={sourceState.teamsBindingDiagnostics} teamsAvailable={sourceState.available}
    sourceError={sourceState.error} query={query} setQuery={setSearch} onSource={(sourceId) => selectSource(sourceId, "Plan")} onOpen={open} onTab={(tab) => visit({ tab, originAgentId: null })}
    onEpicTab={(tab, epicKey, sourceId) => visitContext({ tab, epicKey, sourceId, sourceRevision: sourceId ? sourceState.reads[sourceId]?.revision ?? null : null, originAgentId: null })}
    initialDocumentOffset={readRouteView(host.id, route).documentOffset} onDocumentScroll={(documentOffset) => { writeRouteView(host.id, route, { documentOffset }); }}
    initialOffset={readRouteView(host.id, route).offset} onScroll={(offset) => { writeRouteView(host.id, route, { offset }); }} />;
  return <SafeAreaView style={{ flex: 1, backgroundColor: colors.surface0 }}><KeyboardAvoidingView behavior={layout.platform === "ios" ? "padding" : undefined} style={{ flex: 1 }}>
    <View testID="kontor-context-row" style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingRight: 8, borderBottomWidth: 1, borderColor: colors.border, minHeight: 52 }}>
      <Action label="‹ Back" colors={colors} disabled={hostNavigation(host.id).back.length === 0} onPress={() => { const next = goBack(host.id); setRoute({ ...next }); setQuery(readRouteView(host.id, next).query); }} />
      <View style={{ flex: 1, minWidth: 0, borderWidth: 1, borderColor: colors.border, borderRadius: 7 }}><Action label={`${selectedPlan && scopedEpic ? `${scopedEpic} · ` : ""}${route.sourceId ? selectedPlan?.title ?? "Source unavailable" : projectLabel} ▾`} accessibilityLabel={`Project context: ${scopedEpic ?? projectLabel}, ${selectedPlan?.title ?? "choose plan"}`} colors={colors} onPress={() => setSheet("context")} /></View>
      <NativeFreshnessPill state={directory.state} hostOnline={hostStatus === "online"} colors={colors} />
      {route.originAgentId && props.navigation ? <Action label="Return to conversation" colors={colors} disabled={!directory.state.entries.some((entry) => entry.id === route.originAgentId)} onPress={() => { const entry = directory.state.entries.find((candidate) => candidate.id === route.originAgentId); if (entry) open(entry); }} /> : null}
    </View>
    {!layout.compact ? navigation : null}
    <View style={{ flex: 1 }}>{page}</View>
    {layout.compact ? navigation : null}
    <Modal title={sheet === "more" ? "More" : "Project context"} open={sheet !== null} onOpenChange={(open) => { if (!open) setSheet(null); }}>
      <Modal.Content scrollable={false} contentContainerStyle={{ padding: 12, gap: 8 }}>
          <ScrollView keyboardShouldPersistTaps="handled" style={{ flex: 1 }}>
            {sheet === "more" ? DESTINATIONS.filter((tab) => !primary.includes(tab)).map((tab) => <Action key={tab} label={tab} colors={colors} selected={route.tab === tab} onPress={() => visitContext({ tab })} />) : <>
              <Action colors={colors} label="All projects" accessibilityLabel="All projects, clear selection" selected={route.projectId === null && route.epicKey === null && route.sourceId === null} onPress={() => visit({ projectId: null, epicKey: null, sourceId: null, originAgentId: null })} />
              {projects.map(([id, name]) => <Action key={id} colors={colors} label={name} selected={route.projectId === id} onPress={() => visit({ projectId: id, epicKey: null, sourceId: null, originAgentId: null })} />)}
              {epics.map((epicKey) => <Action key={epicKey} colors={colors} label={epicKey} selected={route.epicKey === epicKey} onPress={() => visitContext({ projectId: null, epicKey, sourceId: null, sourceRevision: null, originAgentId: null })} />)}
              {pickerPlans.map((source) => <Action key={`source:${source.id}`} colors={colors} label={`Plan: ${source.title}`} selected={route.sourceId === source.id} onPress={() => selectSource(source.id)} />)}
            </>}
          </ScrollView>
          <Action label="Close" colors={colors} onPress={() => setSheet(null)} />
      </Modal.Content>
    </Modal>
  </KeyboardAvoidingView></SafeAreaView>;
}
