import { useEffect, useRef, useState, type RefObject } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { TextInput, useToast } from "@getpaseo/plugin/client/react-native";
import { useHosts, useRpc, type PluginButtonContentProps, type PluginHostSummary, type PluginOpenScreenInput } from "@getpaseo/plugin/client";
import { readCreateOptions, type CreateOptions, type CreateOutcome, type CreateScope } from "../shared/session-create";
import { handOffOpen, withdrawHandOff, type HandOffTarget, type OpenSessionResult } from "./session-actions";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { ControlHelpProvider } from "./control-help";
import { NewLsaForm } from "./new-lsa-form";
import { focusControl } from "./mobile-filters";
import { nativeFreshnessLabel } from "./native-freshness";
import { Action, COARSE_POINTER, IconButton, type Colors } from "./page-controls";
import { clearRecent, readRecent, readSavedHistory, recentRows, RECENT_SHOWN, type RecentRecord } from "./recent-sessions";
import { isArchivedSession, type DirectoryEntry, type DirectoryState } from "./session-directory";
import { sessionState } from "./session-organization";
import { SessionStateMarker } from "./session-state-marker";
import { ageInWords } from "./session-time";
import { useSessionDirectory } from "./use-session-directory";

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

/** What the sheet says about New session when it cannot offer it; null when it can. */
export function createAvailability(state: CreateOptionsState): string | null {
  if (state.status === "offline") return "New session: host offline. Nothing can be created until it reconnects.";
  if (state.status === "loading") return null;
  if (state.status === "error") return `New session unavailable: Kontor could not read this host's options (${state.message}).`;
  return state.options.enabled ? null : state.options.disabledReason;
}

/** "DESK • ADAM / ADAM", or the working directory when no workspace is named. */
export function sessionPlace(entry: DirectoryEntry): string {
  return [entry.projectName, entry.workspaceName].filter(Boolean).join(" / ") || entry.cwd || "No workspace";
}

export type SwitchRow = { readonly entry: DirectoryEntry; readonly detail: string };

/**
 * An empty query lists up to 8 Recent rows. A query matches every token across the whole directory: Recent matches
 * first, then the others in directory order, at most 20; `total` counts every match.
 */
export function switchRows(entries: readonly DirectoryEntry[], query: string, now: number, record: RecentRecord): { rows: SwitchRow[]; total: number } {
  const recent = recentRows(entries, record, now).map((row) => ({ entry: row.entry, detail: `${row.reason} ${ageInWords(row.at, now)} · ${sessionPlace(row.entry)}` }));
  const tokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  if (!tokens.length) return { rows: recent.slice(0, RECENT_SHOWN), total: Math.min(recent.length, RECENT_SHOWN) };
  const matches = (entry: DirectoryEntry) => tokens.every((token) => entry.searchText.includes(token));
  const first = recent.filter((row) => matches(row.entry)), ids = new Set(first.map((row) => row.entry.id));
  const rest = entries.filter((entry) => !ids.has(entry.id) && !isArchivedSession(entry) && matches(entry)).map((entry) => ({ entry, detail: sessionPlace(entry) }));
  return { rows: [...first, ...rest].slice(0, MATCHES_SHOWN), total: first.length + rest.length };
}

function persistenceNote(persistence: ReturnType<typeof readRecent>["persistence"]): string | null {
  if (persistence === "session") return "Remembered until Paseo restarts on this device.";
  if (persistence === "failed") return "Recent could not be saved on this device; it lasts until Paseo restarts.";
  if (persistence === "unreadable") return "Recent could not be read on this device. Clear recent replaces it.";
  return null;
}

type StartSwitchProps = {
  compact: boolean; colors: Colors; hostId: string; hostStatus: PluginHostSummary["status"]; directory: DirectoryState;
  /** Whether this Paseo app exposes native navigation to plugins. */
  canOpen: boolean;
  onOpenCreated: (outcome: CreateOutcome) => OpenSessionResult;
  onOpen: (entry: DirectoryEntry) => void; onShowAll: (query: string) => void; onClose: () => void;
  /** Inside the host's own popover or bottom sheet, which owns the title, dismissal and scrolling; Open Kontor joins the actions. */
  popover?: { readonly openKontor: () => void };
  /** The workspace or project the dialog was opened in; its desk becomes the default. */
  initialScope?: CreateScope;
};

/**
 * One body for switching and starting, drawn by the Kontor screen's sheet and by the composer pill's popover: Recent
 * and a search over every session, plus New session as a header action when the host allows it. Rows open only through
 * `onOpen`, so each surface keeps its own single open path and its refusals.
 */
export function useStartSwitch({ compact, colors, hostId, hostStatus, directory, canOpen, onOpenCreated, onOpen, onShowAll, onClose, popover, initialScope }: StartSwitchProps) {
  const [mode, setMode] = useState<"switch" | "new">("switch");
  const creation = useRef({ hostId, open: false });
  const scope = creation.current;
  const startNew = () => { creation.current = { hostId, open: true }; setMode("new"); };
  const leaveNew = () => { creation.current.open = false; setMode("switch"); };
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState(() => readRecent(hostId));
  const [showSaved, setShowSaved] = useState(false);
  const saved = readSavedHistory();
  const create = useCreateOptions(hostStatus === "online", initialScope);
  const now = Date.now();
  const { rows, total } = switchRows(directory.entries, query, now, recent.record);
  const freshness = nativeFreshnessLabel({ hostStatus, status: directory.status, live: directory.live });
  const unavailable = createAvailability(create.state);
  const tall = compact || COARSE_POINTER;
  const dismiss = () => { creation.current.open = false; onClose(); };
  const heading = (text: string) => <Text accessibilityRole="header" numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{text}</Text>;
  const close = popover ? null : <Action small label="Close" accessibilityLabel="Close start or switch" colors={colors} onPress={dismiss} />;
  const header = mode === "new"
    ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingBottom: 4 }}>
      <IconButton label="Back to recent sessions" icon="ChevronLeft" colors={colors} onPress={leaveNew} />
      {heading("New session")}{close}
    </View>
    : <View style={{ gap: 4, paddingBottom: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        {popover ? <View style={{ flex: 1, flexDirection: "row" }}><Action small icon="Building2" label="Open Kontor" colors={colors} onPress={popover.openKontor} /></View> : heading("Start or switch")}
        {create.state.status === "ready" && create.state.options.pending
          ? <Action small label="Check creation" accessibilityLabel="Check pending session creation" colors={colors} onPress={startNew} />
          : create.state.status === "ready" && create.state.options.enabled ? <Action small icon="Plus" label="New session" accessibilityLabel="New session" colors={colors} onPress={startNew} /> : null}
        {close}
      </View>
      <TextInput testID="kontor-switch-search" accessibilityLabel="Jump to any session" placeholder="Jump to any session…" autoFocus={!compact} value={query} onChangeText={setQuery}
        placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 12, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
    </View>;
  const muted = { color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 10, paddingVertical: 4 } as const;
  let empty: string | null = null;
  if (!rows.length && query.trim()) empty = `No session matches “${query.trim()}”.`;
  else if (!rows.length && recent.record.clearedAt !== null) empty = `Cleared on this device at ${new Date(recent.record.clearedAt).toLocaleTimeString()}. Sessions you open from Kontor after that appear here.`;
  else if (!rows.length) empty = "Sessions you open from Kontor appear here, most recent first.";
  const persistence = persistenceNote(recent.persistence);
  const items = <>
    {freshness === "Live" ? null : <Text testID="kontor-switch-freshness" style={muted}>Last known · {freshness}</Text>}
    {canOpen ? null : <Text style={muted}>This Paseo app does not let plugins open sessions; rows are listed only.</Text>}
    {rows.map((row) => <SwitchRowItem key={row.entry.id} row={row} colors={colors} tall={tall} onPress={() => onOpen(row.entry)} />)}
    {empty ? <Text testID="kontor-switch-empty" style={{ ...muted, fontSize: 12, paddingVertical: 10 }}>{empty}</Text> : null}
    {query.trim() && total > rows.length ? <Action small label={`Show all ${total} in Sessions`} colors={colors} onPress={() => onShowAll(query.trim())} /> : null}
    <View style={{ borderTopWidth: 1, borderColor: colors.border, marginTop: 6, paddingTop: 4, gap: 2 }}>
      <Action small label="Clear recent on this device" colors={colors} onPress={() => setRecent(clearRecent(hostId))} />
      <Text style={muted}>Opened from Kontor · this host and app profile only</Text>
      <Text style={muted}>Native sidebar opens and other devices are not included.</Text>
      {saved.ids.length ? <>
        <Action small label={`${showSaved ? "Hide" : "Show"} saved history (${saved.ids.length})`} colors={colors} onPress={() => setShowSaved(!showSaved)} />
        {showSaved ? <View testID="kontor-saved-history">
          <Text style={muted}>Saved history · original host and opening times unknown. Session IDs only.</Text>
          {saved.ids.map((id) => <Text key={id} selectable style={muted}>{id}</Text>)}
        </View> : null}
      </> : null}
      {saved.unreadable ? <Text style={muted}>Earlier saved history could not be read and has been left untouched.</Text> : null}
      {persistence ? <Text style={muted}>{persistence}</Text> : null}
      {unavailable ? <Text testID="kontor-switch-create-status" style={muted}>{unavailable}</Text> : null}
    </View>
  </>;
  const list = { testID: "kontor-switch-list", role: "menu", accessibilityLabel: query.trim() ? "Matching sessions" : "Recent sessions" } as const;
  const body = mode === "new"
    ? <NewLsaForm hostId={hostId} colors={colors} compact={compact} options={create.state} onRefresh={create.refresh} canOpen={canOpen}
      isActive={() => scope === creation.current && scope.open && scope.hostId === hostId}
      onOpenCreated={onOpenCreated} onCancel={leaveNew} />
    // The host's popover already scrolls; a second vertical scroller inside it would fight it on touch.
    : popover ? <View {...list}>{items}</View>
    : <ScrollView {...list} keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>{items}</ScrollView>;
  return { label: mode === "new" ? "New session" : "Start or switch session", header, body, dismiss };
}

/** The Kontor screen's sheet around the shared body; closing it returns focus to its trigger. */
export function StartSwitchSheet({ anchor, root, rootSize, onClose, ...props }: Omit<StartSwitchProps, "popover"> & { anchor: RefObject<View | null>; root: RefObject<View | null>; rootSize: MenuSize }) {
  const { label, header, body, dismiss } = useStartSwitch({ ...props, onClose: () => { onClose(); setTimeout(() => focusControl(anchor.current), 0); } });
  return <AnchoredMenu testID="kontor-switch-sheet" dialog anchor={anchor} root={root} rootSize={rootSize} colors={props.colors} label={label} onClose={dismiss}
    placement={props.compact ? { align: "stretch", width: 0, cover: true } : { align: "end", width: 400, maxHeight: 560 }} header={header}>
    {body}
  </AnchoredMenu>;
}

/**
 * The composer pill's popover: the same body inside the host's popover or bottom sheet. A plugin popover gets no
 * session navigation, so each open is handed once to the Kontor screen, which opens it by exact ID (`openHandOff`).
 * Whether this app lets plugins open sessions is the screen's to say, so rows stay openable here.
 */
export function StartSwitchPopover({ host, theme, layout, workspaceId, close, openScreen, screenId }: PluginButtonContentProps & { openScreen: (input: PluginOpenScreenInput) => void; screenId: string }) {
  const hostStatus = useHosts().find((candidate) => candidate.serverId === host.id)?.status ?? "offline";
  // Reads the shared directory without setting its Archived choice; Recent and search leave archived sessions out.
  const directory = useSessionDirectory({ hostId: host.id, hostStatus });
  const toast = useToast();
  /** Null once Kontor received the request and the popover closed; otherwise why it could not open. */
  const go = (key?: string): string | null => {
    try { openScreen(key ? { screenId, params: { target: key } } : { screenId }); }
    catch (error) { if (key) withdrawHandOff(key); return errorText(error) || "Paseo could not open Kontor."; }
    close();
    return null;
  };
  const handOff = (target: HandOffTarget) => go(handOffOpen(host.id, target));
  const report = (failure: string | null) => { if (failure) toast.error(failure); };
  const { header, body } = useStartSwitch({ compact: layout.compact, colors: theme.colors, hostId: host.id, hostStatus, directory: directory.state, canOpen: true,
    onOpen: (entry) => report(handOff({ kind: "session", agentId: entry.id })),
    onShowAll: (query) => report(handOff({ kind: "search", query })),
    onOpenCreated: (outcome) => {
      const failure = handOff({ kind: "created", outcome });
      return failure ? { ok: false, reason: "navigation", message: failure } : { ok: true, handoff: true };
    },
    onClose: close, initialScope: { workspaceId }, popover: { openKontor: () => report(go()) } });
  return <ControlHelpProvider colors={theme.colors}><View testID="kontor-switch-popover" style={{ gap: 4 }}>{header}{body}</View></ControlHelpProvider>;
}

/** No X glyph here: in Sessions an X archives, and a look-alike must never sit on a row. */
function SwitchRowItem({ row, colors, tall, onPress }: { row: SwitchRow; colors: Colors; tall: boolean; onPress: () => void }) {
  return <Pressable testID={`kontor-switch-row-${row.entry.id}`} role="menuitem" accessibilityLabel={`${row.entry.title}, ${row.detail}`} onPress={onPress}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ minHeight: tall ? 44 : 36, flexDirection: "row", alignItems: "flex-start", gap: 8, paddingHorizontal: 10, paddingVertical: 6, borderRadius: 5,
      backgroundColor: pressed || hovered || focused ? colors.surface2 : undefined })}>
    <SessionStateMarker state={sessionState(row.entry)} colors={colors} />
    <View style={{ flex: 1, minWidth: 0 }}>
      <Text numberOfLines={2} style={{ color: colors.foreground, fontSize: 13 }}>{row.entry.title}</Text>
      <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, marginTop: 2 }}>{row.detail}</Text>
    </View>
  </Pressable>;
}
