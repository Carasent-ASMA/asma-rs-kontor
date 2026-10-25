import { sessionStatusInk } from "./session-status-ink";
import { useEffect, useImperativeHandle, useRef, useState, type ComponentRef, type Ref, type RefObject } from "react";
import { Pressable, Text, View, useWindowDimensions } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useControlHelp } from "./control-help";
import { focusControl } from "./mobile-filters";
import { type Colors } from "./page-controls";
import { quickFilterLabel, QUICK_FILTER_ICONS, QUICK_FILTER_MEANINGS, visibleQuickFilters } from "./mobile-quick-filter-options";
import type { SessionStateCount } from "./session-state-counts";
import { QUICK_STATUS_FILTERS, type StatusFilter } from "./session-view";

/** One existing pressable owns activation and shared pointer/keyboard help. Touch learns the icons in More. */
export function QuickToolbarButton({ label, help = label, icon, iconColor, colors, onPress, selected, expanded, count, caption, testID, target = 44, ref }: {
  label: string; help?: string; icon: string; iconColor?: string; colors: Colors; onPress(): void;
  selected?: boolean; expanded?: boolean; count?: SessionStateCount; caption?: string; testID?: string; target?: number; ref?: Ref<View>;
}) {
  const controlHelp = useControlHelp(help, ref);
  return <Pressable {...controlHelp.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={label}
    accessibilityState={{ ...(selected === undefined ? {} : { selected }), ...(expanded === undefined ? {} : { expanded }) }}
    aria-pressed={selected} aria-expanded={expanded}
    onPress={() => { controlHelp.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ width: target, minHeight: target, flexShrink: 0, paddingVertical: 3,
      alignItems: "center", justifyContent: "center", gap: 1, borderRadius: 7, borderWidth: selected || focused ? 2 : 1,
      borderColor: selected || focused ? colors.accent : colors.border, backgroundColor: pressed || selected ? colors.surface2 : colors.surface1 })}>
    {count?.denominator !== undefined ? <View testID="session-state-ratio" style={{ flexDirection: "row", alignItems: "center", gap: 1 }}>
      <Icon name={icon} size={18} color={iconColor ?? colors.foreground} />
      <View style={{ alignItems: "stretch" }}>
        <Text style={{ textAlign: "center", color: colors.foreground, fontSize: 10, lineHeight: 13, fontVariant: ["tabular-nums"], borderBottomWidth: 1, borderBottomColor: colors.foregroundMuted }}>{count.numerator}</Text>
        <Text style={{ textAlign: "center", color: colors.foregroundMuted, fontSize: 10, lineHeight: 13, fontVariant: ["tabular-nums"] }}>{count.denominator}</Text>
      </View>
    </View> : <>
      <Icon name={icon} size={18} color={iconColor ?? (selected ? colors.accent : colors.foreground)} />
      {count !== undefined || caption ? <Text numberOfLines={1} style={{ maxWidth: "100%", color: colors.foregroundMuted, fontSize: 10, lineHeight: 13 }}>{caption ?? count?.text}</Text> : null}
    </>}
    {selected ? <View importantForAccessibility="no" accessibilityElementsHidden style={{ position: "absolute", top: 1, right: 1 }}><Icon name="Check" size={10} color={colors.foreground} /></View> : null}
  </Pressable>;
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

type ToolbarProps = {
  colors: Colors; width: number; status: StatusFilter; counts: Readonly<Record<StatusFilter, SessionStateCount>>;
  activeCount: number; filtersTriggerRef: RefObject<View | null>; chooserRef: RefObject<View | null>; chooserOpen: boolean;
  helpDismissRef: Ref<() => void>;
  onStatus(status: StatusFilter): void; onFilters(): void; onChooser(): void;
};

/** State choices, Recent first, then More and Filters; narrow rows explicitly overflow into the named legend. */
export function MobileQuickToolbar(props: ToolbarProps) {
  const { colors, status, counts, width, filtersTriggerRef, chooserRef } = props;
  const { fontScale } = useWindowDimensions();
  const help = useControlHelp("");
  useImperativeHandle(props.helpDismissRef, () => help.dismiss, [help.dismiss]);
  const target = Math.max(44, Math.ceil(36 * fontScale));
  const choices = visibleQuickFilters(width || 296, status, target);
  return <View testID="sessions-mobile-toolbar" style={{ gap: 4 }}>
    <View testID="sessions-mobile-quick-row" style={{ flexDirection: "row", flexWrap: width > 0 && width < 4 * (target + 4) ? "wrap" : "nowrap", alignItems: "center", gap: 4 }}>
      {choices.map((choice) => <QuickToolbarButton key={choice} testID={`sessions-quick-${choice}`} icon={QUICK_FILTER_ICONS[choice]} iconColor={sessionStatusInk(choice, colors)} colors={colors} target={target}
        label={`${quickFilterLabel(choice)}, ${counts[choice].label}`} help={`${quickFilterLabel(choice)}. ${counts[choice].label}. ${QUICK_FILTER_MEANINGS[choice]}`} count={counts[choice]} selected={status === choice} onPress={() => props.onStatus(choice)} />)}
      <View style={{ flex: 1 }} />
      <QuickToolbarButton ref={chooserRef} testID="sessions-mobile-quick-chooser-trigger" label={`All ${QUICK_STATUS_FILTERS.length} quick filters and icon meanings, current: ${quickFilterLabel(status)}`}
        icon="ListFilter" caption="More" expanded={props.chooserOpen} colors={colors} target={target} onPress={props.onChooser} />
      <QuickToolbarButton ref={filtersTriggerRef} testID="sessions-mobile-filters-trigger"
        label={props.activeCount ? `Filters and view, ${props.activeCount} active filters or view choices` : "Filters and view"}
        help="Filters and view. Opens the full filter page. Choices apply immediately; Back keeps them."
        icon="SlidersHorizontal" caption="Filters" colors={colors} target={target} onPress={props.onFilters} />
    </View>
  </View>;
}
