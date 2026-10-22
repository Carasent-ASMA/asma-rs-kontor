import { useState } from "react";
import { Pressable, Text, View } from "react-native";
import type { SourceRead } from "../shared/read-sources";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { confirmedEpic, nativeSummary, portfolioFilter, portfolioRows, readTime, shortDates, type PortfolioFilter, type PortfolioRow } from "./page-data";
import { inventoryReadLabel } from "./native-freshness";
import { SourceDetails } from "./source-details";
import { Action, Copy, Disclosure, type Colors } from "./page-controls";
import { kontordSentence } from "./kontord-copy";

type EpicTab = "Plan" | "Board" | "Graph" | "Sessions";
type Props = {
  colors: Colors; compact?: boolean; contextLabel?: string; scoped?: boolean; entries: readonly DirectoryEntry[]; directory: DirectoryState;
  reads: Readonly<Record<string, SourceRead>>; query: string; setQuery: (query: string) => void;
  sourceError?: string | null; kontord?: string;
  onSource: (id: string) => void; onEpicTab: (tab: EpicTab, epicKey: string | null, sourceId: string | null) => void;
};
const FILTERS: readonly PortfolioFilter[] = ["all", "active", "review", "decisions", "completed", "delivered"];
const day = (value: string) => value.split(" / ").map((part) => part.slice(0, 10)).join(" / ");
const count = (value: number, noun: string) => `${value} ${noun}${value === 1 ? "" : "s"}`;
/** Visible signal line: non-zero native counts only, or one "No sessions". */
function signalLine(sessions: ReturnType<typeof nativeSummary>): string {
  if (!sessions.total) return "No sessions";
  return [count(sessions.total, "session"), sessions.running ? `${sessions.running} running` : null, sessions.attention ? `${sessions.attention} attention` : null,
    sessions.needsInput ? `${sessions.needsInput} need input` : null, sessions.errors ? count(sessions.errors, "error") : null].filter(Boolean).join(" · ");
}
/** A visible cue only when the native read is not a complete one; the exact read stays in Sources and coverage. */
function inventoryCue(directory: DirectoryState): string | null {
  if (directory.lastUpdatedAt === null) return "inventory not read yet";
  if (directory.inventoryComplete === true) return null;
  return `${directory.inventoryComplete === false ? "partial inventory" : "inventory completeness Unknown"} · read ${readTime(directory.lastUpdatedAt)}`;
}

/** The prototype portfolio: six summary filters and one compact row per epic, within a readable width. */
export function OverviewPage(props: Props) {
  const { colors } = props;
  const [width, setWidth] = useState(0);
  const reads = Object.values(props.reads);
  const rows = portfolioRows(props.entries, reads, !props.scoped);
  const filter = FILTERS.find((key) => key === props.query) ?? "all";
  const shown = rows.filter((row) => portfolioFilter(row, filter));
  const matching = (key: PortfolioFilter) => rows.filter((row) => portfolioFilter(row, key)).length;
  const undecided = rows.filter((row) => row.decisions === null).length;
  // The fourth entry is the tile's full meaning (its label); only a partial-capture qualifier stays visible.
  const tiles: Array<[PortfolioFilter, string, number | string, string, string | null]> = [
    ["all", "Epics", rows.length, "All work", null],
    ["active", "In progress", matching("active"), "Epics with running sessions", null],
    ["review", "Needs attention", matching("review"), "Native attention, input or error", null],
    ["decisions", "Needs your decision", undecided === rows.length ? "Not captured" : rows.filter((row) => (row.decisions ?? 0) > 0).length, undecided === rows.length ? "Decision records unavailable" : `Recorded open decisions${undecided ? ` · ${undecided} not captured` : ""}`, undecided && undecided < rows.length ? `${undecided} not captured` : null],
    ["completed", "Completed epics", rows.some((row) => row.lifecycle) ? matching("completed") : "Unknown", "Recorded epic lifecycle", null],
    ["delivered", "Delivered", "Unknown", "Delivery confirmation not captured", null],
  ];
  const total = nativeSummary(props.entries);
  const loose = props.entries.filter((entry) => confirmedEpic(entry) === null).length;
  const jira = reads.filter((read) => read.source.kind === "jira");
  const observed = [...new Set(jira.flatMap((read) => read.status === "ready" && read.jiraHistory ? [day(read.jiraHistory.generated)] : []))];
  const wide = width >= 760;
  const sessions = `${props.scoped ? "Sessions in this scope" : "Host sessions"}: ${count(total.total, "session")} · ${total.running} running · ${total.attention} native attention · ${total.needsInput} needs input · ${total.errors} errors.${!props.scoped && loose ? ` ${loose} with an unknown epic stay in Sessions.` : ""}`;
  const scope = `${props.scoped ? "Project" : "Global"} overview, available without kontord.`;
  const meta = [signalLine(total), !props.scoped && loose ? `${loose} epic unknown` : null, inventoryCue(props.directory)].filter(Boolean).join(" · ");
  const jiraLabel = observed.length ? `Jira captured ${observed.map(shortDates).join(" / ")}` : "Jira not connected";
  return <View onLayout={(event) => setWidth(event.nativeEvent.layout.width)} style={{ width: "100%", maxWidth: 1100, alignSelf: "center", gap: 14 }}>
    {/* One title and one meta line; scope, exact counts and the read time stay in Sources and coverage. */}
    <View style={{ gap: props.compact ? 3 : 4 }}>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: props.compact ? 21 : 25, fontWeight: "600" }}>{props.contextLabel ?? "All projects"}</Text>
      <Text testID="overview-meta" accessibilityLabel={`${scope} ${sessions} ${inventoryReadLabel(props.directory)}.`} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{meta}</Text>
    </View>
    <Disclosure small label={`Sources and coverage · ${jiraLabel}`} accessibilityLabel={`Sources and coverage · ${jiraLabel}${observed.length ? ", not live" : ""}`} colors={colors}>
      <View testID="overview-sources" style={{ gap: 8, paddingTop: 4, paddingBottom: 6 }}>
        <Copy colors={colors} muted small>{scope} Native sessions come from this host. The Native status at the top describes that host connection only; plans, Jira and records are captured sources with their own dates.</Copy>
        <Copy colors={colors} muted small>{sessions}</Copy>
        <Copy colors={colors} muted small>{inventoryReadLabel(props.directory)}</Copy>
        {jira.map((read) => <SourceDetails key={read.source.id} read={read} colors={colors} />)}
        {!jira.some((read) => read.jiraHistory) ? <Copy colors={colors} small>Historical Jira: not connected to a configured status source.</Copy> : null}
        {reads.filter((read) => read.source.kind !== "jira" && read.source.kind !== "teams").map((read) => <SourceDetails key={read.source.id} read={read} colors={colors} />)}
        <Copy colors={colors} muted small>Progress counts the captured Jira child records of each epic; Won’t Do records are excluded and full backlog coverage is not established. Decisions and delivery appear only where a configured source records them; delivery confirmation is never inferred.</Copy>
        <Copy colors={colors} muted small>Epic lifecycle, native activity, task status and delivery are separate observations.</Copy>
        <Copy colors={colors} muted small>Governed scheduler and gates: not shown here. {props.kontord ?? kontordSentence(null)}</Copy>
      </View>
    </Disclosure>
    <View accessibilityLabel="Portfolio filters" style={{ flexDirection: "row", flexWrap: "wrap", gap: 10 }}>
      {tiles.map(([key, label, value, help, qualifier]) => <Pressable key={key} accessibilityRole="button" accessibilityState={{ selected: filter === key }} aria-pressed={filter === key} accessibilityLabel={`${label}: ${value}. ${help}`} onPress={() => props.setQuery(key === "all" ? "" : key)}
        style={{ flexGrow: 1, flexBasis: width >= 900 ? "30%" : "45%", gap: 2, padding: props.compact ? 10 : 12, borderWidth: 1, borderRadius: 10, borderColor: filter === key ? colors.accent : colors.border, backgroundColor: filter === key ? colors.surface2 : colors.surface1 }}>
        <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{label}</Text>
        <Text style={{ color: typeof value === "number" ? colors.foreground : colors.foregroundMuted, fontSize: typeof value === "number" ? 24 : 16, lineHeight: 30, fontWeight: "600" }}>{value}</Text>
        {qualifier ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{qualifier}</Text> : null}
      </Pressable>)}
    </View>
    {filter === "decisions" && undecided ? <Copy colors={colors} muted small>Decision records were not captured for {undecided} of these epics; they are shown as Unknown. This does not mean none are waiting.</Copy> : null}
    {filter === "delivered" ? <Copy colors={colors} muted small>Delivery confirmations were not captured, so every epic is shown as Unknown. Recorded pull requests and checks do not establish delivery.</Copy> : null}
    <View style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1, overflow: "hidden" }}>
      <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 10, paddingHorizontal: 14, paddingVertical: 11, borderBottomWidth: 1, borderColor: colors.border }}>
        <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{tiles.find(([key]) => key === filter)![1]}</Text>
        {shown.length !== rows.length ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{shown.length} of {rows.length} shown</Text> : null}
      </View>
      {shown.length ? shown.map((row, index) => <PortfolioItem key={row.id} row={row} last={index === shown.length - 1} wide={wide} {...props} />)
        : <View style={{ padding: 14, gap: 8, alignItems: "flex-start" }}><Copy colors={colors}>{rows.length ? "No epics in this group." : "No epics, plans or Jira records in this scope."}</Copy>{rows.length ? <Action small={!props.compact} label="Show all epics" colors={colors} onPress={() => props.setQuery("")} /> : null}</View>}
    </View>
  </View>;
}
function PortfolioItem({ row, last, wide, colors, compact, onSource, onEpicTab, sourceError }: Props & { row: PortfolioRow; last: boolean; wide: boolean }) {
  const single = row.plans.length === 1 ? row.plans[0]!.source.id : null;
  const go = (tab: EpicTab) => row.epicKey === null && tab === "Plan" && single ? onSource(single) : onEpicTab(tab, row.epicKey, tab === "Sessions" ? null : single);
  const sessions = row.sessions;
  // Full statements stay in the row's label; the visible lines keep only what is captured and non-zero.
  const planProblem = row.plans.length === 1 && row.plans[0]!.status !== "ready" ? row.plans[0]!.status === "partial" ? "Plan graph unavailable" : "Plan unavailable" : null;
  const next = planProblem ?? (row.plans.length === 1 ? "Captured plan available" : row.plans.length > 1 ? `${row.plans.length} configured plans · choose one in the context picker` : sourceError ? "Plan sources unavailable" : row.tasks ? "Plan not captured · captured tasks remain available" : "No plan linked");
  const nextLine = planProblem ?? (row.plans.length === 1 ? null : row.plans.length > 1 ? `${row.plans.length} configured plans` : sourceError ? "Plan sources unavailable" : row.tasks ? "Plan not captured" : "No plan linked");
  const signals = row.epicKey ? `${count(sessions.total, "session")} · ${sessions.running} running · ${sessions.attention} native attention · ${sessions.needsInput} needs input · ${sessions.errors} errors` : "No epic linked · sessions are not attributed to this plan";
  const records = `Decisions: ${row.decisions === null ? "Not captured" : `${row.decisions} open recorded`} · Delivery: ${row.delivery === null ? "Not captured" : `${row.delivery} recorded, confirmation Unknown`}`;
  const recordLine = [row.decisions === null ? null : row.decisions ? count(row.decisions, "open decision") : "No open decisions",
    row.delivery === null ? null : `${row.delivery ? count(row.delivery, "delivery record") : "No delivery records"} · confirmation Unknown`].filter(Boolean).join(" · ");
  const stage = row.epicKey === null ? "No epic linked" : row.lifecycle ? `Epic · ${row.lifecycle.status}` : "Epic status unknown";
  const progress = row.tasks && row.tasks.total > 0 ? `${row.tasks.done} / ${row.tasks.total} captured tasks done` : null;
  const cta = row.plans.length ? "Open plan" : "Inspect tasks";
  const coverage = row.tasks ? `${count(row.tasks.total + row.tasks.excluded, "Jira record")}${row.tasks.excluded ? ` · ${row.tasks.excluded} Won’t Do excluded` : ""} · observed ${day(row.tasks.observedAt)} · full backlog coverage not established` : row.epicKey ? "Jira records not captured" : "No Jira epic";
  // The capture day is said once, beside the stage, unless the task capture differs.
  const coverageLine = row.tasks ? `${count(row.tasks.total + row.tasks.excluded, "Jira record")}${row.tasks.excluded ? ` · ${row.tasks.excluded} Won’t Do excluded` : ""}${row.tasks.observedAt !== row.lifecycle?.observedAt ? ` · captured ${shortDates(day(row.tasks.observedAt))}` : ""} · full backlog coverage not established` : row.epicKey ? "Jira records not captured" : null;
  const small = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 };
  const stageView = <View style={{ width: wide ? 175 : undefined, gap: 2 }}>
    <View style={{ alignSelf: "flex-start", paddingHorizontal: 8, paddingVertical: 2, borderRadius: 999, backgroundColor: row.lifecycle?.done ? colors.statusSuccess : colors.surface2 }}>
      <Text style={{ color: row.lifecycle?.done ? colors.surface0 : colors.foregroundMuted, fontSize: 11, fontWeight: "600" }}>{stage}</Text>
    </View>
    {row.lifecycle ? <Text accessibilityLabel={`Jira, observed ${day(row.lifecycle.observedAt)}`} style={{ color: colors.foregroundMuted, fontSize: 10 }}>Jira, captured {shortDates(day(row.lifecycle.observedAt))}</Text> : null}
  </View>;
  const progressView = <View style={{ width: wide ? 140 : undefined, flexGrow: wide ? 0 : 1, maxWidth: 220, gap: 4 }}>
    {progress ? <><View accessible accessibilityRole="progressbar" accessibilityLabel={`${row.title} · ${progress}`} accessibilityValue={{ min: 0, max: row.tasks!.total, now: row.tasks!.done }} style={{ height: 6, borderRadius: 999, backgroundColor: colors.surface2, overflow: "hidden" }}>
      <View style={{ width: `${Math.round(100 * row.tasks!.done / row.tasks!.total)}%`, height: "100%", backgroundColor: colors.accent }} />
    </View><Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{progress}</Text></> : <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Progress unavailable</Text>}
  </View>;
  const ctaView = <Text style={{ width: wide ? 100 : undefined, textAlign: wide ? "right" : "left", color: colors.accent, fontSize: 12, fontWeight: "700" }}>{cta} →</Text>;
  return <View style={{ borderBottomWidth: last ? 0 : 1, borderColor: colors.border }}>
    <Pressable accessibilityRole="button" accessibilityLabel={`${row.title}. ${next}. ${stage}. ${progress ?? "Progress unavailable"}. ${cta}. ${signals}. ${records}. ${coverage}`} onPress={() => go(row.plans.length ? "Plan" : "Board")}
      style={{ flexDirection: wide ? "row" : "column", alignItems: wide ? "center" : "stretch", gap: wide ? 16 : 10, paddingHorizontal: 14, paddingTop: 14, paddingBottom: 10 }}>
      <View style={{ flex: wide ? 1 : undefined, minWidth: 0, gap: 3 }}>
        <Text style={{ color: colors.foreground, fontSize: 15, fontWeight: "700" }}>{row.title}</Text>
        {nextLine ? <Text style={small}>{nextLine}</Text> : null}
        {row.epicKey ? <Text style={small}>{signalLine(sessions)}</Text> : null}
        {recordLine ? <Text style={small}>{recordLine}</Text> : null}
      </View>
      {wide ? <>{stageView}{progressView}{ctaView}</> : <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 12 }}>{stageView}{progressView}{ctaView}</View>}
    </Pressable>
    <View style={{ flexDirection: wide ? "row" : "column", alignItems: wide ? "center" : "flex-start", justifyContent: "space-between", gap: 7, paddingHorizontal: 14, paddingBottom: 12 }}>
      {coverageLine ? <Text accessibilityLabel={coverage} style={{ color: colors.foregroundMuted, fontSize: 10, lineHeight: 15, maxWidth: wide ? "55%" : undefined }}>{coverageLine}</Text> : <View />}
      <View accessibilityLabel={`${row.title} views`} style={{ flexDirection: "row", flexWrap: "wrap", gap: 5 }}>
        {([["Plan", "Plan"], ["Board", "Tasks"], ["Graph", "Graph"], ["Sessions", "Sessions"]] as const).filter(([tab]) => tab !== "Sessions" || row.epicKey).map(([tab, label]) =>
          <Action key={tab} small={!compact} label={label} accessibilityLabel={`${label}: ${row.title}`} colors={colors} onPress={() => go(tab)} />)}
      </View>
    </View>
  </View>;
}
