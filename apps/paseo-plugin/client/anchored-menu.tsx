import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { BackHandler, Pressable, Text, View } from "react-native";
import { COARSE_POINTER, type Colors } from "./page-controls";

export type MenuRect = { x: number; y: number; width: number; height: number };
export type MenuSize = { width: number; height: number };
export type MenuFrame = { left: number; width: number; top: number | null; bottom: number | null; maxHeight: number };
/** `cover` opens at the root's top margin and may use all of its height inside the margins, instead of opening beside the trigger, so a short screen can show all it fits. */
export type MenuPlacement = { align: "start" | "end" | "stretch"; width: number; gap?: number; margin?: number; maxHeight?: number; cover?: boolean };

/** Places a menu against its trigger inside the screen root, clamped to the visible bounds; opens upward only when below is the smaller space. */
export function menuFrame(anchor: MenuRect, root: MenuSize, placement: MenuPlacement): MenuFrame {
  const gap = placement.gap ?? 4, margin = placement.margin ?? 6, cap = placement.maxHeight ?? Number.POSITIVE_INFINITY;
  const width = Math.max(0, placement.align === "stretch" ? root.width - 2 * margin : Math.min(placement.width, root.width - 2 * margin));
  const preferred = placement.align === "end" ? anchor.x + anchor.width - width : placement.align === "stretch" ? margin : anchor.x;
  const left = Math.max(margin, Math.min(preferred, root.width - margin - width));
  if (placement.cover) return { left, width, top: margin, bottom: null, maxHeight: Math.max(0, Math.min(cap, root.height - 2 * margin)) };
  const below = root.height - (anchor.y + anchor.height + gap) - margin, above = anchor.y - gap - margin;
  if (below >= Math.min(cap, 240) || below >= above) return { left, width, top: anchor.y + anchor.height + gap, bottom: null, maxHeight: Math.max(0, Math.min(cap, below)) };
  return { left, width, top: null, bottom: root.height - anchor.y + gap, maxHeight: Math.max(0, Math.min(cap, above)) };
}

type Measurable = { measureInWindow(callback: (x: number, y: number, width: number, height: number) => void): void };
export type FocusTarget = { focus?: () => void; querySelectorAll?: (selector: string) => ArrayLike<FocusTarget>; contains?: (node: unknown) => boolean; getClientRects?: () => ArrayLike<unknown> };
type KeyEvent = { key: string; shiftKey?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyTarget = { activeElement?: unknown; body?: unknown; addEventListener(type: "keydown", listener: (event: KeyEvent) => void, capture: boolean): void; removeEventListener(type: "keydown", listener: (event: KeyEvent) => void, capture: boolean): void };
const ITEMS = '[role="menuitem"]';
/** What Tab can reach; React Native Web gives each pressable and focusable view a tabindex. */
const TABBABLE = ':is(a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]):not([tabindex="-1"])';
const drawn = (node: FocusTarget) => !node.getClientRects || node.getClientRects().length > 0;
/** Open dialogs, newest last: only the top one keeps Tab inside it. */
const dialogs: object[] = [];
/** A number for each trigger ref, so a swap to another trigger reads as another opening. */
const triggers = new WeakMap<object, number>();
let triggerCount = 0;
const triggerKey = (anchor: object) => { let key = triggers.get(anchor); if (key === undefined) triggers.set(anchor, key = ++triggerCount); return key; };

/**
 * Where Tab (Shift+Tab when `back`) must move focus to stay in a modal panel, or null where the browser's own
 * move already stays inside. Focus behind the panel in its screen, or lost to the page body, comes back in at the
 * first or last stop; focus in the host app outside the screen, or a panel that is not drawn, is left alone.
 */
export function containedTab(panel: FocusTarget | null, screen: FocusTarget | null, active: unknown, body: unknown, back: boolean): FocusTarget | null {
  if (!panel || !drawn(panel)) return null;
  if (active != null && active !== body && !panel.contains?.(active) && !screen?.contains?.(active)) return null;
  const stops = Array.from(panel.querySelectorAll?.(TABBABLE) ?? []).filter(drawn);
  if (!stops.length) return panel;
  const index = stops.findIndex((stop) => stop === active);
  if (index >= 0 && index !== (back ? 0 : stops.length - 1)) return null;
  return back ? stops[stops.length - 1]! : stops[0]!;
}

/** Measures the trigger against the screen root; a later open, close or resize discards an earlier answer. */
export function measureMenu(root: Measurable | null, anchor: Measurable | null, placement: MenuPlacement, isCurrent: () => boolean, apply: (frame: MenuFrame) => void): void {
  root?.measureInWindow((rootX, rootY, rootWidth, rootHeight) => anchor?.measureInWindow((x, y, width, height) => {
    if (!isCurrent() || !width || !height || !rootWidth || !rootHeight) return;
    apply(menuFrame({ x: x - rootX, y: y - rootY, width, height }, { width: rootWidth, height: rootHeight }, placement));
  }));
}

type AnchoredMenuProps = {
  anchor: RefObject<View | null>; root: RefObject<View | null>; rootSize: MenuSize; placement: MenuPlacement;
  onClose: () => void; colors: Colors; label: string; dialog?: boolean; testID?: string; header?: ReactNode; children: ReactNode;
};

/**
 * Plugin-owned popover in the screen root: the SDK offers no anchored screen menu,
 * so this draws its own backdrop for outside presses and handles Escape/arrow keys on web
 * and the Android back button. Mounted only while open. A `dialog` is modal: on web, Tab and
 * Shift+Tab cycle inside the top open one, and with nothing to reach the panel itself holds focus.
 */
export function AnchoredMenu(props: AnchoredMenuProps) {
  // A slot that swaps one opening for another without closing (More, then the context picker; Group by, then the
  // workspace scope) mounts the new one afresh: it joins or leaves the dialogs, measures against its own trigger,
  // focuses its first item and returns focus to that trigger.
  return <AnchoredPanel key={`${props.dialog ? "dialog" : "menu"}:${triggerKey(props.anchor)}`} {...props} />;
}

function AnchoredPanel({ anchor, root, rootSize, placement, onClose, colors, label, dialog = false, testID, header, children }: AnchoredMenuProps) {
  const [frame, setFrame] = useState<MenuFrame | null>(null);
  const token = useRef(0);
  const panel = useRef<View | null>(null);
  const close = useRef(onClose); close.current = onClose;
  const restoreFocus = () => (anchor.current as unknown as FocusTarget | null)?.focus?.();
  useEffect(() => {
    const current = ++token.current;
    measureMenu(root.current, anchor.current, placement, () => token.current === current, setFrame);
    return () => { token.current++; };
  }, [rootSize.width, rootSize.height, placement.align, placement.width, placement.maxHeight]);
  useEffect(() => {
    if (!frame || header) return;
    const items = (panel.current as unknown as FocusTarget | null)?.querySelectorAll?.(ITEMS);
    const first = items?.[0]; if (first) first.focus?.();
  }, [frame !== null]);
  useEffect(() => {
    const back = BackHandler.addEventListener("hardwareBackPress", () => { close.current(); return true; });
    const document = (globalThis as { document?: KeyTarget }).document;
    const self = {};
    if (dialog) dialogs.push(self);
    const onKey = (event: KeyEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close.current(); restoreFocus(); return; }
      if (event.key === "Tab") {
        // Only dialogs join the stack, so a menu is never on top of it.
        if (dialogs[dialogs.length - 1] !== self || event.altKey || event.ctrlKey || event.metaKey) return;
        const target = containedTab(panel.current as unknown as FocusTarget | null, root.current as unknown as FocusTarget | null, document?.activeElement, document?.body, !!event.shiftKey);
        if (target) { event.preventDefault(); target.focus?.(); }
        return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      const node = panel.current as unknown as FocusTarget | null;
      const items = node?.querySelectorAll ? Array.from(node.querySelectorAll(ITEMS)) : [];
      if (!items.length) return;
      event.preventDefault();
      const index = items.findIndex((item) => item === document?.activeElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : event.key === "ArrowDown" ? (index + 1) % items.length : (index <= 0 ? items.length : index) - 1;
      items[next]?.focus?.();
    };
    document?.addEventListener("keydown", onKey, true);
    return () => {
      back.remove(); document?.removeEventListener("keydown", onKey, true);
      const index = dialogs.indexOf(self); if (index >= 0) dialogs.splice(index, 1);
    };
  }, []);
  return <View testID={testID ? `${testID}-layer` : undefined} pointerEvents="box-none" style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, zIndex: 50 }}>
    <Pressable testID={testID ? `${testID}-backdrop` : undefined} accessibilityLabel={`Close ${label}`} importantForAccessibility="no" accessibilityElementsHidden
      onPress={() => { close.current(); restoreFocus(); }} style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0 }} />
    <View ref={panel} testID={testID} role={dialog ? "dialog" : "menu"} accessibilityLabel={label} aria-modal={dialog || undefined} tabIndex={dialog ? -1 : undefined}
      style={{ position: "absolute", opacity: frame ? 1 : 0, left: frame?.left ?? 0, width: frame?.width ?? placement.width, ...(frame?.bottom != null ? { bottom: frame.bottom } : { top: frame?.top ?? 0 }), maxHeight: frame?.maxHeight,
        borderWidth: 1, borderColor: colors.border, borderRadius: 8, padding: 5, backgroundColor: colors.surface1, overflow: "hidden",
        shadowColor: "#000", shadowOpacity: 0.25, shadowRadius: 14, shadowOffset: { width: 0, height: 8 }, elevation: 8 }}>
      {header}
      {children}
    </View>
  </View>;
}

export function MenuItem({ label, detail, accessibilityLabel, selected = false, onPress, colors, tall = false, indent = 0 }: {
  label: string; detail?: string | null; accessibilityLabel?: string; selected?: boolean; onPress: () => void; colors: Colors; tall?: boolean; indent?: number;
}) {
  return <Pressable role="menuitem" accessibilityLabel={accessibilityLabel ?? (detail ? `${label}, ${detail}` : label)} accessibilityState={{ selected }} aria-current={selected || undefined} onPress={onPress}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ minHeight: tall || COARSE_POINTER ? 44 : 32, paddingLeft: 10 + 12 * indent, paddingRight: 10, paddingVertical: 6, borderRadius: 5, justifyContent: "center",
      backgroundColor: pressed || hovered || focused ? colors.surface2 : selected ? colors.surface2 : undefined })}>
    <MenuItemText label={label} detail={detail} selected={selected} colors={colors} tall={tall} />
  </Pressable>;
}
function MenuItemText({ label, detail, selected, colors, tall }: { label: string; detail?: string | null; selected: boolean; colors: Colors; tall: boolean }) {
  return <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
    <Text numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: tall ? 13 : 12, fontWeight: selected ? "600" : "400" }}>{label}</Text>
    {detail ? <Text numberOfLines={1} style={{ flexShrink: 0, maxWidth: "45%", color: colors.foregroundMuted, fontSize: 10, fontFamily: "monospace", marginLeft: "auto" }}>{detail}</Text> : null}
  </View>;
}
