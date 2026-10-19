import { useMemo, useRef, type ReactNode } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { Destination } from "./navigation-state";
import { confirmedEpic, inboxEntries, searchRecords } from "./page-data";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { TeamsSourcePage, type TeamsView } from "./teams-source-page";
import { SourceDetails } from "./source-details";
import { Action, Copy, Disclosure, type Colors } from "./page-controls";
import { safeHttpsUrl } from "./plan-links";
import { OverviewPage } from "./overview-page";
import { PlanReader, type PlanView } from "./plan-reader";

type PageProps = {
  teamsSources?: readonly ReadSourceSummary[]; teamsJsonReads?: Readonly<Record<string, SourceRead>>;
  teamsSnapshots?: Readonly<Record<string, TeamsSourceSnapshot>>; teamsErrors?: Readonly<Record<string, string>>;
  teamsBindingDiagnostics?: readonly string[]; teamsAvailable?: boolean; teamsView?: TeamsView; onTeamsView?: (view: TeamsView) => void;
  contextLabel?: string; scoped?: boolean; tab: Destination; colors: Colors; entries: readonly DirectoryEntry[]; directory: DirectoryState;
  compact?: boolean; onChooseSource?: () => void; onDocumentLink?: (target: string) => void; onExternalLink?: (target: string) => void;
  sources: readonly ReadSourceSummary[]; reads: Readonly<Record<string, SourceRead>>; sourceId: string | null;
  sourceError: string | null; query: string; setQuery: (query: string) => void;
  onSource: (id: string) => void; onOpen: (entry: DirectoryEntry) => void; onTab: (tab: Destination) => void;
  onEpicTab: (tab: "Plan" | "Board" | "Graph" | "Sessions", epicKey: string | null, sourceId: string | null) => void;
  onGraph?: () => void; planView?: PlanView; onPlanView?: (view: PlanView) => void;
  initialOffset: number; onScroll: (offset: number) => void;
};
const day = (value: string) => value.slice(0, 10);
function sessionDetail(entry: DirectoryEntry): string {
  const attention = entry.needsInput ? "needs input" : entry.status === "error" ? "error" : entry.attentionReason ?? (entry.requiresAttention ? "native attention" : null);
  return [entry.status, attention, entry.workspaceName ?? "Unassigned workspace", confirmedEpic(entry) ?? "No epic"].filter(Boolean).join(" · ");
}
/** Compact native session rows; opening one uses Paseo's own session view. */
function SessionRows({ entries, colors, onOpen, detail = sessionDetail }: { entries: readonly DirectoryEntry[]; colors: Colors; onOpen: (entry: DirectoryEntry) => void; detail?: (entry: DirectoryEntry) => string }) {
  if (!entries.length) return null;
  return <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
    {entries.map((entry, index) => <Pressable key={entry.id} accessibilityRole="button" accessibilityLabel={`Open session ${entry.title}`} onPress={() => onOpen(entry)}
      style={{ minHeight: 44, justifyContent: "center", gap: 2, paddingHorizontal: 12, paddingVertical: 7, borderTopWidth: index ? 1 : 0, borderColor: colors.border }}>
      <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{entry.title}</Text>
      <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 12 }}>{detail(entry)}</Text>
    </Pressable>)}
  </View>;
}
function Section({ title, detail, colors, children }: { title: string; detail?: string; colors: Colors; children: ReactNode }) {
  return <View style={{ gap: 8 }}>
    <View style={{ flexDirection: "row", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{title}</Text>
      {detail ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{detail}</Text> : null}
    </View>
    {children}
  </View>;
}
const DELIVERY_GROUPS = [["pull request", "Pull requests"], ["check", "Checks"], ["release", "Releases"]] as const;
const DECISION_GROUPS = [["open", "Open"], ["unknown", "State unknown"], ["closed", "Closed"]] as const;
function RecordPage(props: PageProps) {
  const kind = props.tab === "Delivery" ? "delivery" : "decisions";
  const noun = kind === "delivery" ? "delivery" : "decision";
  const reads = Object.values(props.reads).filter((read) => read.source.kind === kind);
  const { colors } = props;
  const row = (key: string, index: number, title: string, lines: string[], link: ReactNode, extra?: ReactNode) => <View key={key} style={{ gap: 3, paddingHorizontal: 12, paddingVertical: 9, borderTopWidth: index ? 1 : 0, borderColor: colors.border }}>
    <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <Text selectable style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{title}</Text>
        {lines.map((line, number) => <Text key={number} selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{line}</Text>)}
      </View>
      {link}
    </View>
    {extra}
  </View>;
  return <View style={{ gap: 14 }}>
    <Copy colors={colors} muted small>{kind === "delivery" ? "Pull requests, checks and releases recorded by configured sources. States are as captured, not live; delivery and acceptance are not inferred." : "Decisions recorded by configured sources. States are as captured, not live approval requests."} Governed {noun} actions: kontord is not enabled.</Copy>
    {!reads.length ? <View style={{ padding: 14, borderWidth: 1, borderStyle: "dashed", borderColor: colors.border, borderRadius: 10 }}>
      <Copy colors={colors}>{props.tab}: not connected to a configured {noun} source. An empty page here means not captured, not that nothing exists.</Copy>
    </View> : null}
    {reads.map((read) => {
      const records = kind === "delivery" ? read.records?.delivery ?? [] : read.records?.decisions ?? [];
      return <Section key={read.source.id} title={read.source.title} detail={`${records.length} record${records.length === 1 ? "" : "s"} · ${read.status} · ${read.source.revisionKind} · Read ${read.readAt}`} colors={colors}>
        <SourceDetails bare read={read} colors={colors} />
        {read.status === "ready" && read.records && records.length === 0 ? <Copy colors={colors}>No records in this source.</Copy> : null}
        {kind === "delivery" ? DELIVERY_GROUPS.map(([value, label]) => {
          const group = read.records?.delivery.filter((record) => record.kind === value) ?? [];
          return group.length ? <View key={value} style={{ gap: 4 }}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 10, fontWeight: "600", letterSpacing: 0.4 }}>{label.toLocaleUpperCase()} · {group.length}</Text>
            <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
              {group.map((record, index) => row(record.id, index, record.title, [`recorded state: ${record.state} · ${record.recordedAt ? `recorded ${day(record.recordedAt)}` : "recorded time Unknown"}`],
                <RecordLink url={record.url} label="Open evidence" title={record.title} colors={colors} onOpen={props.onExternalLink} />))}
            </View>
          </View> : null;
        }) : DECISION_GROUPS.map(([value, label]) => {
          const group = read.records?.decisions.filter((record) => record.state === value) ?? [];
          return group.length ? <View key={value} style={{ gap: 4 }}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 10, fontWeight: "600", letterSpacing: 0.4 }}>{label.toLocaleUpperCase()} · {group.length}</Text>
            <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
              {group.map((record, index) => row(record.id, index, `${record.id} · ${record.title}`, [`Owner: ${record.owner ?? "Unknown"} · recorded ${record.state}`],
                <RecordLink url={record.url} label="Open decision source" title={record.title} colors={colors} onOpen={props.onExternalLink} />,
                record.detail ? <Disclosure small label="Recorded detail" colors={colors}><Copy colors={colors} small>{record.detail}</Copy></Disclosure> : null))}
            </View>
          </View> : null;
        })}
      </Section>;
    })}
  </View>;
}
function RecordLink({ url, label, title, colors, onOpen }: { url?: string; label: string; title: string; colors: Colors; onOpen?: (target: string) => void }) {
  if (!url) return null;
  const safe = safeHttpsUrl(url);
  if (!safe) return <Copy colors={colors} muted small>Source link unavailable: a valid HTTPS URL without credentials is required.</Copy>;
  return <View style={{ alignItems: "flex-end", gap: 2, maxWidth: 160 }}>
    <Action small label={label === "Open evidence" ? "Evidence" : "Source"} accessibilityLabel={`${label}: ${title}`} colors={colors} disabled={!onOpen} onPress={() => onOpen?.(safe)} />
    {!onOpen ? <Copy colors={colors} muted small>Source link opening is unavailable on this host.</Copy> : null}
  </View>;
}
export function ReadOnlyPage(props: PageProps) {
  const scroller = useRef<ScrollView | null>(null);
  const pendingRestore = useRef(props.initialOffset > 0);
  const results = useMemo(() => searchRecords(props.entries, Object.values(props.reads), props.query), [props.entries, props.reads, props.query]);
  if (props.tab === "Plan") return <PlanReader {...props} view={props.planView} onView={props.onPlanView} />;
  const { colors } = props;
  const inbox = props.tab === "Inbox" ? inboxEntries(props.entries) : [];
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
    {props.tab !== "Overview" ? <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 20, fontWeight: "600", width: "100%", maxWidth: 1100, alignSelf: "center" }}>{props.tab}</Text> : null}
    {props.sourceError ? <Copy colors={colors}>{props.sourceError}</Copy> : null}
    {props.directory.error || props.directory.staleReason ? <Copy colors={colors} muted small>Native inventory: {props.directory.error ?? props.directory.staleReason}</Copy> : null}
    {props.tab === "Overview" ? <OverviewPage {...props} /> : null}
    <View style={{ width: "100%", maxWidth: 1100, alignSelf: "center", gap: 12 }}>
      {props.tab === "Inbox" ? <>
        <Copy colors={colors} muted small>Native needs-input, errors and attention in the current inventory. Opening a session uses Paseo’s own controls. Unseen routing is not captured.</Copy>
        <SessionRows entries={inbox} colors={colors} onOpen={props.onOpen} />
        {!inbox.length ? <Copy colors={colors}>No matching native items in this read.</Copy> : null}
      </> : null}
      {props.tab === "Delivery" || props.tab === "Decisions" ? <RecordPage {...props} /> : null}
      {props.tab === "AI teams & sessions" ? <TeamsSourcePage sources={props.teamsSources ?? props.sources.filter((source) => source.kind === "teams")} snapshots={props.teamsSnapshots} errors={props.teamsErrors} bindingDiagnostics={props.teamsBindingDiagnostics} available={props.teamsAvailable ?? false} reads={props.teamsJsonReads ?? props.reads} entries={props.entries} observedAt={props.directory.lastUpdatedAt === null ? "Unknown" : new Date(props.directory.lastUpdatedAt).toISOString()} colors={colors} view={props.teamsView} onView={props.onTeamsView}>
        <Section title="Sessions in this context" detail={`${props.entries.length} native session${props.entries.length === 1 ? "" : "s"} · ${props.contextLabel ?? "All projects"}`} colors={colors}>
          <Copy colors={colors} muted small>Native sessions and labels are observations, not activated configured teams. Planned or governed assignment: Unknown.</Copy>
          <SessionRows entries={props.entries} colors={colors} onOpen={props.onOpen} detail={(entry) => [entry.labels.role ? `role ${entry.labels.role}` : "role Unknown", entry.status, entry.workspaceName ?? "Unassigned workspace"].join(" · ")} />
          {!props.entries.length ? <Copy colors={colors}>No native sessions in this context.</Copy> : null}
        </Section>
      </TeamsSourcePage> : null}
      {props.tab === "Activity" ? <>
        <Copy colors={colors} muted small>Partial coverage: current session updatedAt values only, as of {props.directory.lastUpdatedAt ? new Date(props.directory.lastUpdatedAt).toISOString() : "Unknown"}. This is not a durable audit log or a transcript history.</Copy>
        <SessionRows entries={[...props.entries].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))} colors={colors} onOpen={props.onOpen} detail={(entry) => `${entry.updatedAt} · native ${entry.status}`} />
      </> : null}
      {props.tab === "Search" ? <>
        <Copy colors={colors} muted small>Scope: available session and workspace names, configured plan text and canonical task records. No private transcript search.</Copy>
        <TextInput accessibilityLabel="Search available records" placeholder="Search available records" placeholderTextColor={colors.foregroundMuted} value={props.query} onChangeText={props.setQuery} style={{ color: colors.foreground, fontSize: 13, paddingHorizontal: 10, minHeight: 44, borderWidth: 1, borderColor: colors.border, borderRadius: 8 }} />
        {results.sessions.length ? <Section title="Sessions" detail={String(results.sessions.length)} colors={colors}><SessionRows entries={results.sessions} colors={colors} onOpen={props.onOpen} /></Section> : null}
        {results.plans.length ? <Section title="Plans" detail={String(results.plans.length)} colors={colors}><View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{results.plans.map((read) => <Action key={read.source.id} small={!props.compact} colors={colors} label={`Plan: ${read.source.title}`} onPress={() => props.onSource(read.source.id)} />)}</View></Section> : null}
        {results.tasks.length ? <Section title="Plan tasks" detail={String(results.tasks.length)} colors={colors}><View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{results.tasks.map(({ node, source }) => <Action key={`${source.id}:${node.id}`} small={!props.compact} colors={colors} label={`${node.jira ?? node.id} · ${node.title}`} onPress={() => props.onSource(source.id)} />)}</View></Section> : null}
        {!results.sessions.length && !results.plans.length && !results.tasks.length ? <Copy colors={colors}>No matching records.</Copy> : null}
      </> : null}
    </View>
  </ScrollView>;
}
