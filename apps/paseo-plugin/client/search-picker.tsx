import { useEffect, useRef, useState, type ReactNode } from "react";
import { Platform, Pressable, ScrollView, Text, View, useWindowDimensions, type ViewProps } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { Action, NO_PRESS_DELAY, pressFeedback, type Colors } from "./page-controls";

export type PickerRow = { id: string; title: string; detail: string; icon?: string; problem?: string | null; action?: ReactNode };

type DomInput = {
  addEventListener?(type: "keydown", listener: (event: DomKey) => void, capture?: boolean): void;
  removeEventListener?(type: "keydown", listener: (event: DomKey) => void, capture?: boolean): void;
  setAttribute(name: string, value: string): void;
  removeAttribute(name: string): void;
};
type DomKey = { key: string; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean; preventDefault(): void; stopPropagation(): void };

/** A manual-selection combobox, with one results scroll and fixed controls on short screens. */
export function SearchPicker({ name, label, placeholder, query, onQuery, onSearch, filters, rows, selected, busy, message, error, onRetry, onSelect, onBack, colors }: {
  name: string; label: string; placeholder: string; query: string; onQuery: (query: string) => void; onSearch?: () => void;
  filters?: ReactNode; rows: readonly PickerRow[]; selected: string | null; busy: boolean; message: string; error?: string | null;
  onRetry: () => void; onSelect: (id: string) => void; onBack: () => void; colors: Colors;
}) {
  const [active, setActive] = useState<string | null>(null);
  const { height } = useWindowDimensions();
  const scroll = useRef<ScrollView>(null), searchRef = useRef(null), positions = useRef(new Map<string, number>());
  const activeRef = useRef<string | null>(null);
  const available = rows.filter((row) => !row.problem);
  activeRef.current = active;
  useEffect(() => { setActive(null); scroll.current?.scrollTo({ y: 0, animated: false }); }, [query]);
  const listId = `${name}-results`, optionId = (id: string) => `${name}-option-${id}`;
  const inputOf = (): DomInput | null => {
    const doc = (globalThis as { document?: { querySelector?(selector: string): DomInput | null } }).document;
    const byTestId = doc?.querySelector?.(`[data-testid="${name}-search"] input, [data-testid="${name}-search"]`) ?? null;
    if (byTestId?.addEventListener) return byTestId;
    const root = searchRef.current as unknown as { querySelector?(selector: string): DomInput | null } | null;
    return root?.querySelector?.("input") ?? (root as DomInput | null);
  };
  const syncActiveDescendant = (id: string | null) => {
    if (Platform.OS !== "web") return;
    const input = inputOf();
    if (!input?.setAttribute) return;
    if (id && !error) input.setAttribute("aria-activedescendant", id);
    else if (input.removeAttribute) input.removeAttribute("aria-activedescendant");
  };
  const choose = (id: string) => { if (!busy && !error && available.some((row) => row.id === id)) onSelect(id); };
  const chooseRef = useRef(choose), onBackRef = useRef(onBack), rowsRef = useRef(rows), busyRef = useRef(busy), errorRef = useRef(error);
  chooseRef.current = choose; onBackRef.current = onBack; rowsRef.current = rows; busyRef.current = busy; errorRef.current = error;
  const focusRow = (id: string) => {
    activeRef.current = id;
    setActive(id);
    syncActiveDescendant(optionId(id));
    scroll.current?.scrollTo({ y: positions.current.get(id) ?? 0, animated: false });
  };
  useEffect(() => { syncActiveDescendant(active && !error ? optionId(active) : null); }, [active, error, name]);
  useEffect(() => {
    if (Platform.OS !== "web") return;
    let input: DomInput | null = null, timer: ReturnType<typeof setTimeout> | undefined;
    const onKey = (event: DomKey) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); onBackRef.current(); return; }
      if (event.altKey || event.ctrlKey || event.metaKey || errorRef.current) return;
      const rowsNow = rowsRef.current.filter((row) => !row.problem);
      const activeId = activeRef.current;
      const highlighted = rowsNow.find((row) => row.id === activeId);
      if (event.key === "Enter" && highlighted) { event.preventDefault(); event.stopPropagation(); chooseRef.current(highlighted.id); return; }
      if (!rowsNow.length || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      event.preventDefault(); event.stopPropagation();
      if (event.key === "Home") { focusRow(rowsNow[0]!.id); return; }
      if (event.key === "End") { focusRow(rowsNow[rowsNow.length - 1]!.id); return; }
      const index = rowsNow.findIndex((row) => row.id === activeId);
      const next = rowsNow[index < 0 ? event.key === "ArrowUp" ? rowsNow.length - 1 : 0 : Math.max(0, Math.min(rowsNow.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]!;
      focusRow(next.id);
    };
    const attach = () => {
      input = inputOf();
      if (!input?.addEventListener) { timer = setTimeout(attach, 0); return; }
      input.addEventListener("keydown", onKey, true);
    };
    attach();
    return () => { if (timer) clearTimeout(timer); input?.removeEventListener?.("keydown", onKey, true); };
  }, [name, query]);
  return <View testID={name} style={{ flex: 1, flexShrink: 1, minHeight: 0, maxHeight: Math.max(180, height - 180), gap: 8, padding: 6 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{label}</Text>
    <View style={{ flexDirection: "row", gap: 6 }}>
      <TextInput ref={searchRef} testID={`${name}-search`} accessibilityLabel={label} accessibilityRole="combobox" aria-expanded aria-controls={listId} aria-autocomplete="list"
        autoFocus placeholder={placeholder} value={query} onChangeText={onQuery} maxLength={200} returnKeyType="search"
        onSubmitEditing={() => { const highlighted = available.find((row) => row.id === activeRef.current); if (highlighted) choose(highlighted.id); else onSearch?.(); }}
        placeholderTextColor={colors.foregroundMuted} style={{ minWidth: 0, flex: 1, color: colors.foreground, fontSize: 13, minHeight: 44, paddingHorizontal: 9, borderWidth: 1, borderRadius: 6, borderColor: colors.border }} />
      <Action label="Clear" accessibilityLabel={`Clear ${label.toLowerCase()} search`} colors={colors} disabled={!query} onPress={() => onQuery("")} />
    </View>
    {filters}
    <Text accessibilityLiveRegion="polite" aria-live="polite" style={{ color: error ? colors.statusWarning : colors.foregroundMuted, fontSize: 12 }}>{busy ? "Searching…" : error ?? message}</Text>
    <ScrollView ref={scroll} testID={`${name}-scroll`} keyboardShouldPersistTaps="handled" style={{ flex: 1, minHeight: 0 }} contentContainerStyle={{ gap: 2 }}>
      {/* RN 0.81's Role type omits the standard web listbox role. Native uses a single-choice group. */}
      <View nativeID={listId} role={Platform.OS === "web" ? "listbox" as ViewProps["role"] : "radiogroup"} accessibilityLabel={`${label} results`} aria-busy={busy}>
        {rows.map((row) => <View key={row.id} onLayout={(event) => { positions.current.set(row.id, event.nativeEvent.layout.y); }}>
          <Pressable {...(Platform.OS === "web" ? { id: optionId(row.id) } : {})} nativeID={optionId(row.id)} testID={`${name}-row-${row.id}`} role="option" accessibilityLabel={`${row.title}, ${row.detail}${row.problem ? `. ${row.problem}` : ""}`}
            accessibilityState={{ selected: active ? active === row.id : selected === row.id, disabled: busy || !!error || !!row.problem }} aria-selected={active ? active === row.id : selected === row.id}
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
