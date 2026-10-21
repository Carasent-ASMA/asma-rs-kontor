import { useEffect, useRef, useState } from "react";
import { BackHandler, Pressable, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { COARSE_POINTER, Disclosure, type Colors } from "./page-controls";
import { focusControl } from "./mobile-filters";
import { OTHER_WORKSTREAM } from "./session-organization";
import { workstreamOptionLabel, type WorkstreamOption } from "./plan-workstreams";

/** Case, punctuation, spacing and "&"/"and" do not matter when finding an association. */
export function normalizeChoice(text: string): string {
  return text.toLocaleLowerCase().replace(/&/g, " and ").replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

/** Leaving All starts from every current choice; explicit keys, unavailable ones included, never collapse back to All. */
export function toggleSelection(selected: readonly string[] | null, universe: readonly string[], key: string): readonly string[] {
  const current = selected ?? universe;
  return current.includes(key) ? current.filter((choice) => choice !== key) : [...current, key];
}

export function selectionSummary(selected: readonly string[] | null, options: readonly WorkstreamOption[]): { readonly line: string; readonly names: string | null } {
  const available = `${options.length} available`;
  if (selected === null) return { line: `Workstream associations · All · ${available}`, names: null };
  const labels = selected.map((key) => options.find((option) => option.key === key)?.label ?? "Unavailable association");
  return { line: `Workstream associations · ${selected.length ? `${selected.length} selected` : "None"} · ${available}`,
    names: labels.length ? `${labels.slice(0, 2).join(", ")}${labels.length > 2 ? ` +${labels.length - 2}` : ""}` : null };
}

function kinds(option: WorkstreamOption, registryLoaded: boolean): string[] {
  if (option.key === OTHER_WORKSTREAM.toLowerCase()) return ["no workstream link"];
  return [option.desk ? "desk category" : null, !option.desk && option.sessions + option.workspaces === 0 && option.plans > 0 ? "plan only" : null,
    registryLoaded && !option.desk && !option.registered ? "unregistered" : null].filter((kind): kind is string => kind !== null);
}

type PageKey = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type PageDocument = {
  addEventListener(type: "keydown", listener: (event: PageKey) => void): void;
  removeEventListener(type: "keydown", listener: (event: PageKey) => void): void;
};

const byLabel = (left: WorkstreamOption, right: WorkstreamOption) => left.label.localeCompare(right.label) || (left.key < right.key ? -1 : 1);

/**
 * Inline association disclosure inside Filters & view, no focus trap. Counts are the inventory facts of
 * planWorkstreamOptions; a person scope adds its own separately labelled count, never replacing them.
 */
export function WorkstreamSelector({ options, selected, open, registryLoaded, about, scopeCounts, scopeLabel, colors, compact, disabled = false, onOpen, onChange }: {
  options: readonly WorkstreamOption[];
  selected: readonly string[] | null;
  open: boolean;
  registryLoaded: boolean;
  about: string;
  /** Sessions per association in the current person scope, with that scope's name ("in My work"). */
  scopeCounts?: ReadonlyMap<string, number>;
  scopeLabel?: string;
  colors: Colors;
  compact: boolean;
  disabled?: boolean;
  onOpen(open: boolean): void;
  onChange(next: readonly string[] | null): void;
}) {
  const [search, setSearch] = useState("");
  const [details, setDetails] = useState<string | null>(null);
  const triggerRef = useRef<View | null>(null);
  const close = useRef(() => {});
  close.current = () => { onOpen(false); focusControl(triggerRef.current); };
  useEffect(() => {
    // Compact Back and Escape close the open list before they leave Filters.
    if (!open || !compact) return;
    const back = BackHandler.addEventListener("hardwareBackPress", () => { close.current(); return true; });
    const document = (globalThis as { document?: PageDocument }).document;
    const escape = (event: PageKey) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault(); event.stopPropagation(); close.current();
    };
    document?.addEventListener("keydown", escape);
    return () => { back.remove(); document?.removeEventListener("keydown", escape); };
  }, [open, compact]);
  const summary = selectionSummary(selected, options);
  const universe = options.map((option) => option.key);
  const needle = normalizeChoice(search);
  const matches = (option: WorkstreamOption) => !needle || normalizeChoice(`${option.label} ${option.key}`).includes(needle);
  const chosen = selected === null ? [] : selected.map((key) => options.find((option) => option.key === key) ?? key);
  const others = [...options].filter((option) => (selected === null || !selected.includes(option.key)) && matches(option)).sort(byLabel);
  const height = COARSE_POINTER ? 44 : compact ? 36 : 30;
  const textSize = compact ? 13 : 12;
  const row = (choice: WorkstreamOption | string) => {
    const option = typeof choice === "string" ? null : choice;
    const key = option?.key ?? (choice as string);
    const label = option?.label ?? "Unavailable association";
    const checked = selected === null || selected.includes(key);
    const count = option ? `${option.sessions} session${option.sessions === 1 ? "" : "s"}` : "not in this context";
    const scoped = option && scopeCounts ? `${scopeCounts.get(key) ?? 0} ${scopeLabel}` : null;
    const facts = option ? workstreamOptionLabel(option, registryLoaded) : `Saved key ${key} is selected but not offered in this context. It stays selected until you clear it or choose All or None.`;
    return <View key={key} style={{ gap: 2 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <Pressable testID={`workstream-option-${key}`} disabled={disabled} accessibilityRole="checkbox" accessibilityState={{ checked, disabled }} aria-checked={checked}
          accessibilityLabel={`${label}, ${count}${scoped ? `, ${scoped}` : ""}`} onPress={() => onChange(toggleSelection(selected, universe, key))}
          style={{ flex: 1, minWidth: 0, minHeight: height, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 6, borderRadius: 6, opacity: disabled ? 0.5 : 1 }}>
          <View style={{ width: 18, height: 18, borderRadius: 4, borderWidth: 1, borderColor: checked ? colors.accent : colors.border, backgroundColor: checked ? colors.accent : colors.surface1, alignItems: "center", justifyContent: "center" }}>
            {checked ? <Text style={{ color: colors.accentForeground, fontSize: 12, fontWeight: "700" }}>✓</Text> : null}
          </View>
          <View style={{ flex: 1, minWidth: 0, flexDirection: "row", flexWrap: "wrap", alignItems: "center", columnGap: 6 }}>
            <Text style={{ flexShrink: 1, color: colors.foreground, fontSize: textSize, fontWeight: checked ? "600" : "400" }}>{label}</Text>
            <Text style={{ color: colors.foregroundMuted, fontSize: textSize, fontVariant: ["tabular-nums"] }}>{count}{scoped ? ` · ${scoped}` : ""}</Text>
            {(option ? kinds(option, registryLoaded) : ["unavailable"]).map((kind) => <Text key={kind} style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 5, borderRadius: 4, borderWidth: 1, borderColor: colors.border }}>{kind}</Text>)}
          </View>
        </Pressable>
        <Pressable testID={`workstream-details-${key}`} accessibilityRole="button" accessibilityLabel={`Details for ${label}`} accessibilityState={{ expanded: details === key }} aria-expanded={details === key}
          onPress={() => setDetails(details === key ? null : key)} style={{ minHeight: height, minWidth: 44, justifyContent: "center", alignItems: "center", paddingHorizontal: 6 }}>
          <Text style={{ color: colors.accent, fontSize: 12 }}>Details</Text>
        </Pressable>
      </View>
      {details === key ? <Text testID={`workstream-facts-${key}`} selectable style={{ marginLeft: 32, color: colors.foregroundMuted, fontSize: 11, lineHeight: 16 }}>{facts}</Text> : null}
    </View>;
  };
  const action = (testID: string, label: string, onPress: () => void, active: boolean) => <Pressable testID={testID} disabled={disabled} accessibilityRole="button"
    accessibilityState={{ selected: active, disabled }} aria-pressed={active} accessibilityLabel={label} onPress={onPress}
    style={{ minHeight: height, minWidth: 44, justifyContent: "center", paddingHorizontal: 10, borderRadius: 6, borderWidth: 1, borderColor: active ? colors.accent : colors.border, backgroundColor: active ? colors.surface2 : colors.surface1 }}>
    <Text style={{ color: colors.foreground, fontSize: textSize, fontWeight: active ? "600" : "400" }}>{label}</Text>
  </Pressable>;
  return <View testID="sessions-associations" style={{ gap: 6 }}>
    <Pressable ref={triggerRef} testID="sessions-associations-toggle" accessibilityRole="button" accessibilityState={{ expanded: open }} aria-expanded={open}
      accessibilityLabel={summary.names ? `${summary.line}: ${summary.names}` : summary.line} onPress={() => onOpen(!open)}
      style={{ minHeight: height, justifyContent: "center", alignSelf: "flex-start", maxWidth: "100%" }}>
      <Text style={{ color: colors.foreground, fontSize: textSize, fontWeight: "600" }}>{open ? "▾" : "▸"} {summary.line}</Text>
    </Pressable>
    {!open && summary.names ? <Pressable testID="sessions-associations-selected" accessibilityRole="button" accessibilityLabel={`Selected: ${summary.names}. Show the selection`} onPress={() => onOpen(true)}
      style={{ minHeight: height, justifyContent: "center", alignSelf: "flex-start", maxWidth: "100%" }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: textSize }}>{summary.names}</Text>
    </Pressable> : null}
    {open ? <View style={{ gap: 6 }}>
      <TextInput testID="sessions-associations-search" value={search} onChangeText={setSearch} placeholder="Find workstream" placeholderTextColor={colors.foregroundMuted}
        accessibilityLabel="Find workstream" autoCapitalize="none" autoCorrect={false}
        style={{ minHeight: height, borderRadius: 7, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface1, color: colors.foreground, paddingHorizontal: 10, fontSize: compact ? 16 : 12 }} />
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
        {action("sessions-associations-all", "All associations", () => onChange(null), selected === null)}
        {action("sessions-associations-none", "None", () => onChange([]), selected?.length === 0)}
      </View>
      {chosen.length ? <View accessibilityLabel={`Selected (${chosen.length})`} style={{ gap: 2 }}>
        <Text accessibilityRole="header" style={{ color: colors.foregroundMuted, fontSize: 11 }}>Selected ({chosen.length})</Text>
        {chosen.map(row)}
      </View> : null}
      <View style={{ gap: 2 }}>
        {chosen.length ? <Text accessibilityRole="header" style={{ color: colors.foregroundMuted, fontSize: 11 }}>{needle ? "Other matches" : "Other associations"}</Text> : null}
        {others.map(row)}
        {!others.length ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{options.length ? "No other association matches." : "No workstream associations available in this context."}</Text> : null}
      </View>
      <Disclosure small label="About these counts" colors={colors}>
        <Text testID="sessions-workstream-availability" style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 16 }}>{about}</Text>
      </Disclosure>
    </View> : null}
  </View>;
}
