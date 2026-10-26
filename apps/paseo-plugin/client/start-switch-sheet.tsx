import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { Platform, Pressable, ScrollView, Text, View } from "react-native";
import { Icon, TextInput, useToast } from "@getpaseo/plugin/client/react-native";
import { useHosts, useRpc, type PluginButtonContentProps, type PluginHostSummary, type PluginOpenScreenInput } from "@getpaseo/plugin/client";
import { organizationPresentation, DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { readCreateOptions, type CreateOptions, type CreateOutcome, type CreateScope } from "../shared/session-create";
import { handOffOpen, withdrawHandOff, type HandOffTarget, type OpenSessionResult } from "./session-actions";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { ControlHelpProvider, useActionHelp } from "./control-help";
import { cancelDelayedOpen, DenseSessionRow, rowTimeLines } from "./dense-session-row";
import { NewLsaForm } from "./new-lsa-form";
import { focusControl } from "./mobile-filters";
import { StateToolbar } from "./mobile-quick-toolbar";
import { quickFilterLabel, QUICK_FILTER_ICONS, QUICK_FILTER_MEANINGS } from "./mobile-quick-filter-options";
import { nativeFreshnessLabel } from "./native-freshness";
import { OrganizationRowView } from "./organization-row";
import { Action, COARSE_POINTER, IconButton, type Colors } from "./page-controls";
import { clearRecent, readSavedEntries, recentRows, RECENT_SHOWN, useRecent, type RecentRecord, type RecentState } from "./recent-sessions";
import { copySessionId, SessionActionsMenu, type SessionAction } from "./session-actions-menu";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { buildOrganizationTree, flattenOrganizationTree, sessionState } from "./session-organization";
import { byLastActivity, totalsDetail, type SessionTotals } from "./session-row-format";
import type { SessionStateCount } from "./session-state-counts";
import { SessionStateMarker } from "./session-state-marker";
import { startSwitchCounts, type SwitchView } from "./session-state-shortcuts";
import { SessionLayoutControl } from "./session-layout-control";
import { sessionStatusInk } from "./session-status-ink";
import { ageInWords, useClock, type Clock } from "./session-time";
import { matchesMembership, matchesQuery, matchesStatus, MORE_STATUS_FILTERS, QUICK_STATUS_FILTERS, type StatusFilter } from "./session-view";
import { useOrganizationSettings } from "./use-organization-settings";
import { useSessionDirectory } from "./use-session-directory";
import { useStartSwitchArchive } from "./start-switch-archive";
import { ARCHIVE_BATCH_LIMIT } from "../shared/session-lifecycle";
import { BulkArchiveDialog, useArchiveSelection } from "./bulk-archive";
import type { LeadSource } from "./new-lead-source";
import { useSessionOperations } from "./session-operations";
import { SelectionBar, SelectionHeader } from "./session-selection";
import { useUsageTotals } from "./use-navigator";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import { sessionRowDetails } from "./session-compact-content";
import { SessionDetailSection } from "./session-detail-section";
import { SessionMetadataGuide, SessionUsageLines } from "./session-usage-view";

/** The Kontor screen param that opens this sheet, from the Command Center item. */
export const SWITCH_SHEET = "switch";
const MATCHES_SHOWN = 20;

export type CreateOptionsState =
  | { readonly status: "offline" }
  | { readonly status: "loading" }
  | { readonly status: "error"; readonly message: string }
  | { readonly status: "ready"; readonly options: CreateOptions };

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

// Only the schema's keys travel: a caller may hand its whole draft for convenience.
const clean = (next?: CreateScope): CreateScope => next ? {
  ...(next.projectId !== undefined ? { projectId: next.projectId } : {}),
  ...(next.workspaceId !== undefined ? { workspaceId: next.workspaceId } : {}),
  ...(next.role !== undefined ? { role: next.role } : {}),
  ...(next.start !== undefined ? { start: next.start } : {}),
  ...(next.issueKey !== undefined ? { issueKey: next.issueKey } : {}),
} : {};

/** The host's create options, read when the sheet opens and again on `refresh`; never while the host is offline. */
export function useCreateOptions(online: boolean, initial: CreateScope = {}): { state: CreateOptionsState; refresh: (scope?: CreateScope) => void } {
  const read = useRpc(readCreateOptions);
  const [state, setState] = useState<CreateOptionsState>({ status: online ? "loading" : "offline" });
  const [revision, setRevision] = useState(0);
  const [scope, setScope] = useState<CreateScope>(clean(initial));
  useEffect(() => {
    if (!online) { setState({ status: "offline" }); return; }
    let live = true;
    setState({ status: "loading" });
    read(Object.keys(scope).length ? scope : {}).then((options) => { if (live) setState({ status: "ready", options }); }, (error: unknown) => { if (live) setState({ status: "error", message: errorText(error) }); });
    return () => { live = false; };
  }, [online, revision, scope]);
  return { state, refresh: (next) => { setScope(clean(next ?? initial)); setRevision((value) => value + 1); } };
}

/** What the sheet says about New lead when it cannot offer it; null when it can. */
export function createAvailability(state: CreateOptionsState): string | null {
  if (state.status === "offline") return "New lead: host offline. Nothing can be created until it reconnects.";
  if (state.status === "loading") return null;
  if (state.status === "error") return `New lead unavailable: Kontor could not read this host's options (${state.message}).`;
  return state.options.enabled ? null : state.options.disabledReason;
}

/** "DESK • ADAM / ADAM", or the working directory when no workspace is named. */
export function sessionPlace(entry: DirectoryEntry): string {
  return [entry.projectName, entry.workspaceName].filter(Boolean).join(" / ") || entry.cwd || "No workspace";
}

export type SwitchRow = { readonly entry: DirectoryEntry; readonly detail: string };

/**
 * Last visited in its own order: the first 8 without a query, the first 20 of its matches with one, every one with
 * `all`. A search only narrows it, like every state, so `total` is the count its shortcut shows and the set Kontor's
 * Sessions opens with.
 */
export function switchRows(entries: readonly DirectoryEntry[], query: string, now: number, record: RecentRecord, all = false): { rows: SwitchRow[]; total: number } {
  const recent = recentRows(entries, record, now).filter((row) => matchesQuery(row.entry, query))
    .map((row) => ({ entry: row.entry, detail: `${row.reason} ${ageInWords(row.at, now)} · ${sessionPlace(row.entry)}` }));
  return { rows: all ? recent : recent.slice(0, query.trim() ? MATCHES_SHOWN : RECENT_SHOWN), total: recent.length };
}

/**
 * The switcher's list for one state: Last visited keeps its own order and search; every other state narrows the
 * non-archived sessions in place, Last activity newest first. `total` counts every match, `rows` the shown ones; the
 * rest are one Show more away, never silently dropped.
 */
export function switchList(entries: readonly DirectoryEntry[], status: StatusFilter, query: string, now: number, record: RecentRecord, favorites: readonly string[], all = false): { rows: DirectoryEntry[]; total: number } {
  if (status === "recent") {
    const { rows, total } = switchRows(entries, query, now, record, all);
    return { rows: rows.map((row) => row.entry), total };
  }
  const matched = entries.filter((entry) => matchesMembership(entry, "sessions") && matchesStatus(entry, status, favorites) && matchesQuery(entry, query));
  const ordered = status === "activity" ? byLastActivity(matched) : matched;
  return { rows: all ? ordered : ordered.slice(0, MATCHES_SHOWN), total: ordered.length };
}

export function persistenceNote(persistence: RecentState["persistence"]): string | null {
  if (persistence === "session") return "Remembered until Paseo restarts on this device.";
  if (persistence === "failed") return "Recent could not be saved on this device; it lasts until Paseo restarts.";
  if (persistence === "unreadable") return "Recent could not be read on this device. Clear recent replaces it.";
  return null;
}

const COVERAGE = "Opened from Kontor · this host and app profile only. Native sidebar opens and other devices are not included.";
const stateHelp = (status: StatusFilter, count: SessionStateCount | undefined) => `${quickFilterLabel(status)} · ${count?.label ?? "count unknown"}. ${QUICK_FILTER_MEANINGS[status]}`;

/** Kontor's state toolbar, with Kontor at the end in place of Filters; More holds every state and shows one chosen there. */
function SwitchStrip({ colors, status, counts, more, onStatus, onMore, onKontor }: {
  colors: Colors; status: StatusFilter; counts: Readonly<Record<StatusFilter, SessionStateCount>>; more: boolean;
  onStatus(status: StatusFilter): void; onMore(): void; onKontor(): void;
}) {
  const extra = MORE_STATUS_FILTERS.includes(status) ? status : null;
  return <StateToolbar testID="kontor-switch-states" stateTestID="kontor-switch-state-" colors={colors} status={status} counts={counts} onStatus={onStatus}
    more={{ testID: "kontor-switch-more", label: extra ? `More: ${quickFilterLabel(extra)}` : "More", help: extra ? stateHelp(extra, counts[extra]) : "Every state, the other states and archived sessions",
      icon: extra ? QUICK_FILTER_ICONS[extra] : "ListFilter", selected: !!extra, expanded: more, count: extra ? counts[extra] : undefined, caption: extra ? undefined : "More", onPress: onMore }}
    trailing={{ testID: "kontor-switch-kontor", label: "Open in Kontor", help: "Open Kontor's Sessions on this host with this state and search; nothing here is discarded.",
      icon: "Building2", caption: "Kontor", onPress: onKontor }} />;
}

/** Every state in the agreed order, so one the toolbar has no room for is a tap away; then the other states and archived membership, which only Kontor lists. */
function MorePanel({ colors, status, counts, onStatus, onArchived }: {
  colors: Colors; status: StatusFilter; counts: Readonly<Record<StatusFilter, SessionStateCount>>; onStatus(status: StatusFilter): void; onArchived(): void;
}) {
  const item = (key: string, label: string, icon: string, help: string, selected: boolean, onPress: () => void, count?: string, iconColor?: string) => <MoreItem key={key} testID={`kontor-switch-more-${key}`}
    label={label} icon={icon} iconColor={iconColor} help={help} selected={selected} count={count} colors={colors} onPress={onPress} />;
  const state = (filter: StatusFilter, iconColor?: string) => item(filter, quickFilterLabel(filter), QUICK_FILTER_ICONS[filter], stateHelp(filter, counts[filter]), status === filter, () => onStatus(filter), counts[filter]?.text, iconColor);
  const group = { flexDirection: "row", flexWrap: "wrap", gap: 4 } as const;
  return <View testID="kontor-switch-more-panel" style={{ gap: 8 }}>
    <View role="group" accessibilityLabel="All states" style={group}>{QUICK_STATUS_FILTERS.map((filter) => state(filter, sessionStatusInk(filter, colors)))}</View>
    <View role="group" accessibilityLabel="More states" style={group}>
      {MORE_STATUS_FILTERS.map((filter) => state(filter))}
      {item("archived", "Archived sessions", "Archive", "Archived sessions on this host, listed in Kontor's Sessions with this search.", false, onArchived)}
    </View>
  </View>;
}

function MoreItem({ label, icon, iconColor, help, selected, count, colors, onPress, testID }: { label: string; icon: string; iconColor?: string; help: string; selected: boolean; count?: string; colors: Colors; onPress(): void; testID: string }) {
  const hint = useActionHelp(help);
  return <Pressable {...hint.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={count ? `${label}, ${count}` : label} aria-pressed={selected} accessibilityState={{ selected }}
    onPress={() => { hint.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ flexBasis: "48%", flexGrow: 1, minHeight: 44, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8,
      borderRadius: 7, borderWidth: selected || focused ? 2 : 1, borderColor: selected || focused ? colors.accent : colors.border, backgroundColor: pressed || selected ? colors.surface2 : colors.surface1 })}>
    <Icon name={icon} size={16} color={iconColor ?? colors.foreground} />
    <Text numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 12 }}>{label}</Text>
    {count ? <Text style={{ color: colors.foregroundMuted, fontSize: 11, fontVariant: ["tabular-nums"] }}>{count}</Text> : null}
  </Pressable>;
}

function DeviceNote({ colors, help }: { colors: Colors; help: string }) {
  const hint = useActionHelp(help);
  return <Pressable {...hint.bindings} testID="kontor-switch-device" accessibilityRole="text" accessibilityLabel={`This device. ${help}`}
    style={{ minHeight: 28, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 10 }}>
    <Icon name="Clock" size={14} color={colors.foregroundMuted} />
    <View style={{ flex: 1 }} />
    <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>This device</Text>
  </Pressable>;
}

type StartSwitchProps = {
  compact: boolean; colors: Colors; hostId: string; hostStatus: PluginHostSummary["status"]; directory: DirectoryState;
  /** Whether this Paseo app exposes native navigation to plugins. */
  canOpen: boolean;
  onOpenCreated: (outcome: CreateOutcome) => OpenSessionResult;
  onOpen: (entry: DirectoryEntry) => void;
  /** Kontor's Sessions with this state, search and membership. */
  onShowView: (view: SwitchView) => void;
  onClose: () => void;
  /** Inside the host's own popover or bottom sheet, which owns dismissal and scrolling; the compact sheet also shows the title. */
  popover?: boolean;
  /** The session this was opened from, marked in the list. */
  currentId?: string | null;
  /** The workspace or project the dialog was opened in; its desk becomes the default. */
  initialScope?: CreateScope;
  /** Opens New lead at once with this source and its saved revision pinned. */
  initialSource?: LeadSource | null;
};

/**
 * One body for switching and starting, drawn by the Kontor screen's sheet and by the composer pill's popover: the
 * state strip, search and layout over this host's sessions, plus New lead as a header action when the host allows it.
 * Rows open only through `onOpen`, so each surface keeps its own single open path and its refusals.
 */
export function useStartSwitch({ compact, colors, hostId, hostStatus, directory, canOpen, onOpenCreated, onOpen, onShowView, onClose, popover = false, currentId = null, initialScope, initialSource = null }: StartSwitchProps) {
  const [mode, setMode] = useState<"switch" | "new">(initialSource ? "new" : "switch");
  const creation = useRef({ hostId, open: !!initialSource });
  const scope = creation.current;
  // The source New lead opens with; each start gets a fresh form, so a pinned source never leaks into the next one.
  const [lead, setLead] = useState<{ key: number; source: LeadSource | null }>({ key: 0, source: initialSource });
  const startNew = (source: LeadSource | null = null) => { creation.current = { hostId, open: true }; setLead((current) => ({ key: current.key + 1, source })); setMode("new"); };
  const leaveNew = () => { creation.current.open = false; setMode("switch"); };
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<StatusFilter>("recent");
  const [more, setMore] = useState(false);
  const [searching, setSearching] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  // Show more reveals the list it was pressed for; a change of state or search starts short again.
  const [revealed, setRevealed] = useState(false);
  // A search, state, layout or disclosure change drops a row's waiting state-icon open first, even if that row stays listed.
  const find = (next: string) => { cancelDelayedOpen(); setQuery(next); setRevealed(false); };
  const recent = useRecent(hostId);
  const saved = readSavedEntries(recent.record);
  const organization = useOrganizationSettings();
  const preferences = organization.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const presentation = organizationPresentation(preferences);
  const toast = useToast();
  const create = useCreateOptions(hostStatus === "online", initialScope);
  const archive = useStartSwitchArchive(hostId, directory, hostStatus === "online");
  const clock = useClock();
  const entries = archive.directory.entries;
  const recentIds = recentRows(entries, recent.record).map((row) => row.entry.id);
  const opened = new Map(recent.record.visits.map((visit) => [visit.id, visit.at]));
  const freshness = nativeFreshnessLabel({ hostStatus, status: directory.status, live: directory.live });
  const counts = startSwitchCounts(archive.directory, freshness, { query, recent: recentIds, favorites: preferences.favorites });
  const { rows, total } = switchList(entries, status, query, clock.now, recent.record, preferences.favorites, revealed);
  // Selection covers every known match, shown or not, and keeps its exact IDs when the state or search changes.
  const matches = switchList(entries, status, query, clock.now, recent.record, preferences.favorites, true).rows.filter((entry) => entry.archivedAt === null).map((entry) => entry.id);
  const selection = useArchiveSelection(hostId, matches);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const ops = useSessionOperations({ hostId, online: hostStatus === "online", colors, compact, entries, opened,
    newLead: (source, revision) => startNew({ entry: source, summary: revision }), onTakeoverStart: () => selection.clear() });
  const totals = useUsageTotals(ops.totals, rows.map((entry) => entry.id));
  const seats = selection.selected.flatMap((id) => entries.filter((entry) => entry.id === id));
  const takeover = ops.takeover(seats);
  const complete = directory.status === "ready" && directory.inventoryComplete === true;
  const allSelected = matches.length > 0 && matches.every((id) => selection.selected.includes(id));
  const selectAllUnavailable = !matches.length ? "No session matches." : !complete ? "The session list is not complete yet; select sessions one by one."
    : new Set([...selection.selected, ...matches]).size > ARCHIVE_BATCH_LIMIT ? `More than ${ARCHIVE_BATCH_LIMIT} sessions: narrow the state or search, or select one by one.` : null;
  const select = (entry: DirectoryEntry) => { if (entry.archivedAt === null && !bulkOpen) selection.toggle(entry.id); };
  const archiveSelected = () => {
    if (archive.disabled || !selection.selected.length) return;
    archive.lifecycle.clear(); setBulkOpen(true); void archive.lifecycle.prepare("archive", "filtered", [...selection.selected], true);
  };
  const chronological = status === "recent" || status === "activity";
  const layout = chronological ? "flat" : presentation.layout;
  const unavailable = createAvailability(create.state);
  const tall = compact || COARSE_POINTER;
  const dismiss = () => { creation.current.open = false; onClose(); };
  const showView = (membership: SwitchView["membership"] = "sessions") => onShowView({ status: membership === "archived" ? "all" : status, query: query.trim(), membership });
  // Provider blocked is answered by exact-ID refusal reads, which only Kontor's Sessions makes.
  const choose = (next: StatusFilter) => { cancelDelayedOpen(); if (next === "provider_blocked") { onShowView({ status: next, query: query.trim(), membership: "sessions" }); return; } setStatus(next); setMore(false); setRevealed(false); };
  const favorite = async (entry: DirectoryEntry) => {
    const has = preferences.favorites.includes(entry.id);
    const ok = await organization.save({ favorites: has ? preferences.favorites.filter((id) => id !== entry.id) : [...preferences.favorites, entry.id] });
    if (!ok) toast.error(`${has ? "Removing" : "Adding"} the favorite was not saved; this host's settings are not writable right now.`);
  };
  const actionsFor = (entry: DirectoryEntry): SessionAction[] => {
    const starred = preferences.favorites.includes(entry.id);
    return [
      ...ops.actions(entry),
      { key: "select", label: "Select sessions", icon: "SquareCheck", unavailable: entry.archivedAt !== null ? "Archived sessions are not selected." : null, perform: () => select(entry) },
      { key: "favorite", label: starred ? "Remove from favorites" : "Add to favorites", icon: "Star", unavailable: organization.state.status === "ready" ? null : "Favorites are not readable yet.", perform: () => favorite(entry) },
      { key: "copy-id", label: "Copy exact session ID", icon: "Copy", perform: () => copySessionId(entry, toast) },
      { key: "details", label: "Details", icon: "Info", opens: true, perform: () => setDetailsId(entry.id) },
      { key: "archive", label: "Archive", icon: "Archive", unavailable: entry.archivedAt !== null ? "Already archived." : archive.disabled || archive.held(entry) ? "Archive is not available for this session right now." : null,
        perform: () => archive.onArchive(entry) },
    ];
  };
  const row = (entry: DirectoryEntry, testID?: string) => <DenseSessionRow key={entry.id} testID={testID} entry={entry} colors={colors} clock={clock} opened={opened.get(entry.id) ?? null}
    current={entry.id === currentId} totals={totals.get(entry.id) ?? null} onSelectStart={entry.archivedAt === null ? () => select(entry) : undefined}
    mode={selection.selecting ? { kind: "select", selected: selection.selected.includes(entry.id), unavailable: entry.archivedAt !== null ? "Archived sessions are not selected." : null }
      : { kind: "open", actions: <SessionActionsMenu entry={entry} colors={colors} actions={actionsFor(entry)} /> }}
    onPress={() => selection.selecting ? select(entry) : onOpen(entry)} />;
  const heading = <View style={{ flex: 1, minWidth: 0 }}>
    {/* The compact host sheet already shows this title, so there the body keeps only its scope; the desktop host popover shows none. */}
    {popover && compact && mode !== "new" ? null : <Text accessibilityRole="header" numberOfLines={1} style={{ color: colors.foreground, fontSize: 15, fontWeight: "600" }}>{mode === "new" ? "New lead" : "Start or switch session"}</Text>}
    {mode === "new" ? null : <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11 }}>This host</Text>}
  </View>;
  const close = popover ? null : <Action small label="Close" accessibilityLabel="Close start or switch" colors={colors} onPress={dismiss} />;
  const pending = create.state.status === "ready" && create.state.options.pending;
  const newLead = create.state.status === "ready" && (pending || create.state.options.enabled)
    ? <Pressable testID="kontor-switch-new-lead" accessibilityRole="button" accessibilityLabel={pending ? "Check pending session creation" : "New lead"} onPress={() => startNew()}
      style={({ pressed }: { pressed: boolean }) => ({ minHeight: tall ? 44 : 34, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 12, borderRadius: 7,
        backgroundColor: colors.accent, opacity: pressed ? 0.85 : 1 })}>
      <Icon name={pending ? "Loader" : "Plus"} size={16} color={colors.accentForeground} />
      <Text style={{ color: colors.accentForeground, fontSize: 13, fontWeight: "600" }}>{pending ? "Check creation" : "New lead"}</Text>
    </Pressable> : null;
  // New lead's heading goes to the form, which sets its model chip at the right (board B1); Back returns to the list.
  const header = mode === "new" ? null
    : <View style={{ gap: 6, paddingBottom: 4 }}>
      {selection.selecting ? <SelectionHeader count={selection.selected.length} total={matches.length} hidden={selection.hidden} colors={colors} onDone={selection.clear}
        selectAll={{ checked: allSelected, unavailable: allSelected ? null : selectAllUnavailable, onPress: () => allSelected ? selection.remove(matches) : selection.selectAll(matches) }} />
        : <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>{heading}{newLead}{close}</View>}
      <SwitchStrip colors={colors} status={status} counts={counts} more={more} onStatus={choose} onMore={() => { cancelDelayedOpen(); setMore((value) => !value); }} onKontor={() => showView()} />
      {more ? <MorePanel colors={colors} status={status} counts={counts} onStatus={choose} onArchived={() => showView("archived")} /> : null}
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 6, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: searching ? 2 : 1, borderColor: searching ? colors.accent : colors.border, borderRadius: 6 }}>
          <Icon name="Search" size={16} color={colors.foregroundMuted} />
          <TextInput testID="kontor-switch-search" accessibilityLabel="Find a session" placeholder="Find a session…" autoFocus={!compact} value={query} onChangeText={find}
            onFocus={() => setSearching(true)} onBlur={() => setSearching(false)} placeholderTextColor={colors.foregroundMuted}
            // The box shows focus in the accent; the browser's own blue ring would sit inside it.
            style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: 13, minHeight: tall ? 40 : 30, ...(Platform.OS === "web" ? { outlineStyle: "solid", outlineWidth: 0 } : {}) }} />
          {query ? <IconButton label="Clear search" icon="X" colors={colors} onPress={() => find("")} /> : null}
        </View>
        <SessionLayoutControl value={layout} colors={colors} onChange={(next) => { cancelDelayedOpen(); void organization.save({ layout: next }); }}
          disabled={chronological ? `${quickFilterLabel(status)} lists sessions newest first, in one list. Layout applies to the other states and stays saved.`
            : organization.state.status === "ready" ? undefined : "Layout is not readable yet."} />
      </View>
    </View>;
  const muted = { color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 10, paddingVertical: 4 } as const;
  let empty: string | null = null;
  if (!rows.length && query.trim()) empty = `No ${status === "recent" ? "" : `${quickFilterLabel(status)} `}session matches “${query.trim()}”.`;
  else if (!rows.length && status !== "recent") empty = `No ${quickFilterLabel(status)} sessions on this host.`;
  else if (!rows.length && recent.record.clearedAt !== null) empty = `Cleared on this device at ${new Date(recent.record.clearedAt).toLocaleTimeString()}. Sessions you open from Kontor after that appear here.`;
  else if (!rows.length) empty = "Sessions you open from Kontor appear here, most recent first.";
  const persistence = persistenceNote(recent.persistence);
  const tree = layout === "grouped"
    ? flattenOrganizationTree(buildOrganizationTree({ entries: rows, favorites: preferences.favorites, siblingOrder: preferences.siblingOrder, displayMappings: preferences.displayMappings, grouping: presentation.grouping }),
      // Every match shows: groups open until collapsed here, and Kontor's saved disclosure is left alone.
      expanded, false, () => true)
    : null;
  const items = <>
    {freshness === "Live" ? null : <Text testID="kontor-switch-freshness" style={muted}>Last known · {freshness}</Text>}
    {canOpen ? null : <Text style={muted}>This Paseo app does not let plugins open sessions; rows are listed only.</Text>}
    {/* The row keeps its menu while one archive runs, so progress is said here until its result replaces it. */}
    {archive.pendingId ? <Text testID="kontor-switch-archiving" role="status" aria-busy accessibilityState={{ busy: true }} accessibilityLiveRegion="polite" style={muted}>
      {`Archiving ${entries.find((entry) => entry.id === archive.pendingId)?.title ?? "the session"}…`}</Text> : null}
    {archive.notice ? <Text testID="kontor-switch-archive-result" accessibilityRole={archive.notice.error ? "alert" : undefined} role={archive.notice.error ? undefined : "status"} accessibilityLiveRegion="polite" style={{ ...muted, color: archive.notice.error ? colors.statusWarning : colors.foreground }}>{archive.notice.message}</Text> : null}
    {archive.notice?.review ? <Action small label="Open Sessions to review archive" colors={colors} onPress={() => onShowView({ status: "all", query: "", membership: "sessions" })} /> : null}
    {status === "recent" ? <DeviceNote colors={colors} help={COVERAGE} /> : null}
    {tree ? tree.map((item) => {
      if (item.kind === "empty") return null;
      const key = item.kind === "group" ? item.group.key : `${item.parentKey}:${item.entry.id}`;
      return <OrganizationRowView key={key} row={item} colors={colors} compact={compact} disabled={false} arranging={false} selected={false} favorite={false}
        onToggle={() => { cancelDelayedOpen(); if (item.kind === "group") setExpanded((current) => ({ ...current, [item.group.key]: !item.expanded })); }} onMove={() => undefined} onSelect={() => undefined}>
        {item.kind === "session" ? row(item.entry, `session-row-${item.entry.id}`) : null}
      </OrganizationRowView>;
    }) : rows.map((entry) => row(entry))}
    {empty ? <Text testID="kontor-switch-empty" style={{ ...muted, fontSize: 12, paddingVertical: 10 }}>{empty}</Text> : null}
    <View style={{ borderTopWidth: 1, borderColor: colors.border, marginTop: 6, paddingTop: 4, gap: 2 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, minHeight: 32 }}>
        <Text testID="kontor-switch-count" style={{ ...muted, flex: 1 }}>{`${rows.length} of ${total}`}</Text>
        {rows.length < total ? <Action small label={`Show ${total - rows.length} more`} accessibilityLabel={`Show the other ${total - rows.length} of ${total}`} colors={colors} onPress={() => { cancelDelayedOpen(); setRevealed(true); }} /> : null}
        {status === "recent" ? <Action small label="Clear recent" accessibilityLabel="Clear recent on this device" colors={colors} onPress={() => clearRecent(hostId)} /> : null}
      </View>
      {saved.ids.length ? <Text style={muted}>{`Saved entries from an earlier version (${saved.ids.length}), visit time and host unknown, are listed under Last visited in Sessions.`}</Text> : null}
      {saved.unreadable ? <Text style={muted}>Saved entries from an earlier version could not be read and are left untouched.</Text> : null}
      {persistence ? <Text style={muted}>{persistence}</Text> : null}
      {unavailable ? <Text testID="kontor-switch-create-status" style={muted}>{unavailable}</Text> : null}
    </View>
  </>;
  const list = { testID: "kontor-switch-list", role: "list", accessibilityLabel: `${quickFilterLabel(status)} sessions${query.trim() ? ` matching ${query.trim()}` : ""}` } as const;
  const body = mode === "new"
    ? <NewLsaForm key={lead.key} initialSource={lead.source} hostId={hostId} colors={colors} compact={compact} hostScrolls={popover} options={create.state} onRefresh={create.refresh} canOpen={canOpen}
      isActive={() => scope === creation.current && scope.open && scope.hostId === hostId}
      onOpenCreated={onOpenCreated} onCancel={leaveNew} sources={entries.filter((entry) => matchesMembership(entry, "sessions"))} opened={opened}
      header={<><IconButton label="Back to sessions" icon="ChevronLeft" colors={colors} onPress={leaveNew} />{heading}</>} />
    // The host's popover already scrolls; a second vertical scroller inside it would fight it on touch.
    : popover ? <View {...list}>{items}</View>
    : <ScrollView {...list} keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>{items}</ScrollView>;
  // Board D1's sticky bar under the list, and the sheets the row menu and the bar open.
  // An unfinished takeover stays reachable here even after its results closed and its archived predecessor left the list.
  const footer = mode === "switch" && selection.selecting ? <SelectionBar count={seats.length} colors={colors} disclosure={takeover.disclosure}
    takeover={{ unavailable: takeover.unavailable, onPress: takeover.start }} auto={{ unavailable: takeover.auto, onPress: takeover.choose }}
    archive={{ unavailable: archive.disabled ? "Archive waits for the host and any running archive." : null, onPress: archiveSelected }} /> : mode === "switch" ? ops.notice : null;
  const detailsEntry = detailsId === null ? null : entries.find((entry) => entry.id === detailsId) ?? null;
  const layers = <>
    {ops.layer}
    {detailsEntry ? <SwitchDetails entry={detailsEntry} colors={colors} clock={clock} opened={opened.get(detailsEntry.id) ?? null} totals={totals.get(detailsEntry.id) ?? null}
      stale={freshness !== "Live"} onClose={() => setDetailsId(null)} /> : null}
    {bulkOpen ? <BulkArchiveDialog lifecycle={archive.lifecycle} colors={colors} compact={compact} onFinished={(summary) => toast.show(summary)} onClose={(archived) => {
      // Archived sessions leave the selection; a blocked one stays selected for another review.
      selection.remove(archived); setBulkOpen(false);
    }} /> : null}
  </>;
  return { label: mode === "new" ? "New lead" : selection.selecting ? "Select sessions" : "Start or switch session", header, body: <>{body}{layers}</>, footer, dismiss };
}

/** Board C1's Details for a switcher row: its full values, read-only. Kontor's own Details adds placement, plan and retirement. */
function SwitchDetails({ entry, colors, clock, opened, totals, stale, onClose }: {
  entry: DirectoryEntry; colors: Colors; clock: Clock; opened: number | null; totals: SessionTotals | null; stale: boolean; onClose: () => void;
}) {
  return <OperationSheet testID="kontor-switch-details" title="Details" subtitle={entry.title} colors={colors} onClose={onClose}
    footer={<SheetFooter colors={colors} secondary={{ testID: "kontor-switch-details-close", label: "Close", accessibilityLabel: `Close details: ${entry.title}`, onPress: onClose }} />}>
    <SessionDetailSection title="Session" colors={colors} values={[{ label: "Title", value: entry.title }, { label: "Session ID", value: entry.id }, ...rowTimeLines(entry, opened, clock)]} />
    <SessionUsageLines entry={entry} colors={colors} stale={stale} />
    <SessionDetailSection title="Session totals" values={[totalsDetail(totals)]} colors={colors} />
    <SessionDetailSection title="Native status and availability" values={sessionRowDetails(entry)} colors={colors} />
    <SessionMetadataGuide entry={entry} colors={colors} />
  </OperationSheet>;
}

/** The Kontor screen's sheet around the shared body; closing it returns focus to its trigger. */
export function StartSwitchSheet({ anchor, root, rootSize, onClose, ...props }: Omit<StartSwitchProps, "popover"> & { anchor: RefObject<View | null>; root: RefObject<View | null>; rootSize: MenuSize }) {
  const { label, header, body, footer, dismiss } = useStartSwitch({ ...props, onClose: () => { onClose(); setTimeout(() => focusControl(anchor.current), 0); } });
  return <AnchoredMenu testID="kontor-switch-sheet" dialog anchor={anchor} root={root} rootSize={rootSize} colors={props.colors} label={label} onClose={dismiss}
    placement={props.compact ? { align: "stretch", width: 0, cover: true } : { align: "end", width: 420, maxHeight: 640 }} header={header}>
    {body}
    {footer}
  </AnchoredMenu>;
}

/**
 * The composer pill's popover: the same body inside the host's popover or bottom sheet. A plugin popover gets no
 * session navigation, so each open is handed once to the Kontor screen, which opens it by exact ID (`openHandOff`).
 * Whether this app lets plugins open sessions is the screen's to say, so rows stay openable here.
 */
export function StartSwitchPopover(props: PluginButtonContentProps & { openScreen: (input: PluginOpenScreenInput) => void; screenId: string }) {
  const { host, theme, layout, workspaceId, close, openScreen, screenId } = props;
  const hostStatus = useHosts().find((candidate) => candidate.serverId === host.id)?.status ?? "offline";
  // Reads the shared directory without setting its Archived choice; the switcher leaves archived sessions out.
  const directory = useSessionDirectory({ hostId: host.id, hostStatus });
  const toast = useToast();
  const viewOpened = useRef(false);
  /** Null once Kontor received the request and the popover closed; otherwise why it could not open. */
  const go = (key: string): string | null => {
    try { openScreen({ screenId, params: { target: key } }); }
    catch (error) { withdrawHandOff(key); return errorText(error) || "Paseo could not open Kontor."; }
    close();
    return null;
  };
  const handOff = (target: HandOffTarget) => {
    if (target.kind === "view" && viewOpened.current) return null;
    if (target.kind === "view") viewOpened.current = true;
    const failure = go(handOffOpen(host.id, target));
    if (failure && target.kind === "view") viewOpened.current = false;
    return failure;
  };
  const report = (failure: string | null) => { if (failure) toast.error(failure); };
  const { header, body, footer } = useStartSwitch({ compact: layout.compact, colors: theme.colors, hostId: host.id, hostStatus, directory: directory.state, canOpen: true,
    onOpen: (entry) => report(handOff({ kind: "session", agentId: entry.id })),
    onShowView: (view) => report(handOff({ kind: "view", ...view })),
    onOpenCreated: (outcome) => {
      const failure = handOff({ kind: "created", outcome });
      return failure ? { ok: false, reason: "navigation", message: failure } : { ok: true, handoff: true };
    },
    onClose: close, initialScope: { workspaceId }, popover: true, currentId: props.context === "agent" ? props.agentId : null });
  return <ControlHelpProvider colors={theme.colors}><View testID="kontor-switch-popover" style={{ gap: 4 }}>{header}{body}{footer}</View></ControlHelpProvider>;
}

/** Open and the shared archive X have independent hit areas; the action is never nested in Open. */
export function SwitchRowItem({ row, colors, tall, onPress, role = "menuitem", action }: { row: SwitchRow; colors: Colors; tall: boolean; onPress: () => void; role?: "menuitem" | "button"; action?: ReactNode }) {
  return <View style={{ flexDirection: "row", alignItems: "center" }}><Pressable testID={`kontor-switch-row-${row.entry.id}`} role={role} accessibilityLabel={`${row.entry.title}, ${row.detail}`} onPress={onPress}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ minHeight: tall ? 44 : 36, flexDirection: "row", alignItems: "flex-start", gap: 8, paddingHorizontal: 10, paddingVertical: 6, borderRadius: 5,
      flex: 1, minWidth: 0, backgroundColor: pressed || hovered || focused ? colors.surface2 : undefined })}>
    <SessionStateMarker state={sessionState(row.entry)} colors={colors} />
    <View style={{ flex: 1, minWidth: 0 }}>
      <Text numberOfLines={2} style={{ color: colors.foreground, fontSize: 13 }}>{row.entry.title}</Text>
      <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, marginTop: 2 }}>{row.detail}</Text>
    </View>
  </Pressable>{action}</View>;
}
