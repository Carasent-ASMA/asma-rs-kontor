import { useEffect, useRef, type ReactNode, type Ref } from "react";
import { AccessibilityInfo, BackHandler, findNodeHandle, Platform, Pressable, Text, View } from "react-native";
import { ScrollView } from "@getpaseo/plugin/client/react-native";
import { Action, type Colors } from "./page-controls";

/** Restore keyboard focus on web and screen-reader focus on native without opening the keyboard. */
export function focusControl(target: View | null): void {
  if (!target) return;
  target.focus?.();
  if (Platform.OS !== "web") {
    const handle = findNodeHandle(target);
    if (handle !== null) AccessibilityInfo.setAccessibilityFocus(handle);
  }
}

type PageKey = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type PageDocument = {
  addEventListener(type: "keydown", listener: (event: PageKey) => void): void;
  removeEventListener(type: "keydown", listener: (event: PageKey) => void): void;
};

/** Local page, not a modal: host tabs and browser history keep their native behavior. */
export function MobileFiltersPage({ colors, onBack, menuOpen, children }: {
  colors: Colors; onBack(): void; menuOpen: boolean; children: ReactNode;
}) {
  const backRef = useRef<View | null>(null);
  const latestOnBack = useRef(onBack);
  latestOnBack.current = onBack;
  useEffect(() => { focusControl(backRef.current); }, []);
  useEffect(() => {
    // A nested select handles its own Back/Escape first.
    if (menuOpen) return;
    // Keep this subscription in place: native Back visits newer outer menus first.
    const back = BackHandler.addEventListener("hardwareBackPress", () => { latestOnBack.current(); return true; });
    const document = (globalThis as { document?: PageDocument }).document;
    const escape = (event: PageKey) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault(); event.stopPropagation(); latestOnBack.current();
    };
    document?.addEventListener("keydown", escape);
    return () => { back.remove(); document?.removeEventListener("keydown", escape); };
  }, [menuOpen]);
  return <View testID="sessions-mobile-filters" style={{ flex: 1, minHeight: 0 }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, borderBottomWidth: 1, borderColor: colors.border }}>
      <Action ref={backRef} label="Back" icon="ArrowLeft" accessibilityLabel="Back to sessions, keep filter choices" onPress={onBack} colors={colors} />
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 17, fontWeight: "600" }}>Filters &amp; view</Text>
    </View>
    <ScrollView testID="sessions-mobile-filters-scroll" keyboardShouldPersistTaps="handled" automaticallyAdjustKeyboardInsets
      style={{ flex: 1, minHeight: 0 }} contentContainerStyle={{ padding: 12, paddingBottom: 24, gap: 12 }}>
      {children}
    </ScrollView>
  </View>;
}

export function MobileFiltersTrigger({ colors, count, onPress, ref }: {
  colors: Colors; count: number; onPress(): void; ref?: Ref<View>;
}) {
  return <Pressable ref={ref} testID="sessions-mobile-filters-trigger" accessibilityRole="button"
    accessibilityLabel={count ? `Filters and view, ${count} active filters or view choices` : "Filters and view"}
    accessibilityHint="Opens the full filter page. Choices apply immediately."
    onPress={onPress} style={{ minHeight: 44, minWidth: 44, paddingHorizontal: 12, borderRadius: 7, borderWidth: 1, borderColor: colors.accent, backgroundColor: colors.surface2, justifyContent: "center" }}>
    <Text style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>Filters{count ? ` · ${count}` : ""}</Text>
  </Pressable>;
}
