import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import {
  FlatList,
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
  hasActiveView,
  matchesQuery,
  matchesStatus,
  MORE_STATUS_FILTERS,
  QUICK_STATUS_FILTERS,
  statusFilterLabel,
  type SessionFilters,
  type StatusFilter,
} from "./session-view";
import {
  patchLocalExpansion,
  patchSessionView,
  readLocalExpansion,
  readSessionView,
  resetSessionView,
  writeScrollOffset,
  type SessionGrouping,
  type SessionViewState,
} from "./session-view-store";
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
import { allGroupKeys, buildOrganizationTree, defaultGroupExpanded, filterOrganizationTree, flattenOrganizationTree, inEpicScope, isDeskWork, organizationScope, sessionState, visibleSessions, workstreamRank, type OrganizationGroup, type OrganizationRow, type OrganizationWorkspace } from "./session-organization";
import { rowKey, siblingKeys, reorderSavedSiblingOrder } from "./organization-order";
import { OrganizationRowView, StateBadge, stateColor as sessionStateColor, WORKSPACE_KINDS } from "./organization-row";
import { Action, Disclosure } from "./page-controls";

type ThemeColors = PluginTheme["colors"];

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
  const workstream = organizationScope(entry, mappings).workstream.trim().toLocaleLowerCase();
  switch (shortcut) {
    case null:
      return true;
    case "PR Review":
      return workstream === "pr review" || workstream === "pr-review";
    case "Adam":
      return workstream === "adam";
    case "Desks":
      return isDeskWork(entry, mappings);
    case "Other work":
      return !isDeskWork(entry, mappings);
    default:
      return workstream === shortcut.trim().toLocaleLowerCase();
  }
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
}): string[] {
  const query = input.query.trim();
  return [
    query ? `“${query}”` : null,
    input.status !== "all" ? statusFilterLabel(input.status) : null,
    input.shortcut !== null ? shortcutLabel(input.shortcut) : null,
    input.streams !== null ? `${input.streams.length} workstream${input.streams.length === 1 ? "" : "s"}` : null,
    input.includeArchived ? "Archived included" : null,
  ].filter((part): part is string => part !== null);
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
};

const SessionRow = memo(function SessionRow({
  entry,
  colors,
  compact,
  onPress,
  displayTitle,
  displayContext,
}: SessionRowProps) {
  const attention = attentionLabel(entry);
  const archived = archivedLabel(entry);
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
    archived,
    detail,
  ]
    .filter((part): part is string => part !== null && part.length > 0)
    .join(". ");
  const meta = [displayContext, archived].filter((part): part is string => !!part).join(" · ");
  const runtime = (state === "needs_input" || state === "attention" || state === "failed") && entry.status !== "error";
  const runtimeLabel = entry.status.charAt(0).toLocaleUpperCase() + entry.status.slice(1);
  // Compact: one line. The title leads, runtime/context/archived follow muted in
  // the same truncating line, and the state badge stays visible at the end. The
  // native ID stays in the accessible label only (review ST-02).
  const compactMeta = [runtime ? runtimeLabel : null, displayContext, archived]
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
        minHeight: compact ? 44 : 40,
        paddingLeft: 8,
        paddingRight: 4,
        paddingVertical: compact ? 4 : 5,
        flexDirection: "row",
        alignItems: "center",
        gap: compact ? 8 : 10,
        borderRadius: 5,
        backgroundColor: pressed || hovered ? colors.surface2 : undefined,
      })}
    >
      {compact ? (
        <>
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
          <StateBadge state={state} colors={colors} compact={false} />
        </>
      ) : (
        <>
          <View style={{ flex: 1, minWidth: 0 }}>
            <Text
              numberOfLines={1}
              style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}
            >
              {displayTitle ?? entry.title}
            </Text>
            {meta ? (
              <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11, marginTop: 1 }}>
                {meta}
              </Text>
            ) : null}
          </View>
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

export function FiltersButton({
  activeCount,
  expanded,
  colors,
  compact,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
}: {
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
      testID={FILTERS_TOGGLE_TEST_ID}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityState={{ expanded }}
      accessibilityLabel={filtersAccessibilityLabel(activeCount)}
      accessibilityHint={expanded ? "Collapses filter options" : "Expands filter options"}
      hitSlop={4}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: compact ? FILTERS_TOUCH_TARGET : 36,
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
      accessibilityHint="Clears the state, desk, workstream and archive filters"
      hitSlop={4}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: compact ? FILTERS_TOUCH_TARGET : 32,
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
          accessibilityHint="Clears the search, state, desk, workstream and archive filters"
          onPress={onClear}
          style={pressableStyle}
        >
          {chip(parts.join(" · "), true)}
        </Pressable>
      ) : null}
    </View>
  );
}

function FilterChip({
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
      accessibilityLabel={count === undefined ? label : `${label}, ${count}`}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: compact ? 36 : 30,
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
export function SessionsScreen({ host, layout, navigation, theme, directory, organization, projectId = null, onIncludeArchived, workspaces, workspaceNote, epicKey = null, viewScope = host.id, scopeLabel = null, onClearScope, planSources, onEpicTab }: SessionsScreenProps) {
  const colors = theme.colors;
  const compact = layout.compact;
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
  const { state, refresh } = directory ?? localDirectory;
  const preferences = organization?.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const shortcut = view.shortcut ?? null;
  const grouping: SessionGrouping = view.grouping ?? preferences.grouping;
  const streams = view.streams ?? null;
  const arranging = view.arranging ?? false;
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
  const [, setExpansionRevision] = useState(0);
  const [orderSelection, setOrderSelection] = useState<string | null>(null);
  const editsDisabled = !hostOnline || organization?.state.status !== "ready" || organization.state.saving;
  useEffect(() => { onIncludeArchived?.(view.includeArchived); }, [view.includeArchived, onIncludeArchived]);
  const savePreference = (patch: Partial<OrganizationSettings>) => {
    if (!organization || editsDisabled) return;
    void organization.save(patch).then((saved) => setSaveMessage(saved ? null : "Organization was not saved. Reload preferences in Settings and review the conflict; no newer revision was overwritten."));
  };
  const toast = useToast();
  const listRef = useRef<NativeFlatList<OrganizationRow> | null>(null);
  const entriesRef = useRef(state.entries);
  entriesRef.current = state.entries;
  const restoredRef = useRef(false);
  const [toolbarCollapsed, setToolbarCollapsed] = useState(
    () => initialToolbarCollapsed(readSessionView(viewScope), compact),
  );
  const toolbarCollapsedRef = useRef(toolbarCollapsed);
  toolbarCollapsedRef.current = toolbarCollapsed;
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
    const nextView = readSessionView(viewScope);
    setHostView({ hostId: host.id, viewScope, view: nextView });
    restoredRef.current = false;
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
    const collapsed = initialToolbarCollapsed(nextView, compact);
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
    const collapsed = initialToolbarCollapsed(
      { ...readSessionView(viewScope), scrollOffset: scrollOffsetRef.current },
      compact,
    );
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

  const updateView = useCallback(
    (patch: Partial<SessionViewState>) => {
      setHostView({
        hostId: host.id,
        viewScope,
        view: patchSessionView(viewScope, patch),
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

  const handleToolbarControlFocus = useCallback(() => {
    toolbarControlFocusedRef.current = true;
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
      if (!view.advancedOpen) {
        updateView({ advancedOpen: true });
      }
      openCompactToolbar();
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
  }, [compact, openCompactToolbar, pinFilterInteraction, revealToolbar, setToolbarVisibility, updateView, view.advancedOpen]);

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
    updateView({
      status: "all",
      includeArchived: false,
      shortcut: null,
      streams: null,
      scrollOffset: 0,
    });
  }, [pinFilterInteraction, updateView]);

  /** One tap on the collapsed summary: clears what narrows the list, keeps grouping and disclosure. */
  const handleClearSummary = useCallback(() => {
    pinFilterInteraction();
    updateView({
      query: "",
      status: "all",
      includeArchived: false,
      shortcut: null,
      streams: null,
      scrollOffset: 0,
    });
  }, [pinFilterInteraction, updateView]);

  const handleClearAll = useCallback(() => {
    pinFilterInteraction();
    const reset = resetSessionView(viewScope);
    setHostView({
      hostId: host.id,
      viewScope,
      // Clearing filters does not also open or close the compact toolbar.
      view: compact ? patchSessionView(viewScope, toolbarOpenPatch(!toolbarCollapsedRef.current)) : reset,
    });
  }, [compact, host.id, viewScope, pinFilterInteraction]);

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
  const tree = useMemo(() => buildOrganizationTree({ entries: scoped, ...preferences, grouping, workspaces }), [scoped, preferences, grouping, workspaces]);
  const tokens = view.query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const filterTree = (status: StatusFilter): OrganizationGroup[] => {
    const eligible = (entry: DirectoryEntry) => matchesStatus(entry, status, favorites) && matchesShortcut(entry, shortcut, mappings)
      && (streams === null || streams.includes(workstreamKey(organizationScope(entry, mappings).workstream)));
    // A matching group label shows its eligible descendants; with no search, empty workspaces stay visible.
    return filterOrganizationTree(tree, (entry) => eligible(entry) && matchesQuery(entry, view.query),
      (group) => tokens.length ? tokens.every((token) => group.label.toLocaleLowerCase().includes(token)) : status === "all" && shortcut === null && streams === null && group.kind === "workspace",
      eligible);
  };
  const filtered = useMemo(() => filterTree(view.status), [tree, view.query, view.status, favorites, shortcut, streams, mappings]);
  const unstated = useMemo(() => view.status === "all" ? filtered : filterTree("all"), [filtered, tree, view.query, view.status, favorites, shortcut, streams, mappings]);
  const statusCounts = useMemo(() => {
    const candidates = visibleSessions(unstated);
    return Object.fromEntries([...QUICK_STATUS_FILTERS, ...MORE_STATUS_FILTERS].map((status) => [status, candidates.filter((entry) => matchesStatus(entry, status, favorites)).length])) as Record<StatusFilter, number>;
  }, [unstated, favorites]);
  const shortcutCounts = useMemo(() => SESSION_SHORTCUTS.map(({ id }) => scoped.filter((entry) => matchesShortcut(entry, id, mappings)).length), [scoped, mappings]);
  const streamChoices = useMemo(() => {
    const labels = new Map<string, string>();
    for (const entry of scoped) { const label = organizationScope(entry, mappings).workstream; labels.set(workstreamKey(label), label); }
    return [...labels].sort(([, left], [, right]) => workstreamRank(left) - workstreamRank(right) || left.localeCompare(right));
  }, [scoped, mappings]);
  const localExpansion = readLocalExpansion(host.id);
  const expanded = useMemo(() => ({ ...preferences.expandedGroups, ...localExpansion }), [preferences.expandedGroups, localExpansion]);
  // Search and state filters reveal every match; the saved disclosure returns once they clear.
  const reveal = tokens.length > 0 || view.status !== "all";
  const rows = useMemo(() => flattenOrganizationTree(filtered, expanded, reveal, defaultGroupExpanded), [filtered, expanded, reveal]);
  const rowSignature = useMemo(() => rows.map(rowKey).join("\n"), [rows]);
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
  const shown = useMemo(() => visibleSessions(filtered), [filtered]);
  const workspaceCount = new Set(shown.map((entry) => entry.workspaceId ?? entry.workspaceName ?? entry.cwd)).size;
  const setExpansion = (patch: Readonly<Record<string, boolean>>) => {
    const local = patchLocalExpansion(host.id, patch);
    setExpansionRevision((revision) => revision + 1);
    savePreference({ expandedGroups: { ...preferences.expandedGroups, ...local } });
  };
  const setAllGroups = (open: boolean) => setExpansion(Object.fromEntries(allGroupKeys(tree).map((key) => [key, open])));
  const chooseGrouping = (next: SessionGrouping) => {
    pinFilterInteraction();
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
    const movable = parent === "root" && row.kind === "group" ? peers.filter((key) => workstreamRank(tree.find((group) => group.key === key)?.label ?? "") === workstreamRank(row.group.label)) : peers;
    savePreference({ siblingOrder: { ...preferences.siblingOrder, [parent]: reorderSavedSiblingOrder(preferences.siblingOrder[parent] ?? [], siblingKeys(tree, parent), movable, rowKey(row), distance) } });
  };
  const moveSelection = (distance: number) => {
    const row = rows.find((candidate) => rowKey(candidate) === orderSelection);
    if (row) moveRow(row, distance);
  };

  const narrowed = tokens.length > 0 || view.status !== "all" || shortcut !== null || streams !== null;
  const arrangeEnabled = arranging && !narrowed && !editsDisabled;
  const optionParts = [
    grouping !== "hierarchy" ? GROUPINGS.find((choice) => choice.id === grouping)?.label : null,
    streams !== null ? `${streams.length} workstream${streams.length === 1 ? "" : "s"}` : null,
    shortcut === "Desks" ? "Durable desks only" : null,
    view.status === "closed" ? "Closed sessions" : view.status === "initializing" ? "Starting sessions" : null,
    view.includeArchived ? "Including archived" : null,
  ].filter((part): part is string => !!part);
  const activeCount = optionParts.length;
  const hasOrganizationFilter = shortcut !== null || streams !== null;
  const clearable = view.status !== "all" || view.includeArchived || hasOrganizationFilter;
  const summaryParts = activeFilterParts({ query: view.query, status: view.status, shortcut, streams, includeArchived: view.includeArchived });
  // The compact slim badge also counts the state and desk chips its closed toolbar hides; search has its own button.
  const slimActiveCount = compact
    ? [view.status !== "all", shortcut !== null, streams !== null, view.includeArchived, grouping !== "hierarchy"].filter(Boolean).length
    : activeCount;
  // While the compact toolbar is open, a short list still has room for the small drag that closes it.
  const scrollSlack = compact && !toolbarCollapsed && listHeight > 0 ? { minHeight: listHeight + COMPACT_SCROLL_SLACK } : {};

  useEffect(() => {
    if (restoredRef.current || state.entries.length === 0) {
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
  }, [viewScope, state.entries.length]);

  const staleMessage = state.error ?? state.staleReason;
  const showBanner =
    state.status === "stale" ||
    state.status === "error" ||
    (state.status === "ready" && !state.live && state.staleReason !== null);
  const navigationMissing = navigation === undefined;

  const focusProps = { disabled: toolbarCollapsed, onBlur: handleToolbarControlBlur, onFocus: handleToolbarControlFocus };
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
  const fullToolbar = (
    <View
      testID="sessions-full-toolbar"
      onLayout={handleToolbarLayout}
      style={{ gap: 8 }}
    >
      {compact ? null : notices}

      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        {scopeLabel ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 1 }}>
          <Text testID="sessions-heading" accessibilityRole="header" numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: compact ? 16 : 17, fontWeight: "600" }}>Sessions · {scopeLabel}</Text>
          {onClearScope ? <Action small={!compact} colors={colors} label="All sessions" accessibilityLabel={`Show all sessions instead of ${scopeLabel}`} disabled={toolbarCollapsed} onPress={onClearScope} /> : null}
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
          editable={!toolbarCollapsed}
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
            minHeight: compact ? 44 : 36,
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
            disabled={toolbarCollapsed}
            onPress={() => updateFilter({ query: "" })}
            style={{ minHeight: compact ? 44 : 32, justifyContent: "center", paddingHorizontal: 6 }}
          >
            <Text style={{ color: colors.accent, fontSize: 12, fontWeight: "600" }}>
              Clear
            </Text>
          </Pressable>
        ) : null}
        <FiltersButton
          activeCount={activeCount}
          expanded={view.advancedOpen}
          colors={colors}
          compact={compact}
          {...focusProps}
          onPress={handleAdvancedToggle}
        />
      </View>

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
        {clearable ? (
          <ClearFiltersButton
            colors={colors}
            compact={compact}
            {...focusProps}
            onPress={handleClearFilters}
          />
        ) : null}
      </View>
      {optionParts.length > 0 && !view.advancedOpen ? <Text testID="sessions-options-summary" numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{optionParts.join(" · ")}</Text> : null}

      {view.advancedOpen ? (
        <View
          testID="sessions-options"
          accessibilityLabel="Filters and view options"
          style={{
            borderColor: colors.border,
            borderWidth: 1,
            borderRadius: 8,
            backgroundColor: colors.surface1,
            padding: 10,
            gap: 10,
          }}
        >
          <OptionLine label="Group by" colors={colors}>
            {GROUPINGS.map(({ id, label }) => <FilterChip key={id} testID={`sessions-grouping-${id}`} label={label} compact={compact} active={grouping === id} colors={colors} {...focusProps} onPress={() => chooseGrouping(id)} />)}
          </OptionLine>
          {streamChoices.length > 1 ? <OptionLine label={streams === null ? "Workstreams · all" : `Workstreams · ${streams.length} of ${streamChoices.length}`} colors={colors}>
            {streamChoices.map(([key, label]) => <FilterChip key={key} checkbox label={label} compact={compact} active={streams === null || streams.includes(key)} colors={colors} {...focusProps} onPress={() => toggleStream(key)} />)}
            {streams !== null ? <FilterChip label="All" compact={compact} active={false} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ streams: null, scrollOffset: 0 }); }} /> : null}
          </OptionLine> : null}
          <OptionLine label="Workspace scope" colors={colors}>
            {SCOPE_CHOICES.map(({ id, label }) => <FilterChip key={label} label={label} compact={compact} active={shortcut === id} colors={colors} {...focusProps} onPress={() => { pinFilterInteraction(); updateView({ shortcut: id, scrollOffset: 0 }); }} />)}
          </OptionLine>
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
            <FilterChip testID="sessions-arrange" label={arranging ? "Done arranging" : "Arrange"} compact={compact} active={arranging} colors={colors} {...focusProps} disabled={toolbarCollapsed || (!arranging && (narrowed || editsDisabled))} onPress={() => { pinFilterInteraction(); setOrderSelection(null); updateView({ arranging: !arranging }); }} />
            <View style={{ flex: 1 }} />
            <FilterChip testID="sessions-expand-all" label="Expand all" compact={compact} active={false} colors={colors} {...focusProps} disabled={toolbarCollapsed || reveal} onPress={() => setAllGroups(true)} />
            <FilterChip testID="sessions-collapse-all" label="Collapse all" compact={compact} active={false} colors={colors} {...focusProps} disabled={toolbarCollapsed || reveal} onPress={() => setAllGroups(false)} />
          </View>
          {arranging ? <View testID="sessions-arrange-help" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <Text style={{ flexBasis: "100%", color: colors.foregroundMuted, fontSize: 11 }}>{narrowed ? "Clear search and filters to arrange." : editsDisabled ? "Saved order is unavailable while organization settings are unavailable or saving." : "Select a handle, then Move up or Move down, or drag it among its siblings. Desks stay above epic work."}</Text>
            <Action small={!compact} colors={colors} label="↑ Move up" accessibilityLabel="Move selected row up" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(-1)} />
            <Action small={!compact} colors={colors} label="↓ Move down" accessibilityLabel="Move selected row down" disabled={!arrangeEnabled || orderSelection === null} onPress={() => moveSelection(1)} />
          </View> : null}
          <Disclosure small label="Legend" colors={colors}>
            <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 18 }}>
              Icons mark a workstream, epic, workspace or native project. {Object.entries(WORKSPACE_KINDS).map(([code, meaning]) => `${code} · ${meaning}`).join("  ")}. Repeated title prefixes group siblings for display only; they do not establish an authorized role.
            </Text>
          </Disclosure>
        </View>
      ) : null}
    </View>
  );

  const toolbar = toolbarLayout({
    collapsed: toolbarCollapsed,
    compact,
  });
  const slimSlot = (
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
      {compact && toolbarCollapsed ? (
        <ActiveFilterSummary
          parts={summaryParts}
          scopeLabel={scopeLabel}
          colors={colors}
          onClear={handleClearSummary}
          onClearScope={onClearScope}
        />
      ) : null}
    </View>
  );
  // Compact renders the full toolbar only while it is open: the default
  // state is the slim line, and closed controls are neither drawn nor focusable.
  // Open, it heads the list content (see listHeader), so a drag on it scrolls
  // the rows and it scrolls away with them; its gap is part of that content.
  const fullToolbarSlot = compact && toolbarCollapsed ? null : (
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

  if (state.entries.length === 0 && (state.status === "loading" || state.status === "idle")) {
    return (
      <View
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
      </View>
    );
  }

  const extraData = { colors, arrangeEnabled, orderSelection, editsDisabled, favorites };
  const emptyMessage = hasActiveView(view) || hasOrganizationFilter
    ? "No sessions match the current search and filters."
    : state.status === "error"
      ? "No sessions could be read from this host."
      : "No sessions in this host's scope.";
  const countLine = state.entries.length === 0 ? null : (
    <Text testID="sessions-count" accessibilityRole="text" style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 4, paddingBottom: 8 }}>
      {shown.length === scoped.length ? `${scoped.length}` : `${shown.length} of ${scoped.length}`} session{scoped.length === 1 ? "" : "s"} · {workspaceCount} workspace{workspaceCount === 1 ? "" : "s"} ·{" "}
      {state.lastUpdatedAt === null
        ? "not read yet"
        : state.inventoryComplete
          ? `complete inventory read ${updatedLabel(state.lastUpdatedAt)}`
          : `partial inventory read ${updatedLabel(state.lastUpdatedAt)}`}
    </Text>
  );
  // An element, not a component, so the search input keeps its focus across renders.
  const listHeader = compact && fullToolbarSlot !== null ? (
    <View testID="sessions-list-header">
      {fullToolbarSlot}
      {countLine}
    </View>
  ) : countLine;

  return (
    <View
      style={{
        flex: 1,
        backgroundColor: colors.surface0,
        paddingTop: compact ? 12 : 16,
        paddingHorizontal: compact ? 12 : 18,
        paddingBottom: compact ? 20 : 16,
        gap: TOOLBAR_LAYOUT_GAP,
      }}
    >
      {slimSlot}
      {compactNotices}
      {compact ? null : fullToolbarSlot}
      <FlatList
        ref={listRef}
        testID="sessions-list"
        data={rows}
        keyExtractor={rowKey}
        extraData={extraData}
        initialNumToRender={24}
        windowSize={11}
        onLayout={compact ? handleListLayout : undefined}
        onScroll={handleListScroll}
        onScrollBeginDrag={handleScrollBeginDrag}
        // The compact toolbar sits in the list: a tap on a chip while typing
        // applies the chip instead of only dismissing the keyboard.
        keyboardShouldPersistTaps={compact ? "handled" : undefined}
        maintainVisibleContentPosition={{ minIndexForVisible: 0 }}
        scrollEventThrottle={16}
        contentContainerStyle={rows.length === 0 ? { flexGrow: 1, ...scrollSlack } : { paddingBottom: 16, ...scrollSlack }}
        ListHeaderComponent={listHeader}
        renderItem={({ item }) => {
          const key = rowKey(item);
          if (item.kind === "empty") return <OrganizationRowView row={item} colors={colors} compact={compact} disabled arranging={false} selected={false} favorite={false} onFavorite={() => undefined} onToggle={() => undefined} onMove={() => undefined} onSelect={() => undefined} />;
          const epic = item.kind === "group" && item.group.kind === "epic" ? item.group.epic : null;
          const plans = epic ? planSources?.[epic.toLocaleUpperCase()] ?? [] : [];
          const actions = epic && onEpicTab && plans.length > 0 ? (["Plan", "Board", "Graph"] as const).map((tab) => <Action key={tab} small={!compact} colors={colors} label={tab}
            accessibilityLabel={`${tab} for ${epic}${plans.length > 1 ? `, choose among ${plans.length} configured plans` : ""}`} onPress={() => onEpicTab(tab, epic, plans.length === 1 ? plans[0]! : null)} />) : undefined;
          return <OrganizationRowView row={item} colors={colors} compact={compact} disabled={editsDisabled} arranging={arrangeEnabled} selected={orderSelection === key}
            favorite={item.kind === "session" && favorites.includes(item.entry.id)} actions={actions}
            onToggle={() => { if (item.kind === "group") setExpansion({ [item.group.key]: !item.expanded }); }}
            onSelect={() => setOrderSelection(orderSelection === key ? null : key)}
            onMove={(distance) => moveRow(item, distance)} onFavorite={() => { if (item.kind === "session") savePreference({ favorites: favorites.includes(item.entry.id) ? favorites.filter((id) => id !== item.entry.id) : [...favorites, item.entry.id] }); }}>
          {item.kind === "session" ? <SessionRow
            entry={item.entry}
            displayContext={grouping === "role" ? `${organizationScope(item.entry, mappings).workstream} · ${organizationScope(item.entry, mappings).epic} · ${item.entry.workspaceName ?? "Unassigned workspace"}`
              : grouping === "flat" ? [item.entry.workspaceName ?? item.entry.cwd, item.entry.projectName !== item.entry.workspaceName ? item.entry.projectName : null].filter(Boolean).join(" · ") : undefined}
            displayTitle={item.parentKey.includes("/role:") ? item.entry.title.slice(item.entry.title.indexOf("•") + 1).trim() : undefined}
            colors={colors}
            compact={compact}
            onPress={handleOpen}
          /> : null}
          </OrganizationRowView>;
        }}
        ListEmptyComponent={
          <View
            testID="sessions-empty"
            style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 20 }}
          >
            <Text style={{ color: colors.foreground, fontSize: 14 }}>{emptyMessage}</Text>
            {hasActiveView(view) || hasOrganizationFilter ? (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel="Clear search and filters"
                onPress={handleClearAll}
                style={{ paddingVertical: 6 }}
              >
                <Text style={{ color: colors.accent, fontSize: 13, fontWeight: "600" }}>
                  Clear search and filters
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
          state.entries.length === 0 ? null : (
            <Text
              testID="sessions-footer"
              style={{ color: colors.foregroundMuted, fontSize: 11, paddingVertical: 12, paddingHorizontal: 4 }}
            >
              Select a session to open its existing Paseo conversation. Placement stays unchanged.
              {workspaceNote ? ` ${workspaceNote}.` : ""}
            </Text>
          )
        }
      />
    </View>
  );
}
