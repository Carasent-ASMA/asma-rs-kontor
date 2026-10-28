import { Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { useControlHelp } from "./control-help";
import { deskIcon } from "./desk-icon";
import type { Colors } from "./page-controls";

/** IDs remain the saved scope values; short captions supplement the icons on every input type.
 * Outside desks keeps "Other work" as its saved ID, distinct from the Other work association group.
 */
export const SESSION_SHORTCUTS = [
  { id: null, label: "All sessions", caption: "All", icon: "Layers", help: "Include sessions from every desk and outside desks." },
  { id: "PR Review", label: "PR Review", caption: "Review", icon: deskIcon("pr-review"), help: "Show PR Review desk sessions, including desk associations outside native desk projects." },
  { id: "Adam", label: "Adam desk", caption: "Adam", icon: deskIcon("adam"), help: "Show Adam desk sessions, including desk associations outside native desk projects." },
  { id: "Tickets", label: "Tickets desk", caption: "Tickets", icon: deskIcon("tickets"), help: "Show Tickets desk sessions, including desk associations outside native desk projects." },
  { id: "Other work", label: "Outside desks", caption: "Outside", icon: "Compass", help: "Show sessions outside every desk, including workstreams and sessions with no desk association." },
] as const;

type Shortcut = (typeof SESSION_SHORTCUTS)[number];
export function SessionScopeChip({ choice, count, active, colors, disabled = false, onFocus, onBlur, onPress, testID }: {
  choice: Shortcut; count: number | null; active: boolean; colors: Colors; disabled?: boolean;
  onFocus?: () => void; onBlur?: () => void; onPress(): void; testID?: string;
}) {
  const countLabel = count === null ? "desk count unavailable" : `${count} session${count === 1 ? "" : "s"}`;
  const explanation = count === null
    ? `${choice.help} The shared catalog does not carry session labels, so this desk count is unavailable. Search and other filters still apply.`
    : `${choice.help} Search and other filters still apply.`;
  const help = useControlHelp(`${choice.label}, ${countLabel}. ${explanation} Counts are before search and other filters.`);
  return <Pressable {...help.bindings} testID={testID} accessibilityRole="button"
    accessibilityLabel={`${choice.label}, ${countLabel}`} accessibilityHint={explanation}
    accessibilityState={{ selected: active, disabled }} aria-pressed={active} disabled={disabled}
    onFocus={() => { help.bindings.onFocus(); onFocus?.(); }} onBlur={() => { help.bindings.onBlur(); onBlur?.(); }}
    onPress={() => { help.dismiss(); onPress(); }}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({
      minWidth: 58, minHeight: 44, paddingHorizontal: 7, paddingVertical: 4, alignItems: "center", justifyContent: "center", gap: 2,
      borderRadius: 7, borderWidth: 2, borderColor: focused ? colors.foreground : active ? colors.accent : colors.border,
      backgroundColor: active || pressed || hovered ? colors.surface2 : colors.surface1, opacity: disabled ? 0.5 : pressed ? 0.7 : 1,
    })}>
    <View accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
      <Icon name={choice.icon} size={16} color={colors.foreground} />
      <Text style={{ color: colors.foreground, fontSize: 12, fontVariant: ["tabular-nums"] }}>{count ?? "—"}</Text>
    </View>
    <Text style={{ color: colors.foreground, fontSize: 10, fontWeight: active ? "600" : "400", textDecorationLine: active ? "underline" : "none" }}>{choice.caption}</Text>
  </Pressable>;
}
