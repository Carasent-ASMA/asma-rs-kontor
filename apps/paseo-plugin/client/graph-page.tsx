import { useMemo } from "react";
import { Text, View } from "react-native";
import { FlatList, ScrollView } from "@getpaseo/plugin/client/react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { PlanGraph, createPlanGraphStore, planGraphViewKey, type PlanGraphStore } from "./plan-graph/graph";
import { validatePlanGraph } from "./plan-graph/contract";
import { SourceDetails } from "./source-details";
import { historicalPlanProgress } from "./page-data";
import { Action, Card, Copy } from "./page-controls";

const stores = new Map<string, PlanGraphStore>();
function retainedStore(hostId: string): PlanGraphStore {
  let store = stores.get(hostId);
  if (!store) { store = createPlanGraphStore(); stores.set(hostId, store); }
  return store;
}
export function GraphPage({ mode, hostId, theme, compact, sources, selectedId, reads, available, error, onSelect, onPlan, onIssue }: {
  mode: "Board" | "Graph"; hostId: string; theme: PluginTheme; compact: boolean;
  sources: readonly ReadSourceSummary[]; selectedId: string | null; reads: Readonly<Record<string, SourceRead>>;
  available: boolean; error: string | null; onSelect: (sourceId: string) => void;
  onPlan: (sourceId: string, taskId?: string) => void;
  onIssue: (key: string) => void;
}) {
  const store = useMemo(() => retainedStore(hostId), [hostId]);
  const colors = theme.colors;
  const read = selectedId ? reads[selectedId] : undefined;
  const graph = read?.graph;
  const validated = useMemo(() => graph && read?.revision ? validatePlanGraph(graph, {
    source: graph.source, source_sha256: read.revision, graph_sha256: graph.graph_sha256,
  }) : null, [graph, read?.revision]);
  const progress = graph ? historicalPlanProgress(graph, Object.values(reads)) : null;
  const historicalStates = progress ? Object.fromEntries(progress.states.flatMap(({ node, evidence }) => evidence ? [[node.id, { label: `${evidence.issue.status} (historical)`, source: evidence.observations[0]!.sourceTitle, observedAt: evidence.observations[0]!.observedAt }]] : [])) : {};
  const header = <View style={{ padding: 12, gap: 8 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 18 }}>{mode}</Text>
    {error ? <Copy colors={colors}>{error}</Copy> : null}
    {!available ? <Copy colors={colors}>Source service unavailable. Retained exports are stale.</Copy> : null}
    {read ? <>
      <SourceDetails read={read} colors={colors} />
      {Object.values(reads).filter((value) => value.source.kind === "jira").map((value) => <SourceDetails key={value.source.id} read={value} colors={colors} />)}
      <Action colors={colors} label="Open source plan" onPress={() => onPlan(read.source.id)} />
    </> : <Copy colors={colors}>{selectedId ? "Selected source unavailable in this context." : "No plan linked. Select a plan in the top context picker."}</Copy>}
    {validated && !validated.ok ? validated.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic.path}: {diagnostic.message}</Copy>) : null}
    {!validated?.ok ? <Copy colors={colors}>No valid exact-source canonical graph available. No legacy text extraction or inferred task state.</Copy> : null}
  </View>;
  if (!validated?.ok || !read) return <ScrollView>{header}</ScrollView>;
  if (mode === "Graph") return <View style={{ flex: 1 }}>{header}<PlanGraph graph={validated.graph} hostId={hostId} theme={theme} store={store} compact={compact}
    sourcedStates={Object.keys(historicalStates).length ? { viewKey: planGraphViewKey(hostId, validated.graph), nodes: historicalStates } : undefined}
    sourceActions={{ plan: (action) => { if (action.kind === "plan" && action.source === validated.graph.source && action.source_sha256 === read.revision) onPlan(read.source.id, action.taskId); },
      jira: (action) => { if (action.kind === "jira" && validated.graph.nodes.some((node) => node.jira === action.jira)) onIssue(action.jira); } }} /></View>;
  return <FlatList data={validated.graph.nodes} keyExtractor={(node) => node.id} initialNumToRender={20} windowSize={7}
    contentContainerStyle={{ paddingBottom: 24 }} ListHeaderComponent={header} renderItem={({ item }) => <View style={{ marginHorizontal: 12, marginBottom: 10 }}>
      <Card title={`${item.jira ?? item.id} · ${item.title}`} colors={colors}>
        <Copy colors={colors}>Task declaration: {item.id} · {item.phase} · dependency depth {item.wave}</Copy>
        <Copy colors={colors}>Jira status: {historicalStates[item.id] ? `${historicalStates[item.id]!.label} · observed ${historicalStates[item.id]!.observedAt}` : "Unknown"} · Readiness / governance: Unknown</Copy>
        <Copy colors={colors}>Direct prerequisites: {item.depends_on.join(", ") || "None declared"}</Copy>
        <Copy colors={colors} muted>Source {read.source.id} · read {read.readAt}</Copy>
        <Action colors={colors} label={`Open plan task ${item.id}`} onPress={() => onPlan(read.source.id, item.id)} />
        {item.jira ? <Action colors={colors} label={`Open issue ${item.jira}`} onPress={() => onIssue(item.jira!)} /> : null}
      </Card>
    </View>} />;
}
