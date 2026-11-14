import { createContext, isValidElement, useContext, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { ScrollView, TextInput } from "@getpaseo/plugin/client/ui";
import { Pressable, Text, View } from "react-native";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { focusControl } from "./mobile-filters";
import type { Colors } from "./page-controls";

type Option = { value: string; label: string };
type ChoiceProps = { label: string; options: Option[]; value: string[]; onChange: (value: string[]) => void; colors: Colors; multiple?: boolean; compact?: boolean; joined?: boolean };
const Choices = createContext<{ root: RefObject<View | null>; size: MenuSize; popup: (node: ReactNode) => void } | null>(null);

/** The existing native menu measures against the visible report or sheet, outside its scrolling content. */
export function TokenomicsChoices({ children }: { children: ReactNode }) {
  const root = useRef<View>(null), [size, setSize] = useState({ width: 0, height: 0 }), [popup, setPopup] = useState<ReactNode>(null);
  const context = useMemo(() => ({ root, size, popup: setPopup }), [size]);
  return <Choices.Provider value={context}><View ref={root} onLayout={(e) => setSize({ width: e.nativeEvent.layout.width, height: e.nativeEvent.layout.height })} style={{ flex: 1, minHeight: 0 }}><View accessibilityElementsHidden={isValidElement(popup) && popup.type === ChoicePopup} importantForAccessibility={isValidElement(popup) && popup.type === ChoicePopup ? "no-hide-descendants" : "auto"} style={{ flex: 1, minHeight: 0 }}>{children}</View>{popup}</View></Choices.Provider>;
}
export function useTokenomicsOverlay() { return useContext(Choices); }
export const choiceSearchable = (count: number) => count > 15;

export function choiceOptions(options: Option[], selected: string[], search: string) {
  const matches = options.filter((o) => o.label.toLowerCase().includes(search.toLowerCase()));
  const pinned = !search && selected.length < options.length ? options.filter((o) => selected.includes(o.value)).slice(0, 20) : [];
  const keys = new Set(pinned.map((o) => o.value));
  return { rows: [...pinned, ...matches.filter((o) => !keys.has(o.value)).slice(0, 100 - pinned.length)], matches: matches.length };
}
/** Long option inventories stay searchable; selected values remain visible while refining the list. */
export function TokenomicsOptions({ label, options, value, onChange, colors, multiple = false, compact = false, onDone, selectedRef }: ChoiceProps & { onDone?: () => void; selectedRef?: RefObject<View | null> }) {
  const [search, setSearch] = useState("");
  const filtered = choiceOptions(options, value, search), height = compact ? 44 : 34;
  return <>
    {choiceSearchable(options.length) ? <TextInput accessibilityLabel={`Find ${label}`} placeholder={`Find ${label}`} value={search} onChangeText={setSearch} style={{ color: colors.foreground, backgroundColor: colors.surface1, borderColor: colors.border, borderWidth: 1, borderRadius: 5, padding: 8, minHeight: 44, fontSize: compact ? 16 : 14 }} /> : null}
    {multiple ? <View style={{ flexDirection: "row", gap: 8 }}>{[["All", options.map((o) => o.value)], ["None", []]].map(([name, values]) => <Pressable key={String(name)} accessibilityRole="button" accessibilityLabel={`${label}: select ${name}`} onPress={() => onChange(values as string[])} style={{ minHeight: 44, paddingHorizontal: 12, justifyContent: "center" }}><Text style={{ color: colors.accent, fontSize: 14 }}>{String(name)}</Text></Pressable>)}{onDone ? <Pressable accessibilityRole="button" accessibilityLabel={`Done choosing ${label}`} onPress={onDone} style={{ minHeight: 44, paddingHorizontal: 12, marginLeft: "auto", justifyContent: "center" }}><Text style={{ color: colors.accent, fontSize: 14 }}>Done</Text></Pressable> : null}</View> : null}
    <ScrollView keyboardShouldPersistTaps="handled" style={{ flexShrink: 1 }}>
      {filtered.rows.map((o) => <Pressable key={o.value} ref={o.value === (value[0] ?? filtered.rows[0]?.value) ? selectedRef : undefined} accessibilityRole={multiple ? "checkbox" : "radio"} accessibilityLabel={o.label} accessibilityState={{ checked: value.includes(o.value) }} onPress={() => { onChange(multiple ? value.includes(o.value) ? value.filter((v) => v !== o.value) : [...value, o.value] : [o.value]); if (!multiple) onDone?.(); }} style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ minHeight: height, paddingHorizontal: 10, paddingVertical: 7, borderRadius: 4, flexDirection: "row", alignItems: "center", gap: 8, backgroundColor: pressed || hovered || focused ? colors.surface2 : colors.surface1, borderWidth: 1, borderColor: focused ? colors.accent : "transparent" })}><Text style={{ width: 16, color: colors.accent }}>{value.includes(o.value) ? "✓" : ""}</Text><Text style={{ flex: 1, color: colors.foreground, fontSize: compact ? 15 : 14, fontWeight: value.includes(o.value) ? "600" : "400", lineHeight: compact ? 22 : 20 }}>{o.label}</Text></Pressable>)}
      {!filtered.matches ? <Text accessibilityLiveRegion="polite" style={{ color: colors.foregroundMuted, padding: 10 }}>No matching choices</Text> : filtered.matches > 100 ? <Text style={{ color: colors.foregroundMuted, padding: 10, fontSize: 13 }}>{value.length > 20 && value.length !== options.length ? `${value.length.toLocaleString("en-US")} selected. ` : ""}Showing up to 100 matches. Search all {options.length.toLocaleString("en-US")} options.</Text> : null}
    </ScrollView>
  </>;
}
function ChoicePopup({ anchor, close, ...props }: ChoiceProps & { anchor: RefObject<View | null>; close: () => void }) {
  const selectedRef = useRef<View>(null);
  const context = useContext(Choices)!;
  const { label, options, value, colors, multiple = false, compact = false } = props;
  const cap = Math.min(compact ? 360 : 320, Math.min(options.length, 100) * (compact ? 44 : 34) + 12 + (choiceSearchable(options.length) ? 48 : 0) + (multiple ? 44 : 0));
  return <AnchoredMenu initialFocus={selectedRef} trapFocus={multiple || choiceSearchable(options.length)} focusSearch={choiceSearchable(options.length)} selection={multiple ? "multiple" : "single"} anchor={anchor} root={context.root} rootSize={context.size} colors={colors} label={label} onClose={close} testID="tokenomics-choice-popup" placement={{ align: compact ? "stretch" : "start", width: options.reduce((max, o) => Math.max(max, Math.min(360, o.label.length * 7.3 + 46)), 160), maxHeight: cap }}>
    <TokenomicsOptions selectedRef={selectedRef} {...props} value={value} onDone={() => { close(); requestAnimationFrame(() => focusControl(anchor.current)); }} />
  </AnchoredMenu>;
}
export function TokenomicsChoice(props: ChoiceProps) {
  const { label, options, value, onChange, colors, multiple = false, compact = false, joined = false } = props;
  const context = useContext(Choices), anchor = useRef<View>(null), [open, setOpen] = useState(false), [focused, setFocused] = useState(false);
  useEffect(() => { if (!open || !context) return; context.popup(<ChoicePopup {...props} anchor={anchor} close={() => setOpen(false)} />); return () => context.popup(null); }, [open, context, options, value, onChange, colors, compact]);
  const plain = multiple && label !== "Workstreams", field = ["User", "Revision", "vs", "Period"].includes(label);
  const standaloneWidth = options.reduce((max, o) => Math.max(max, Math.min(406, o.label.length * 7 + 30)), 70);
  const summary = multiple ? value.length === options.length ? "All" : `${value.length}/${options.length}` : options.find((o) => value.includes(o.value))?.label ?? "None";
  return <View style={{ flexDirection: "row", alignItems: "center", gap: field || multiple ? 0 : 6, maxWidth: "100%", flexShrink: compact && joined ? 1 : undefined, minWidth: 0, width: compact && !multiple && !joined ? "100%" : undefined }}>{!field && !multiple ? <Text style={{ color: colors.foreground, fontSize: compact ? 14 : 14, lineHeight: 20.3, flexShrink: 0 }}>{label}</Text> : null}<Pressable ref={anchor} accessibilityRole="button" accessibilityLabel={`${label}: ${summary}`} accessibilityState={{ expanded: open }} onFocus={() => setFocused(true)} onBlur={() => setFocused(false)} onPress={() => setOpen(!open)} style={{ minHeight: compact ? 44 : 30, paddingHorizontal: 9, borderWidth: joined ? 0 : 1, borderColor: focused ? colors.accent : colors.border, borderRadius: joined ? 0 : 6, backgroundColor: colors.surface1, flexDirection: "row", alignItems: "center", gap: 7, maxWidth: "100%", flexShrink: compact || joined ? 1 : 0, flexGrow: compact && !multiple ? 1 : 0, minWidth: compact || plain || multiple ? 0 : label === "User" ? 126 : label === "Revision" ? 250 : label === "vs" ? 230 : label === "Period" ? Math.min(280, Math.max(150, summary.length * 7 + 68)) : label === "Group 1" || label === "then" ? 140 : standaloneWidth, width: compact && ["User", "Period"].includes(label) ? "100%" : undefined }}>
    {field || multiple ? <Text style={{ color: plain || multiple ? colors.foreground : colors.foregroundMuted, fontSize: plain || multiple ? 13 : 11, textTransform: plain || multiple ? "none" : "uppercase" }}>{label === "Workstreams" ? `Workstreams (${value.length}/${options.length})` : label}</Text> : null}{label !== "Workstreams" && !(plain && value.length === options.length) ? <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, flexShrink: 1 }}>{summary}</Text> : null}<Text style={{ color: colors.foregroundMuted }}>▾</Text>
  </Pressable></View>;
}
