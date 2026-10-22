import { useMemo, type ReactNode } from "react";
import { PanResponder, Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { aggregateState, leadingStates, ROLE_ROOT_KEY, STATE_LABELS, type OrganizationAggregate, type OrganizationGroup, type OrganizationGroupKind, type OrganizationRow, type OrganizationState } from "./session-organization";
import { SessionStateMarker } from "./session-state-marker";
import { COARSE_POINTER, type Colors } from "./page-controls";
import { organizationIcon } from "./desk-icon";
import { useControlHelp } from "./control-help";
export { KIND_ICONS } from "./desk-icon";

/** Display glossary for repeated workspace-name prefixes; grouping by prefix establishes no authority. */
export const WORKSPACE_KINDS: Readonly<Record<string, string>> = {
  RSW: "Review workspace", TSW: "Task workspace", ASW: "Advisory workspace", CSW: "Committee workspace",
  ESW: "Epic workspace", ECP: "Legacy epic coordination", QSW: "Quick Session Workspace", DESK: "Durable desk",
};
export const KIND_NAMES: Readonly<Record<OrganizationGroupKind, string>> = {
  workstream: "Workstream", epic: "Epic", "workspace-kind": "Workspace kind", workspace: "Workspace", project: "Native project", role: "Role", flat: "Sessions",
};

export { stateColor } from "./session-status-ink";

/** Ratios count displayed states; native lifecycle and attention can overlap those states. */
export function groupStateSummary(aggregate: OrganizationAggregate): string {
  const unit = aggregate.total === 1 ? "session" : "sessions";
  return leadingStates(aggregate, Infinity).map(({ state, count }) => `${STATE_LABELS[state]}: ${count} of ${aggregate.total} ${unit}`).join(". ") || "No sessions";
}

export function groupStateScopeHelp(contextual: boolean): string {
  if (contextual) return "The denominator counts every descendant session in the retained surrounding context, including collapsed branches. Show in tree can include sessions outside the results' search, task or explicit state filter. Project, desk, workstream, epic and assignment scopes still apply; Current still applies when selected. Back to results returns to matching sessions.";
  return "The denominator counts descendant sessions matching the current search and filters, including collapsed branches.";
}

export function groupStateHelp(aggregate: OrganizationAggregate, contextual = false): string {
  return `${groupStateSummary(aggregate)}. ${groupStateScopeHelp(contextual)} Each session has one displayed state, in priority order: Needs input, Error, Running, Attention, Starting, Idle, Closed. Native lifecycle and attention signals can overlap; for example a running session that needs input is counted as Needs input here. These are displayed-state counts, not counts of every attention signal. ${nativeCounts(aggregate)}`;
}

/** One primary state and its share, at every depth and width; the row owns focus and full help. */
export function StateBadge({ state, aggregate, colors, compact, stale = false }: { state: OrganizationState | null; aggregate: OrganizationAggregate; colors: Colors; compact: boolean; stale?: boolean }) {
  if (state === null) return <Text style={{ color: colors.foregroundMuted, fontSize: 9, opacity: 0.75 }}>No sessions</Text>;
  const count = leadingStates(aggregate, 1)[0]!.count;
  return <View testID="organization-state-counts" style={{ alignItems: "flex-end", flexShrink: 0 }}>
    {stale ? <Text style={{ color: colors.foregroundMuted, fontSize: 9 }}>Last known</Text> : null}
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
      <SessionStateMarker state={state} colors={colors} />
      <Text style={{ color: colors.foregroundMuted, fontSize: compact ? 10 : 11, fontVariant: ["tabular-nums"] }}>{`${STATE_LABELS[state]} ${count}/${aggregate.total}`}</Text>
    </View>
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
  return `Native counts: ${native.initializing} initializing, ${native.idle} idle, ${native.running} running, ${native.error} error${native.error === 1 ? "" : "s"}, ${native.closed} closed.`;
}

export function groupDisplayLabel(group: OrganizationGroup): string {
  return group.kind === "workspace" && group.parentKey?.includes("/kind:") ? group.label.slice(group.label.indexOf("•") + 1).trim() : group.label;
}

export function OrganizationRowView({ row, colors, compact, children, details, lifecycleAction, disabled, arranging, selected, favorite, stale = false, contextual = false, actions, onFavorite, onToggle, onMove, onSelect }: {
  row: OrganizationRow; colors: Colors; compact: boolean; children?: ReactNode;
  /** A session's own control beside its row button, never inside it. */
  details?: ReactNode;
  lifecycleAction?: ReactNode;
  /** Saved-preference writes are unavailable; disclosure still works. */
  disabled: boolean;
  arranging: boolean; selected: boolean; favorite: boolean;
  /** The sessions were not read live; a Role header calls its states last known. */
  stale?: boolean;
  /** Show in tree retains surrounding sessions outside the result filters. */
  contextual?: boolean;
  /** Contextual destinations shown under an expanded group. */
  actions?: ReactNode;
  onFavorite: () => void; onToggle: () => void; onMove: (distance: number) => void; onSelect: () => void;
}) {
  const help = useControlHelp(row.kind === "group" ? `${stale ? "Last known. " : ""}${groupStateHelp(row.group.aggregate, contextual)}` : "");
  if (row.kind === "empty") {
    return <View style={{ minHeight: compact ? 36 : 30, justifyContent: "center", paddingLeft: indentOf(row.depth, compact) + 8 }}>
      <Guides depth={row.depth} compact={compact} colors={colors} />
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{row.entity === "project" ? "No native workspaces in this desk" : "No non-archived sessions in this workspace"}</Text>
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
  const icon = organizationIcon(group);
  const meaning = group.kind === "workspace-kind" ? WORKSPACE_KINDS[group.label] : undefined;
  const top = row.depth === 0;
  const height = 44;
  const roleView = group.kind === "role" && group.parentKey === ROLE_ROOT_KEY;
  const name = roleView && group.shortCode === null ? label : `${KIND_NAMES[group.kind]} ${label}`;
  const summary = `${stale ? "Last known: " : ""}${groupStateSummary(group.aggregate)}`;
  const title = <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: top ? 13 : 12.5, fontWeight: "600", fontFamily: group.kind === "workspace-kind" || (group.kind === "role" && group.shortCode !== null) ? "monospace" : undefined }}>{label}</Text>;
  return <View style={{ paddingLeft: indent, borderTopWidth: top ? 1 : 0, borderColor: colors.border }}>
    <Guides depth={row.depth} compact={compact} colors={colors} />
    <View style={{ flexDirection: "row", alignItems: "center" }}>
      {handle}
      <Pressable {...help.bindings} accessibilityRole="button" accessibilityState={{ expanded: row.expanded }} aria-expanded={row.expanded}
        accessibilityLabel={`${name}${meaning ? ` (${meaning})` : ""}${group.detail ? `, ${group.detail}` : ""}, ${groupCount(group)} ${group.unitTotal === 1 ? group.unit.slice(0, -1) : group.unit}. ${summary}. ${nativeCounts(group.aggregate)}`}
        accessibilityHint={groupStateHelp(group.aggregate, contextual)}
        onPress={() => { help.dismiss(); onToggle(); }}
        style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ flex: 1, minWidth: 0, minHeight: height, flexDirection: "row", alignItems: "center", gap: compact ? 5 : 8, paddingHorizontal: 8, borderRadius: 6, borderWidth: 1, borderColor: focused ? colors.foreground : "transparent", backgroundColor: pressed || hovered || focused ? colors.surface2 : undefined })}>
        <Text style={{ width: 10, color: colors.foregroundMuted, fontSize: 13 }}>{row.expanded ? "⌄" : "›"}</Text>
        {icon ? <Icon name={icon} size={compact ? 14 : 16} color={colors.foregroundMuted} /> : null}
        {compact && group.detail ? <View style={{ flexShrink: 1, minWidth: 0, paddingVertical: 4 }}>
          {title}
          {/* A phone has no room beside the label, so the detail takes up to two lines under it. */}
          <Text numberOfLines={2} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{group.detail}</Text>
        </View> : title}
        {group.detail && !compact ? <Text numberOfLines={1} style={{ flexShrink: 2, color: colors.foregroundMuted, fontSize: 11 }}>{group.detail}</Text> : null}
        <View style={{ flex: 1 }} />
        <StateBadge state={state} aggregate={group.aggregate} colors={colors} compact={compact} stale={stale} />
        {/* "No sessions" already says zero; the empty slot keeps the count column aligned. */}
        {state === null && groupCount(group) === "0" ? <View style={{ minWidth: compact ? 22 : 28 }} />
          : <Text style={{ minWidth: compact ? 22 : 28, textAlign: "center", color: colors.foregroundMuted, fontSize: 11, fontVariant: ["tabular-nums"] }}>{groupCount(group)}</Text>}
      </Pressable>
    </View>
    {row.expanded && actions ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6, paddingLeft: compact ? 22 : 34, paddingBottom: 6 }}>{actions}</View> : null}
  </View>;
}
