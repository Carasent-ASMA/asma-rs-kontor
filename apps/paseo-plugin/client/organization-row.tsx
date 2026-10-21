import { useMemo, type ReactNode } from "react";
import { PanResponder, Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { aggregateState, aggregateSummary, leadingStates, ROLE_ROOT_KEY, STATE_LABELS, type OrganizationAggregate, type OrganizationGroup, type OrganizationGroupKind, type OrganizationRow, type OrganizationState } from "./session-organization";
import { CHAIN_STATE_GLYPHS } from "./session-results";
import { COARSE_POINTER, type Colors } from "./page-controls";

/** Display glossary for repeated workspace-name prefixes; grouping by prefix establishes no authority. */
export const WORKSPACE_KINDS: Readonly<Record<string, string>> = {
  RSW: "Review workspace", TSW: "Task workspace", ASW: "Advisory workspace", CSW: "Committee workspace",
  ESW: "Epic workspace", ECP: "Legacy epic coordination", DESK: "Durable desk",
};
const KIND_ICONS: Partial<Record<OrganizationGroupKind, string>> = { workstream: "Route", epic: "Flag", workspace: "PanelsTopLeft", project: "Folder" };
const KIND_NAMES: Readonly<Record<OrganizationGroupKind, string>> = {
  workstream: "Workstream", epic: "Epic", "workspace-kind": "Workspace kind", workspace: "Workspace", project: "Native project", role: "Role", flat: "Sessions",
};

export function stateColor(state: OrganizationState | null, colors: Colors): string {
  switch (state) {
    case "needs_input": return colors.statusWarning;
    case "failed": return colors.statusDanger;
    case "running": return colors.accent;
    case "attention": return colors.statusSuccess;
    default: return colors.foregroundMuted;
  }
}

/** Dot and label of the highest-priority state; the full breakdown stays in the accessible label. */
export function StateBadge({ state, colors, compact }: { state: OrganizationState | null; colors: Colors; compact: boolean }) {
  if (state === null) return <Text style={{ color: colors.foregroundMuted, fontSize: 9, opacity: 0.75 }}>No sessions</Text>;
  return <View style={{ flexDirection: "row", alignItems: "center", gap: compact ? 4 : 6 }}>
    <View style={{ width: compact ? 5 : 7, height: compact ? 5 : 7, borderRadius: 4, backgroundColor: stateColor(state, colors), opacity: state === "closed" ? 0.6 : 1 }} />
    <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: compact ? 9 : 10 }}>{STATE_LABELS[state]}</Text>
  </View>;
}

/**
 * A Role header's states: up to two non-zero ones, most urgent first, each as shape, count and
 * word, so one error among five sessions reads as one. The total stays in its own column.
 * While the inventory is stale they follow "Last known", as on the Board.
 */
function StateCounts({ aggregate, colors, compact, stale }: { aggregate: OrganizationAggregate; colors: Colors; compact: boolean; stale: boolean }) {
  const states = leadingStates(aggregate);
  if (!states.length) return <StateBadge state={null} colors={colors} compact={compact} />;
  return <View testID="organization-state-counts" style={{ flexDirection: "row", alignItems: "center", gap: compact ? 6 : 10, flexShrink: 1, minWidth: 0 }}>
    {stale ? <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foregroundMuted, fontSize: compact ? 9 : 10 }}>Last known</Text> : null}
    {states.map(({ state, count }) => <View key={state} style={{ flexDirection: "row", alignItems: "center", gap: 3, flexShrink: 1, minWidth: 0 }}>
      <Text style={{ color: stateColor(state, colors), fontSize: compact ? 10 : 11, opacity: state === "closed" ? 0.6 : 1 }}>{CHAIN_STATE_GLYPHS[state]}</Text>
      <Text style={{ color: colors.foreground, fontSize: compact ? 10 : 11, fontVariant: ["tabular-nums"] }}>{count}</Text>
      {/* The word gives way first on a narrow header; the shape and count stay. */}
      <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foregroundMuted, fontSize: compact ? 9 : 10 }}>{STATE_LABELS[state].toLocaleLowerCase()}</Text>
    </View>)}
  </View>;
}

export function indentOf(depth: number, compact: boolean): number { return depth * (compact ? 14 : 18); }

/** One guide per ancestor level; adjacent rows join them into continuous lines. */
function Guides({ depth, compact, colors }: { depth: number; compact: boolean; colors: Colors }) {
  const step = compact ? 14 : 18;
  return <>{Array.from({ length: depth }, (_, level) => <View key={level} pointerEvents="none" style={{ position: "absolute", top: 0, bottom: 0, left: level * step + step / 2, width: 1, backgroundColor: colors.border }} />)}</>;
}

export function groupCount(group: OrganizationGroup): string {
  return group.unitVisible === group.unitTotal ? `${group.unitTotal}` : `${group.unitVisible} of ${group.unitTotal}`;
}

function nativeCounts(aggregate: OrganizationAggregate): string {
  const native = aggregate.native;
  return `Native counts: ${native.initializing} initializing, ${native.idle} idle, ${native.running} running, ${native.error} errors, ${native.closed} closed.`;
}

export function groupDisplayLabel(group: OrganizationGroup): string {
  return group.kind === "workspace" && group.parentKey?.includes("/kind:") ? group.label.slice(group.label.indexOf("•") + 1).trim() : group.label;
}

export function OrganizationRowView({ row, colors, compact, children, details, lifecycleAction, disabled, arranging, selected, favorite, stale = false, actions, onFavorite, onToggle, onMove, onSelect }: {
  row: OrganizationRow; colors: Colors; compact: boolean; children?: ReactNode;
  /** A session's own control beside its row button, never inside it. */
  details?: ReactNode;
  lifecycleAction?: ReactNode;
  /** Saved-preference writes are unavailable; disclosure still works. */
  disabled: boolean;
  arranging: boolean; selected: boolean; favorite: boolean;
  /** The sessions were not read live; a Role header calls its states last known. */
  stale?: boolean;
  /** Contextual destinations shown under an expanded group. */
  actions?: ReactNode;
  onFavorite: () => void; onToggle: () => void; onMove: (distance: number) => void; onSelect: () => void;
}) {
  if (row.kind === "empty") {
    return <View style={{ minHeight: compact ? 36 : 30, justifyContent: "center", paddingLeft: indentOf(row.depth, compact) + 8 }}>
      <Guides depth={row.depth} compact={compact} colors={colors} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>No non-archived sessions in this workspace</Text>
    </View>;
  }
  const label = row.kind === "group" ? groupDisplayLabel(row.group) : row.entry.title;
  // A finger gets 44pt targets at any width; a mouse keeps the dense ones.
  const coarse = compact || COARSE_POINTER;
  const handleHeight = coarse ? 44 : 32;
  const responder = useMemo(() => PanResponder.create({
    onStartShouldSetPanResponder: () => false,
    onMoveShouldSetPanResponder: (_, gesture) => arranging && !disabled && Math.abs(gesture.dy) > 8,
    onPanResponderRelease: (_, gesture) => { if (Math.abs(gesture.dy) >= handleHeight / 2) onMove(Math.round(gesture.dy / handleHeight)); },
    onPanResponderTerminationRequest: () => false,
  }), [arranging, disabled, handleHeight, onMove]);
  const indent = indentOf(row.depth, compact);
  const handle = arranging ? <View {...responder.panHandlers}>
    <Pressable accessibilityRole="adjustable" accessibilityLabel={`Reorder ${label}`} accessibilityHint="Select, then use Move up or Move down; drag vertically among siblings"
      accessibilityState={{ selected, disabled }} accessibilityActions={[{ name: "increment", label: "Move down" }, { name: "decrement", label: "Move up" }]}
      onAccessibilityAction={(event) => { if (disabled) return; if (event.nativeEvent.actionName === "increment") onMove(1); else if (event.nativeEvent.actionName === "decrement") onMove(-1); }}
      disabled={disabled} onPress={onSelect}
      style={{ width: coarse ? 44 : 28, minHeight: handleHeight, alignItems: "center", justifyContent: "center", borderRadius: 4, borderWidth: selected ? 1 : 0, borderColor: colors.accent, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.4 : 1 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 17 }}>⠿</Text>
    </Pressable>
  </View> : null;
  if (row.kind === "session") {
    return <View style={{ flexDirection: "row", alignItems: "center", paddingLeft: indent }}>
      <Guides depth={row.depth} compact={compact} colors={colors} />
      {handle}
      <View style={{ flex: 1, minWidth: 0 }}>{children}</View>
      {details}
      {lifecycleAction}
      <Pressable accessibilityRole="button" accessibilityLabel={`${favorite ? "Unfavorite" : "Favorite"} ${label}`} accessibilityState={{ selected: favorite, disabled }} aria-pressed={favorite} disabled={disabled} onPress={onFavorite}
        style={{ width: coarse ? 44 : 30, height: coarse ? 44 : 30, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.4 : 1 }}>
        <Text style={{ color: favorite ? colors.statusWarning : colors.foregroundMuted, fontSize: compact ? 18 : 16 }}>{favorite ? "★" : "☆"}</Text>
      </Pressable>
    </View>;
  }
  const group = row.group;
  const state = aggregateState(group.aggregate);
  const icon = group.kind === "workspace" && group.parentKey?.includes("/kind:") ? undefined : KIND_ICONS[group.kind];
  const meaning = group.kind === "workspace-kind" ? WORKSPACE_KINDS[group.label] : undefined;
  const top = row.depth === 0;
  const height = compact || top ? 44 : 40;
  const roleView = group.kind === "role" && group.parentKey === ROLE_ROOT_KEY;
  const name = roleView && group.shortCode === null ? label : `${KIND_NAMES[group.kind]} ${label}`;
  const summary = `${roleView && stale ? "Last known: " : ""}${aggregateSummary(group.aggregate)}`;
  return <View style={{ paddingLeft: indent, borderTopWidth: top ? 1 : 0, borderColor: colors.border }}>
    <Guides depth={row.depth} compact={compact} colors={colors} />
    <View style={{ flexDirection: "row", alignItems: "center" }}>
      {handle}
      <Pressable accessibilityRole="button" accessibilityState={{ expanded: row.expanded }} aria-expanded={row.expanded}
        accessibilityLabel={`${name}${meaning ? ` (${meaning})` : ""}${group.detail ? `, ${group.detail}` : ""}, ${groupCount(group)} ${group.unit}. ${summary}. ${nativeCounts(group.aggregate)}`}
        onPress={onToggle}
        style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ flex: 1, minWidth: 0, minHeight: height, flexDirection: "row", alignItems: "center", gap: compact ? 5 : 8, paddingHorizontal: 8, borderRadius: 6, backgroundColor: pressed || hovered || focused ? colors.surface2 : undefined })}>
        <Text style={{ width: 10, color: colors.foregroundMuted, fontSize: 13 }}>{row.expanded ? "⌄" : "›"}</Text>
        {icon ? <Icon name={icon} size={compact ? 14 : 16} color={colors.foregroundMuted} /> : null}
        <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: top ? 13 : 12.5, fontWeight: "600", fontFamily: group.kind === "workspace-kind" || (group.kind === "role" && group.shortCode !== null) ? "monospace" : undefined }}>{label}</Text>
        {group.detail && !compact ? <Text numberOfLines={1} style={{ flexShrink: 2, color: colors.foregroundMuted, fontSize: 11 }}>{group.detail}</Text> : null}
        <View style={{ flex: 1 }} />
        {roleView ? <StateCounts aggregate={group.aggregate} colors={colors} compact={compact} stale={stale} /> : <StateBadge state={state} colors={colors} compact={compact} />}
        {/* "No sessions" already says zero; the empty slot keeps the count column aligned. */}
        {state === null && groupCount(group) === "0" ? <View style={{ minWidth: compact ? 22 : 28 }} />
          : <Text style={{ minWidth: compact ? 22 : 28, textAlign: "center", color: colors.foregroundMuted, fontSize: 11, fontVariant: ["tabular-nums"] }}>{groupCount(group)}</Text>}
      </Pressable>
    </View>
    {row.expanded && actions ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingLeft: compact ? 22 : 34, paddingBottom: 6 }}>{actions}</View> : null}
  </View>;
}
