import { createContext, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode, type Ref, type RefObject } from "react";
import { BackHandler, Platform, ScrollView, Text, View, type GestureResponderEvent } from "react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { anchoredPanelOpen, menuFrame, type MenuFrame, type MenuSize } from "./anchored-menu";

type Anchor = RefObject<View | null>;
type Help = { anchor: Anchor; text: string; focused: boolean; held?: boolean };
type Controller = { id: string; show(anchor: Anchor, text: string, focused?: boolean, held?: boolean): void; refresh(anchor: Anchor, text: string): void; fit(help: Help, size: MenuSize): void; press(touch: boolean, target?: unknown): void; leave(anchor: Anchor, blur?: boolean): void; forget(anchor: Anchor): void; enter(help: Help): void; exit(help: Help): void; dismiss(): void };
const HelpContext = createContext<Controller | null>(null);
type KeyEvent = { key?: string; pointerType?: string; clientX?: number; clientY?: number; target?: unknown; preventDefault(): void; stopPropagation(): void; stopImmediatePropagation?(): void };
type EventTarget = { addEventListener(type: string, listener: (event: KeyEvent) => void, capture?: boolean): void; removeEventListener(type: string, listener: (event: KeyEvent) => void, capture?: boolean): void };
type Contains = { contains?(target: unknown): boolean };

/** One screen-level, noninteractive overlay using the existing menu's bounds calculation.
 * Paseo 0.12 supplies no tooltip primitive. Icon actions opt in to Pressable's long
 * press; metadata remains in Details. No gesture timer, polling, portal or dependency.
 */
export function ControlHelpProvider({ children, colors }: { children: ReactNode; colors: PluginTheme["colors"] }) {
  const helpId = `kontor-help-${useId()}`;
  const root = useRef<View | null>(null), panel = useRef<View | null>(null);
  const current = useRef<Help | null>(null);
  const overPanel = useRef<Help | null>(null);
  const input = useRef<"keyboard" | "pointer" | "touch">("keyboard");
  const pressedAt = useRef<{ x?: number; y?: number } | null>(null);
  const fitted = useRef<{ text: string; size: MenuSize } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const revision = useRef(0);
  const [help, setHelp] = useState<Help | null>(null);
  const [frame, setFrame] = useState<MenuFrame | null>(null);
  const controller = useMemo<Controller>(() => {
    const cancel = () => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; };
    const dismiss = () => { cancel(); revision.current++; current.current = null; overPanel.current = null; fitted.current = null; setHelp(null); setFrame(null); };
    // Source hover-out can arrive after panel pointer-enter on React Native Web.
    const leave = (anchor: Anchor, blur = false) => {
      if (current.current?.anchor !== anchor) return;
      if (blur) current.current.focused = false;
      if (current.current.focused || current.current.held || overPanel.current?.anchor === anchor) return;
      cancel(); timer.current = setTimeout(dismiss, 160);
    };
    const place = (next: Help) => {
      const token = ++revision.current;
      // Hover belongs to this panel, not to the next control that reuses the overlay.
      if (current.current?.anchor !== next.anchor || current.current.text !== next.text) overPanel.current = null;
      current.current = next;
      root.current?.measureInWindow((rootX, rootY, width, height) => next.anchor.current?.measureInWindow((x, y, w, h) => {
        if (token !== revision.current || !w || !h || !width || !height) return;
        if (x + w < rootX || x > rootX + width || y + h < rootY || y > rootY + height) { setHelp(null); setFrame(null); return; }
        const size = fitted.current?.text === next.text ? fitted.current.size : null;
        // Native/web layout measurements can round down a fractional text width.
        setFrame(menuFrame({ x: x - rootX, y: y - rootY, width: w, height: h }, { width, height }, { align: "start", width: Math.min(size ? size.width + 1 : 360, 360), gap: 0, margin: 6, maxHeight: Math.min(size?.height ?? 220, 220, height - 12) }));
        setHelp(next);
      }));
    };
    return {
      id: helpId, dismiss, leave,
      enter(help) {
        if (current.current?.anchor !== help.anchor || current.current.text !== help.text) return;
        overPanel.current = help; cancel();
      },
      exit(help) {
        if (overPanel.current?.anchor !== help.anchor || overPanel.current.text !== help.text) return;
        overPanel.current = null; leave(help.anchor);
      },
      show(anchor, text, focused = false, held = false) {
        // Native keyboard/accessibility focus has no document keydown to reset touch mode.
        if (focused && Platform.OS !== "web") input.current = "keyboard";
        if (!text || (!held && (input.current === "touch" || (focused && input.current === "pointer") || (!focused && pressedAt.current) || (current.current?.focused && !focused)))) return;
        cancel();
        place({ anchor, text, focused, held });
      },
      refresh(anchor, text) {
        const value = current.current;
        if (!value || value.anchor !== anchor || value.text === text) return;
        if (!text) { dismiss(); return; }
        // Preserve focus ownership and any pending pointer-leave dismissal.
        place({ ...value, text });
      },
      fit(help, size) {
        const value = current.current;
        // Focus scrolling can replace the help object before its first panel layout arrives.
        if (!value || value.anchor !== help.anchor || value.text !== help.text || !size.width || !size.height) return;
        const previous = fitted.current;
        if (previous?.text === help.text && Math.abs(previous.size.width - size.width) < 0.5 && Math.abs(previous.size.height - size.height) < 0.5) return;
        fitted.current = { text: help.text, size };
        place(value);
      },
      press(touch, target) {
        input.current = touch ? "touch" : "pointer";
        if (!(panel.current as Contains | null)?.contains?.(target)) dismiss();
      },
      forget(anchor) { if (current.current?.anchor === anchor) dismiss(); },
    };
  }, [helpId]);
  useEffect(() => {
    const document = Platform.OS === "web" ? (globalThis as { document?: EventTarget }).document : undefined;
    const window = Platform.OS === "web" ? (globalThis as { window?: EventTarget }).window : undefined;
    const key = (event: KeyEvent) => {
      if (event.key && !["Shift", "Control", "Alt", "Meta"].includes(event.key)) input.current = "keyboard";
      if (event.key !== "Escape" || !current.current) return;
      controller.dismiss();
      // A menu/dialog keeps its existing one-press Escape and focus return.
      if (anchoredPanelOpen()) return;
      event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation?.();
    };
    const press = (event: KeyEvent) => {
      pressedAt.current = { x: event.clientX, y: event.clientY };
      controller.press(event.pointerType === "touch", event.target);
    };
    const pointer = (event: KeyEvent) => {
      if (event.pointerType === "touch") input.current = "touch";
      else if (event.pointerType && input.current === "touch") input.current = "pointer";
      // A closing panel can reveal another control under a stationary pointer.
      // Resume hover help only after the pointer actually moves.
      const point = pressedAt.current;
      if (point && (point.x !== event.clientX || point.y !== event.clientY)) pressedAt.current = null;
    };
    // Focus can scroll a row into view after its focus event. Re-measure its help;
    // hover-only help closes when its source moves. Scrolling help itself stays usable.
    const scroll = (event: KeyEvent) => {
      if ((panel.current as Contains | null)?.contains?.(event.target)) return;
      const value = current.current;
      if (value?.focused) controller.show(value.anchor, value.text, true);
      else controller.dismiss();
    };
    const moved = () => controller.dismiss();
    document?.addEventListener("keydown", key, true); document?.addEventListener("scroll", scroll, true);
    document?.addEventListener("pointerdown", press, true); document?.addEventListener("pointerover", pointer, true);
    document?.addEventListener("pointermove", pointer, true);
    document?.addEventListener("pointercancel", moved, true);
    window?.addEventListener("resize", moved);
    const back = BackHandler.addEventListener("hardwareBackPress", () => { if (!current.current) return false; controller.dismiss(); return true; });
    return () => { controller.dismiss(); document?.removeEventListener("keydown", key, true); document?.removeEventListener("scroll", scroll, true); document?.removeEventListener("pointerdown", press, true); document?.removeEventListener("pointerover", pointer, true); document?.removeEventListener("pointermove", pointer, true); document?.removeEventListener("pointercancel", moved, true); window?.removeEventListener("resize", moved); back.remove(); };
  }, [controller]);
  return <HelpContext.Provider value={controller}>
    <View ref={root} style={{ flex: 1, minWidth: 0, minHeight: 0 }} onLayout={controller.dismiss} onTouchStart={() => controller.press(true)} onTouchCancel={controller.dismiss}>
      {children}
      {help && frame ? <View testID="control-help-layer" pointerEvents="box-none" style={{ position: "absolute", inset: 0, zIndex: 100 }}>
        <View key={help.text} ref={panel} testID="control-help" nativeID={helpId} role="tooltip" onPointerEnter={() => controller.enter(help)} onPointerLeave={() => controller.exit(help)}
          onLayout={({ nativeEvent }) => controller.fit(help, nativeEvent.layout)}
          style={{ position: "absolute", left: frame.left, maxWidth: frame.width, opacity: fitted.current?.text === help.text ? 1 : 0, ...(frame.bottom === null ? { top: frame.top ?? 0 } : { bottom: frame.bottom }), maxHeight: frame.maxHeight,
            padding: 10, borderWidth: 1, borderColor: colors.border, borderRadius: 7, backgroundColor: colors.surface2, elevation: 12, shadowColor: "#000", shadowOpacity: 0.25, shadowRadius: 8 }}>
          <ScrollView style={{ flexShrink: 1 }}><Text selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 18 }}>{help.text}</Text></ScrollView>
        </View>
      </View> : null}
    </View>
  </HelpContext.Provider>;
}

/** Attach to the existing control; never nest an extra control inside a session row. */
export function useControlHelp(text: string, forwarded?: Ref<View>) {
  const controller = useContext(HelpContext);
  const anchor = useRef<View | null>(null);
  const focused = useRef(false);
  return {
    bindings: {
      "aria-describedby": text ? controller?.id : undefined,
      ref: (node: View | null) => {
        anchor.current = node;
        if (typeof forwarded === "function") forwarded(node); else if (forwarded) forwarded.current = node;
        // The existing ref is committed with the latest text. Only the active
        // anchor refreshes; no row effects, polling, or dismissed-help reopening.
        if (node) controller?.refresh(anchor, text);
      },
      onHoverIn: () => controller?.show(anchor, text, focused.current),
      onHoverOut: () => controller?.leave(anchor),
      onFocus: () => { focused.current = true; controller?.show(anchor, text, true); },
      onBlur: () => { focused.current = false; controller?.leave(anchor, true); },
    },
    dismiss: () => controller?.dismiss(),
    hold: () => controller?.show(anchor, text, false, true),
    forget: () => controller?.forget(anchor),
  };
}

/** Long-press discovery is opt-in for icon actions; ordinary row help has no cleanup effect. */
export function useActionHelp(text: string, forwarded?: Ref<View>) {
  const help = useControlHelp(text, forwarded);
  const gesture = useRef<{ x: number; y: number } | null>(null);
  const latestForget = useRef(help.forget); latestForget.current = help.forget;
  const cancelHold = () => { gesture.current = null; help.forget(); };
  useEffect(() => () => { gesture.current = null; latestForget.current(); }, []);
  return { ...help, bindings: {
    ...help.bindings,
    // Pressable owns the 500ms recognizer, movement cancellation and suppression of release activation.
    onPressIn: (event: GestureResponderEvent) => { gesture.current = { x: event.nativeEvent.pageX, y: event.nativeEvent.pageY }; },
    onPressOut: () => { gesture.current = null; },
    onLongPress: () => { if (gesture.current) help.hold(); },
    onTouchMove: (event: GestureResponderEvent) => {
      const start = gesture.current;
      if (!start || Math.hypot(event.nativeEvent.pageX - start.x, event.nativeEvent.pageY - start.y) > 10) cancelHold();
    },
    onTouchCancel: cancelHold,
    onPointerCancel: cancelHold,
  } };
}
