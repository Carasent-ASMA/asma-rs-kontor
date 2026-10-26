import { sessionStatusInk } from "./session-status-ink";
import { useEffect, useImperativeHandle, useRef, useState, type ComponentRef, type Ref, type RefObject } from "react";
import { Pressable, Text, View, useWindowDimensions } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useActionHelp, useControlHelp } from "./control-help";
import { focusControl } from "./mobile-filters";
import { type Colors } from "./page-controls";
import { quickFilterLabel, QUICK_FILTER_ICONS, QUICK_FILTER_MEANINGS, visibleQuickFilters } from "./mobile-quick-filter-options";
import type { SessionStateCount } from "./session-state-counts";
import { MORE_STATUS_FILTERS, QUICK_STATUS_FILTERS, type StatusFilter } from "./session-view";

/** One existing pressable owns activation and shared help: hover, focus, or a long hold on touch. */
export function QuickToolbarButton({ label, help = label, icon, iconColor, colors, onPress, selected, expanded, count, caption, testID, target = 44, ref }: {
  label: string; help?: string; icon: string; iconColor?: string; colors: Colors; onPress(): void;
  selected?: boolean; expanded?: boolean; count?: SessionStateCount; caption?: string; testID?: string; target?: number; ref?: Ref<View>;
}) {
  const controlHelp = useActionHelp(help, ref);
  return <Pressable {...controlHelp.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={label}
    accessibilityState={{ ...(selected === undefined ? {} : { selected }), ...(expanded === undefined ? {} : { expanded }) }}
    aria-pressed={selected} aria-expanded={expanded}
    onPress={() => { controlHelp.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ width: target, minHeight: target, flexShrink: 0, paddingVertical: 3,
      alignItems: "center", justifyContent: "center", gap: 1, borderRadius: 7, borderWidth: selected || focused ? 2 : 1,
      borderColor: selected || focused ? colors.accent : colors.border, backgroundColor: pressed || selected ? colors.surface2 : colors.surface1 })}>
    {count?.denominator !== undefined ? <View testID="session-state-ratio" style={{ flexDirection: "row", alignItems: "center", gap: 1 }}>
      <Icon name={icon} size={18} color={iconColor ?? colors.foreground} />
      <StateRatio count={count} colors={colors} />
    </View> : <>
      <Icon name={icon} size={18} color={iconColor ?? (selected ? colors.accent : colors.foreground)} />
      {count !== undefined || caption ? <Text numberOfLines={1} style={{ maxWidth: "100%", color: colors.foregroundMuted, fontSize: 10, lineHeight: 13 }}>{caption ?? count?.text}</Text> : null}
    </>}
    {selected ? <View importantForAccessibility="no" accessibilityElementsHidden style={{ position: "absolute", top: 1, right: 1 }}><Icon name="Check" size={10} color={colors.foreground} /></View> : null}
  </Pressable>;
}

/** Filtered over total, split by a rule, once another filter narrows a state; the exact counts stay in the label. */
export function StateRatio({ count, colors }: { count: SessionStateCount; colors: Pick<Colors, "foreground" | "foregroundMuted"> }) {
  return <View testID="session-state-ratio-count" style={{ alignItems: "stretch" }}>
    <Text style={{ textAlign: "center", color: colors.foreground, fontSize: 10, lineHeight: 13, fontVariant: ["tabular-nums"], borderBottomWidth: 1, borderBottomColor: colors.foregroundMuted }}>{count.numerator}</Text>
    <Text style={{ textAlign: "center", color: colors.foregroundMuted, fontSize: 10, lineHeight: 13, fontVariant: ["tabular-nums"] }}>{count.denominator}</Text>
  </View>;
}

type SearchProps = {
  colors: Colors; query: string; searchInputRef: RefObject<ComponentRef<typeof TextInput> | null>; onQuery(query: string): void;
};

/**
 * Compact Search, first in the row with Select and layout. Open, it takes a full line of its own with Clear or Close, so the
 * controls stay underneath and closing returns focus to the trigger.
 */
export function CompactSearch({ colors, query, searchInputRef, onQuery }: SearchProps) {
  const { fontScale } = useWindowDimensions();
  const [open, setOpen] = useState(false);
  const help = useControlHelp("");
  const triggerRef = useRef<View | null>(null);
  const target = Math.max(44, Math.ceil(36 * fontScale));
  useEffect(() => {
    if (!open) return;
    const timer = setTimeout(() => searchInputRef.current?.focus(), 0);
    return () => clearTimeout(timer);
  }, [open, searchInputRef]);
  const close = () => {
    searchInputRef.current?.blur(); setOpen(false);
    setTimeout(() => { focusControl(triggerRef.current); help.dismiss(); }, 0);
  };
  if (!open) return <QuickToolbarButton ref={triggerRef} testID="sessions-mobile-search-trigger" label={query ? `Search sessions, current search: ${query}` : "Search sessions"}
    icon="Search" caption={query ? "Edit" : undefined} expanded={false} colors={colors} target={target} onPress={() => setOpen(true)} />;
  return <View testID="sessions-mobile-search" style={{ flexBasis: "100%", flexDirection: "row", alignItems: "center", gap: 4 }}>
    <TextInput ref={searchInputRef} testID="sessions-search" value={query} onChangeText={onQuery}
      accessibilityLabel="Search sessions by topic, role or ticket" placeholder="Search sessions…" placeholderTextColor={colors.foregroundMuted}
      autoCapitalize="none" autoCorrect={false} returnKeyType="search" onSubmitEditing={close}
      style={{ flex: 1, minWidth: 0, minHeight: target, fontSize: 16, color: colors.foreground, backgroundColor: colors.surface1,
        borderWidth: 1, borderColor: colors.border, borderRadius: 7, paddingHorizontal: 10, paddingVertical: 6 }} />
    <QuickToolbarButton label={query ? "Clear search" : "Close search"} icon={query ? "X" : "ChevronLeft"} colors={colors} target={target}
      onPress={() => { if (query) { onQuery(""); searchInputRef.current?.focus(); } else close(); }} />
  </View>;
}

type Utility = Omit<Parameters<typeof QuickToolbarButton>[0], "colors" | "target">;
const GAP = 4;

/**
 * The state toolbar Kontor's compact Sessions and Start or switch both draw. Its measured row holds as many whole state
 * tiles as fit, Last visited first, with a chosen state past them in the last tile; More and the caller's trailing action
 * keep their own room at the end. Nothing scrolls or wraps, and More reaches every state.
 */
export function StateToolbar({ testID, stateTestID, colors, status, counts, onStatus, more, trailing }: {
  testID: string; stateTestID: string; colors: Colors; status: StatusFilter; counts: Readonly<Record<StatusFilter, SessionStateCount>>;
  onStatus(status: StatusFilter): void; more: Utility; trailing: Utility;
}) {
  const { fontScale } = useWindowDimensions();
  const [width, setWidth] = useState(0);
  const target = Math.max(44, Math.ceil(36 * fontScale));
  // Until the row is measured, the narrowest supported phone row stands in, so the first frame never overflows.
  const choices = visibleQuickFilters(width || 296, status, target, GAP);
  return <View testID={testID} role="toolbar" accessibilityLabel="Session states" onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
    style={{ flexDirection: "row", alignItems: "center", gap: GAP }}>
    <View style={{ flex: 1, minWidth: 0, flexDirection: "row", gap: GAP }}>
      {choices.map((choice) => <QuickToolbarButton key={choice} testID={`${stateTestID}${choice}`} icon={QUICK_FILTER_ICONS[choice]} iconColor={sessionStatusInk(choice, colors)} colors={colors} target={target}
        label={`${quickFilterLabel(choice)}, ${counts[choice].label}`} help={`${quickFilterLabel(choice)}. ${counts[choice].label}. ${QUICK_FILTER_MEANINGS[choice]}`} count={counts[choice]} selected={status === choice} onPress={() => onStatus(choice)} />)}
    </View>
    <QuickToolbarButton {...more} colors={colors} target={target} />
    <QuickToolbarButton {...trailing} colors={colors} target={target} />
  </View>;
}

type ToolbarProps = {
  colors: Colors; status: StatusFilter; counts: Readonly<Record<StatusFilter, SessionStateCount>>;
  activeCount: number; filtersTriggerRef: RefObject<View | null>; chooserRef: RefObject<View | null>; chooserOpen: boolean;
  helpDismissRef: Ref<() => void>;
  onStatus(status: StatusFilter): void; onFilters(): void; onChooser(): void;
};

/** Kontor's compact state toolbar: More opens the nine shortcuts' chooser and Filters the full filter page, which holds the other states. */
export function MobileQuickToolbar(props: ToolbarProps) {
  const { colors, status, counts } = props;
  const help = useControlHelp("");
  useImperativeHandle(props.helpDismissRef, () => help.dismiss, [help.dismiss]);
  // A state chosen in Filters shows on the Filters trigger, as the switcher's More shows its own; it never adds a shortcut.
  const extra = MORE_STATUS_FILTERS.includes(status) ? status : null;
  return <View testID="sessions-mobile-toolbar" style={{ gap: 4 }}>
    <StateToolbar testID="sessions-mobile-quick-row" stateTestID="sessions-quick-" colors={colors} status={status} counts={counts} onStatus={props.onStatus}
      more={{ ref: props.chooserRef, testID: "sessions-mobile-quick-chooser-trigger", label: `All ${QUICK_STATUS_FILTERS.length} quick filters and icon meanings, current: ${quickFilterLabel(status)}`,
        icon: "ListFilter", caption: "More", expanded: props.chooserOpen, onPress: props.onChooser }}
      trailing={{ ref: props.filtersTriggerRef, testID: "sessions-mobile-filters-trigger",
        label: ["Filters and view", extra ? `state: ${quickFilterLabel(extra)}, ${counts[extra].label}` : null, props.activeCount ? `${props.activeCount} active filters or view choices` : null].filter(Boolean).join(", "),
        help: "Filters and view. Opens the full filter page. Choices apply immediately; Back keeps them.",
        icon: extra ? QUICK_FILTER_ICONS[extra] : "SlidersHorizontal", iconColor: extra ? sessionStatusInk(extra, colors) : undefined, caption: extra ? undefined : "Filters",
        count: extra ? counts[extra] : undefined, selected: extra ? true : undefined, onPress: props.onFilters }} />
  </View>;
}
