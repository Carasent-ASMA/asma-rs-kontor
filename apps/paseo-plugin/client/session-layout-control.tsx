import { useEffect, useRef } from "react";
import { Platform, Pressable, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
import type { Colors } from "./page-controls";
import type { SessionLayout } from "./session-view-store";

type Key = { key: string; target?: unknown; defaultPrevented?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyTarget = { addEventListener(type: "keydown", listener: (event: Key) => void): void; removeEventListener(type: "keydown", listener: (event: Key) => void): void };
const HELP = {
  grouped: "Grouped. Show sessions in their groups. Search, filters and sort stay the same.",
  flat: "Flat. Show sessions in one list. Search, filters and sort stay the same.",
};

/**
 * One labelled radio segment per layout, independent of the collapsing filters. `disabled` says why a view ignores
 * layout: the radios keep showing the saved choice and their help explains it, but change nothing. The group carries
 * the web's disabled state, since React Native Web's Pressable drops it unless pressing is off, and that would also
 * end hover and hold help.
 */
export function SessionLayoutControl({ value, colors, onChange, disabled }: {
  value: SessionLayout; colors: Colors; onChange(value: SessionLayout): void; disabled?: string;
}) {
  const root = useRef<View | null>(null);
  const grouped = useRef<View | null>(null), flat = useRef<View | null>(null);
  const groupedHelp = useActionHelp(disabled ?? HELP.grouped, grouped), flatHelp = useActionHelp(disabled ?? HELP.flat, flat);
  useEffect(() => {
    if (Platform.OS !== "web") return;
    const target = root.current as unknown as KeyTarget | null;
    const keydown = (event: Key) => {
      if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return;
      // React Native Web's Pressable supplies Enter, but Space only for button roles.
      const next = event.key === " " || event.key === "Spacebar"
        ? event.target === grouped.current ? "grouped" : "flat"
        : ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)
        ? value === "grouped" ? "flat" : "grouped"
        : event.key === "Home" ? "grouped" : event.key === "End" ? "flat" : null;
      if (next === null || disabled) return;
      event.preventDefault(); event.stopPropagation();
      if (next !== value) onChange(next);
      (next === "grouped" ? grouped : flat).current?.focus();
    };
    target?.addEventListener("keydown", keydown);
    return () => target?.removeEventListener("keydown", keydown);
  }, [value, onChange, disabled]);
  return <View ref={root} testID="sessions-layout" accessibilityRole="radiogroup" accessibilityLabel="Session layout" aria-disabled={disabled ? true : undefined}
    style={{ flexDirection: "row", flexWrap: "wrap", alignSelf: "flex-start", gap: 2, flexShrink: 1 }}>
    {(["grouped", "flat"] as const).map((layout) => {
      const selected = value === layout;
      const help = layout === "grouped" ? groupedHelp : flatHelp;
      return <Pressable key={layout} {...help.bindings} testID={`sessions-layout-${layout}`}
        accessibilityRole="radio" accessibilityLabel={layout === "grouped" ? "Grouped" : "Flat"}
        accessibilityHint={disabled ?? HELP[layout]} accessibilityState={{ checked: selected, ...(disabled ? { disabled: true } : {}) }} aria-checked={selected}
        tabIndex={Platform.OS === "web" ? selected ? 0 : -1 : undefined}
        onPress={() => { if (disabled) { help.hold(); return; } help.dismiss(); if (!selected) onChange(layout); }}
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 44, paddingHorizontal: 8, paddingVertical: 6,
          alignItems: "center", justifyContent: "center", borderWidth: 1, borderRadius: 6,
          borderColor: focused ? colors.foreground : selected ? colors.accent : "transparent", backgroundColor: selected || pressed ? colors.surface2 : undefined,
          opacity: disabled ? 0.5 : 1,
          ...(focused && Platform.OS === "web" ? { outlineStyle: "solid", outlineWidth: 2, outlineColor: colors.foreground } : {}) })}>
        <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name={layout === "grouped" ? "ListTree" : "List"} size={18} color={selected ? colors.foreground : colors.foregroundMuted} /></View>
        {selected ? <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ position: "absolute", right: 3, top: 3 }}><Icon name="Check" size={9} color={colors.foreground} /></View> : null}
      </Pressable>;
    })}
  </View>;
}
