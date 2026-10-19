import type { ReactNode } from "react";
import { Pressable, Text, View } from "react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { Icon } from "@getpaseo/plugin/client/react-native";
export type Colors = PluginTheme["colors"];
export function Action({ label, accessibilityLabel, onPress, colors, icon, stacked = false, tab = false, selected = false, disabled = false }: {
  label: string; onPress: () => void; colors: Colors; selected?: boolean; disabled?: boolean;
  icon?: string;
  stacked?: boolean; tab?: boolean;
  accessibilityLabel?: string;
}) {
  return <Pressable accessibilityRole={tab ? "tab" : "button"} accessibilityLabel={accessibilityLabel ?? label} accessibilityState={{ selected, disabled }}
    disabled={disabled} onPress={onPress} style={{ minHeight: stacked ? 56 : 44, minWidth: 44, flex: stacked ? 1 : undefined, paddingHorizontal: stacked ? 2 : 10, justifyContent: "center",
      borderRadius: 8, backgroundColor: selected ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1, flexDirection: stacked ? "column" : "row", alignItems: "center", gap: 5 }}>
    {icon ? <Icon name={icon} size={16} color={selected ? colors.accent : colors.foregroundMuted} /> : null}
    <Text numberOfLines={1} style={{ color: selected ? colors.accent : colors.foreground, fontSize: stacked ? 11 : 13 }}>{label}</Text>
  </Pressable>;
}
export function Copy({ children, colors, muted = false }: { children: ReactNode; colors: Colors; muted?: boolean }) {
  return <Text selectable style={{ color: muted ? colors.foregroundMuted : colors.foreground, fontSize: 13, lineHeight: 20 }}>{children}</Text>;
}
export function Card({ title, children, colors }: { title: string; children: ReactNode; colors: Colors }) {
  return <View style={{ padding: 12, gap: 6, borderWidth: 1, borderColor: colors.border, borderRadius: 10, backgroundColor: colors.surface1 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 15, fontWeight: "600" }}>{title}</Text>{children}
  </View>;
}
