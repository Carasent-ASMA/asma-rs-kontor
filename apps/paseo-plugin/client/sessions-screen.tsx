import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import {
  FlatList,
  Icon,
  ScrollView,
  TextInput,
  useToast,
} from "@getpaseo/plugin/client/react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentRef,
  type ReactNode,
  type Ref,
  type RefObject,
} from "react";
import {
  AccessibilityInfo,
  ActivityIndicator,
  Animated,
  Pressable,
  Text,
  View,
  type FlatList as NativeFlatList,
  type LayoutChangeEvent,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
} from "react-native";
import { openSessionById } from "./session-actions";
import { type DirectoryEntry } from "./session-directory";
import {
  attentionLabel,
  archivedLabel,
  earlierErrorLabel,
  matchesQuery,
  matchesStatus,
  MORE_STATUS_FILTERS,
  QUICK_STATUS_FILTERS,
  statusFilterLabel,
  sessionAvailabilityLabel,
  type SessionFilters,
  type StatusFilter,
} from "./session-view";
import {
  isResultMode,
  clearSessionFilterPatch,
  sessionFilterCount,
  patchLocalExpansion,
  patchSessionView,
  readLocalExpansion,
  readSessionView,
  sessionViewTransition,
  writeScrollOffset,
  type SessionGrouping,
  type SessionViewState,
} from "./session-view-store";
import { inWorkspace, labelledRole, matchesResultSearch, sessionAncestors, sessionProvenance, taskSessions, TASK_JIRA_LABELS, TASK_TICKET_LABEL, type SessionProvenance } from "./session-results";
import {
  accumulateScrollDelta,
  EMPTY_SCROLL_ACCUMULATOR,
  initialToolbarVisibility,
  reduceToolbarVisibility,
  TOOLBAR_LAYOUT_GAP,
  toolbarCompensationHeight,
  toolbarLayout,
  toolbarScrollCompensation,
  type ScrollAccumulator,
  type ToolbarVisibility,
} from "./toolbar-visibility";
import { useSessionDirectory } from "./use-session-directory";
import { DEFAULT_ORGANIZATION_SETTINGS, type OrganizationSettings } from "../shared/organization";
import type { useOrganizationSettings } from "./use-organization-settings";
import { allGroupKeys, buildOrganizationTree, declaredScope, defaultGroupExpanded, deriveDeskScopes, filterOrganizationTree, flattenOrganizationTree, inEpicScope, matchesScope, nativeDesk, NO_LINKED_EPIC, NO_ROLE_KEY, organizationScope, OTHER_WORKSTREAM, ROLE_ROOT_KEY, scopeKeepsGroup, sessionRole, sessionState, STATE_LABELS, titleRoleCode, UNASSIGNED_EPIC, visibleSessions, workstreamRank, type OrganizationGroup, type OrganizationRow, type OrganizationWorkspace, type OrganizationProject, type RoleSource } from "./session-organization";
import { rowKey, siblingKeys, reorderSavedSiblingOrder } from "./organization-order";
import { OrganizationRowView, StateBadge, stateColor as sessionStateColor, WORKSPACE_KINDS } from "./organization-row";
import { AnchoredMenu, MenuItem } from "./anchored-menu";
import { Action, COARSE_POINTER, Disclosure } from "./page-controls";
import { nativeFreshnessLabel } from "./native-freshness";
import { sessionTimeLines, useClock, useUpdatedTime, type RowTime, type TimeFreshness } from "./session-time";
import { sessionUsageLabel } from "./session-usage";
import { SessionUsage, SessionUsageLines } from "./session-usage-view";
import { MobileFiltersPage, focusControl } from "./mobile-filters";
import { MobileQuickToolbar } from "./mobile-quick-toolbar";
import { MobileQuickChooser } from "./mobile-quick-chooser";
import { useSessionLifecycle } from "./use-session-lifecycle";
import { ArchiveSessionButton, SessionLifecycleDialog } from "./session-lifecycle-view";
import { useSessionAvailability } from "./use-session-availability";

type ThemeColors = PluginTheme["colors"];
/** A tree row, or a flat match while a search, state or task filter is on. */
type ListRow = OrganizationRow | { readonly kind: "result"; readonly entry: DirectoryEntry };
function listRowKey(row: ListRow): string {
  return row.kind === "result" ? `result:${row.entry.id}` : rowKey(row);
}

/** Status dot colour of a session: its highest-priority state, as on group badges. */
export function indicatorColor(entry: DirectoryEntry, colors: ThemeColors): string {
  return sessionStateColor(sessionState(entry), colors);
}

/** Desk shortcuts; Other work is every session outside a desk workstream. */
export const SESSION_SHORTCUTS: readonly { readonly id: string | null; readonly label: string }[] = [
  { id: null, label: "All sessions" },
  { id: "PR Review", label: "PR Review" },
  { id: "Adam", label: "Adam desk" },
  { id: "Other work", label: "Other work" },
];
/** Workspace scope, a menu in Filters & view; the shortcuts above repeat all but Durable desks. */
const SCOPE_CHOICES: readonly { readonly id: string | null; readonly label: string }[] = [
  { id: null, label: "All Paseo workspaces" },
  { id: "Desks", label: "Durable desks" },
  { id: "PR Review", label: "PR Review desk" },
  { id: "Adam", label: "Adam desk" },
  { id: "Other work", label: "Other workspaces" },
];
const GROUPINGS: readonly { readonly id: SessionGrouping; readonly label: string }[] = [
  { id: "hierarchy", label: "Workstreams" },
  { id: "placement", label: "Desks & workspaces" },
  { id: "role", label: "Role" },
  { id: "flat", label: "Flat" },
];
const STATUS_DOTS: Partial<Record<StatusFilter, Parameters<typeof sessionStateColor>[0]>> = {
  running: "running", idle: "idle", error: "failed", needs_input: "needs_input", attention: "attention", initializing: "initializing", closed: "closed",
};

export function matchesShortcut(
  entry: DirectoryEntry,
  shortcut: string | null,
  mappings: Parameters<typeof organizationScope>[1],
): boolean {
  return matchesScope(entry, shortcut, mappings);
}

function workstreamKey(label: string): string {
  return label.trim().toLocaleLowerCase();
}

export const FILTERS_TOGGLE_TEST_ID = "sessions-advanced-toggle";
export const CLEAR_FILTERS_TEST_ID = "sessions-clear-filters";
export const FILTERS_TOUCH_TARGET = 44;

export function filtersAccessibilityLabel(activeCount: number): string {
  return activeCount > 0 ? `Filters and view, ${activeCount} active` : "Filters and view";
}

export function filtersDisclosureGlyph(expanded: boolean): string {
  return expanded ? "⌃" : "⌄";
}

export const HIDE_FILTERS_TEST_ID = "sessions-hide-filters";
export const FILTER_SUMMARY_TEST_ID = "sessions-filter-summary";
export const SCOPE_SUMMARY_TEST_ID = "sessions-scope-summary";
/** Downward travel, from the lowest offset since the compact toolbar opened, that closes it. */
export const COMPACT_COLLAPSE_DISTANCE = 24;
/** Scroll room a short list keeps while the compact toolbar is open, so a small drag closes it on every platform. */
export const COMPACT_SCROLL_SLACK = 2 * COMPACT_COLLAPSE_DISTANCE;
/**
 * Filters & view scrolls inside its own frame: the toolbar ends at this share of the space it shares
 * with the list, so the rest still shows sessions, and the frame shows at least three 44pt controls.
 */
export const OPTIONS_SPACE_SHARE = 0.7;
export const OPTIONS_MIN_HEIGHT = 3 * FILTERS_TOUCH_TARGET;
/** Wide screen padding above the toolbar and below the list. */
const WIDE_SCREEN_PADDING = 16;
/**
 * Below this compact screen height, Show in tree folds the slim toolbar and the filter summary into
 * its Back row: at 320pt they would otherwise fill the screen and hide the session it shows.
 */
export const SHORT_PEEK_HEIGHT = 420;

/** Compact toolbar disclosure, kept in the local in-memory view store beside the filters. */
export function compactToolbarOpen(view: SessionViewState): boolean {
  return view.toolbarOpen === true;
}

export function toolbarOpenPatch(open: boolean): Partial<SessionViewState> {
  return { toolbarOpen: open };
}

/** Compact starts with the slim line until the user opens the toolbar; wide layouts follow the restored offset. */
function initialToolbarCollapsed(view: SessionViewState, compact: boolean): boolean {
  return compact
    ? !compactToolbarOpen(view)
    : initialToolbarVisibility(view.scrollOffset) === "collapsed";
}

/**
 * Compact scroll collapse. The open toolbar heads the list content and scrolls
 * away with the rows. On a list long enough to scroll it out of view, it closes
 * once it has left the viewport, so the compensating offset moves nothing on
 * screen. On a shorter list it closes after a small deliberate drag. Both need
 * the hysteresis distance from the lowest offset since it opened; the raw native
 * offset counts, so an iOS rubber-band drag past a short list closes it too.
 * Focus, a screen reader or a programmatic scroll re-anchor instead of closing.
 * Nothing here reopens.
 */
export function compactToolbarScroll(input: {
  readonly anchor: number;
  readonly offset: number;
  readonly pinned: boolean;
  readonly suppressed: boolean;
  /** Content height the open toolbar adds above the rows, its gap included; 0 while unmeasured. */
  readonly toolbarHeight: number;
  /** Scroll range of the list while the toolbar is open. */
  readonly maxOffset: number;
}): { readonly anchor: number; readonly collapse: boolean } {
  if (!Number.isFinite(input.offset)) {
    return { anchor: input.anchor, collapse: false };
  }
  const offset = Math.max(0, input.offset);
  if (input.pinned || input.suppressed || !Number.isFinite(input.anchor)) {
    return { anchor: offset, collapse: false };
  }
  const anchor = Math.min(input.anchor, offset);
  const deliberate = offset - anchor >= COMPACT_COLLAPSE_DISTANCE;
  const canLeaveView = input.toolbarHeight > 0 && input.maxOffset >= input.toolbarHeight;
  return { anchor, collapse: deliberate && (!canLeaveView || offset >= input.toolbarHeight) };
}

/**
 * Offset to apply when the compact toolbar inside the list opens or closes.
 * Closing removes its height from above the rows, so the offset drops by the
 * same height and the rows in view stay put. Opening shows it from the top of
 * the list, where it lives. Null leaves the offset alone.
 */
export function compactToolbarCompensation(input: {
  readonly collapsed: boolean;
  readonly offset: number;
  readonly toolbarHeight: number;
}): number | null {
  if (!Number.isFinite(input.offset) || input.offset <= 0) {
    return null;
  }
  if (!input.collapsed) {
    return 0;
  }
  return input.toolbarHeight > 0 ? Math.max(0, input.offset - input.toolbarHeight) : null;
}

function shortcutLabel(shortcut: string): string {
  return (
    SESSION_SHORTCUTS.find((choice) => choice.id === shortcut)?.label ??
    SCOPE_CHOICES.find((choice) => choice.id === shortcut)?.label ??
    shortcut
  );
}

/** Every choice that narrows or widens the visible sessions, for the collapsed compact summary. */
export function activeFilterParts(input: {
  readonly query: string;
  readonly status: StatusFilter;
  readonly shortcut: string | null;
  readonly streams: readonly string[] | null;
  readonly includeArchived: boolean;
  readonly task?: string | null;
}): string[] {
  const query = input.query.trim();
  return [
    input.task ? `Task ${input.task}` : null,
    query ? `“${query}”` : null,
    input.status !== "all" ? statusFilterLabel(input.status) : null,
    input.shortcut !== null ? shortcutLabel(input.shortcut) : null,
    input.streams !== null ? `${input.streams.length} workstream${input.streams.length === 1 ? "" : "s"}` : null,
    input.includeArchived ? "Archived included" : null,
  ].filter((part): part is string => part !== null);
}

/**
 * On web a mouse or touch press also focuses the control it presses. Browsers mark only keyboard
 * focus as :focus-visible, so a focus without it came from a press. Native focus is keyboard-only.
 */
export function pressFocus(event: unknown): boolean {
  const target = (event as { target?: { matches?: (selector: string) => boolean } } | undefined)?.target;
  if (typeof target?.matches !== "function") return false;
  try {
    return !target.matches(":focus-visible");
  } catch {
    return false;
  }
}

/** Prefix of the failure note kontor-screen passes as workspaceNote. */
const WORKSPACE_LIST_ERROR = "Workspace list unavailable";

/** What the list shows: the tree, flat results, or one result in its place in the tree. */
function listModeOf(view: SessionViewState): string {
  return !isResultMode(view) ? "tree" : view.peek ? `peek:${view.peek.sessionId}` : "results";
}

function updatedLabel(timestamp: number | null): string {
  if (timestamp === null) {
    return "not read yet";
  }
  return new Date(timestamp).toLocaleTimeString();
}

type SessionRowProps = {
  readonly entry: DirectoryEntry;
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly onPress: (entry: DirectoryEntry) => void;
  readonly displayTitle?: string;
  /** Placement context, shown only where the tree does not already say it (Role and Flat). */
  readonly displayContext?: string;
  /** A shorter context that compact keeps beside the title instead of after it, so it stays in view (Role). */
  readonly compactContext?: string;
  /** Spoken in place of displayContext. */
  readonly accessibilityContext?: string;
  /** The result a user asked to see in the tree. */
  readonly highlighted?: boolean;
  /** The list is reading live: only then does the row give its Updated time as an age. */
  readonly live?: boolean;
};

/** A row's Updated time, kept whole while the title beside it truncates. */
function rowAge(time: RowTime, compact: boolean, colors: ThemeColors) {
  return (
    <Text testID="sessions-row-age" numberOfLines={1} style={{ flexShrink: 0, color: colors.foregroundMuted, fontSize: compact ? 12 : 11 }}>
      {compact ? time.compact : time.wide}
    </Text>
  );
}

export const SessionRow = memo(function SessionRow({
  entry,
  colors,
  compact,
  onPress,
  displayTitle,
  displayContext,
  compactContext,
  accessibilityContext,
  highlighted = false,
  live = false,
}: SessionRowProps) {
  const time = useUpdatedTime(entry.updatedAt, live);
  const age = time ? rowAge(time, compact, colors) : null;
  const attention = attentionLabel(entry);
  const archived = archivedLabel(entry);
  const history = earlierErrorLabel(entry);
  const state = sessionState(entry);
  const context = entry.workspaceName ?? entry.projectName ?? entry.cwd;
  const project =
    entry.projectName && entry.projectName !== entry.workspaceName
      ? entry.projectName
      : null;
  const detail = displayContext ?? (project ? `${context} · ${project}` : context);
  const accessibilityLabel = [
    entry.title,
    `Native ID ${entry.id}`,
    `Native status ${entry.status}`,
    entry.needsInput
      ? "Needs input"
      : attention
        ? `Needs attention: ${attention}`
        : null,
    time?.spoken ?? null,
    archived,
    history,
    accessibilityContext ?? detail,
    sessionUsageLabel(entry),
    highlighted ? "Shown from search results" : null,
  ]
    .filter((part): part is string => part !== null && part.length > 0)
    .join(". ");
  const meta = [displayContext, archived, history, sessionAvailabilityLabel(entry)].filter((part): part is string => !!part).join(" · ");
  const runtime = (state === "needs_input" || state === "attention" || state === "failed") && entry.status !== "error";
  const runtimeLabel = entry.status.charAt(0).toLocaleUpperCase() + entry.status.slice(1);
  // Compact: the title and context lead, then model/report metadata can wrap.
  // The state badge stays visible; the native ID remains in the accessible label.
  const compactMeta = [runtime ? runtimeLabel : null, compactContext ?? displayContext, archived, history, sessionAvailabilityLabel(entry)]
    .filter((part): part is string => !!part)
    .join(" · ");

  return (
    <Pressable
      testID={`sessions-row-${entry.id}`}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityHint="Opens the existing Paseo session"
      onPress={() => onPress(entry)}
      style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
        minHeight: compact || COARSE_POINTER ? 44 : 40,
        paddingLeft: 8,
        paddingRight: 4,
        paddingVertical: compact ? 4 : 5,
        flexDirection: "row",
        alignItems: "center",
        gap: compact ? 8 : 10,
        borderRadius: 5,
        borderLeftWidth: highlighted ? 3 : 0,
        borderLeftColor: colors.accent,
        backgroundColor: pressed || hovered || highlighted ? colors.surface2 : undefined,
      })}
    >
      {compact && compactContext !== undefined ? (
        <>
          {/* The context keeps up to about half the line; the title gives way first. */}
          <View style={{ flex: 1, minWidth: 0 }}>
          <View style={{ minWidth: 0, flexDirection: "row", alignItems: "center", gap: 8 }}>
            <Text testID="sessions-row-title" numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: 14, lineHeight: 20 }}>
              {displayTitle ?? entry.title}
            </Text>
            <Text testID="sessions-row-meta" numberOfLines={1} style={{ flexShrink: 0, maxWidth: "55%", color: colors.foregroundMuted, fontSize: 12 }}>
              {compactMeta}
            </Text>
            {age}
          </View>
          <SessionUsage entry={entry} colors={colors} />
          </View>
          <StateBadge state={state} colors={colors} compact={false} />
        </>
      ) : compact ? (
        <>
          <View style={{ flex: 1, minWidth: 0 }}>
          <Text
            testID="sessions-row-title"
            numberOfLines={1}
            style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: 14, lineHeight: 20 }}
          >
            {displayTitle ?? entry.title}
            {compactMeta ? (
              <Text testID="sessions-row-meta" style={{ color: colors.foregroundMuted, fontSize: 12 }}>
                {`  ${compactMeta}`}
              </Text>
            ) : null}
          </Text>
          {age}
          <SessionUsage entry={entry} colors={colors} />
          </View>
          <StateBadge state={state} colors={colors} compact={false} />
        </>
      ) : (
        <>
          <View style={{ flex: 1, minWidth: 0 }}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <Text
                numberOfLines={1}
                style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: 13, lineHeight: 18 }}
              >
                {displayTitle ?? entry.title}
              </Text>
              {age}
            </View>
            {meta ? (
              <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, marginTop: 1 }}>
                {meta}
              </Text>
            ) : null}
          </View>
          <View style={{ maxWidth: 340, minWidth: 0, flexShrink: 1 }}><SessionUsage entry={entry} colors={colors} /></View>
          <View style={{ width: 95, gap: 2 }}>
            <StateBadge state={state} colors={colors} compact={false} />
            {runtime ? (
              <Text style={{ color: colors.foregroundMuted, fontSize: 9 }}>
                {runtimeLabel}
              </Text>
            ) : null}
          </View>
        </>
      )}
    </Pressable>
  );
});

function capitalized(value: string): string {
  return value.charAt(0).toLocaleUpperCase() + value.slice(1);
}

/**
 * Cues a result shows after its place: role, task and PR labels, and the native
 * runtime where the state badge reports something else, such as an error
 * reported earlier on a session that is idle now. The title is never parsed.
 */
export function resultCues(entry: DirectoryEntry): string[] {
  const state = sessionState(entry);
  const task = [...TASK_JIRA_LABELS, TASK_TICKET_LABEL].map((key) => entry.labels[key]?.trim()).find((value) => !!value);
  const pr = entry.labels.pr?.trim().replace(/^#/, "");
  const derived = (state === "needs_input" || state === "attention" || state === "failed") && entry.status !== "error";
  return [
    earlierErrorLabel(entry) ?? (derived ? `Now ${entry.status}` : null),
    archivedLabel(entry),
    labelledRole(entry),
    task ?? null,
    pr ? `PR ${pr}` : null,
  ].filter((part): part is string => !!part);
}

function spokenProvenance(provenance: SessionProvenance): string {
  return provenance.desk ? `Desk ${provenance.parts.join(", workspace ")}` : `Workstream ${provenance.parts[0]}, epic ${provenance.parts[1]}, workspace ${provenance.parts[2]}`;
}

const ROLE_SOURCE_NAMES: Readonly<Record<RoleSource, string>> = { "kontor.role": "kontor.role label", role: "role label", title: "title" };

/**
 * A Role view row: a source that names another role comes first as a cue, then where the session
 * sits. The title loses its prefix only when the prefix names the role it is shown under.
 */
export function roleRowContext(entry: DirectoryEntry, mappings: Parameters<typeof organizationScope>[1]) {
  const role = sessionRole(entry);
  const cues = role.conflicts.map(({ source, code }) => `${ROLE_SOURCE_NAMES[source]} says ${code}`);
  const provenance = sessionProvenance(entry, mappings);
  const scope = organizationScope(entry, mappings);
  const place = provenance.desk ? provenance.parts[0]! : scope.epic === UNASSIGNED_EPIC ? scope.workstream : scope.epic;
  return {
    title: role.code !== null && titleRoleCode(entry.title) === role.code ? entry.title.slice(entry.title.indexOf("•") + 1).trim() : undefined,
    context: [...cues, ...provenance.parts].join(" · "),
    compact: [...cues, place, provenance.workspace].join(" · "),
    spoken: [role.code === null ? "No role" : `Role ${role.code} from the ${ROLE_SOURCE_NAMES[role.source!]}`, ...cues, spokenProvenance(provenance)].join(". "),
  };
}

/** The page's document on web; native has none, and keeps its own focus rules. */
const pageDocument = () => (globalThis as { document?: { activeElement?: unknown; body?: unknown } }).document;

/**
 * Everything a Role row shortens, for its details: the role and where it came from, every other
 * declaration, and the whole desk or workstream, epic and workspace chain, with absent values named.
 * A workstream may come from a native desk project when no display mapping or label names it.
 */
export function sessionDetailLines(entry: DirectoryEntry, mappings: Parameters<typeof organizationScope>[1]): { label: string; value: string }[] {
  const role = sessionRole(entry);
  const provenance = sessionProvenance(entry, mappings);
  const lines = [
    { label: "Role", value: role.code === null ? "No role: no kontor.role or role label, and no role prefix in the title" : `${role.code}, from the ${ROLE_SOURCE_NAMES[role.source!]}` },
    { label: "Other role declarations", value: role.conflicts.length ? role.conflicts.map(({ source, code }) => `${ROLE_SOURCE_NAMES[source]} says ${code}`).join("; ") : "None" },
  ];
  if (provenance.desk) return [...lines, { label: "Desk", value: provenance.parts[0]! }, { label: "Workspace", value: provenance.workspace }];
  const declared = declaredScope(entry, mappings);
  return [...lines,
    { label: "Workstream", value: declared.workstream ?? (nativeDesk(entry.projectName) !== null ? `${organizationScope(entry, mappings).workstream} (native desk project)` : `${OTHER_WORKSTREAM}: no workstream or desk label and no display mapping`) },
    { label: "Epic", value: declared.epic ?? NO_LINKED_EPIC },
    { label: "Workspace", value: provenance.workspace }];
}

/** Opens session details beside the row's own session-navigation button. */
export function SessionDetailsButton({ title, expanded, compact, colors, onPress, ref }: {
  readonly title: string; readonly expanded: boolean; readonly compact: boolean; readonly colors: ThemeColors; readonly onPress: () => void; readonly ref?: Ref<View>;
}) {
  const size = compact || COARSE_POINTER ? 44 : 30;
  return (
    <Pressable ref={ref} testID="sessions-row-details" accessibilityRole="button" accessibilityLabel={`Details: ${title}`} accessibilityHint="Shows exact times with their time zone, model, token report, role and placement details"
      accessibilityState={{ expanded }} aria-expanded={expanded} onPress={onPress}
      style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ width: size, height: size, borderRadius: 6, alignItems: "center", justifyContent: "center", backgroundColor: pressed || hovered || focused ? colors.surface2 : undefined })}>
      <Icon name="Info" size={compact ? 16 : 14} color={colors.foregroundMuted} />
    </Pressable>
  );
}

/** A session's times at the top of its details, redrawn by the shared clock while they are open. */
export function SessionTimeLines({ entry, freshness, colors }: { readonly entry: DirectoryEntry; readonly freshness: TimeFreshness; readonly colors: ThemeColors }) {
  const clock = useClock();
  return (
    <View testID="sessions-details-times">
      {sessionTimeLines(entry, clock, freshness).map(({ label, value }) => (
        <View key={label} style={{ paddingHorizontal: 6, paddingVertical: 4 }}>
          <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
          <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value}</Text>
        </View>
      ))}
    </View>
  );
}

export function resultAccessibilityLabel(entry: DirectoryEntry, provenance: SessionProvenance): string {
  const role = labelledRole(entry);
  const task = [...TASK_JIRA_LABELS, TASK_TICKET_LABEL].map((key) => entry.labels[key]?.trim()).find((value) => !!value);
  const derived = resultCues(entry).find((cue) => cue.startsWith("Earlier error") || cue.startsWith("Now "));
  return [
    entry.title,
    STATE_LABELS[sessionState(entry)],
    derived ? `Native status ${entry.status}${derived.startsWith("Earlier") ? ", error reported earlier" : ""}` : null,
    spokenProvenance(provenance),
    role ? `Role label ${role}` : null,
    task ? `Task label ${task}` : null,
    entry.labels.pr?.trim() ? `PR label ${entry.labels.pr.trim()}` : null,
    archivedLabel(entry),
    `Native ID ${entry.id}`,
    sessionUsageLabel(entry),
  ].filter((part): part is string => !!part).join(". ");
}

type SessionResultRowProps = {
  readonly lifecycleAction?: ReactNode;
  readonly entry: DirectoryEntry;
  readonly provenance: SessionProvenance;
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly onPress: (entry: DirectoryEntry) => void;
  readonly onShowInTree: (entry: DirectoryEntry) => void;
  readonly treeButtonRef: (id: string, node: View | null) => void;
  /** The list is reading live: only then does the row give its Updated time as an age. */
  readonly live: boolean;
  /** The session's details button, beside the row's own button. */
  readonly details?: ReactNode;
};

/** A flat match: the session opens from the row, and its place in the tree is a separate control beside it. */
const SessionResultRow = memo(function SessionResultRow({ entry, provenance, colors, compact, onPress, onShowInTree, treeButtonRef, live, details, lifecycleAction }: SessionResultRowProps) {
  const cues = resultCues(entry);
  const coarse = compact || COARSE_POINTER;
  const time = useUpdatedTime(entry.updatedAt, live);
  return (
    <View testID={`sessions-result-${entry.id}`} style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
      <Pressable
        testID={`sessions-row-${entry.id}`}
        accessibilityRole="button"
        accessibilityLabel={[resultAccessibilityLabel(entry, provenance), time?.spoken].filter(Boolean).join(". ")}
        accessibilityHint="Opens the existing Paseo session"
        onPress={() => onPress(entry)}
        style={({ pressed, hovered }: { pressed: boolean; hovered?: boolean }) => ({
          flex: 1,
          minWidth: 0,
          minHeight: coarse ? 52 : 40,
          paddingHorizontal: 8,
          paddingVertical: coarse ? 6 : 4,
          justifyContent: "center",
          gap: 2,
          borderRadius: 5,
          backgroundColor: pressed || hovered ? colors.surface2 : undefined,
        })}
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Text testID="sessions-row-title" numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: compact ? 14 : 13, lineHeight: compact ? 20 : 18 }}>
            {entry.title}
          </Text>
          {time ? rowAge(time, compact, colors) : null}
          {compact ? null : <View style={{ maxWidth: 340, minWidth: 0, flexShrink: 1 }}><SessionUsage entry={entry} colors={colors} /></View>}
          <StateBadge state={sessionState(entry)} colors={colors} compact={false} />
        </View>
        <Text testID="sessions-result-place" numberOfLines={compact ? 2 : 1} style={{ color: colors.foregroundMuted, fontSize: compact ? 12 : 11, lineHeight: compact ? 16 : 15 }}>
          {provenance.parts.join(" › ")}
          {cues.length > 0 ? `  ·  ${cues.join(" · ")}` : ""}
        </Text>
        {sessionAvailabilityLabel(entry) ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{sessionAvailabilityLabel(entry)}</Text> : null}
        {compact ? <SessionUsage entry={entry} colors={colors} /> : null}
      </Pressable>
      {details}
      <Action ref={(node) => treeButtonRef(entry.id, node)} small={!compact} colors={colors} label={compact ? "In tree" : "Show in tree"}
        accessibilityLabel={`Show ${entry.title} in the tree`} onPress={() => onShowInTree(entry)} />
      {lifecycleAction}
    </View>
  );
});

export function FiltersButton({
  activeCount,
  expanded,
  colors,
  compact,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
  ref,
}: {
  readonly ref?: Ref<View>;
  readonly activeCount: number;
  readonly expanded: boolean;
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly disabled?: boolean;
  readonly onBlur?: () => void;
  readonly onFocus?: () => void;
  readonly onPress: () => void;
}) {
  return (
    <Pressable
      ref={ref}
      testID={FILTERS_TOGGLE_TEST_ID}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityState={{ expanded }}
      aria-expanded={expanded}
      accessibilityLabel={filtersAccessibilityLabel(activeCount)}
      accessibilityHint={expanded ? "Collapses filter options" : "Expands filter options"}
      hitSlop={4}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 36,
        minWidth: FILTERS_TOUCH_TARGET,
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        paddingHorizontal: 10,
        paddingVertical: 6,
        borderRadius: 7,
        borderWidth: 1,
        borderColor: activeCount > 0 ? colors.accent : colors.border,
        backgroundColor: colors.surface1,
      }}
    >
      <Text
        numberOfLines={1}
        style={{
          color: colors.foreground,
          fontSize: compact ? 13 : 12,
          fontWeight: "600",
        }}
      >
        Filters &amp; view
      </Text>
      {activeCount > 0 ? (
        <View
          accessibilityElementsHidden
          importantForAccessibility="no"
          style={{
            minWidth: 20,
            height: 20,
            borderRadius: 10,
            paddingHorizontal: 5,
            alignItems: "center",
            justifyContent: "center",
            backgroundColor: colors.accent,
          }}
        >
          <Text
            style={{
              color: colors.accentForeground,
              fontSize: 11,
              fontWeight: "700",
            }}
          >
            {activeCount}
          </Text>
        </View>
      ) : null}
      <Text
        accessibilityElementsHidden
        importantForAccessibility="no"
        style={{
          color: colors.foregroundMuted,
          fontSize: 14,
          lineHeight: 16,
        }}
      >
        {filtersDisclosureGlyph(expanded)}
      </Text>
    </Pressable>
  );
}

export function ClearFiltersButton({
  colors,
  compact,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
}: {
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly disabled?: boolean;
  readonly onBlur?: () => void;
  readonly onFocus?: () => void;
  readonly onPress: () => void;
}) {
  return (
    <Pressable
      testID={CLEAR_FILTERS_TEST_ID}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel="Clear filters"
      accessibilityHint="Clears search, state, task, desk, workstream and archive filters; keeps the current context and view preferences"
      hitSlop={4}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 32,
        minWidth: FILTERS_TOUCH_TARGET,
        paddingHorizontal: 10,
        paddingVertical: 6,
        borderRadius: 7,
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <Text
        style={{
          color: colors.accent,
          fontSize: compact ? 13 : 12,
          fontWeight: "600",
        }}
      >
        Clear filters
      </Text>
    </Pressable>
  );
}

export function SlimActionButton({
  testID,
  label,
  accessibilityLabel,
  accessibilityHint,
  count,
  colors,
  disabled = false,
  expanded,
  onPress,
}: {
  readonly testID: string;
  readonly label: string;
  readonly accessibilityLabel: string;
  readonly accessibilityHint: string;
  readonly count?: number;
  readonly colors: ThemeColors;
  readonly disabled?: boolean;
  /** Disclosure state of the toolbar this action opens or closes. */
  readonly expanded?: boolean;
  readonly onPress: () => void;
}) {
  return (
    <Pressable
      testID={testID}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityHint={accessibilityHint}
      accessibilityState={expanded === undefined ? undefined : { expanded }}
      aria-expanded={expanded}
      hitSlop={4}
      onPress={onPress}
      style={{
        minHeight: FILTERS_TOUCH_TARGET,
        minWidth: FILTERS_TOUCH_TARGET,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "center",
        gap: 5,
        paddingHorizontal: 10,
        paddingVertical: 8,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: colors.border,
        backgroundColor: colors.surface1,
      }}
    >
      <Text
        numberOfLines={1}
        style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}
      >
        {label}
      </Text>
      {count !== undefined && count > 0 ? (
        <View
          accessibilityElementsHidden
          importantForAccessibility="no"
          style={{
            minWidth: 18,
            height: 18,
            borderRadius: 9,
            paddingHorizontal: 4,
            alignItems: "center",
            justifyContent: "center",
            backgroundColor: colors.accent,
          }}
        >
          <Text style={{ color: colors.accentForeground, fontSize: 10, fontWeight: "700" }}>
            {count}
          </Text>
        </View>
      ) : null}
    </Pressable>
  );
}

/**
 * Slim line above the session list: the host name and the Search and Filters
 * actions. The host's live state is not repeated here; the Kontor top bar's
 * native freshness pill shows it once, and problems surface in the source banner.
 */
export function SlimToolbar({
  action,
  hostLabel,
  activeCount,
  colors,
  compact,
  disabled = false,
  open = false,
  onSearch,
  onFilters,
  onHide,
}: {
  readonly action?: ReactNode;
  readonly hostLabel: string;
  readonly activeCount: number;
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly disabled?: boolean;
  /** Compact only: the full toolbar is open, so Filters becomes Hide filters. */
  readonly open?: boolean;
  readonly onSearch: () => void;
  readonly onFilters: () => void;
  readonly onHide?: () => void;
}) {
  const hide = compact && open && onHide !== undefined;
  return (
    <View
      testID="sessions-slim-toolbar"
      style={{
        minHeight: FILTERS_TOUCH_TARGET,
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
      }}
    >
      <View testID="sessions-slim-host" style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center" }}>
        <Text
          numberOfLines={1}
          style={{
            flexShrink: 1,
            color: colors.foreground,
            fontSize: compact ? 14 : 13,
            fontWeight: "600",
          }}
        >
          {hostLabel}
        </Text>
      </View>
      {action}
      <SlimActionButton
        testID="sessions-slim-search"
        label="Search"
        accessibilityLabel="Search sessions"
        accessibilityHint="Shows the search and filter controls"
        colors={colors}
        disabled={disabled}
        expanded={compact ? open : undefined}
        onPress={onSearch}
      />
      {hide ? (
        <SlimActionButton
          testID={HIDE_FILTERS_TEST_ID}
          label="Hide filters"
          accessibilityLabel="Hide filters"
          accessibilityHint="Hides search and filters so the sessions get the space"
          colors={colors}
          disabled={disabled}
          expanded
          onPress={onHide}
        />
      ) : (
        <SlimActionButton
          testID="sessions-slim-filters"
          label="Filters"
          accessibilityLabel={filtersAccessibilityLabel(activeCount)}
          accessibilityHint="Shows the filter options"
          count={activeCount}
          colors={colors}
          disabled={disabled}
          expanded={compact ? false : undefined}
          onPress={onFilters}
        />
      )}
    </View>
  );
}

/**
 * Collapsed compact toolbar: the page scope and every active filter stay
 * visible as one-tap chips, so hidden controls never silently hide sessions.
 */
export function ActiveFilterSummary({
  parts,
  scopeLabel,
  colors,
  onClear,
  onClearScope,
}: {
  readonly parts: readonly string[];
  readonly scopeLabel: string | null;
  readonly colors: ThemeColors;
  readonly onClear: () => void;
  readonly onClearScope?: () => void;
}) {
  if (parts.length === 0 && !scopeLabel) {
    return null;
  }
  const chip = (label: string, clearable: boolean) => (
    <View
      style={{
        flexShrink: 1,
        minWidth: 0,
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        paddingHorizontal: 10,
        paddingVertical: 5,
        borderRadius: 14,
        borderWidth: 1,
        borderColor: colors.accent,
        backgroundColor: colors.surface2,
      }}
    >
      <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: 12, fontWeight: "600" }}>
        {label}
      </Text>
      {clearable ? (
        <Text accessibilityElementsHidden importantForAccessibility="no" style={{ color: colors.foregroundMuted, fontSize: 12 }}>
          ✕
        </Text>
      ) : null}
    </View>
  );
  const pressableStyle = { flexShrink: 1, minWidth: 0, minHeight: FILTERS_TOUCH_TARGET, justifyContent: "center" } as const;
  return (
    <View
      testID="sessions-filter-summary-row"
      style={{ flexDirection: "row", alignItems: "center", gap: 6, minWidth: 0 }}
    >
      {scopeLabel ? (
        onClearScope ? (
          <Pressable
            testID={SCOPE_SUMMARY_TEST_ID}
            accessibilityRole="button"
            accessibilityLabel={`Sessions scoped to ${scopeLabel}. Show all sessions`}
            onPress={onClearScope}
            style={pressableStyle}
          >
            {chip(scopeLabel, true)}
          </Pressable>
        ) : (
          <View testID={SCOPE_SUMMARY_TEST_ID} accessible accessibilityLabel={`Sessions scoped to ${scopeLabel}`} style={pressableStyle}>
            {chip(scopeLabel, false)}
          </View>
        )
      ) : null}
      {parts.length > 0 ? (
        <Pressable
          testID={FILTER_SUMMARY_TEST_ID}
          accessibilityRole="button"
          accessibilityLabel={`Active filters: ${parts.join(", ")}. Clear filters`}
          accessibilityHint="Clears search, state, task, desk, workstream and archive filters; keeps the current context and view preferences"
          onPress={onClear}
          style={pressableStyle}
        >
          {chip(parts.join(" · "), true)}
        </Pressable>
      ) : null}
    </View>
  );
}

/** A touch pointer gets a 44pt chip; a mouse keeps the denser one. */
export function FilterChip({
  active,
  label,
  colors,
  compact = false,
  count,
  dot,
  checkbox = false,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
  testID,
}: {
  active: boolean;
  label: string;
  colors: ThemeColors;
  compact?: boolean;
  /** Matching sessions under the other active filters. */
  count?: number;
  dot?: string;
  /** Multi-select choice rather than one of a set. */
  checkbox?: boolean;
  disabled?: boolean;
  onBlur?: () => void;
  onFocus?: () => void;
  onPress: () => void;
  testID?: string;
}) {
  return (
    <Pressable
      testID={testID}
      disabled={disabled}
      accessibilityRole={checkbox ? "checkbox" : "button"}
      accessibilityState={checkbox ? { checked: active, disabled } : { selected: active, disabled }}
      {...(checkbox ? { "aria-checked": active } : { "aria-pressed": active })}
      accessibilityLabel={count === undefined ? label : `${label}, ${count}`}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: COARSE_POINTER ? FILTERS_TOUCH_TARGET : compact ? 36 : 30,
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        paddingHorizontal: 8,
        borderRadius: 6,
        borderWidth: 1,
        borderColor: active ? colors.accent : colors.border,
        backgroundColor: active ? colors.surface2 : colors.surface1,
        opacity: disabled ? 0.5 : 1,
      }}
    >
      {dot ? <View style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: dot }} /> : null}
      <Text style={{ color: colors.foreground, fontSize: compact ? 12 : 11, fontWeight: active ? "600" : "400" }}>
        {label}
      </Text>
      {count !== undefined ? (
        <Text style={{ color: colors.foregroundMuted, fontSize: compact ? 12 : 11, fontVariant: ["tabular-nums"] }}>
          {count}
        </Text>
      ) : null}
    </Pressable>
  );
}

/** A labelled line of choices inside Filters & view. */
function OptionLine({ label, colors, children }: { label: string; colors: ThemeColors; children: ReactNode }) {
  return (
    <View style={{ gap: 5 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4, alignItems: "center" }}>{children}</View>
    </View>
  );
}

/** A labelled single choice inside Filters & view that opens as a menu, like the prototype's selects; the current value stays in view. */
export function OptionSelect({
  label,
  value,
  expanded,
  colors,
  compact,
  anchor,
  testID,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
}: {
  label: string;
  value: string;
  expanded: boolean;
  colors: ThemeColors;
  compact: boolean;
  anchor: RefObject<View | null>;
  testID: string;
  disabled?: boolean;
  onBlur?: () => void;
  onFocus?: (event?: unknown) => void;
  onPress: () => void;
}) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
      <Pressable
        ref={anchor}
        testID={testID}
        disabled={disabled}
        accessibilityRole="button"
        accessibilityLabel={`${label}: ${value}`}
        accessibilityHint={`Opens the ${label.toLocaleLowerCase()} choices`}
        accessibilityState={{ expanded, disabled }}
        aria-expanded={expanded}
        onBlur={onBlur}
        onFocus={onFocus}
        onPress={onPress}
        style={{
          minHeight: COARSE_POINTER ? FILTERS_TOUCH_TARGET : compact ? 36 : 30,
          flexShrink: 1,
          minWidth: 0,
          flexDirection: "row",
          alignItems: "center",
          gap: 6,
          paddingHorizontal: 8,
          borderRadius: 6,
          borderWidth: 1,
          borderColor: colors.border,
          backgroundColor: colors.surface1,
        }}
      >
        <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: compact ? 12 : 11, fontWeight: "600" }}>
          {value}
        </Text>
        <Text accessibilityElementsHidden importantForAccessibility="no" style={{ color: colors.foregroundMuted, fontSize: 12 }}>
          ▾
        </Text>
      </Pressable>
    </View>
  );
}

function ToggleRow({
  label,
  description,
  checked,
  colors,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
  testID,
}: {
  label: string;
  description: string;
  checked: boolean;
  colors: ThemeColors;
  disabled?: boolean;
  onBlur?: () => void;
  onFocus?: () => void;
  onPress: () => void;
  testID?: string;
}) {
  return (
    <Pressable
      testID={testID}
      disabled={disabled}
      accessibilityRole="switch"
      accessibilityState={{ checked }}
      aria-checked={checked}
      accessibilityLabel={label}
      accessibilityHint={description}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: 10,
        paddingVertical: 8,
      }}
    >
      <View
        style={{
          width: 20,
          height: 20,
          borderRadius: 5,
          borderWidth: 1,
          borderColor: checked ? colors.accent : colors.border,
          backgroundColor: checked ? colors.accent : colors.surface1,
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {checked ? (
          <Text style={{ color: colors.accentForeground, fontSize: 13, fontWeight: "700" }}>
            ✓
          </Text>
        ) : null}
      </View>
      <View style={{ flex: 1 }}>
        <Text style={{ color: colors.foreground, fontSize: 13 }}>{label}</Text>
        <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{description}</Text>
      </View>
    </Pressable>
  );
}

function Banner({
  message,
  actionLabel,
  colors,
  disabled = false,
  tone,
  onAction,
  onBlur,
  onFocus,
  testID,
}: {
  message: string;
  actionLabel?: string;
  colors: ThemeColors;
  disabled?: boolean;
  tone: "warning" | "danger";
  onAction?: () => void;
  onBlur?: () => void;
  onFocus?: () => void;
  testID?: string;
}) {
  return (
    <View
      testID={testID}
      style={{
        backgroundColor: colors.surface1,
        borderColor: tone === "danger" ? colors.statusDanger : colors.statusWarning,
        borderWidth: 1,
        borderRadius: 8,
        padding: 10,
        gap: 6,
        marginBottom: 10,
      }}
    >
      <Text style={{ color: colors.foreground, fontSize: 12 }}>{message}</Text>
      {actionLabel && onAction ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={actionLabel}
          disabled={disabled}
          onBlur={onBlur}
          onFocus={onFocus}
          onPress={onAction}
          style={{ alignSelf: "flex-start", paddingVertical: 4 }}
        >
          <Text style={{ color: colors.accent, fontSize: 12, fontWeight: "600" }}>
            {actionLabel}
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}

type SessionsScreenProps = PluginScreenProps & {
  directory?: ReturnType<typeof useSessionDirectory>;
  organization?: ReturnType<typeof useOrganizationSettings>;
  projectId?: string | null;
  onIncludeArchived?: (include: boolean) => void;
  workspaces?: readonly OrganizationWorkspace[];
  projects?: readonly OrganizationProject[];
  workspaceNote?: string;
  epicKey?: string | null;
  viewScope?: string;
  /** Explicit project/epic scope from another page; the Sessions tab itself opens unscoped. */
  scopeLabel?: string | null;
  onClearScope?: () => void;
  /** Configured plan source IDs by upper-case epic key; epics without one get no Plan/Board/Graph actions. */
  planSources?: Readonly<Record<string, readonly string[]>>;
  onEpicTab?: (tab: "Plan" | "Board" | "Graph", epicKey: string, sourceId: string | null) => void;
};
export function SessionsScreen({ host, layout, navigation, theme, directory, organization, projectId = null, onIncludeArchived, workspaces, projects, workspaceNote, epicKey = null, viewScope = host.id, scopeLabel = null, onClearScope, planSources, onEpicTab }: SessionsScreenProps) {
  const colors = theme.colors;
  const compact = layout.compact;
  const [mobileFilterScope, setMobileFilterScope] = useState<string | null>(null);
  const mobileFiltersOpen = compact && mobileFilterScope === `${host.id}:${viewScope}`;
  const filtersTriggerRef = useRef<View | null>(null);
  const quickHelpDismissRef = useRef<(() => void) | null>(null);
  const quickChooserRef = useRef<View | null>(null);
  const [quickChooserScope, setQuickChooserScope] = useState<string | null>(null);
  useEffect(() => { setQuickChooserScope(null); }, [compact, viewScope, mobileFiltersOpen]);
  const returnFromFiltersRef = useRef(false);
  const hosts = useHosts();
  const hostSummary = hosts.find((candidate) => candidate.serverId === host.id);
  const hostOnline = hostSummary?.status === "online";
  const [hostView, setHostView] = useState<{
    readonly hostId: string;
    readonly viewScope: string;
    readonly view: SessionViewState;
  }>(() => ({ hostId: host.id, viewScope, view: readSessionView(viewScope) }));
  const view =
    hostView.hostId === host.id && hostView.viewScope === viewScope
      ? hostView.view
      : readSessionView(viewScope);
  const localDirectory = useSessionDirectory({
    hostId: host.id,
    hostStatus: hostSummary?.status ?? "offline",
    includeArchived: view.includeArchived,
    enabled: directory === undefined,
  });
  const { state: rawState, refresh } = directory ?? localDirectory;
  const availability = useSessionAvailability(host.id, rawState.entries, hostOnline && rawState.status === "ready" && view.status === "attention");
  const state = useMemo(() => ({ ...rawState, entries: availability.entries }), [rawState, availability.entries]);
  const lifecycle = useSessionLifecycle(host.id, refresh);
  const [lifecycleOpen, setLifecycleOpen] = useState(false);
  const lifecycleAnchor = useRef<View | null>(null);
  const recoveryButton = useRef<View | null>(null);
  const slimRecoveryButton = useRef<View | null>(null);
  const archiveButtons = useRef(new Map<string, View>());
  useEffect(() => { setLifecycleOpen(false); }, [host.id]);
  const preferences = organization?.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const shortcut = view.shortcut ?? null;
  const grouping: SessionGrouping = view.grouping ?? preferences.grouping;
  const streams = view.streams ?? null;
  const arranging = view.arranging ?? false;
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [, setExpansionRevision] = useState(0);
  const [orderSelection, setOrderSelection] = useState<string | null>(null);
  const [optionMenu, setOptionMenu] = useState<"grouping" | "scope" | null>(null);
  const [rootSize, setRootSize] = useState({ width: 0, height: 0 });
  const rootRef = useRef<View | null>(null);
  const groupingRef = useRef<View | null>(null);
  const scopeRef = useRef<View | null>(null);
  const editsDisabled = !hostOnline || organization?.state.status !== "ready" || organization.state.saving;
  useEffect(() => { onIncludeArchived?.(view.includeArchived); }, [view.includeArchived, onIncludeArchived]);
  const savePreference = (patch: Partial<OrganizationSettings>) => {
    if (!organization || editsDisabled) return;
    void organization.save(patch).then((saved) => setSaveMessage(saved ? null : "Organization was not saved. Reload preferences in Settings and review the conflict; no newer revision was overwritten."));
  };
  const toast = useToast();
  const listRef = useRef<NativeFlatList<ListRow> | null>(null);
  const entriesRef = useRef(state.entries);
  entriesRef.current = state.entries;
  const restoredRef = useRef(false);
  const [toolbarCollapsed, setToolbarCollapsed] = useState(
    () => compact || initialToolbarCollapsed(readSessionView(viewScope), compact),
  );
  const toolbarCollapsedRef = useRef(toolbarCollapsed);
  toolbarCollapsedRef.current = toolbarCollapsed;
  // A select menu closes with the options panel that holds its select.
  useEffect(() => { if (!compact && (!view.advancedOpen || toolbarCollapsed)) setOptionMenu(null); }, [compact, view.advancedOpen, toolbarCollapsed]);
  const searchFocusedRef = useRef(false);
  const toolbarControlFocusedRef = useRef(false);
  const filterPinnedUntilRef = useRef(0);
  const screenReaderRef = useRef(false);
  const reduceMotionRef = useRef(false);
  const toolbarOpacityRef = useRef<Animated.Value | null>(null);
  if (toolbarOpacityRef.current === null) {
    toolbarOpacityRef.current = new Animated.Value(toolbarCollapsed ? 0 : 1);
  }
  const toolbarOpacity = toolbarOpacityRef.current;
  const expandedToolbarHeightRef = useRef(0);
  const slimHeightRef = useRef(FILTERS_TOUCH_TARGET);
  const scrollOffsetRef = useRef(readSessionView(viewScope).scrollOffset);
  const lastScrollOffsetRef = useRef(scrollOffsetRef.current);
  const maxScrollOffsetRef = useRef(0);
  const scrollAccumulatorRef = useRef<ScrollAccumulator>(EMPTY_SCROLL_ACCUMULATOR);
  /** Lowest offset since the compact toolbar opened; see compactToolbarScroll. */
  const compactAnchorRef = useRef(scrollOffsetRef.current);
  const previousCompactRef = useRef(compact);
  const [listHeight, setListHeight] = useState(0);
  const [optionsTop, setOptionsTop] = useState(0);
  const programmaticUntilRef = useRef(0);
  const pendingCompensationRef = useRef<number | null>(null);
  const pendingRevealRef = useRef<{ readonly focusSearch: boolean } | null>(null);
  const revealFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const searchInputRef = useRef<ComponentRef<typeof TextInput> | null>(null);
  const previousOrderRef = useRef(state.entries.map((entry) => entry.id).join("\n"));
  const previousViewRef = useRef({ hostId: host.id, viewScope });

  useEffect(() => {
    return () => {
      if (revealFallbackTimerRef.current !== null) {
        clearTimeout(revealFallbackTimerRef.current);
        revealFallbackTimerRef.current = null;
      }
    };
  }, []);

  useLayoutEffect(() => {
    if (previousViewRef.current.hostId === host.id && previousViewRef.current.viewScope === viewScope) {
      return;
    }
    previousViewRef.current = { hostId: host.id, viewScope };
    setMobileFilterScope(null);
    setOptionMenu(null);
    const nextView = readSessionView(viewScope);
    setHostView({ hostId: host.id, viewScope, view: nextView });
    restoredRef.current = false;
    listModeRef.current = listModeOf(nextView);
    placementRef.current += 1;
    treeAnchorRef.current = nextView.scrollOffset;
    scrollOffsetRef.current = nextView.scrollOffset;
    lastScrollOffsetRef.current = nextView.scrollOffset;
    maxScrollOffsetRef.current = 0;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    compactAnchorRef.current = nextView.scrollOffset;
    programmaticUntilRef.current = 0;
    pendingCompensationRef.current = null;
    pendingRevealRef.current = null;
    filterPinnedUntilRef.current = 0;
    searchFocusedRef.current = false;
    toolbarControlFocusedRef.current = false;
    previousOrderRef.current = "";
    const collapsed = compact || initialToolbarCollapsed(nextView, compact);
    toolbarCollapsedRef.current = collapsed;
    setToolbarCollapsed(collapsed);
    toolbarOpacity.setValue(collapsed ? 0 : 1);
  }, [host.id, viewScope, compact, toolbarOpacity]);

  useLayoutEffect(() => {
    if (previousCompactRef.current === compact) {
      return;
    }
    previousCompactRef.current = compact;
    // Compact keeps the user's open/closed choice; wide layouts follow the offset.
    const collapsed = compact || (mobileFilterScope === null && initialToolbarCollapsed(
      { ...readSessionView(viewScope), scrollOffset: scrollOffsetRef.current },
      compact,
    ));
    pendingCompensationRef.current = null;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    compactAnchorRef.current = scrollOffsetRef.current;
    toolbarCollapsedRef.current = collapsed;
    setToolbarCollapsed(collapsed);
    toolbarOpacity.setValue(collapsed ? 0 : 1);
  }, [compact, viewScope, toolbarOpacity]);

  useLayoutEffect(() => {
    const order = state.entries.map((entry) => entry.id).join("\n");
    if (order === previousOrderRef.current) {
      return;
    }
    previousOrderRef.current = order;
    // A live reorder can adjust the list offset. Do not read that as a
    // deliberate scroll and toggle the toolbar. Ordinary status updates that
    // keep the order do not suppress scrolling.
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    programmaticUntilRef.current = Math.max(
      programmaticUntilRef.current,
      Date.now() + 150,
    );
  }, [state.entries]);

  /** Tree offset below the compact toolbar, from the last tree scroll that moved the rows. */
  const treeAnchorRef = useRef(scrollOffsetRef.current);
  const listModeRef = useRef(listModeOf(readSessionView(viewScope)));
  /** Counts list placements (a mode or scope change); a scroll retry belongs to the one that asked for it. */
  const placementRef = useRef(0);
  /** The compact toolbar opening scrolls to its top; that is not a new tree position. */
  const anchorFrozenUntilRef = useRef(0);
  const updateView = useCallback(
    (patch: Partial<SessionViewState>) => {
      setHostView({
        hostId: host.id,
        viewScope,
        view: patchSessionView(viewScope, sessionViewTransition(readSessionView(viewScope), patch, treeAnchorRef.current)),
      });
    },
    [host.id, viewScope],
  );

  /** Content height the open compact toolbar adds above the rows; 0 until measured. */
  const compactToolbarHeight = useCallback(
    () =>
      expandedToolbarHeightRef.current > 0
        ? toolbarCompensationHeight({
            fullHeight: expandedToolbarHeightRef.current,
            slimHeight: slimHeightRef.current,
            compact: true,
            gap: TOOLBAR_LAYOUT_GAP,
          })
        : 0,
    [],
  );

  /**
   * Every change compensates the offset so the rows in view stay put. On
   * compact the open toolbar heads the list content: closing scrolls up by its
   * height, opening shows it from the top. Compact persists the choice.
   */
  const setToolbarVisibility = useCallback((next: ToolbarVisibility) => {
    if (next === (toolbarCollapsedRef.current ? "collapsed" : "expanded")) {
      return;
    }
    const collapsed = next === "collapsed";
    if (!compact && collapsed && expandedToolbarHeightRef.current <= 0) {
      return;
    }
    pendingCompensationRef.current = compact
      ? compactToolbarCompensation({
          collapsed,
          offset: scrollOffsetRef.current,
          toolbarHeight: compactToolbarHeight(),
        })
      : toolbarScrollCompensation({
          currentOffset: scrollOffsetRef.current,
          headerHeight: toolbarCompensationHeight({
            fullHeight: expandedToolbarHeightRef.current,
            slimHeight: slimHeightRef.current,
            compact,
            gap: TOOLBAR_LAYOUT_GAP,
          }),
          direction: collapsed ? "collapse" : "reveal",
          maxOffset: maxScrollOffsetRef.current,
        });
    toolbarCollapsedRef.current = collapsed;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    compactAnchorRef.current = scrollOffsetRef.current;
    if (compact && !collapsed) anchorFrozenUntilRef.current = Date.now() + 400;
    setToolbarCollapsed(collapsed);
    if (compact) {
      updateView(toolbarOpenPatch(!collapsed));
    }
  }, [compact, compactToolbarHeight, updateView]);

  useLayoutEffect(() => {
    const target = pendingCompensationRef.current;
    if (target === null) {
      return;
    }
    pendingCompensationRef.current = null;
    programmaticUntilRef.current = Date.now() + 250;
    scrollOffsetRef.current = target;
    lastScrollOffsetRef.current = target;
    compactAnchorRef.current = target;
    writeScrollOffset(viewScope, target);
    listRef.current?.scrollToOffset({ offset: target, animated: false });
  }, [viewScope, toolbarCollapsed]);

  useEffect(() => {
    let mounted = true;
    void AccessibilityInfo.isScreenReaderEnabled()
      .then((enabled) => {
        if (mounted) {
          screenReaderRef.current = enabled;
        }
      })
      .catch(() => undefined);
    void AccessibilityInfo.isReduceMotionEnabled()
      .then((enabled) => {
        if (mounted) {
          reduceMotionRef.current = enabled;
        }
      })
      .catch(() => undefined);
    const subscription = AccessibilityInfo.addEventListener(
      "screenReaderChanged",
      (enabled) => {
        screenReaderRef.current = enabled;
      },
    );
    const motionSubscription = AccessibilityInfo.addEventListener(
      "reduceMotionChanged",
      (enabled) => {
        reduceMotionRef.current = enabled;
      },
    );
    return () => {
      mounted = false;
      subscription.remove();
      motionSubscription.remove();
    };
  }, []);

  useLayoutEffect(() => {
    if (toolbarCollapsed) {
      // Collapse immediately. Keeping a fading overlay above the reclaimed
      // list would obscure the rows the user is scrolling through.
      toolbarOpacity.stopAnimation();
      toolbarOpacity.setValue(0);
      return;
    }
    const animation = Animated.timing(toolbarOpacity, {
      toValue: 1,
      duration: reduceMotionRef.current ? 0 : 120,
      useNativeDriver: true,
    });
    animation.start();
    return () => animation.stop();
  }, [toolbarCollapsed, toolbarOpacity]);

  const pinFilterInteraction = useCallback(() => {
    filterPinnedUntilRef.current = Date.now() + 1_500;
  }, []);

  const handleToolbarControlFocus = useCallback((event?: unknown) => {
    // Keyboard focus keeps the toolbar until it moves on; a pressed chip only gets the short interaction pin.
    toolbarControlFocusedRef.current = !pressFocus(event);
    pinFilterInteraction();
  }, [pinFilterInteraction]);

  const handleToolbarControlBlur = useCallback(() => {
    toolbarControlFocusedRef.current = false;
  }, []);

  const revealToolbar = useCallback(
    (focusSearch = false) => {
      pinFilterInteraction();
      setToolbarVisibility("expanded");
      if (focusSearch) {
        setTimeout(() => searchInputRef.current?.focus(), 0);
      }
    },
    [pinFilterInteraction, setToolbarVisibility],
  );

  /** Compact manual open: the list shows the toolbar from the top, and hysteresis starts there. */
  const openCompactToolbar = useCallback(() => {
    pinFilterInteraction();
    setToolbarVisibility("expanded");
  }, [pinFilterInteraction, setToolbarVisibility]);

  const handleRevealSearch = useCallback(() => {
    if (!compact) {
      revealToolbar(true);
      return;
    }
    openCompactToolbar();
    setTimeout(() => searchInputRef.current?.focus(), 0);
  }, [compact, openCompactToolbar, revealToolbar]);

  const handleHideToolbar = useCallback(() => {
    // An explicit choice: it overrides the focus and interaction pins.
    searchInputRef.current?.blur();
    searchFocusedRef.current = false;
    toolbarControlFocusedRef.current = false;
    filterPinnedUntilRef.current = 0;
    pendingRevealRef.current = null;
    setToolbarVisibility("collapsed");
  }, [setToolbarVisibility]);

  const handleRevealFilters = useCallback(() => {
    if (compact) {
      searchInputRef.current?.blur();
      setOptionMenu(null);
      setMobileFilterScope(`${host.id}:${viewScope}`);
      updateView({ advancedOpen: true });
      return;
    }
    pinFilterInteraction();
    if (!view.advancedOpen) {
      updateView({ advancedOpen: true });
      if (toolbarCollapsedRef.current) {
        pendingRevealRef.current = { focusSearch: false };
        if (revealFallbackTimerRef.current !== null) {
          clearTimeout(revealFallbackTimerRef.current);
        }
        revealFallbackTimerRef.current = setTimeout(() => {
          revealFallbackTimerRef.current = null;
          if (!pendingRevealRef.current) {
            return;
          }
          pendingRevealRef.current = null;
          setToolbarVisibility("expanded");
        }, 80);
      }
      return;
    }
    revealToolbar(false);
  }, [compact, host.id, viewScope, pinFilterInteraction, revealToolbar, setToolbarVisibility, updateView, view.advancedOpen]);

  const handleToolbarLayout = useCallback(
    (event: LayoutChangeEvent) => {
      const height = event.nativeEvent.layout.height;
      if (height > 0) {
        expandedToolbarHeightRef.current = height;
      }
      if (revealFallbackTimerRef.current !== null) {
        clearTimeout(revealFallbackTimerRef.current);
        revealFallbackTimerRef.current = null;
      }
      const pending = pendingRevealRef.current;
      if (!pending) {
        return;
      }
      pendingRevealRef.current = null;
      setToolbarVisibility("expanded");
      if (pending.focusSearch) {
        setTimeout(() => searchInputRef.current?.focus(), 0);
      }
    },
    [setToolbarVisibility],
  );

  const handleAdvancedToggle = useCallback(() => {
    pinFilterInteraction();
    updateView({ advancedOpen: !view.advancedOpen });
  }, [pinFilterInteraction, updateView, view.advancedOpen]);

  const updateFilter = useCallback(
    (patch: Partial<SessionFilters>) => {
      pinFilterInteraction();
      updateView({ ...patch, scrollOffset: 0 });
    },
    [pinFilterInteraction, updateView],
  );

  const handleOpen = useCallback(
    (entry: DirectoryEntry) => {
      const result = openSessionById({
        entries: entriesRef.current,
        id: entry.id,
        serverId: host.id,
        navigation,
      });
      if (!result.ok) {
        toast.error(result.message);
      }
    },
    [host.id, navigation, toast],
  );

  const handleClearFilters = useCallback(() => {
    pinFilterInteraction();
    updateView(clearSessionFilterPatch(readSessionView(viewScope)));
  }, [pinFilterInteraction, updateView, viewScope]);

  const handleScrollBeginDrag = useCallback(() => {
    filterPinnedUntilRef.current = 0;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
  }, []);

  const handleListLayout = useCallback((event: LayoutChangeEvent) => {
    const height = Math.round(event.nativeEvent.layout.height);
    setListHeight((previous) => (previous === height ? previous : height));
  }, []);

  const handleListScroll = useCallback(
    (event: NativeSyntheticEvent<NativeScrollEvent>) => {
      const { contentOffset, contentSize, layoutMeasurement } = event.nativeEvent;
      const offset = contentOffset.y;
      const maxOffset = Math.max(
        0,
        contentSize.height - layoutMeasurement.height,
      );
      maxScrollOffsetRef.current = maxOffset;
      const outOfRange = offset < 0 || offset > maxOffset;
      // Bounce samples must not reach the saved offset, the visibility reducer
      // or the deliberate-scroll accumulator. Clamping the last sample also
      // makes the bounce return a zero delta rather than a reverse scroll.
      const visibleOffset = outOfRange
        ? Math.min(maxOffset, Math.max(0, offset))
        : offset;
      scrollOffsetRef.current = visibleOffset;
      writeScrollOffset(viewScope, visibleOffset);
      if (listModeRef.current === "tree" && Date.now() >= anchorFrozenUntilRef.current) {
        treeAnchorRef.current = Math.max(0, visibleOffset - (compact && !toolbarCollapsedRef.current ? compactToolbarHeight() : 0));
      }

      const suppressed = Date.now() < programmaticUntilRef.current;
      if (compact) {
        // Compact only closes on scroll; Search, Filters and Hide filters are
        // the manual controls. The raw offset counts so a short list closes too.
        lastScrollOffsetRef.current = visibleOffset;
        scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
        if (toolbarCollapsedRef.current) {
          return;
        }
        const decision = compactToolbarScroll({
          anchor: compactAnchorRef.current,
          offset,
          // React Native Web always reports a screen reader, so on web the
          // focus pin is what keeps the toolbar for keyboard users.
          pinned:
            searchFocusedRef.current ||
            toolbarControlFocusedRef.current ||
            (screenReaderRef.current && layout.platform !== "web") ||
            arranging ||
            Date.now() < filterPinnedUntilRef.current,
          suppressed,
          toolbarHeight: compactToolbarHeight(),
          maxOffset,
        });
        compactAnchorRef.current = decision.anchor;
        if (decision.collapse) {
          setToolbarVisibility("collapsed");
        }
        return;
      }
      if (suppressed || outOfRange) {
        scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
      } else {
        scrollAccumulatorRef.current = accumulateScrollDelta(
          scrollAccumulatorRef.current,
          visibleOffset - lastScrollOffsetRef.current,
        );
      }
      const current: ToolbarVisibility = toolbarCollapsedRef.current
        ? "collapsed"
        : "expanded";
      const next = reduceToolbarVisibility(current, {
        offset: visibleOffset,
        delta: scrollAccumulatorRef.current.distance,
        contentHeight: contentSize.height,
        viewportHeight: layoutMeasurement.height,
        headerHeight: toolbarCompensationHeight({
          fullHeight: expandedToolbarHeightRef.current,
          slimHeight: slimHeightRef.current,
          compact,
          gap: TOOLBAR_LAYOUT_GAP,
        }),
        pinned:
          searchFocusedRef.current ||
          toolbarControlFocusedRef.current ||
          screenReaderRef.current ||
          Date.now() < filterPinnedUntilRef.current,
        suppressed,
      });
      lastScrollOffsetRef.current = visibleOffset;
      if (next !== current) {
        setToolbarVisibility(next);
      }
    },
    [arranging, compact, compactToolbarHeight, layout.platform, viewScope, setToolbarVisibility],
  );

  const mappings = preferences.displayMappings;
  const favorites = preferences.favorites;
  const scoped = useMemo(
    () => state.entries.filter((entry) => (projectId === null || entry.projectKey === projectId) && (epicKey === null || inEpicScope(entry, mappings, epicKey))),
    [state.entries, projectId, epicKey, mappings],
  );
  const deskScopes = useMemo(() => deriveDeskScopes(scoped, workspaces,
    projects?.filter((project) => projectId === null || project.id === projectId)), [scoped, workspaces, projects, projectId]);
  // Retain legacy shortcuts; native ID choices distinguish same-named desks and survive renames.
  const scopeChoices = [...SCOPE_CHOICES.slice(0, -1), ...deskScopes, SCOPE_CHOICES[SCOPE_CHOICES.length - 1]!];
  const scopeChoiceLabel = shortcut === null ? null : scopeChoices.find(({ id }) => id === shortcut)?.label ?? (shortcut?.startsWith("desk-project:") ? "Native desk project" : shortcut);
  const tree = useMemo(() => buildOrganizationTree({ entries: scoped, ...preferences, grouping, workspaces }), [scoped, preferences, grouping, workspaces]);
  const tokens = useMemo(() => view.query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean), [view.query]);
  const task = view.task ?? null;
  const resultMode = isResultMode(view);
  const eligible = useCallback((entry: DirectoryEntry) => matchesShortcut(entry, shortcut, mappings)
    && (streams === null || streams.includes(workstreamKey(organizationScope(entry, mappings).workstream))), [shortcut, streams, mappings]);
  // Search and state never narrow the tree; with no workstream choice, empty workspaces stay visible, a desk context's own included.
  const filtered = useMemo(() => filterOrganizationTree(tree, eligible,
    (group) => streams === null && scopeKeepsGroup(group, shortcut, mappings), eligible), [tree, eligible, shortcut, streams, mappings]);
  const linked = useMemo(() => task ? new Set(taskSessions(scoped, task).map(({ entry }) => entry.id)) : null, [scoped, task]);
  // Matches before the state filter, so each state chip counts what choosing it would show.
  const candidates = useMemo(() => scoped.filter((entry) => eligible(entry) && (linked === null || linked.has(entry.id)) && matchesResultSearch(entry, tokens, mappings)),
    [scoped, eligible, linked, tokens, mappings]);
  const results = useMemo(() => resultMode ? candidates.filter((entry) => matchesStatus(entry, view.status, favorites)) : [], [resultMode, candidates, view.status, favorites]);
  const statusCounts = useMemo(() => Object.fromEntries([...QUICK_STATUS_FILTERS, ...MORE_STATUS_FILTERS].map((status) => [status, candidates.filter((entry) => matchesStatus(entry, status, favorites)).length])) as Record<StatusFilter, number>,
    [candidates, favorites]);
  const shortcutCounts = useMemo(() => SESSION_SHORTCUTS.map(({ id }) => scoped.filter((entry) => matchesShortcut(entry, id, mappings)).length), [scoped, mappings]);
  const streamChoices = useMemo(() => {
    const labels = new Map<string, string>();
    for (const entry of scoped) { const label = organizationScope(entry, mappings).workstream; labels.set(workstreamKey(label), label); }
    return [...labels].sort(([, left], [, right]) => workstreamRank(left) - workstreamRank(right) || left.localeCompare(right));
  }, [scoped, mappings]);
  const localExpansion = readLocalExpansion(host.id);
  const expanded = useMemo(() => ({ ...preferences.expandedGroups, ...localExpansion }), [preferences.expandedGroups, localExpansion]);
  const peek = resultMode ? view.peek ?? null : null;
  // Branches opened or closed while a result shows in the tree; never saved.
  const [peekToggles, setPeekToggles] = useState<Readonly<Record<string, boolean>>>({});
  const peekAncestors = useMemo(() => peek ? sessionAncestors(filtered, peek.sessionId) : null, [filtered, peek]);
  const treeExpanded = useMemo(() => peekAncestors ? { ...expanded, ...Object.fromEntries(peekAncestors.map((key) => [key, true])), ...peekToggles } : expanded, [expanded, peekAncestors, peekToggles]);
  const rows: readonly ListRow[] = useMemo(() => resultMode && !peek ? results.map((entry) => ({ kind: "result" as const, entry }))
    : flattenOrganizationTree(filtered, treeExpanded, false, defaultGroupExpanded), [resultMode, peek, results, filtered, treeExpanded]);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  const provenance = useMemo(() => new Map(results.map((entry) => [entry.id, sessionProvenance(entry, mappings)])), [results, mappings]);
  const rowSignature = useMemo(() => rows.map(listRowKey).join("\n"), [rows]);
  const previousRowSignatureRef = useRef(rowSignature);
  useLayoutEffect(() => {
    if (previousRowSignatureRef.current === rowSignature) {
      return;
    }
    previousRowSignatureRef.current = rowSignature;
    // Rows inserted above the viewport move the native offset
    // (maintainVisibleContentPosition). That is not a gesture: the next
    // compact sample re-anchors instead of closing the toolbar.
    if (compact) {
      compactAnchorRef.current = Number.NaN;
    }
  }, [compact, rowSignature]);
  const shown = useMemo(() => resultMode ? results : visibleSessions(filtered), [resultMode, results, filtered]);
  const workspaceCount = new Set(shown.map((entry) => entry.workspaceId ?? entry.workspaceName ?? entry.cwd)).size;
  // The full count and read statement; Filters shows it, the visible count line only when it adds something.
  const sessionCount = `${shown.length === scoped.length ? `${scoped.length}` : `${shown.length} of ${scoped.length}`} session${scoped.length === 1 ? "" : "s"}`;
  const inventoryRead = state.lastUpdatedAt === null
    ? "not read yet"
    : state.inventoryComplete
      ? `complete inventory read ${updatedLabel(state.lastUpdatedAt)}`
      : `partial inventory read ${updatedLabel(state.lastUpdatedAt)}`;
  const countSummary = `${sessionCount} · ${workspaceCount} workspace${workspaceCount === 1 ? "" : "s"} · ${inventoryRead}`;
  const setExpansion = (patch: Readonly<Record<string, boolean>>) => {
    const local = patchLocalExpansion(host.id, patch);
    setExpansionRevision((revision) => revision + 1);
    savePreference({ expandedGroups: { ...preferences.expandedGroups, ...local } });
  };
  const setAllGroups = (open: boolean) => setExpansion(Object.fromEntries(allGroupKeys(tree).map((key) => [key, open])));
  const toggleGroup = (row: ListRow) => {
    if (row.kind !== "group") return;
    if (peek) setPeekToggles((current) => ({ ...current, [row.group.key]: !row.expanded }));
    else setExpansion({ [row.group.key]: !row.expanded });
  };
  const chooseGrouping = (next: SessionGrouping) => {
    pinFilterInteraction();
    setDetailsId(null);
    updateView({ grouping: next, scrollOffset: 0 });
    if (next !== "placement") savePreference({ grouping: next });
  };
  const toggleStream = (key: string) => {
    const all = streamChoices.map(([choice]) => choice);
    const current = streams ?? all;
    const next = current.includes(key) ? current.filter((choice) => choice !== key) : [...current, key];
    pinFilterInteraction();
    updateView({ streams: all.every((choice) => next.includes(choice)) ? null : next, scrollOffset: 0 });
  };
  const moveRow = (row: OrganizationRow, distance: number) => {
    const parent = row.kind === "group" ? row.group.parentKey ?? "root" : row.parentKey;
    const peers = siblingKeys(tree, parent).filter((key) => (row.kind === "session") === key.startsWith("session:"));
    const movable = parent === "root" && row.kind === "group" ? peers.filter((key) => workstreamRank(tree.find((group) => group.key === key)?.label ?? "") === workstreamRank(row.group.label))
      // No role stays last.
      : parent === ROLE_ROOT_KEY && row.kind === "group" ? peers.filter((key) => (key === NO_ROLE_KEY) === (row.group.key === NO_ROLE_KEY)) : peers;
    savePreference({ siblingOrder: { ...preferences.siblingOrder, [parent]: reorderSavedSiblingOrder(preferences.siblingOrder[parent] ?? [], siblingKeys(tree, parent), movable, rowKey(row), distance) } });
  };
  const moveSelection = (distance: number) => {
    const row = rows.find((candidate) => listRowKey(candidate) === orderSelection);
    if (row && row.kind !== "result") moveRow(row, distance);
  };

  const narrowed = resultMode || shortcut !== null || streams !== null;
  const arrangeEnabled = arranging && !narrowed && !editsDisabled;
  const optionParts = [
    grouping !== "hierarchy" ? GROUPINGS.find((choice) => choice.id === grouping)?.label : null,
    streams !== null ? `${streams.length} workstream${streams.length === 1 ? "" : "s"}` : null,
    shortcut === "Desks" ? "Durable desks only" : null,
    view.status === "closed" ? "Closed sessions" : view.status === "initializing" ? "Starting sessions" : null,
    view.includeArchived ? "Including archived" : null,
  ].filter((part): part is string => !!part);
  const activeCount = sessionFilterCount(view);
  const clearable = activeCount > 0;
  const taskKeys = task ? [task.jira ? `${TASK_JIRA_LABELS.join(" or ")} equal to ${task.jira}` : null, task.kontorId ? `${TASK_TICKET_LABEL} equal to ${task.kontorId}` : null].filter(Boolean).join(", or ") : "";
  const summaryParts = activeFilterParts({ query: view.query, status: view.status, shortcut: shortcut?.startsWith("desk-project:") ? scopeChoiceLabel : shortcut, streams, includeArchived: view.includeArchived, task: task?.label });
  // The compact slim badge also counts the state and desk chips its closed toolbar hides; search has its own button.
  const slimActiveCount = activeCount;
  // While the compact toolbar is open, a short list still has room for the small drag that closes it.
  const scrollSlack = compact && !toolbarCollapsed && listHeight > 0 ? { minHeight: listHeight + COMPACT_SCROLL_SLACK } : {};
  // Compact draws the toolbar inside the list, so they share the list viewport; wide stacks them in the screen.
  const optionsSpace = compact ? listHeight : rootSize.height - 2 * WIDE_SCREEN_PADDING;
  const optionsMaxHeight = optionsSpace > 0 ? Math.max(OPTIONS_MIN_HEIGHT, Math.round(optionsSpace * OPTIONS_SPACE_SHARE) - optionsTop) : undefined;

  useEffect(() => {
    if (mobileFiltersOpen || restoredRef.current || state.entries.length === 0) {
      return;
    }
    restoredRef.current = true;
    const offset = readSessionView(viewScope).scrollOffset;
    if (offset <= 0) {
      return;
    }
    scrollOffsetRef.current = offset;
    lastScrollOffsetRef.current = offset;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    programmaticUntilRef.current = Date.now() + 300;
    const timer = setTimeout(() => {
      listRef.current?.scrollToOffset({ offset, animated: false });
    }, 0);
    return () => clearTimeout(timer);
  }, [viewScope, state.entries.length, mobileFiltersOpen]);

  const treeButtonsRef = useRef(new Map<string, View>());
  const treeButtonRef = useCallback((id: string, node: View | null) => {
    if (node) treeButtonsRef.current.set(id, node);
    else treeButtonsRef.current.delete(id);
  }, []);
  // One session's details at a time. The anchor reads the row's current button, so focus
  // returns to it even if the list drew the row again while the details were open.
  const [detailsId, setDetailsId] = useState<string | null>(null);
  const detailsIdRef = useRef(detailsId);
  detailsIdRef.current = detailsId;
  // The grouping and list mode the details opened in; another one draws the row elsewhere, or not at all.
  const detailsWhereRef = useRef("");
  const detailsButtonsRef = useRef(new Map<string, View>());
  const detailsButtonRef = useCallback((id: string, node: View | null) => {
    if (node) detailsButtonsRef.current.set(id, node);
    else detailsButtonsRef.current.delete(id);
  }, []);
  const detailsAnchor = useMemo<RefObject<View | null>>(() => ({
    get current() { return detailsIdRef.current === null ? null : detailsButtonsRef.current.get(detailsIdRef.current) ?? null; },
    set current(_node) {},
  }), []);
  const detailsCloseRef = useRef<View | null>(null);
  const [detailsLinesFocused, setDetailsLinesFocused] = useState(false);
  useEffect(() => {
    setDetailsLinesFocused(false);
    if (detailsId !== null) (detailsCloseRef.current as unknown as { focus?: () => void } | null)?.focus?.();
  }, [detailsId]);
  // A row that has left the list has no button to return to; the list takes focus instead, which does not scroll it.
  const detailsReturn = () => (detailsAnchor.current ?? listRef.current?.getScrollableNode()) as { focus?: () => void } | null | undefined;
  const peekBackRef = useRef<View | null>(null);
  const focusAfterModeRef = useRef<(() => void) | null>(null);
  const handleShowInTree = useCallback((entry: DirectoryEntry) => {
    setPeekToggles({});
    focusAfterModeRef.current = () => (peekBackRef.current as unknown as { focus?: () => void } | null)?.focus?.();
    const head = compact && !toolbarCollapsedRef.current ? compactToolbarHeight() : 0;
    updateView({ peek: { sessionId: entry.id, resultsOffset: Math.max(0, scrollOffsetRef.current - head) }, scrollOffset: 0 });
  }, [compact, compactToolbarHeight, updateView]);
  const handleBackToResults = useCallback(() => {
    const current = readSessionView(viewScope).peek;
    if (!current) return;
    focusAfterModeRef.current = () => (treeButtonsRef.current.get(current.sessionId) as unknown as { focus?: () => void } | undefined)?.focus?.();
    updateView({ peek: null, scrollOffset: current.resultsOffset });
  }, [updateView, viewScope]);

  // Each list mode keeps its place: results open at the top or where a peek left
  // them, a peek scrolls to its session, and the tree returns where it was.
  const listMode = listModeOf(view);
  useLayoutEffect(() => {
    const previous = listModeRef.current;
    if (previous === listMode) {
      return;
    }
    listModeRef.current = listMode;
    placementRef.current += 1;
    const stored = readSessionView(viewScope).scrollOffset;
    const head = compact && !toolbarCollapsedRef.current ? compactToolbarHeight() : 0;
    const offset = listMode === "results" && previous === "tree" ? 0 : stored > 0 ? stored + head : 0;
    const sessionId = peek?.sessionId ?? null;
    // Each pass finds the session's row again: a collapse or a live update can move or remove it first.
    const place = () => {
      const index = sessionId === null ? -1 : rowsRef.current.findIndex((row) => row.kind === "session" && row.entry.id === sessionId);
      if (index >= 0) listRef.current?.scrollToIndex({ index, viewPosition: 0.3, animated: false });
      return index >= 0;
    };
    scrollOffsetRef.current = offset;
    lastScrollOffsetRef.current = offset;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    compactAnchorRef.current = Number.NaN;
    programmaticUntilRef.current = Date.now() + 400;
    if (listMode === "tree") treeAnchorRef.current = Math.max(0, offset - head);
    writeScrollOffset(viewScope, offset);
    const focus = focusAfterModeRef.current;
    focusAfterModeRef.current = null;
    const timer = setTimeout(() => {
      if (!place()) listRef.current?.scrollToOffset({ offset, animated: false });
      focus?.();
    }, 0);
    // The first pass places the session by estimated row heights, which leave out a list header
    // such as open compact Filters; once the new rows are measured, place it again.
    const settle = sessionId !== null ? setTimeout(place, 150) : undefined;
    return () => { clearTimeout(timer); clearTimeout(settle); };
  }, [listMode]);

  const staleMessage = state.error ?? state.staleReason;
  // Ages tick only on a live list; otherwise rows give the time itself, and Details says as of when.
  const timesLive = nativeFreshnessLabel({ hostStatus: hostSummary?.status ?? "offline", status: state.status, live: state.live }) === "Live";
  const showBanner =
    state.status === "stale" ||
    state.status === "error" ||
    (state.status === "ready" && !state.live && state.staleReason !== null);
  const navigationMissing = navigation === undefined;

  const focusProps = { disabled: !compact && toolbarCollapsed, onBlur: handleToolbarControlBlur, onFocus: handleToolbarControlFocus };
  // Compact keeps source, navigation and settings warnings outside the
  // collapsible toolbar, so hiding filters never hides a problem.
  const noticeProps = compact ? {} : focusProps;
  const settingsUnavailable = organization !== undefined && organization.state.status !== "ready";
  const settingsAlert = saveMessage ?? organization?.state.saveError ?? null;
  const hasNotices = (showBanner && !!staleMessage) || navigationMissing || settingsUnavailable || !!settingsAlert;
  const notices = (
    <>
      {showBanner && staleMessage ? (
        <Banner
          testID="sessions-source-banner"
          message={`${staleMessage} ${
            state.inventoryComplete ? "Last complete read" : "Inventory read attempt"
          }: ${updatedLabel(state.lastUpdatedAt)}.`}
          tone={state.status === "error" ? "danger" : "warning"}
          actionLabel="Retry"
          onAction={refresh}
          colors={colors}
          {...noticeProps}
        />
      ) : null}

      {navigationMissing ? (
        <Banner
          testID="sessions-navigation-banner"
          message="This Paseo app does not expose native session navigation to plugins. Update Paseo to open sessions from this screen."
          tone="warning"
          colors={colors}
          {...noticeProps}
        />
      ) : null}

      {organization && organization.state.status !== "ready" ? <Text style={{ color: colors.statusWarning, fontSize: 12 }}>Organization settings {organization.state.status}; saved preferences are unavailable. Native sessions remain visible and branches still open and close here.</Text> : null}
      {saveMessage || organization?.state.saveError ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{saveMessage ?? organization?.state.saveError}</Text> : null}
    </>
  );
  const compactNotices = compact && hasNotices ? (
    <View testID="sessions-notices" style={{ gap: 8 }}>
      {notices}
    </View>
  ) : null;
  const OptionsContainer = compact ? View : ScrollView;
  const toolbar = toolbarLayout({ collapsed: toolbarCollapsed, compact });
  const fullToolbar = (
    <View
      testID="sessions-full-toolbar"
      onLayout={handleToolbarLayout}
      style={{ gap: 8 }}
    >
      {compact ? null : notices}
      <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
        {!toolbar.showSlimRow ? <Action ref={recoveryButton} small colors={colors} label="Recover" accessibilityLabel="Recover sessions" disabled={!hostOnline || lifecycle.running || lifecycle.busy || state.status !== "ready"}
          onPress={() => { lifecycle.clear(); lifecycleAnchor.current = recoveryButton.current; setLifecycleOpen(true); }} /> : null}
        {view.status === "attention" ? <>
          <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{availability.checking ? "Checking exact session IDs…" : "Attention excludes confirmed missing IDs. Unknown availability stays visible."}</Text>
          <Action small={!compact} colors={colors} label="Check availability" onPress={availability.refresh} />
        </> : null}
      </View>

      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        {scopeLabel ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 1 }}>
          <Text testID="sessions-heading" accessibilityRole="header" numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: compact ? 16 : 17, fontWeight: "600" }}>Sessions · {scopeLabel}</Text>
          {onClearScope ? <Action small={!compact} colors={colors} label="All sessions" accessibilityLabel={`Show all sessions instead of ${scopeLabel}`} disabled={!compact && toolbarCollapsed} onPress={onClearScope} /> : null}
        </View> : <Text testID="sessions-heading" accessibilityRole="header" style={{ color: colors.foreground, fontSize: compact ? 16 : 17, fontWeight: "600" }}>All sessions</Text>}
        <View accessibilityRole="toolbar" accessibilityLabel="Desk shortcuts" style={{ flexDirection: "row", flexWrap: "wrap", gap: 4, ...(compact ? { width: "100%" } : { marginLeft: "auto" }) }}>
          {SESSION_SHORTCUTS.map(({ id, label }, index) => <FilterChip key={label} testID={`sessions-shortcut-${index}`} label={label} count={shortcutCounts[index]} compact={compact} active={shortcut === id} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ shortcut: id, scrollOffset: 0 }); }} />)}
        </View>
      </View>

      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <TextInput
          ref={searchInputRef}
          testID="sessions-search"
          value={view.query}
          editable={compact || !toolbarCollapsed}
          onChangeText={(query) => updateFilter({ query })}
          onFocus={() => {
            searchFocusedRef.current = true;
          }}
          onBlur={() => {
            searchFocusedRef.current = false;
          }}
          placeholder="Search sessions, workstreams, epics, PRs…"
          placeholderTextColor={colors.foregroundMuted}
          autoCapitalize="none"
          autoCorrect={false}
          accessibilityLabel="Search sessions by topic, role or ticket"
          style={{
            flex: 1,
            minWidth: 0,
            minHeight: compact || COARSE_POINTER ? 44 : 36,
            borderRadius: 7,
            borderWidth: 1,
            borderColor: colors.border,
            backgroundColor: colors.surface1,
            color: colors.foreground,
            paddingHorizontal: 10,
            paddingVertical: 6,
            fontSize: compact ? 16 : 13,
          }}
        />
        {view.query.length > 0 ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Clear search"
            disabled={!compact && toolbarCollapsed}
            onPress={() => updateFilter({ query: "" })}
            style={{ minHeight: compact || COARSE_POINTER ? 44 : 32, justifyContent: "center", paddingHorizontal: 6 }}
          >
            <Text style={{ color: colors.accent, fontSize: 12, fontWeight: "600" }}>
              Clear
            </Text>
          </Pressable>
        ) : null}
        {compact ? null : <FiltersButton
          ref={filtersTriggerRef}
          activeCount={activeCount}
          expanded={view.advancedOpen}
          colors={colors}
          compact={compact}
          {...focusProps}
          onPress={handleAdvancedToggle}
        />}
      </View>

      {task ? (
        <Pressable
          testID="sessions-task-filter"
          accessibilityRole="button"
          accessibilityLabel={`Task ${task.label}: sessions with a label ${taskKeys}. Remove task filter`}
          {...focusProps}
          onPress={() => { pinFilterInteraction(); updateView({ task: null, scrollOffset: 0 }); }}
          style={{ alignSelf: "flex-start", maxWidth: "100%", minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 30, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 10, borderRadius: 15, borderWidth: 1, borderColor: colors.accent, backgroundColor: colors.surface2 }}
        >
          <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: 12, fontWeight: "600" }}>Task {task.label}</Text>
          <Text accessibilityElementsHidden importantForAccessibility="no" style={{ color: colors.foregroundMuted, fontSize: 12 }}>✕</Text>
        </Pressable>
      ) : null}

      <View accessibilityLabel="Frequent session state filters" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 4 }}>
        {QUICK_STATUS_FILTERS.map((status) => (
          <FilterChip
            key={status}
            testID={`sessions-filter-${status}`}
            label={statusFilterLabel(status)}
            count={statusCounts[status]}
            dot={STATUS_DOTS[status] ? sessionStateColor(STATUS_DOTS[status], colors) : undefined}
            compact={compact}
            active={view.status === status}
            colors={colors}
            {...focusProps}
            onPress={() => updateFilter({ status })}
          />
        ))}
          <ClearFiltersButton
            colors={colors}
            compact={compact}
            {...focusProps}
            disabled={focusProps.disabled || !clearable}
            onPress={handleClearFilters}
          />
      </View>
      {optionParts.length > 0 && !view.advancedOpen ? <Text testID="sessions-options-summary" numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{optionParts.join(" · ")}</Text> : null}

      {compact || view.advancedOpen ? (
        <OptionsContainer
          testID="sessions-options"
          accessibilityLabel="Filters and view options"
          nestedScrollEnabled
          keyboardShouldPersistTaps="handled"
          onLayout={(event: LayoutChangeEvent) => { const top = Math.round(event.nativeEvent.layout.y); if (top !== optionsTop) setOptionsTop(top); }}
          style={compact ? { gap: 12 } : {
            flexGrow: 0,
            maxHeight: optionsMaxHeight,
            borderColor: colors.border,
            borderWidth: 1,
            borderRadius: 8,
            backgroundColor: colors.surface1,
          }}
          {...(compact ? {} : { contentContainerStyle: { padding: 10, gap: 10 } })}
        >
          <OptionSelect testID="sessions-grouping" label="Group by" value={GROUPINGS.find(({ id }) => id === grouping)?.label ?? grouping} expanded={optionMenu === "grouping"} anchor={groupingRef} compact={compact} colors={colors} {...focusProps} onPress={() => setOptionMenu(optionMenu === "grouping" ? null : "grouping")} />
          <OptionLine label={streams === null ? `Workstream associations · all ${streamChoices.length} available` : `Workstream associations · ${streams.length} selected of ${streamChoices.length} available`} colors={colors}>
            <Text testID="sessions-workstream-availability" style={{ width: "100%", color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>
              Choices come from session metadata and display mappings, with desk and Other work fallbacks. Names may be unregistered. The full workstream registry and plan declarations are not loaded here; this list may be incomplete.
            </Text>
            {streamChoices.length === 0 ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>No workstream associations available in this context.</Text> : null}
            {streamChoices.map(([key, label]) => <FilterChip key={key} checkbox label={label} compact={compact} active={streams === null || streams.includes(key)} colors={colors} {...focusProps} onPress={() => toggleStream(key)} />)}
            {streams !== null ? <FilterChip label="All" compact={compact} active={false} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ streams: null, scrollOffset: 0 }); }} /> : null}
          </OptionLine>
          <OptionSelect testID="sessions-scope" label="Workspace scope" value={scopeChoiceLabel ?? SCOPE_CHOICES[0]!.label} expanded={optionMenu === "scope"} anchor={scopeRef} compact={compact} colors={colors} {...focusProps} onPress={() => setOptionMenu(optionMenu === "scope" ? null : "scope")} />
          <OptionLine label="Epic lifecycle" colors={colors}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Not captured in session data; no lifecycle is inferred from idle or closed sessions.</Text>
          </OptionLine>
          <OptionLine label="More states" colors={colors}>
            {MORE_STATUS_FILTERS.map((status) => <FilterChip key={status} testID={`sessions-filter-${status}`} label={statusFilterLabel(status)} count={statusCounts[status]} dot={sessionStateColor(STATUS_DOTS[status] ?? null, colors)} compact={compact} active={view.status === status} colors={colors} {...focusProps} onPress={() => updateFilter({ status })} />)}
          </OptionLine>
          <ToggleRow
            testID="sessions-advanced-archived"
            label="Include archived sessions"
            description="Archived is separate from the native closed state."
            checked={view.includeArchived}
            colors={colors}
            {...focusProps}
            onPress={() => updateFilter({ includeArchived: !view.includeArchived })}
          />
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <FilterChip testID="sessions-arrange" label={arranging ? "Done arranging" : "Arrange"} compact={compact} active={arranging} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || (!arranging && (narrowed || editsDisabled))} onPress={() => { pinFilterInteraction(); setOrderSelection(null); updateView({ arranging: !arranging }); }} />
            <View style={{ flex: 1 }} />
            <FilterChip testID="sessions-expand-all" label="Expand all" compact={compact} active={false} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || resultMode} onPress={() => setAllGroups(true)} />
            <FilterChip testID="sessions-collapse-all" label="Collapse all" compact={compact} active={false} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || resultMode} onPress={() => setAllGroups(false)} />
          </View>
          {arranging ? <View testID="sessions-arrange-help" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <Text style={{ flexBasis: "100%", color: colors.foregroundMuted, fontSize: 11 }}>{narrowed ? "Clear search and filters to arrange." : editsDisabled ? "Saved order is unavailable while organization settings are unavailable or saving." : "Select a handle, then Move up or Move down, or drag it among its siblings. Desks stay above epic work."}</Text>
            <Action small={!compact} colors={colors} label="↑ Move up" accessibilityLabel="Move selected row up" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(-1)} />
            <Action small={!compact} colors={colors} label="↓ Move down" accessibilityLabel="Move selected row down" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(1)} />
          </View> : null}
          <Text testID="sessions-options-count" style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 18 }}>{countSummary}{workspaceNote ? `. ${workspaceNote}.` : ""}</Text>
          <Disclosure small label="Legend" colors={colors}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 18 }}>
              Icons mark a workstream, epic, workspace or native project. {Object.entries(WORKSPACE_KINDS).map(([code, meaning]) => `${code} · ${meaning}`).join("  ")}. Role groups by the kontor.role or role label, else the title prefix before •; repeated title prefixes also group siblings in a workspace. Both are for display only and establish no authorized role.
            </Text>
          </Disclosure>
        </OptionsContainer>
      ) : null}
    </View>
  );

  // Group by and Workspace scope open as menus over the screen; a choice returns focus to its select.
  const optionAnchor = optionMenu === "grouping" ? groupingRef : scopeRef;
  const chooseOption = (choose: () => void) => {
    setOptionMenu(null);
    choose();
    (optionAnchor.current as unknown as { focus?: () => void } | null)?.focus?.();
  };
  const optionMenuLayer = optionMenu === null ? null : (
    <AnchoredMenu testID={`sessions-${optionMenu}-menu`} anchor={optionAnchor} root={rootRef} rootSize={rootSize} colors={colors}
      label={optionMenu === "grouping" ? "Group sessions by" : "Session workspace scope"} onClose={() => setOptionMenu(null)}
      placement={compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.6, 360) } : { align: "start", width: 240 }}>
      {/* The frame is clamped to the screen, so a short screen scrolls the choices instead of cutting off the last ones. */}
      <ScrollView testID={`sessions-${optionMenu}-choices`} keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
        {optionMenu === "grouping"
          ? GROUPINGS.map(({ id, label }) => <MenuItem key={id} label={label} tall={compact} selected={grouping === id} colors={colors} onPress={() => chooseOption(() => chooseGrouping(id))} />)
          : scopeChoices.map(({ id, label }) => <MenuItem key={id ?? "all"} label={label} detail={id?.startsWith("desk-project:") ? deskScopes.find((choice) => choice.id === id)?.projectId : undefined} tall={compact} selected={shortcut === id} colors={colors} onPress={() => chooseOption(() => { pinFilterInteraction(); updateView({ shortcut: id, scrollOffset: 0 }); })} />)}
      </ScrollView>
    </AnchoredMenu>
  );
  const detailsWhere = `${grouping}|${listMode}`;
  const openDetails = (id: string) => {
    detailsWhereRef.current = detailsWhere;
    setDetailsId(id);
  };
  const detailsEntry = detailsId === null || detailsWhereRef.current !== detailsWhere ? null : state.entries.find((entry) => entry.id === detailsId) ?? null;
  const closeDetails = () => {
    const target = detailsReturn();
    setDetailsId(null);
    target?.focus?.();
  };
  // Details whose session has left, or whose grouping or list mode has changed, close. Focus they held,
  // which the page has now dropped, goes where Close would send it; focus elsewhere stays put.
  useEffect(() => {
    if (detailsId === null || detailsEntry !== null) return;
    const document = pageDocument();
    const target = document && (document.activeElement == null || document.activeElement === document.body) ? detailsReturn() : null;
    setDetailsId(null);
    target?.focus?.();
  }, [detailsId, detailsEntry === null]);
  // Over the list on a compact screen, so a short one still shows the title, Close and a scrolling chain.
  const detailsLayer = detailsEntry === null ? null : (
    <AnchoredMenu testID="sessions-details" dialog anchor={detailsAnchor} root={rootRef} rootSize={rootSize} colors={colors}
      label={`Details: ${detailsEntry.title}`} onClose={closeDetails}
      placement={compact ? { align: "stretch", width: 0, cover: true } : { align: "end", width: 360, maxHeight: 440 }}
      header={<View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, paddingLeft: 6, paddingBottom: 4 }}>
        <Text testID="sessions-details-title" selectable style={{ flex: 1, minWidth: 0, paddingTop: 6, color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{detailsEntry.title}</Text>
        <Action ref={detailsCloseRef} small={!compact} colors={colors} label="Close" accessibilityLabel={`Close details: ${detailsEntry.title}`} onPress={closeDetails} />
      </View>}>
      {/* On web the lines are a named stop of their own, so a keyboard can scroll a chain longer than the screen. */}
      <ScrollView testID="sessions-details-lines" {...(pageDocument() ? { tabIndex: 0, role: "region", accessibilityLabel: "Times, role, placement, model and token report" } as const : {})}
        onFocus={() => setDetailsLinesFocused(true)} onBlur={() => setDetailsLinesFocused(false)}
        style={{ flexGrow: 0, flexShrink: 1, borderRadius: 6, ...(detailsLinesFocused ? { outlineColor: colors.accent, outlineStyle: "solid", outlineWidth: 2 } as const : null) }}>
        <SessionTimeLines entry={detailsEntry} freshness={{ live: timesLive, readAt: state.lastUpdatedAt }} colors={colors} />
        {sessionDetailLines(detailsEntry, mappings).map(({ label, value }) => (
          <View key={label} style={{ paddingHorizontal: 6, paddingVertical: 4 }}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
            <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value}</Text>
          </View>
        ))}
        <SessionUsageLines entry={detailsEntry} colors={colors} stale={showBanner} />
      </ScrollView>
    </AnchoredMenu>
  );
  const handleRootLayout = (event: LayoutChangeEvent) => {
    const { width, height } = event.nativeEvent.layout;
    if (width !== rootSize.width || height !== rootSize.height) setRootSize({ width, height });
  };

  const mobileToolbar = <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
    <View style={{ flex: 1, minWidth: 0 }}>
    <MobileQuickToolbar key={viewScope} colors={colors} width={Math.max(0, rootSize.width - 24 - 88)}
    query={view.query} status={view.status} counts={statusCounts} activeCount={slimActiveCount}
    summary={[scopeLabel, ...summaryParts, grouping !== "hierarchy" ? `Group: ${GROUPINGS.find((choice) => choice.id === grouping)?.label}` : null].filter(Boolean).join(" · ")}
    searchInputRef={searchInputRef} filtersTriggerRef={filtersTriggerRef} chooserRef={quickChooserRef} chooserOpen={quickChooserScope === viewScope} helpDismissRef={quickHelpDismissRef}
    onQuery={(query) => updateFilter({ query })} onStatus={(status) => updateFilter({ status })} onFilters={handleRevealFilters}
    onClear={handleClearFilters}
    onChooser={() => { searchInputRef.current?.blur(); setQuickChooserScope(viewScope); }} />
    </View>
    <View style={{ width: 80, alignItems: "flex-end" }}>
      <Action ref={slimRecoveryButton} colors={colors} label="Recover" accessibilityLabel="Recover sessions" disabled={!hostOnline || lifecycle.running || lifecycle.busy || state.status !== "ready"}
        onPress={() => { quickHelpDismissRef.current?.(); setQuickChooserScope(null); lifecycle.clear(); lifecycleAnchor.current = slimRecoveryButton.current; setLifecycleOpen(true); }} />
    </View>
  </View>;
  const quickChooserLayer = compact && !mobileFiltersOpen && quickChooserScope === viewScope ? <MobileQuickChooser
    root={rootRef} rootSize={rootSize} anchor={quickChooserRef} colors={colors} status={view.status} counts={statusCounts}
    onStatus={(status) => updateFilter({ status })} onClose={() => setQuickChooserScope(null)} /> : null;
  const slimSlot = compact ? mobileToolbar : (
    <View
      testID="sessions-slim-toolbar-slot"
      onLayout={(event) => {
        const height = event.nativeEvent.layout.height;
        if (height > 0) {
          slimHeightRef.current = height;
        }
      }}
      pointerEvents={toolbar.showSlimRow ? "auto" : "none"}
      accessibilityElementsHidden={!toolbar.showSlimRow}
      importantForAccessibility={
        toolbar.showSlimRow ? "auto" : "no-hide-descendants"
      }
      style={
        toolbar.showSlimRow
          ? compact
            ? { gap: 2 }
            : undefined
          : {
              position: "absolute",
              top: 0,
              left: 0,
              right: 0,
              opacity: 0,
            }
      }
    >
      <SlimToolbar
        action={toolbar.showSlimRow ? <Action ref={slimRecoveryButton} small colors={colors} label="Recover" accessibilityLabel="Recover sessions" disabled={!hostOnline || lifecycle.running || lifecycle.busy || state.status !== "ready"}
          onPress={() => { lifecycle.clear(); lifecycleAnchor.current = slimRecoveryButton.current; setLifecycleOpen(true); }} /> : undefined}
        hostLabel={host.label}
        activeCount={slimActiveCount}
        colors={colors}
        compact={compact}
        disabled={!toolbar.showSlimRow}
        open={compact && !toolbarCollapsed}
        onSearch={handleRevealSearch}
        onFilters={handleRevealFilters}
        onHide={compact ? handleHideToolbar : undefined}
      />
      {toolbar.showSlimRow ? (
        <ActiveFilterSummary
          parts={summaryParts}
          scopeLabel={scopeLabel}
          colors={colors}
          onClear={handleClearFilters}
          onClearScope={onClearScope}
        />
      ) : null}
    </View>
  );
  // Desktop keeps its existing inline toolbar; compact mounts these controls only on the filter page.
  const fullToolbarSlot = compact ? null : (
    <Animated.View
      testID="sessions-full-toolbar-slot"
      pointerEvents={toolbar.showFullToolbar ? "auto" : "none"}
      accessibilityElementsHidden={!toolbar.showFullToolbar}
      importantForAccessibility={
        toolbar.showFullToolbar ? "auto" : "no-hide-descendants"
      }
      style={[
        toolbar.showFullToolbar
          ? compact
            ? { paddingBottom: TOOLBAR_LAYOUT_GAP }
            : undefined
          : {
              position: "absolute",
              top: 0,
              left: 0,
              right: 0,
            },
        { opacity: toolbarOpacity },
      ]}
    >
      {fullToolbar}
    </Animated.View>
  );

  const closeMobileFilters = () => {
    setOptionMenu(null);
    setMobileFilterScope(null);
    restoredRef.current = false;
    returnFromFiltersRef.current = true;
  };
  useLayoutEffect(() => {
    if (mobileFiltersOpen || !returnFromFiltersRef.current) return;
    returnFromFiltersRef.current = false;
    const timer = setTimeout(() => { focusControl(filtersTriggerRef.current); quickHelpDismissRef.current?.(); }, 0);
    return () => clearTimeout(timer);
  }, [mobileFiltersOpen]);
  // An open page becomes the existing desktop controls on resize; selections already live in the scoped view.
  useLayoutEffect(() => {
    if (compact || mobileFilterScope === null) return;
    setMobileFilterScope(null);
    restoredRef.current = false;
    returnFromFiltersRef.current = true;
    const timer = setTimeout(() => { focusControl(filtersTriggerRef.current); quickHelpDismissRef.current?.(); }, 0);
    return () => clearTimeout(timer);
  }, [compact]);

  if (mobileFiltersOpen) return (
    <View ref={rootRef} onLayout={handleRootLayout} style={{ flex: 1, minHeight: 0, backgroundColor: colors.surface0 }}>
      <MobileFiltersPage colors={colors} onBack={closeMobileFilters} menuOpen={optionMenu !== null}>
        {compactNotices}
        <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>Changes apply immediately. Back keeps your choices.</Text>
        {fullToolbar}
        <Action colors={colors} label="Show results" accessibilityLabel="Show sessions with the current filters" onPress={closeMobileFilters} />
      </MobileFiltersPage>
      {optionMenuLayer}
    </View>
  );

  if (state.entries.length === 0 && (state.status === "loading" || state.status === "idle")) {
    return (
      <View
        ref={rootRef}
        onLayout={handleRootLayout}
        style={{
          flex: 1,
          backgroundColor: colors.surface0,
          padding: compact ? 16 : 20,
          gap: 12,
        }}
      >
        {slimSlot}
        {compactNotices}
        {fullToolbarSlot}
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 10 }}>
          <ActivityIndicator color={colors.foregroundMuted} />
          <Text style={{ color: colors.foregroundMuted, fontSize: 13 }}>
            Reading sessions from {host.label}…
          </Text>
        </View>
        {optionMenuLayer}
      </View>
    );
  }

  const extraData = { colors, arrangeEnabled, orderSelection, editsDisabled, favorites, peek, showBanner, timesLive };
  const emptyMessage = task && candidates.length === 0
    ? `No session in this view is linked to ${task.label}.`
    : clearable
      ? "No sessions match the current search and filters."
      : state.status === "error"
        ? "No sessions could be read from this host."
        : "No sessions in this host's scope.";
  // Shown only when narrowed or when the read is partial or missing; group rows already carry the
  // counts. The workspace count stays in the label and, with the complete read, in Filters.
  const countLine = state.entries.length === 0 || (shown.length === scoped.length && state.inventoryComplete && state.lastUpdatedAt !== null) ? null : (
    <Text testID="sessions-count" accessibilityRole="text" accessibilityLabel={countSummary} style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 4, paddingBottom: 8 }}>
      {sessionCount}{state.inventoryComplete && state.lastUpdatedAt !== null ? "" : ` · ${inventoryRead}`}
    </Text>
  );
  const resultWorkspaces = new Set(results.filter(inWorkspace).map((entry) => entry.workspaceId ?? entry.workspaceName)).size;
  const unplaced = results.filter((entry) => !inWorkspace(entry)).length;
  const resultLine = resultMode && results.length > 0 ? (
    <Text testID="sessions-count" accessibilityLiveRegion="polite" style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 4, paddingBottom: 8 }}>
      {`${results.length} matching session${results.length === 1 ? "" : "s"} · ${resultWorkspaces} workspace${resultWorkspaces === 1 ? "" : "s"}${unplaced ? ` · ${unplaced} without a workspace` : ""}`}
      {state.inventoryComplete && state.lastUpdatedAt !== null ? "" : ` · ${inventoryRead}`}
    </Text>
  ) : null;
  const peekTitle = peek ? state.entries.find((entry) => entry.id === peek.sessionId)?.title ?? "This session" : "";
  const shortPeek = peek !== null && compact && rootSize.height > 0 && rootSize.height < SHORT_PEEK_HEIGHT;
  const peekDescription = peekAncestors ? `Showing “${peekTitle}” in the tree. Branches opened here are not saved.` : `“${peekTitle}” is not in the tree under the current scope.`;
  let peekText = peekDescription;
  if (shortPeek) peekText = peekAncestors ? "In the tree · not saved" : "Not in this tree";
  else if (compact) peekText = peekAncestors ? "Shown in the tree. Branches opened here are not saved." : "Not in the tree under this scope.";
  const peekBar = peek ? (
    <View testID="sessions-peek" style={{ flexDirection: "row", flexWrap: compact ? "nowrap" : "wrap", alignItems: "center", gap: 8, paddingHorizontal: 4 }}>
      {/* Compact keeps the bar to one row beside Back; the highlighted row already names the session. */}
      <Text accessibilityLiveRegion="polite" numberOfLines={shortPeek ? 1 : compact ? 2 : undefined} accessibilityLabel={peekDescription}
        style={{ flex: 1, minWidth: compact ? 0 : 180, color: colors.foregroundMuted, fontSize: compact ? 11 : 12, lineHeight: compact ? 15 : 17 }}>
        {peekText}
      </Text>
      <Action ref={peekBackRef} small={!compact} colors={colors} label={`Back to ${results.length} result${results.length === 1 ? "" : "s"}`} onPress={handleBackToResults} />
    </View>
  ) : null;
  // A peek pins its way back above the list, so scrolling to the session never hides it.
  const listLine = peek ? null : resultMode ? resultLine : countLine;
  const listHeader = listLine;

  return (
    <View
      ref={rootRef}
      onLayout={handleRootLayout}
      style={{
        flex: 1,
        backgroundColor: colors.surface0,
        paddingTop: compact ? 12 : WIDE_SCREEN_PADDING,
        paddingHorizontal: compact ? 12 : 18,
        paddingBottom: compact ? 20 : WIDE_SCREEN_PADDING,
        gap: TOOLBAR_LAYOUT_GAP,
      }}
    >
      {/* A short-screen peek keeps only its Back row; Back brings the toolbar and filters back. */}
      {shortPeek ? null : slimSlot}
      {compactNotices}
      {compact ? null : fullToolbarSlot}
      {peekBar}
      <FlatList
        ref={listRef}
        testID="sessions-list"
        tabIndex={pageDocument() ? -1 : undefined}
        data={rows}
        keyExtractor={listRowKey}
        extraData={extraData}
        onScrollToIndexFailed={({ index, averageItemLength }) => {
          // Rows past the rendered window have no measured offset yet: land near it, then on it. The retry
          // belongs to the placement that asked for it, so Back, a new placement or a scope change drops it.
          // It follows its row, not the index: a collapse or a live update can move or remove the row first.
          const placement = placementRef.current;
          const target = rowsRef.current[index];
          listRef.current?.scrollToOffset({ offset: index * averageItemLength, animated: false });
          setTimeout(() => {
            const now = target && placementRef.current === placement ? rowsRef.current.findIndex((row) => listRowKey(row) === listRowKey(target)) : -1;
            if (now >= 0) listRef.current?.scrollToIndex({ index: now, viewPosition: 0.3, animated: false });
          }, 60);
        }}
        initialNumToRender={24}
        windowSize={11}
        onLayout={compact ? handleListLayout : undefined}
        onScroll={handleListScroll}
        onScrollBeginDrag={handleScrollBeginDrag}
        // Session taps remain usable while the compact search keyboard is open.
        keyboardShouldPersistTaps={compact ? "handled" : undefined}
        maintainVisibleContentPosition={{ minIndexForVisible: 0 }}
        scrollEventThrottle={16}
        contentContainerStyle={rows.length === 0 ? { flexGrow: 1, ...scrollSlack } : { paddingBottom: 16, ...scrollSlack }}
        ListHeaderComponent={listHeader}
        renderItem={({ item }) => {
          const details = item.kind === "result" || item.kind === "session"
            ? <SessionDetailsButton ref={(node) => detailsButtonRef(item.entry.id, node)} title={item.entry.title} expanded={detailsId === item.entry.id} compact={compact} colors={colors} onPress={() => openDetails(item.entry.id)} /> : undefined;
          const lifecycleAction = (item.kind === "result" || item.kind === "session") && item.entry.archivedAt === null ? <ArchiveSessionButton session={item.entry} compact={compact} colors={colors} disabled={!hostOnline || lifecycle.running || lifecycle.busy || state.status !== "ready"}
                buttonRef={(node) => { if (node) archiveButtons.current.set(item.entry.id, node); else archiveButtons.current.delete(item.entry.id); }}
                onPress={() => { lifecycle.clear(); lifecycleAnchor.current = archiveButtons.current.get(item.entry.id) ?? slimRecoveryButton.current; setLifecycleOpen(true); void lifecycle.prepare("archive", "filtered", [item.entry.id]); }} /> : null;
          if (item.kind === "result") return <SessionResultRow entry={item.entry} provenance={provenance.get(item.entry.id) ?? sessionProvenance(item.entry, mappings)} colors={colors} compact={compact} onPress={handleOpen} onShowInTree={handleShowInTree} treeButtonRef={treeButtonRef} live={timesLive} details={details} lifecycleAction={lifecycleAction} />;
          const key = listRowKey(item);
          if (item.kind === "empty") return <OrganizationRowView row={item} colors={colors} compact={compact} disabled arranging={false} selected={false} favorite={false} onFavorite={() => undefined} onToggle={() => undefined} onMove={() => undefined} onSelect={() => undefined} />;
          const epic = item.kind === "group" && item.group.kind === "epic" ? item.group.epic : null;
          const plans = epic ? planSources?.[epic.toLocaleUpperCase()] ?? [] : [];
          const actions = epic && onEpicTab && plans.length > 0 ? (["Plan", "Board", "Graph"] as const).map((tab) => <Action key={tab} small={!compact} colors={colors} label={tab}
            accessibilityLabel={`${tab} for ${epic}${plans.length > 1 ? `, choose among ${plans.length} configured plans` : ""}`} onPress={() => onEpicTab(tab, epic, plans.length === 1 ? plans[0]! : null)} />) : undefined;
          const role = item.kind === "session" && grouping === "role" ? roleRowContext(item.entry, mappings) : null;
          return <OrganizationRowView row={item} colors={colors} compact={compact} disabled={editsDisabled} arranging={arrangeEnabled} selected={orderSelection === key}
            details={details}
            lifecycleAction={lifecycleAction}
            favorite={item.kind === "session" && favorites.includes(item.entry.id)} stale={showBanner} actions={actions}
            onToggle={() => toggleGroup(item)}
            onSelect={() => setOrderSelection(orderSelection === key ? null : key)}
            onMove={(distance) => moveRow(item, distance)} onFavorite={() => { if (item.kind === "session") savePreference({ favorites: favorites.includes(item.entry.id) ? favorites.filter((id) => id !== item.entry.id) : [...favorites, item.entry.id] }); }}>
          {item.kind === "session" ? <SessionRow
            entry={item.entry}
            displayContext={role ? role.context
              : grouping === "flat" ? [item.entry.workspaceName ?? item.entry.cwd, item.entry.projectName !== item.entry.workspaceName ? item.entry.projectName : null].filter(Boolean).join(" · ") : undefined}
            compactContext={role?.compact}
            accessibilityContext={role?.spoken}
            displayTitle={role ? role.title : item.parentKey.includes("/role:") ? item.entry.title.slice(item.entry.title.indexOf("•") + 1).trim() : undefined}
            colors={colors}
            compact={compact}
            highlighted={peek?.sessionId === item.entry.id}
            live={timesLive}
            onPress={handleOpen}
          /> : null}
          </OrganizationRowView>;
        }}
        ListEmptyComponent={
          <View
            testID="sessions-empty"
            style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 20 }}
          >
            <Text style={{ color: colors.foreground, fontSize: 14, textAlign: "center" }}>{emptyMessage}</Text>
            {clearable ? (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Clear filters"
                onPress={handleClearFilters}
                style={{ minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 32, justifyContent: "center", paddingHorizontal: 8 }}
              >
                <Text style={{ color: colors.accent, fontSize: 13, fontWeight: "600" }}>
                  Clear filters
                </Text>
              </Pressable>
            ) : (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Retry reading sessions"
                onPress={refresh}
                style={{ paddingVertical: 6 }}
              >
                <Text style={{ color: colors.accent, fontSize: 13, fontWeight: "600" }}>
                  Retry
                </Text>
              </Pressable>
            )}
          </View>
        }
        ListFooterComponent={
          // Only a workspace list failure stays below the list; the routine read note is in Filters.
          state.entries.length === 0 || !workspaceNote?.startsWith(WORKSPACE_LIST_ERROR) ? null : (
            <Text
              testID="sessions-footer"
              style={{ color: colors.foregroundMuted, fontSize: 11, paddingVertical: 12, paddingHorizontal: 4 }}
            >
              {workspaceNote}.
            </Text>
          )
        }
      />
      {optionMenuLayer}
      {detailsLayer}
      {quickChooserLayer}
      {lifecycleOpen ? <SessionLifecycleDialog lifecycle={lifecycle} scopeIds={shown.map((entry) => entry.id)} root={rootRef} anchor={lifecycleAnchor} rootSize={rootSize} colors={colors} compact={compact}
        onClose={() => { setLifecycleOpen(false); const target = lifecycleAnchor.current; const kept = target === recoveryButton.current || target === slimRecoveryButton.current || [...archiveButtons.current.values()].includes(target!); const restore = target && kept ? target : recoveryButton.current ?? slimRecoveryButton.current ?? rootRef.current; lifecycleAnchor.current = restore; (restore as View & { focus?: () => void } | null)?.focus?.(); }} /> : null}
    </View>
  );
}
