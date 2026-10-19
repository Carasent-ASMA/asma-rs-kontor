import { useState, type ReactNode, type Ref } from "react";
import { Pressable, Text, View } from "react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { Icon } from "@getpaseo/plugin/client/react-native";
export type Colors = PluginTheme["colors"];
/** Wide tabs follow the prototype's compact left-grouped strip; stacked tabs are the compact bottom bar. */
export function Action({ label, accessibilityLabel, onPress, colors, icon, stacked = false, tab = false, selected = false, disabled = false, expanded, trailing, small = false, ref }: {
  label: string; onPress: () => void; colors: Colors; selected?: boolean; disabled?: boolean;
  icon?: string;
  stacked?: boolean; tab?: boolean;
  accessibilityLabel?: string;
  /** Menu or disclosure trigger state. */
  expanded?: boolean; trailing?: string; small?: boolean;
  ref?: Ref<View>;
}) {
  const strip = !stacked && (tab || trailing !== undefined);
  const compactButton = small && !stacked && !strip;
  return <Pressable ref={ref} accessibilityRole={tab ? "tab" : "button"} accessibilityLabel={accessibilityLabel ?? label} accessibilityState={{ selected, disabled, ...(expanded === undefined ? {} : { expanded }) }}
    disabled={disabled} onPress={onPress} style={strip ? { minHeight: 32, paddingHorizontal: 10, paddingVertical: 7, borderRadius: 5, justifyContent: "center", flexDirection: "row", alignItems: "center", gap: 4,
      backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 } : compactButton ? { minHeight: 30, minWidth: 30, paddingHorizontal: 9, justifyContent: "center", flexDirection: "row", alignItems: "center", gap: 5,
      borderRadius: 6, borderWidth: 1, borderColor: colors.border, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 } : { minHeight: stacked ? 48 : 44, minWidth: 44, flex: stacked ? 1 : undefined, paddingHorizontal: stacked ? 2 : 10, justifyContent: "center",
      borderRadius: stacked ? 6 : 8, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1, flexDirection: stacked ? "column" : "row", alignItems: "center", gap: stacked ? 3 : 5 }}>
    {icon && !strip ? <Icon name={icon} size={stacked ? 19 : 16} color={selected ? colors.accent : colors.foregroundMuted} /> : null}
    <Text numberOfLines={1} ellipsizeMode="tail" style={{ flexShrink: 1, color: strip ? (selected ? colors.foreground : colors.foregroundMuted) : selected ? colors.accent : colors.foreground, fontSize: stacked ? 10 : strip || compactButton ? 12 : 13, fontWeight: strip && selected ? "600" : compactButton ? "600" : "400" }}>{label}</Text>
    {trailing ? <Text importantForAccessibility="no" accessibilityElementsHidden style={{ color: colors.foregroundMuted, fontSize: 11 }}>{trailing}</Text> : null}
  </Pressable>;
}
export function Copy({ children, colors, muted = false, small = false }: { children: ReactNode; colors: Colors; muted?: boolean; small?: boolean }) {
  return <Text selectable style={{ color: muted ? colors.foregroundMuted : colors.foreground, fontSize: small ? 12 : 13, lineHeight: small ? 18 : 20 }}>{children}</Text>;
}
export function Card({ title, children, colors }: { title: string; children: ReactNode; colors: Colors }) {
  return <View style={{ padding: 12, gap: 6, borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 15, fontWeight: "600" }}>{title}</Text>{children}
  </View>;
}
/** Disclosure with expanded semantics; pass open/onToggle to keep its state in a view store. */
export function Disclosure({ label, colors, children, open, onToggle, small = false }: { label: string; colors: Colors; children: ReactNode; open?: boolean; onToggle?: (open: boolean) => void; small?: boolean }) {
  const [local, setLocal] = useState(false);
  const expanded = open ?? local;
  return <View>
    <Pressable accessibilityRole="button" accessibilityState={{ expanded }} accessibilityLabel={label} onPress={() => { setLocal(!expanded); onToggle?.(!expanded); }} style={{ minHeight: small ? 30 : 36, justifyContent: "center", alignSelf: "flex-start" }}>
      <Text style={{ color: small ? colors.foregroundMuted : colors.accent, fontSize: small ? 12 : 13 }}>{expanded ? "▾" : "▸"} {label}</Text>
    </Pressable>
    {expanded ? children : null}
  </View>;
}
