import { sessionStatusInk } from "./session-status-ink";
import { useRef, type RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { Icon, ScrollView } from "@getpaseo/plugin/client/react-native";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { focusControl } from "./mobile-filters";
import { Action, type Colors } from "./page-controls";
import { useControlHelp } from "./control-help";
import { quickFilterLabel, QUICK_FILTER_ICONS, QUICK_FILTER_MEANINGS } from "./mobile-quick-filter-options";
import { QUICK_STATUS_FILTERS, type StatusFilter } from "./session-view";

/** The same seven choices as desktop, with touch-readable names, counts and a noninteractive legend. */
export function MobileQuickChooser({ root, rootSize, anchor, colors, status, counts, onStatus, onClose }: {
  root: RefObject<View | null>; rootSize: MenuSize; anchor: RefObject<View | null>; colors: Colors;
  status: StatusFilter; counts: Readonly<Record<StatusFilter, number>>; onStatus(status: StatusFilter): void; onClose(): void;
}) {
  const closeRef = useRef<View | null>(null);
  const focused = useRef(false);
  const help = useControlHelp("");
  // Returning focus is navigation, not a request for help over the next touch target.
  const close = () => { onClose(); setTimeout(() => { focusControl(anchor.current); help.dismiss(); }, 0); };
  return <AnchoredMenu dialog testID="sessions-mobile-quick-chooser" label="Quick filters and icon meanings" root={root} rootSize={rootSize} anchor={anchor}
    colors={colors} onClose={close} placement={{ align: "stretch", width: 0, cover: true }}
    header={<View onLayout={() => { if (!focused.current) { focused.current = true; focusControl(closeRef.current); } }} style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Text accessibilityRole="header" style={{ flex: 1, color: colors.foreground, fontSize: 16, fontWeight: "600" }}>Quick filters</Text>
      <Action ref={closeRef} colors={colors} label="Close" onPress={close} />
    </View>}>
    <ScrollView testID="sessions-mobile-quick-choices" keyboardShouldPersistTaps="handled" style={{ flexShrink: 1 }} contentContainerStyle={{ padding: 6, gap: 8 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 13, lineHeight: 19 }}>Choose one filter. Counts use your current search and scope. Opening this legend changes nothing.</Text>
      {QUICK_STATUS_FILTERS.map((choice) => <Pressable key={choice} testID={`sessions-choose-${choice}`} accessibilityRole="button"
        accessibilityLabel={`${quickFilterLabel(choice)}, ${counts[choice]} sessions. ${QUICK_FILTER_MEANINGS[choice]}`}
        accessibilityState={{ selected: choice === status }} aria-pressed={choice === status}
        onPress={() => { onStatus(choice); close(); }}
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 48, flexDirection: "row", alignItems: "center", gap: 10, padding: 8, borderRadius: 7,
          borderWidth: 1, borderColor: choice === status || focused ? colors.accent : colors.border, backgroundColor: choice === status || pressed ? colors.surface2 : colors.surface1 })}>
        <Icon name={QUICK_FILTER_ICONS[choice]} size={20} color={sessionStatusInk(choice, colors)} />
        <View style={{ flex: 1, minWidth: 0 }}>
          <Text style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{quickFilterLabel(choice)} · {counts[choice]}{choice === status ? " · Selected" : ""}</Text>
          <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{QUICK_FILTER_MEANINGS[choice]}</Text>
        </View>
        {choice === status ? <Icon name="Check" size={16} color={colors.foreground} /> : null}
      </Pressable>)}
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}><Icon name="Info" size={16} color={colors.foregroundMuted} />
        <Text style={{ flex: 1, color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>Tap an icon once to apply it. More shows these meanings without applying a filter. Filters opens the separate full page for scope, grouping and more states.</Text></View>
    </ScrollView>
  </AnchoredMenu>;
}
