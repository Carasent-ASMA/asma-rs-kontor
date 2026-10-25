import { sessionStateCounts, type SessionStateCount } from "./session-state-counts";
import { providerBlockedView, type ProviderRefusalReads } from "./provider-blocked-filter";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { SessionLayoutControl } from "./session-layout-control";
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
  Platform,
  Pressable,
  Settings,
  Text,
  View,
  type CellRendererProps,
  type FlatList as NativeFlatList,
  type LayoutChangeEvent,
  type NativeScrollEvent,
  type NativeSyntheticEvent,
  type ViewStyle,
} from "react-native";
import { openRecoveredSession, openSessionById } from "./session-actions";
import { type DirectoryEntry, type DirectoryState } from "./session-directory";
import {
  attentionLabel,
  archivedLabel,
  earlierErrorLabel,
  matchesQuery,
  matchesMembership,
  matchesStatus,
  readCoversMembership,
  MORE_STATUS_FILTERS,
  QUICK_STATUS_FILTERS,
  ALL_STATUS_FILTER,
  DEFAULT_SESSION_FILTERS,
  statusFilterLabel,
  sessionAvailabilityLabel,
  type SessionFilters,
  type StatusFilter,
} from "./session-view";
import {
  assignmentOf,
  configureWorkingAsStorage,
  isResultMode,
  sessionPresentation,
  clearSessionScopePatch,
  clearSessionFilterPatch,
  resetSessionViewPatch,
  sessionFilterCount,
  patchLocalExpansion,
  patchSessionView,
  readLocalExpansion,
  readRememberedWorkingAs,
  readSessionView,
  sessionViewTransition,
  workScopeOf,
  writeScrollOffset,
  writeWorkingAs,
  type AssignmentScope,
  type SessionGrouping,
  type SessionLayout,
  type SessionViewState,
  type WorkScope,
} from "./session-view-store";
import { deviceProfileStorage } from "./viewer-profile";
import { defaultSortDirection, explicitLeafOrder, sessionComparator, SESSION_SORTS, sortDirectionLabel, sortLabel, type SessionSort, type SortDirection } from "./session-sort";
import { indexResponsibility, MATCH_REASONS, responsibilityExpired, rosterChoices, sessionResponsibility, type MatchReason, type SessionResponsibility } from "./responsibility";
import { WorkstreamSelector } from "./workstream-selector";
import { SessionScopeSelector } from "./session-scope-selector";
import { SESSION_SHORTCUTS, SessionScopeChip } from "./session-scope-chip";
export { SESSION_SHORTCUTS } from "./session-scope-chip";
import { epicScopeChoice, epicScopeChoices, epicScopeKey, epicScopeLabel, matchesEpicScope } from "./session-scope";
import type { ResponsibilityRead } from "../shared/responsibility";
import { inWorkspace, matchesResultSearch, sessionAncestors, sessionProvenance, taskSessions, TASK_JIRA_LABELS, TASK_TICKET_LABEL, type SessionProvenance } from "./session-results";
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
import { allGroupKeys, buildOrganizationTree, declaredScope, defaultGroupExpanded, deriveDeskScopes, epicDetail, filterOrganizationTree, flattenOrganizationTree, inEpicScope, matchesScope, nativeDesk, NO_ROLE_KEY, organizationScope, organizationGroupRank, organizationWorkspaceCount, OTHER_WORKSTREAM, ROLE_ROOT_KEY, scopeKeepsGroup, sessionEpic, sessionRole, sessionState, STATE_LABELS, titleRoleCode, UNASSIGNED_EPIC, visibleSessions, workstreamRank, type OrganizationGroup, type OrganizationRow, type OrganizationWorkspace, type OrganizationProject, type RoleSource } from "./session-organization";
import { rowKey, siblingKeys, reorderSavedSiblingOrder } from "./organization-order";
import { OrganizationRowView, stateColor as sessionStateColor } from "./organization-row";
import { SessionStatusIcon } from "./session-state-marker";
import { CompactRetirementNotice, compactRetirementNotice, sessionRowDetails } from "./session-compact-content";
import { cancelDelayedOpen, DenseSessionRow, type DenseRowMode } from "./dense-session-row";
import { sessionWork, totalsDetail } from "./session-row-format";
import { copySessionId, SessionActionsMenu, type SessionAction } from "./session-actions-menu";
import { AnchoredMenu, MenuItem } from "./anchored-menu";
import { Action, COARSE_POINTER, Disclosure } from "./page-controls";
import { SessionsLegend } from "./sessions-legend";
import { nativeFreshnessLabel } from "./native-freshness";
import { readTime } from "./page-data";
import { sessionTimeLines, useClock, type TimeFreshness } from "./session-time";
import { derivedWorkstreamNote, planFallbackNote, planWorkstreamOptions, workstreamOptionLabel } from "./plan-workstreams";
import { relateSplitProjects } from "./split-project-rows";
import type { ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
import { useControlHelp } from "./control-help";
import { SessionUsageLines, SessionMetadataGuide } from "./session-usage-view";
import { SessionDetailSection } from "./session-detail-section";
import { MobileFiltersPage, focusControl } from "./mobile-filters";
import { CompactSearch, MobileQuickToolbar, StateRatio } from "./mobile-quick-toolbar";
import { useRecent } from "./recent-sessions";
import { RecentFooter, recentEmptyMessage } from "./recent-visits";
import { MobileQuickChooser } from "./mobile-quick-chooser";
import { useSessionLifecycle, type SessionRetirementRead } from "./use-session-lifecycle";
import { SessionRetirementConflict, SessionRetirementDetails, retirementDispositionLabel } from "./session-lifecycle-view";
import { ArchiveRowFeedback, archiveFeedbackShown, BulkArchiveDialog, BulkArchiveToolbar, useArchiveSelection } from "./bulk-archive";
import type { LeadSource } from "./new-lead-source";
import { useSessionOperations } from "./session-operations";
import { SelectionBar, SelectionHeader } from "./session-selection";
import { useUsageTotals } from "./use-navigator";
import { ARCHIVE_BATCH_LIMIT, freshAvailability, unresolvedArchive } from "../shared/session-lifecycle";
import { archiveAckKey, archiveNoticeKey, configureArchiveNoticeStorage, notifyArchive, recoveryLine as archiveSummary, useArchiveRecovery, type RecoveryItem } from "./archive-feedback";
import { ArchiveRecoverySheet } from "./archive-recovery";
import { hideNotices, NoticeAction, NoticesButton, noticesHidden, NoticeTray, presentHidden, showNotices, type Notice } from "./notice-tray";
import { useSessionAvailability } from "./use-session-availability";
import { readableInk } from "./session-status-ink";

type ThemeColors = PluginTheme["colors"];
/** A tree row, or a flat match while a search, state or task filter is on. */
type ListRow = OrganizationRow | { readonly kind: "result"; readonly entry: DirectoryEntry };
function listRowKey(row: ListRow): string {
  return row.kind === "result" ? `result:${row.entry.id}` : rowKey(row);
}

/** A row's height before any row has been measured at the list's width. */
const ESTIMATED_ROW_HEIGHT = 96;
/** A reader this close to an end of the list is at that end. */
const LIST_EDGE = 1;
/** Scroll events this close together are one gesture; the list holds no place for the reader during it. */
const SCROLL_SETTLE_MS = 150;

const isSession = (row: ListRow): row is Extract<ListRow, { readonly entry: DirectoryEntry }> => row.kind === "result" || row.kind === "session";

/** Where the reader is: at the top, at the end, or at a row whose top is this far down. */
type ListPlace = { readonly at: "top" } | { readonly at: "end" } | { readonly at: "row"; readonly key: string; readonly top: number };

/**
 * The list's row layout from heights measured per row, not per index, and the reader's place in it. Left to
 * itself the list keeps measured heights by index, so a live re-sort turns every unrendered row it shifts into
 * an estimate and moves the rows in view. A row not yet measured at the current width counts as the average of
 * those that are. Each place change says how far the offset moves to keep the reader where they were: at the
 * top, at the end, or at the same distance from the exact first visible session while it remains in this view.
 */
export function createRowLayout(resizedCallback: (by?: number, fromOffset?: number) => void = () => {}) {
  const measured = new Map<string, { readonly height: number; readonly width: number }>();
  const mounted = new Map<string, View>();
  const cellRefs = new Map<string, (node: View | null) => void>();
  let readCapture: (() => void) | null = null;
  let applyCapture: (() => void) | null = null;
  let captureVersion = 0, capturePending = false;
  let renderedRows: readonly ListRow[] | null = null;
  let following: { readonly rows: readonly ListRow[]; readonly withHeader: boolean } | null = null;
  const cellRef = (key: string) => {
    let ref = cellRefs.get(key);
    if (!ref) { ref = (node) => { if (node) { mounted.set(key, node); readCapture?.(); } else mounted.delete(key); }; cellRefs.set(key, ref); }
    return ref;
  };
  let width = 0, header = 0, version = 0;
  type Table = { readonly rows: readonly ListRow[]; readonly version: number; readonly keys: readonly string[]; readonly index: ReadonlyMap<string, number>;
    readonly lengths: readonly number[]; readonly offsets: readonly number[]; readonly length: number };
  let built: Table | null = null;
  let place: ListPlace = { at: "top" };
  const measure = (key: string, layout: { readonly width: number; readonly height: number; readonly y: number }) => {
    const entry = measured.get(key);
    const resized = entry?.width !== layout.width || Math.abs(entry.height - layout.height) >= 0.5;
    if (!resized) return;
    measured.set(key, { height: layout.height, width: layout.width });
    width = layout.width;
    if (resized) version += 1;
    resizedCallback();
  };
  const build = (rows: readonly ListRow[]): Table => {
    if (built?.rows === rows && built.version === version) return built;
    let total = 0, count = 0;
    for (const entry of measured.values()) if (entry.width === width) { total += entry.height; count += 1; }
    const estimate = count ? total / count : ESTIMATED_ROW_HEIGHT;
    const keys = rows.map(listRowKey);
    const lengths = keys.map((key) => { const entry = measured.get(key); return entry?.width === width ? entry.height : estimate; });
    const offsets: number[] = [];
    const length = lengths.reduce((offset, size) => { offsets.push(offset); return offset + size; }, 0);
    return (built = { rows, version, keys, index: new Map(keys.map((key, index) => [key, index])), lengths, offsets, length });
  };
  const head = (withHeader: boolean) => withHeader ? header : 0;
  /** The first row in view at this content offset: more of it shows than an edge. */
  const firstShown = (table: Table, withHeader: boolean, offset: number) => {
    let low = 0, high = table.keys.length;
    while (low < high) { const middle = (low + high) >> 1; if (head(withHeader) + table.offsets[middle]! + table.lengths[middle]! <= offset + LIST_EDGE) low = middle + 1; else high = middle; }
    return low;
  };
  /** The first session in view at this offset that this keeps, if one shows. */
  const sessionAt = (rows: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, keeps = (_row: ListRow, _key: string) => true): ListPlace | null => {
    const table = build(rows);
    for (let index = firstShown(table, withHeader, offset); index < rows.length && head(withHeader) + table.offsets[index]! < offset + viewport; index += 1) {
      if (isSession(rows[index]!) && keeps(rows[index]!, table.keys[index]!)) return { at: "row", key: table.keys[index]!, top: head(withHeader) + table.offsets[index]! };
    }
    return null;
  };
  const placeAt = (rows: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, content: number): ListPlace => {
    const table = build(rows);
    const first = firstShown(table, withHeader, offset);
    // At both ends of a list too short to scroll, the top wins; an empty list has only its top. In between, the
    // first session in view holds the place; a group heading only when no session shows. The rows say where the
    // end is too: the list reports a new content height after the screen has already placed its offset in it.
    return offset <= LIST_EDGE || !table.keys.length ? { at: "top" }
      : offset >= Math.max(content, head(withHeader) + table.length) - viewport - LIST_EDGE || first >= table.keys.length ? { at: "end" }
      : sessionAt(rows, withHeader, offset, viewport) ?? { at: "row", key: table.keys[first]!, top: head(withHeader) + table.offsets[first]! };
  };
  const followPlace = (previous: readonly ListRow[], next: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, content: number, was: ListPlace,
    geometry?: { readonly table: Table; readonly header: number }): number => {
    const before = geometry?.table ?? build(previous), after = build(next);
    const beforeHead = geometry?.header ?? head(withHeader);
    if (was.at !== "row") {
      place = was;
      return was.at === "top" ? 0 : after.length - before.length;
    }
    // Reordering does not release an exact native ID. Only a row leaving this view permits fallback.
    if (after.index.has(was.key)) {
      const index = after.index.get(was.key)!;
      const top = before.index.get(was.key) === index
        ? was.top + head(withHeader) - beforeHead + after.offsets[index]! - before.offsets[index]!
        : head(withHeader) + after.offsets[index]!;
      place = { ...was, top };
      return top - was.top;
    }
    const visible: number[] = [];
    for (let index = firstShown(before, withHeader, offset + head(withHeader) - beforeHead); index < before.keys.length && beforeHead + before.offsets[index]! < offset + viewport; index += 1) {
      if (after.index.has(before.keys[index]!)) visible.push(index);
    }
    const index = visible.find((index) => isSession(previous[index]!)) ?? visible[0];
    if (index === undefined) {
      place = placeAt(next, withHeader, offset, viewport, content);
      return 0;
    }
    const key = before.keys[index]!;
    const top = head(withHeader) + after.offsets[after.index.get(key)!]!;
    const by = top - (beforeHead + before.offsets[index]!);
    // Only a missing anchor permits a new first visible session, including one its removal reveals.
    place = sessionAt(next, withHeader, offset + by, viewport) ?? { at: "row", key, top };
    return by;
  };
  const layout = {
    // Native measure replies can outlive the screen that requested them.
    cancelMeasurements() {
      captureVersion += 1;
      readCapture = null;
      applyCapture = null;
      capturePending = false;
      following = null;
    },
    // Stop reissuing an old capture against post-update cells before the parent's layout effect runs.
    renderRows(rows: readonly ListRow[]) { renderedRows = rows; },
    /** Cells measure themselves: with getItemLayout the list measures none. */
    Cell({ cellKey: _cellKey, index: _index, item, children, ...props }: CellRendererProps<ListRow>) {
      const key = listRowKey(item);
      return <View {...props} ref={cellRef(key)} collapsable={false} onLayout={(event) => { measure(key, event.nativeEvent.layout); props.onLayout?.(event); }}>{children}</View>;
    },
    onHeaderLayout(event: LayoutChangeEvent) {
      if (Math.abs(event.nativeEvent.layout.height - header) < 0.5) return;
      header = event.nativeEvent.layout.height;
      resizedCallback();
    },
    layout(rows: readonly ListRow[], index: number, withHeader: boolean) {
      const { lengths, offsets } = build(rows);
      return { length: lengths[index]!, offset: head(withHeader) + offsets[index]!, index };
    },
    /** Back must restore an exact row, not a pixel estimate over rows never measured. */
    bookmark(rows: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, content: number) {
      readCapture = null;
      applyCapture = null;
      capturePending = false;
      following = null;
      return { place, offset };
    },
    restore(saved: { readonly place: ListPlace; readonly offset: number }, rows: readonly ListRow[], withHeader: boolean, viewport: number) {
      readCapture = null;
      applyCapture = null;
      capturePending = false;
      following = null;
      const table = build(rows), content = head(withHeader) + table.length;
      const previous = saved.place;
      let target = saved.offset;
      if (previous.at === "top") target = 0;
      else if (previous.at === "end") target = content - viewport;
      else if (table.index.has(previous.key)) {
        const top = head(withHeader) + table.offsets[table.index.get(previous.key)!]!;
        target += top - previous.top;
        place = { ...previous, top };
      }
      target = Math.max(0, Math.min(Math.max(0, content - viewport), target));
      if (previous.at !== "row" || !table.index.has(previous.key)) place = placeAt(rows, withHeader, target, viewport, content);
      return target;
    },
    /** Notes where the reader is. */
    capture(rows: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, content: number) {
      place = placeAt(rows, withHeader, offset, viewport, content);
      readCapture = null;
      applyCapture = null;
      capturePending = false;
      following = null;
      if (place.at !== "row") return;
      const table = build(rows);
      // A virtualized window can mount after the scroll event. Read its current public View geometry,
      // including late cells; onLayout size events do not report every spacer/reorder movement.
      const read = () => {
        if (following || (renderedRows !== null && renderedRows !== rows)) return;
        const version = ++captureVersion;
        const geometry = { table: build(rows), header: head(withHeader) };
        const cells = [...mounted].filter(([key, node]) => table.index.has(key) && typeof node.measure === "function");
        let first: { readonly key: string; readonly top: number } | null = null;
        let remaining = cells.length;
        capturePending = remaining > 0;
        applyCapture = remaining ? () => {
          if (!first) return;
          // Layout can change without a row update. Apply its delta from the request's baseline too.
          const by = followPlace(rows, following?.rows ?? rows, following?.withHeader ?? withHeader, offset, viewport, content, { at: "row", ...first }, geometry);
          if (following || Math.abs(by) >= 0.5) resizedCallback(by, offset);
        } : null;
        for (const [key, node] of cells) node.measure((_x, y, _width, height) => {
          if (readCapture !== read || captureVersion !== version) return;
          const row = rows[table.index.get(key)!]!;
          if (isSession(row) && y + height > offset + LIST_EDGE && y < offset + viewport && (!first || y < first.top)) {
            first = { key, top: y };
            // Detached offscreen cells may never reply. Visible evidence need not wait for them;
            // an earlier visible reply can still refine this same request until reader intent replaces it.
            capturePending = false;
            place = { at: "row", ...first };
            applyCapture?.();
          }
          if (--remaining === 0) {
            capturePending = false;
            applyCapture = null;
            if (following) readCapture = null;
            following = null;
          }
        });
      };
      readCapture = read;
      read();
    },
    /**
     * Keep the exact first visible session through reorders and resizes. Only a missing session passes its place
     * to a surviving visible session; with none, keep the bounded offset.
     */
    follow(previous: readonly ListRow[], next: readonly ListRow[], withHeader: boolean, offset: number, viewport: number, content: number, current = false): number {
      if (current && applyCapture !== null) {
        following = { rows: next, withHeader };
        applyCapture();
        return 0; // Retain this request's geometry, including any earlier visible reply still queued.
      }
      readCapture = null;
      applyCapture = null;
      capturePending = false;
      following = null;
      return followPlace(previous, next, withHeader, offset, viewport, content, current ? place : placeAt(previous, withHeader, offset, viewport, content));
    },
    /**
     * How far the offset moves to keep the reader's place after a row or the header changed height. Not the end's:
     * a row the list already shows can be measured after the reader reached the end, so the end is the list's own.
     */
    restate(rows: readonly ListRow[], withHeader: boolean): number {
      if (capturePending) return 0;
      if (place.at !== "row") return 0;
      const table = build(rows);
      if (!table.index.has(place.key)) return 0;
      const index = table.index.get(place.key)!;
      const node = mounted.get(place.key);
      if (typeof node?.measure === "function") {
        const held = place;
        const requestVersion = captureVersion;
        node.measure((_x, y) => {
          if (captureVersion !== requestVersion || place !== held || !Number.isFinite(y)) return;
          const by = y - held.top;
          if (Math.abs(by) < 0.5) return;
          place = { ...held, top: y };
          resizedCallback(by);
        });
        return 0;
      }
      const now = head(withHeader) + table.offsets[index]!;
      const by = now - place.top;
      place = { ...place, top: now };
      return by;
    },
    atEnd() {
      return place.at === "end";
    },
  };
  return layout;
}

// Working as is remembered in this device's store where the runtime offers one, before the first read; otherwise for this app session.
configureWorkingAsStorage(deviceProfileStorage(() => ({ platform: Platform.OS, settings: Settings })));
configureArchiveNoticeStorage(deviceProfileStorage(() => ({ platform: Platform.OS, settings: Settings })));

/** Status dot colour of a session: its highest-priority state, as on group badges. */
export function indicatorColor(entry: DirectoryEntry, colors: ThemeColors): string {
  return sessionStateColor(sessionState(entry), colors);
}

/** Workspace scope, a menu in Filters & view; the shortcuts above repeat all but Durable desks. */
const SCOPE_CHOICES: readonly { readonly id: string | null; readonly label: string }[] = [
  { id: null, label: "All Paseo workspaces" },
  { id: "Desks", label: "Durable desks" },
  { id: "PR Review", label: "PR Review desk" },
  { id: "Adam", label: "Adam desk" },
  { id: "Tickets", label: "Tickets desk" },
  { id: "Other work", label: "Outside desks" },
];
const GROUPINGS: readonly { readonly id: SessionGrouping; readonly label: string }[] = [
  { id: "hierarchy", label: "Desks & workstreams" },
  { id: "placement", label: "Desks & workspaces" },
  { id: "role", label: "Role" },
];

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
/** What the toolbar's actions entry acts on: the sessions shown, never one row. */
const TOOLBAR_SUBJECT = { id: "toolbar", title: "the sessions shown" };
/** No list-wide refusal source is admitted yet (ASMA-8648: host list field or bounded native-error reads), so every
 *  native error shows as not checked. Replace with that source; never with status, attention, age or titles. */
const NO_REFUSAL_READS: ProviderRefusalReads = new Map();
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
  readonly epic?: string | null;
  /** The responsibility scope's name, absent for All work. */
  readonly work?: string | null;
}): string[] {
  const query = input.query.trim();
  return [
    input.work ?? null,
    input.task ? `Task ${input.task}` : null,
    input.epic ? `Epic ${input.epic}` : null,
    query ? `“${query}”` : null,
    input.status !== "all" ? statusFilterLabel(input.status) : null,
    input.shortcut !== null ? shortcutLabel(input.shortcut) : null,
    input.streams !== null ? `${input.streams.length} workstream${input.streams.length === 1 ? "" : "s"}` : null,
    input.includeArchived ? "Archived history" : null,
  ].filter((part): part is string => part !== null);
}

/** A session's relation to the scope's person, as Details states it; unknown is never "someone else". */
export function responsibilityLine(result: SessionResponsibility): string {
  if (result.status === "other") return "Not this person's: every relation was read and none names them.";
  if (result.status === "unknown") return `Not evaluated: ${result.unknown.join("; ")}.`;
  return `${result.reasons.map((reason) => MATCH_REASONS[reason]).join(", ")}${result.unknown.length ? `. Other responsibility unknown: ${result.unknown.join("; ")}` : ""}.`;
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
function listModeOf(view: SessionViewState, preferences: OrganizationSettings = DEFAULT_ORGANIZATION_SETTINGS): string {
  return !isResultMode(view, preferences) ? "tree" : view.peek ? `peek:${view.peek.sessionId}` : "results";
}

function updatedLabel(timestamp: number | null): string {
  if (timestamp === null) {
    return "not read yet";
  }
  return new Date(timestamp).toLocaleTimeString();
}

/**
 * The Filters header count of final matching sessions. Exact only from a ready, live and complete read that
 * covers the membership in view; an archive-inclusive read covers ordinary membership, while an ordinary
 * read does not cover Archived. A read in progress never shows as an empty 0.
 */
export function matchingCount({ count, state, includeArchived, freshness, blocked }: {
  count: number; state: DirectoryState; includeArchived: boolean; freshness: string; blocked: string | null;
}): { text: string; label: string } {
  if (blocked !== null) return { text: "Not evaluated", label: `Matching sessions not evaluated: ${blocked} is not evaluated` };
  if (!readCoversMembership(state, includeArchived) || state.lastUpdatedAt === null) {
    return state.status === "error" && readCoversMembership(state, includeArchived)
      ? { text: "—", label: "Matching sessions unavailable: the session list could not be read" }
      : { text: "…", label: "Reading matching sessions" };
  }
  const sessions = `${count} matching session${count === 1 ? "" : "s"}`;
  if (freshness === "Live") {
    return state.inventoryComplete === true
      ? { text: sessions, label: `${count} matching session${count === 1 ? "" : "s"}` }
      : { text: `${sessions} · partial`, label: `${sessions} match a partial read; the count may be incomplete` };
  }
  if (count === 0) return { text: "…", label: `Matching sessions updating: ${freshness}` };
  return { text: `${sessions} · ${freshness.toLocaleLowerCase()}`, label: `${sessions} matched the read at ${updatedLabel(state.lastUpdatedAt)}; ${freshness}` };
}

/**
 * The native runtime where the state badge reports something else, such as an error reported earlier on a session
 * that is idle now. A row speaks it; Details keeps it.
 */
export function resultCues(entry: DirectoryEntry): string[] {
  const state = sessionState(entry);
  const derived = (state === "needs_input" || state === "attention" || state === "failed") && entry.status !== "error";
  const cue = earlierErrorLabel(entry) ?? (derived ? `Now ${entry.status}` : null);
  return cue ? [cue] : [];
}

function spokenProvenance(provenance: SessionProvenance): string {
  const place = provenance.desk ? `Desk ${provenance.parts.join(", workspace ")}` : `Workstream ${provenance.parts[0]}, epic ${provenance.parts[1]}, workspace ${provenance.parts[2]}`;
  return provenance.association ? `${place}. ${provenance.association}` : place;
}

/**
 * What a Sessions row adds to the shared dense row, which already shows the title, state, origin with its task and
 * PR, model and times. The note line is kept for exceptions to act on: role declarations that disagree, archive
 * membership, and a native record found absent or already archived. Routine facts, such as the native runtime behind
 * a derived state or unverified provider history, are spoken with the exact native ID and the whole place, and
 * Details keeps every value.
 */
export function sessionRowExtras(entry: DirectoryEntry, place: string): { note: string | null; spoken: string } {
  const evidence = entry.requiresAttention ? freshAvailability(entry) : null;
  const availability = sessionAvailabilityLabel(entry), exception = evidence?.state === "missing" || !!evidence?.archivedAt;
  const note = [...roleConflicts(entry), archivedLabel(entry), exception ? availability : null].filter((part): part is string => !!part);
  const attention = entry.needsInput ? null : attentionLabel(entry);
  return { note: note.length ? note.join(" · ") : null,
    spoken: [`Native ID ${entry.id}`, `Native status ${entry.status}`, attention ? `Needs attention: ${attention}` : null, ...resultCues(entry), exception ? null : availability, place].filter(Boolean).join(". ") };
}

const ROLE_SOURCE_NAMES: Readonly<Record<RoleSource, string>> = { "kontor.role": "kontor.role label", role: "role label", title: "title" };
/** Each role declaration that disagrees with the role the session is shown under, as "role label says TPM". */
const roleConflicts = (entry: DirectoryEntry) => sessionRole(entry).conflicts.map(({ source, code }) => `${ROLE_SOURCE_NAMES[source]} says ${code}`);

/**
 * A Role view row: a source that names another role comes first as a cue, then where the session
 * sits. The title loses its prefix only when the prefix names the role it is shown under.
 */
export function roleRowContext(entry: DirectoryEntry, mappings: Parameters<typeof organizationScope>[1]) {
  const role = sessionRole(entry);
  const cues = roleConflicts(entry);
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
 * Web only: the browser's scroll anchoring keeps the first visible row in place, so a live update that
 * re-sorts that row to the top carries the list back to the start. Native gets no anchoring prop either.
 */
const NO_SCROLL_ANCHOR: ViewStyle & { readonly overflowAnchor: "none" } = { overflowAnchor: "none" };

/**
 * Everything a Role row shortens, for its details: the role and where it came from, every other
 * declaration, and the whole desk or workstream, epic and workspace chain, with absent values named.
 * A workstream may come from a native desk project when no display mapping or label names it. A session
 * nothing links also says why no plan links it and where an explicit link is made; no name is read.
 */
export function sessionDetailLines(entry: DirectoryEntry, mappings: Parameters<typeof organizationScope>[1], planNote: string | null = null, sourceNote: string | null = null): { label: string; value: string }[] {
  const source = sourceNote ? [{ label: "Workstream source", value: sourceNote }] : [];
  const role = sessionRole(entry);
  const provenance = sessionProvenance(entry, mappings);
  const lines = [
    { label: "Role", value: role.code === null ? "No role: no kontor.role or role label, and no role prefix in the title" : `${role.code}, from the ${ROLE_SOURCE_NAMES[role.source!]}` },
    { label: "Other role declarations", value: roleConflicts(entry).join("; ") || "None" },
  ];
  if (provenance.nativeDesk) {
    const declared = declaredScope(entry, mappings);
    return [...lines,
      { label: "Native desk", value: provenance.nativeDesk.name },
      { label: "Native project ID", value: provenance.nativeDesk.id },
      { label: "Workspace", value: provenance.workspace },
      { label: "Native workspace ID", value: provenance.nativeDesk.workspaceId ?? "Unavailable" },
      { label: "Workstream association", value: declared.workstream ?? "Not declared" },
      ...source,
      { label: "Epic association", value: epicDetail(sessionEpic(entry, mappings)) },
      { label: "Association verification", value: "Workstream registry membership and epic/plan links are not checked here." }];
  }
  if (provenance.desk) return [...lines, { label: "Desk", value: provenance.parts[0]! }, { label: "Workspace", value: provenance.workspace }];
  const declared = declaredScope(entry, mappings);
  const unlinked = declared.workstream === null && nativeDesk(entry.projectName) === null;
  return [...lines,
    { label: "Workstream", value: declared.workstream ?? (nativeDesk(entry.projectName) !== null ? `${organizationScope(entry, mappings).workstream} (native desk project)` : `${OTHER_WORKSTREAM}: no workstream or desk label and no display mapping`) },
    ...source,
    { label: "Epic", value: epicDetail(sessionEpic(entry, mappings)) },
    ...(unlinked ? [{ label: "Linking", value: `${planNote ? `${planNote} ` : ""}Settings, under Presentation and associations, can map this session or link it to a plan. Project and session names never link it.` }] : []),
    { label: "Workspace", value: provenance.workspace }];
}

/** Opens session details beside the row's own session-navigation button. */
export function SessionDetailsButton({ title, expanded, compact, colors, onPress, ref }: {
  readonly title: string; readonly expanded: boolean; readonly compact: boolean; readonly colors: ThemeColors; readonly onPress: () => void; readonly ref?: Ref<View>;
}) {
  const help = useControlHelp("Show model, reported usage, timing and placement.", ref);
  const size = compact || COARSE_POINTER ? 44 : 30;
  return (
    <Pressable {...help.bindings} testID="sessions-row-details" accessibilityRole="button" accessibilityLabel={`Details: ${title}`} accessibilityHint="Shows session facts and optional explanations"
      accessibilityState={{ expanded }} aria-expanded={expanded} onPress={() => { help.dismiss(); onPress(); }}
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
          <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value.split("\n")[0]}</Text>
          {value.includes("\n") ? <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17, marginTop: 3 }}>{value.slice(value.indexOf("\n") + 1)}</Text> : null}
        </View>
      ))}
    </View>
  );
}

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
          color: readableInk(colors.accent, colors, 4.5),
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
  notice,
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
  /** The status button a hidden notice tray leaves in the toolbar. */
  readonly notice?: ReactNode;
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
      {notice}
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
  status,
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
  count?: number | SessionStateCount;
  /** Same shape as session rows; the existing label supplies its meaning. */
  status?: StatusFilter;
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
      accessibilityLabel={count === undefined ? label : `${label}, ${typeof count === "number" ? count : count.label}`}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: COARSE_POINTER ? FILTERS_TOUCH_TARGET : compact ? 36 : 30,
        maxWidth: checkbox ? "100%" : undefined,
        paddingVertical: checkbox ? 4 : undefined,
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
      {status ? <SessionStatusIcon status={status} colors={colors} /> : null}
      <Text style={{ flexShrink: checkbox ? 1 : undefined, color: colors.foreground, fontSize: compact ? 12 : 11, fontWeight: active ? "600" : "400" }}>
        {label}
      </Text>
      {count === undefined ? null : typeof count !== "number" && count.denominator !== undefined ? <StateRatio count={count} colors={colors} /> : (
        <Text style={{ color: colors.foregroundMuted, fontSize: compact ? 12 : 11, fontVariant: ["tabular-nums"] }}>
          {typeof count === "number" ? count : count.text}
        </Text>
      )}
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

/** A link-styled recovery action with a 44pt touch row, for the work line and the empty list. */
function TextAction({ testID, label, accessibilityLabel, colors, compact, onPress }: {
  testID: string; label: string; accessibilityLabel?: string; colors: ThemeColors; compact: boolean; onPress: () => void;
}) {
  return <Pressable testID={testID} accessibilityRole="button" accessibilityLabel={accessibilityLabel ?? label} onPress={onPress}
    style={{ minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 28, minWidth: FILTERS_TOUCH_TARGET, maxWidth: "100%", flexShrink: 1, justifyContent: "center", paddingHorizontal: 6 }}>
    <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: compact ? 13 : 12, fontWeight: "600" }}>{label}</Text>
  </Pressable>;
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
          <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12, fontWeight: "600" }}>
            {actionLabel}
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}

type SessionsScreenProps = PluginScreenProps & {
  directory?: ReturnType<typeof useSessionDirectory>;
  /** Opens New lead with a source and its saved revision pinned; absent where this screen has no New lead. */
  onNewLead?: (source: LeadSource) => void;
  organization?: ReturnType<typeof useOrganizationSettings>;
  projectId?: string | null;
  onIncludeArchived?: (include: boolean) => void;
  workspaces?: readonly OrganizationWorkspace[];
  /** `root` is the native project's root folder; only one folder of it names same-named projects apart. */
  projects?: readonly (OrganizationProject & { readonly root?: string })[];
  workspaceNote?: string;
  epicKey?: string | null;
  viewScope?: string;
  /** Explicit project/epic scope from another page; the Sessions tab itself opens unscoped. */
  scopeLabel?: string | null;
  onClearScope?: () => void;
  /** Configured plan source IDs by upper-case epic key; epics without one get no Plan/Board/Graph actions. */
  planMappings?: Parameters<typeof organizationScope>[1];
  workstreamRegistry?: WorkstreamRegistry;
  workstreamSources?: readonly ReadSourceSummary[];
  workstreamReads?: Readonly<Record<string, SourceRead>>;
  workstreamNote?: string | null;
  /** The host's responsibility projection read; absent or not ready leaves My work unevaluated. */
  responsibility?: ResponsibilityRead | null;
  /** Read-only retention projection over the full host inventory; absent means no retirement wiring. */
  retirement?: SessionRetirementRead;
  planSources?: Readonly<Record<string, readonly string[]>>;
  onEpicTab?: (tab: "Plan" | "Board" | "Graph", epicKey: string, sourceId: string | null) => void;
};
export function SessionsScreen({ host, layout, navigation, theme, directory, organization, projectId = null, onIncludeArchived, workspaces, projects, workspaceNote, epicKey = null, viewScope = host.id, scopeLabel = null, onClearScope, planSources, onEpicTab, planMappings, workstreamRegistry, workstreamSources = [], workstreamReads = {}, workstreamNote, responsibility = null, retirement, onNewLead }: SessionsScreenProps) {
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
  const includeArchived = view.membership === "archived";
  const localDirectory = useSessionDirectory({
    hostId: host.id,
    hostStatus: hostSummary?.status ?? "offline",
    // The local fallback has no parent read: request the full inventory once and filter this view
    // from it, so an ordinary count is never blocked by an archive-exclusive read.
    includeArchived: true,
    enabled: directory === undefined,
  });
  const { state: rawState, refresh, retry = refresh } = directory ?? localDirectory;
  // Read when an action settles, not when it started: a live, complete directory already streams that row's change.
  const liveDirectory = useRef(false);
  liveDirectory.current = rawState.status === "ready" && rawState.live && rawState.inventoryComplete === true;
  const availability = useSessionAvailability(host.id, rawState.entries, hostOnline && rawState.status === "ready" && view.status === "attention");
  const state = useMemo(() => ({ ...rawState, entries: availability.entries.filter((entry) => matchesMembership(entry, view.membership)) }), [rawState, availability.entries, view.membership]);
  const lifecycleChanged = (rows: Parameters<Parameters<typeof useSessionLifecycle>[1]>[0]) => {
    // The settled rows' own dispositions hide or update them at once; only a directory without a complete live
    // stream needs the full walk, which would otherwise rebuild every row for one session's change.
    availability.reconcile(rows); if (!liveDirectory.current) refresh();
  };
  const lifecycle = useSessionLifecycle(host.id, lifecycleChanged);
  // Archives whose fence is still held stay checkable after their rows leave, a later review and a reload.
  const recovery = useArchiveRecovery(host.id, lifecycle, lifecycleChanged);
  const [archivingId, setArchivingId] = useState<string | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  // The notice's Review archive in whichever tray shows it; closing the sheet returns focus there.
  const reviewArchiveRef = useRef<View | null>(null);
  const bulkToggleRef = useRef<View | null>(null);
  const bulkReviewRef = useRef<View | null>(null);
  const bulkReturnFocusRef = useRef(false);
  // The status button a hidden notice tray leaves, and each tray's Hide, so focus moves between them.
  const noticesButton = useRef<View | null>(null);
  const noticesHide = useRef<View | null>(null);
  const slimNoticesHide = useRef<View | null>(null);
  useEffect(() => { setBulkOpen(false); setRecoveryOpen(false); }, [host.id]);
  const preferences = organization?.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const shortcut = view.shortcut ?? null;
  const { layout: sessionLayout, grouping } = sessionPresentation(view, preferences);
  const streams = view.streams ?? null;
  const arranging = view.arranging ?? false;
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [, setExpansionRevision] = useState(0);
  const [orderSelection, setOrderSelection] = useState<string | null>(null);
  const [optionMenu, setOptionMenu] = useState<"grouping" | "sort" | "person" | "workingAs" | null>(null);
  const [rootSize, setRootSize] = useState({ width: 0, height: 0 });
  const rootRef = useRef<View | null>(null);
  const groupingRef = useRef<View | null>(null);
  const sortRef = useRef<View | null>(null);
  const personRef = useRef<View | null>(null);
  const workingAsRef = useRef<View | null>(null);
  const editsDisabled = !hostOnline || organization?.state.status !== "ready" || organization.state.saving;
  useEffect(() => { onIncludeArchived?.(includeArchived); }, [includeArchived, onIncludeArchived]);
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
  const viewportHeightRef = useRef(0);
  const contentHeightRef = useRef(0);
  // A finger on the list, its momentum or a scroll a moment ago: the reader is scrolling and owns the offset.
  const draggingRef = useRef(false);
  const userScrollAtRef = useRef(0);
  /** The offset the last scroll event reported. */
  const eventOffsetRef = useRef(scrollOffsetRef.current);
  const scrolling = (at = Date.now()) => draggingRef.current || at - userScrollAtRef.current < SCROLL_SETTLE_MS;
  const withHeaderRef = useRef(false);
  const resizedRef = useRef<(by?: number, fromOffset?: number) => void>(() => {});
  /** The offset the reader's place was last read at; the screen's own placements move the offset away from it. */
  const placedAtRef = useRef(scrollOffsetRef.current);
  // Native/web scroll events can arrive after several own placements; the older ones are not reader input.
  const ownScrollsRef = useRef<readonly { readonly offset: number; readonly at: number }[]>([]);
  /** How far the browser pulled a held row place back to the end of a list that got shorter; the next hold returns it. */
  const clampedByRef = useRef(0);
  const [rowLayout] = useState(() => createRowLayout((by, fromOffset) => resizedRef.current(by, fromOffset)));
  useLayoutEffect(() => () => rowLayout.cancelMeasurements(), [rowLayout]);
  const resultsPlaceRef = useRef<{ readonly saved: ReturnType<typeof rowLayout.bookmark>; readonly returning: boolean } | null>(null);
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
    listModeRef.current = listModeOf(nextView, preferences);
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
    // A live reorder that shortens the list can clamp its offset. Do not read
    // that as a deliberate scroll and toggle the toolbar. Ordinary status
    // updates that keep the order do not suppress scrolling.
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    programmaticUntilRef.current = Math.max(
      programmaticUntilRef.current,
      Date.now() + 150,
    );
  }, [state.entries]);

  /** Tree offset below the compact toolbar, from the last tree scroll that moved the rows. */
  const treeAnchorRef = useRef(scrollOffsetRef.current);
  const listModeRef = useRef(listModeOf(readSessionView(viewScope), preferences));
  /** Counts list placements (a mode or scope change); a scroll retry belongs to the one that asked for it. */
  const placementRef = useRef(0);
  /** The compact toolbar opening scrolls to its top; that is not a new tree position. */
  const anchorFrozenUntilRef = useRef(0);
  const updateView = useCallback(
    (patch: Partial<SessionViewState>) => {
      setHostView({
        hostId: host.id,
        viewScope,
        view: patchSessionView(viewScope, sessionViewTransition(readSessionView(viewScope), patch, treeAnchorRef.current, preferences)),
      });
    },
    [host.id, viewScope, preferences],
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

  // Every filter, search, sort and layout intent passes here first, so a row's waiting state-icon open is dropped before
  // the list changes under it, whether or not that row stays listed.
  const pinFilterInteraction = useCallback(() => {
    cancelDelayedOpen();
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
    ownScrollsRef.current = [];
    draggingRef.current = true;
    filterPinnedUntilRef.current = 0;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
  }, []);
  // A finger on the list or its momentum: the browser reports neither drag nor momentum, but it does report touches.
  const handleReaderBegin = useCallback(() => { ownScrollsRef.current = []; draggingRef.current = true; }, []);
  const handleScrollSettle = useCallback(() => {
    draggingRef.current = false;
    userScrollAtRef.current = Date.now();
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
      // An offset the screen did not ask for is the reader's scroll; a held place lands on the one it asked for. Nor
      // is the reader's the browser pulling an offset past the end of a list that got shorter back to its end.
      const clamped = Math.abs(visibleOffset - maxOffset) < 1 && Math.max(scrollOffsetRef.current, eventOffsetRef.current) > maxOffset + 1;
      const now = Date.now();
      ownScrollsRef.current = ownScrollsRef.current.filter((event) => now - event.at < SCROLL_SETTLE_MS * 2);
      const own = !draggingRef.current && ownScrollsRef.current.some((event) => Math.abs(event.offset - visibleOffset) < 1);
      if (!own && !clamped && Math.abs(visibleOffset - scrollOffsetRef.current) >= 1) {
        userScrollAtRef.current = now;
        ownScrollsRef.current = [];
      }
      if (clamped) clampedByRef.current += Math.max(0, scrollOffsetRef.current - visibleOffset);
      eventOffsetRef.current = visibleOffset;
      viewportHeightRef.current = layoutMeasurement.height;
      contentHeightRef.current = contentSize.height;
      // The screen's own landing keeps the place it held, and so does the browser pulling it back to the end of a
      // list that got shorter: rows shrink before their measurements arrive, and these restate the place. Any other
      // offset, or one the screen placed itself, is read again.
      if ((!own && !clamped && Math.abs(visibleOffset - scrollOffsetRef.current) >= 1) || placedAtRef.current !== scrollOffsetRef.current) {
        rowLayout.capture(rowsRef.current, withHeaderRef.current, visibleOffset, layoutMeasurement.height, contentSize.height);
        clampedByRef.current = 0;
      }
      placedAtRef.current = visibleOffset;
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
    [arranging, compact, compactToolbarHeight, layout.platform, viewScope, setToolbarVisibility, rowLayout],
  );

  const mappings = planMappings ?? preferences.displayMappings;
  // A key the display-only fallback derived, not a saved mapping or label, names its plans and capture in Details.
  const derivedSource = (entry: DirectoryEntry) => {
    const key = declaredScope(entry, preferences.displayMappings).workstream === null ? declaredScope(entry, mappings).workstream : null;
    return key === null ? null : derivedWorkstreamNote(workstreamSources, workstreamReads, declaredScope(entry, mappings).epic, entry.projectKey, key, preferences.planAssociations[entry.id]);
  };
  const favorites = preferences.favorites;
  const recent = useRecent(host.id);
  const rowClock = useClock();
  const opened = useMemo(() => new Map(recent.record.visits.map((visit) => [visit.id, visit.at])), [recent.record]);
  const recentIds = useMemo(() => recent.record.visits.map((visit) => visit.id), [recent.record]);
  // A current verified promotion is a logical epic binding even though the native ADHOC labels stay untouched.
  const projectedEntries = useMemo(() => {
    if (!retirement) return state.entries;
    return state.entries.map((entry) => {
      const promotion = retirement.projections.get(entry.id)?.promotion;
      if (!promotion || promotion.source.fresh !== "current" || entry.labels.epic) return entry;
      return { ...entry, labels: { ...entry.labels, epic: promotion.epic } };
    });
  }, [state.entries, retirement]);
  const scoped = useMemo(
    () => projectedEntries.filter((entry) => (projectId === null || entry.projectKey === projectId) && (epicKey === null || inEpicScope(entry, mappings, epicKey))),
    [projectedEntries, projectId, epicKey, mappings],
  );
  const scopedWorkspaces = useMemo(() => workspaces?.filter((workspace) => (projectId === null || workspace.projectKey === projectId)
    && (epicKey === null || mappings[`workspace:${workspace.id}`]?.epic?.trim().toLocaleLowerCase() === epicKey.trim().toLocaleLowerCase())), [workspaces, projectId, epicKey, mappings]);
  const scopedProjects = useMemo(() => projects?.filter((project) => (projectId === null || project.id === projectId)
    && (epicKey === null || scoped.some((entry) => entry.projectKey === project.id))), [projects, projectId, epicKey, scoped]);
  const deskScopes = useMemo(() => deriveDeskScopes(scoped, scopedWorkspaces, scopedProjects), [scoped, scopedWorkspaces, scopedProjects]);
  // Retain legacy shortcuts; native ID choices distinguish same-named desks and survive renames.
  const scopeChoices = [...SCOPE_CHOICES.slice(0, -1), ...deskScopes, SCOPE_CHOICES[SCOPE_CHOICES.length - 1]!];
  const scopeChoiceLabel = shortcut === null ? null : scopeChoices.find(({ id }) => id === shortcut)?.label ?? (shortcut?.startsWith("desk-project:") ? `Unavailable native desk (${shortcut})` : shortcut);
  const projectRoots = useMemo(() => new Map((projects ?? []).flatMap((project) => project.root ? [[project.id, project.root] as const] : [])), [projects]);
  const sort: SessionSort = view.sort ?? "current";
  const sortDirection: SortDirection = view.sortDirection ?? defaultSortDirection(sort);
  // An explicit sort orders sessions within each group and the flat results; it never moves a group.
  const leafOrder = useMemo(() => explicitLeafOrder(scoped, sort, sortDirection), [scoped, sort, sortDirection]);
  const tree = useMemo(() => relateSplitProjects(buildOrganizationTree({ entries: scoped, ...preferences, displayMappings: mappings, grouping, workspaces: scopedWorkspaces, projects: scopedProjects, leafOrder, byWorkstream: streams !== null }), projectRoots), [scoped, preferences, mappings, grouping, scopedWorkspaces, scopedProjects, projectRoots, leafOrder, streams]);
  const flatTree = useMemo(() => buildOrganizationTree({ entries: scoped, ...preferences, displayMappings: mappings, grouping: "flat" }), [scoped, preferences, mappings]);
  const tokens = useMemo(() => view.query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean), [view.query]);
  const task = view.task ?? null;
  const resultMode = isResultMode(view, preferences);
  // My work is every session on this host. Assignment is a separate filter, off until chosen, read against the host's projection.
  const work: WorkScope = workScopeOf(view);
  const assignment: AssignmentScope = assignmentOf(view);
  // ponytail: expiry is checked on each render, which live native updates drive; subscribe to the minute clock if an idle screen must flip on time.
  const clock = { now: Date.now() };
  const projection = responsibility?.status === "ready" ? responsibility.projection : null;
  const responsibilityIndex = useMemo(() => projection ? indexResponsibility(projection) : null, [projection]);
  const roster = useMemo(() => responsibilityIndex ? rosterChoices(responsibilityIndex) : [], [responsibilityIndex]);
  const realm = projection?.realm ?? null;
  // Remembered per host and realm; read before the first render, kept while the projection or roster is unavailable.
  const remembered = realm === null ? null : readRememberedWorkingAs(host.id, realm);
  const workingAs = remembered?.accountId ?? null;
  const workingAsLabel = roster.find(({ accountId }) => accountId === workingAs)?.label ?? null;
  const subject = assignment === "any" ? null : assignment.startsWith("person:") ? assignment.slice("person:".length) : workingAs;
  const subjectLabel = roster.find(({ accountId }) => accountId === subject)?.label ?? null;
  const assignmentState = assignment === "any" ? "any" as const
    : responsibilityIndex === null ? "unavailable" as const
      : responsibilityExpired(responsibilityIndex, clock.now) ? "expired" as const
        : subject === null ? "no-profile" as const
          : !responsibilityIndex.people.has(subject) ? "unknown-person" as const : "ready" as const;
  const assignmentPart = assignment === "any" ? null : assignment === "me" ? "Assigned to me" : assignment === "unknown" ? "Unknown assignment" : `Assigned to ${subjectLabel ?? "a person not in this projection"}`;
  const responsibilityById = useMemo(() => assignmentState === "ready" ? new Map(scoped.map((entry) => [entry.id, sessionResponsibility(entry, responsibilityIndex!, subject!)])) : null,
    [assignmentState, scoped, responsibilityIndex, subject]);
  const inAssignment = useCallback((entry: DirectoryEntry) => assignment === "any"
    || responsibilityById?.get(entry.id)?.status === (assignment === "unknown" ? "unknown" : "match"), [assignment, responsibilityById]);
  const eligible = useCallback((entry: DirectoryEntry) => matchesShortcut(entry, shortcut, mappings)
    && (streams === null || streams.includes(workstreamKey(organizationScope(entry, mappings).workstream)))
    && matchesEpicScope(entry, view.epic, mappings) && inAssignment(entry), [shortcut, streams, view.epic, mappings, inAssignment]);
  // Current narrows native-state visibility; explicit result filters keep their tree for Show in tree.
  // All work with no workstream or assignment choice keeps empty workspaces; other scopes keep only groups holding sessions.
  const treeEligible = useCallback((entry: DirectoryEntry) => eligible(entry)
    && (view.status !== "current" || matchesStatus(entry, "current", favorites)), [eligible, view.status, favorites]);
  const contextTree = useMemo(() => filterOrganizationTree(tree, treeEligible,
    (group) => streams === null && work === "all" && assignment === "any" && scopeKeepsGroup(group, shortcut, mappings), treeEligible), [tree, treeEligible, shortcut, streams, work, assignment, mappings]);
  const linked = useMemo(() => task ? new Set(taskSessions(scoped, task).map(({ entry }) => entry.id)) : null, [scoped, task]);
  // Matches before the state filter, so each state chip counts what choosing it would show.
  const candidates = useMemo(() => scoped.filter((entry) => eligible(entry) && (linked === null || linked.has(entry.id)) && matchesResultSearch(entry, tokens, mappings)),
    [scoped, eligible, linked, tokens, mappings]);
  const countFreshness = nativeFreshnessLabel({ hostStatus: hostSummary?.status ?? "offline", status: state.status, live: state.live });
  const assignmentBlocked = assignment !== "any" && assignmentState !== "ready";
  const blockedView = useMemo(() => providerBlockedView({ candidates, reads: NO_REFUSAL_READS, state, includeArchived, freshness: countFreshness,
    blocked: assignmentBlocked ? assignmentPart : null, now: clock.now }), [candidates, state, includeArchived, countFreshness, assignmentBlocked, assignmentPart, clock.now]);
  const results = useMemo(() => {
    const matching = view.status === "provider_blocked" ? [...blockedView.sessions] : candidates.filter((entry) => matchesStatus(entry, view.status, favorites, recentIds));
    const compare = sessionComparator(sort, sortDirection);
    if (compare) return matching.sort(compare);
    // Recent's own order is the order sessions were opened, most recent first.
    if (view.status === "recent") {
      const byId = new Map(matching.map((entry) => [entry.id, entry]));
      return recentIds.flatMap((id) => byId.get(id) ?? []);
    }
    const ids = new Set(matching.map((entry) => entry.id));
    return visibleSessions(flatTree).filter((entry) => ids.has(entry.id));
  }, [candidates, blockedView, view.status, favorites, recentIds, sort, sortDirection, flatTree]);
  const matchingIds = useMemo(() => new Set(results.map((entry) => entry.id)), [results]);
  const sessionNarrowing = tokens.length > 0 || !!task || (view.status !== "all" && view.status !== "current")
    || streams !== null || view.epic != null || assignment !== "any";
  const filtered = useMemo(() => filterOrganizationTree(tree, (entry) => matchingIds.has(entry.id),
    (group) => !sessionNarrowing && work === "all" && scopeKeepsGroup(group, shortcut, mappings), (entry) => matchingIds.has(entry.id)),
    [tree, matchingIds, sessionNarrowing, work, shortcut, mappings]);
  const epicChoices = useMemo(() => epicScopeChoices(scoped, mappings), [scoped, mappings]);
  const narrowed = tokens.length > 0 || view.status !== "all" || !!task || shortcut !== null || streams !== null || view.epic != null || work !== "all" || assignment !== "any";
  const arrangeEnabled = arranging && !narrowed && !editsDisabled && sort === "current";
  const arrangementTree = resultMode ? flatTree : tree;
  // An assignment filter's own count beside the inventory count of every association; it never replaces it.
  const assignmentCounts = useMemo(() => {
    if (assignment === "any" || responsibilityById === null) return null;
    const counts = new Map<string, number>();
    for (const entry of scoped) if (inAssignment(entry)) {
      const key = workstreamKey(organizationScope(entry, mappings).workstream);
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return counts;
  }, [assignment, responsibilityById, scoped, inAssignment, mappings]);
  // Work/layout/sort/disclosure never refine session membership. State is intentionally absent here.
  const refined = tokens.length > 0 || !!task || shortcut !== null || streams !== null || view.epic != null || assignment !== "any";
  const statusCounts = useMemo(() => sessionStateCounts({ universe: scoped, candidates, favorites, recent: recentIds, refined, state, includeArchived,
    freshness: countFreshness, blocked: assignmentBlocked ? assignmentPart : null, providerBlocked: blockedView.count }),
    [scoped, candidates, favorites, recentIds, refined, state, includeArchived, countFreshness, assignmentBlocked, assignmentPart, blockedView]);
  const shortcutCounts = useMemo(() => SESSION_SHORTCUTS.map(({ id }) => scoped.filter((entry) => matchesShortcut(entry, id, mappings)).length), [scoped, mappings]);
  const workstreamOptions = useMemo(() => planWorkstreamOptions(workstreamSources, workstreamReads, workstreamRegistry, scoped, mappings, scopedWorkspaces), [workstreamSources, workstreamReads, workstreamRegistry, scoped, mappings, scopedWorkspaces]);
  // My work offers the associations this host's sessions carry, and any already chosen; All work adds plan-only and registry choices.
  const workstreamChoices = useMemo(() => work === "all" ? workstreamOptions : workstreamOptions.filter((option) => option.sessions > 0 || !!streams?.includes(option.key)),
    [work, workstreamOptions, streams]);
  // My work counts, never lists, the choices it leaves out.
  const hiddenChoices = useMemo(() => work === "all" ? [] : workstreamOptions.filter((option) => !workstreamChoices.includes(option)), [work, workstreamOptions, workstreamChoices]);
  // Responsibility facts over this context's inventory, before the other filters.
  const workTally = useMemo(() => {
    if (responsibilityById === null) return null;
    const reasons = new Map<MatchReason, number>();
    let matched = 0;
    let unknown = 0;
    for (const result of responsibilityById.values()) {
      if (result.status === "match") {
        matched += 1;
        for (const reason of result.reasons) reasons.set(reason, (reasons.get(reason) ?? 0) + 1);
      } else if (result.status === "unknown") unknown += 1;
    }
    return { matched, unknown, reasons };
  }, [responsibilityById]);
  const localExpansion = readLocalExpansion(host.id);
  const expanded = useMemo(() => ({ ...preferences.expandedGroups, ...localExpansion }), [preferences.expandedGroups, localExpansion]);
  const peek = resultMode ? view.peek ?? null : null;
  // Branches opened or closed while a result shows in the tree; never saved.
  const [peekToggles, setPeekToggles] = useState<Readonly<Record<string, boolean>>>({});
  const peekAncestors = useMemo(() => peek ? sessionAncestors(contextTree, peek.sessionId) : null, [contextTree, peek]);
  const expandMatches = !resultMode && (tokens.length > 0 || !!task || (view.status !== "all" && view.status !== "current"));
  const groupFilterKey = JSON.stringify([viewScope, view.query, view.status, task, grouping, shortcut, streams, view.epic, assignment]);
  const [groupToggles, setGroupToggles] = useState<{ key: string; values: Readonly<Record<string, boolean>> }>({ key: "", values: {} });
  const matchingExpansion = useMemo(() => expandMatches ? { ...Object.fromEntries(allGroupKeys(filtered).map((key) => [key, true])), ...(groupToggles.key === groupFilterKey ? groupToggles.values : {}) } : {}, [expandMatches, filtered, groupToggles, groupFilterKey]);
  const treeExpanded = useMemo(() => peekAncestors ? { ...expanded, ...Object.fromEntries(peekAncestors.map((key) => [key, true])), ...peekToggles } : { ...expanded, ...matchingExpansion }, [expanded, peekAncestors, peekToggles, matchingExpansion]);
  const rows: readonly ListRow[] = useMemo(() => {
    if (resultMode && !peek) return arrangeEnabled ? flattenOrganizationTree(flatTree, {}, true) : results.map((entry) => ({ kind: "result" as const, entry }));
    return flattenOrganizationTree(peek ? contextTree : filtered, treeExpanded, false, defaultGroupExpanded);
  },
    [resultMode, peek, arrangeEnabled, flatTree, results, contextTree, filtered, treeExpanded]);
  const rowsRef = useRef(rows);
  rowsRef.current = rows;
  rowLayout.renderRows(rows);
  const provenance = useMemo(() => new Map(results.map((entry) => [entry.id, sessionProvenance(entry, mappings)])), [results, mappings]);
  const rowSignature = useMemo(() => rows.map(listRowKey).join("\n"), [rows]);
  /** When this render began: a long one holds back the scroll events of a reader still scrolling. */
  const renderedAt = Date.now();
  /** Scrolls by this much to keep the reader's place; the scroll it causes is not the reader's. */
  const holdPlace = (by: number) => {
    if (Math.abs(by) >= 0.5) {
      const target = Math.max(0, scrollOffsetRef.current + by);
      scrollOffsetRef.current = target;
      lastScrollOffsetRef.current = target;
      if (compact) compactAnchorRef.current = target;
      writeScrollOffset(viewScope, target);
      ownScrollsRef.current = [...ownScrollsRef.current.filter((event) => Date.now() - event.at < SCROLL_SETTLE_MS * 2), { offset: target, at: Date.now() }].slice(-16);
      listRef.current?.scrollToOffset({ offset: target, animated: false });
    }
    placedAtRef.current = scrollOffsetRef.current;
  };
  /** The part of a held row place the browser's pull to the end took: given back with the next hold. */
  const owed = () => {
    const by = rowLayout.atEnd() ? 0 : clampedByRef.current;
    clampedByRef.current = 0;
    return by;
  };
  // A row, the header, the content or the viewport changed height under the reader: a reader at the end goes to the
  // end the list reports. While they scroll, the place follows them instead.
  resizedRef.current = (measuredBy, fromOffset) => {
    if (scrolling()) {
      rowLayout.capture(rowsRef.current, withHeaderRef.current, scrollOffsetRef.current, viewportHeightRef.current, contentHeightRef.current);
      return;
    }
    if (measuredBy !== undefined) {
      if (fromOffset === undefined) holdPlace(measuredBy + owed());
      else {
        clampedByRef.current = 0;
        holdPlace(fromOffset + measuredBy - scrollOffsetRef.current);
      }
      return;
    }
    // Since the place was read, the screen placed the list itself (a mode, scope, Back or toolbar): read it there.
    if (placedAtRef.current !== scrollOffsetRef.current) {
      rowLayout.capture(rowsRef.current, withHeaderRef.current, scrollOffsetRef.current, viewportHeightRef.current, contentHeightRef.current);
      placedAtRef.current = scrollOffsetRef.current;
      clampedByRef.current = 0;
      return;
    }
    const by = rowLayout.atEnd() ? contentHeightRef.current - viewportHeightRef.current - scrollOffsetRef.current : rowLayout.restate(rowsRef.current, withHeaderRef.current);
    if (!scrolling()) holdPlace(by + owed());
  };
  const previousRowSignatureRef = useRef(rowSignature);
  const previousRowsRef = useRef(rows);
  const renderedRef = useRef({ entries: state.entries, view });
  useLayoutEffect(() => {
    if (previousRowSignatureRef.current === rowSignature) {
      return;
    }
    previousRowSignatureRef.current = rowSignature;
    const previous = previousRowsRef.current;
    previousRowsRef.current = rows;
    // New rows move the content under the offset, and a shorter list clamps
    // it. That is not a gesture: the next compact sample re-anchors instead
    // of closing the toolbar.
    if (compact) {
      compactAnchorRef.current = Number.NaN;
    }
    // A streamed change keeps the reader's place. Their own choices place the list themselves, and a scroll
    // in progress keeps its course.
    const offset = scrollOffsetRef.current;
    if (state.entries !== renderedRef.current.entries && view === renderedRef.current.view && !scrolling(renderedAt)) {
      const by = rowLayout.follow(previous, rows, withHeaderRef.current, offset, viewportHeightRef.current, contentHeightRef.current, placedAtRef.current === offset);
      holdPlace(by + owed());
      if (!rowLayout.atEnd()) resizedRef.current();
    } else {
      rowLayout.capture(rows, withHeaderRef.current, offset, viewportHeightRef.current, contentHeightRef.current);
      placedAtRef.current = offset;
      clampedByRef.current = 0;
    }
  }, [compact, rowSignature]);
  useLayoutEffect(() => { renderedRef.current = { entries: state.entries, view }; });
  const shown = useMemo(() => resultMode ? results : visibleSessions(filtered), [resultMode, results, filtered]);
  const bulkScopeIds = useMemo(() => shown.filter((entry) => entry.archivedAt === null).map((entry) => entry.id), [shown]);
  const bulkSelection = useArchiveSelection(host.id, bulkScopeIds);
  const ops = useSessionOperations({ hostId: host.id, online: hostOnline, colors, compact, entries: availability.entries, opened,
    newLead: onNewLead ? (source, revision) => onNewLead({ entry: source, summary: revision }) : "New lead is not offered on this screen; start it from the session switcher.",
    onTakeoverStart: () => bulkSelection.clear() });
  const rowTotals = useUsageTotals(ops.totals, shown.map((entry) => entry.id));
  const selectedSeats = bulkSelection.selected.flatMap((id) => availability.entries.filter((entry) => entry.id === id));
  const allSelected = bulkScopeIds.length > 0 && bulkScopeIds.every((id) => bulkSelection.selected.includes(id));
  const workspaceCount = resultMode ? new Set(shown.map((entry) => entry.workspaceId ?? entry.workspaceName ?? entry.cwd)).size : organizationWorkspaceCount(filtered);
  const resultWorkspaces = new Set(shown.filter(inWorkspace).map((entry) => entry.workspaceId ?? entry.workspaceName)).size;
  const unplaced = shown.filter((entry) => !inWorkspace(entry)).length;
  // Inventory and placement are secondary facts in Filters; the matching count has one primary home per page.
  const sessionCount = `Inventory: ${scoped.length} session${scoped.length === 1 ? "" : "s"} in this host context${includeArchived ? ", including archived" : ", excluding archived"}`;
  const inventoryRead = state.lastUpdatedAt === null
    ? "not read yet"
    : state.inventoryComplete
      ? `complete inventory read ${updatedLabel(state.lastUpdatedAt)}`
      : `partial inventory read ${updatedLabel(state.lastUpdatedAt)}`;
  const countSummary = !readCoversMembership(state, includeArchived) || state.lastUpdatedAt === null
    ? "Inventory for this membership has not been read yet."
    : `${sessionCount}. ${assignmentBlocked ? "Results not evaluated" : `Results span ${workspaceCount} workspace / folder group${workspaceCount === 1 ? "" : "s"}${resultMode ? "" : ", including retained empty workspaces"}. Matching sessions: ${resultWorkspaces} native workspace${resultWorkspaces === 1 ? "" : "s"}, ${unplaced} session${unplaced === 1 ? "" : "s"} without a workspace`}. ${inventoryRead}.`;
  // The same final IDs both layouts draw; layout, grouping, sort and disclosure never change it.
  const filterCount = matchingCount({ count: matchingIds.size, state, includeArchived, blocked: assignmentBlocked ? assignmentPart : null,
    freshness: countFreshness });
  const setExpansion = (patch: Readonly<Record<string, boolean>>) => {
    const local = patchLocalExpansion(host.id, patch);
    setExpansionRevision((revision) => revision + 1);
    savePreference({ expandedGroups: { ...preferences.expandedGroups, ...local } });
  };
  const setAllGroups = (open: boolean) => {
    const values = Object.fromEntries(allGroupKeys(tree).map((key) => [key, open]));
    if (expandMatches) setGroupToggles({ key: groupFilterKey, values }); else setExpansion(values);
  };
  const toggleGroup = (row: ListRow) => {
    if (row.kind !== "group") return;
    if (peek) setPeekToggles((current) => ({ ...current, [row.group.key]: !row.expanded }));
    else if (expandMatches) setGroupToggles({ key: groupFilterKey, values: { ...matchingExpansion, [row.group.key]: !row.expanded } });
    else setExpansion({ [row.group.key]: !row.expanded });
  };
  const chooseLayout = (next: SessionLayout) => {
    pinFilterInteraction();
    setDetailsId(null);
    setOptionMenu(null);
    updateView({ layout: next, grouping, scrollOffset: 0 });
    savePreference({ layout: next, grouping: grouping === "placement" ? "hierarchy" : grouping });
  };
  const chooseGrouping = (next: SessionGrouping) => {
    pinFilterInteraction();
    setDetailsId(null);
    updateView({ grouping: next, scrollOffset: 0 });
    if (next !== "placement") savePreference({ grouping: next, layout: sessionLayout });
  };
  const chooseStreams = (next: readonly string[] | null) => {
    pinFilterInteraction();
    updateView({ streams: next, scrollOffset: 0 });
  };
  const chooseWork = (next: WorkScope) => {
    pinFilterInteraction();
    setDetailsId(null);
    updateView({ work: next, scrollOffset: 0 });
  };
  const chooseAssignment = (next: AssignmentScope) => {
    pinFilterInteraction();
    setDetailsId(null);
    updateView({ assignment: next, scrollOffset: 0 });
  };
  const chooseSort = (next: SessionSort) => {
    pinFilterInteraction();
    updateView({ sort: next, sortDirection: next === "current" ? undefined : defaultSortDirection(next), scrollOffset: 0 });
  };
  // The profile is per host and tracker realm; the store ends the peeks it affects, this view's included.
  const chooseWorkingAs = (accountId: string | null) => {
    if (realm === null) return;
    pinFilterInteraction();
    writeWorkingAs(host.id, realm, accountId);
    setDetailsId(null);
    updateView({ scrollOffset: 0 });
  };
  const handleResetView = () => {
    pinFilterInteraction();
    setOrderSelection(null);
    setDetailsId(null);
    updateView(resetSessionViewPatch(readSessionView(viewScope)));
  };
  const moveRow = (row: OrganizationRow, distance: number) => {
    const parent = row.kind === "group" ? row.group.parentKey ?? "root" : row.parentKey;
    const peers = siblingKeys(arrangementTree, parent).filter((key) => (row.kind === "session") === key.startsWith("session:"));
    const movable = parent === "root" && row.kind === "group" ? peers.filter((key) => organizationGroupRank(arrangementTree.find((group) => group.key === key) ?? row.group) === organizationGroupRank(row.group))
      // No role stays last.
      : parent === ROLE_ROOT_KEY && row.kind === "group" ? peers.filter((key) => (key === NO_ROLE_KEY) === (row.group.key === NO_ROLE_KEY)) : peers;
    savePreference({ siblingOrder: { ...preferences.siblingOrder, [parent]: reorderSavedSiblingOrder(preferences.siblingOrder[parent] ?? [], siblingKeys(arrangementTree, parent), movable, rowKey(row), distance) } });
  };
  const moveSelection = (distance: number) => {
    const row = rows.find((candidate) => listRowKey(candidate) === orderSelection);
    if (row && row.kind !== "result") moveRow(row, distance);
  };

  // Under Recent an explicit sort says it replaces the order sessions were opened.
  const sortPart = sort === "current" ? null : `${sortLabel(sort)}, ${sortDirectionLabel(sort, sortDirection).toLocaleLowerCase()}${view.status === "recent" ? ", not open order" : ""}`;
  const optionParts = [
    sessionLayout === "flat" ? "Flat" : null,
    sessionLayout === "grouped" && grouping !== "hierarchy" ? GROUPINGS.find((choice) => choice.id === grouping)?.label : null,
    sortPart,
    streams !== null ? `${streams.length} workstream${streams.length === 1 ? "" : "s"}` : null,
    shortcut === "Desks" ? "Durable desks only" : null,
    view.status === "closed" ? "Closed sessions" : view.status === "initializing" ? "Starting sessions" : null,
    includeArchived ? "Archived history" : null,
  ].filter((part): part is string => !!part);
  // One explanation of the inventory counts, under About these counts in the association selector.
  const aboutCounts = `${!workstreamRegistry?.revision && workstreamSources.length === 0 ? "Choices come from session metadata and display mappings, with desk and Other work fallbacks. Names may be unregistered. The full workstream registry and plan declarations are not loaded here; this list may be incomplete. " : ""}${workstreamRegistry?.revision ? `${Object.keys(workstreamRegistry.workstreams).length} registered workstreams · ${workstreamOptions.filter((option) => option.plans > 0).length} declared plan keys here · ${Object.keys(workstreamRegistry.desks).length} registered desk categories. Registered keys are global. ` : workstreamSources.length ? "The workstream registry is not loaded, so no choice says whether it is registered. " : ""}Counts follow each session's workstream association, which these choices filter by, in this project/epic context before search, state and workstream filters. The tree keeps every session of a native desk project under that desk, so a choice names the desk where it counts sessions shown there. Workspaces count where a counted session runs or a workspace mapping points; in All work the tree also lists empty native workspaces until workstream or assignment choices narrow it. Zero sessions means none linked in this inventory: a plan links a session only through the session's known epic or an explicit plan link. ${work === "all" ? "Plan-only choices are included." : "My work lists choices with sessions here; All work adds plan-only choices."}${workstreamNote ? ` ${workstreamNote}` : ""} An assignment filter adds its own count to each choice and never changes these.`;
  const workingAsStore = remembered?.persistence === "device" ? "remembered on this device for this host and tracker"
    : remembered?.persistence === "failed" ? "not remembered: this device's store failed, so a choice lasts this app session"
      : "not remembered on this device, which offers Kontor no store, so a choice lasts this app session";
  const responsibilitySource = responsibility === null ? "Responsibility projection not read yet; assignment filters cannot be evaluated. My work does not need it."
    : responsibility.status !== "ready" || !projection ? `${responsibility.diagnostics.join(" ") || "Responsibility projection unavailable."} Assignment filters cannot be evaluated; My work does not need it.`
      : `Responsibility from ${projection.realm}, observed ${readTime(projection.generatedAt, clock.now)}, ${responsibilityExpired(responsibilityIndex!, clock.now) ? "expired" : "expires"} ${readTime(projection.expiresAt, clock.now)}${projection.provenance.complete ? "" : ", partial"} · ${projection.people.length} people · ${projection.issues.length} issues · workstream responsibility ${projection.provenance.workstreams ? "declared" : "not declared, so workstream relations are unknown"}. Working as is a viewing choice for Assigned to me, not sign-in or access control; it is ${workingAsStore}.`;
  const activeCount = sessionFilterCount(view);
  const clearable = activeCount > 0;
  const taskKeys = task ? [task.jira ? `${TASK_JIRA_LABELS.join(" or ")} equal to ${task.jira}` : null, task.kontorId ? `${TASK_TICKET_LABEL} equal to ${task.kontorId}` : null].filter(Boolean).join(", or ") : "";
  const summaryParts = activeFilterParts({ query: view.query, status: view.status, shortcut: shortcut?.startsWith("desk-project:") ? scopeChoiceLabel : shortcut, streams, includeArchived, task: task?.label, epic: view.epic ? epicScopeLabel(view.epic) : null, work: [work === "mine" ? "My work" : null, assignmentPart].filter(Boolean).join(" · ") || null });
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
      // A scroll event since may have moved the known offset; the place is read again where this lands.
      scrollOffsetRef.current = offset;
      placedAtRef.current = Number.NaN;
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
  const detailsHelp = useControlHelp("");
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
    resultsPlaceRef.current = { saved: rowLayout.bookmark(rowsRef.current, withHeaderRef.current, scrollOffsetRef.current, viewportHeightRef.current, contentHeightRef.current), returning: false };
    updateView({ peek: { sessionId: entry.id, resultsOffset: Math.max(0, scrollOffsetRef.current - head) }, scrollOffset: 0 });
  }, [compact, compactToolbarHeight, updateView]);
  const handleBackToResults = useCallback(() => {
    const current = readSessionView(viewScope).peek;
    if (!current) return;
    focusAfterModeRef.current = () => {
      const node = treeButtonsRef.current.get(current.sessionId);
      if (pageDocument()) (node as unknown as { focus?: (options: { preventScroll: boolean }) => void } | undefined)?.focus?.({ preventScroll: true });
      else node?.focus?.();
    };
    if (resultsPlaceRef.current) resultsPlaceRef.current = { ...resultsPlaceRef.current, returning: true };
    updateView({ peek: null, scrollOffset: current.resultsOffset });
  }, [updateView, viewScope]);

  // Each list mode keeps its place: results open at the top or where a peek left
  // them, a peek scrolls to its session, and the tree returns where it was.
  const listMode = listModeOf(view, preferences);
  useLayoutEffect(() => {
    const previous = listModeRef.current;
    if (previous === listMode) {
      return;
    }
    listModeRef.current = listMode;
    placementRef.current += 1;
    const stored = readSessionView(viewScope).scrollOffset;
    const head = compact && !toolbarCollapsedRef.current ? compactToolbarHeight() : 0;
    const saved = listMode === "results" && resultsPlaceRef.current?.returning ? resultsPlaceRef.current.saved : null;
    if (resultsPlaceRef.current?.returning) resultsPlaceRef.current = null;
    const offset = saved ? rowLayout.restore(saved, rowsRef.current, withHeaderRef.current, viewportHeightRef.current)
      : listMode === "results" && previous === "tree" ? 0 : stored > 0 ? stored + head : 0;
    const sessionId = peek?.sessionId ?? null;
    // Each pass finds the session's row again: a collapse or a live update can move or remove it first.
    const place = () => {
      const index = sessionId === null ? -1 : rowsRef.current.findIndex((row) => row.kind === "session" && row.entry.id === sessionId);
      if (index < 0) return false;
      // Where the row lands, so a row measured meanwhile holds this place rather than the one left.
      const row = rowLayout.layout(rowsRef.current, index, withHeaderRef.current);
      scrollOffsetRef.current = Math.max(0, row.offset - 0.3 * (viewportHeightRef.current - row.length));
      placedAtRef.current = Number.NaN;
      listRef.current?.scrollToIndex({ index, viewPosition: 0.3, animated: false });
      return true;
    };
    scrollOffsetRef.current = offset;
    if (saved) placedAtRef.current = offset;
    lastScrollOffsetRef.current = offset;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    compactAnchorRef.current = Number.NaN;
    programmaticUntilRef.current = Date.now() + 400;
    if (listMode === "tree") treeAnchorRef.current = Math.max(0, offset - head);
    writeScrollOffset(viewScope, offset);
    const focus = focusAfterModeRef.current;
    focusAfterModeRef.current = null;
    const timer = setTimeout(() => {
      if (!place()) {
        const target = saved ? scrollOffsetRef.current : offset;
        scrollOffsetRef.current = target;
        placedAtRef.current = saved ? target : Number.NaN;
        listRef.current?.scrollToOffset({ offset: target, animated: false });
      }
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
  // One lifecycle action at a time: the X, bulk Archive and Refresh status wait while another runs.
  const lifecycleIdle = !lifecycle.busy && !lifecycle.running && !recovery.checking;
  const canStart = hostOnline && lifecycleIdle && state.status === "ready";
  useLayoutEffect(() => {
    if (bulkOpen || !bulkReturnFocusRef.current) return;
    bulkReturnFocusRef.current = false;
    // The dialog disables its trigger; restore only after the closed render re-enables it.
    const origin = bulkSelection.selected.length ? bulkReviewRef.current : null;
    focusControl(canStart && !includeArchived ? origin ?? bulkToggleRef.current ?? rootRef.current : rootRef.current);
  }, [bulkOpen]);
  const selectForArchive = (entry: DirectoryEntry) => { if (canStart && !bulkOpen) bulkSelection.toggle(entry.id); };
  const openOrSelect = (entry: DirectoryEntry) => { if (bulkSelection.selecting) selectForArchive(entry); else handleOpen(entry); };
  // Acknowledged items are out of the line, its count and the review; their rows keep their archive feedback.
  const reviewCount = recovery.attention.length;
  // One compact line (NN/g: one notice, its action beside it): the text wraps in its own column and the icon action follows
  // it, so no width gives the action a row of its own. Review archive opens every unresolved item; Retry only re-reads a failed list.
  const reviewName = reviewCount === 1 ? "Review archive" : `Review ${reviewCount} archives`;
  const recoveryLine = recovery.line ? <View key="archive" testID="sessions-archive-recovery" style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
    <Text role="status" accessibilityLiveRegion="polite" style={{ flexShrink: 1, minWidth: 0, color: colors.statusWarning, fontSize: 12 }}>{recovery.line}</Text>
    {reviewCount ? <NoticeAction ref={reviewArchiveRef} icon="ClipboardList" colors={colors} accessibilityLabel={reviewName} expanded={recoveryOpen}
      help={`${reviewName}. Shows its state, Refresh status and details; nothing is archived again.`} onPress={() => setRecoveryOpen(true)} />
      : <NoticeAction icon="RefreshCw" colors={colors} accessibilityLabel="Retry archive checks" help="Retry archive checks. Reads the archive list again; nothing is archived."
        disabled={recovery.checking} onPress={() => { void recovery.reload(); }} />}
  </View> : null;
  const openRecovered = (item: RecoveryItem) => {
    const result = openRecoveredSession({ row: item.row, id: item.ref.agentId, serverId: host.id, navigation });
    if (result.ok) setRecoveryOpen(false); else toast.error(result.message);
  };
  const closeRecovery = () => { setRecoveryOpen(false); recovery.clearNotes(); setTimeout(() => focusControl(reviewArchiveRef.current ?? noticesButton.current ?? rootRef.current), 0); };
  // The acknowledged item's own key, persisted per host like a hidden tray: the same state stays out, a changed one is a new notice.
  const hideRecovered = (item: RecoveryItem) => { saveHidden([...new Set([...hiddenNotices, archiveAckKey(item)])]); closeRecovery(); };
  // Each notice's keys name what it reports; a hidden tray opens again for any key it has not hidden.
  const banners = ([
    showBanner && staleMessage ? { keys: [`source:${state.status}:${staleMessage}`], summary: state.status === "error" ? "Session read failed" : "Sessions not live",
      tone: state.status === "error" ? "danger" : "warning", node: (
        <Banner
          key="source"
          testID="sessions-source-banner"
          message={`${staleMessage} ${
            state.inventoryComplete ? "Last complete read" : "Inventory read attempt"
          }: ${updatedLabel(state.lastUpdatedAt)}.`}
          tone={state.status === "error" ? "danger" : "warning"}
          actionLabel="Retry"
          onAction={retry}
          colors={colors}
          {...noticeProps}
        />
      ) } : null,
    navigationMissing ? { keys: ["navigation"], summary: "Session navigation unavailable", tone: "warning", node: (
        <Banner
          key="navigation"
          testID="sessions-navigation-banner"
          message="This Paseo app does not expose native session navigation to plugins. Update Paseo to open sessions from this screen."
          tone="warning"
          colors={colors}
          {...noticeProps}
        />
      ) } : null,
    organization && settingsUnavailable ? { keys: [`settings:${organization.state.status}`], summary: `Organization settings ${organization.state.status}`, tone: "warning",
      node: <Text key="settings" style={{ color: colors.statusWarning, fontSize: 12 }}>Organization settings {organization.state.status}; saved preferences are unavailable. Native sessions remain visible and branches still open and close here.</Text> } : null,
    settingsAlert ? { keys: [`save:${settingsAlert}`], summary: "Organization not saved", tone: "warning",
      node: <Text key="save" accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{settingsAlert}</Text> } : null,
  ] satisfies (Notice | null)[]).filter((notice) => notice !== null);
  // One key per exact review, session and state, so a confirmed one leaves the tray hidden and a new or changed one opens it; a read error is a new notice too.
  const archiveNotice: Notice | null = recoveryLine ? { summary: archiveSummary(recovery.attention, null) ?? "Archive checks could not be read", tone: "warning", node: recoveryLine,
    keys: [...recovery.attention.map(archiveNoticeKey), ...(recovery.error ? [`archive-error:${recovery.error}`] : [])] } : null;
  const allNotices = archiveNotice ? [...banners, archiveNotice] : banners;
  // Hidden archive items persist per host on this device; every other hidden notice stays with the view for this app session.
  const hiddenNotices = useMemo(() => [...(view.noticesHidden ?? []).filter((key) => !key.startsWith("archive:")), ...recovery.hidden], [view.noticesHidden, recovery.hidden]);
  const saveHidden = (keys: readonly string[]) => {
    updateView({ noticesHidden: keys.filter((key) => !key.startsWith("archive:")) });
    recovery.setHidden(keys.filter((key) => key.startsWith("archive:")));
  };
  // A notice missing while its source reads (first load, refresh, save, Refresh status) is unread, not absent; navigation is always read.
  const noticeRead: Readonly<Record<string, boolean>> = {
    source: state.status !== "idle" && state.status !== "loading",
    settings: organization?.state.status !== "loading",
    save: !organization?.state.saving,
    // The archive state still names the previous host for one render after a switch, so only this host's list counts,
    // and only a list whose every owner review was read may forget a hidden archive.
    archive: recovery.listed === host.id && recovery.complete && !recovery.checking,
    "archive-error": recovery.listed === host.id && !recovery.checking,
  };
  // A hidden notice a completed reading no longer reports is forgotten, so its return opens the tray; one continuously present stays hidden.
  // An acknowledged item still listed keeps its key; once a complete read lists it no longer, or in another state, it is forgotten.
  const presentNotices = presentHidden(allNotices, hiddenNotices, (key) => noticeRead[key.split(":")[0]!] ?? true, recovery.items.map(archiveAckKey));
  useEffect(() => { if (presentNotices !== hiddenNotices) saveHidden(presentNotices); }, [presentNotices, hiddenNotices]);
  const OptionsContainer = compact ? View : ScrollView;
  const toolbar = toolbarLayout({ collapsed: toolbarCollapsed, compact });
  // Hide and Show change presentation only: no action runs, and focus moves to the control that replaces the pressed one.
  const hideNoticeTray = (shown: readonly Notice[]) => {
    saveHidden(hideNotices(shown, hiddenNotices));
    setTimeout(() => focusControl(noticesButton.current), 0);
  };
  const showNoticeTray = (shown: readonly Notice[], hide: RefObject<View | null>) => {
    saveHidden(showNotices(shown, hiddenNotices));
    setTimeout(() => focusControl(hide.current), 0);
  };
  const noticeTray = (shown: readonly Notice[], hide: RefObject<View | null>, testID: string, disabled = false) => shown.length > 0 && !noticesHidden(shown, hiddenNotices) ? (
    <NoticeTray testID={testID} colors={colors} compact={compact} disabled={disabled} hideRef={hide} onHide={() => hideNoticeTray(shown)}>{shown.map((notice) => notice.node)}</NoticeTray>
  ) : null;
  const noticesTrigger = (shown: readonly Notice[], hide: RefObject<View | null>) => noticesHidden(shown, hiddenNotices) ? (
    <NoticesButton ref={noticesButton} notices={shown} colors={colors} compact={compact} onPress={() => showNoticeTray(shown, hide)} />
  ) : null;
  // One placement per layout: compact notices, the wide full toolbar (the slim row takes the archive line while it shows), or the wide slim row.
  const slimNotices = archiveNotice && toolbar.showSlimRow ? [archiveNotice] : [];
  const fullNotices = toolbar.showSlimRow ? banners : allNotices;
  const compactNotices = compact ? noticeTray(allNotices, noticesHide, "sessions-notices") : null;
  const scopeRemovals = [
    shortcut !== null ? { id: "desk", label: `Desk / workspace: ${scopeChoiceLabel}${shortcut.startsWith("desk-project:") ? ` · ${shortcut.slice("desk-project:".length)}` : ""}`, patch: { shortcut: null } } : null,
    streams !== null ? { id: "streams", label: `Workstreams: ${streams.length ? streams.map((key) => workstreamOptions.find((option) => option.key === key)?.label ?? `Unavailable (${key})`).slice(0, 2).join(", ") + (streams.length > 2 ? ` +${streams.length - 2}` : "") : "None"}`, patch: { streams: null } } : null,
    view.epic ? { id: "epic", label: `Epic: ${epicScopeLabel(view.epic)}${epicChoices.some((choice) => choice.key === epicScopeKey(view.epic)) ? "" : " (unavailable)"}`, patch: { epic: null } } : null,
  ].filter((choice) => choice !== null);
  const activeScopeSummary = scopeRemovals.length ? <View testID="sessions-active-scope" accessibilityLabel="Active session scope" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 4 }}>
    {scopeRemovals.map((choice) => <TextAction key={choice.id} testID={`sessions-remove-${choice.id}`} label={`${choice.label} ×`} accessibilityLabel={`Remove ${choice.label} scope`} colors={colors} compact={compact}
      onPress={() => { pinFilterInteraction(); updateView({ ...choice.patch, scrollOffset: 0 }); }} />)}
    <TextAction testID="sessions-clear-scope" label="Clear scope" colors={colors} compact={compact} onPress={() => { pinFilterInteraction(); updateView(clearSessionScopePatch()); }} />
  </View> : null;

  const fullToolbar = (
    <View
      testID="sessions-full-toolbar"
      onLayout={handleToolbarLayout}
      style={{ gap: 8 }}
    >
      {compact ? null : noticeTray(fullNotices, noticesHide, "sessions-top-notices", focusProps.disabled)}
      <View style={{ flexDirection: "row", alignItems: "center", flexWrap: "wrap", gap: 6 }}>
        {!toolbar.showSlimRow ? noticesTrigger(fullNotices, noticesHide) : null}
        {view.status === "attention" ? <>
          <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{availability.checking ? "Checking exact session IDs…" : "Attention includes unread results. Native record checks do not verify provider history."}</Text>
          <Action small={!compact} colors={colors} label="Check availability" onPress={() => { availability.refresh(); refresh(); }} />
        </> : null}
      </View>

      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        {scopeLabel ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 1 }}>
          <Text testID="sessions-heading" accessibilityRole="header" numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: compact ? 16 : 17, fontWeight: "600" }}>Sessions · {scopeLabel}</Text>
          {onClearScope ? <Action small={!compact} colors={colors} label="All sessions" accessibilityLabel={`Show all sessions instead of ${scopeLabel}`} disabled={!compact && toolbarCollapsed} onPress={onClearScope} /> : null}
        </View> : <Text testID="sessions-heading" accessibilityRole="header" style={{ color: colors.foreground, fontSize: compact ? 16 : 17, fontWeight: "600" }}>All sessions</Text>}
        <View accessibilityRole="toolbar" accessibilityLabel="Desk shortcuts" style={{ flexDirection: "row", flexWrap: "wrap", gap: 4, ...(compact ? { width: "100%" } : { marginLeft: "auto" }) }}>
          {SESSION_SHORTCUTS.map((choice, index) => <SessionScopeChip key={choice.label} testID={`sessions-shortcut-${index}`} choice={choice} count={shortcutCounts[index]!} active={shortcut === choice.id} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ shortcut: choice.id, scrollOffset: 0 }); }} />)}
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
            <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12, fontWeight: "600" }}>
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
            status={status}
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
          <View testID="sessions-work-options" style={{ gap: 6 }}>
            <OptionLine label="Work" colors={colors}>
              <FilterChip testID="sessions-work-mine" label="My work" compact={compact} active={work === "mine"} colors={colors} {...focusProps} onPress={() => chooseWork("mine")} />
              <FilterChip testID="sessions-work-all" label="All work" compact={compact} active={work === "all"} colors={colors} {...focusProps} onPress={() => chooseWork("all")} />
            </OptionLine>
            <OptionLine label="Assignment" colors={colors}>
              <FilterChip testID="sessions-assignment-any" label="Any assignment" compact={compact} active={assignment === "any"} colors={colors} {...focusProps} onPress={() => chooseAssignment("any")} />
              <FilterChip testID="sessions-assignment-me" label="Assigned to me" compact={compact} active={assignment === "me"} colors={colors} {...focusProps} onPress={() => chooseAssignment("me")} />
              <FilterChip testID="sessions-assignment-unknown" label="Unknown assignment" compact={compact} active={assignment === "unknown"} colors={colors} {...focusProps} onPress={() => chooseAssignment("unknown")} />
            </OptionLine>
            <OptionSelect testID="sessions-person" label="Assigned to" value={assignment.startsWith("person:") ? subjectLabel ?? "Person not in this projection" : "Choose a person…"} expanded={optionMenu === "person"} anchor={personRef} compact={compact} colors={colors} {...focusProps} disabled={focusProps.disabled || roster.length === 0} onPress={() => setOptionMenu(optionMenu === "person" ? null : "person")} />
            {/* While the projection loads or is unavailable the remembered choice is not known to be unset, so it never reads "Not set". */}
            <OptionSelect testID="sessions-working-as" label="Working as" value={realm === null ? (responsibility === null ? "Loading…" : "Unavailable") : workingAsLabel ?? (workingAs ? "Not in this projection" : "Not set")} expanded={optionMenu === "workingAs"} anchor={workingAsRef} compact={compact} colors={colors} {...focusProps} disabled={focusProps.disabled || roster.length === 0} onPress={() => setOptionMenu(optionMenu === "workingAs" ? null : "workingAs")} />
            <Text testID="sessions-responsibility-source" style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16 }}>{responsibilitySource}</Text>
          </View>
          {sessionLayout === "grouped" ? <OptionSelect testID="sessions-grouping" label="Group by" value={GROUPINGS.find(({ id }) => id === grouping)?.label ?? grouping} expanded={optionMenu === "grouping"} anchor={groupingRef} compact={compact} colors={colors} {...focusProps} onPress={() => setOptionMenu(optionMenu === "grouping" ? null : "grouping")} /> : null}
          <OptionSelect testID="sessions-sort" label="Sort by" value={sortLabel(sort)} expanded={optionMenu === "sort"} anchor={sortRef} compact={compact} colors={colors} {...focusProps} onPress={() => setOptionMenu(optionMenu === "sort" ? null : "sort")} />
          {sort !== "current" ? <OptionLine label="Direction" colors={colors}>
            {(sort === "title" ? ["asc", "desc"] as const : ["desc", "asc"] as const).map((direction) => <FilterChip key={direction} testID={`sessions-sort-${direction}`} label={sortDirectionLabel(sort, direction)} compact={compact} active={sortDirection === direction} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ sortDirection: direction, scrollOffset: 0 }); }} />)}
          </OptionLine> : null}
          <Text testID="sessions-sort-help" style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16 }}>{sort === "current" ? "Current order keeps state order, your saved arrangement and Role order." : "Sorted within each group and in results; groups keep their order. Missing times sort last. Choose Current order to arrange."}</Text>
          <WorkstreamSelector options={workstreamChoices} selected={streams} open={view.associationsOpen ?? false} registryLoaded={!!workstreamRegistry?.revision} about={aboutCounts}
            scopeCounts={assignmentCounts ?? undefined} scopeLabel={assignment === "me" ? "assigned to me" : assignment === "unknown" ? "unknown assignment" : `assigned to ${subjectLabel ?? "this person"}`}
            colors={colors} compact={compact} disabled={focusProps.disabled} onOpen={(associationsOpen) => updateView({ associationsOpen, scopeOpen: null })} onChange={chooseStreams}
            hidden={hiddenChoices} onShowAll={() => chooseWork("all")} />
          <SessionScopeSelector id="scope" label="Desk / workspace scope" selected={shortcut} open={view.scopeOpen === "desk"}
            options={scopeChoices.flatMap(({ id, label }) => id === null ? [] : [{ key: id, label, detail: deskScopes.find((choice) => choice.id === id)?.projectId }])}
            colors={colors} compact={compact} disabled={focusProps.disabled} onOpen={(open) => updateView({ scopeOpen: open ? "desk" : null, associationsOpen: false })}
            onChange={(shortcut) => { pinFilterInteraction(); updateView({ shortcut, scrollOffset: 0 }); }} />
          <SessionScopeSelector id="epic" label="Epic scope" options={epicChoices} selected={epicScopeKey(view.epic)} open={view.scopeOpen === "epic"}
            colors={colors} compact={compact} disabled={focusProps.disabled} onOpen={(open) => updateView({ scopeOpen: open ? "epic" : null, associationsOpen: false })}
            onChange={(key) => { pinFilterInteraction(); updateView({ epic: epicScopeChoice(key), scrollOffset: 0 }); }} />
          <OptionLine label="Epic lifecycle" colors={colors}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Not captured in session data; no lifecycle is inferred from idle or closed sessions.</Text>
          </OptionLine>
          <OptionLine label="More states" colors={colors}>
            {MORE_STATUS_FILTERS.map((status) => <FilterChip key={status} testID={`sessions-filter-${status}`} label={statusFilterLabel(status)} count={statusCounts[status]} status={status} compact={compact} active={view.status === status} colors={colors} {...focusProps} onPress={() => updateFilter({ status })} />)}
          </OptionLine>
          <ToggleRow
            testID="sessions-advanced-archived"
            label="Archived history"
            description="Only confirmed archived sessions. State, search and assignment still apply."
            checked={includeArchived}
            colors={colors}
            {...focusProps}
            onPress={() => updateFilter({ membership: includeArchived ? "sessions" : "archived" })}
          />
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <FilterChip testID="sessions-arrange" label={arranging ? "Done arranging" : "Arrange"} compact={compact} active={arranging} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || (!arranging && (narrowed || editsDisabled || sort !== "current"))} onPress={() => { pinFilterInteraction(); setOrderSelection(null); updateView({ arranging: !arranging }); }} />
            <FilterChip testID="sessions-reset-view" label="Reset view" compact={compact} active={false} colors={colors} {...focusProps} onPress={handleResetView} />
            <View style={{ flex: 1 }} />
            <FilterChip testID="sessions-expand-all" label="Expand all" compact={compact} active={false} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || resultMode} onPress={() => setAllGroups(true)} />
            <FilterChip testID="sessions-collapse-all" label="Collapse all" compact={compact} active={false} colors={colors} {...focusProps} disabled={(!compact && toolbarCollapsed) || resultMode} onPress={() => setAllGroups(false)} />
          </View>
          {arranging ? <View testID="sessions-arrange-help" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <Text style={{ flexBasis: "100%", color: colors.foregroundMuted, fontSize: 11 }}>{narrowed ? "Clear search and filters to arrange." : sort !== "current" ? "Choose Current order to arrange." : editsDisabled ? "Saved order is unavailable while organization settings are unavailable or saving." : "Select a handle, then Move up or Move down, or drag it among its siblings. Desks stay above epic work."}</Text>
            <Action small={!compact} colors={colors} label="↑ Move up" accessibilityLabel="Move selected row up" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(-1)} />
            <Action small={!compact} colors={colors} label="↓ Move down" accessibilityLabel="Move selected row down" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(1)} />
          </View> : null}
          <Text testID="sessions-options-count" style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 18 }}>{countSummary}{workspaceNote ? ` ${workspaceNote}.` : ""}</Text>
          <SessionsLegend colors={colors} compact={compact} contextual={peek !== null} open={view.legendOpen ?? false} onToggle={(legendOpen) => updateView({ legendOpen })} />
        </OptionsContainer>
      ) : null}
    </View>
  );

  // Group by and Workspace scope open as menus over the screen; a choice returns focus to its select.
  const optionAnchor = { grouping: groupingRef, sort: sortRef, person: personRef, workingAs: workingAsRef, none: sortRef }[optionMenu ?? "none"];
  const chooseOption = (choose: () => void) => {
    setOptionMenu(null);
    choose();
    (optionAnchor.current as unknown as { focus?: () => void } | null)?.focus?.();
  };
  const optionMenuLayer = optionMenu === null ? null : (
    <AnchoredMenu testID={`sessions-${optionMenu}-menu`} anchor={optionAnchor} root={rootRef} rootSize={rootSize} colors={colors}
      label={{ grouping: "Group sessions by", sort: "Sort sessions by", person: "Assigned to", workingAs: "Working as" }[optionMenu]} onClose={() => setOptionMenu(null)}
      placement={compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.6, 360) } : { align: "start", width: 240 }}>
      {/* The frame is clamped to the screen, so a short screen scrolls the choices instead of cutting off the last ones. */}
      <ScrollView testID={`sessions-${optionMenu}-choices`} keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
        {optionMenu === "grouping"
          ? GROUPINGS.map(({ id, label }) => <MenuItem key={id} label={label} tall={compact} selected={grouping === id} colors={colors} onPress={() => chooseOption(() => chooseGrouping(id))} />)
          : optionMenu === "sort"
          ? SESSION_SORTS.map(({ id, label }) => <MenuItem key={id} label={label} tall={compact} selected={sort === id} colors={colors} onPress={() => chooseOption(() => chooseSort(id))} />)
          : optionMenu === "person"
          ? roster.map(({ accountId, label }) => <MenuItem key={accountId} label={label} tall={compact} selected={assignment === `person:${accountId}`} colors={colors} onPress={() => chooseOption(() => chooseAssignment(`person:${accountId}`))} />)
          : optionMenu === "workingAs"
          ? [<MenuItem key="none" label={workingAs === null ? "Not set" : remembered?.persistence === "device" ? "Forget on this device" : "Forget"} tall={compact} selected={workingAs === null} colors={colors} onPress={() => chooseOption(() => chooseWorkingAs(null))} />,
            ...roster.map(({ accountId, label }) => <MenuItem key={accountId} label={label} tall={compact} selected={workingAs === accountId} colors={colors} onPress={() => chooseOption(() => chooseWorkingAs(accountId))} />)]
          : null}
      </ScrollView>
    </AnchoredMenu>
  );
  const detailsWhere = `${sessionLayout}|${grouping}|${listMode}`;
  const openDetails = (id: string) => {
    detailsWhereRef.current = detailsWhere;
    setDetailsId(id);
  };
  // One archive from the row menu: no sheet and no focus move; the toast and the row's own line report it, and Review
  // archive on the recovery line opens what stays unresolved.
  const archiveOne = (entry: DirectoryEntry) => {
    if (!canStart) return;
    lifecycle.clear();
    setArchivingId(entry.id);
    void lifecycle.prepare("archive", "filtered", [entry.id]).then((result) => {
      setArchivingId(null);
      const changed = entriesRef.current.some((current) => current.id === entry.id && current.updatedAt !== entry.updatedAt && current.archivedAt === null);
      if (changed && result && typeof result !== "string" && (result.outcome === "archived" || result.outcome === "already_archived" || result.outcome === "absent")) toast.show("The session changed while archiving. Showing its current native state.");
      else notifyArchive(toast, result);
    });
  };
  // Flat results and tree sessions share the dense row. Its menu holds favorites, the exact ID, Details, Show in tree
  // (results only), Select sessions and Archive; while selecting, the row is its own checkbox.
  const sessionListRow = (entry: DirectoryEntry, result: boolean) => {
    const id = entry.id, active = entry.archivedAt === null, selected = bulkSelection.selected.includes(id), starred = favorites.includes(id);
    const role = !result && grouping === "role" ? roleRowContext(entry, mappings) : null;
    const projection = retirement?.projections.get(id) ?? null;
    const retirementLabel = projection ? compact ? compactRetirementNotice(projection) : `Retirement ${retirementDispositionLabel(projection.retirement.disposition)}${projection.retirement.attention ? ", attention required" : ""}${projection.conflicts.length ? ", evidence conflict" : ""}` : null;
    const outcome = lifecycle.batch && lifecycle.review?.action === "archive" ? lifecycle.review.rows.find((row) => row.id === id) : undefined;
    const highlighted = !result && peek?.sessionId === id;
    const extras = sessionRowExtras(entry, role ? role.spoken : spokenProvenance(provenance.get(id) ?? sessionProvenance(entry, mappings)));
    const note = [extras.note, archivingId === id ? "Archiving…" : null].filter(Boolean).join(" · ") || null;
    const blocked = !active ? "Archived sessions are not selected for archive." : !canStart ? "Selection waits for the host and any running archive." : bulkOpen ? "The archive review is open."
      : !bulkScopeIds.includes(id) ? "Not among the shown sessions." : !selected && bulkSelection.selected.length >= ARCHIVE_BATCH_LIMIT ? `At most ${ARCHIVE_BATCH_LIMIT} sessions per archive.` : null;
    const actions: SessionAction[] = [
      ...ops.actions(entry),
      { key: "favorite", label: starred ? "Remove from favorites" : "Add to favorites", icon: "Star", unavailable: editsDisabled ? "Favorites are not writable right now." : null,
        perform: () => savePreference({ favorites: starred ? favorites.filter((value) => value !== id) : [...favorites, id] }) },
      { key: "copy-id", label: "Copy exact session ID", icon: "Copy", perform: () => copySessionId(entry, toast) },
      { key: "details", label: "Details", icon: "Info", opens: true, perform: () => openDetails(id) },
      ...(result ? [{ key: "show-in-tree", label: "Show in tree", icon: "ListTree", perform: () => handleShowInTree(entry) }] : []),
      { key: "select", label: "Select sessions", icon: "SquareCheck", unavailable: blocked, perform: () => selectForArchive(entry) },
      { key: "archive", label: "Archive", icon: "Archive", unavailable: !active ? "Already archived." : !canStart ? "Archive waits for the host and any running archive." : null, perform: () => archiveOne(entry) },
    ];
    const mode: DenseRowMode = bulkSelection.selecting ? { kind: "select", selected, unavailable: blocked, pending: lifecycle.running && lifecycle.batch?.currentId === id }
      // Focus returns to the menu after Details closes and after Back from Show in tree.
      : { kind: "open", actions: <SessionActionsMenu ref={(node) => { detailsButtonRef(id, node); treeButtonRef(id, node); }} entry={entry} colors={colors} actions={actions} /> };
    const feedback = outcome ?? recovery.items.find((item) => item.ref.agentId === id)?.row ?? undefined;
    const refreshable = !!outcome && (unresolvedArchive(outcome) || !!outcome.reconcileReviewId);
    // The 2 points between sessions sit above each row, and the lines under it render only when they say something:
    // an empty strip under a row would hold that session as the first in view while the reader sees the next one.
    return <View testID={result ? `sessions-result-${id}` : undefined} style={{ paddingTop: 2, gap: 2 }}>
      <DenseSessionRow testID={`sessions-row-${id}`} entry={entry} colors={colors} clock={rowClock} opened={opened.get(id) ?? null} highlighted={highlighted} totals={rowTotals.get(id) ?? null}
        note={note} spokenDetail={[extras.spoken, retirementLabel, highlighted ? "Shown from search results" : null].filter(Boolean).join(". ")}
        mode={mode} onPress={() => openOrSelect(entry)} onSelectStart={active ? () => selectForArchive(entry) : undefined} />
      {archiveFeedbackShown(feedback) || refreshable || (projection && compactRetirementNotice(projection)) ? <View style={{ paddingLeft: 40, gap: 2 }}>
        {/* A row still listed keeps its unresolved archive's state after the batch that sent it is gone. */}
        <ArchiveRowFeedback row={feedback} colors={colors} />
        {refreshable ? <Action small colors={colors} label="Refresh status" accessibilityLabel={`Refresh archive status: ${entry.title} (${id})`}
          disabled={!canStart} onPress={() => { if (canStart) void lifecycle.reconcile(id); }} /> : null}
        {/* Only a conflict or needed review adds a line, at every width; the full retirement read is in Details. */}
        {projection ? <CompactRetirementNotice projection={projection} colors={colors} /> : null}
      </View> : null}
    </View>;
  };
  const detailsEntry = detailsId === null || detailsWhereRef.current !== detailsWhere ? null : availability.entries.find((entry) => entry.id === detailsId) ?? null;
  const detailsWork = detailsEntry ? sessionWork(detailsEntry) : { task: null, pr: null };
  const closeDetails = () => {
    const target = detailsReturn();
    setDetailsId(null);
    target?.focus?.();
    // Restoring focus must not cover the next touch target with help.
    detailsHelp.dismiss();
  };
  // Details whose session has left, or whose grouping or list mode has changed, close. Focus they held,
  // which the page has now dropped, goes where Close would send it; focus elsewhere stays put.
  useEffect(() => {
    if (detailsId === null || detailsEntry !== null) return;
    const document = pageDocument();
    const target = document && (document.activeElement == null || document.activeElement === document.body) ? detailsReturn() : null;
    setDetailsId(null);
    target?.focus?.();
    if (target) detailsHelp.dismiss();
  }, [detailsId, detailsEntry === null]);
  // Over the list on a compact screen, so a short one still shows the title, Close and a scrolling chain.
  const detailsLayer = detailsEntry === null ? null : (
    <AnchoredMenu testID="sessions-details" dialog anchor={detailsAnchor} root={rootRef} rootSize={rootSize} colors={colors}
      label={`Details: ${detailsEntry.title}`} onClose={closeDetails}
      placement={compact ? { align: "stretch", width: 0, cover: true } : { align: "end", width: 360, maxHeight: 440 }}
      header={<View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, paddingLeft: 6, paddingBottom: 4 }}>
        <Text testID="sessions-details-title" selectable style={{ flex: 1, minWidth: 0, paddingTop: 6, color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{detailsEntry.title}</Text>
        <Action ref={detailsCloseRef} small={!compact} colors={colors} label="Close" icon="X" iconOnly help="Close details" accessibilityLabel={`Close details: ${detailsEntry.title}`} onPress={closeDetails} />
      </View>}>
      {/* On web the lines are a named stop of their own, so a keyboard can scroll a chain longer than the screen. */}
      <ScrollView testID="sessions-details-lines" {...(pageDocument() ? { tabIndex: 0, role: "region", accessibilityLabel: "Model, reported usage, timing and placement" } as const : {})}
        onFocus={() => setDetailsLinesFocused(true)} onBlur={() => setDetailsLinesFocused(false)}
        style={{ flexGrow: 0, flexShrink: 1, borderRadius: 6, ...(detailsLinesFocused ? { outlineColor: colors.accent, outlineStyle: "solid", outlineWidth: 2 } as const : null) }}>
        <SessionUsageLines entry={detailsEntry} colors={colors} stale={showBanner} />
        <SessionDetailSection title="Session totals" values={[totalsDetail(rowTotals.get(detailsEntry.id))]} colors={colors} />
        <SessionDetailSection title="Timing" colors={colors}>
          <SessionTimeLines entry={detailsEntry} freshness={{ live: timesLive, readAt: state.lastUpdatedAt }} colors={colors} />
        </SessionDetailSection>
        <SessionDetailSection title="Role and placement" values={sessionDetailLines(detailsEntry, mappings, planFallbackNote(workstreamSources, workstreamReads, declaredScope(detailsEntry, mappings).epic, detailsEntry.projectKey, preferences.planAssociations[detailsEntry.id]), derivedSource(detailsEntry))} colors={colors} />
        <SessionDetailSection title="Native status and availability" values={sessionRowDetails(detailsEntry)} colors={colors} />
        {detailsWork.task || detailsWork.pr ? <SessionDetailSection title="Task and pull request" colors={colors} values={[
          ...(detailsWork.task ? [{ label: "Task", value: detailsWork.task }] : []), ...(detailsWork.pr ? [{ label: "Pull request", value: detailsWork.pr }] : [])]} /> : null}
        {retirement?.projections.get(detailsEntry.id) ? <SessionRetirementDetails projection={retirement.projections.get(detailsEntry.id)!} colors={colors} /> : null}
        {responsibilityById?.has(detailsEntry.id) ? <SessionDetailSection title="Assignment" colors={colors} values={[
          { label: assignment === "me" ? `Assigned to me, working as ${subjectLabel}` : assignment === "unknown" ? `Unknown assignment for ${subjectLabel}` : `Assigned to ${subjectLabel}`, value: responsibilityLine(responsibilityById.get(detailsEntry.id)!) },
          { label: "Source", value: `${projection!.realm} responsibility projection, observed ${projection!.generatedAt}` },
        ]} /> : null}
        <SessionMetadataGuide key={detailsEntry.id} entry={detailsEntry} colors={colors} />
      </ScrollView>
    </AnchoredMenu>
  );
  const handleRootLayout = (event: LayoutChangeEvent) => {
    const { width, height } = event.nativeEvent.layout;
    if (width !== rootSize.width || height !== rootSize.height) setRootSize({ width, height });
  };

  const mobileToolbar = <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
    <View style={{ flex: 1, minWidth: 0 }}>
    {/* The toolbar sizes its whole tiles from the room this column gets beside Attention's Check IDs. */}
    <MobileQuickToolbar key={viewScope} colors={colors}
    status={view.status} counts={statusCounts} activeCount={slimActiveCount}
    filtersTriggerRef={filtersTriggerRef} chooserRef={quickChooserRef} chooserOpen={quickChooserScope === viewScope} helpDismissRef={quickHelpDismissRef}
    onStatus={(status) => updateFilter({ status })} onFilters={handleRevealFilters}
    onChooser={() => { cancelDelayedOpen(); searchInputRef.current?.blur(); setQuickChooserScope(viewScope); }} />
    </View>
    {view.status === "attention" ? <View style={{ width: 80, alignItems: "flex-end" }}>
      <Action small colors={colors} label="Check IDs" accessibilityLabel="Check availability" disabled={!hostOnline || availability.checking} onPress={() => { availability.refresh(); refresh(); }} />
    </View> : null}
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
        notice={noticesTrigger(slimNotices, slimNoticesHide)}
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
      {noticeTray(slimNotices, slimNoticesHide, "sessions-slim-notices")}
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

  // A hidden tray leaves its button where the tray was.
  const filtersNoticesTrigger = noticesTrigger(allNotices, noticesHide);
  if (mobileFiltersOpen) return (
    <View ref={rootRef} onLayout={handleRootLayout} style={{ flex: 1, minHeight: 0, backgroundColor: colors.surface0 }}>
      <MobileFiltersPage colors={colors} onBack={closeMobileFilters} count={filterCount} menuOpen={optionMenu !== null || (view.associationsOpen ?? false) || view.scopeOpen != null}>
        {compactNotices}
        {filtersNoticesTrigger ? <View style={{ alignItems: "flex-start" }}>{filtersNoticesTrigger}</View> : null}
        {activeScopeSummary}
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
        {activeScopeSummary}
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
  // Only the work scope and assignment narrow this view: say which, and offer the one way back that widens it.
  const scopeParts = [work === "mine" ? "My work" : null, assignmentPart].filter((part): part is string => part !== null);
  const onlyScope = scopeParts.length > 0 && activeCount === scopeParts.length;
  const emptyMessage = assignmentBlocked
    ? `No sessions shown: ${assignmentPart} is not evaluated.`
    : task && candidates.length === 0
    ? `No session in this view is linked to ${task.label}.`
    : view.status === "recent"
    ? recentEmptyMessage(recent.record)
    : onlyScope
    ? `No sessions in ${scopeParts.join(", ")} in this view.`
    : view.status === "provider_blocked" && blockedView.empty
    ? blockedView.empty
    : clearable
      ? "No sessions match the current search and filters."
      : state.status === "error" || state.source === "unavailable"
        ? "No sessions could be read from this host."
        : state.inventoryComplete === false
          ? "No sessions could be read from this host's partial inventory."
          : "No sessions in this host's scope.";
  const peekTitle = peek ? state.entries.find((entry) => entry.id === peek.sessionId)?.title ?? "This session" : "";
  const shortScreen = compact && rootSize.height > 0 && rootSize.height < SHORT_PEEK_HEIGHT;
  const shortPeek = peek !== null && shortScreen;
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
  const plural = (count: number) => `${count} session${count === 1 ? "" : "s"}`;
  const reasonText = workTally ? [...workTally.reasons].map(([reason, count]) => `${count} ${MATCH_REASONS[reason].toLocaleLowerCase()}`).join(", ") : "";
  // Always reachable outside Filters: the scope, any assignment filter and why it shows what it shows, and the ways back.
  const scopeMessage = work === "mine" ? "My work: every session on this host" : "All work: every session on this host, with empty workspaces and plan-only choices";
  const assignmentMessage = assignmentState === "any" ? null
    : assignmentState === "unavailable" ? `${assignmentPart} cannot be evaluated on this host: ${responsibility === null ? "the responsibility projection is not read yet." : responsibility.diagnostics.join(" ") || "the responsibility projection is unavailable."}`
    : assignmentState === "expired" ? `${assignmentPart} is not evaluated: the responsibility projection expired ${readTime(projection!.expiresAt, clock.now)}.`
    : assignmentState === "no-profile" ? `${assignmentPart} needs Working as: choose who you are working as in Filters. It is not sign-in.`
    : assignmentState === "unknown-person" ? `${assignmentPart} is not evaluated: that person is not in the current responsibility projection.`
    : assignment === "unknown" ? `Unknown assignment for ${subjectLabel}: ${plural(workTally!.unknown)} here with missing or unknown assignment data.`
    : `${assignment === "me" ? `Assigned to me, working as ${subjectLabel}` : `Assigned to ${subjectLabel}`}: ${plural(workTally!.matched)} here${reasonText ? ` (${reasonText})` : ""} · ${workTally!.unknown} with unknown assignment.`;
  const workMessage = assignmentMessage === null ? `${scopeMessage}.` : `${work === "mine" ? "My work" : "All work"} · ${assignmentMessage}`;
  const workLine = (
    <View testID="sessions-work" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 4, paddingHorizontal: 4, paddingBottom: 6 }}>
      <Text testID="sessions-work-message" accessibilityLiveRegion="polite" style={{ position: "absolute", width: 1, height: 1, overflow: "hidden" }}>{workMessage}</Text>
      {work === "all" ? <TextAction testID="sessions-work-show-mine" label="My work" accessibilityLabel="Show My work" colors={colors} compact={compact} onPress={() => chooseWork("mine")} />
        : <TextAction testID="sessions-work-show-all" label="Show All work" colors={colors} compact={compact} onPress={() => chooseWork("all")} />}
      {assignment !== "any" ? <TextAction testID="sessions-assignment-show-any" label="Any assignment" accessibilityLabel="Show any assignment" colors={colors} compact={compact} onPress={() => chooseAssignment("any")} /> : null}
      {assignmentState === "no-profile" ? <TextAction testID="sessions-work-choose" label="Choose Working as" colors={colors} compact={compact} onPress={handleRevealFilters} /> : null}
      {assignmentState === "ready" && assignment !== "unknown" && workTally!.unknown > 0 ? <TextAction testID="sessions-assignment-show-unknown" label="Show unknown assignment" accessibilityLabel="Show sessions whose assignment is unknown" colors={colors} compact={compact} onPress={() => chooseAssignment("unknown")} /> : null}
      {compact ? <Pressable testID="sessions-mobile-clear-filters" accessibilityRole="button" accessibilityLabel="Clear filters"
        accessibilityState={{ disabled: activeCount === 0 }} aria-disabled={activeCount === 0} disabled={activeCount === 0}
        accessibilityHint="Clears all search and filter choices; keeps this context and view preferences" onPress={handleClearFilters}
        style={{ minHeight: FILTERS_TOUCH_TARGET, paddingHorizontal: 6, justifyContent: "center", opacity: activeCount === 0 ? 0.45 : 1 }}>
        <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 13, fontWeight: "600" }}>Clear filters</Text>
      </Pressable> : null}
    </View>
  );
  // On short screens the utility row must scroll, so it cannot consume the list's last usable pixels.
  // One canonical readback of applied refinements, wrapped in full. A state other than the default Last visited and the
  // unrefined All is named here too, since a phone's state controls show it only as a glyph or under More; layout has its own control.
  const appliedParts = [...new Set([scopeLabel, work === "mine" ? "My work" : null,
    view.status === DEFAULT_SESSION_FILTERS.status || view.status === ALL_STATUS_FILTER ? null : statusFilterLabel(view.status),
    assignmentPart && (assignment === "me" || assignment === "unknown") ? `${assignmentPart}${subjectLabel ? ` (${subjectLabel})` : ""}` : assignmentPart,
    view.query.trim() ? `“${view.query.trim().replace(/\s+/g, " ")}”` : null, task?.label,
    compact ? scopeChoiceLabel : null, !compact || streams === null ? null : streams.length ? streams.map((key) => workstreamOptions.find((option) => option.key === key)?.label ?? `Unavailable (${key})`).join(", ") : "No associations",
    compact && view.epic ? `${epicScopeLabel(view.epic)}${epicChoices.some((choice) => choice.key === epicScopeKey(view.epic)) ? "" : " (unavailable)"}` : null, includeArchived ? "Archived" : null, sortPart,
  ].filter((part): part is string => !!part))];
  const appliedSummary = appliedParts.length ? <Text testID="sessions-applied-filters" accessibilityLiveRegion="polite"
    accessibilityLabel={`${appliedParts.join(". ")}. ${filterCount.label}`}
    style={{ color: assignmentBlocked ? colors.statusWarning : colors.foregroundMuted, fontSize: 12, lineHeight: 17, paddingHorizontal: 4 }}>{appliedParts.join(" • ")}</Text> : null;
  const layoutBar = <View testID="sessions-layout-bar" style={{ flexDirection: compact ? "column" : "row", flexWrap: "wrap", alignItems: compact ? "stretch" : "center", gap: 4 }}>
    {appliedSummary ? <View style={{ flexGrow: compact ? 0 : 1 }}>{appliedSummary}</View> : null}
    <View testID="sessions-view-controls" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 10, rowGap: 4 }}>
      {compact ? <CompactSearch key={viewScope} colors={colors} query={view.query} searchInputRef={searchInputRef} onQuery={(query) => updateFilter({ query })} /> : null}
      {/* Where Resume stood: the row menu's actions entry for the sessions shown; selecting then shows board D1's header and bar. */}
      {(!includeArchived || lifecycle.batch) && !bulkSelection.selecting ? <SessionActionsMenu ref={bulkToggleRef} entry={TOOLBAR_SUBJECT} colors={colors} actions={[{ key: "select", label: "Select sessions", icon: "SquareCheck",
        unavailable: !canStart ? "Selection waits for the host and any running archive." : bulkOpen ? "The archive review is open." : includeArchived ? "Archived sessions are not selected for archive." : null,
        perform: bulkSelection.start }]} /> : null}
      <SessionLayoutControl value={sessionLayout} colors={colors} onChange={chooseLayout}
        disabled={view.status === "recent" ? "Recent lists sessions in the order you opened them, in one list. Layout applies to the other filters and stays saved." : undefined} />
      {compact ? noticesTrigger(allNotices, noticesHide) : null}
    </View>
  </View>;
  // Archived obligations stay reachable from every membership, search and scope state, including an
  // empty Archived result. The index is derived only from the full host projection, never from the
  // current filtered rows; it shows exact native identity and opens the same read-only details, and
  // it never adds archived sessions to the ordinary count or tree.
  const retirementConflicts = !retirement ? [] : [...retirement.projections.values()].filter((projection) => {
    const entry = availability.entries.find((candidate) => candidate.id === projection.sessionId);
    return !!entry && projection.native.archiveMembership === "archived" && projection.conflicts.some((conflict) => conflict.kind.startsWith("archived-with-"));
  });
  const retirementConflictIndex = retirementConflicts.length === 0 ? null : (
    <View testID="sessions-retirement-conflicts" style={{ gap: 4, paddingHorizontal: 4, paddingVertical: 2 }}>
      <Text accessibilityRole="header" style={{ color: colors.statusWarning, fontSize: 12, fontWeight: "600" }}>Archived conflicts</Text>
      {retirementConflicts.map((projection) => (
        <View key={projection.sessionId} testID={`sessions-retirement-conflict-${projection.sessionId}`} style={{ gap: 2 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text selectable style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: 12 }}>{`${projection.sessionId} · native ${projection.native.status}`}</Text>
            <SessionDetailsButton title={projection.sessionId} expanded={detailsId === projection.sessionId} compact={compact} colors={colors}
              ref={(node) => detailsButtonRef(projection.sessionId, node)} onPress={() => openDetails(projection.sessionId)} />
          </View>
          <SessionRetirementConflict conflicts={projection.conflicts} colors={colors} />
        </View>
      ))}
    </View>
  );
  // Unchecked error sessions are counted, not listed as blocked; Error is the one way to see them.
  const providerNotice = view.status === "provider_blocked" && blockedView.notice ? <View testID="sessions-provider-notice" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6, paddingHorizontal: 4 }}>
    <Text style={{ flexShrink: 1, color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{blockedView.notice}</Text>
    <TextAction testID="sessions-provider-show-error" label="Show Error" accessibilityLabel="Show all error sessions" colors={colors} compact={compact} onPress={() => updateFilter({ status: "error" })} />
  </View> : null;
  const recentFooter = view.status === "recent" && !peek ? <RecentFooter recent={recent} known={availability.entries} candidates={candidates} shown={results.length}
    colors={colors} tall={compact || COARSE_POINTER} onOpen={handleOpen} /> : null;
  // Only a workspace list failure stays below the list; the routine read note is in Filters.
  const workspaceFooter = state.entries.length === 0 || !workspaceNote?.startsWith(WORKSPACE_LIST_ERROR) ? null
    : <Text testID="sessions-footer" style={{ color: colors.foregroundMuted, fontSize: 11, paddingVertical: 12, paddingHorizontal: 4 }}>{workspaceNote}.</Text>;
  const promotionCoverageNotice = retirement?.promotionRead.state === "unknown" ? (
    <View testID="sessions-promotion-coverage" style={{ paddingHorizontal: 4, paddingVertical: 2 }}>
      <Text style={{ color: colors.statusWarning, fontSize: 12 }}>{`Promotion evidence unavailable: ${retirement.promotionRead.error ?? "the host did not report a readable promotion list"}.`}</Text>
    </View>
  ) : null;
  const listHeader = peek && !shortScreen ? null : <>{peek ? null : retirementConflictIndex}{peek ? null : promotionCoverageNotice}{shortScreen ? <>{layoutBar}{peek ? null : workLine}{peek ? null : providerNotice}</> : <>{workLine}{providerNotice}</>}</>;
  withHeaderRef.current = listHeader !== null;

  return (
    <View
      ref={rootRef}
      onLayout={handleRootLayout}
      tabIndex={-1}
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
      {compact ? null : activeScopeSummary}
      {compact ? null : fullToolbarSlot}
      {peekBar}
      {shortScreen ? null : layoutBar}
      {bulkSelection.selecting ? <SelectionHeader count={bulkSelection.selected.length} total={bulkScopeIds.length} hidden={bulkSelection.hidden} colors={colors} onDone={bulkSelection.clear}
        selectAll={{ checked: allSelected, onPress: () => allSelected ? bulkSelection.remove(bulkScopeIds) : bulkSelection.selectAll(bulkScopeIds),
          unavailable: allSelected ? null : !canStart || bulkOpen ? "Selection waits for the host and any running archive." : !bulkScopeIds.length ? "No session matches."
            : !(rawState.status === "ready" && rawState.inventoryComplete === true) ? "The session list is not complete yet; select sessions one by one."
            : new Set([...bulkSelection.selected, ...bulkScopeIds]).size > ARCHIVE_BATCH_LIMIT ? `More than ${ARCHIVE_BATCH_LIMIT} sessions: narrow the view, or select one by one.` : null }} /> : null}
      {lifecycle.batch ? <BulkArchiveToolbar selection={bulkSelection} lifecycle={lifecycle} colors={colors} maxHeight={shortScreen ? 2 * FILTERS_TOUCH_TARGET : undefined} announce={!bulkOpen} /> : null}
      <FlatList
        ref={listRef}
        testID="sessions-list"
        tabIndex={pageDocument() ? -1 : undefined}
        data={rows}
        keyExtractor={listRowKey}
        getItemLayout={(data, index) => rowLayout.layout(data as readonly ListRow[], index, listHeader !== null)}
        CellRendererComponent={rowLayout.Cell}
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
        onLayout={(event) => {
          viewportHeightRef.current = event.nativeEvent.layout.height;
          resizedRef.current();
          if (compact) handleListLayout(event);
        }}
        onScroll={handleListScroll}
        onScrollBeginDrag={handleScrollBeginDrag}
        onScrollEndDrag={handleScrollSettle}
        onMomentumScrollBegin={handleReaderBegin}
        onMomentumScrollEnd={handleScrollSettle}
        onTouchStart={handleReaderBegin}
        onTouchEnd={handleScrollSettle}
        onTouchCancel={handleScrollSettle}
        onContentSizeChange={(_width, height) => {
          contentHeightRef.current = height;
          resizedRef.current();
        }}
        // Session taps remain usable while the compact search keyboard is open.
        keyboardShouldPersistTaps={compact ? "handled" : undefined}
        // No platform anchoring: it follows the first visible row, so a live re-sort of that row jumps the list.
        // The screen keeps the reader's place itself; see createRowLayout.
        style={pageDocument() ? NO_SCROLL_ANCHOR : undefined}
        scrollEventThrottle={16}
        contentContainerStyle={rows.length === 0 ? { flexGrow: 1, ...scrollSlack } : { paddingBottom: 16, ...scrollSlack }}
        ListHeaderComponent={listHeader === null ? null : <View onLayout={rowLayout.onHeaderLayout}>{listHeader}</View>}
        renderItem={({ item }) => {
          const row = item.kind === "result" || item.kind === "session" ? sessionListRow(item.entry, item.kind === "result") : null;
          if (item.kind === "result") return row;
          const key = listRowKey(item);
          if (item.kind === "empty") return <OrganizationRowView row={item} colors={colors} compact={compact} disabled arranging={false} selected={false} favorite={false} onToggle={() => undefined} onMove={() => undefined} onSelect={() => undefined} />;
          const epic = item.kind === "group" && item.group.kind === "epic" ? item.group.epic : null;
          const plans = epic ? planSources?.[epic.toLocaleUpperCase()] ?? [] : [];
          const actions = epic && onEpicTab && plans.length > 0 ? (["Plan", "Board", "Graph"] as const).map((tab) => <Action key={tab} small={!compact} colors={colors} label={tab}
            accessibilityLabel={`${tab} for ${epic}${plans.length > 1 ? `, choose among ${plans.length} configured plans` : ""}`} onPress={() => onEpicTab(tab, epic, plans.length === 1 ? plans[0]! : null)} />) : undefined;
          // The row menu owns favorites, Details and Archive, so the tree adds only its indent, guides and arrange handle.
          return <OrganizationRowView row={item} colors={colors} compact={compact} contextual={peek !== null} disabled={editsDisabled} arranging={arrangeEnabled} selected={orderSelection === key}
            favorite={item.kind === "session" && favorites.includes(item.entry.id)} stale={showBanner} actions={actions}
            onToggle={() => toggleGroup(item)}
            onSelect={() => setOrderSelection(orderSelection === key ? null : key)}
            onMove={(distance) => moveRow(item, distance)}>
            {row}
          </OrganizationRowView>;
        }}
        ListEmptyComponent={
          <View
            testID="sessions-empty"
            style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 20 }}
          >
            <Text style={{ color: colors.foreground, fontSize: 14, textAlign: "center" }}>{emptyMessage}</Text>
            {view.status === "recent" && !assignmentBlocked && !(task && candidates.length === 0) ? (
              <View style={{ flexDirection: "row", flexWrap: "wrap", justifyContent: "center", gap: 8 }}>
                <TextAction testID="sessions-empty-current" label="Show Current" colors={colors} compact={compact} onPress={() => updateFilter({ status: "current" })} />
                <TextAction testID="sessions-empty-all" label="Show All" colors={colors} compact={compact} onPress={() => updateFilter({ status: "all" })} />
              </View>
            ) : assignmentBlocked || (onlyScope && assignment !== "any") ? (
              <TextAction testID="sessions-empty-any-assignment" label="Any assignment" accessibilityLabel="Show any assignment" colors={colors} compact={compact} onPress={() => chooseAssignment("any")} />
            ) : onlyScope ? (
              <TextAction testID="sessions-empty-all-work" label="Show All work" colors={colors} compact={compact} onPress={() => chooseWork("all")} />
            ) : clearable ? (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Clear filters"
                onPress={handleClearFilters}
                style={{ minHeight: compact || COARSE_POINTER ? FILTERS_TOUCH_TARGET : 32, justifyContent: "center", paddingHorizontal: 8 }}
              >
                <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 13, fontWeight: "600" }}>
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
                <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 13, fontWeight: "600" }}>
                  Retry
                </Text>
              </Pressable>
            )}
          </View>
        }
        ListFooterComponent={recentFooter || workspaceFooter ? <>{recentFooter}{workspaceFooter}</> : null}
      />
      {/* Board D1's sticky bar: the frozen selection's two actions; otherwise an unfinished takeover's way back to its results. */}
      {bulkSelection.selecting ? null : ops.notice}
      {bulkSelection.selecting ? <SelectionBar count={bulkSelection.selected.length} colors={colors} archiveRef={bulkReviewRef} disclosure={ops.takeover(selectedSeats).disclosure}
        takeover={{ unavailable: ops.takeover(selectedSeats).unavailable, onPress: ops.takeover(selectedSeats).start }}
        auto={{ unavailable: ops.takeover(selectedSeats).auto, onPress: ops.takeover(selectedSeats).choose }}
        archive={{ unavailable: !canStart ? "Archive waits for the host and any running archive." : bulkOpen ? "The archive review is open." : includeArchived ? "Leave Archived to archive sessions." : null,
          onPress: () => { if (!canStart || bulkOpen || !bulkSelection.selected.length) return; lifecycle.clear(); setBulkOpen(true); void lifecycle.prepare("archive", "filtered", [...bulkSelection.selected], true); } }} /> : null}
      {ops.layer}
      {optionMenuLayer}
      {detailsLayer}
      {quickChooserLayer}
      {bulkOpen ? <BulkArchiveDialog lifecycle={lifecycle} root={rootRef} anchor={bulkReviewRef} rootSize={rootSize} colors={colors} compact={compact}
        onClose={(archived) => {
          // Archived sessions leave the frozen selection; a blocked one stays for the batch's follow-ups.
          bulkSelection.remove(archived);
          bulkReturnFocusRef.current = true; setBulkOpen(false);
        }}
        onFinished={(summary) => toast.show(summary)} /> : null}
      {recoveryOpen ? <ArchiveRecoverySheet recovery={{ ...recovery, items: recovery.attention }} root={rootRef} rootSize={rootSize} colors={colors} compact={compact}
        canRefresh={hostOnline && lifecycleIdle} hostOnline={hostOnline} onOpen={openRecovered} onHide={hideRecovered} onClose={closeRecovery} /> : null}
    </View>
  );
}
