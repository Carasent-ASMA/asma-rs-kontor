import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Icon } from "@getpaseo/plugin/client/ui";
import { BackHandler, Pressable, Text, View } from "react-native";
import { AnchoredMenu, consumeEscapeRelease } from "./anchored-menu";
import { useTokenomicsOverlay } from "./tokenomics-choice";
import { focusControl } from "./mobile-filters";
import type { Colors } from "./page-controls";

/** Secondary prose is available on demand; scope, units and actionable data warnings stay visible. */
export function TokenomicsHelp({ title, children, colors, short = false, hint }: { title: string; children: ReactNode; colors: Colors; short?: boolean; hint?: string }) {
  const helpId = `report-help-${useId()}`;
  const [open, setOpen] = useState(false), [focused, setFocused] = useState(false), anchor = useRef<View>(null), closeButton = useRef<View>(null), context = useTokenomicsOverlay();
  const close = () => { setOpen(false); requestAnimationFrame(() => focusControl(anchor.current)); };
  useEffect(() => {
    if (!open || !short || !context) return;
    context.popup(<AnchoredMenu contentRole="note" anchor={anchor} root={context.root} rootSize={context.size} placement={{ align: "start", width: 320, maxHeight: 260 }} colors={colors} label={`About ${title}`} onClose={close}><View nativeID={helpId} style={{ padding: 10, gap: 8 }}>{children}</View></AnchoredMenu>);
    return () => context.popup(null);
  }, [open, short, context, children, colors]);
  useEffect(() => {
    if (!open || short) return;
    const back = BackHandler.addEventListener("hardwareBackPress", () => { close(); return true; });
    type Key = { key: string; preventDefault(): void; stopPropagation(): void };
    const document = (globalThis as { document?: { addEventListener(type: "keydown" | "keyup", cb: (e: Key) => void, capture: boolean): void; removeEventListener(type: "keydown" | "keyup", cb: (e: Key) => void, capture: boolean): void } }).document;
    const key = (e: Key) => { if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); consumeEscapeRelease(document); close(); } };
    document?.addEventListener("keydown", key, true);
    return () => { back.remove(); document?.removeEventListener("keydown", key, true); };
  }, [open, short]);
  return <>
    <Pressable ref={anchor} accessibilityRole="button" accessibilityLabel={`About ${title}`} accessibilityHint={hint} aria-describedby={short && open ? helpId : undefined} accessibilityState={{ expanded: open }} onFocus={() => { setFocused(true); context?.reveal?.(anchor.current, title); }} onBlur={() => setFocused(false)} onPress={() => setOpen(!open)} style={({ pressed }) => ({ minHeight: 44, minWidth: 44, padding: 10, alignItems: "center", justifyContent: "center", alignSelf: "flex-start", borderRadius: 8, borderWidth: 1, borderColor: focused ? colors.accent : "transparent", backgroundColor: pressed || open ? colors.surface2 : undefined })}><Icon name="Info" size={17} color={colors.foregroundMuted} /></Pressable>
    {open && (!short || !context) ? <View style={{ width: "100%" }}><View testID="report-contextual-help" style={{ width: "100%", maxWidth: 680, padding: 12, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.surface2, borderRadius: 8, gap: 10 }}>{children}<Pressable ref={closeButton} onFocus={() => context?.reveal?.(closeButton.current, title)} accessibilityRole="button" accessibilityLabel={`Close help about ${title}`} onPress={close} style={{ minHeight: 44, minWidth: 44, alignSelf: "flex-start", justifyContent: "center" }}><Text style={{ color: colors.accent, fontSize: 14 }}>Close help</Text></Pressable></View></View> : null}
  </>;
}
export function TokenomicsExplanation({ children }: { children: ReactNode }) { return <>{children}</>; }
