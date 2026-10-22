import { useEffect, useRef } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { useControlHelp } from "./control-help";
import type { Colors } from "./page-controls";
import type { SessionLayout } from "./session-view-store";

type Key = { key: string; target?: unknown; defaultPrevented?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyTarget = { addEventListener(type: "keydown", listener: (event: Key) => void): void; removeEventListener(type: "keydown", listener: (event: Key) => void): void };
const HELP = "Grouped shows sessions in their groups. Flat shows one list. Search, filters and sort stay the same. Your layout is remembered.";

/** One labelled radio segment per layout, independent of the collapsing filters. */
export function SessionLayoutControl({ value, colors, onChange }: {
  value: SessionLayout; colors: Colors; onChange(value: SessionLayout): void;
}) {
  const root = useRef<View | null>(null);
  const grouped = useRef<View | null>(null), flat = useRef<View | null>(null);
  const groupedHelp = useControlHelp(HELP, grouped), flatHelp = useControlHelp(HELP, flat);
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
      if (next === null) return;
      event.preventDefault(); event.stopPropagation();
      if (next !== value) onChange(next);
      (next === "grouped" ? grouped : flat).current?.focus();
    };
    target?.addEventListener("keydown", keydown);
    return () => target?.removeEventListener("keydown", keydown);
  }, [value, onChange]);
  return <View ref={root} testID="sessions-layout" accessibilityRole="radiogroup" accessibilityLabel="Session layout"
    style={{ flexDirection: "row", alignSelf: "flex-start", flexShrink: 0, padding: 2, borderWidth: 1, borderColor: colors.border, borderRadius: 9, backgroundColor: colors.surface1 }}>
    {(["grouped", "flat"] as const).map((layout) => {
      const selected = value === layout;
      const help = layout === "grouped" ? groupedHelp : flatHelp;
      return <Pressable key={layout} {...help.bindings} testID={`sessions-layout-${layout}`}
        accessibilityRole="radio" accessibilityLabel={layout === "grouped" ? "Grouped" : "Flat"}
        accessibilityHint={HELP} accessibilityState={{ checked: selected }} aria-checked={selected}
        tabIndex={Platform.OS === "web" ? selected ? 0 : -1 : undefined}
        onPress={() => { help.dismiss(); if (!selected) onChange(layout); }}
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 60, paddingHorizontal: 8,
          flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 4, borderWidth: 2, borderRadius: 6,
          borderColor: selected || focused ? colors.accent : "transparent", backgroundColor: selected || pressed ? colors.surface2 : colors.surface1,
          ...(focused && Platform.OS === "web" ? { outlineStyle: "solid", outlineWidth: 2, outlineColor: colors.foreground } : {}) })}>
        <Text accessibilityElementsHidden importantForAccessibility="no" style={{ width: 12, color: colors.foreground, fontSize: 12 }}>{selected ? "✓" : ""}</Text>
        <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{layout === "grouped" ? "Grouped" : "Flat"}</Text>
      </Pressable>;
    })}
  </View>;
}
