import { useEffect, useRef, useState } from "react";
import { BackHandler, Pressable, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { COARSE_POINTER, type Colors } from "./page-controls";
import { focusControl } from "./mobile-filters";
import { normalizeChoice } from "./workstream-selector";
import type { ScopeChoice } from "./session-scope";
import { readableInk } from "./session-status-ink";
import { onEscape } from "./web";

/** Searchable single choice; the selected key survives an unavailable inventory. */
export function SessionScopeSelector({ id, label, options, selected, open, colors, compact, disabled, onOpen, onChange }: {
  id: string; label: string; options: readonly ScopeChoice[]; selected: string | null; open: boolean;
  colors: Colors; compact: boolean; disabled?: boolean;
  onOpen(open: boolean): void; onChange(key: string | null): void;
}) {
  const [search, setSearch] = useState("");
  const trigger = useRef<View | null>(null);
  const close = useRef(() => {});
  close.current = () => { onOpen(false); focusControl(trigger.current); };
  useEffect(() => {
    if (!open) return;
    const back = BackHandler.addEventListener("hardwareBackPress", () => { close.current(); return true; });
    const escape = onEscape(() => close.current());
    return () => { back.remove(); escape(); };
  }, [open]);
  const chosen = selected === null ? null : options.find((option) => option.key === selected)
    ?? { key: selected, label: "Unavailable selection", detail: selected };
  const needle = normalizeChoice(search);
  const choices = options.filter((option) => option.key !== selected && normalizeChoice(`${option.label} ${option.key} ${option.detail ?? ""}`).includes(needle));
  const height = compact || COARSE_POINTER ? 44 : 32;
  const choose = (key: string | null) => { onChange(key); close.current(); };
  const row = (option: ScopeChoice | null) => <Pressable key={option?.key ?? "all"} testID={`sessions-${id}-choice-${option?.key ?? "all"}`}
    disabled={disabled} accessibilityRole="button" accessibilityState={{ selected: selected === (option?.key ?? null), disabled }}
    accessibilityLabel={[option?.label ?? `All ${label.toLowerCase()}`, option?.detail].filter(Boolean).join(". ")}
    onPress={() => choose(option?.key ?? null)} style={{ minHeight: height, justifyContent: "center", paddingHorizontal: 8, gap: 2 }}>
    <Text style={{ color: colors.foreground, fontSize: compact ? 13 : 12 }}>{selected === (option?.key ?? null) ? "✓ " : ""}{option?.label ?? `All ${label.toLowerCase()}`}</Text>
    {option?.detail ? <Text selectable style={{ color: colors.foregroundMuted, fontSize: 11 }}>{option.detail}</Text> : null}
  </Pressable>;
  return <View style={{ gap: 6 }}>
    <Pressable ref={trigger} testID={`sessions-${id}-toggle`} disabled={disabled} accessibilityRole="button"
      accessibilityState={{ expanded: open, disabled }} aria-expanded={open}
      accessibilityLabel={`${label}: ${chosen ? `${chosen.label}${chosen.detail ? `, ${chosen.detail}` : ""}` : "All"}`}
      onPress={() => onOpen(!open)} style={{ minHeight: height, justifyContent: "center" }}>
      <Text style={{ color: colors.foreground, fontSize: compact ? 13 : 12, fontWeight: "600" }}>{open ? "▾" : "▸"} {label}: {chosen?.label ?? "All"}</Text>
    </Pressable>
    {open ? <View style={{ gap: 4 }}>
      <TextInput testID={`sessions-${id}-search`} value={search} onChangeText={setSearch} accessibilityLabel={`Find ${label.toLowerCase()}`}
        placeholder={`Find ${label.toLowerCase()}`} placeholderTextColor={colors.foregroundMuted} autoCapitalize="none" autoCorrect={false}
        style={{ minHeight: height, borderWidth: 1, borderColor: colors.border, borderRadius: 7, paddingHorizontal: 10, color: colors.foreground, fontSize: compact ? 16 : 12 }} />
      {row(null)}
      {chosen ? row(chosen) : null}
      {choices.map(row)}
      {!choices.length ? <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>No other {label.toLowerCase()} matches.</Text> : null}
      <Pressable testID={`sessions-${id}-close`} accessibilityRole="button" accessibilityLabel={`Close ${label.toLowerCase()} choices`} onPress={() => close.current()}
        style={{ minHeight: height, justifyContent: "center" }}><Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12 }}>Done</Text></Pressable>
    </View> : null}
  </View>;
}
