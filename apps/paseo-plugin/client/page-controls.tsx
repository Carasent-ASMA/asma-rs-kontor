import { useState, type ReactNode, type Ref } from "react";
import { Pressable, Text, View } from "react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { useActionHelp, useControlHelp } from "./control-help";
import { Icon } from "@getpaseo/plugin/client/react-native";
export type Colors = PluginTheme["colors"];
/** Wide tabs follow the prototype's compact left-grouped strip; stacked tabs are the compact bottom bar. A touch pointer gets 44pt in every style. */
export function Action({ label, accessibilityLabel, onPress, colors, icon, stacked = false, tab = false, selected: selectedProp, disabled = false, expanded, trailing, small = false, iconOnly = false, help, ref }: {
  label: string; onPress: () => void; colors: Colors; selected?: boolean; disabled?: boolean;
  icon?: string;
  /** Only familiar secondary actions opt in. Touch keeps the visible label. */
  iconOnly?: boolean; help?: string;
  stacked?: boolean; tab?: boolean;
  accessibilityLabel?: string;
  /** Menu or disclosure trigger state. */
  expanded?: boolean; trailing?: string; small?: boolean;
  ref?: Ref<View>;
}) {
  const selected = selectedProp ?? false;
  const hideLabel = iconOnly && !!icon && !COARSE_POINTER && !tab && !stacked;
  const hint = useControlHelp(hideLabel || help ? (help ?? accessibilityLabel ?? label) : "", ref);
  // Web reads state from ARIA: a tab is selected, and a button given `selected` is a toggle. A menu
  // button already says it is expanded, and its label names the current view.
  const aria = tab ? { "aria-selected": selected } : selectedProp === undefined || expanded !== undefined ? {} : { "aria-pressed": selectedProp };
  const strip = !stacked && (tab || trailing !== undefined);
  const compactButton = small && !stacked && !strip;
  return <Pressable {...hint.bindings} accessibilityRole={tab ? "tab" : "button"} accessibilityLabel={accessibilityLabel ?? label} accessibilityState={{ ...(tab || selectedProp !== undefined ? { selected } : {}), disabled, ...(expanded === undefined ? {} : { expanded }) }} aria-expanded={expanded} {...aria}
    disabled={disabled} onPress={() => { hint.dismiss(); onPress(); }} style={hideLabel ? { minWidth: 34, minHeight: 30, paddingHorizontal: 7, alignItems: "center", justifyContent: "center", borderRadius: 6, borderWidth: 1, borderColor: colors.border, opacity: disabled ? 0.5 : 1, backgroundColor: selected ? colors.surface2 : undefined } : strip ? { minHeight: COARSE_POINTER ? 44 : 32, paddingHorizontal: 10, paddingVertical: 7, borderRadius: 5, justifyContent: "center", flexDirection: "row", alignItems: "center", gap: 4,
      backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 } : compactButton ? { minHeight: COARSE_POINTER ? 44 : 30, minWidth: COARSE_POINTER ? 44 : 30, paddingHorizontal: 9, justifyContent: "center", flexDirection: "row", alignItems: "center", gap: 5,
      borderRadius: 6, borderWidth: 1, borderColor: colors.border, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 } : { minHeight: stacked ? 48 : 44, minWidth: 44, flex: stacked ? 1 : undefined, paddingHorizontal: stacked ? 2 : 10, justifyContent: "center",
      borderRadius: stacked ? 6 : 8, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1, flexDirection: stacked ? "column" : "row", alignItems: "center", gap: stacked ? 3 : 5 }}>
    {icon && (!strip || hideLabel) ? <Icon name={icon} size={stacked ? 19 : 16} color={selected ? colors.accent : colors.foregroundMuted} /> : null}
    {hideLabel ? null : <Text numberOfLines={1} ellipsizeMode="tail" style={{ flexShrink: 1, color: strip ? (selected ? colors.foreground : colors.foregroundMuted) : selected ? colors.accent : colors.foreground, fontSize: stacked ? 10 : strip || compactButton ? 12 : 13, fontWeight: strip && selected ? "600" : compactButton ? "600" : "400" }}>{label}</Text>}
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
/** Square icon button; a touch pointer gets a real 44pt target, since the web ignores a hit slop. A disabled button keeps its place. `expanded` makes it a menu trigger. */
export function IconButton({ label, icon, colors, onPress, disabled = false, expanded, ref }: { label: string; icon: string; colors: Colors; onPress: () => void; disabled?: boolean; expanded?: boolean; ref?: Ref<View> }) {
  // A long hold shows the label instead of activating on release.
  const hint = useActionHelp(label, ref);
  const size = COARSE_POINTER ? 44 : 34;
  return <Pressable {...hint.bindings} accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled, ...(expanded === undefined ? {} : { expanded }) }} aria-expanded={expanded} aria-disabled={disabled} disabled={disabled} onPress={() => { hint.dismiss(); onPress(); }} style={{ width: size, height: size, borderRadius: 7, borderWidth: 1, borderColor: expanded ? colors.accent : colors.border, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.45 : 1 }}>
    <Icon name={icon} size={16} color={disabled ? colors.foregroundMuted : colors.foreground} />
  </Pressable>;
}
/** True where a finger is the primary pointer: native apps, and web on phones and tablets. A mouse keeps the denser rows. */
export const COARSE_POINTER = (() => {
  const matchMedia = (globalThis as { matchMedia?: (query: string) => { matches: boolean } }).matchMedia;
  return typeof matchMedia !== "function" || matchMedia("(pointer: coarse)").matches;
})();
/** Disclosure with expanded semantics; pass open/onToggle to keep its state in a view store. Touch gets a 44pt row. */
export function Disclosure({ label, accessibilityLabel, colors, children, open, onToggle, small = false }: { label: string; accessibilityLabel?: string; colors: Colors; children: ReactNode; open?: boolean; onToggle?: (open: boolean) => void; small?: boolean }) {
  const [local, setLocal] = useState(false);
  const expanded = open ?? local;
  return <View>
    <Pressable accessibilityRole="button" accessibilityState={{ expanded }} aria-expanded={expanded} accessibilityLabel={accessibilityLabel ?? label} onPress={() => { setLocal(!expanded); onToggle?.(!expanded); }} style={{ minHeight: COARSE_POINTER ? 44 : small ? 30 : 36, minWidth: 44, justifyContent: "center", alignSelf: "flex-start" }}>
      <Text style={{ color: small ? colors.foregroundMuted : colors.accent, fontSize: small ? 12 : 13 }}>{expanded ? "▾" : "▸"} {label}</Text>
    </Pressable>
    {expanded ? children : null}
  </View>;
}
