import type { ReactNode } from "react";
import type { PluginTheme } from "@getpaseo/plugin";
import { Text, View } from "react-native";
import type { DetailValue } from "./session-metadata-content";

/** Named groups share the same reading order and exact, selectable values. */
export function SessionDetailSection({ title, values, colors, children }: {
  readonly title: string;
  readonly values?: readonly DetailValue[];
  readonly colors: PluginTheme["colors"];
  readonly children?: ReactNode;
}) {
  return <View style={{ paddingTop: 10, paddingBottom: 6, gap: 4 }}>
    <Text accessibilityRole="header" style={{ paddingHorizontal: 6, color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{title}</Text>
    {values?.map(({ label, value }) => <View key={label} style={{ paddingHorizontal: 6, paddingVertical: 3 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{label}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{value}</Text>
    </View>)}
    {children}
  </View>;
}
