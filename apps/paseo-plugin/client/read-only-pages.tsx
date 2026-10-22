import { useMemo, useRef, type ReactNode } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { Destination } from "./navigation-state";
import { confirmedEpic, inboxEntries, readTime, searchRecords, shortDates } from "./page-data";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { TEAMS_SESSIONS_NOTE, TeamsSourcePage, type TeamsView } from "./teams-source-page";
import { SourceDetails } from "./source-details";
import { Action, Copy, Disclosure, type Colors } from "./page-controls";
import { safeHttpsUrl } from "./plan-links";
import { OverviewPage } from "./overview-page";
import { PlanReader, type PlanView } from "./plan-reader";
import { attentionLabel, earlierErrorLabel } from "./session-view";
import { NO_LINKED_EPIC } from "./session-organization";
import { kontordSentence } from "./kontord-copy";

type PageProps = {
  teamsSources?: readonly ReadSourceSummary[]; teamsJsonReads?: Readonly<Record<string, SourceRead>>;
  teamsSnapshots?: Readonly<Record<string, TeamsSourceSnapshot>>; teamsErrors?: Readonly<Record<string, string>>;
  teamsBindingDiagnostics?: readonly string[]; teamsAvailable?: boolean; teamsView?: TeamsView; onTeamsView?: (view: TeamsView) => void;
  contextLabel?: string; scoped?: boolean; tab: Destination; colors: Colors; entries: readonly DirectoryEntry[]; directory: DirectoryState;
  compact?: boolean; onChooseSource?: () => void; onDocumentLink?: (target: string) => void; onExternalLink?: (target: string) => void;
  sources: readonly ReadSourceSummary[]; reads: Readonly<Record<string, SourceRead>>; sourceId: string | null;
  sourceError: string | null; query: string; setQuery: (query: string) => void;
  /** What kontor.status observed on this host; unknown when absent. */
  kontord?: string;
  onSource: (id: string) => void; onOpen: (entry: DirectoryEntry) => void; onTab: (tab: Destination) => void;
  onEpicTab: (tab: "Plan" | "Board" | "Graph" | "Sessions", epicKey: string | null, sourceId: string | null) => void;
  onGraph?: () => void; onCopy?: (text: string, what: string) => void; planView?: PlanView; onPlanView?: (view: PlanView) => void;
  initialOffset: number; onScroll: (offset: number) => void;
};
const day = (value: string) => value.slice(0, 10);
function sessionDetail(entry: DirectoryEntry): string {
  return [entry.status, attentionLabel(entry), earlierErrorLabel(entry), entry.workspaceName ?? "Unassigned workspace", confirmedEpic(entry) ?? NO_LINKED_EPIC].filter(Boolean).join(" · ");
}
/** Compact native session rows; opening one uses Paseo's own session view. `label` adds exact values to the row's label. */
function SessionRows({ entries, colors, onOpen, detail = sessionDetail, label }: { entries: readonly DirectoryEntry[]; colors: Colors; onOpen: (entry: DirectoryEntry) => void; detail?: (entry: DirectoryEntry) => string; label?: (entry: DirectoryEntry) => string }) {
  if (!entries.length) return null;
  return <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
    {entries.map((entry, index) => <Pressable key={entry.id} accessibilityRole="button" accessibilityLabel={`Open session ${entry.title}${label ? `, ${label(entry)}` : ""}`} onPress={() => onOpen(entry)}
      style={{ minHeight: 44, justifyContent: "center", gap: 2, paddingHorizontal: 12, paddingVertical: 7, borderTopWidth: index ? 1 : 0, borderColor: colors.border }}>
      <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{entry.title}</Text>
      <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 12 }}>{detail(entry)}</Text>
    </Pressable>)}
  </View>;
}
function Section({ title, titleLabel, detail, colors, children }: { title: string; titleLabel?: string; detail?: string; colors: Colors; children: ReactNode }) {
  return <View style={{ gap: 8 }}>
    <View style={{ flexDirection: "row", alignItems: "baseline", gap: 8, flexWrap: "wrap" }}>
      <Text accessibilityRole="header" accessibilityLabel={titleLabel} style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{title}</Text>
      {detail ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{detail}</Text> : null}
    </View>
    {children}
  </View>;
}
/** Visible: the recorded state with short dates; the exact recorded state is in the label and in Source details. */
const recordedLine = (line: string) => [shortDates(line), `recorded state: ${line}`] as const;
const SEARCH_SCOPE = "Scope: available session and workspace names, configured plan text and canonical task records. No private transcript search.";
/** A page's scope statement, one tap away where the page has no Source details. */
function AboutPage({ colors, children }: { colors: Colors; children: string }) {
  return <View testID="about-page"><Disclosure small label="About this page" colors={colors}><Copy colors={colors} muted small>{children}</Copy></Disclosure></View>;
}
const DELIVERY_GROUPS = [["pull request", "Pull requests"], ["check", "Checks"], ["release", "Releases"]] as const;
const DECISION_GROUPS = [["open", "Open"], ["unknown", "State unknown"], ["closed", "Closed"]] as const;
function RecordPage(props: PageProps) {
  const kind = props.tab === "Delivery" ? "delivery" : "decisions";
  const noun = kind === "delivery" ? "delivery" : "decision";
  const reads = Object.values(props.reads).filter((read) => read.source.kind === kind);
  const { colors } = props;
  // A line is shown with short dates; `[visible, label]` pairs keep a fuller label. Exact text stays in the label.
  const row = (key: string, index: number, title: string, lines: (string | readonly [string, string])[], link: ReactNode, extra?: ReactNode) => <View key={key} style={{ gap: 3, paddingHorizontal: 12, paddingVertical: 9, borderTopWidth: index ? 1 : 0, borderColor: colors.border }}>
    <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
        <Text selectable style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{title}</Text>
        {lines.map((line, number) => {
          const [shown, label] = typeof line === "string" ? [shortDates(line), line] : line;
          return <Text key={number} selectable accessibilityLabel={shown === label ? undefined : label} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{shown}</Text>;
        })}
      </View>
      {link}
    </View>
    {extra}
  </View>;
  // The page explanation and the kontord line are in each source's Source details; with no source, in About this page.
  const intro = `${kind === "delivery" ? "Pull requests, checks and releases recorded by configured sources. States are as captured, not live; delivery and acceptance are not inferred." : "Decisions recorded by configured sources. States are as captured, not live approval requests."} Governed ${noun} actions: not shown here. ${props.kontord ?? kontordSentence(null)}`;
  return <View style={{ gap: 14 }}>
    {!reads.length ? <View style={{ padding: 14, borderWidth: 1, borderStyle: "dashed", borderColor: colors.border, borderRadius: 10 }}>
      <Text selectable testID="record-empty" style={{ color: colors.foreground, fontSize: 13, lineHeight: 20 }}>{kind === "delivery" ? "Delivery" : "Decision"} records not captured: no source connected.</Text>
      <AboutPage colors={colors}>{`${props.tab}: not connected to a configured ${noun} source. An empty page here means not captured, not that nothing exists. ${intro}`}</AboutPage>
    </View> : null}
    {reads.map((read) => {
      const records = kind === "delivery" ? read.records?.delivery ?? [] : read.records?.decisions ?? [];
      const count = `${records.length} record${records.length === 1 ? "" : "s"}`;
      // The status, revision kind and exact read time are in the meta label and in Source details, which also lists
      // each delivery record's exact state and recorded time. A zero count is left to "No records in this source."
      // or to the status.
      return <Section key={read.source.id} title={read.source.title} colors={colors}>
        <SourceDetails read={read} colors={colors} notes={<>
          <Copy colors={colors} muted small>{intro}</Copy>
          {kind === "delivery" && records.length ? <View testID={`record-exact-${read.source.id}`}>
            <Copy colors={colors} muted small>Exact recorded values</Copy>
            {read.records?.delivery.map((record) => <Copy key={record.id} colors={colors} small>{`${record.title} · ${record.state} · recorded ${record.recordedAt ?? "Unknown"}`}</Copy>)}
          </View> : null}
        </>}
          summary={<Text testID={`record-meta-${read.source.id}`} accessibilityLabel={`${count} · ${read.status} · ${read.source.revisionKind} · Read ${read.readAt}`} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>
            {[records.length ? count : null, read.status === "ready" ? null : read.status, `read ${readTime(read.readAt)}`].filter(Boolean).join(" · ")}
          </Text>} />
        {read.status === "ready" && read.records && records.length === 0 ? <Copy colors={colors}>No records in this source.</Copy> : null}
        {kind === "delivery" ? DELIVERY_GROUPS.map(([value, label]) => {
          const group = read.records?.delivery.filter((record) => record.kind === value) ?? [];
          return group.length ? <View key={value} style={{ gap: 4 }}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 10, fontWeight: "600", letterSpacing: 0.4 }}>{label.toLocaleUpperCase()} · {group.length}</Text>
            <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
              {group.map((record, index) => row(record.id, index, record.title, [recordedLine(`${record.state} · ${record.recordedAt ? `recorded ${day(record.recordedAt)}` : "recorded time Unknown"}`)],
                <RecordLink url={record.url} label="Open evidence" title={record.title} colors={colors} compact={props.compact} onOpen={props.onExternalLink} />))}
            </View>
          </View> : null;
        }) : DECISION_GROUPS.map(([value, label]) => {
          const group = read.records?.decisions.filter((record) => record.state === value) ?? [];
          return group.length ? <View key={value} style={{ gap: 4 }}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 10, fontWeight: "600", letterSpacing: 0.4 }}>{label.toLocaleUpperCase()} · {group.length}</Text>
            <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
              {/* The group header shows the recorded state; the row label repeats it. */}
              {group.map((record, index) => row(record.id, index, `${record.id} · ${record.title}`, [[`Owner: ${record.owner ?? "Unknown"}`, `Owner: ${record.owner ?? "Unknown"} · recorded ${record.state}`]],
                <RecordLink url={record.url} label="Open decision source" title={record.title} colors={colors} compact={props.compact} onOpen={props.onExternalLink} />,
                record.detail ? <Disclosure small label="Recorded detail" colors={colors}><Copy colors={colors} small>{record.detail}</Copy></Disclosure> : null))}
            </View>
          </View> : null;
        })}
      </Section>;
    })}
  </View>;
}
function RecordLink({ url, label, title, colors, compact = false, onOpen }: { url?: string; label: string; title: string; colors: Colors; compact?: boolean; onOpen?: (target: string) => void }) {
  if (!url) return null;
  const safe = safeHttpsUrl(url);
  if (!safe) return <Copy colors={colors} muted small>Source link unavailable: a valid HTTPS URL without credentials is required.</Copy>;
  return <View style={{ alignItems: "flex-end", gap: 2, maxWidth: 160 }}>
    <Action small={!compact} label={label === "Open evidence" ? "Evidence" : "Source"} icon="ExternalLink" iconOnly accessibilityLabel={`${label}: ${title}`} colors={colors} disabled={!onOpen} onPress={() => onOpen?.(safe)} />
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
  const asOf = props.directory.lastUpdatedAt ? new Date(props.directory.lastUpdatedAt).toISOString() : "Unknown";
  // Pages without Source details keep their scope statement one tap away: About this page on Inbox
  // and Search, and the coverage cue's own disclosure on Activity.
  const inboxScope = "Native needs-input, errors and attention in the current inventory. Opening a session uses Paseo’s own controls. Unseen routing is not captured.";
  const activity = props.tab === "Activity" ? [...props.entries].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)) : [];
  const activityScope = `Partial coverage: current session updatedAt values only, as of ${asOf}. This is not a durable audit log or a transcript history.`;
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
    {props.tab !== "Overview" ? <Text accessibilityRole="header" testID="page-title" style={{ color: colors.foreground, fontSize: 20, fontWeight: "600", width: "100%", maxWidth: 1100, alignSelf: "center" }}>{props.tab}</Text> : null}
    {props.sourceError ? <Copy colors={colors}>{props.sourceError}</Copy> : null}
    {props.directory.error || props.directory.staleReason ? <Copy colors={colors} muted small>Native inventory: {props.directory.error ?? props.directory.staleReason}</Copy> : null}
    {props.tab === "Overview" ? <OverviewPage {...props} /> : null}
    <View style={{ width: "100%", maxWidth: 1100, alignSelf: "center", gap: 12 }}>
      {props.tab === "Inbox" ? <>
        <AboutPage colors={colors}>{inboxScope}</AboutPage>
        <SessionRows entries={inbox} colors={colors} onOpen={props.onOpen} />
        {!inbox.length ? <Copy colors={colors}>Nothing needs input or attention in this read.</Copy> : null}
      </> : null}
      {props.tab === "Delivery" || props.tab === "Decisions" ? <RecordPage {...props} /> : null}
      {props.tab === "AI teams & sessions" ? <TeamsSourcePage sources={props.teamsSources ?? props.sources.filter((source) => source.kind === "teams")} snapshots={props.teamsSnapshots} errors={props.teamsErrors} bindingDiagnostics={props.teamsBindingDiagnostics} available={props.teamsAvailable ?? false} reads={props.teamsJsonReads ?? props.reads} entries={props.entries} observedAt={props.directory.lastUpdatedAt === null ? "Unknown" : new Date(props.directory.lastUpdatedAt).toISOString()} colors={colors} view={props.teamsView} onView={props.onTeamsView} kontord={props.kontord}>
        {/* The context picker names the context; a zero count is left to the empty sentence. */}
        <Section title="Sessions in this context" titleLabel={`Native sessions in ${props.contextLabel ?? "All projects"}. ${TEAMS_SESSIONS_NOTE}`} detail={props.entries.length ? `${props.entries.length} session${props.entries.length === 1 ? "" : "s"}` : undefined} colors={colors}>
          <SessionRows entries={props.entries} colors={colors} onOpen={props.onOpen} detail={(entry) => [entry.labels.role ? `role ${entry.labels.role}` : "role Unknown", entry.status, entry.workspaceName ?? "Unassigned workspace"].join(" · ")} />
          {!props.entries.length ? <Copy colors={colors}>No sessions in this context.</Copy> : null}
        </Section>
      </TeamsSourcePage> : null}
      {props.tab === "Activity" ? <>
        {/* Short partial-coverage cue; the full statement, its read time and each session's exact update time are inside it. */}
        <View testID="activity-coverage"><Disclosure small label="Latest update per session · not a full log" colors={colors}>
          <Copy colors={colors} muted small>{activityScope}</Copy>
          {activity.map((entry) => <Copy key={entry.id} colors={colors} small>{`${entry.title} · updated ${entry.updatedAt}`}</Copy>)}
        </Disclosure></View>
        <SessionRows entries={activity} colors={colors} onOpen={props.onOpen}
          detail={(entry) => `${readTime(entry.updatedAt)} · ${entry.status}`} label={(entry) => `updated ${entry.updatedAt}, native ${entry.status}`} />
        {!props.entries.length ? <Copy colors={colors}>No sessions in this context.</Copy> : null}
      </> : null}
      {props.tab === "Search" ? <>
        <TextInput accessibilityLabel="Search available records" accessibilityHint={SEARCH_SCOPE} placeholder="Search sessions, plans and tasks" placeholderTextColor={colors.foregroundMuted} value={props.query} onChangeText={props.setQuery} style={{ color: colors.foreground, fontSize: 13, paddingHorizontal: 10, minHeight: 44, borderWidth: 1, borderColor: colors.border, borderRadius: 8 }} />
        <AboutPage colors={colors}>{SEARCH_SCOPE}</AboutPage>
        {results.sessions.length ? <Section title="Sessions" detail={String(results.sessions.length)} colors={colors}><SessionRows entries={results.sessions} colors={colors} onOpen={props.onOpen} /></Section> : null}
        {results.plans.length ? <Section title="Plans" detail={String(results.plans.length)} colors={colors}><View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{results.plans.map((read) => <Action key={read.source.id} small={!props.compact} colors={colors} label={read.source.title} accessibilityLabel={`Plan: ${read.source.title}`} onPress={() => props.onSource(read.source.id)} />)}</View></Section> : null}
        {results.tasks.length ? <Section title="Plan tasks" detail={String(results.tasks.length)} colors={colors}><View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>{results.tasks.map(({ node, source }) => <Action key={`${source.id}:${node.id}`} small={!props.compact} colors={colors} label={`${node.jira ?? node.id} · ${node.title}`} onPress={() => props.onSource(source.id)} />)}</View></Section> : null}
        {!results.sessions.length && !results.plans.length && !results.tasks.length ? <Copy colors={colors}>No matching records.</Copy> : null}
      </> : null}
    </View>
  </ScrollView>;
}
