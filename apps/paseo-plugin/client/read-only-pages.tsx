import { useMemo, useRef, useState } from "react";
import { ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { Destination } from "./navigation-state";
import { historicalJiraIssue, confirmedEpic, inboxEntries, nativeSummary, overviewRows, searchRecords, filteredOverviewRows, recordedDecisions, type OverviewFilter } from "./page-data";
import { NativeSummaryIndicators, PlanTaskStatus } from "./overview-status";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { TeamsSourcePage } from "./teams-source-page";
import { SourceDetails } from "./source-details";
import { Action, Card, Copy, type Colors } from "./page-controls";
import { FormattedPlanBlock, headingAnchor, planBlocks } from "./plan-document";

type PageProps = {
  teamsSources?: readonly ReadSourceSummary[]; teamsJsonReads?: Readonly<Record<string, SourceRead>>;
  teamsSnapshots?: Readonly<Record<string, TeamsSourceSnapshot>>; teamsErrors?: Readonly<Record<string, string>>;
  teamsBindingDiagnostics?: readonly string[]; teamsAvailable?: boolean;
  contextLabel?: string; tab: Destination; colors: Colors; entries: readonly DirectoryEntry[]; directory: DirectoryState;
  compact?: boolean; onChooseSource?: () => void; onDocumentLink?: (target: string) => void;
  sources: readonly ReadSourceSummary[]; reads: Readonly<Record<string, SourceRead>>; sourceId: string | null;
  sourceError: string | null; query: string; setQuery: (query: string) => void;
  onSource: (id: string) => void; onOpen: (entry: DirectoryEntry) => void; onTab: (tab: Destination) => void;
  onEpicTab: (tab: "Board" | "Graph" | "Sessions", epicKey: string | null, sourceId: string | null) => void;
  initialDocumentOffset: number; onDocumentScroll: (offset: number) => void;
  initialOffset: number; onScroll: (offset: number) => void;
};
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
  const filters: Array<[OverviewFilter, string]> = [["all", "All"], ["attention", "Native attention"], ["needs-input", "Needs input"], ["decisions", "Open / Unknown decisions"]];
  const filter = filters.find(([key]) => key === props.query)?.[0] ?? "all";
  const rows = filteredOverviewRows(props.entries, reads, filter);
  const allRows = overviewRows(props.entries, reads);
  return <>
    <Card colors={colors} title={props.contextLabel ?? "All projects"}>
      <NativeSummaryIndicators counts={total} colors={colors} />
      <Copy colors={colors} muted>Inventory: {directory.status} · {directory.live ? "Live (reconciled subscription)" : "Not Live"} · last complete read {directory.lastUpdatedAt ? new Date(directory.lastUpdatedAt).toISOString() : "Unknown"}</Copy>
      <Copy colors={colors}>Lifecycle: Unknown · Delivered work: Unknown</Copy>
      <Copy colors={colors}>Governed scheduler / gates: kontord is not enabled</Copy>
      {reads.filter((read) => read.source.kind === "jira").map((read) => <SourceDetails key={read.source.id} read={read} colors={colors} />)}
      {!reads.some((read) => read.jiraHistory) ? <Copy colors={colors}>Historical Jira: not connected to a configured status source.</Copy> : null}
    </Card>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{filters.map(([key, label]) => <Action key={key} label={label} colors={colors} selected={filter === key} onPress={() => props.setQuery(key === "all" ? "" : key)} />)}</View>
    <Copy colors={colors} muted>{rows.filter((row) => /^ASMA-\d+$/.test(row.key)).length}/{allRows.filter((row) => /^ASMA-\d+$/.test(row.key)).length} epics · {rows.reduce((sum, row) => sum + row.entries.length, 0)} shown sessions</Copy>
    {!rows.length ? <Copy colors={colors}>No matching observations. Decision coverage can remain Unknown.</Copy> : null}
    {rows.map((row) => {
      const counts = nativeSummary(row.entries);
      const plans = row.reads.filter((read) => read.source.kind === "plan");
      const decisions = recordedDecisions(row.reads);
      const workflow = historicalJiraIssue(row.key, null, reads);
      return <Card key={row.key} colors={colors} title={row.key}>
        <Copy colors={colors}>Lifecycle: Unknown · Native runtime snapshot</Copy>
        <NativeSummaryIndicators counts={counts} colors={colors} />
        <Copy colors={colors}>Jira workflow: {workflow ? `${workflow.issue.status} · historical, observed ${workflow.observations.map((value) => value.observedAt).join(" / ")}` : "Unknown"}. Current completion: Unknown.</Copy>
        {plans.map((read) => <PlanTaskStatus key={read.source.id} plan={read} reads={reads} colors={colors} />)}
        <Copy colors={colors}>Unresolved decisions: {decisions.open === null ? "Unknown — decision source not connected" : `${decisions.open} recorded open · ${decisions.unknown} recorded Unknown · coverage ${decisions.captured}/${decisions.configured} configured sources${decisions.complete ? "" : " · remaining coverage Unknown"}`}</Copy>
        <Copy colors={colors}>Delivery: Unknown</Copy>
        {plans.length === 1 ? <><SourceDetails read={plans[0]!} colors={colors} /><Action colors={colors} label="Open plan" onPress={() => onSource(plans[0]!.source.id)} /></> : plans.length > 1 ? <Copy colors={colors}>Choose a plan in the top context picker.</Copy> : null}
        {!plans.length ? <Copy colors={colors} muted>No plan linked</Copy> : null}
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
          {plans.length ? <Action label="Tasks" colors={colors} onPress={() => props.onEpicTab("Board", /^ASMA-\d+$/.test(row.key) ? row.key : null, plans.length === 1 ? plans[0]!.source.id : null)} /> : null}
          {plans.length ? <Action label="Graph" colors={colors} onPress={() => props.onEpicTab("Graph", /^ASMA-\d+$/.test(row.key) ? row.key : null, plans.length === 1 ? plans[0]!.source.id : null)} /> : null}
          <Action label={/^ASMA-\d+$/.test(row.key) ? "Sessions" : "All sessions"} colors={colors} onPress={() => props.onEpicTab("Sessions", /^ASMA-\d+$/.test(row.key) ? row.key : null, null)} />
        </View>
        <NativeRows entries={row.entries.slice(0, 3)} colors={colors} onOpen={onOpen} />
      </Card>;
    })}
  </>;
}
function Plan(props: PageProps) {
  const [sourceMode, setSourceMode] = useState(false);
  const [contentsOpen, setContentsOpen] = useState(!props.compact);
  const [linkMessage, setLinkMessage] = useState<string | null>(null);
  const candidate = props.sourceId ? props.reads[props.sourceId] : undefined;
  const selected = candidate?.source.kind === "plan" ? candidate : undefined;
  const offsets = useRef<Record<number, number>>({});
  const pendingHeading = useRef<number | null>(null);
  const scroller = useRef<ScrollView | null>(null);
  const position = useRef({ restoring: props.initialDocumentOffset > 0, target: props.initialDocumentOffset, requested: null as number | null, height: null as number | null, viewport: null as number | null });
  const restoreDocument = () => {
    const state = position.current;
    if (!state.restoring || state.height === null || state.viewport === null) return;
    const target = Math.min(state.target, Math.max(0, state.height - state.viewport));
    if (state.requested === target) return;
    state.requested = target;
    scroller.current?.scrollTo({ y: target, animated: false });
  };
  const scrollToHeading = (index: number) => {
    position.current.restoring = false;
    const y = offsets.current[index] ?? 0;
    props.onDocumentScroll(y);
    scroller.current?.scrollTo({ y, animated: true });
  };
  const blocks = useMemo(() => planBlocks(selected?.content ?? ""), [selected?.content]);
  const headings = blocks.filter((block) => block.kind === "heading").map((block) => ({ label: block.text, index: block.line }));
  const navigateHeading = (index: number) => {
    if (sourceMode) { pendingHeading.current = index; setSourceMode(false); }
    else scrollToHeading(index);
  };
  const needle = props.query.trim().toLocaleLowerCase();
  const openLink = (target: string) => {
    if (target.startsWith("#")) {
      let anchor: string; try { anchor = decodeURIComponent(target.slice(1)); } catch { setLinkMessage("Invalid heading link."); return; }
      const match = headings.find((heading) => headingAnchor(heading.label) === anchor || heading.label === anchor);
      if (match) navigateHeading(match.index); else setLinkMessage("This heading is not present in the exact source.");
    } else if (props.onDocumentLink) props.onDocumentLink(target);
    else setLinkMessage("Link navigation is unavailable on this host.");
  };
  return <>
    {!selected ? <><Copy colors={props.colors}>{props.sources.filter((source) => source.kind === "plan").length > 1 ? "Several explicitly linked plans. Choose an exact source." : "No plan linked. Select a configured plan in the top context picker."} Conversation prose and titles do not create links.</Copy>{props.onChooseSource ? <Action label="Choose plan" colors={props.colors} onPress={props.onChooseSource} /> : null}</> : <>
      <SourceDetails read={selected} colors={props.colors} />
      <Copy colors={props.colors} muted>Document content is read-only. Approval and lifecycle are not inferred from prose.</Copy>
      <TextInput accessibilityLabel="Search this plan" placeholder="Search this plan" value={props.query} onChangeText={(query) => { setSourceMode(false); props.setQuery(query); }} style={{ color: props.colors.foreground, minHeight: 44, padding: 10, borderWidth: 1, borderColor: props.colors.border }} />
      {needle ? <Copy colors={props.colors} muted>{blocks.filter((block) => block.text.toLocaleLowerCase().includes(needle)).length} matching blocks · search highlights the formatted view</Copy> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
        <Action label="Formatted" colors={props.colors} selected={!sourceMode} onPress={() => setSourceMode(false)} />
        <Action label="Exact Source" colors={props.colors} selected={sourceMode} onPress={() => setSourceMode(true)} />
        <Action label={`Contents (${headings.length})`} colors={props.colors} selected={contentsOpen} onPress={() => setContentsOpen(!contentsOpen)} />
      </View>
      {contentsOpen ? <View style={{ flexDirection: "row", flexWrap: "wrap" }}>{headings.filter((heading) => !needle || heading.label.toLocaleLowerCase().includes(needle)).map((heading) => <Action key={heading.index} label={heading.label} colors={props.colors} onPress={() => navigateHeading(heading.index)} />)}</View> : null}
      {linkMessage ? <Copy colors={props.colors}>{linkMessage}</Copy> : null}
      {selected.content === null ? <Copy colors={props.colors}>This configured plan is unavailable.</Copy> :
        <ScrollView testID="plan-document" ref={scroller} style={{ minHeight: 280, maxHeight: 640 }} nestedScrollEnabled
          onLayout={(event) => { position.current.viewport = event.nativeEvent.layout.height; restoreDocument(); }}
          onContentSizeChange={(_width, height) => { position.current.height = height; restoreDocument(); }}
          onScrollBeginDrag={() => { position.current.restoring = false; }}
          onScroll={(event) => {
            const y = Math.max(0, event.nativeEvent.contentOffset.y);
            const state = position.current;
            if (state.restoring) {
              // Preserve a matching clamped restore for later measurement. A different
              // offset releases it: web wheel scrolling has no begin-drag callback.
              if (state.requested === null) return;
              if (Math.abs(y - state.requested) <= 1 && state.requested < state.target) return;
              state.restoring = false;
            }
            props.onDocumentScroll(y);
          }} scrollEventThrottle={100}>
          {sourceMode ? <Text selectable testID="plan-exact-source" style={{ color: props.colors.foreground, fontFamily: "monospace", fontSize: 13, lineHeight: 21 }}>{selected.content}</Text> : blocks.map((block) => <View key={block.line} testID={`plan-line-${block.line}`} onLayout={(event) => {
            offsets.current[block.line] = event.nativeEvent.layout.y;
            if (pendingHeading.current === block.line) { pendingHeading.current = null; scrollToHeading(block.line); }
          }} style={{ paddingVertical: 5 }}><FormattedPlanBlock block={block} colors={props.colors} query={props.query} onLink={openLink} /></View>)}
        </ScrollView>}
    </>}
  </>;
}
function RecordPage(props: PageProps) {
  const kind = props.tab === "Delivery" ? "delivery" : "decisions";
  const reads = Object.values(props.reads).filter((read) => read.source.kind === kind);
  return <>
    {!reads.length ? <Copy colors={props.colors}>{props.tab}: not connected to a configured {kind} source.</Copy> : null}
    {reads.map((read) => <Card key={read.source.id} title={read.source.title} colors={props.colors}>
      <SourceDetails read={read} colors={props.colors} />
      {kind === "delivery" ? read.records?.delivery.map((record) => <Copy key={record.id} colors={props.colors}>{record.kind} · {record.title} · recorded state: {record.state} · {record.recordedAt ?? "time Unknown"} {record.url ?? ""}</Copy>) : null}
      {kind === "decisions" ? read.records?.decisions.map((record) => <Copy key={record.id} colors={props.colors}>{record.id} · {record.title} · {record.state} · owner: {record.owner ?? "Unknown"} {record.detail ?? ""} {record.url ?? ""}</Copy>) : null}
      {read.status === "ready" && read.records?.[kind].length === 0 ? <Copy colors={props.colors}>No records in this source.</Copy> : null}
    </Card>)}
  </>;
}
export function ReadOnlyPage(props: PageProps) {
  const scroller = useRef<ScrollView | null>(null);
  const pendingRestore = useRef(props.initialOffset > 0);
  const results = useMemo(() => searchRecords(props.entries, Object.values(props.reads), props.query), [props.entries, props.reads, props.query]);
  return <ScrollView ref={scroller}
    onContentSizeChange={(_width, height) => {
      if (!pendingRestore.current || height <= 0) return;
      // Restore once per entry; the host clamps to its actual range. Release
      // before scrollTo so its onScroll callback persists the reachable offset.
      pendingRestore.current = false;
      scroller.current?.scrollTo({ y: Math.max(0, props.initialOffset), animated: false });
    }}
    onScrollBeginDrag={() => { pendingRestore.current = false; }}
    onScroll={(event) => {
      const y = Math.max(0, event.nativeEvent.contentOffset.y);
      if (pendingRestore.current && y === 0) return;
      // Wheel/web scrolling may arrive without a begin-drag callback.
      pendingRestore.current = false;
      props.onScroll(y);
    }} scrollEventThrottle={100} keyboardShouldPersistTaps="handled"
    contentContainerStyle={{ padding: 12, paddingBottom: 28, gap: 12 }}>
    <Text accessibilityRole="header" style={{ color: props.colors.foreground, fontSize: 20, fontWeight: "600" }}>{props.tab}</Text>
    {props.sourceError ? <Copy colors={props.colors}>{props.sourceError}</Copy> : null}
    {props.directory.error || props.directory.staleReason ? <Copy colors={props.colors}>Native inventory: {props.directory.error ?? props.directory.staleReason}</Copy> : null}
    {props.tab === "Overview" ? <Overview {...props} /> : null}
    {props.tab === "Plan" ? <Plan {...props} /> : null}
    {props.tab === "Inbox" ? <><Copy colors={props.colors}>Native needs-input, errors and attention in the current inventory. Opening a session uses Paseo's own controls. Unseen routing is not captured.</Copy><NativeRows {...props} entries={inboxEntries(props.entries)} />{!inboxEntries(props.entries).length ? <Copy colors={props.colors}>No matching native items in this read.</Copy> : null}</> : null}
    {props.tab === "Delivery" || props.tab === "Decisions" ? <RecordPage {...props} /> : null}
    {props.tab === "AI teams & sessions" ? <TeamsSourcePage sources={props.teamsSources ?? props.sources.filter((source) => source.kind === "teams")} snapshots={props.teamsSnapshots} errors={props.teamsErrors} bindingDiagnostics={props.teamsBindingDiagnostics} available={props.teamsAvailable ?? false} reads={props.teamsJsonReads ?? props.reads} entries={props.entries} observedAt={props.directory.lastUpdatedAt === null ? "Unknown" : new Date(props.directory.lastUpdatedAt).toISOString()} colors={props.colors}>
      <Card title="Observed native sessions" colors={props.colors}><Copy colors={props.colors}>Native sessions and labels are observations, not activated configured teams. Planned/governed assignment: Unknown.</Copy><NativeRows {...props} /></Card>
    </TeamsSourcePage> : null}
    {props.tab === "Activity" ? <><Copy colors={props.colors}>Partial coverage: current session updatedAt values only, as of {props.directory.lastUpdatedAt ? new Date(props.directory.lastUpdatedAt).toISOString() : "Unknown"}. This is not a durable audit log or a transcript history.</Copy>{[...props.entries].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).map((entry) => <View key={entry.id}><Action label={entry.title} colors={props.colors} onPress={() => props.onOpen(entry)} /><Copy colors={props.colors} muted>{entry.updatedAt} · native {entry.status}</Copy></View>)}</> : null}
    {props.tab === "Search" ? <><Copy colors={props.colors}>Scope: available session/workspace names, configured plan text and canonical task records. No private transcript search.</Copy><TextInput accessibilityLabel="Search available records" placeholder="Search available records" value={props.query} onChangeText={props.setQuery} style={{ color: props.colors.foreground, padding: 10, minHeight: 44, borderWidth: 1, borderColor: props.colors.border }} /><NativeRows {...props} entries={results.sessions} />{results.plans.map((read) => <Action key={read.source.id} colors={props.colors} label={`Plan: ${read.source.title}`} onPress={() => props.onSource(read.source.id)} />)}{results.tasks.map(({ node, source }) => <Action key={`${source.id}:${node.id}`} colors={props.colors} label={`${node.jira ?? node.id} · ${node.title}`} onPress={() => props.onSource(source.id)} />)}</> : null}
  </ScrollView>;
}
