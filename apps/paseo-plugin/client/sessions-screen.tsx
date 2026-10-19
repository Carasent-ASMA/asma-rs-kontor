import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { useHosts } from "@getpaseo/plugin/client";
import {
  FlatList,
  TextInput,
  useToast,
} from "@getpaseo/plugin/client/react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  Text,
  View,
  type FlatList as NativeFlatList,
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
import { useSessionDirectory } from "./use-session-directory";

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

function indicatorColor(entry: DirectoryEntry, colors: ThemeColors): string {
  return entry.needsInput ? colors.statusWarning : statusColor(entry, colors);
}

function scopeLabel(includeArchived: boolean): string {
  return includeArchived
    ? "All sessions, including archived"
    : "All non-archived sessions";
}

function stateLabel(input: {
  hostOnline: boolean;
  status: "idle" | "loading" | "ready" | "stale" | "error";
  live: boolean;
}): string {
  if (!input.hostOnline) {
    return "Host offline";
  }
  if (input.status === "loading" || input.status === "idle") {
    return "Reading";
  }
  if (input.status === "error") {
    return "Unavailable";
  }
  if (input.status === "stale" || !input.live) {
    return "Stale";
  }
  return "Live";
}

function stateColor(label: string, colors: ThemeColors): string {
  if (label === "Live") {
    return colors.statusSuccess;
  }
  if (label === "Stale" || label === "Reading") {
    return colors.statusWarning;
  }
  if (label === "Host offline" || label === "Unavailable") {
    return colors.statusDanger;
  }
  return colors.foregroundMuted;
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
};

const SessionRow = memo(function SessionRow({
  entry,
  colors,
  compact,
  rowHeight,
  onPress,
}: SessionRowProps) {
  const attention = attentionLabel(entry);
  const archived = archivedLabel(entry);
  const context = entry.workspaceName ?? entry.projectName ?? entry.cwd;
  const project =
    entry.projectName && entry.projectName !== entry.workspaceName
      ? entry.projectName
      : null;
  const detail = project ? `${context} · ${project}` : context;
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
          {entry.title}
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
              backgroundColor: colors.statusWarning,
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

function FilterChip({
  active,
  label,
  colors,
  onPress,
  testID,
}: {
  active: boolean;
  label: string;
  colors: ThemeColors;
  onPress: () => void;
  testID?: string;
}) {
  return (
    <Pressable
      testID={testID}
      accessibilityRole="button"
      accessibilityState={{ selected: active }}
      accessibilityLabel={label}
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
  onPress,
  testID,
}: {
  label: string;
  description: string;
  checked: boolean;
  colors: ThemeColors;
  onPress: () => void;
  testID?: string;
}) {
  return (
    <Pressable
      testID={testID}
      accessibilityRole="switch"
      accessibilityState={{ checked }}
      accessibilityLabel={label}
      accessibilityHint={description}
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
  tone,
  onAction,
  testID,
}: {
  message: string;
  actionLabel?: string;
  colors: ThemeColors;
  tone: "warning" | "danger";
  onAction?: () => void;
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

export function SessionsScreen({ host, layout, navigation, theme }: PluginScreenProps) {
  const colors = theme.colors;
  const compact = layout.compact;
  const hosts = useHosts();
  const hostSummary = hosts.find((candidate) => candidate.serverId === host.id);
  const hostOnline = hostSummary?.status === "online";
  const [view, setView] = useState<SessionViewState>(() => readSessionView(host.id));
  const { state, refresh } = useSessionDirectory({
    hostId: host.id,
    hostStatus: hostSummary?.status ?? "offline",
    includeArchived: view.includeArchived,
  });
  const toast = useToast();
  const listRef = useRef<NativeFlatList<DirectoryEntry> | null>(null);
  const entriesRef = useRef(state.entries);
  entriesRef.current = state.entries;
  const restoredRef = useRef(false);
  const rowHeight = compact ? 64 : 68;

  const updateView = useCallback(
    (patch: Partial<SessionViewState>) => {
      setView(patchSessionView(host.id, patch));
    },
    [host.id],
  );

  const updateFilter = useCallback(
    (patch: Partial<SessionFilters>) => {
      updateView({ ...patch, scrollOffset: 0 });
    },
    [updateView],
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
    updateView({
      status: "all",
      attentionOnly: false,
      includeArchived: false,
      scrollOffset: 0,
    });
  }, [updateView]);

  const handleClearAll = useCallback(() => {
    setView(resetSessionView(host.id));
  }, [host.id]);

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

  const activeCount = activeFilterCount(view);

  useEffect(() => {
    if (restoredRef.current || state.entries.length === 0) {
      return;
    }
    restoredRef.current = true;
    const offset = readSessionView(host.id).scrollOffset;
    if (offset <= 0) {
      return;
    }
    const timer = setTimeout(() => {
      listRef.current?.scrollToOffset({ offset, animated: false });
    }, 0);
    return () => clearTimeout(timer);
  }, [host.id, state.entries.length]);

  const label = stateLabel({
    hostOnline,
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

  const header = (
    <View style={{ gap: 10 }}>
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
        />
      ) : null}

      {navigationMissing ? (
        <Banner
          testID="sessions-navigation-banner"
          message="This Paseo app does not expose native session navigation to plugins. Update Paseo to open sessions from this screen."
          tone="warning"
          colors={colors}
        />
      ) : null}

      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <TextInput
          testID="sessions-search"
          value={view.query}
          onChangeText={(query) => updateFilter({ query })}
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
            onPress={() => updateFilter({ status })}
          />
        ))}
        <FilterChip
          testID="sessions-filter-attention"
          label="Attention"
          active={view.attentionOnly}
          colors={colors}
          onPress={() => updateFilter({ attentionOnly: !view.attentionOnly })}
        />
      </View>

      <View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Pressable
            testID="sessions-advanced-toggle"
            accessibilityRole="button"
            accessibilityState={{ expanded: view.advancedOpen }}
            accessibilityLabel={`Advanced filters${activeCount > 0 ? `, ${activeCount} active` : ""}`}
            onPress={() => updateView({ advancedOpen: !view.advancedOpen })}
            style={{
              flexDirection: "row",
              alignItems: "center",
              gap: 6,
              paddingVertical: 6,
              flex: 1,
            }}
          >
            <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>
              Filters
            </Text>
            {activeCount > 0 ? (
              <Text style={{ color: colors.accent, fontSize: 12 }}>({activeCount})</Text>
            ) : null}
            <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>
              {view.advancedOpen ? "Hide" : "Show"}
            </Text>
          </Pressable>
          {activeCount > 0 ? (
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Clear filters"
              onPress={handleClearFilters}
              style={{ paddingVertical: 6, paddingHorizontal: 4 }}
            >
              <Text style={{ color: colors.accent, fontSize: 12, fontWeight: "600" }}>
                Clear filters
              </Text>
            </Pressable>
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
              description="Finished, failed or waiting for permission."
              checked={view.attentionOnly}
              colors={colors}
              onPress={() => updateFilter({ attentionOnly: !view.attentionOnly })}
            />
            <ToggleRow
              testID="sessions-advanced-archived"
              label="Include archived sessions"
              description="Archived is separate from the native closed state."
              checked={view.includeArchived}
              colors={colors}
              onPress={() => updateFilter({ includeArchived: !view.includeArchived })}
            />
          </View>
        ) : null}
      </View>
    </View>
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
        {header}
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 10 }}>
          <ActivityIndicator color={colors.foregroundMuted} />
          <Text style={{ color: colors.foregroundMuted, fontSize: 13 }}>
            Reading sessions from {host.label}…
          </Text>
        </View>
      </View>
    );
  }

  const emptyMessage = hasActiveView(view)
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
        gap: 8,
      }}
    >
      {header}
      <FlatList
        ref={listRef}
        testID="sessions-list"
        data={visible}
        keyExtractor={(item) => item.id}
        extraData={colors}
        getItemLayout={(_, index) => ({
          length: rowHeight,
          offset: rowHeight * index,
          index,
        })}
        initialNumToRender={24}
        windowSize={11}
        onScroll={(event) => {
          writeScrollOffset(host.id, event.nativeEvent.contentOffset.y);
        }}
        scrollEventThrottle={120}
        contentContainerStyle={visible.length === 0 ? { flexGrow: 1 } : undefined}
        renderItem={({ item }) => (
          <SessionRow
            entry={item}
            colors={colors}
            compact={compact}
            rowHeight={rowHeight}
            onPress={handleOpen}
          />
        )}
        ListEmptyComponent={
          <View
            testID="sessions-empty"
            style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 20 }}
          >
            <Text style={{ color: colors.foreground, fontSize: 14 }}>{emptyMessage}</Text>
            {hasActiveView(view) ? (
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
                {visible.length} of {state.entries.length} shown · complete inventory read{" "}
                {updatedLabel(state.lastUpdatedAt)}
              </Text>
            </View>
          )
        }
      />
    </View>
  );
}
