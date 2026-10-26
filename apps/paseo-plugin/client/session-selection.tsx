import type { Ref } from "react";
import { Platform, Pressable, Text, View, useWindowDimensions } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { spaceActivates, type Colors } from "./page-controls";

const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
const focusRing = (colors: Colors, focused?: boolean) => focused && Platform.OS === "web" ? { outlineStyle: "solid" as const, outlineWidth: 2, outlineColor: colors.accent } : {};

/** A control that can be refused: `unavailable` says why, and the control stays visible and says so. */
export type SelectionControl = { readonly unavailable: string | null; readonly onPress: () => void };

/**
 * Board D1's header: "N / total selected" over the current matches, the select-all icon over every known match,
 * including rows scrolled out of view, and Done. A selected session the filters now hide stays selected and is counted
 * under the header instead, so N never exceeds the total.
 */
export function SelectionHeader({ count, total, hidden, selectAll, colors, onDone }: {
  /** Every selected session, shown or not. */
  count: number;
  /** Every known match of the current view. */
  total: number;
  /** Selected sessions the current view does not show. */
  hidden: number;
  /** Whether every known match is selected; pressing selects them all, or clears them when they are. */
  selectAll: SelectionControl & { readonly checked: boolean };
  colors: Colors; onDone: () => void;
}) {
  const blocked = !!selectAll.unavailable;
  const label = selectAll.checked ? `Clear the ${plural(total, "matching session")}` : `Select all ${plural(total, "matching session")}`;
  return <View testID="selection-header" style={{ gap: 2 }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Text testID="selection-count" role="status" accessibilityLiveRegion="polite" style={{ flex: 1, color: colors.foreground, fontSize: 15, fontWeight: "600", fontVariant: ["tabular-nums"] }}>{`${count - hidden} / ${total} selected`}</Text>
      <Pressable testID="selection-all" accessibilityRole="checkbox" accessibilityLabel={label} accessibilityHint={selectAll.unavailable ?? undefined}
        accessibilityState={{ checked: selectAll.checked, disabled: blocked }} aria-checked={selectAll.checked} aria-disabled={blocked || undefined} disabled={blocked} {...(Platform.OS === "web" ? { tabIndex: 0, ...spaceActivates(selectAll.onPress, blocked) } : {})}
        onPress={() => { if (!blocked) selectAll.onPress(); }}
        // Checked inverts to the foreground, as the earlier checkbox did: the accent stays under 3:1 on the dark sheet surfaces.
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ width: 44, height: 44, alignItems: "center", justifyContent: "center", borderRadius: 8, borderWidth: 2,
          borderColor: focused ? colors.foreground : "transparent", backgroundColor: selectAll.checked ? colors.foreground : pressed ? colors.surface2 : undefined, opacity: blocked ? 0.4 : 1, ...focusRing(colors, focused) })}>
        <Icon name="ListChecks" size={20} color={selectAll.checked ? colors.surface0 : colors.foreground} />
      </Pressable>
      <Pressable testID="selection-done" accessibilityRole="button" accessibilityLabel="Done selecting" onPress={onDone}
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 64, paddingHorizontal: 14, borderRadius: 8, alignItems: "center", justifyContent: "center",
          borderWidth: 2, borderColor: focused ? colors.foreground : colors.accent, backgroundColor: colors.accent, opacity: pressed ? 0.85 : 1, ...focusRing(colors, focused) })}>
        <Text style={{ color: colors.accentForeground, fontSize: 13, fontWeight: "600" }}>Done</Text>
      </Pressable>
    </View>
    {hidden ? <Text testID="selection-hidden" style={{ color: colors.foregroundMuted, fontSize: 11 }}>{`${plural(hidden, "selected session")} not shown by the current filters; ${hidden === 1 ? "it stays" : "they stay"} selected.`}</Text> : null}
    {blocked && !selectAll.checked ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{selectAll.unavailable}</Text> : null}
  </View>;
}

/** Below this width the bar drops its decorative icons so every label stays whole. */
const NARROW = 360;

// A refused control is `disabled`, the only way React Native Web writes aria-disabled, and keeps tabIndex 0 so its reason stays reachable.
function BarButton({ testID, label, accessibilityLabel, icon, trailing, unavailable, filled, colors, onPress, grow, narrow, ref }: {
  testID: string; label: string; accessibilityLabel: string; icon?: string; trailing?: string; unavailable: string | null; filled: boolean; colors: Colors; onPress: () => void;
  /** The share of spare width; each button first takes its own label's width. */
  grow: number; narrow: boolean; ref?: Ref<View>;
}) {
  const blocked = !!unavailable, ink = filled && !blocked ? colors.accentForeground : blocked ? colors.foregroundMuted : colors.foreground;
  return <Pressable ref={ref} testID={testID} accessibilityRole="button" accessibilityLabel={accessibilityLabel} accessibilityHint={unavailable ?? undefined} accessibilityState={{ disabled: blocked }} aria-disabled={blocked || undefined} disabled={blocked} {...(Platform.OS === "web" ? { tabIndex: 0 } : {})}
    onPress={() => { if (!blocked) onPress(); }} style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ flexGrow: grow, flexBasis: "auto", minHeight: 44, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: narrow ? 4 : 6,
      paddingHorizontal: narrow ? 8 : 10, borderRadius: 8, borderWidth: filled ? 0 : 1, borderColor: focused ? colors.foreground : colors.border,
      backgroundColor: filled ? blocked ? colors.surface2 : colors.accent : colors.surface0, opacity: pressed ? 0.85 : 1, ...focusRing(colors, focused) })}>
    {icon && !narrow ? <Icon name={icon} size={16} color={ink} /> : null}
    <Text numberOfLines={1} style={{ color: ink, fontSize: 13, fontWeight: "600" }}>{label}</Text>
    {trailing ? <Icon name={trailing} size={14} color={ink} /> : null}
  </Pressable>;
}

/**
 * Board D1's sticky bar: [Take over N | Auto ▾] and [Archive N]. Two actions on the frozen selection, not a dismiss and
 * confirm pair. Take over starts at once on each seat's chain, so what it does and the host's stated limits are written
 * under the bar first; Auto ▾ opens the model choice and Archive opens its review. A refused action stays and its
 * reason is written under the bar.
 */
export function SelectionBar({ count, takeover, auto, archive, colors, archiveRef, disclosure = null }: {
  count: number; takeover: SelectionControl; auto: SelectionControl; archive: SelectionControl; colors: Colors;
  /** What Take over does and the host's stated limits, shown while it can start. */
  disclosure?: string | null;
  /** Archive N, where focus returns after its review closes. */
  archiveRef?: Ref<View>;
}) {
  const narrow = useWindowDimensions().width < NARROW;
  const none = count ? null : "Select sessions first.";
  const reasons = [...new Set([none ?? takeover.unavailable ?? auto.unavailable, none ? null : archive.unavailable].filter((reason): reason is string => !!reason))];
  return <View testID="selection-bar" style={{ gap: 4, paddingTop: 6, borderTopWidth: 1, borderColor: colors.border }}>
    {/* Buttons keep their labels' width; when large text no longer fits one row, Archive wraps below rather than any label being cut. */}
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
      <View style={{ flexGrow: 3, flexBasis: "auto", flexDirection: "row", gap: 1, borderRadius: 8, overflow: "hidden" }}>
        <BarButton testID="selection-takeover" grow={2} narrow={narrow} filled icon="RefreshCw" label={`Take over ${count}`} accessibilityLabel={`Take over ${plural(count, "selected session")} on each seat's chain`}
          unavailable={none ?? takeover.unavailable} colors={colors} onPress={takeover.onPress} />
        <BarButton testID="selection-takeover-auto" grow={0} narrow={narrow} filled label="Auto" trailing="ChevronDown" accessibilityLabel={`Choose a model for taking over ${plural(count, "selected session")}`}
          unavailable={none ?? takeover.unavailable ?? auto.unavailable} colors={colors} onPress={auto.onPress} />
      </View>
      <BarButton ref={archiveRef} testID="selection-archive" grow={2} narrow={narrow} filled={false} icon="Archive" label={`Archive ${count}`} accessibilityLabel={`Review archive of ${plural(count, "selected session")}`}
        unavailable={none ?? archive.unavailable} colors={colors} onPress={archive.onPress} />
    </View>
    {reasons.map((reason) => <Text key={reason} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{reason}</Text>)}
    {disclosure && !none && !takeover.unavailable ? <Text testID="selection-takeover-disclosure" style={{ color: colors.foregroundMuted, fontSize: 11 }}>{`Take over: ${disclosure}`}</Text> : null}
  </View>;
}
