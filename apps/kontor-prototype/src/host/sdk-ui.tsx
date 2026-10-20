/**
 * Fixture stand-in for `@getpaseo/plugin/client/ui`. Only SidebarRow is used by
 * the Kontor plugin; it is drawn approximately like the Paseo sidebar row.
 */
import type { ReactNode } from "react";
import { Pressable, Text, View } from "react-native-web";
import { Icon } from "./sdk-react-native";
import { shellColors } from "./theme";

export function SidebarRow({ icon, label, onPress, active, trailing }: { id?: string; icon?: string; label?: string; onPress(): void; active?: boolean; trailing?: ReactNode }) {
  const colors = shellColors();
  return <Pressable accessibilityRole="button" accessibilityState={{ selected: !!active }} onPress={onPress}
    style={({ hovered }: { pressed: boolean; hovered?: boolean }) => ({ flexDirection: "row", alignItems: "center", gap: 8, minHeight: 32, paddingHorizontal: 10, borderRadius: 6, backgroundColor: active ? colors.surface2 : hovered ? colors.surface1 : undefined })}>
    {icon ? <Icon name={icon} size={16} color={colors.foregroundMuted} /> : null}
    <Text style={{ flex: 1, color: colors.foreground, fontSize: 13 }}>{label}</Text>
    {trailing}
  </Pressable>;
}

export function SidebarSeparator() {
  const colors = shellColors();
  return <View style={{ height: 1, backgroundColor: colors.border, marginVertical: 6 }} />;
}

export function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  // react-native-web renders `href` as an anchor; React Native's Text types do not declare it.
  return <Text accessibilityRole="link" {...({ href } as object)}>{children}</Text>;
}
