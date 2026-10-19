import { useMemo, useState, type ReactNode } from "react";
import { Modal, PanResponder, Pressable, Text, View } from "react-native";
import type { OrganizationRow } from "./session-organization";
import { Action, Copy, type Colors } from "./page-controls";
export function OrganizationRowView({ row, colors, height, children, disabled, favorite, onFavorite, onToggle, onMove }: {
  row: OrganizationRow; colors: Colors; height: number; children?: ReactNode; disabled: boolean;
  favorite: boolean; onFavorite: () => void; onToggle: () => void; onMove: (distance: number) => void;
}) {
  const [menu, setMenu] = useState(false);
  const label = row.kind === "group" ? row.group.label : row.entry.title;
  const displayLabel = row.kind === "group" && row.group.kind === "workspace" && row.group.parentKey?.includes("/kind:") ? label.slice(label.indexOf("•") + 1).trim() : label;
  const responder = useMemo(() => PanResponder.create({
    onStartShouldSetPanResponder: () => !disabled,
    onMoveShouldSetPanResponder: (_, gesture) => !disabled && Math.abs(gesture.dy) > 8,
    onPanResponderRelease: (_, gesture) => { if (Math.abs(gesture.dy) >= height / 2) onMove(Math.round(gesture.dy / height)); else setMenu(true); },
    onPanResponderTerminationRequest: () => false,
  }), [disabled, height, onMove]);
  return <View style={{ height, flexDirection: "row", alignItems: "center", paddingLeft: Math.min(row.depth, 3) * 10, borderBottomWidth: 1, borderColor: colors.border }}>
    <View style={{ flex: 1, minWidth: 0 }}>{row.kind === "session" ? children : <Pressable accessibilityRole="button" accessibilityState={{ expanded: row.expanded }} accessibilityLabel={`${label}, ${row.group.visibleCount} of ${row.group.totalCount} sessions`} onPress={onToggle} style={{ height, justifyContent: "center", paddingHorizontal: 8 }}>
      <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{row.expanded ? "⌄" : "›"} {displayLabel} · {row.group.visibleCount === row.group.totalCount ? row.group.totalCount : `${row.group.visibleCount} of ${row.group.totalCount}`}</Text>
      <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 10 }}>{row.group.aggregate.running} running · {row.group.aggregate.needsInput} input · {row.group.aggregate.failed} errors · {row.group.aggregate.attention} attention · {row.group.aggregate.done} closed</Text>
    </Pressable>}</View>
    {row.kind === "session" ? <Action colors={colors} label={favorite ? "★" : "☆"} selected={favorite} disabled={disabled} onPress={onFavorite} /> : null}
    <View {...responder.panHandlers} accessible accessibilityRole="adjustable" accessibilityLabel={`Reorder ${label}; drag vertically among siblings or use move controls`} accessibilityActions={[{ name: "increment", label: "Move down" }, { name: "decrement", label: "Move up" }, { name: "activate", label: "Move controls" }]}
      accessibilityState={{ disabled }} onAccessibilityAction={(event) => { if (disabled) return; if (event.nativeEvent.actionName === "increment") onMove(1); else if (event.nativeEvent.actionName === "decrement") onMove(-1); else setMenu(true); }} style={{ minWidth: 44, height: 44, justifyContent: "center", alignItems: "center", opacity: disabled ? 0.4 : 1 }}>
      <Pressable accessibilityRole="button" accessibilityLabel={`Move ${label}`} disabled={disabled} onPress={() => setMenu(true)} style={{ minWidth: 44, minHeight: 44, alignItems: "center", justifyContent: "center" }}><Text style={{ color: colors.foregroundMuted }}>↕</Text></Pressable>
    </View>
    <Modal visible={menu} transparent onRequestClose={() => setMenu(false)}>
      <View accessibilityViewIsModal style={{ flex: 1, justifyContent: "center", padding: 24, backgroundColor: "#00000066" }}><View style={{ backgroundColor: colors.surface0, padding: 16, borderRadius: 12 }}>
        <Copy colors={colors}>{label} — sibling order only</Copy>
        <Action colors={colors} label="Move up" disabled={disabled} onPress={() => { onMove(-1); setMenu(false); }} />
        <Action colors={colors} label="Move down" disabled={disabled} onPress={() => { onMove(1); setMenu(false); }} />
        <Action colors={colors} label="Close" onPress={() => setMenu(false)} />
      </View></View>
    </Modal>
  </View>;
}
