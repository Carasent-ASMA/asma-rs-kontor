import { useEffect, useRef, type ReactNode, type Ref } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { Action, COARSE_POINTER, type Colors } from "./page-controls";

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
  return <View ref={tray} testID={testID} role="region" accessibilityLabel="Notices" style={{ flexDirection: "row-reverse", alignItems: "flex-start", gap: 6 }}>
    <Action ref={hideRef} small={!compact} colors={colors} label="Hide" icon="X" iconOnly help="Hide notices" accessibilityLabel="Hide notices" disabled={disabled} onPress={onHide} />
    <View style={{ flex: 1, minWidth: 0, gap: 8 }}>{children}</View>
  </View>;
}

/** What a hidden tray leaves beside Resume: its tone, the first notice (a count on compact) and every notice in its name. */
export function NoticesButton({ notices, colors, compact, onPress, ref }: { notices: readonly Notice[]; colors: Colors; compact: boolean; onPress: () => void; ref?: Ref<View> }) {
  const ink = notices.some((notice) => notice.tone === "danger") ? colors.statusDanger : colors.statusWarning;
  const label = compact ? `${notices.length} notice${notices.length === 1 ? "" : "s"}` : `${notices[0]?.summary ?? ""}${notices.length > 1 ? ` +${notices.length - 1}` : ""}`;
  const touch = compact || COARSE_POINTER;
  return <Pressable ref={ref} testID="sessions-notices-button" accessibilityRole="button" accessibilityLabel={`Show notices: ${notices.map((notice) => notice.summary).join("; ")}`}
    accessibilityState={{ expanded: false }} aria-expanded={false} onPress={onPress}
    style={{ minHeight: touch ? 44 : 30, minWidth: 44, maxWidth: "100%", flexShrink: 1, flexDirection: compact ? "column" : "row", alignItems: "center", justifyContent: "center", gap: compact ? 1 : 5,
      paddingHorizontal: compact ? 4 : 9, borderRadius: compact ? 7 : 6, borderWidth: 1, borderColor: ink, backgroundColor: colors.surface1 }}>
    <Icon name="CircleAlert" size={compact ? 18 : 14} color={ink} />
    <Text numberOfLines={1} style={{ flexShrink: 1, color: compact ? colors.foreground : ink, fontSize: compact ? 10 : 12, fontWeight: "600" }}>{label}</Text>
  </Pressable>;
}
