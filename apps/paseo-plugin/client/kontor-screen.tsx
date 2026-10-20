import { useCallback, useEffect, useMemo, useRef, useState, type Ref } from "react";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import { copyText, Icon, TextInput, useToast } from "@getpaseo/plugin/client/react-native";
import { KeyboardAvoidingView, Linking, Pressable, SafeAreaView, ScrollView, Text, View } from "react-native";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { SessionsScreen } from "./sessions-screen";
import { useOrganizationSettings } from "./use-organization-settings";
import { useKeyboardSettings } from "./use-keyboard-settings";
import { useBackShortcut } from "./keyboard-shortcuts";
import { KeyboardSettingsCard } from "./keyboard-settings-card";
import { useReadSources } from "./use-read-sources";
import { useSessionDirectory } from "./use-session-directory";
import { patchSessionView, readSessionView, sessionViewTransition } from "./session-view-store";
import { DESTINATIONS, contextHeading, goBack, hostNavigation, navigate, NO_SCOPE, readPageView, readRouteView, refreshSourceRevision, routeViewKey, sessionScopeKey, tabScope, writePageView, writeRouteView, type Destination, type KontorRoute } from "./navigation-state";
import { AnchoredMenu, MenuItem } from "./anchored-menu";
import { Action, IconButton, type Colors } from "./page-controls";
import { ReadOnlyPage } from "./read-only-pages";
import { DEFAULT_PLAN_VIEW } from "./plan-reader";
import { DEFAULT_TEAMS_VIEW } from "./teams-source-page";
import { OrganizationSettingsPage } from "./organization-settings-page";
import { openSessionById } from "./session-actions";
import { confirmedEpic, jiraIssueUrl } from "./page-data";
import { linkedPlans } from "./navigation-state";
import { useWorkspaceDirectory } from "./use-workspace-directory";
import { GraphPage } from "./graph-page";
import { contextPlanSources, resolveContextPlan, refreshContextSource } from "./navigation-state";
import { NativeFreshnessPill } from "./native-freshness";
import { resolvePlanLink, safeHttpsUrl } from "./plan-links";
import { taskTeamReader } from "./task-team-source";

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
  const keyboard = useKeyboardSettings();
  const [route, setRoute] = useState(() => hostNavigation(host.id).route);
  const [menu, setMenu] = useState<"more" | "context" | null>(null);
  const [pickerQuery, setPickerQuery] = useState("");
  const [rootSize, setRootSize] = useState({ width: 0, height: 0 });
  const rootRef = useRef<View | null>(null), moreRef = useRef<View | null>(null), contextRef = useRef<View | null>(null);
  const [query, setQuery] = useState(() => readRouteView(host.id, route).query);
  const [includeArchived, setIncludeArchived] = useState(() => readSessionView(sessionScopeKey(host.id, route)).includeArchived);
  // The Teams page is controlled by its view in the navigation store, which does not render by itself.
  const [, setTeamsViewRevision] = useState(0);
  const directory = useSessionDirectory({ hostId: host.id, hostStatus, includeArchived });
  const sourceState = useReadSources(host.id, hostStatus);
  const taskTeams = useMemo(() => taskTeamReader(sourceState.sources, sourceState.teamsSnapshots), [sourceState.sources, sourceState.teamsSnapshots]);
  const workspaces = useWorkspaceDirectory(host.id, hostStatus === "online");
  const presentation = sourceState.error && !sourceState.available
    ? { ...organization, state: { ...organization.state, status: "error" as const, error: "Plugin source service unavailable; organization preferences are not current." } }
    : organization;
  const toast = useToast();
  const visit = useCallback((patch: Partial<KontorRoute>, queryOverride?: string, prepare?: (next: KontorRoute) => void) => {
    const next = navigate(host.id, patch);
    if (queryOverride !== undefined) writeRouteView(host.id, next, { query: queryOverride, offset: 0 });
    prepare?.(next);
    setRoute({ ...next }); setQuery(readRouteView(host.id, next).query); setMenu(null);
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
  const sources = linkedContext ? sourcesInContext.filter((source) => source.kind !== "plan" || associationIds!.includes(source.id)) : sourcesInContext;
  const reads = Object.fromEntries(Object.entries(sourceState.reads).filter(([id]) => sources.some((source) => source.id === id)));
  const epics = [...new Set([...directory.state.entries.map(confirmedEpic), ...sourceState.sources.map((source) => sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic)].filter((key): key is string => key !== null))].sort();
  const projectName = route.projectId === null ? null : projects.find(([id]) => id === route.projectId)?.[1] ?? null;
  const projectLabel = route.epicKey ?? (route.projectId === null ? "All projects" : projectName ?? "Project unavailable");
  const heading = contextHeading(route, sourceState.sources, sourceState.reads, linkedContext ? associationIds : null, projectName);
  const pickerPlans = contextPlanSources(linkedContext ? { ...route, epicKey: null, projectId: null } : route, sourceState.sources, sourceState.reads, linkedContext ? associationIds : null);
  const visitContext = (patch: Partial<KontorRoute>, queryOverride?: string) => {
    const candidate = { ...hostNavigation(host.id).route, ...patch };
    const resolved = resolveContextPlan(candidate, sourceState.sources, sourceState.reads, candidate.originAgentId ? saved.planAssociations[candidate.originAgentId] ?? [] : null);
    visit({ ...patch, sourceId: resolved.sourceId, sourceRevision: resolved.sourceRevision }, queryOverride);
  };
  const selectSource = (sourceId: string, tab?: Destination) => {
    const source = sourceState.sources.find((candidate) => candidate.id === sourceId && candidate.kind === "plan");
    if (!source) return;
    // A global picker selection leaves the conversation's plan-association scope.
    visit({ ...(tab ? { tab } : {}), sourceId, sourceRevision: sourceState.reads[sourceId]?.revision ?? null,
      projectId: source.projectId, epicKey: sourceState.reads[sourceId]?.graph?.epic.jira ?? source.epic,
      originAgentId: linkedContext && associationIds?.includes(sourceId) ? route.originAgentId : null });
  };
  const primary = layout.compact ? PRIMARY_MOBILE : PRIMARY_DESKTOP;
  const tabPress = (tab: Destination) => visitContext({ tab, ...tabScope(host.id, tab) });
  const toggle = (kind: "more" | "context") => { setPickerQuery(""); setMenu(menu === kind ? null : kind); };
  const back = () => { const next = goBack(host.id); setRoute({ ...next }); setQuery(readRouteView(host.id, next).query); setMenu(null); };
  useBackShortcut(keyboard.state.status === "ready" ? keyboard.state.values.backShortcut : "off", layout.platform === "web", rootRef, () => hostNavigation(host.id).back.length > 0, back);
  const setSearch = (value: string) => { writeRouteView(host.id, route, { query: value, offset: 0 }); setQuery(value); };
  const open = (entry: (typeof entries)[number]) => {
    const result = openSessionById({ entries: directory.state.entries, id: entry.id, serverId: host.id, navigation: props.navigation });
    if (!result.ok) toast.error(result.message);
  };
  const openExternalLink = (target: string) => {
    const url = safeHttpsUrl(target);
    if (!url) { toast.error("This source link is not a valid HTTPS URL without credentials."); return; }
    void Linking.openURL(url).catch(() => toast.error("This device could not open the source link."));
  };
  // No file export exists in the plugin SDK; copying is the export, and a refusal says so.
  const copy = (text: string, what: string) => {
    void copyText(text).then(() => toast.show(`${what} copied`), () => toast.error(`${what} not copied: the clipboard is unavailable or denied on this device.`));
  };
  const openDocumentLink = (target: string) => {
    if (!selectedPlan) { toast.error("The selected plan source is unavailable."); return; }
    const linkedSources = associationIds ? sourceState.sources.filter((source) => associationIds.includes(source.id)) : sourceState.sources;
    const link = resolvePlanLink(target, selectedPlan, linkedSources, directory.state.entries);
    if (link.kind === "source") selectSource(link.id, "Plan");
    else if (link.kind === "session") { const entry = directory.state.entries.find((entry) => entry.id === link.id); if (entry) open(entry); }
    else if (link.kind === "url") openExternalLink(link.url);
    else toast.error(link.reason);
  };
  const conversationReturn = route.originAgentId && props.navigation ? <Action label="Return to conversation" colors={colors} disabled={!directory.state.entries.some((entry) => entry.id === route.originAgentId)} onPress={() => { const entry = directory.state.entries.find((candidate) => candidate.id === route.originAgentId); if (entry) open(entry); }} /> : null;
  const canGoBack = hostNavigation(host.id).back.length > 0;
  // Desktop keeps Back in place, disabled when there is nowhere to go, so the views beside it never move.
  const backButton = <IconButton label="Back" icon="ChevronLeft" colors={colors} disabled={!canGoBack} onPress={back} />;
  const contextPicker = route.tab !== "Sessions" ? <ContextPicker ref={contextRef} label={heading.title} epicKey={heading.key} compact={layout.compact} expanded={menu === "context"} colors={colors} onPress={() => toggle("context")} /> : null;
  const freshness = <NativeFreshnessPill state={directory.state} hostStatus={hostStatus} hostLabel={host.label} colors={colors} />;
  const moreActive = !primary.includes(route.tab);
  const navigationItems = <>
      {primary.map((tab) => <Action key={tab} tab stacked={layout.compact} colors={colors} icon={ICONS[tab]} label={tab} selected={route.tab === tab} onPress={() => tabPress(tab)} />)}
      <Action ref={moreRef} stacked={layout.compact} colors={colors} icon="Ellipsis" label="More" trailing={layout.compact ? undefined : "⌄"} accessibilityLabel={`More Kontor views${moreActive ? `, current: ${route.tab}` : ""}`} expanded={menu === "more"} selected={moreActive} onPress={() => toggle("more")} />
    </>;
  // A desktop strip inside the context row and a five-column compact bottom bar, as in the prototype.
  const navigation = layout.compact
    ? <View accessibilityRole="tablist" accessibilityLabel="Kontor views" style={{ flexDirection: "row", borderTopWidth: 1, borderColor: colors.border, backgroundColor: colors.surface0, gap: 2, paddingHorizontal: 6, paddingTop: 5, paddingBottom: 6 }}>{navigationItems}</View>
    : <View accessibilityRole="tablist" accessibilityLabel="Kontor views" style={{ flexShrink: 1, minWidth: 0 }}>
      <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={{ flexDirection: "row", alignItems: "center", gap: 3 }}>{navigationItems}</ScrollView>
    </View>;
  const needle = pickerQuery.trim().toLocaleLowerCase();
  const matches = (...values: (string | null | undefined)[]) => !needle || values.some((value) => value?.toLocaleLowerCase().includes(needle));
  const boundEpics = new Set(sourceState.sources.filter((source) => source.kind === "plan").map((source) => sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic));
  const planSources: Record<string, string[]> = {};
  for (const source of sourceState.sources) {
    const key = source.kind === "plan" ? (sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic)?.toLocaleUpperCase() : undefined;
    if (key) (planSources[key] ??= []).push(source.id);
  }
  const planChoices = pickerPlans.filter((source) => matches(source.title, sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic));
  const epicChoices = linkedContext ? [] : epics.filter((epicKey) => !boundEpics.has(epicKey) && matches(epicKey));
  const projectChoices = linkedContext ? [] : projects.filter(([id, name]) => matches(name, id));
  const group = (label: string) => <Text key={`group:${label}`} style={{ color: colors.foregroundMuted, fontSize: 10, fontWeight: "600", letterSpacing: 0.4, paddingHorizontal: 10, paddingTop: 8, paddingBottom: 3 }}>{label.toLocaleUpperCase()}</Text>;
  const unscoped = route.projectId === null && route.epicKey === null && route.sourceId === null;
  const menus = menu === "more" ? <AnchoredMenu testID="kontor-more-menu" anchor={moreRef} root={rootRef} rootSize={rootSize} colors={colors} label="More Kontor views" onClose={() => setMenu(null)}
    placement={layout.compact ? { align: "stretch", width: 0, gap: 6, maxHeight: Math.min(rootSize.height * 0.6, 430) } : { align: "end", width: 190 }}>
    {/* The frame is clamped to the screen, so a short screen scrolls the views instead of cutting off the last ones. */}
    <ScrollView testID="kontor-more-choices" keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
      {DESTINATIONS.filter((tab) => !primary.includes(tab)).map((tab) => <MenuItem key={tab} label={tab} tall={layout.compact} selected={route.tab === tab} colors={colors} onPress={() => tabPress(tab)} />)}
    </ScrollView>
  </AnchoredMenu> : menu === "context" ? <AnchoredMenu testID="kontor-context-menu" dialog anchor={contextRef} root={rootRef} rootSize={rootSize} colors={colors} label="Choose plan or project" onClose={() => setMenu(null)}
    placement={layout.compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.7, 480) } : { align: "start", width: 380, maxHeight: 460 }}
    header={<TextInput accessibilityLabel="Search plans, epics and projects" placeholder="Search plans, epics, projects…" autoFocus={!layout.compact} value={pickerQuery} onChangeText={setPickerQuery}
      placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 12, minHeight: 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6, marginBottom: 4 }} />}>
    <ScrollView role="menu" accessibilityLabel="Plan and project choices" keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
      <MenuItem label="All projects" accessibilityLabel="All projects, clear selection" tall={layout.compact} selected={unscoped} colors={colors} onPress={() => visit({ ...NO_SCOPE })} />
      {planChoices.length ? group(linkedContext ? "Plans linked to this conversation" : "Configured plans") : null}
      {planChoices.map((source) => { const key = sourceState.reads[source.id]?.graph?.epic.jira ?? source.epic; return <MenuItem key={`source:${source.id}`} label={source.title} detail={key && jiraIssueUrl(key) ? key : null} accessibilityLabel={`Plan: ${source.title}${key ? `, ${key}` : ""}`} tall={layout.compact} selected={route.sourceId === source.id} colors={colors} onPress={() => selectSource(source.id)} />; })}
      {epicChoices.length ? group("Epics without a configured plan") : null}
      {epicChoices.map((epicKey) => { const count = directory.state.entries.filter((entry) => confirmedEpic(entry) === epicKey).length; return <MenuItem key={`epic:${epicKey}`} label={epicKey} detail={`${count} session${count === 1 ? "" : "s"}`} tall={layout.compact} selected={route.sourceId === null && route.epicKey === epicKey} colors={colors} onPress={() => visitContext({ projectId: null, epicKey, sourceId: null, sourceRevision: null, originAgentId: null })} />; })}
      {projectChoices.length ? group("Native projects") : null}
      {projectChoices.map(([id, name]) => <MenuItem key={`project:${id}`} label={name} tall={layout.compact} selected={route.projectId === id && route.epicKey === null && route.sourceId === null} colors={colors} onPress={() => visit({ projectId: id, epicKey: null, sourceId: null, originAgentId: null })} />)}
      {!planChoices.length && !epicChoices.length && !projectChoices.length && needle ? <Text style={{ color: colors.foregroundMuted, fontSize: 12, padding: 10 }}>No plan, epic or project matches “{pickerQuery.trim()}”.</Text> : null}
    </ScrollView>
  </AnchoredMenu> : null;
  let page;
  if (route.tab === "Sessions") page = <SessionsScreen key={sessionScopeKey(host.id, route)} viewScope={sessionScopeKey(host.id, route)} {...props} theme={{ ...props.theme, colors }} directory={directory} organization={presentation} projectId={route.projectId} epicKey={route.epicKey} onIncludeArchived={setIncludeArchived}
    scopeLabel={route.epicKey ?? (route.projectId === null ? null : projectName ?? "Project unavailable")} onClearScope={() => visit({ ...NO_SCOPE })}
    planSources={planSources} onEpicTab={(tab, epicKey, sourceId) => visitContext({ tab, epicKey, sourceId, sourceRevision: sourceId ? sourceState.reads[sourceId]?.revision ?? null : null, originAgentId: null })}
    workspaces={workspaces.entries.filter((workspace) => (route.projectId === null || workspace.projectId === route.projectId) && (route.epicKey === null || saved.displayMappings[`workspace:${workspace.id}`]?.epic?.trim().toLocaleLowerCase() === route.epicKey.trim().toLocaleLowerCase())).map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name, projectKey: workspace.projectId, projectName: workspace.projectCustomName ?? workspace.projectDisplayName }))}
    workspaceNote={workspaces.error ? `Workspace list unavailable: ${workspaces.error}` : workspaces.readAt ? `Empty workspaces from the workspace list read ${new Date(workspaces.readAt).toLocaleTimeString()}` : "Reading the workspace list…"} />;
  else if (route.tab === "Settings") page = <OrganizationSettingsPage colors={colors} state={presentation.state} entries={directory.state.entries} workspaces={workspaces.entries.map((workspace) => ({ id: workspace.id, name: workspace.title ?? workspace.name }))} sources={sourceState.sources} reads={sourceState.reads} teamsSnapshots={sourceState.teamsSnapshots} teamsErrors={sourceState.teamsErrors} sourcesAvailable={sourceState.available} sourceError={sourceState.error} refresh={() => { void sourceState.refresh(); }}
    onOpenTeams={() => visit({ tab: "AI teams & sessions", ...tabScope(host.id, "AI teams & sessions") }, undefined, (next) => writePageView(host.id, next, { ...readPageView(host.id, next, DEFAULT_TEAMS_VIEW), configured: true }))}
    keyboard={<KeyboardSettingsCard state={keyboard.state} save={keyboard.save} colors={colors} web={layout.platform === "web"} root={rootRef} copy={copy} />} />;
  else if (route.tab === "Board" || route.tab === "Graph") page = <GraphPage key={routeViewKey(route)} mode={route.tab} hostId={host.id} theme={{ ...props.theme, colors }} compact={layout.compact} sources={sources} selectedId={route.sourceId} reads={reads} available={sourceState.available} error={sourceState.error} epicKey={scopedEpic} onBoard={() => visit({ tab: "Board" })}
    initialOffset={readRouteView(host.id, route).offset} onScroll={(offset) => writeRouteView(host.id, route, { offset })}
    onChooseSource={() => toggle("context")} onCopy={copy}
    onSelect={selectSource} onPlan={(sourceId, taskId) => {
      const revision = sourceState.reads[sourceId]?.revision ?? null;
      // A task link carries the exact declaration to reveal, never a search query.
      visit({ tab: "Plan", sourceId, sourceRevision: revision, originAgentId: route.originAgentId }, "", taskId ? (next) => writePageView(host.id, next, { ...readPageView(host.id, next, DEFAULT_PLAN_VIEW), target: { taskId, revision }, revealed: false }) : undefined);
    }}
    onIssue={(key) => { const url = jiraIssueUrl(key); if (url) void Linking.openURL(url).catch(() => toast.error("This device could not open the verified Jira link.")); }}
    sessions={{ state: directory.state, onOpen: open,
      onOpenWorkspace: props.navigation ? (workspaceId) => props.navigation?.openWorkspace({ workspaceId, serverId: host.id }) : undefined,
      // Sessions opens unscoped with the task's exact label join, never a text search; Back returns to Board.
      onShowInSessions: (task) => visit({ tab: "Sessions", ...NO_SCOPE }, undefined, (next) => {
        const scope = sessionScopeKey(host.id, next);
        patchSessionView(scope, sessionViewTransition(readSessionView(scope), { task, query: "", status: "all", scrollOffset: 0 }));
      }) }} taskTeams={taskTeams} />;
  else page = <ReadOnlyPage key={routeViewKey(route)} compact={layout.compact} onChooseSource={() => toggle("context")} onDocumentLink={openDocumentLink} onCopy={copy} onExternalLink={openExternalLink} contextLabel={route.sourceId ? selectedPlan?.title ?? "Source unavailable" : projectLabel} tab={route.tab} colors={colors} entries={entries} directory={directory.state} sources={sources} reads={reads} sourceId={route.sourceId}
    teamsSources={sourceState.sources.filter((source) => source.kind === "teams")} teamsJsonReads={Object.fromEntries(Object.entries(sourceState.reads).filter(([, read]) => read.source.kind === "teams"))}
    teamsSnapshots={sourceState.teamsSnapshots} teamsErrors={sourceState.teamsErrors} teamsBindingDiagnostics={sourceState.teamsBindingDiagnostics} teamsAvailable={sourceState.available}
    sourceError={sourceState.error} query={query} setQuery={setSearch} onSource={(sourceId) => selectSource(sourceId, "Plan")} onOpen={open} onTab={(tab) => visit({ tab, originAgentId: null })}
    onEpicTab={(tab, epicKey, sourceId) => visitContext({ tab, epicKey, sourceId, sourceRevision: sourceId ? sourceState.reads[sourceId]?.revision ?? null : null, originAgentId: null })}
    scoped={!unscoped} onGraph={() => visit({ tab: "Graph" })}
    planView={readPageView(host.id, route, DEFAULT_PLAN_VIEW)} onPlanView={(view) => writePageView(host.id, route, view)}
    teamsView={readPageView(host.id, route, DEFAULT_TEAMS_VIEW)} onTeamsView={(view) => { writePageView(host.id, route, view); setTeamsViewRevision((revision) => revision + 1); }}
    initialOffset={readRouteView(host.id, route).offset} onScroll={(offset) => { writeRouteView(host.id, route, { offset }); }} />;
  return <SafeAreaView style={{ flex: 1, backgroundColor: colors.surface0 }}><KeyboardAvoidingView behavior={layout.platform === "ios" ? "padding" : undefined} style={{ flex: 1 }}>
    <View ref={rootRef} style={{ flex: 1 }} onLayout={(event) => { const { width, height } = event.nativeEvent.layout; if (width !== rootSize.width || height !== rootSize.height) setRootSize({ width, height }); }}>
    {/* Paseo's own header above already names Kontor and, when there is a choice, the host; this row adds only what it cannot show. */}
    {layout.compact ? <View testID="kontor-context-row" style={{ flexDirection: "row", alignItems: "center", gap: 5, paddingHorizontal: 6, paddingVertical: 3, minHeight: 51, borderBottomWidth: 1, borderColor: colors.border, backgroundColor: colors.surface0 }}>
      {canGoBack ? backButton : null}
      {contextPicker}
      <View style={{ flex: 1 }} />
      {freshness}
    </View> : <View testID="kontor-context-row" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 12, paddingHorizontal: 10, paddingVertical: 3, minHeight: 51, borderBottomWidth: 1, borderColor: colors.border, backgroundColor: colors.surface0 }}>
      {/* One row when it fits; on a narrow screen the context, status and return wrap below the views instead of hiding. */}
      <View testID="kontor-views-group" style={{ flexDirection: "row", alignItems: "center", gap: 8, flexShrink: 1, minWidth: 0, maxWidth: "100%", minHeight: 44 }}>
        {backButton}
        {navigation}
      </View>
      <View testID="kontor-context-group" style={{ flexDirection: "row", alignItems: "center", gap: 8, flexGrow: 1, flexShrink: 1, minWidth: 0, minHeight: 44 }}>
        {contextPicker}
        <View style={{ flexGrow: 1 }} />
        {freshness}
        {conversationReturn}
      </View>
    </View>}
    {layout.compact && conversationReturn ? <View testID="kontor-conversation-return-row" style={{ minHeight: 44, flexDirection: "row", justifyContent: "flex-end" }}>{conversationReturn}</View> : null}
    <View style={{ flex: 1 }}>{page}</View>
    {layout.compact ? navigation : null}
    {menus}
    </View>
  </KeyboardAvoidingView></SafeAreaView>;
}
/** Bounded, left-aligned title-only selector; the confirmed Jira key floats on its lower border. */
function ContextPicker({ label, epicKey, compact, expanded, colors, onPress, ref }: { label: string; epicKey: string | null; compact: boolean; expanded: boolean; colors: Colors; onPress: () => void; ref?: Ref<View> }) {
  return <Pressable ref={ref} testID="kontor-context-picker" accessibilityRole="button" accessibilityLabel={`Plan / project: ${epicKey ? `${epicKey} · ` : ""}${label}`} accessibilityHint="Choose a plan, epic or project" accessibilityState={{ expanded }} aria-expanded={expanded} onPress={onPress}
    style={{ flexGrow: 1, flexShrink: 1, flexBasis: compact ? 180 : 220, minWidth: compact ? 44 : 100, maxWidth: 420, height: 44, justifyContent: "center", paddingLeft: 9, paddingRight: 26 }}>
    <View pointerEvents="none" style={{ position: "absolute", top: 5, bottom: 8, left: 0, right: 0, borderWidth: 1, borderColor: expanded ? colors.accent : colors.border, borderRadius: 6, backgroundColor: colors.surface1 }} />
    <Text numberOfLines={1} ellipsizeMode="tail" style={{ color: colors.foreground, fontSize: 12, lineHeight: 16, fontWeight: "600", marginBottom: 3 }}>{label}</Text>
    {epicKey ? <Text testID="kontor-context-epic" accessible={false} pointerEvents="none" numberOfLines={1} style={{ position: "absolute", left: 7, bottom: 2, maxWidth: "80%", paddingHorizontal: 4, fontSize: 9, lineHeight: 12, fontFamily: "monospace", color: colors.foregroundMuted, backgroundColor: colors.surface1 }}>{epicKey}</Text> : null}
    <View pointerEvents="none" style={{ position: "absolute", right: 8, top: 13 }}><Icon name="ChevronDown" size={14} color={colors.foregroundMuted} /></View>
  </Pressable>;
}
