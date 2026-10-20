import type { RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { ScrollView } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { workspaceLabel } from "./session-organization";
import { CHAIN_STATE_GLYPHS, chainLabel, chainState, chainSummary, roleLanes, teamRuns, type ChainState, type RoleLane, type TaskLink } from "./session-results";
import { taskTeamView, teamHeadline, type TaskTeamDeclaration, type TaskTeamRead, type TeamRoleView } from "./task-teams";
import type { SessionTaskFilter } from "./session-view-store";
import { stateColor } from "./organization-row";
import { AnchoredMenu, MenuItem, type MenuSize } from "./anchored-menu";
import { COARSE_POINTER, Copy, Disclosure, type Colors } from "./page-controls";

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

/** The one session a task's control opens directly; a declared team, or several sessions, open the chooser instead. */
export function directSession(links: readonly TaskLink[], team: TaskTeamDeclaration | null): DirectoryEntry | null {
  return !team && links.length === 1 ? links[0]!.entry : null;
}

function chainColor(state: ChainState, colors: Colors): string {
  return state === "archived" ? colors.foregroundMuted : stateColor(state, colors);
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

const NO_SESSION_GLYPH = "–";

function roleStates(role: TeamRoleView): string {
  return role.current.length ? role.current.map(chainLabel).join(" and ") : "No session";
}

/** Spoken team: its steps in order, parallel roles together, then the labelled roles it does not name. */
function teamSummary(declaration: TaskTeamDeclaration, links: readonly TaskLink[]): string {
  const view = taskTeamView(declaration, links);
  const step = (roles: readonly TeamRoleView[]) => roles.length > 1
    ? `${roles.map((role) => role.role).join(" and ")} in parallel: ${roles.map((role) => `${role.role}: ${roleStates(role)}`).join("; ")}`
    : `${roles[0]!.role}: ${roleStates(roles[0]!)}`;
  return [
    `${teamHeadline(declaration)}. Team ${declaration.team ?? "not declared"}`,
    view.steps.length ? (view.ordered ? view.steps.map(step).join(", then ") : `No order declared. ${view.steps[0]!.map((role) => `${role.role}: ${roleStates(role)}`).join("; ")}`) : null,
    chainSummary(view.outside) ? `Not in the declared team: ${chainSummary(view.outside)}` : null,
  ].filter((part): part is string => !!part).join(". ");
}

/** Spoken summary of a task's linked sessions; roles come from labels and imply no handoff order unless a team declares one. */
export function chainDescription(links: readonly TaskLink[], live: boolean, team: TaskTeamDeclaration | null = null): string {
  const lanes = roleLanes(links);
  const earlier = lanes.reduce((count, lane) => count + lane.earlier.length, 0);
  const runs = teamRuns(links);
  return [
    `${links.length ? plural(links.length, "linked session") : "No linked session"}${live || !links.length ? "" : ", last known states"}`,
    team ? teamSummary(team, links) : chainSummary(lanes) || null,
    earlier ? `${earlier} replaced or archived` : null,
    runs.length ? `Kontor team run label ${runs.join(", ")}` : null,
    team ? "Sessions sit under a role by their role label" : "Roles come from session labels; no handoff order is declared",
  ].filter((part): part is string => !!part).join(". ") + ".";
}

function Glyphs({ entries, colors, words }: { entries: readonly DirectoryEntry[]; colors: Colors; words: boolean }) {
  return <>{entries.map((entry) => {
    const state = chainState(entry);
    return <Text key={entry.id} style={{ color: chainColor(state, colors) }}>{` ${CHAIN_STATE_GLYPHS[state]}${words ? ` ${chainLabel(entry)}` : ""}`}</Text>;
  })}</>;
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
      <Glyphs entries={lane.current} colors={colors} words={words} />
    </Text>)}
    {earlier ? <Text style={{ color: colors.foregroundMuted }}>{`${shown.length ? "  ·  " : ""}${earlier} earlier`}</Text> : null}
  </>;
}

/**
 * A declared team's roles in its order: arrows between steps, `&` between roles
 * that work in parallel, a dash for a role no linked session is labelled with,
 * then the labelled roles the team does not name.
 */
function TeamChainText({ declaration, links, colors, words, live }: { declaration: TaskTeamDeclaration; links: readonly TaskLink[]; colors: Colors; words: boolean; live: boolean }) {
  const view = taskTeamView(declaration, links);
  const earlier = view.steps.flat().reduce((count, role) => count + role.earlier, 0) + view.outside.reduce((count, lane) => count + lane.earlier.length, 0);
  const outside = view.outside.filter((lane) => lane.current.length > 0);
  const muted = (value: string) => <Text style={{ color: colors.foregroundMuted }}>{value}</Text>;
  return <>
    {live || !links.length ? null : muted("Last known  ")}
    {view.steps.map((step, index) => <Text key={step.map((role) => role.role).join()}>
      {index ? muted(view.ordered ? "  →  " : "  ·  ") : null}
      {step.map((role, position) => <Text key={role.role}>
        {position ? muted(view.ordered ? " & " : "  ·  ") : null}
        <Text style={{ color: colors.foreground, fontWeight: "600" }}>{role.role}</Text>
        {role.current.length ? <Glyphs entries={role.current} colors={colors} words={words} /> : muted(` ${NO_SESSION_GLYPH}${words ? " No session" : ""}`)}
      </Text>)}
    </Text>)}
    {outside.map((lane) => <Text key={lane.role ?? "unlabelled"}>
      {muted(`  ·  ${lane.role ? "" : "No role label"}`)}
      {lane.role ? <Text style={{ color: colors.foreground }}>{lane.role}</Text> : null}
      <Glyphs entries={lane.current} colors={colors} words={words} />
    </Text>)}
    {earlier ? muted(`  ·  ${earlier} earlier`) : null}
  </>;
}

/**
 * The linked sessions of one Board task, below the row and beside its other
 * controls rather than inside them. One session opens directly; several, or a
 * declared team, open the chooser. A declared team adds its risk tier and
 * workflow above the roles, inside the same control.
 */
export function TaskSessionsControl({ testID, links, team = null, live, compact, colors, onPress, buttonRef }: {
  testID: string; links: readonly TaskLink[]; team?: TaskTeamDeclaration | null; live: boolean; compact: boolean; colors: Colors;
  onPress: () => void; buttonRef: (node: View | null) => void;
}) {
  const single = directSession(links, team);
  const line = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 };
  return <Pressable testID={testID} ref={buttonRef} accessibilityRole="button"
    accessibilityLabel={single ? `Open linked session ${single.title}. ${chainDescription(links, live, team)}` : `Choose a linked session. ${chainDescription(links, live, team)}`}
    onPress={onPress}
    style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({ minHeight: COARSE_POINTER ? 44 : 24, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 6, borderRadius: 6,
      backgroundColor: pressed || hovered ? colors.surface2 : undefined })}>
    <View style={{ flex: 1, minWidth: 0 }}>
      {team ? <Text testID={`${testID}-team`} numberOfLines={1} style={line}>{teamHeadline(team)}</Text> : null}
      <Text numberOfLines={1} style={line}>
        {team && team.roles.length ? <TeamChainText declaration={team} links={links} colors={colors} words={!compact} live={live} />
          : links.length ? <ChainText lanes={roleLanes(links)} colors={colors} words={!compact} live={live} /> : "No linked session"}
      </Text>
    </View>
    <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>{single ? "Open session ›" : `${plural(links.length, "session")} ›`}</Text>
  </Pressable>;
}

/** What a read of the task's team says in the chooser, in a line or two; where it comes from stays in Source details. */
function TeamBlock({ team, links, colors }: { team: TaskTeamRead; links: readonly TaskLink[]; colors: Colors }) {
  const small = { color: colors.foregroundMuted, fontSize: 11, lineHeight: 15 };
  if (team.state === "not-provided") return <Text testID="board-sessions-team" style={small}>Team, workflow and risk tier: not provided</Text>;
  const { declaration } = team;
  return <View testID="board-sessions-team" style={{ gap: 2 }}>
    <Text style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>{teamHeadline(declaration)}</Text>
    <Text style={small}>{`Team ${declaration.team ?? "not declared"}`}</Text>
    {declaration.roles.length ? <Text style={{ ...small, fontSize: 12, lineHeight: 16 }}><TeamChainText declaration={declaration} links={links} colors={colors} words={false} live /></Text> : null}
  </View>;
}

/** Linked sessions by workspace, then replaced or archived ones; the team comes first and how the links were read comes last. */
export function TaskSessionsChooser({ taskKey, task, links, team, live, archivedIncluded, anchor, root, rootSize, compact, colors, sessions, onClose }: {
  taskKey: string; task: SessionTaskFilter; links: readonly TaskLink[]; team?: TaskTeamRead; live: boolean; archivedIncluded: boolean;
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
  const keys = [...new Set(links.map((link) => link.via))];
  const details = [
    links.length ? `Linked by an exact ${keys.join(" or ")} label equal to ${taskKey}. Titles are not read.` : `No session has a jira.issue, task or kontor.ticket label equal to ${taskKey}.`,
    team?.state === "declared" ? `Team, workflow and risk tier declared by ${team.declaration.source}. A session sits under a role by its own role label, which assigns nothing.` : null,
    team?.state === "not-provided" ? team.coverage : null,
    runs.length ? `Kontor team run label: ${runs.join(", ")}.` : null,
    live ? null : "Last known states: the host's live session feed is not attached.",
    archivedIncluded ? null : "Archived sessions are not read.",
  ].filter((part): part is string => !!part);
  return <AnchoredMenu testID="board-sessions-menu" dialog anchor={anchor} root={root} rootSize={rootSize} colors={colors} label={`Sessions linked to ${taskKey}`} onClose={onClose}
    placement={compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.7, 480) } : { align: "end", width: 400, maxHeight: 480 }}>
    <ScrollView keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
      <View style={{ paddingHorizontal: 10, paddingTop: 6, paddingBottom: 2, gap: 4 }}>
        <Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 13 }}>{`${taskKey} · ${links.length ? plural(links.length, "linked session") : "no linked session"}`}</Text>
        {team ? <TeamBlock team={team} links={links} colors={colors} /> : null}
        {live || !links.length ? null : <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 15 }}>Last known states</Text>}
      </View>
      {[...workspaces.values()].map((group) => <View key={group.id ?? group.label}>
        {section(group.label)}
        {group.id && sessions.onOpenWorkspace ? <MenuItem indent={1} tall={compact} colors={colors} label="Open workspace" accessibilityLabel={`Open workspace ${group.label}`} onPress={() => choose(() => sessions.onOpenWorkspace?.(group.id!))} /> : null}
        {group.rows.map(item)}
      </View>)}
      {earlier.length ? <View>{section("Replaced or archived")}{earlier.map(item)}</View> : null}
      <View style={{ height: 1, backgroundColor: colors.border, marginVertical: 4 }} />
      <MenuItem tall={compact} colors={colors} label="Show in Sessions" accessibilityLabel={`Show the sessions linked to ${taskKey} in Sessions`} onPress={() => choose(() => sessions.onShowInSessions(task))} />
      <View testID="board-sessions-details" style={{ paddingHorizontal: 10, paddingBottom: 4 }}>
        <Disclosure small label="Source details" accessibilityLabel={`Source details for the sessions linked to ${taskKey}`} colors={colors}>
          <View style={{ gap: 2, paddingBottom: 4 }}>{details.map((detail) => <Copy key={detail} colors={colors} muted small>{detail}</Copy>)}</View>
        </Disclosure>
      </View>
    </ScrollView>
  </AnchoredMenu>;
}
