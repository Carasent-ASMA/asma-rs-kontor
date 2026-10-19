import { useMemo } from "react";
import { Text, View } from "react-native";
import { FlatList, ScrollView } from "@getpaseo/plugin/client/react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { PlanGraph, createPlanGraphStore, type PlanGraphStore } from "./plan-graph/graph";
import { validatePlanGraph } from "./plan-graph/contract";
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
  const header = <View style={{ padding: 12, gap: 8 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 18 }}>{mode}</Text>
    <ScrollView horizontal showsHorizontalScrollIndicator={false}>{sources.filter((source) => source.kind === "plan").map((source) => <Action key={source.id} colors={colors} label={source.title} selected={source.id === selectedId} onPress={() => onSelect(source.id)} />)}</ScrollView>
    {error ? <Copy colors={colors}>{error}</Copy> : null}
    {!available ? <Copy colors={colors}>Source service unavailable. Retained exports are stale.</Copy> : null}
    {read ? <>
      <Copy colors={colors}>{read.source.path} · {read.source.revisionKind} · {read.status}{available ? "" : " · stale"}</Copy>
      <Copy colors={colors} muted>Source SHA-256 {read.revision ?? "Unknown"} · read {read.readAt}</Copy>
      {read.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic}</Copy>)}
      <Action colors={colors} label="Open source plan" onPress={() => onPlan(read.source.id)} />
    </> : <Copy colors={colors}>{selectedId ? "Selected source unavailable in this context." : "No plan linked. Select a configured plan source."}</Copy>}
    {validated && !validated.ok ? validated.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic.path}: {diagnostic.message}</Copy>) : null}
    {!validated?.ok ? <Copy colors={colors}>No valid exact-source canonical graph available. No legacy text extraction or inferred task state.</Copy> : null}
  </View>;
  if (!validated?.ok || !read) return <ScrollView>{header}</ScrollView>;
  if (mode === "Graph") return <View style={{ flex: 1 }}>{header}<PlanGraph graph={validated.graph} hostId={hostId} theme={theme} store={store} compact={compact}
    sourceActions={{ plan: (action) => { if (action.kind === "plan" && action.source === validated.graph.source && action.source_sha256 === read.revision) onPlan(read.source.id, action.taskId); },
      jira: (action) => { if (action.kind === "jira" && validated.graph.nodes.some((node) => node.jira === action.jira)) onIssue(action.jira); } }} /></View>;
  return <FlatList data={validated.graph.nodes} keyExtractor={(node) => node.id} initialNumToRender={20} windowSize={7}
    contentContainerStyle={{ paddingBottom: 24 }} ListHeaderComponent={header} renderItem={({ item }) => <View style={{ marginHorizontal: 12, marginBottom: 10 }}>
      <Card title={`${item.jira ?? item.id} · ${item.title}`} colors={colors}>
        <Copy colors={colors}>Task declaration: {item.id} · {item.phase} · dependency depth {item.wave}</Copy>
        <Copy colors={colors}>Status: Unknown · Readiness / governance: Unknown · progress: Not captured</Copy>
        <Copy colors={colors}>Direct prerequisites: {item.depends_on.join(", ") || "None declared"}</Copy>
        <Copy colors={colors} muted>Source {read.source.id} · revision {read.revision}</Copy>
        <Action colors={colors} label={`Open plan task ${item.id}`} onPress={() => onPlan(read.source.id, item.id)} />
        {item.jira ? <Action colors={colors} label={`Open issue ${item.jira}`} onPress={() => onIssue(item.jira!)} /> : null}
      </Card>
    </View>} />;
}
