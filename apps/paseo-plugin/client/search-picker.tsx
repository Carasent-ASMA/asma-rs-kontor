import { useEffect, useRef, useState, type ReactNode } from "react";
import { Platform, Pressable, ScrollView, Text, View, useWindowDimensions, type ViewProps } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { Action, NO_PRESS_DELAY, pressFeedback, type Colors } from "./page-controls";

export type PickerRow = { id: string; title: string; detail: string; icon?: string; problem?: string | null; action?: ReactNode };

/** A manual-selection combobox, with one results scroll and fixed controls on short screens. */
export function SearchPicker({ name, label, placeholder, query, onQuery, onSearch, filters, rows, selected, busy, message, error, onRetry, onSelect, onBack, colors }: {
  name: string; label: string; placeholder: string; query: string; onQuery: (query: string) => void; onSearch?: () => void;
  filters?: ReactNode; rows: readonly PickerRow[]; selected: string | null; busy: boolean; message: string; error?: string | null;
  onRetry: () => void; onSelect: (id: string) => void; onBack: () => void; colors: Colors;
}) {
  const [active, setActive] = useState<string | null>(null);
  const { height } = useWindowDimensions();
  const scroll = useRef<ScrollView>(null), positions = useRef(new Map<string, number>());
  const available = rows.filter((row) => !row.problem), current = available.find((row) => row.id === active);
  useEffect(() => { setActive(null); scroll.current?.scrollTo({ y: 0, animated: false }); }, [query]);
  const choose = (id: string) => { if (!busy && !error && available.some((row) => row.id === id)) onSelect(id); };
  const listId = `${name}-results`, optionId = (id: string) => `${name}-option-${id}`;
  const keys = Platform.OS === "web" ? { onKeyDown: (event: { key: string; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean; preventDefault(): void; stopPropagation(): void }) => {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onBack(); return; }
    if (event.altKey || event.ctrlKey || event.metaKey || busy || error) return;
    if (event.key === "Enter" && current) { event.preventDefault(); event.stopPropagation(); choose(current.id); return; }
    if (!["ArrowDown", "ArrowUp"].includes(event.key) || !available.length) return;
    event.preventDefault(); event.stopPropagation();
    const index = available.findIndex((row) => row.id === active);
    const next = available[index < 0 ? event.key === "ArrowUp" ? available.length - 1 : 0 : Math.max(0, Math.min(available.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]!;
    setActive(next.id); scroll.current?.scrollTo({ y: positions.current.get(next.id) ?? 0, animated: false });
  } } : {};
  return <View testID={name} style={{ flex: 1, flexShrink: 1, minHeight: 0, maxHeight: Math.max(180, height - 180), gap: 8, padding: 6 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{label}</Text>
    <View style={{ flexDirection: "row", gap: 6 }}>
      <TextInput testID={`${name}-search`} accessibilityLabel={label} accessibilityRole="combobox" aria-expanded aria-controls={listId} aria-autocomplete="list"
        aria-activedescendant={!busy && !error && current ? optionId(current.id) : undefined} {...keys}
        autoFocus placeholder={placeholder} value={query} onChangeText={onQuery} maxLength={200} returnKeyType="search"
        onSubmitEditing={() => { if (current) choose(current.id); else onSearch?.(); }}
        placeholderTextColor={colors.foregroundMuted} style={{ minWidth: 0, flex: 1, color: colors.foreground, fontSize: 13, minHeight: 44, paddingHorizontal: 9, borderWidth: 1, borderRadius: 6, borderColor: colors.border }} />
      <Action label="Clear" accessibilityLabel={`Clear ${label.toLowerCase()} search`} colors={colors} disabled={!query} onPress={() => onQuery("")} />
    </View>
    {filters}
    <Text accessibilityLiveRegion="polite" aria-live="polite" style={{ color: error ? colors.statusWarning : colors.foregroundMuted, fontSize: 12 }}>{busy ? "Searching…" : error ?? message}</Text>
    <ScrollView ref={scroll} testID={`${name}-scroll`} keyboardShouldPersistTaps="handled" style={{ flex: 1, minHeight: 0 }} contentContainerStyle={{ gap: 2 }}>
      {/* RN 0.81's Role type omits the standard web listbox role. Native uses a single-choice group. */}
      <View nativeID={listId} role={Platform.OS === "web" ? "listbox" as ViewProps["role"] : "radiogroup"} accessibilityLabel={`${label} results`} aria-busy={busy}>
        {rows.map((row) => <View key={row.id} onLayout={(event) => { positions.current.set(row.id, event.nativeEvent.layout.y); }}>
          <Pressable nativeID={optionId(row.id)} testID={`${name}-row-${row.id}`} role="option" accessibilityLabel={`${row.title}, ${row.detail}${row.problem ? `. ${row.problem}` : ""}`}
            accessibilityState={{ selected: current ? active === row.id : selected === row.id, disabled: busy || !!error || !!row.problem }} aria-selected={current ? active === row.id : selected === row.id}
            aria-disabled={busy || !!error || !!row.problem} disabled={busy || !!error || !!row.problem} {...(Platform.OS === "web" ? { tabIndex: -1 } : {})} {...NO_PRESS_DELAY}
            onPress={() => choose(row.id)} style={({ pressed }) => ({ minHeight: 44, padding: 9, borderRadius: 6, borderWidth: 1, borderColor: selected === row.id || active === row.id ? colors.accent : colors.border, ...pressFeedback(colors, pressed, selected === row.id || active === row.id, !!row.problem) })}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
              {row.icon ? <Icon name={row.icon} size={16} color={colors.foregroundMuted} /> : null}
              <Text numberOfLines={2} style={{ flex: 1, color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{selected === row.id ? "✓ " : ""}{row.title}</Text>
            </View>
            <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{row.detail}</Text>
            {row.problem ? <Text style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>{row.problem}</Text> : null}
          </Pressable>
          {row.action}
        </View>)}
      </View>
    </ScrollView>
    <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8, borderTopWidth: 1, borderColor: colors.border, paddingTop: 6 }}>
      <Action label="Back" accessibilityLabel={`Back from ${label.toLowerCase()}`} colors={colors} onPress={onBack} />
      {error ? <Action label="Retry" colors={colors} onPress={onRetry} /> : onSearch ? <Action label="Search" accessibilityLabel={`Search ${label.toLowerCase()}`} colors={colors} onPress={onSearch} /> : null}
    </View>
  </View>;
}
