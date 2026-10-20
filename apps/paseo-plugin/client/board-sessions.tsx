import type { RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { ScrollView } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { workspaceLabel } from "./session-organization";
import { CHAIN_STATE_GLYPHS, chainLabel, chainState, chainSummary, roleLanes, teamRuns, type ChainState, type RoleLane, type TaskLink } from "./session-results";
import type { SessionTaskFilter } from "./session-view-store";
import { stateColor } from "./organization-row";
import { AnchoredMenu, MenuItem, type MenuSize } from "./anchored-menu";
import { COARSE_POINTER, type Colors } from "./page-controls";

/** Board's read of native sessions and the navigation it may ask for; without it Board shows no session links. */
export type BoardSessions = {
  readonly state: DirectoryState;
  readonly onOpen: (entry: DirectoryEntry) => void;
  /** Absent on hosts without workspace navigation; Open workspace is then not offered. */
  readonly onOpenWorkspace?: (workspaceId: string) => void;
  readonly onShowInSessions: (task: SessionTaskFilter) => void;
};

/** Current only while the host's session feed is attached; otherwise the states are the last read. */
export function sessionsLive(state: DirectoryState): boolean {
  return state.status === "ready" && state.live;
}

function chainColor(state: ChainState, colors: Colors): string {
  return state === "archived" ? colors.foregroundMuted : stateColor(state, colors);
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

/** Spoken summary of a task's linked sessions; roles come from labels and imply no handoff order. */
export function chainDescription(links: readonly TaskLink[], live: boolean): string {
  const lanes = roleLanes(links);
  const earlier = lanes.reduce((count, lane) => count + lane.earlier.length, 0);
  const runs = teamRuns(links);
  return [
    `${plural(links.length, "linked session")}${live ? "" : ", last known states"}`,
    chainSummary(lanes) || null,
    earlier ? `${earlier} replaced or archived` : null,
    runs.length ? `Kontor team run label ${runs.join(", ")}` : null,
    "Roles come from session labels; no handoff order is declared",
  ].filter((part): part is string => !!part).join(". ") + ".";
}

/** Role codes with one shape per session, side by side: parallel sessions stay separate and nothing reads as a sequence. */
function ChainText({ lanes, colors, words, live }: { lanes: readonly RoleLane[]; colors: Colors; words: boolean; live: boolean }) {
  const shown = lanes.filter((lane) => lane.current.length > 0);
  const earlier = lanes.reduce((count, lane) => count + lane.earlier.length, 0);
  return <>
    {live ? null : <Text style={{ color: colors.foregroundMuted }}>{"Last known  "}</Text>}
    {shown.map((lane, index) => <Text key={lane.role ?? "unlabelled"}>
      {index ? <Text style={{ color: colors.foregroundMuted }}>{"  ·  "}</Text> : null}
      <Text style={{ color: colors.foreground, fontWeight: "600" }}>{lane.role ?? "No role label"}</Text>
      {lane.current.map((entry) => {
        const state = chainState(entry);
        return <Text key={entry.id} style={{ color: chainColor(state, colors) }}>{` ${CHAIN_STATE_GLYPHS[state]}${words ? ` ${chainLabel(entry)}` : ""}`}</Text>;
      })}
    </Text>)}
    {earlier ? <Text style={{ color: colors.foregroundMuted }}>{`${shown.length ? "  ·  " : ""}${earlier} earlier`}</Text> : null}
  </>;
}

/**
 * The linked sessions of one Board task, below the row and beside its other
 * controls rather than inside them. One session opens directly; several open
 * the chooser.
 */
export function TaskSessionsControl({ testID, links, live, compact, colors, onPress, buttonRef }: {
  testID: string; links: readonly TaskLink[]; live: boolean; compact: boolean; colors: Colors;
  onPress: () => void; buttonRef: (node: View | null) => void;
}) {
  const single = links.length === 1 ? links[0]!.entry : null;
  return <Pressable testID={testID} ref={buttonRef} accessibilityRole="button"
    accessibilityLabel={single ? `Open linked session ${single.title}. ${chainDescription(links, live)}` : `Choose a linked session. ${chainDescription(links, live)}`}
    onPress={onPress}
    style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({ minHeight: COARSE_POINTER ? 44 : 24, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 6, borderRadius: 6,
      backgroundColor: pressed || hovered ? colors.surface2 : undefined })}>
    <Text numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>
      <ChainText lanes={roleLanes(links)} colors={colors} words={!compact} live={live} />
    </Text>
    <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>{single ? "Open session ›" : `${plural(links.length, "session")} ›`}</Text>
  </Pressable>;
}

/** Linked sessions by workspace, then replaced or archived ones; the read and its limits come first. */
export function TaskSessionsChooser({ taskKey, task, links, live, archivedIncluded, anchor, root, rootSize, compact, colors, sessions, onClose }: {
  taskKey: string; task: SessionTaskFilter; links: readonly TaskLink[]; live: boolean; archivedIncluded: boolean;
  anchor: RefObject<View | null>; root: RefObject<View | null>; rootSize: MenuSize; compact: boolean; colors: Colors;
  sessions: BoardSessions; onClose: () => void;
}) {
  const lanes = roleLanes(links);
  const via = new Map(links.map((link) => [link.entry.id, link.via]));
  const current = lanes.flatMap((lane) => lane.current.map((entry) => ({ entry, role: lane.role })));
  const earlier = lanes.flatMap((lane) => lane.earlier.map((entry) => ({ entry, role: lane.role })));
  const workspaces = new Map<string, { id: string | null; label: string; rows: typeof current }>();
  for (const row of current) {
    const label = workspaceLabel(row.entry);
    const key = row.entry.workspaceId ?? `label:${label}`;
    const group = workspaces.get(key) ?? { id: row.entry.workspaceId, label, rows: [] };
    group.rows.push(row);
    workspaces.set(key, group);
  }
  const runs = teamRuns(links);
  const choose = (action: () => void) => { onClose(); action(); };
  const item = ({ entry, role }: (typeof current)[number]) => {
    const state = chainLabel(entry);
    // The title as the session names itself; the role label beside it may repeat a title prefix, so it goes with the state.
    return <MenuItem key={entry.id} indent={1} tall={compact} colors={colors} label={entry.title} detail={`${role ?? "No role label"} · ${state}`}
      accessibilityLabel={`Open session ${entry.title}. Role label ${role ?? "none"}. ${state}${live ? "" : ", last known"}. Linked by its ${via.get(entry.id)} label.`}
      onPress={() => choose(() => sessions.onOpen(entry))} />;
  };
  const section = (label: string) => <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 10, paddingTop: 8, paddingBottom: 2 }}>{label}</Text>;
  const keys = [...new Set(links.map((link) => link.via))].join(" or ");
  return <AnchoredMenu testID="board-sessions-menu" dialog anchor={anchor} root={root} rootSize={rootSize} colors={colors} label={`Sessions linked to ${taskKey}`} onClose={onClose}
    placement={compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.7, 480) } : { align: "end", width: 400, maxHeight: 480 }}>
    <ScrollView keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
      <View style={{ paddingHorizontal: 10, paddingTop: 6, paddingBottom: 2, gap: 2 }}>
        <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{`${taskKey} · ${plural(links.length, "linked session")}`}</Text>
        <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 15 }}>
          {`Joined by an exact ${keys} label. Team, workflow and risk tier: not declared by any configured source.${runs.length ? ` Kontor team run label: ${runs.join(", ")}.` : ""}${live ? "" : " Last known states; the host's live session feed is not attached."}${archivedIncluded ? "" : " Archived sessions are not read."}`}
        </Text>
      </View>
      {[...workspaces.values()].map((group) => <View key={group.id ?? group.label}>
        {section(group.label)}
        {group.id && sessions.onOpenWorkspace ? <MenuItem indent={1} tall={compact} colors={colors} label="Open workspace" accessibilityLabel={`Open workspace ${group.label}`} onPress={() => choose(() => sessions.onOpenWorkspace?.(group.id!))} /> : null}
        {group.rows.map(item)}
      </View>)}
      {earlier.length ? <View>{section("Replaced or archived")}{earlier.map(item)}</View> : null}
      <View style={{ height: 1, backgroundColor: colors.border, marginVertical: 4 }} />
      <MenuItem tall={compact} colors={colors} label="Show in Sessions" accessibilityLabel={`Show the sessions linked to ${taskKey} in Sessions`} onPress={() => choose(() => sessions.onShowInSessions(task))} />
    </ScrollView>
  </AnchoredMenu>;
}
