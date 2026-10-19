import { useMemo, useRef } from "react";
import { ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { Destination } from "./navigation-state";
import { confirmedEpic, inboxEntries, nativeSummary, overviewRows, searchRecords } from "./page-data";
import { Action, Card, Copy, type Colors } from "./page-controls";

type PageProps = {
  tab: Destination; colors: Colors; entries: readonly DirectoryEntry[]; directory: DirectoryState;
  sources: readonly ReadSourceSummary[]; reads: Readonly<Record<string, SourceRead>>; sourceId: string | null;
  sourceError: string | null; query: string; setQuery: (query: string) => void;
  onSource: (id: string) => void; onOpen: (entry: DirectoryEntry) => void; onTab: (tab: Destination) => void;
  onEpicTab: (tab: "Board" | "Sessions", epicKey: string | null, sourceId: string | null) => void;
  initialOffset: number; onScroll: (offset: number) => void;
};
function SourceStamp({ read, colors }: { read: SourceRead; colors: Colors }) {
  return <View style={{ gap: 2 }}>
    <Copy colors={colors} muted>{read.source.title} · {read.status} · {read.source.revisionKind}</Copy>
    <Copy colors={colors} muted>{read.source.path}</Copy>
    <Copy colors={colors} muted>SHA-256 {read.revision ?? "Unknown"} · read {read.readAt} · modified {read.modifiedAt ?? "Unknown"}</Copy>
    {read.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic}</Copy>)}
  </View>;
}
function NativeRows({ entries, colors, onOpen }: Pick<PageProps, "entries" | "colors" | "onOpen">) {
  return <>{entries.map((entry) => <View key={entry.id} style={{ borderBottomWidth: 1, borderColor: colors.border }}>
    <Action label={entry.title} colors={colors} onPress={() => onOpen(entry)} />
    <Copy colors={colors} muted>{entry.status} · {entry.needsInput ? "Needs input" : entry.attentionReason ?? "No native attention reason"} · {entry.workspaceName ?? "Unassigned workspace"}</Copy>
    <Copy colors={colors} muted>Observed labels: role {entry.labels.role ?? "Unknown"} · epic {confirmedEpic(entry) ?? "Unassigned"} · workstream {entry.labels.workstream ?? entry.labels.desk ?? "Unknown"}</Copy>
  </View>)}</>;
}
function Overview(props: PageProps) {
  const { colors, directory, onSource, onOpen } = props;
  const reads = Object.values(props.reads);
  const total = nativeSummary(props.entries);
  return <>
    <Card colors={colors} title="All projects">
      <Copy colors={colors}>{total.total} native sessions · {total.running} running · {total.idle} idle · {total.closed} closed</Copy>
      <Copy colors={colors}>{total.attention} native attention · {total.needsInput} needs input · {total.errors} native errors</Copy>
      <Copy colors={colors} muted>Inventory: {directory.status} · {directory.live ? "Live (reconciled subscription)" : "Not Live"} · last complete read {directory.lastUpdatedAt ? new Date(directory.lastUpdatedAt).toISOString() : "Unknown"}</Copy>
      <Copy colors={colors}>Lifecycle: Unknown · Delivered work: Unknown</Copy>
      <Copy colors={colors}>Governed scheduler / gates: kontord not connected</Copy>
    </Card>
    {overviewRows(props.entries, reads).map((row) => {
      const counts = nativeSummary(row.entries);
      const plans = row.reads.filter((read) => read.source.kind === "plan");
      const decisions = row.reads.filter((read) => read.source.kind === "decisions" && read.status === "ready");
      const configuredDecisions = row.reads.filter((read) => read.source.kind === "decisions");
      const openDecisions = decisions.reduce((sum, read) => sum + (read.records?.decisions.filter((decision) => decision.state === "open").length ?? 0), 0);
      const tasks = plans.reduce((sum, read) => sum + (read.graph?.nodes.length ?? 0), 0);
      return <Card key={row.key} colors={colors} title={row.key}>
        <Copy colors={colors}>Lifecycle: Unknown · Runtime: {counts.running} running / {counts.idle} idle / {counts.closed} closed</Copy>
        <Copy colors={colors}>Native attention: {counts.attention} · Needs input: {counts.needsInput}</Copy>
        <Copy colors={colors}>Task progress: Unknown / {tasks || "Unknown"} · completion not captured by canonical export · coverage: {plans.filter((read) => read.graph).length}/{plans.length} plan sources</Copy>
        <Copy colors={colors}>Unresolved decisions: {decisions.length ? `${openDecisions} recorded open · coverage ${decisions.length}/${configuredDecisions.length} configured sources` : "Unknown — decision source not connected"}</Copy>
        <Copy colors={colors}>Needs decision: {decisions.length ? `${openDecisions} recorded open; see Decisions` : "Unknown"} · Delivery: Unknown</Copy>
        {plans.map((read) => <View key={read.source.id}><SourceStamp read={read} colors={colors} /><Action colors={colors} label={`Open plan: ${read.source.title}`} onPress={() => onSource(read.source.id)} /></View>)}
        {!plans.length ? <Copy colors={colors} muted>No plan linked</Copy> : null}
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
          {plans.length ? <Action label="Tasks" colors={colors} onPress={() => props.onEpicTab("Board", /^ASMA-\d+$/.test(row.key) ? row.key : null, plans.length === 1 ? plans[0]!.source.id : null)} /> : null}
          <Action label={/^ASMA-\d+$/.test(row.key) ? "Sessions" : "All sessions"} colors={colors} onPress={() => props.onEpicTab("Sessions", /^ASMA-\d+$/.test(row.key) ? row.key : null, null)} />
        </View>
        <NativeRows entries={row.entries.slice(0, 3)} colors={colors} onOpen={onOpen} />
      </Card>;
    })}
  </>;
}
function Plan(props: PageProps) {
  const candidate = props.sourceId ? props.reads[props.sourceId] : undefined;
  const selected = candidate?.source.kind === "plan" ? candidate : undefined;
  const offsets = useRef<Record<number, number>>({});
  const scroller = useRef<ScrollView | null>(null);
  const lines = useMemo(() => selected?.content?.split("\n") ?? [], [selected?.content]);
  const headings = lines.flatMap((line, index) => /^#{1,6}\s+/.test(line) ? [{ label: line.replace(/^#+\s+/, ""), index }] : []);
  const needle = props.query.trim().toLocaleLowerCase();
  return <>
    {props.sources.filter((source) => source.kind === "plan").map((source) => <Action key={source.id} colors={props.colors} label={`${source.title} · ${source.epic ?? "draft / no epic key"}`} selected={source.id === props.sourceId} onPress={() => props.onSource(source.id)} />)}
    {!selected ? <Copy colors={props.colors}>No plan linked. Select an explicit configured source; conversation prose and titles do not create links.</Copy> : <>
      <SourceStamp read={selected} colors={props.colors} />
      <Copy colors={props.colors} muted>Document content is read-only. Approval and lifecycle are not inferred from prose.</Copy>
      <TextInput accessibilityLabel="Search this plan" placeholder="Search this plan" value={props.query} onChangeText={props.setQuery} style={{ color: props.colors.foreground, minHeight: 44, padding: 10, borderWidth: 1, borderColor: props.colors.border }} />
      <View style={{ flexDirection: "row", flexWrap: "wrap" }}>{headings.filter((heading) => !needle || heading.label.toLocaleLowerCase().includes(needle)).map((heading) => <Action key={heading.index} label={heading.label} colors={props.colors} onPress={() => scroller.current?.scrollTo({ y: offsets.current[heading.index] ?? 0, animated: true })} />)}</View>
      {selected.content === null ? <Copy colors={props.colors}>This configured plan is unavailable.</Copy> :
        <ScrollView ref={scroller} style={{ minHeight: 280, maxHeight: 640 }} nestedScrollEnabled>
          {lines.map((line, index) => <Text key={index} selectable onLayout={(event) => { offsets.current[index] = event.nativeEvent.layout.y; }}
            style={{ color: props.colors.foreground, fontSize: /^#+\s/.test(line) ? 16 : 13, lineHeight: 21,
              fontWeight: /^#+\s/.test(line) ? "600" : "400", backgroundColor: needle && line.toLocaleLowerCase().includes(needle) ? props.colors.surface2 : undefined }}>{line || " "}</Text>)}
        </ScrollView>}
    </>}
  </>;
}
function RecordPage(props: PageProps) {
  const kind = props.tab === "Delivery" ? "delivery" : props.tab === "Decisions" ? "decisions" : "teams";
  const reads = Object.values(props.reads).filter((read) => read.source.kind === kind);
  return <>
    {!reads.length ? <Copy colors={props.colors}>{props.tab}: not connected to a configured {kind} source.</Copy> : null}
    {reads.map((read) => <Card key={read.source.id} title={read.source.title} colors={props.colors}>
      <SourceStamp read={read} colors={props.colors} />
      {kind === "delivery" ? read.records?.delivery.map((record) => <Copy key={record.id} colors={props.colors}>{record.kind} · {record.title} · recorded state: {record.state} · {record.recordedAt ?? "time Unknown"} {record.url ?? ""}</Copy>) : null}
      {kind === "decisions" ? read.records?.decisions.map((record) => <Copy key={record.id} colors={props.colors}>{record.id} · {record.title} · {record.state} · owner: {record.owner ?? "Unknown"} {record.detail ?? ""} {record.url ?? ""}</Copy>) : null}
      {kind === "teams" ? read.records?.teams.map((team) => <View key={team.id}><Copy colors={props.colors}>Configured definition: {team.title} ({team.id})</Copy>{team.roles.map((role, index) => <Copy key={index} colors={props.colors} muted>{role.role} · configured chain: {role.chain.join(" → ") || "Not captured"}</Copy>)}</View>) : null}
      {read.status === "ready" && read.records?.[kind].length === 0 ? <Copy colors={props.colors}>No records in this source.</Copy> : null}
    </Card>)}
    {kind === "teams" ? <Card colors={props.colors} title="Observed native sessions">
      <Copy colors={props.colors}>Observed sessions and labels are separate from configured team definitions. Planned/governed assignment: Unknown. Governed fleet: kontord not connected.</Copy>
      <NativeRows {...props} />
    </Card> : null}
  </>;
}
export function ReadOnlyPage(props: PageProps) {
  const scroller = useRef<ScrollView | null>(null);
  const results = useMemo(() => searchRecords(props.entries, Object.values(props.reads), props.query), [props.entries, props.reads, props.query]);
  return <ScrollView ref={scroller} onContentSizeChange={() => scroller.current?.scrollTo({ y: props.initialOffset, animated: false })}
    onScroll={(event) => props.onScroll(event.nativeEvent.contentOffset.y)} scrollEventThrottle={100} keyboardShouldPersistTaps="handled"
    contentContainerStyle={{ padding: 12, paddingBottom: 28, gap: 12 }}>
    <Text accessibilityRole="header" style={{ color: props.colors.foreground, fontSize: 20, fontWeight: "600" }}>{props.tab}</Text>
    {props.sourceError ? <Copy colors={props.colors}>{props.sourceError}</Copy> : null}
    {props.directory.error || props.directory.staleReason ? <Copy colors={props.colors}>Native inventory: {props.directory.error ?? props.directory.staleReason}</Copy> : null}
    {props.tab === "Overview" ? <Overview {...props} /> : null}
    {props.tab === "Plan" ? <Plan {...props} /> : null}
    {props.tab === "Inbox" ? <><Copy colors={props.colors}>Native needs-input, errors and attention in the current inventory. Opening a session uses Paseo's own controls. Unseen routing is not captured.</Copy><NativeRows {...props} entries={inboxEntries(props.entries)} />{!inboxEntries(props.entries).length ? <Copy colors={props.colors}>No matching native items in this read.</Copy> : null}</> : null}
    {props.tab === "Delivery" || props.tab === "Decisions" || props.tab === "AI teams & sessions" ? <RecordPage {...props} /> : null}
    {props.tab === "Activity" ? <><Copy colors={props.colors}>Partial coverage: current session updatedAt values only, as of {props.directory.lastUpdatedAt ? new Date(props.directory.lastUpdatedAt).toISOString() : "Unknown"}. This is not a durable audit log or a transcript history.</Copy>{[...props.entries].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).map((entry) => <View key={entry.id}><Action label={entry.title} colors={props.colors} onPress={() => props.onOpen(entry)} /><Copy colors={props.colors} muted>{entry.updatedAt} · native {entry.status}</Copy></View>)}</> : null}
    {props.tab === "Search" ? <><Copy colors={props.colors}>Scope: available session/workspace names, configured plan text and canonical task records. No private transcript search.</Copy><TextInput accessibilityLabel="Search available records" placeholder="Search available records" value={props.query} onChangeText={props.setQuery} style={{ color: props.colors.foreground, padding: 10, minHeight: 44, borderWidth: 1, borderColor: props.colors.border }} /><NativeRows {...props} entries={results.sessions} />{results.plans.map((read) => <Action key={read.source.id} colors={props.colors} label={`Plan: ${read.source.title}`} onPress={() => props.onSource(read.source.id)} />)}{results.tasks.map(({ node, source }) => <Action key={`${source.id}:${node.id}`} colors={props.colors} label={`${node.jira ?? node.id} · ${node.title}`} onPress={() => props.onSource(source.id)} />)}</> : null}
  </ScrollView>;
}
