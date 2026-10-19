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
import { Action, Copy, type Colors } from "./page-controls";

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

export function GraphPage({ mode, hostId, theme, compact, sources, selectedId, reads, available, error, epicKey, onPlan, onIssue, initialOffset = 0, onScroll, onChooseSource, onBoard }: {
  mode: "Board" | "Graph"; hostId: string; theme: PluginTheme; compact: boolean;
  sources: readonly ReadSourceSummary[]; selectedId: string | null; reads: Readonly<Record<string, SourceRead>>;
  available: boolean; error: string | null; onSelect: (sourceId: string) => void;
  /** Context epic for captured Jira records when no valid canonical graph supplies one. */
  epicKey?: string | null;
  onPlan: (sourceId: string, taskId?: string) => void;
  onIssue: (key: string) => void;
  initialOffset?: number; onScroll?: (offset: number) => void;
  onChooseSource?: () => void;
  /** Opens Board for the same context; offered when Graph has no valid canonical graph. */
  onBoard?: () => void;
}) {
  const [evidenceOpen, setEvidenceOpen] = useState(false);
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
    <PlanGraph graph={valid} hostId={hostId} theme={theme} store={store} compact={compact}
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
  if (!read) sourceNote = selectedId ? "Selected source unavailable in this context." : epic ? "No plan selected. Plan declarations and dependencies are unavailable." : "No plan or epic in this context. Select a plan in the top context picker.";
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
  const days = captureDays(captureTimes);
  const notes = [sourceNote, !read && plans.length > 1 ? "Several explicitly linked plans; select an exact source." : null, jiraNote];
  const noteCopy = (values: readonly (string | null)[]) => values.map((value, index) => value ? <Copy key={index} colors={colors}>{value}</Copy> : null);
  const title = `Board${epic ? ` · ${epic}` : ""}${read ? ` · ${read.source.title}` : ""}`;
  // Compact is list-first: one title line and one meta line. The truth statement and the notes
  // stay reachable in Source details (and in the meta line's label); empty states keep their notes.
  let jiraCount = "Jira not captured";
  if (captured.found) jiraCount = captured.records.length ? `${captured.records.length} Jira` : "no Jira children captured";
  const header = compact ? <View style={{ paddingHorizontal: 8, paddingVertical: 8, gap: 4 }}>
    <Text accessibilityRole="header" numberOfLines={1} style={{ color: colors.foreground, fontWeight: "600", fontSize: 15 }}>{title}</Text>
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
      {details}
    </View> : null}
  </View> : <View style={{ padding: 12, gap: 6 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 15 }}>{title}</Text>
    {notices}
    <Copy colors={colors} small muted>Read-only capture · {captured.records.length} captured Jira tasks · {valid ? `${valid.nodes.length} plan tasks` : "plan graph unavailable"}{captureTimes.length ? ` · captured ${captureTimes.join(" / ")}` : ""}</Copy>
    <Copy colors={colors} small muted>{truth}</Copy>
    {noteCopy(notes)}
    {actions}
    {evidenceOpen ? details : null}
  </View>;
  const planSource = read?.source.id;
  return <FlatList ref={list} data={items} keyExtractor={(item) => item.id} initialNumToRender={20} windowSize={7}
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
    contentContainerStyle={{ paddingBottom: 24 }} ListHeaderComponent={header} renderItem={({ item }) => {
      if (item.kind === "section") return <View style={{ paddingHorizontal: compact ? 8 : 12, paddingTop: compact ? 8 : 10, paddingBottom: compact ? 4 : 6, gap: 2 }}>
        <Text accessibilityRole="header" numberOfLines={compact ? 1 : undefined} accessibilityHint={compact ? item.note : undefined} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{item.title}</Text>
        {compact ? null : <Copy colors={colors} small muted>{item.note}</Copy>}
      </View>;
      if (item.kind === "jira") {
        const key = item.record.key;
        const state = capturedState(item.record.observations);
        const declaration = valid ? "Not declared in this plan revision" : "Plan declaration and dependencies unavailable";
        if (compact) return <CompactBoardRow testID={`board-jira-${key}`} colors={colors} tone={state.tone} meta={state.short}
          title={<>{key}<Text style={{ fontWeight: "400", color: colors.foregroundMuted }}> · Title unavailable in the captured Jira projection</Text></>}
          label={`${key}: title unavailable in the captured Jira projection. ${state.full}. ${declaration}.`} issue={{ key, open: () => onIssue(key) }} />;
        return <BoardRow testID={`board-jira-${key}`} colors={colors}
          title={<>{key}<Text style={{ fontWeight: "400", color: colors.foregroundMuted }}> · Title unavailable in the captured Jira projection</Text></>}
          lines={[...item.record.observations.map((value) => `${value.status} (${value.cat}) · captured ${value.observedAt} · ${value.sourceTitle}`), declaration]}
          actions={<Action colors={colors} small label="Open issue" accessibilityLabel={`Open issue ${key}`} onPress={() => onIssue(key)} />} />;
      }
      const { node } = item;
      if (!planSource) return null;
      const evidenceValue = evidence.get(node.id);
      const openPlan = () => onPlan(planSource, node.id);
      const issueKey = node.jira;
      if (compact) {
        let state = { short: "Unknown · no Jira key", full: "No Jira binding declared, status Unknown", tone: "unknown" as Tone };
        if (evidenceValue) state = capturedState(evidenceValue.observations.map((value) => ({ status: evidenceValue.issue.status, cat: evidenceValue.issue.cat, observedAt: value.observedAt, sourceId: value.sourceId, sourceTitle: value.sourceTitle })));
        else if (issueKey) state = { short: "Unknown · no consistent capture", full: `Jira ${issueKey}: no consistent captured record, status Unknown`, tone: "unknown" };
        const after = node.depends_on.length ? ` · after ${node.depends_on.join(", ")}` : "";
        return <CompactBoardRow testID={`board-task-${node.id}`} colors={colors} tone={state.tone} title={`${issueKey ?? node.id} · ${node.title}`}
          meta={`${node.id} · ${state.short} · ${node.phase}${after}`} onPress={openPlan}
          label={`Open plan task ${node.id}: ${node.title}. ${state.full}. ${node.phase}, dependency depth ${node.wave}. ${node.depends_on.length ? `After ${node.depends_on.join(", ")}` : "No prerequisites declared"}.${issueKey ? ` Jira ${issueKey}.` : ""}`}
          issue={issueKey ? { key: issueKey, open: () => onIssue(issueKey) } : undefined} />;
      }
      let jira = ["No Jira binding declared · status Unknown"];
      if (evidenceValue) jira = evidenceValue.observations.map((value) => `${evidenceValue.issue.status} (${evidenceValue.issue.cat}) · captured ${value.observedAt} · ${value.sourceTitle}`);
      else if (issueKey) jira = [`Jira ${issueKey}: no consistent captured record · status Unknown`];
      return <BoardRow testID={`board-task-${node.id}`} colors={colors} title={`${issueKey ?? node.id} · ${node.title}`}
        lines={[`${node.id} · ${node.phase} · depth ${node.wave} · prerequisites ${node.depends_on.join(", ") || "none declared"}`, ...jira]}
        actions={<>
          <Action colors={colors} small label="Open in plan" accessibilityLabel={`Open plan task ${node.id}`} onPress={openPlan} />
          {issueKey ? <Action colors={colors} small label="Open issue" accessibilityLabel={`Open issue ${issueKey}`} onPress={() => onIssue(issueKey)} /> : null}
        </>} />;
    }} />;
}

/** Wide Board row: full detail lines with the actions beside them. */
function BoardRow({ testID, title, lines, actions, colors }: { testID: string; title: ReactNode; lines: readonly string[]; actions: ReactNode; colors: Colors }) {
  return <View testID={testID} style={{ marginHorizontal: 12, marginBottom: 6, paddingHorizontal: 10, paddingVertical: 8, gap: 8, borderWidth: 1, borderColor: colors.border, borderRadius: 8, backgroundColor: colors.surface1,
    flexDirection: "row", alignItems: "center" }}>
    <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
      <Text selectable numberOfLines={2} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{title}</Text>
      {lines.map((line, index) => <Copy key={index} colors={colors} small muted>{line}</Copy>)}
    </View>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{actions}</View>
  </View>;
}

/** Visible issue action is 22pt tall; its hit slop brings the touch target to 44pt. */
export const COMPACT_ISSUE_SLOP = { top: 11, bottom: 11, left: 6, right: 6 };

/**
 * Compact Board row: at most two title lines and one meta line. A plan task row is one button
 * (open the task in its plan); Open issue is a trailing action that screen readers also reach as
 * a custom action, because an accessible row hides its nested controls on iOS.
 */
function CompactBoardRow({ testID, title, meta, label, tone, onPress, issue, colors }: {
  testID: string; title: ReactNode; meta: string; label: string; tone: Tone; colors: Colors;
  onPress?: () => void; issue?: { key: string; open: () => void };
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
  const body = <>
    <Text numberOfLines={2} style={{ color: colors.foreground, fontWeight: "600", fontSize: 13, lineHeight: 17 }}>{title}</Text>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minHeight: 22 }}>
      <View testID={`${testID}-tone`} importantForAccessibility="no" accessibilityElementsHidden style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: dot }} />
      <Text numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>{meta}</Text>
      {issue ? <Pressable accessibilityRole="button" accessibilityLabel={`Open issue ${issue.key}`} hitSlop={COMPACT_ISSUE_SLOP} onPress={issue.open}
        style={{ minHeight: 22, minWidth: 44, paddingHorizontal: 8, justifyContent: "center", alignItems: "center", borderWidth: 1, borderRadius: 6, borderColor: colors.border }}>
        <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>Open issue</Text>
      </Pressable> : null}
    </View>
  </>;
  return onPress
    ? <Pressable testID={testID} accessibilityRole="button" onPress={onPress} style={style} {...accessibility}>{body}</Pressable>
    : <View testID={testID} style={style} {...accessibility}>{body}</View>;
}
