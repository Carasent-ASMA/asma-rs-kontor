import { useEffect, useRef, type ReactNode, type Ref } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { Action, type Colors } from "./page-controls";
import { useActionHelp } from "./control-help";

/** One top-of-screen notice. Its keys name what it reports, so a changed or new notice is never one the operator hid. */
export type Notice = { readonly keys: readonly string[]; readonly summary: string; readonly tone: "warning" | "danger"; readonly node: ReactNode };

/** Hidden only while every key of every current notice was hidden; anything new shows the tray again. */
export function noticesHidden(notices: readonly Notice[], hidden: readonly string[]): boolean {
  return notices.length > 0 && notices.every((notice) => notice.keys.every((key) => hidden.includes(key)));
}

export function hideNotices(notices: readonly Notice[], hidden: readonly string[]): string[] {
  return [...new Set([...hidden, ...notices.flatMap((notice) => notice.keys)])];
}

export function showNotices(notices: readonly Notice[], hidden: readonly string[]): string[] {
  const shown = new Set(notices.flatMap((notice) => notice.keys));
  return hidden.filter((key) => !shown.has(key));
}

/**
 * Hidden keys still reported, or not yet re-read: `read(key)` says its source completed a reading. A key a completed
 * reading no longer reports is forgotten, so the same notice coming back is a new occurrence and opens the tray.
 * Returns `hidden` itself when nothing is forgotten, so a caller writes only on a real change.
 */
export function presentHidden(notices: readonly Notice[], hidden: readonly string[], read: (key: string) => boolean): readonly string[] {
  const present = new Set(notices.flatMap((notice) => notice.keys));
  const kept = hidden.filter((key) => present.has(key) || !read(key));
  return kept.length === hidden.length ? hidden : kept;
}

type EscapeKey = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyDocument = { activeElement?: unknown; addEventListener(type: "keydown", listener: (event: EscapeKey) => void, capture: boolean): void; removeEventListener(type: "keydown", listener: (event: EscapeKey) => void, capture: boolean): void };

/**
 * Top notices as one non-modal disclosure region (WAI-ARIA APG Disclosure). Hide is presentation only: it never
 * runs, cancels or resolves anything. Escape hides it only while focus is inside, in the capture phase, so a menu,
 * dialog or page that owns focus keeps its own Escape. No outside-press dismissal: a status is never lost by accident.
 */
export function NoticeTray({ colors, compact, disabled = false, onHide, hideRef, testID, children }: {
  colors: Colors; compact: boolean; disabled?: boolean; onHide: () => void; hideRef?: Ref<View>; testID?: string; children: ReactNode;
}) {
  const tray = useRef<View | null>(null);
  const hide = useRef(onHide); hide.current = onHide;
  useEffect(() => {
    if (Platform.OS !== "web") return;
    const document = (globalThis as { document?: KeyDocument }).document;
    const key = (event: EscapeKey) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (!(tray.current as unknown as { contains?: (node: unknown) => boolean } | null)?.contains?.(document?.activeElement)) return;
      event.preventDefault(); event.stopPropagation(); hide.current();
    };
    document?.addEventListener("keydown", key, true);
    return () => document?.removeEventListener("keydown", key, true);
  }, []);
  // Hide comes first for focus and reading order, drawn at the top right: Show lands on it, so Enter toggles back.
  // Compact draws it as a 44 pt close mark, as the desktop does: "X Hide" would push a notice's actions to a second row.
  return <View ref={tray} testID={testID} role="region" accessibilityLabel="Notices" style={{ flexDirection: "row-reverse", alignItems: "flex-start", gap: 6 }}>
    {compact ? <Pressable ref={hideRef} accessibilityRole="button" accessibilityLabel="Hide notices" accessibilityState={{ disabled }} aria-disabled={disabled} disabled={disabled} onPress={onHide}
      style={{ width: 44, height: 44, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.5 : 1 }}><Icon name="X" size={18} color={colors.foregroundMuted} /></Pressable>
      : <Action ref={hideRef} small colors={colors} label="Hide" icon="X" iconOnly help="Hide notices" accessibilityLabel="Hide notices" disabled={disabled} onPress={onHide} />}
    <View style={{ flex: 1, minWidth: 0, gap: 8 }}>{children}</View>
  </View>;
}

/** Icon and count for the hidden tray; its name and deliberate help retain every notice summary. */
export function NoticesButton({ notices, colors, onPress, ref }: { notices: readonly Notice[]; colors: Colors; compact: boolean; onPress: () => void; ref?: Ref<View> }) {
  const ink = notices.some((notice) => notice.tone === "danger") ? colors.statusDanger : colors.statusWarning;
  const summary = notices.map((notice) => notice.summary).join("; ");
  const hint = `Notices. Show ${notices.length} notice${notices.length === 1 ? "" : "s"}: ${summary}.`;
  const help = useActionHelp(hint, ref);
  return <Pressable {...help.bindings} testID="sessions-notices-button" accessibilityRole="button" accessibilityLabel={`Show notices: ${summary}`} accessibilityHint={hint}
    accessibilityState={{ expanded: false }} aria-expanded={false} onPress={() => { help.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 44, alignItems: "center", justifyContent: "center",
      borderRadius: 6, borderWidth: 1, borderColor: focused ? colors.foreground : "transparent", backgroundColor: pressed ? colors.surface2 : undefined })}>
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name="CircleAlert" size={18} color={ink} /></View>
    <Text accessibilityElementsHidden importantForAccessibility="no" style={{ position: "absolute", top: 1, right: 2, minWidth: 14, textAlign: "center", color: colors.foreground, backgroundColor: colors.surface2, borderRadius: 7, fontSize: 10, fontWeight: "600" }}>{notices.length}</Text>
  </Pressable>;
}
