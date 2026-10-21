import { useMemo, useRef, useState, type ReactNode } from "react";
import type { FlatList as NativeFlatList } from "react-native";
import { Pressable, Text, View } from "react-native";
import { FlatList, ScrollView } from "@getpaseo/plugin/client/react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import type { JiraIssue, ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { PlanGraph, createPlanGraphStore, planGraphViewKey, type PlanGraphStore } from "./plan-graph/graph";
import { validatePlanGraph } from "./plan-graph/contract";
import { SourceDetails } from "./source-details";
import { historicalPlanProgress } from "./page-data";
import { Action, COARSE_POINTER, Copy, type Colors } from "./page-controls";
import { directSession, sessionsLive, TaskSessionsChooser, TaskSessionsControl, type BoardSessions } from "./board-sessions";
import { taskSessions, type TaskLink } from "./session-results";
import type { SessionTaskFilter } from "./session-view-store";
import type { TaskTeamRead, TaskTeamReader } from "./task-teams";

const stores = new Map<string, PlanGraphStore>();
function retainedStore(hostId: string): PlanGraphStore {
  let store = stores.get(hostId);
  if (!store) { store = createPlanGraphStore(); stores.set(hostId, store); }
  return store;
}
type GraphNode = NonNullable<SourceRead["graph"]>["nodes"][number];
export type CapturedJiraObservation = { status: string; cat: JiraIssue["cat"]; observedAt: string; sourceId: string; sourceTitle: string };
export type CapturedJiraRecord = { key: string; observations: CapturedJiraObservation[] };
/** Exact-key child records of one epic from ready historical projections. The projection carries no titles. */
export function capturedJiraChildren(epic: string | null | undefined, reads: readonly SourceRead[]): { found: boolean; records: CapturedJiraRecord[] } {
  const records = new Map<string, CapturedJiraRecord>();
  let found = false;
  if (!epic || !/^ASMA-[1-9]\d*$/.test(epic)) return { found, records: [] };
  for (const read of reads) {
    if (read.source.kind !== "jira" || read.status !== "ready" || !read.jiraHistory) continue;
    const parent = read.jiraHistory.epics.find((value) => value.key === epic);
    if (!parent) continue;
    found = true;
    for (const child of parent.children) {
      const record = records.get(child.key) ?? { key: child.key, observations: [] };
      record.observations.push({ status: child.status, cat: child.cat, observedAt: read.jiraHistory.generated, sourceId: read.source.id, sourceTitle: read.source.title });
      records.set(child.key, record);
    }
  }
  return { found, records: [...records.values()] };
}
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/** "18 Oct" from the capture's own calendar date; anything unparseable is shown as captured. */
export function captureDay(value: string): string {
  const match = /^\d{4}-(\d{2})-(\d{2})/.exec(value);
  const month = match ? MONTHS[Number(match[1]) - 1] : undefined;
  return match && month ? `${Number(match[2])} ${month}` : value;
}
const captureDays = (values: readonly string[]) => [...new Set(values.map(captureDay))].join(" / ");
type Tone = JiraIssue["cat"] | "unknown";
/** One line per row: agreeing captures share one status; disagreeing captures are all listed, never resolved. */
function capturedState(observations: readonly CapturedJiraObservation[]): { short: string; full: string; tone: Tone } {
  const first = observations[0];
  const full = observations.map((value) => `${value.status} (${value.cat}), captured ${value.observedAt} from ${value.sourceTitle}`).join("; ");
  if (!first) return { short: "Unknown", full: "Status Unknown", tone: "unknown" };
  if (observations.every((value) => value.status === first.status && value.cat === first.cat)) {
    return { short: `${first.status} (${first.cat}) · captured ${captureDays(observations.map((value) => value.observedAt))}`, full, tone: first.cat };
  }
  return { short: `Captures disagree: ${observations.map((value) => `${value.status} (${value.cat}) ${captureDay(value.observedAt)}`).join(" / ")}`, full: `Captures disagree: ${full}`, tone: "unknown" };
}

export type BoardItem =
  | { kind: "section"; id: string; title: string; note: string }
  | { kind: "task"; id: string; node: GraphNode }
  | { kind: "jira"; id: string; record: CapturedJiraRecord };

export function GraphPage({ mode, hostId, theme, compact, sources, selectedId, reads, available, error, epicKey, onPlan, onIssue, initialOffset = 0, onScroll, onChooseSource, onBoard, onCopy, sessions, taskTeams }: {
  mode: "Board" | "Graph"; hostId: string; theme: PluginTheme; compact: boolean;
  sources: readonly ReadSourceSummary[]; selectedId: string | null; reads: Readonly<Record<string, SourceRead>>;
  available: boolean; error: string | null; onSelect: (sourceId: string) => void;
  /** Context epic for captured Jira records when no valid canonical graph supplies one. */
  epicKey?: string | null;
  onPlan: (sourceId: string, taskId?: string) => void;
  onIssue: (key: string) => void;
  initialOffset?: number; onScroll?: (offset: number) => void;
  onChooseSource?: () => void;
  /** Copies text to this client's clipboard; Graph offers it for the canonical graph. */
  onCopy?: (text: string, what: string) => void;
  /** Opens Board for the same context; offered when Graph has no valid canonical graph. */
  onBoard?: () => void;
  /** Native sessions for exact task-label joins on Board rows. */
  sessions?: BoardSessions;
  /** A task's declared team, workflow and risk tier, or what the configured sources cover instead. */
  taskTeams?: TaskTeamReader;
}) {
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [chooser, setChooser] = useState<string | null>(null);
  const [rootSize, setRootSize] = useState({ width: 0, height: 0 });
  const root = useRef<View | null>(null);
  const chooserAnchor = useRef<View | null>(null);
  const sessionButtons = useRef(new Map<string, View>());
  const list = useRef<NativeFlatList<BoardItem> | null>(null);
  const pendingRestore = useRef(initialOffset > 0);
  const store = useMemo(() => retainedStore(hostId), [hostId]);
  const colors = theme.colors;
  const read = selectedId ? reads[selectedId] : undefined;
  const graph = read?.graph;
  const validated = useMemo(() => graph && read?.revision ? validatePlanGraph(graph, {
    source: graph.source, source_sha256: read.revision, graph_sha256: graph.graph_sha256,
  }) : null, [graph, read?.revision]);
  const valid = read && validated?.ok ? validated.graph : null;
  const allReads = Object.values(reads);
  const jiraReads = allReads.filter((value) => value.source.kind === "jira");
  const readyJira = jiraReads.filter((value) => value.status === "ready" && value.jiraHistory);
  const epic = (valid ? valid.epic.jira : null) ?? epicKey ?? read?.source.epic ?? null;
  const captured = capturedJiraChildren(epic, readyJira);
  const progress = valid ? historicalPlanProgress(valid, allReads) : null;
  const evidence = new Map(progress?.states.flatMap(({ node, evidence: value }) => value ? [[node.id, value] as const] : []) ?? []);
  const historicalStates = Object.fromEntries([...evidence].map(([id, value]) => [id, { label: `${value.issue.status} (historical)`, source: value.observations[0]!.sourceTitle, observedAt: value.observations[0]!.observedAt }]));
  const captureTimes = [...new Set([...captured.records, ...evidence.values()].flatMap((value) => value.observations.map((observation) => observation.observedAt)))];
  const details = <>
    {read ? <SourceDetails read={read} colors={colors} expanded /> : null}
    {jiraReads.map((value) => <SourceDetails key={value.source.id} read={value} colors={colors} expanded />)}
    {validated && !validated.ok ? validated.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic.path}: {diagnostic.message}</Copy>) : null}
  </>;
  const notices = <>
    {error ? <Copy colors={colors}>{error}</Copy> : null}
    {!available ? <Copy colors={colors}>Source service unavailable. Retained exports are stale.</Copy> : null}
  </>;

  if (mode === "Graph" && valid && read) return <View style={{ flex: 1 }}>
    {error || !available ? <View style={{ paddingHorizontal: 12, paddingTop: 8 }}>{notices}</View> : null}
    <PlanGraph graph={valid} hostId={hostId} theme={theme} store={store} compact={compact} onCopy={onCopy}
      sourcedStates={evidence.size ? { viewKey: planGraphViewKey(hostId, valid), nodes: historicalStates } : undefined}
      sourceActions={{ plan: (action) => { if (action.kind === "plan" && action.source === valid.source && action.source_sha256 === read.revision && valid.nodes.some((node) => node.id === action.taskId)) onPlan(read.source.id, action.taskId); },
        jira: (action) => { if (action.kind === "jira" && valid.nodes.some((node) => node.jira === action.jira)) onIssue(action.jira); } }}
      details={<View style={{ gap: 6, paddingTop: 4 }}>
        <Copy colors={colors} small muted>Historical Jira: {evidence.size}/{valid.nodes.length} tasks observed · capture {captureTimes.join(" / ") || "Not captured"} · current completion Unknown</Copy>
        {details}
        <Action colors={colors} small={!compact} label="Open source plan" onPress={() => onPlan(read.source.id)} />
      </View>} />
  </View>;

  const plans = sources.filter((source) => source.kind === "plan");
  const toggle = <Action colors={colors} small={!compact} label="Source details" expanded={evidenceOpen} onPress={() => setEvidenceOpen(!evidenceOpen)} />;
  const actions = <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
    {read ? <Action colors={colors} small={!compact} label="Open source plan" onPress={() => onPlan(read.source.id)} /> : null}
    {!read && onChooseSource ? <Action colors={colors} small={!compact} label="Choose plan" onPress={onChooseSource} /> : null}
    {mode === "Graph" && onBoard && epic ? <Action colors={colors} small={!compact} label="Inspect captured tasks" onPress={onBoard} /> : null}
    {read || jiraReads.length ? toggle : null}
  </View>;
  let sourceNote: string | null = null;
  if (!read) {
    if (error) sourceNote = "Plan sources unavailable. See source diagnostics.";
    else if (selectedId) sourceNote = "Selected source unavailable in this context.";
    else if (plans.length > 1) sourceNote = "Several plans are linked here. Choose an exact source.";
    else sourceNote = epic ? "No plan selected. Plan declarations and dependencies are unavailable." : "No plan or epic in this context. Select a plan in the top context picker.";
  }
  else if (!valid) sourceNote = "No valid exact-source canonical graph available. No legacy text extraction or inferred task state.";
  let jiraNote: string | null = null;
  if (!epic) jiraNote = read ? "No epic linked to this plan; captured Jira tasks cannot be matched." : null;
  else if (!jiraReads.length) jiraNote = "No historical Jira source is configured in this context.";
  else if (!readyJira.length) jiraNote = "Historical Jira source unavailable or partial. See Source details.";
  else if (!captured.found) jiraNote = `${epic} is not in the captured Jira projection. This is not evidence that the epic has no tasks.`;
  else if (!captured.records.length) jiraNote = `No child task records were captured for ${epic}. This is not evidence that the epic has no tasks.`;

  if (mode === "Graph") return <ScrollView contentContainerStyle={{ padding: 12, gap: 8 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 15 }}>Dependency graph unavailable{read ? ` · ${read.source.title}` : ""}</Text>
    {notices}
    {sourceNote ? <Copy colors={colors}>{sourceNote}</Copy> : null}
    {!read && plans.length > 1 ? <Copy colors={colors}>Several explicitly linked plans; select an exact source.</Copy> : null}
    <Copy colors={colors} muted>No dependencies are inferred from Markdown, Mermaid or ASCII diagrams.{epic && captured.records.length ? " Captured Jira tasks remain available on Board." : ""}</Copy>
    {actions}
    {evidenceOpen ? details : null}
  </ScrollView>;

  const declared = new Set(valid?.nodes.flatMap((node) => node.jira ? [node.jira] : []) ?? []);
  const undeclared = valid ? captured.records.filter((record) => !declared.has(record.key)) : captured.records;
  const items: BoardItem[] = [];
  if (valid) {
    items.push({ kind: "section", id: "section:plan", title: `Plan tasks · ${valid.nodes.length}`, note: "Canonical declarations in this exact plan revision. Jira state appears only where the task's exact key was captured." });
    for (const node of valid.nodes) items.push({ kind: "task", id: `task:${node.id}`, node });
    if (undeclared.length) items.push({ kind: "section", id: "section:undeclared", title: `Captured Jira tasks not declared in this plan · ${undeclared.length}`, note: `Children of ${epic} in the historical capture whose exact key no plan task declares. No plan declaration or dependencies.` });
  } else if (undeclared.length) items.push({ kind: "section", id: "section:captured", title: `Captured Jira tasks · ${undeclared.length}`, note: "Plan declaration and dependencies: unavailable without a valid canonical plan graph." });
  for (const record of undeclared) items.push({ kind: "jira", id: `jira:${record.key}`, record });

  const truth = "Jira states are dated historical captures. Current completion and readiness: Unknown.";
  // Exact label joins only (see taskSessions); a row without a key to join on links nothing.
  const live = sessions ? sessionsLive(sessions.state) : false;
  const taskOf = (item: BoardItem): SessionTaskFilter | null => item.kind === "task"
    ? (item.node.jira || item.node.kontor_id ? { jira: item.node.jira, kontorId: item.node.kontor_id, label: item.node.jira ?? item.node.kontor_id ?? item.node.id } : null)
    : item.kind === "jira" ? { jira: item.record.key, kontorId: null, label: item.record.key } : null;
  const links = new Map<string, TaskLink[]>();
  const teams = new Map<string, TaskTeamRead>();
  if (sessions) for (const item of items) {
    const task = taskOf(item);
    if (!task) continue;
    links.set(item.id, taskSessions(sessions.state.entries, task));
    if (taskTeams) teams.set(item.id, taskTeams(task));
  }
  const linkedTasks = [...links.values()].filter((value) => value.length > 0).length;
  const declaredOf = (id: string) => { const team = teams.get(id); return team?.state === "declared" ? team.declaration : null; };
  const declaredTasks = [...teams.keys()].filter((id) => declaredOf(id) !== null).length;
  // A risk tier alone is not a team; the row still shows it.
  const teamTasks = [...teams.keys()].filter((id) => { const team = declaredOf(id); return !!team && (team.team !== null || team.roles.length > 0); }).length;
  const declaringSources = [...new Set([...teams.values()].flatMap((team) => team.state === "declared" ? [team.declaration.source] : []))];
  const coverage = [...teams.values()].flatMap((team) => team.state === "not-provided" ? [team.coverage] : [])[0] ?? null;
  // How sessions and teams were read stays in Source details; the Board shows only what they say.
  const sessionNotes = sessions && links.size ? [
    "Linked sessions: a session links to a task only when its jira.issue or task label equals the task's Jira key, or its kontor.ticket label equals its Kontor ticket. Titles are not read; a role comes from the session's role label.",
    declaredTasks ? `Team, workflow and risk tier: declared for ${declaredTasks} of ${links.size} tasks by ${declaringSources.join(", ")}.` : coverage,
  ].filter((note): note is string => !!note) : [];
  const days = captureDays(captureTimes);
  const notes = [sourceNote, !read && plans.length > 1 ? "Several explicitly linked plans; select an exact source." : null, jiraNote];
  const noteCopy = (values: readonly (string | null)[]) => values.map((value, index) => value ? <Copy key={index} colors={colors}>{value}</Copy> : null);
  // The context picker already shows the plan and epic; the full title stays in the header's label.
  const title = `Board${epic ? ` · ${epic}` : ""}${read ? ` · ${read.source.title}` : ""}`;
  // List-first at every width: one title line and one meta line. The truth statement and the notes
  // stay reachable in Source details (and in the meta line's label); empty states keep their notes.
  let jiraCount = "Jira not captured";
  if (captured.found) jiraCount = captured.records.length ? `${captured.records.length} Jira` : "no Jira children captured";
  const header = <View style={{ paddingHorizontal: compact ? 8 : 12, paddingVertical: compact ? 8 : 10, gap: 4 }}>
    <Text accessibilityRole="header" accessibilityLabel={title} numberOfLines={1} style={{ color: colors.foreground, fontWeight: "600", fontSize: 15 }}>Board</Text>
    {notices}
    <Text testID="board-meta" numberOfLines={1} accessibilityLabel={`Read-only capture. ${captured.records.length} captured Jira tasks. ${valid ? `${valid.nodes.length} plan tasks` : "Plan graph unavailable"}.${captureTimes.length ? ` Captured ${captureTimes.join(" / ")}.` : ""} ${truth}`}
      style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>
      {`${jiraCount} · ${valid ? `${valid.nodes.length} plan tasks` : "no plan graph"}${days ? ` · captured ${days}` : ""}`}
    </Text>
    {items.length ? null : noteCopy(notes)}
    {actions}
    {evidenceOpen ? <View testID="board-source-notes" style={{ gap: 4 }}>
      <Copy colors={colors} small muted>{truth}</Copy>
      {items.length ? noteCopy(notes) : null}
      {items.flatMap((item) => item.kind === "section" ? [<Copy key={item.id} colors={colors} small muted>{`${item.title}: ${item.note}`}</Copy>] : [])}
      {sessionNotes.map((note) => <Copy key={note} colors={colors} small muted>{note}</Copy>)}
      {details}
    </View> : null}
  </View>;
  const planSource = read?.source.id;
  const sessionControl = (item: BoardItem) => {
    const linked = links.get(item.id) ?? [];
    const team = declaredOf(item.id);
    if (!sessions || (!linked.length && !team)) return null;
    const open = () => {
      const direct = directSession(linked, team);
      if (direct) { sessions.onOpen(direct); return; }
      chooserAnchor.current = sessionButtons.current.get(item.id) ?? null;
      setChooser(item.id);
    };
    return <TaskSessionsControl testID={`${item.id}-sessions`} links={linked} team={team} live={live} compact={compact} colors={colors} onPress={open}
      buttonRef={(node) => { if (node) sessionButtons.current.set(item.id, node); else sessionButtons.current.delete(item.id); }} />;
  };
  const chosen = typeof chooser === "string" ? items.find((item) => item.id === chooser) : undefined;
  const chosenTask = chosen ? taskOf(chosen) : null;
  const chooserLayer = sessions && chosen && chosenTask ? <TaskSessionsChooser taskKey={chosenTask.label} task={chosenTask} links={links.get(chosen.id) ?? []} team={teams.get(chosen.id)} live={live}
    archivedIncluded={sessions.state.includeArchived} anchor={chooserAnchor} root={root} rootSize={rootSize} compact={compact} colors={colors} sessions={sessions} onClose={() => setChooser(null)} /> : null;
  let sessionsMeta: string | null = null;
  if (sessions && links.size) {
    const read = sessions.state;
    if (!read.entries.length && (read.status === "loading" || read.status === "idle")) sessionsMeta = "Linked sessions: reading…";
    else if (!read.entries.length && read.status === "error") sessionsMeta = "Linked sessions unavailable: the session list could not be read";
    else sessionsMeta = `${linkedTasks ? `${linkedTasks} of ${links.size} tasks with sessions` : "No task has a linked session"}${teamTasks ? ` · ${teamTasks} with a declared team` : ""}${live ? "" : " · last known"}`;
  }
  const sessionsHeader = sessionsMeta ? <Text testID="board-sessions-meta" numberOfLines={1}
    accessibilityLabel={`${sessionsMeta}. Source details say how sessions and teams are read.`}
    style={{ paddingHorizontal: compact ? 8 : 12, paddingBottom: compact ? 4 : 6, color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>{sessionsMeta}</Text> : null;
  const listElement = <FlatList ref={list} data={items} keyExtractor={(item) => item.id} initialNumToRender={20} windowSize={7}
    onContentSizeChange={(_width, height) => {
      if (!pendingRestore.current || height <= 0) return;
      pendingRestore.current = false;
      list.current?.scrollToOffset({ offset: Math.max(0, initialOffset), animated: false });
    }}
    onScrollBeginDrag={() => { pendingRestore.current = false; }}
    onScroll={(event) => {
      const offset = Math.max(0, event.nativeEvent.contentOffset.y);
      if (pendingRestore.current && offset === 0) return;
      pendingRestore.current = false; onScroll?.(offset);
    }} scrollEventThrottle={100}
    contentContainerStyle={{ paddingBottom: 24 }} ListHeaderComponent={sessionsHeader ? <>{header}{sessionsHeader}</> : header} extraData={{ chooser, live, sessions: sessions?.state }} renderItem={({ item }) => {
      if (item.kind === "section") return <View style={{ paddingHorizontal: compact ? 8 : 12, paddingTop: compact ? 8 : 10, paddingBottom: compact ? 4 : 6, gap: 2 }}>
        <Text accessibilityRole="header" numberOfLines={compact ? 1 : undefined} accessibilityHint={item.note} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{item.title}</Text>
      </View>;
      if (item.kind === "jira") {
        const key = item.record.key;
        const state = capturedState(item.record.observations);
        const declaration = valid ? "Not declared in this plan revision" : "Plan declaration and dependencies unavailable";
        // The section header says whether rows are declared; exact capture times, sources and the declaration stay in the label.
        const rowTitle = <>{key}<Text style={{ fontWeight: "400", color: colors.foregroundMuted }}> · title not captured</Text></>;
        const description = `${key}: title unavailable in the captured Jira projection. ${state.full}. ${declaration}.`;
        if (compact) return <CompactBoardRow testID={`board-jira-${key}`} colors={colors} tone={state.tone} meta={state.short}
          title={rowTitle} label={description} issue={{ key, open: () => onIssue(key) }} sessions={sessionControl(item)} />;
        return <BoardRow testID={`board-jira-${key}`} colors={colors} title={rowTitle} lines={[state.short]} label={description} footer={sessionControl(item)}
          actions={<Action colors={colors} small label="Open issue" accessibilityLabel={`Open issue ${key}`} onPress={() => onIssue(key)} />} />;
      }
      const { node } = item;
      if (!planSource) return null;
      const evidenceValue = evidence.get(node.id);
      const openPlan = () => onPlan(planSource, node.id);
      const issueKey = node.jira;
      let state = { short: "Unknown · no Jira key", full: "No Jira binding declared, status Unknown", tone: "unknown" as Tone };
      if (evidenceValue) state = capturedState(evidenceValue.observations.map((value) => ({ status: evidenceValue.issue.status, cat: evidenceValue.issue.cat, observedAt: value.observedAt, sourceId: value.sourceId, sourceTitle: value.sourceTitle })));
      else if (issueKey) state = { short: "Unknown · no consistent capture", full: `Jira ${issueKey}: no consistent captured record, status Unknown`, tone: "unknown" };
      const after = node.depends_on.length ? ` · after ${node.depends_on.join(", ")}` : "";
      const meta = `${node.id} · ${state.short} · ${node.phase}${after}`;
      // Depth, an empty prerequisite list and the exact capture stay in the label.
      const description = `${node.id}: ${node.title}. ${state.full}. ${node.phase}, dependency depth ${node.wave}. ${node.depends_on.length ? `After ${node.depends_on.join(", ")}` : "No prerequisites declared"}.${issueKey ? ` Jira ${issueKey}.` : ""}`;
      if (compact) return <CompactBoardRow testID={`board-task-${node.id}`} colors={colors} tone={state.tone} title={`${issueKey ?? node.id} · ${node.title}`}
        meta={meta} onPress={openPlan} label={`Open plan task ${description}`}
        issue={issueKey ? { key: issueKey, open: () => onIssue(issueKey) } : undefined} sessions={sessionControl(item)} />;
      return <BoardRow testID={`board-task-${node.id}`} colors={colors} title={`${issueKey ?? node.id} · ${node.title}`} lines={[meta]} label={description} footer={sessionControl(item)}
        actions={<>
          <Action colors={colors} small label="Open in plan" accessibilityLabel={`Open plan task ${node.id}`} onPress={openPlan} />
          {issueKey ? <Action colors={colors} small label="Open issue" accessibilityLabel={`Open issue ${issueKey}`} onPress={() => onIssue(issueKey)} /> : null}
        </>} />;
    }} />;
  return <View ref={root} style={{ flex: 1 }} onLayout={(event) => {
    const { width, height } = event.nativeEvent.layout;
    if (width !== rootSize.width || height !== rootSize.height) setRootSize({ width, height });
  }}>
    {listElement}
    {chooserLayer}
  </View>;
}

/** Wide Board row: the compact meta line with the actions beside them; `label` reads the full detail. Linked sessions sit below. */
function BoardRow({ testID, title, lines, label, actions, footer, colors }: { testID: string; title: ReactNode; lines: readonly string[]; label: string; actions: ReactNode; footer?: ReactNode; colors: Colors }) {
  const content = <>
    <View accessible accessibilityLabel={label} style={{ flex: 1, minWidth: 0, gap: 2 }}>
      <Text selectable numberOfLines={2} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{title}</Text>
      {lines.map((line, index) => <Copy key={index} colors={colors} small muted>{line}</Copy>)}
    </View>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{actions}</View>
  </>;
  const card = { marginHorizontal: 12, marginBottom: 6, paddingHorizontal: 10, paddingVertical: 8, borderWidth: 1, borderColor: colors.border, borderRadius: 8, backgroundColor: colors.surface1 };
  if (!footer) return <View testID={testID} style={{ ...card, gap: 8, flexDirection: "row", alignItems: "center" }}>{content}</View>;
  return <View testID={testID} style={{ ...card, gap: 4 }}>
    <View style={{ gap: 8, flexDirection: "row", alignItems: "center" }}>{content}</View>
    {footer}
  </View>;
}

/**
 * Compact Board row: at most two title lines and one meta line. The text of a plan task row is one
 * button (open the task in its plan); Open issue sits beside it, never inside it, and screen readers
 * also reach it as a custom action, because an accessible row hides its controls on iOS. A mouse
 * gets a 22pt action level with the meta line; a touch pointer a real 44pt action, since the web
 * ignores a hit slop, and a row button at least 44pt tall even when the title takes one line.
 * Linked sessions, when there are any, add one line below.
 */
function CompactBoardRow({ testID, title, meta, label, tone, onPress, issue, sessions, colors }: {
  testID: string; title: ReactNode; meta: string; label: string; tone: Tone; colors: Colors;
  onPress?: () => void; issue?: { key: string; open: () => void }; sessions?: ReactNode;
}) {
  let dot = colors.foregroundMuted;
  if (tone === "done") dot = colors.statusSuccess;
  else if (tone === "indeterminate") dot = colors.statusWarning;
  const accessibility = {
    accessible: true, accessibilityLabel: label,
    accessibilityActions: issue ? [{ name: "openIssue", label: `Open issue ${issue.key}` }] : [],
    onAccessibilityAction: ({ nativeEvent }: { nativeEvent: { actionName: string } }) => { if (nativeEvent.actionName === "openIssue") issue?.open(); },
  };
  const style = { marginHorizontal: 8, marginBottom: 6, paddingHorizontal: 8, paddingVertical: 6, gap: 2, borderWidth: 1, borderRadius: 8, borderColor: colors.border, backgroundColor: colors.surface1 };
  const action = issue ? <Pressable testID={`${testID}-issue`} accessibilityRole="button" accessibilityLabel={`Open issue ${issue.key}`} onPress={issue.open}
    style={{ minHeight: COARSE_POINTER ? 44 : 22, minWidth: 44, paddingHorizontal: 8, justifyContent: "center", alignItems: "center", borderWidth: 1, borderRadius: 6, borderColor: colors.border }}>
    <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>Open issue</Text>
  </Pressable> : null;
  const lines = <>
    <Text numberOfLines={2} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13, lineHeight: 17 }}>{title}</Text>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minHeight: 22 }}>
      <View testID={`${testID}-tone`} importantForAccessibility="no" accessibilityElementsHidden style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: dot }} />
      <Text numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>{meta}</Text>
    </View>
  </>;
  const text = { flex: 1, minWidth: 0, gap: 2 };
  const main = onPress
    ? <Pressable testID={testID} accessibilityRole="button" onPress={onPress} style={{ ...text, justifyContent: "center", minHeight: COARSE_POINTER ? 44 : 0 }} {...accessibility}>{lines}</Pressable>
    : <View testID={testID} style={text} {...accessibility}>{lines}</View>;
  return <View testID={`${testID}-card`} style={style}>
    <View style={{ flexDirection: "row", alignItems: COARSE_POINTER ? "center" : "flex-end", gap: 8 }}>
      {main}
      {action}
    </View>
    {sessions}
  </View>;
}
