import { sessionStatusInk } from "./session-status-ink";
import { useEffect, useImperativeHandle, useRef, useState, type ComponentRef, type Ref, type RefObject } from "react";
import { Pressable, Text, View, useWindowDimensions } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useControlHelp } from "./control-help";
import { focusControl } from "./mobile-filters";
import { type Colors } from "./page-controls";
import { quickFilterHelp, quickFilterLabel, QUICK_FILTER_ICONS, visibleQuickFilters } from "./mobile-quick-filter-options";
import { type StatusFilter } from "./session-view";

/** One existing pressable owns activation and shared pointer/keyboard help. Touch learns the icons in More. */
export function QuickToolbarButton({ label, help = label, icon, iconColor, colors, onPress, selected, expanded, count, caption, testID, target = 44, ref }: {
  label: string; help?: string; icon: string; iconColor?: string; colors: Colors; onPress(): void;
  selected?: boolean; expanded?: boolean; count?: number; caption?: string; testID?: string; target?: number; ref?: Ref<View>;
}) {
  const controlHelp = useControlHelp(help, ref);
  return <Pressable {...controlHelp.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={label}
    accessibilityState={{ ...(selected === undefined ? {} : { selected }), ...(expanded === undefined ? {} : { expanded }) }}
    aria-pressed={selected} aria-expanded={expanded}
    onPress={() => { controlHelp.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ width: target, minHeight: target, flexShrink: 0, paddingVertical: 3,
      alignItems: "center", justifyContent: "center", gap: 1, borderRadius: 7, borderWidth: selected || focused ? 2 : 1,
      borderColor: selected || focused ? colors.accent : colors.border, backgroundColor: pressed || selected ? colors.surface2 : colors.surface1 })}>
    <Icon name={icon} size={18} color={iconColor ?? (selected ? colors.accent : colors.foreground)} />
    {count !== undefined || caption ? <Text numberOfLines={1} style={{ maxWidth: "100%", color: colors.foregroundMuted, fontSize: 10, lineHeight: 13 }}>{caption ?? (count! > 999 ? "999+" : count)}</Text> : null}
    {selected ? <View importantForAccessibility="no" accessibilityElementsHidden style={{ position: "absolute", top: 1, right: 1 }}><Icon name="Check" size={10} color={colors.foreground} /></View> : null}
  </Pressable>;
}

type ToolbarProps = {
  colors: Colors; width: number; query: string; status: StatusFilter; counts: Readonly<Record<StatusFilter, number>>;
  activeCount: number; summary: string; searchInputRef: RefObject<ComponentRef<typeof TextInput> | null>;
  filtersTriggerRef: RefObject<View | null>; chooserRef: RefObject<View | null>; chooserOpen: boolean;
  helpDismissRef: Ref<() => void>;
  onQuery(query: string): void; onStatus(status: StatusFilter): void; onFilters(): void; onChooser(): void; onClear(): void;
};

/** Expandable search leaves a useful editing width; narrow rows explicitly overflow into the named seven-choice legend. */
export function MobileQuickToolbar(props: ToolbarProps) {
  const { colors, query, status, counts, width, summary, searchInputRef, filtersTriggerRef, chooserRef } = props;
  const { fontScale } = useWindowDimensions();
  const [searchOpen, setSearchOpen] = useState(false);
  const help = useControlHelp("");
  useImperativeHandle(props.helpDismissRef, () => help.dismiss, [help.dismiss]);
  const searchTriggerRef = useRef<View | null>(null);
  const target = Math.max(44, Math.ceil(36 * fontScale));
  const choices = visibleQuickFilters(width || 296, status, target);
  // Larger text gets a dedicated editing line rather than a squeezed input.
  const stackedSearch = width > 0 && width < 136 * fontScale + 3 * (target + 4);
  useEffect(() => {
    if (!searchOpen) return;
    const timer = setTimeout(() => searchInputRef.current?.focus(), 0);
    return () => clearTimeout(timer);
  }, [searchOpen, searchInputRef]);
  const closeSearch = () => {
    searchInputRef.current?.blur(); setSearchOpen(false);
    setTimeout(() => { focusControl(searchTriggerRef.current); help.dismiss(); }, 0);
  };
  const search = <TextInput ref={searchInputRef} testID="sessions-search" value={query} onChangeText={props.onQuery}
    accessibilityLabel="Search sessions by topic, role or ticket" placeholder="Search sessions…" placeholderTextColor={colors.foregroundMuted}
    autoCapitalize="none" autoCorrect={false} returnKeyType="search" onSubmitEditing={closeSearch}
    style={{ flex: 1, minWidth: 0, minHeight: target, fontSize: 16, color: colors.foreground, backgroundColor: colors.surface1,
      borderWidth: 1, borderColor: colors.border, borderRadius: 7, paddingHorizontal: 10, paddingVertical: 6 }} />;
  return <View testID="sessions-mobile-toolbar" style={{ gap: 4 }}>
    {searchOpen && stackedSearch ? <View style={{ flexDirection: "row", gap: 4 }}>{search}</View> : null}
    <View testID="sessions-mobile-quick-row" style={{ flexDirection: "row", flexWrap: width > 0 && width < 4 * (target + 4) ? "wrap" : "nowrap", alignItems: "center", gap: 4 }}>
      {searchOpen && !stackedSearch ? search : null}
      {searchOpen ? <QuickToolbarButton label={query ? "Clear search" : "Close search"} icon={query ? "X" : "ChevronLeft"} colors={colors} target={target}
        onPress={() => { if (query) { props.onQuery(""); searchInputRef.current?.focus(); } else closeSearch(); }} /> : <>
        <QuickToolbarButton ref={searchTriggerRef} testID="sessions-mobile-search-trigger" label={query ? `Search sessions, current search: ${query}` : "Search sessions"}
          icon="Search" caption={query ? "Edit" : undefined} expanded={false} colors={colors} target={target} onPress={() => setSearchOpen(true)} />
        {choices.map((choice) => <QuickToolbarButton key={choice} testID={`sessions-quick-${choice}`} icon={QUICK_FILTER_ICONS[choice]} iconColor={sessionStatusInk(choice, colors)} colors={colors} target={target}
          label={`${quickFilterLabel(choice)}, ${counts[choice]} sessions`} help={quickFilterHelp(choice, counts[choice])} count={counts[choice]} selected={status === choice} onPress={() => props.onStatus(choice)} />)}
      </>}
      <View style={{ flex: searchOpen ? 0 : 1 }} />
      <QuickToolbarButton ref={chooserRef} testID="sessions-mobile-quick-chooser-trigger" label={`All 8 quick filters and icon meanings, current: ${quickFilterLabel(status)}`}
        icon="ListFilter" caption="More" expanded={props.chooserOpen} colors={colors} target={target} onPress={props.onChooser} />
      <QuickToolbarButton ref={filtersTriggerRef} testID="sessions-mobile-filters-trigger"
        label={props.activeCount ? `Filters and view, ${props.activeCount} active filters or view choices` : "Filters and view"}
        help="Filters and view. Opens the full filter page. Choices apply immediately; Back keeps them."
        icon="SlidersHorizontal" caption="Filters" colors={colors} target={target} onPress={props.onFilters} />
    </View>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
    <Text testID="sessions-mobile-selected-filter" accessibilityLiveRegion="polite" style={{ flex: 1, color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>
      {quickFilterLabel(status)} · {counts[status]}{summary ? ` · ${summary}` : ""}
    </Text>
    <Pressable testID="sessions-mobile-clear-filters" accessibilityRole="button" accessibilityLabel="Clear filters"
      accessibilityState={{ disabled: props.activeCount === 0 }} aria-disabled={props.activeCount === 0} disabled={props.activeCount === 0}
      accessibilityHint="Clears all search and filter choices; keeps this context and view preferences" onPress={props.onClear}
      style={{ minHeight: 44, paddingHorizontal: 8, justifyContent: "center", borderRadius: 6, opacity: props.activeCount === 0 ? 0.45 : 1 }}>
      <Text style={{ color: colors.accent, fontSize: 13, fontWeight: "600" }}>Clear filters</Text>
    </Pressable>
    </View>
  </View>;
}
