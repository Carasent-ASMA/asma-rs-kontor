import { useState, type Ref } from "react";
import { Platform, Pressable, ScrollView, Text, View, useWindowDimensions } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { spaceActivates, type Colors } from "./page-controls";
import { useControlHelp } from "./control-help";
import { TAKEOVER_EFFECTS } from "./session-takeover";

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
 * reason is written under the bar. The bar's text is bounded: at most two one-line reasons, and the takeover disclosure
 * clamped to two lines with a Limits control that holds the whole text open in the existing help popover (R38 D1).
 */
export function SelectionBar({ count, takeover, auto, archive, colors, archiveRef, disclosure = null, hasLimits = false }: {
  count: number; takeover: SelectionControl; auto: SelectionControl; archive: SelectionControl; colors: Colors;
  /** What Take over does and the host's stated limits, shown while it can start. */
  disclosure?: string | null;
  /** The host stated limits beyond the fixed effects sentence: the Limits control exists only then (R38 V2). */
  hasLimits?: boolean;
  /** Archive N, where focus returns after its review closes. */
  archiveRef?: Ref<View>;
}) {
  const narrow = useWindowDimensions().width < NARROW;
  const none = count ? null : "Select sessions first.";
  const reasons = [...new Set([none ?? takeover.unavailable ?? auto.unavailable, none ? null : archive.unavailable].filter((reason): reason is string => !!reason))];
  const effects = disclosure && !none && !takeover.unavailable ? `Take over: ${disclosure}` : null;
  // R38-D3-r1: Yoga's StretchFlexBasis errata (RN 0.81.5's default) inflates a wrapping row — the
  // container reported two 44 pt lines while every button sat on line one, the dead space under the
  // buttons Igor reported. The row therefore never wraps. Stacking Archive under the takeover group
  // is an explicit, measured choice, made from the labels' INTRINSIC widths measured off the layout
  // path (a stretched button's width depends on the very line count this decides, which would feed
  // back into itself). Both inputs are stable across a stack, so the decision is too.
  const takeoverLabel = `Take over ${count}`;
  const archiveLabel = `Archive ${count}`;
  const [labelWidths, setLabelWidths] = useState<{ takeover: number; auto: number; archive: number } | null>(null);
  const [rowWidth, setRowWidth] = useState(0);
  const pad = narrow ? 8 : 10, innerGap = narrow ? 4 : 6, iconWidth = narrow ? 0 : 16;
  const takeoverGroupWidth = labelWidths === null ? null : pad * 2 + iconWidth + innerGap + labelWidths.takeover + 1 + (pad * 2 + 14 + innerGap + labelWidths.auto);
  const archiveWidth = labelWidths === null ? null : pad * 2 + 2 + iconWidth + innerGap + labelWidths.archive; // +2: Archive's 1 pt border, border-box
  const stacked = takeoverGroupWidth !== null && archiveWidth !== null && rowWidth > 0 && takeoverGroupWidth + 8 + archiveWidth > rowWidth;
  const measure = (key: "takeover" | "auto" | "archive") => (event: { nativeEvent: { layout: { width: number } } }) =>
    setLabelWidths((current) => ({ takeover: 0, auto: 0, archive: 0, ...current, [key]: event.nativeEvent.layout.width }));
  // The host's limits leave the layout. They open held in the existing ControlHelp popover, collapsed by
  // default and never persisted; `expanded` mirrors the provider (true iff the popover is showing this
  // control's text), so its own dismissals — Escape, an outside press, scroll, resize, back — close the
  // state too and the next press on this control opens it again.
  const limits = useControlHelp(effects ?? "");
  // The bar never grows past 60% of its parent: very large text scrolls inside it instead of pushing the
  // session list out (R38 D2). In the switch sheet the footer has no definite height, so there the sheet's
  // own scroll owns the bound. R38-D2-r2: the bar carries NO minHeight of its own — on a short root the
  // list-floor cap keeps the buttons' 112 pt and the bar takes what is left, its ScrollView scrolling
  // first the text and then the buttons; nothing on the Sessions screen draws under the nav, and no
  // label is ever cut.
  return <View testID="selection-bar" style={{ paddingTop: 6, borderTopWidth: 1, borderColor: colors.border, flexShrink: 1, maxHeight: "60%" }}>
    <ScrollView style={{ flexGrow: 0, flexShrink: 1 }} contentContainerStyle={{ gap: 4 }}>
      {/* Buttons keep their labels' width. Archive stacks under the takeover group when the measured
        intrinsic widths do not fit one line (large text), rather than any label being cut. The row never
        uses flexWrap: see R38-D3-r1 above. */}
      <View testID="selection-buttons-row" onLayout={(event) => setRowWidth(event.nativeEvent.layout.width)}
        style={{ flexDirection: stacked ? "column" : "row", gap: 8 }}>
        <View style={{ flexGrow: stacked ? 0 : 3, flexBasis: "auto", alignSelf: stacked ? "stretch" : undefined, flexDirection: "row", gap: 1, borderRadius: 8, overflow: "hidden" }}>
          <BarButton testID="selection-takeover" grow={2} narrow={narrow} filled icon="RefreshCw" label={takeoverLabel} accessibilityLabel={`Take over ${plural(count, "selected session")} on each seat's chain`}
            unavailable={none ?? takeover.unavailable} colors={colors} onPress={takeover.onPress} />
          <BarButton testID="selection-takeover-auto" grow={0} narrow={narrow} filled label="Auto" trailing="ChevronDown" accessibilityLabel={`Choose a model for taking over ${plural(count, "selected session")}`}
            unavailable={none ?? takeover.unavailable ?? auto.unavailable} colors={colors} onPress={auto.onPress} />
        </View>
        <BarButton ref={archiveRef} testID="selection-archive" grow={stacked ? 0 : 2} narrow={narrow} filled={false} icon="Archive" label={archiveLabel} accessibilityLabel={`Review archive of ${plural(count, "selected session")}`}
          unavailable={none ?? archive.unavailable} colors={colors} onPress={archive.onPress} />
      </View>
      {/* Off-screen measurer for the labels' intrinsic widths; hidden from accessibility and touch. */}
      <View testID="selection-label-measure" aria-hidden accessibilityElementsHidden importantForAccessibility="no-hide-descendants" pointerEvents="none"
        style={{ position: "absolute", top: 0, left: 0, flexDirection: "row", opacity: 0 }}>
        <Text onLayout={measure("takeover")} numberOfLines={1} style={{ fontSize: 13, fontWeight: "600" }}>{takeoverLabel}</Text>
        <Text onLayout={measure("auto")} numberOfLines={1} style={{ fontSize: 13, fontWeight: "600" }}>Auto</Text>
        <Text onLayout={measure("archive")} numberOfLines={1} style={{ fontSize: 13, fontWeight: "600" }}>{archiveLabel}</Text>
      </View>
      {/* Today's distinct reason set, in today's order: the takeover group, then Archive. Each clamps to one line; the full reason stays on its control's accessibilityHint. */}
      {reasons.map((reason) => <Text key={reason} numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 11 }}>{reason}</Text>)}
      {effects ? <View testID="selection-takeover-effects" style={{ flexDirection: "row", alignItems: "flex-start", gap: 6 }}>
        {/* D1: the host limits leave the inline text — the line is the fixed effects sentence only
          (R38 V2); the full effects-plus-limits disclosure stays behind the Limits popover. R38 F2:
          lineHeight 22 keeps the two-line row at the 44 pt the Limits target occupies, so the row
          carries no empty band under the last text line (the ≤ 8 pt breath holds). */}
        <Text testID="selection-takeover-disclosure" numberOfLines={2} style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 22, flex: 1 }}>{`Take over: ${TAKEOVER_EFFECTS}`}</Text>
        {hasLimits ? <Pressable testID="selection-limits" {...limits.bindings} accessibilityRole="button" accessibilityLabel="Take over limits"
          accessibilityState={{ expanded: limits.open }} accessibilityHint="Opens the full take over effects and this host's stated limits."
          hitSlop={6} onPress={() => (limits.open ? limits.dismiss() : limits.hold())}
          style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 44, alignItems: "center", justifyContent: "center", borderRadius: 6, ...focusRing(colors, focused) })}>
          <Text numberOfLines={1} style={{ color: colors.accent, fontSize: 11, fontWeight: "600" }}>Limits</Text>
        </Pressable> : null}
      </View> : null}
    </ScrollView>
  </View>;
}
