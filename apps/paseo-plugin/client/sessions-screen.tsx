import { nativeFreshnessLabel as stateLabel, nativeFreshnessColor as stateColor } from "./native-freshness";
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
import {
  type DirectoryEntry,
  type SessionStatus,
} from "./session-directory";
import {
  activeFilterCount,
  attentionLabel,
  archivedLabel,
  filterDirectoryEntries,
  hasActiveView,
  STATUS_FILTERS,
  statusFilterLabel,
  type SessionFilters,
  type StatusFilter,
} from "./session-view";
import {
  patchSessionView,
  readSessionView,
  resetSessionView,
  writeScrollOffset,
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
import { buildOrganizationTree, filterOrganizationTree, flattenOrganizationTree, organizationScope, workstreamRank, type OrganizationRow, type OrganizationWorkspace } from "./session-organization";
import { rowKey, siblingKeys, reorderSavedSiblingOrder } from "./organization-order";
import { OrganizationRowView } from "./organization-row";

type ThemeColors = PluginTheme["colors"];

const STATUS_COLORS: Record<SessionStatus, "accent" | "statusSuccess" | "statusWarning" | "statusDanger" | "foregroundMuted"> = {
  initializing: "statusWarning",
  running: "accent",
  idle: "statusSuccess",
  error: "statusDanger",
  closed: "foregroundMuted",
};

function statusColor(entry: DirectoryEntry, colors: ThemeColors): string {
  return colors[STATUS_COLORS[entry.status]];
}

export function indicatorColor(entry: DirectoryEntry, colors: ThemeColors): string {
  if (entry.needsInput) {
    return colors.statusWarning;
  }
  if (entry.status === "error" || entry.attentionReason === "error") {
    return colors.statusDanger;
  }
  return statusColor(entry, colors);
}

export const FILTERS_TOGGLE_TEST_ID = "sessions-advanced-toggle";
export const CLEAR_FILTERS_TEST_ID = "sessions-clear-filters";
export const FILTERS_TOUCH_TARGET = 44;

export function filtersAccessibilityLabel(activeCount: number): string {
  return activeCount > 0 ? `Filters, ${activeCount} active` : "Filters";
}

export function filtersDisclosureGlyph(expanded: boolean): string {
  return expanded ? "⌃" : "⌄";
}

function scopeLabel(includeArchived: boolean): string {
  return includeArchived
    ? "All sessions, including archived"
    : "All non-archived sessions";
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
  readonly rowHeight: number;
  readonly onPress: (entry: DirectoryEntry) => void;
  readonly displayTitle?: string;
  readonly displayContext?: string;
};

const SessionRow = memo(function SessionRow({
  entry,
  colors,
  compact,
  rowHeight,
  onPress,
  displayTitle,
  displayContext,
}: SessionRowProps) {
  const attention = attentionLabel(entry);
  const archived = archivedLabel(entry);
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

  return (
    <Pressable
      testID={`sessions-row-${entry.id}`}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityHint="Opens the existing Paseo session"
      onPress={() => onPress(entry)}
      style={({ pressed }) => ({
        height: rowHeight,
        paddingHorizontal: compact ? 14 : 18,
        paddingVertical: 8,
        flexDirection: "row",
        alignItems: "center",
        gap: 12,
        borderBottomWidth: 1,
        borderBottomColor: colors.border,
        backgroundColor: pressed ? colors.surface2 : colors.surface0,
      })}
    >
      <View
        style={{
          width: 9,
          height: 9,
          borderRadius: 5,
          backgroundColor: indicatorColor(entry, colors),
        }}
      />
      <View style={{ flex: 1, minWidth: 0 }}>
        <Text
          numberOfLines={1}
          style={{ color: colors.foreground, fontSize: compact ? 15 : 14, fontWeight: "600" }}
        >
          {displayTitle ?? entry.title}
        </Text>
        <Text
          numberOfLines={1}
          style={{ color: colors.foregroundMuted, fontSize: 12, marginTop: 2 }}
        >
          {detail || "No workspace context"}
          {archived ? ` · ${archived}` : ""}
        </Text>
        <Text
          numberOfLines={1}
          adjustsFontSizeToFit
          minimumFontScale={0.75}
          ellipsizeMode="middle"
          style={{ color: colors.foregroundMuted, fontSize: 10, marginTop: 1, opacity: 0.8 }}
        >
          {entry.id}
        </Text>
      </View>
      <View style={{ alignItems: "flex-end", gap: 2, maxWidth: compact ? 84 : 120 }}>
        <Text
          style={{
            color: statusColor(entry, colors),
            fontSize: 11,
            textTransform: "uppercase",
            letterSpacing: 0.4,
          }}
        >
          {entry.status}
        </Text>
        {attention ? (
          <Text
            numberOfLines={1}
            style={{
              color: colors.surface0,
              backgroundColor:
                attention === "Error" ? colors.statusDanger : colors.statusWarning,
              borderRadius: 6,
              overflow: "hidden",
              paddingHorizontal: 5,
              paddingVertical: 1,
              fontSize: 10,
              fontWeight: "600",
            }}
          >
            {attention}
          </Text>
        ) : null}
      </View>
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
        flex: 1,
        minHeight: FILTERS_TOUCH_TARGET,
        minWidth: FILTERS_TOUCH_TARGET,
        flexDirection: "row",
        alignItems: "center",
        gap: 8,
        paddingHorizontal: 12,
        paddingVertical: 8,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: activeCount > 0 ? colors.accent : colors.border,
        backgroundColor: colors.surface1,
      }}
    >
      <Text
        style={{
          color: colors.foreground,
          fontSize: compact ? 15 : 13,
          fontWeight: "600",
        }}
      >
        Filters
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
      <View style={{ flex: 1 }} />
      <Text
        accessibilityElementsHidden
        importantForAccessibility="no"
        style={{
          color: colors.foregroundMuted,
          fontSize: 18,
          lineHeight: 20,
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
      accessibilityHint="Clears status, attention and archive filters"
      hitSlop={4}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        minHeight: FILTERS_TOUCH_TARGET,
        minWidth: FILTERS_TOUCH_TARGET,
        paddingHorizontal: 10,
        paddingVertical: 8,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: colors.border,
        backgroundColor: colors.surface0,
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
  onPress,
}: {
  readonly testID: string;
  readonly label: string;
  readonly accessibilityLabel: string;
  readonly accessibilityHint: string;
  readonly count?: number;
  readonly colors: ThemeColors;
  readonly disabled?: boolean;
  readonly onPress: () => void;
}) {
  return (
    <Pressable
      testID={testID}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel}
      accessibilityHint={accessibilityHint}
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

export function SlimToolbar({
  hostLabel,
  statusLabel,
  statusColor,
  activeCount,
  colors,
  compact,
  disabled = false,
  onSearch,
  onFilters,
}: {
  readonly hostLabel: string;
  readonly statusLabel: string;
  readonly statusColor: string;
  readonly activeCount: number;
  readonly colors: ThemeColors;
  readonly compact: boolean;
  readonly disabled?: boolean;
  readonly onSearch: () => void;
  readonly onFilters: () => void;
}) {
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
      <View style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 7 }}>
        <View
          style={{
            width: 8,
            height: 8,
            borderRadius: 4,
            backgroundColor: statusColor,
          }}
        />
        <View style={{ flex: 1, minWidth: 0 }}>
          <Text
            numberOfLines={1}
            style={{
              color: colors.foreground,
              fontSize: compact ? 14 : 13,
              fontWeight: "600",
            }}
          >
            {hostLabel}
          </Text>
          <Text
            numberOfLines={1}
            style={{ color: statusColor, fontSize: 11, marginTop: 1 }}
          >
            {statusLabel}
          </Text>
        </View>
      </View>
      <SlimActionButton
        testID="sessions-slim-search"
        label="Search"
        accessibilityLabel="Search sessions"
        accessibilityHint="Shows the search and filter controls"
        colors={colors}
        disabled={disabled}
        onPress={onSearch}
      />
      <SlimActionButton
        testID="sessions-slim-filters"
        label="Filters"
        accessibilityLabel={filtersAccessibilityLabel(activeCount)}
        accessibilityHint="Shows the filter options"
        count={activeCount}
        colors={colors}
        disabled={disabled}
        onPress={onFilters}
      />
    </View>
  );
}

function FilterChip({
  active,
  label,
  colors,
  disabled = false,
  onBlur,
  onFocus,
  onPress,
  testID,
}: {
  active: boolean;
  label: string;
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
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      accessibilityLabel={label}
      onBlur={onBlur}
      onFocus={onFocus}
      onPress={onPress}
      style={{
        paddingHorizontal: 10,
        paddingVertical: 6,
        borderRadius: 8,
        borderWidth: 1,
        borderColor: active ? colors.accent : colors.border,
        backgroundColor: active ? colors.accent : colors.surface1,
      }}
    >
      <Text
        style={{
          color: active ? colors.accentForeground : colors.foreground,
          fontSize: 12,
          fontWeight: "600",
        }}
      >
        {label}
      </Text>
    </Pressable>
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
};
export function SessionsScreen({ host, layout, navigation, theme, directory, organization, projectId = null, onIncludeArchived, workspaces, workspaceNote, epicKey = null, viewScope = host.id }: SessionsScreenProps) {
  const colors = theme.colors;
  const compact = layout.compact;
  const hosts = useHosts();
  const hostSummary = hosts.find((candidate) => candidate.serverId === host.id);
  const hostOnline = hostSummary?.status === "online";
  const [view, setView] = useState<SessionViewState>(() => readSessionView(viewScope));
  const localDirectory = useSessionDirectory({
    hostId: host.id,
    hostStatus: hostSummary?.status ?? "offline",
    includeArchived: view.includeArchived,
    enabled: directory === undefined,
  });
  const { state, refresh } = directory ?? localDirectory;
  const preferences = organization?.state.status === "ready" ? organization.state.values : DEFAULT_ORGANIZATION_SETTINGS;
  const favoritesOnly = view.favoritesOnly ?? false;
  const shortcut = view.shortcut ?? null;
  const [saveMessage, setSaveMessage] = useState<string | null>(null);
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
  const rowHeight = compact ? 64 : 68;
  const [toolbarCollapsed, setToolbarCollapsed] = useState(
    () => initialToolbarVisibility(readSessionView(viewScope).scrollOffset) === "collapsed",
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
  const programmaticUntilRef = useRef(0);
  const pendingCompensationRef = useRef<number | null>(null);
  const pendingRevealRef = useRef<{ readonly focusSearch: boolean } | null>(null);
  const revealFallbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const searchInputRef = useRef<ComponentRef<typeof TextInput> | null>(null);
  const previousOrderRef = useRef(state.entries.map((entry) => entry.id).join("\n"));

  useEffect(() => {
    return () => {
      if (revealFallbackTimerRef.current !== null) {
        clearTimeout(revealFallbackTimerRef.current);
        revealFallbackTimerRef.current = null;
      }
    };
  }, []);

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
      setView(patchSessionView(viewScope, patch));
    },
    [viewScope],
  );

  const setToolbarVisibility = useCallback((next: ToolbarVisibility) => {
    if (next === (toolbarCollapsedRef.current ? "collapsed" : "expanded")) {
      return;
    }
    const collapsed = next === "collapsed";
    if (collapsed && expandedToolbarHeightRef.current <= 0) {
      return;
    }
    const target = toolbarScrollCompensation({
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
    pendingCompensationRef.current = target;
    toolbarCollapsedRef.current = collapsed;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
    setToolbarCollapsed(collapsed);
  }, [compact]);

  useLayoutEffect(() => {
    const target = pendingCompensationRef.current;
    if (target === null) {
      return;
    }
    pendingCompensationRef.current = null;
    programmaticUntilRef.current = Date.now() + 250;
    scrollOffsetRef.current = target;
    lastScrollOffsetRef.current = target;
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

  const handleRevealSearch = useCallback(() => {
    revealToolbar(true);
  }, [revealToolbar]);

  const handleRevealFilters = useCallback(() => {
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
  }, [pinFilterInteraction, revealToolbar, updateView, view.advancedOpen]);

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
      attentionOnly: false,
      includeArchived: false,
      favoritesOnly: false,
      shortcut: null,
      scrollOffset: 0,
    });
  }, [pinFilterInteraction, updateView]);

  const handleClearAll = useCallback(() => {
    pinFilterInteraction();
    setView(resetSessionView(viewScope));
  }, [viewScope, pinFilterInteraction]);

  const handleScrollBeginDrag = useCallback(() => {
    filterPinnedUntilRef.current = 0;
    scrollAccumulatorRef.current = EMPTY_SCROLL_ACCUMULATOR;
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
    [compact, viewScope, setToolbarVisibility],
  );

  const visible = useMemo(
    () =>
      filterDirectoryEntries(state.entries, {
        query: view.query,
        status: view.status,
        attentionOnly: view.attentionOnly,
        includeArchived: view.includeArchived,
        advancedOpen: view.advancedOpen,
      }),
    [
      state.entries,
      view.query,
      view.status,
      view.attentionOnly,
      view.includeArchived,
      view.advancedOpen,
    ],
  );
  const tree = useMemo(() => buildOrganizationTree({ entries: state.entries.filter((entry) => (projectId === null || entry.projectKey === projectId) && (epicKey === null || [entry.labels.epic, entry.labels["jira.epic"], entry.labels["origin.epic"]].includes(epicKey))), ...preferences, workspaces }), [state.entries, projectId, epicKey, preferences, workspaces]);
  const visibleIds = useMemo(() => new Set(visible.map((entry) => entry.id)), [visible]);
  const rows = useMemo(() => {
    const eligible = new Set(filterDirectoryEntries(state.entries, { ...view, query: "" }).map((entry) => entry.id));
    const organizationMatches = (entry: DirectoryEntry) => (!favoritesOnly || preferences.favorites.includes(entry.id)) && (shortcut === null || organizationScope(entry, preferences.displayMappings).workstream.toLocaleLowerCase() === shortcut.toLocaleLowerCase());
    const tokens = view.query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const filtered = filterOrganizationTree(tree, (entry) => visibleIds.has(entry.id) && organizationMatches(entry),
      (group) => tokens.length ? tokens.every((token) => group.label.toLocaleLowerCase().includes(token)) : !favoritesOnly && shortcut === null && view.status === "all" && !view.attentionOnly && group.kind === "workspace",
      (entry) => eligible.has(entry.id) && organizationMatches(entry));
    return flattenOrganizationTree(filtered, preferences.expandedGroups, tokens.length > 0);
  }, [tree, state.entries, visibleIds, favoritesOnly, preferences, shortcut, view]);
  const moveRow = (row: OrganizationRow, distance: number) => {
    const parent = row.kind === "group" ? row.group.parentKey ?? "root" : row.parentKey;
    const peers = siblingKeys(tree, parent).filter((key) => (row.kind === "session") === key.startsWith("session:"));
    const movable = parent === "root" && row.kind === "group" ? peers.filter((key) => workstreamRank(tree.find((group) => group.key === key)?.label ?? "") === workstreamRank(row.group.label)) : peers;
    savePreference({ siblingOrder: { ...preferences.siblingOrder, [parent]: reorderSavedSiblingOrder(preferences.siblingOrder[parent] ?? [], siblingKeys(tree, parent), movable, rowKey(row), distance) } });
  };

  const activeCount = activeFilterCount(view) + Number(favoritesOnly) + Number(shortcut !== null);
  const hasOrganizationFilter = favoritesOnly || shortcut !== null;
  const shownCount = rows.filter((row) => row.kind === "session").length;

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

  const label = stateLabel({
    hostStatus: hostSummary?.status ?? "offline",
    status: state.status,
    live: state.live,
  });
  const labelColor = stateColor(label, colors);
  const staleMessage = state.error ?? state.staleReason;
  const showBanner =
    state.status === "stale" ||
    state.status === "error" ||
    (state.status === "ready" && !state.live && state.staleReason !== null);
  const navigationMissing = navigation === undefined;

  const fullToolbar = (
    <View
      testID="sessions-full-toolbar"
      onLayout={handleToolbarLayout}
      style={{ gap: 10 }}
    >
      <View style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
        <View style={{ flex: 1, minWidth: 0 }}>
          <Text
            numberOfLines={1}
            style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}
          >
            {host.label}
          </Text>
          <Text
            numberOfLines={1}
            style={{ color: colors.foregroundMuted, fontSize: 11, marginTop: 1 }}
          >
            {scopeLabel(state.includeArchived)} · {state.entries.length} sessions ·{" "}
            {state.pages} page{state.pages === 1 ? "" : "s"} read
          </Text>
        </View>
        <View
          style={{
            borderColor: labelColor,
            borderWidth: 1,
            borderRadius: 999,
            paddingHorizontal: 9,
            paddingVertical: 3,
          }}
        >
          <Text style={{ color: labelColor, fontSize: 11, fontWeight: "600" }}>{label}</Text>
        </View>
      </View>

      {showBanner && staleMessage ? (
        <Banner
          testID="sessions-source-banner"
          message={`${staleMessage} Last complete read: ${updatedLabel(state.lastUpdatedAt)}.`}
          tone={state.status === "error" ? "danger" : "warning"}
          actionLabel="Retry"
          onAction={refresh}
          colors={colors}
          disabled={toolbarCollapsed}
          onBlur={handleToolbarControlBlur}
          onFocus={handleToolbarControlFocus}
        />
      ) : null}

      {navigationMissing ? (
        <Banner
          testID="sessions-navigation-banner"
          message="This Paseo app does not expose native session navigation to plugins. Update Paseo to open sessions from this screen."
          tone="warning"
          colors={colors}
          disabled={toolbarCollapsed}
          onBlur={handleToolbarControlBlur}
          onFocus={handleToolbarControlFocus}
        />
      ) : null}

      {organization && organization.state.status !== "ready" ? <Text style={{ color: colors.statusWarning, fontSize: 12 }}>Organization settings {organization.state.status}; saved preferences are unavailable. Native sessions remain visible.</Text> : null}
      {workspaceNote ? <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{workspaceNote}</Text> : null}
      {saveMessage || organization?.state.saveError ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{saveMessage ?? organization?.state.saveError}</Text> : null}
      {organization ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        {(["hierarchy", "role", "flat"] as const).map((grouping) => <FilterChip key={grouping} label={grouping === "hierarchy" ? "Hierarchy" : grouping === "role" ? "Role" : "Flat"} active={preferences.grouping === grouping} colors={colors} disabled={toolbarCollapsed || editsDisabled} onFocus={handleToolbarControlFocus} onBlur={handleToolbarControlBlur} onPress={() => savePreference({ grouping })} />)}
        <FilterChip label="Favorites" active={favoritesOnly} colors={colors} disabled={toolbarCollapsed} onFocus={handleToolbarControlFocus} onBlur={handleToolbarControlBlur} onPress={() => updateView({ favoritesOnly: !favoritesOnly, scrollOffset: 0 })} />
        {["PR Review", "Adam", "Other work"].map((label) => <FilterChip key={label} label={label} active={shortcut === label} colors={colors} disabled={toolbarCollapsed} onFocus={handleToolbarControlFocus} onBlur={handleToolbarControlBlur} onPress={() => updateView({ shortcut: shortcut === label ? null : label, scrollOffset: 0 })} />)}
      </View> : null}

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
          placeholder="Search sessions, workspaces, projects"
          placeholderTextColor={colors.foregroundMuted}
          autoCapitalize="none"
          autoCorrect={false}
          accessibilityLabel="Search sessions"
          style={{
            flex: 1,
            minHeight: 40,
            borderRadius: 8,
            borderWidth: 1,
            borderColor: colors.border,
            backgroundColor: colors.surface1,
            color: colors.foreground,
            paddingHorizontal: 10,
            paddingVertical: 8,
            fontSize: compact ? 16 : 13,
          }}
        />
        {view.query.length > 0 ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel="Clear search"
            disabled={toolbarCollapsed}
            onPress={() => updateFilter({ query: "" })}
            style={{ paddingHorizontal: 8, paddingVertical: 8 }}
          >
            <Text style={{ color: colors.accent, fontSize: 12, fontWeight: "600" }}>
              Clear
            </Text>
          </Pressable>
        ) : null}
      </View>

      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
        {STATUS_FILTERS.map((status) => (
          <FilterChip
            key={status}
            testID={`sessions-filter-${status}`}
            label={statusFilterLabel(status)}
            active={view.status === status}
            colors={colors}
            disabled={toolbarCollapsed}
            onBlur={handleToolbarControlBlur}
            onFocus={handleToolbarControlFocus}
            onPress={() => updateFilter({ status })}
          />
        ))}
        <FilterChip
          testID="sessions-filter-attention"
          label="Attention"
          active={view.attentionOnly}
          colors={colors}
          disabled={toolbarCollapsed}
          onBlur={handleToolbarControlBlur}
          onFocus={handleToolbarControlFocus}
          onPress={() => updateFilter({ attentionOnly: !view.attentionOnly })}
        />
      </View>

      <View>
        <View style={{ flexDirection: "row", alignItems: "stretch", gap: 8 }}>
          <FiltersButton
            activeCount={activeCount}
            expanded={view.advancedOpen}
            colors={colors}
            compact={compact}
            disabled={toolbarCollapsed}
            onBlur={handleToolbarControlBlur}
            onFocus={handleToolbarControlFocus}
            onPress={handleAdvancedToggle}
          />
          {activeCount > 0 ? (
            <ClearFiltersButton
              colors={colors}
              compact={compact}
              disabled={toolbarCollapsed}
              onBlur={handleToolbarControlBlur}
              onFocus={handleToolbarControlFocus}
              onPress={handleClearFilters}
            />
          ) : null}
        </View>
        {view.advancedOpen ? (
          <View
            style={{
              borderColor: colors.border,
              borderWidth: 1,
              borderRadius: 8,
              backgroundColor: colors.surface1,
              paddingHorizontal: 10,
              paddingVertical: 4,
            }}
          >
            <ToggleRow
              testID="sessions-advanced-attention"
              label="Only sessions needing attention"
              description="Finished, failed, needs input or waiting for permission."
              checked={view.attentionOnly}
              colors={colors}
              disabled={toolbarCollapsed}
              onBlur={handleToolbarControlBlur}
              onFocus={handleToolbarControlFocus}
              onPress={() => updateFilter({ attentionOnly: !view.attentionOnly })}
            />
            <ToggleRow
              testID="sessions-advanced-archived"
              label="Include archived sessions"
              description="Archived is separate from the native closed state."
              checked={view.includeArchived}
              colors={colors}
              disabled={toolbarCollapsed}
              onBlur={handleToolbarControlBlur}
              onFocus={handleToolbarControlFocus}
              onPress={() => updateFilter({ includeArchived: !view.includeArchived })}
            />
          </View>
        ) : null}
      </View>
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
          ? undefined
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
        statusLabel={label}
        statusColor={labelColor}
        activeCount={activeCount}
        colors={colors}
        compact={compact}
        disabled={!toolbar.showSlimRow}
        onSearch={handleRevealSearch}
        onFilters={handleRevealFilters}
      />
    </View>
  );
  const fullToolbarSlot = (
    <Animated.View
      testID="sessions-full-toolbar-slot"
      pointerEvents={toolbar.showFullToolbar ? "auto" : "none"}
      accessibilityElementsHidden={!toolbar.showFullToolbar}
      importantForAccessibility={
        toolbar.showFullToolbar ? "auto" : "no-hide-descendants"
      }
      style={[
        toolbar.showFullToolbar
          ? undefined
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

  const emptyMessage = hasActiveView(view) || hasOrganizationFilter
    ? "No sessions match the current search and filters."
    : state.status === "error"
      ? "No sessions could be read from this host."
      : "No sessions in this host's scope.";

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
      {fullToolbarSlot}
      <FlatList
        ref={listRef}
        testID="sessions-list"
        data={rows}
        keyExtractor={rowKey}
        extraData={colors}
        getItemLayout={(_, index) => ({
          length: rowHeight,
          offset: rowHeight * index,
          index,
        })}
        initialNumToRender={24}
        windowSize={11}
        onScroll={handleListScroll}
        onScrollBeginDrag={handleScrollBeginDrag}
        maintainVisibleContentPosition={{ minIndexForVisible: 0 }}
        scrollEventThrottle={16}
        contentContainerStyle={rows.length === 0 ? { flexGrow: 1 } : { paddingBottom: 16 }}
        renderItem={({ item }) => (
          <OrganizationRowView row={item} colors={colors} height={rowHeight} disabled={editsDisabled} favorite={item.kind === "session" && preferences.favorites.includes(item.entry.id)}
            onToggle={() => { if (item.kind === "group") savePreference({ expandedGroups: { ...preferences.expandedGroups, [item.group.key]: !item.expanded } }); }}
            onMove={(distance) => moveRow(item, distance)} onFavorite={() => { if (item.kind === "session") savePreference({ favorites: preferences.favorites.includes(item.entry.id) ? preferences.favorites.filter((id) => id !== item.entry.id) : [...preferences.favorites, item.entry.id] }); }}>
          {item.kind === "session" ? <SessionRow
            entry={item.entry}
            displayContext={preferences.grouping === "role" ? `${organizationScope(item.entry, preferences.displayMappings).workstream} · ${organizationScope(item.entry, preferences.displayMappings).epic} · ${item.entry.workspaceName ?? "Unassigned workspace"}` : undefined}
            displayTitle={item.parentKey.includes("/role:") || item.parentKey.startsWith("roles/role:") ? item.entry.title.slice(item.entry.title.indexOf("•") + 1).trim() : undefined}
            colors={colors}
            compact={compact}
            rowHeight={rowHeight}
            onPress={handleOpen}
          /> : null}
          </OrganizationRowView>
        )}
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
            <View style={{ paddingVertical: 12, alignItems: "center" }}>
              <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>
                {shownCount} of {state.entries.length} session rows visible ·{" "}
                {state.lastUpdatedAt === null
                  ? "not read yet"
                  : `complete inventory read ${updatedLabel(state.lastUpdatedAt)}`}
              </Text>
            </View>
          )
        }
      />
    </View>
  );
}
