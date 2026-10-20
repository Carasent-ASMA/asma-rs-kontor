import { createContext, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode, type Ref, type RefObject } from "react";
import { BackHandler, ScrollView, Text, View } from "react-native";
import type { PluginTheme } from "@getpaseo/plugin";
import { anchoredPanelOpen, menuFrame, type MenuFrame } from "./anchored-menu";

type Anchor = RefObject<View | null>;
type Help = { anchor: Anchor; text: string; focused: boolean };
type Controller = { id: string; show(anchor: Anchor, text: string, focused?: boolean): void; leave(anchor: Anchor, blur?: boolean): void; enter(): void; dismiss(): void };
const HelpContext = createContext<Controller | null>(null);
type KeyEvent = { key: string; target?: unknown; preventDefault(): void; stopPropagation(): void; stopImmediatePropagation?(): void };
type EventTarget = { addEventListener(type: string, listener: (event: KeyEvent) => void, capture?: boolean): void; removeEventListener(type: string, listener: (event: KeyEvent) => void, capture?: boolean): void };
type Contains = { contains?(target: unknown): boolean };

/** One screen-level, noninteractive overlay using the existing menu's bounds calculation.
 * Paseo 0.12 supplies no tooltip primitive. There are no row effects, polling, DOM
 * portals or new dependencies. Touch keeps action labels and opens metadata in Details.
 */
export function ControlHelpProvider({ children, colors }: { children: ReactNode; colors: PluginTheme["colors"] }) {
  const helpId = `kontor-help-${useId()}`;
  const root = useRef<View | null>(null), panel = useRef<View | null>(null);
  const current = useRef<Help | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const revision = useRef(0);
  const [help, setHelp] = useState<Help | null>(null);
  const [frame, setFrame] = useState<MenuFrame | null>(null);
  const controller = useMemo<Controller>(() => {
    const cancel = () => { if (timer.current !== null) clearTimeout(timer.current); timer.current = null; };
    const dismiss = () => { cancel(); revision.current++; current.current = null; setHelp(null); setFrame(null); };
    return {
      id: helpId, dismiss, enter: cancel,
      show(anchor, text, focused = false) {
        if (!text || (current.current?.focused && !focused)) return;
        cancel();
        const next = { anchor, text, focused };
        const token = ++revision.current;
        current.current = next;
        root.current?.measureInWindow((rootX, rootY, width, height) => anchor.current?.measureInWindow((x, y, w, h) => {
          if (token !== revision.current || !w || !h || !width || !height) return;
          if (x + w < rootX || x > rootX + width || y + h < rootY || y > rootY + height) { setHelp(null); setFrame(null); return; }
          setFrame(menuFrame({ x: x - rootX, y: y - rootY, width: w, height: h }, { width, height }, { align: "start", width: 360, gap: 0, margin: 6, maxHeight: Math.min(220, height - 12) }));
          setHelp(next);
        }));
      },
      leave(anchor, blur = false) {
        if (current.current?.anchor !== anchor) return;
        if (blur) current.current.focused = false;
        if (current.current.focused) return;
        cancel(); timer.current = setTimeout(dismiss, 160);
      },
    };
  }, [helpId]);
  useEffect(() => {
    const document = (globalThis as { document?: EventTarget }).document;
    const window = (globalThis as { window?: EventTarget }).window;
    const key = (event: KeyEvent) => {
      if (event.key !== "Escape" || !current.current) return;
      controller.dismiss();
      // A menu/dialog keeps its existing one-press Escape and focus return.
      if (anchoredPanelOpen()) return;
      event.preventDefault(); event.stopPropagation(); event.stopImmediatePropagation?.();
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
    window?.addEventListener("resize", moved);
    const back = BackHandler.addEventListener("hardwareBackPress", () => { if (!current.current) return false; controller.dismiss(); return true; });
    return () => { controller.dismiss(); document?.removeEventListener("keydown", key, true); document?.removeEventListener("scroll", scroll, true); window?.removeEventListener("resize", moved); back.remove(); };
  }, [controller]);
  return <HelpContext.Provider value={controller}>
    <View ref={root} style={{ flex: 1, minWidth: 0, minHeight: 0 }} onLayout={controller.dismiss}>
      {children}
      {help && frame ? <View testID="control-help-layer" pointerEvents="box-none" style={{ position: "absolute", inset: 0, zIndex: 100 }}>
        <View ref={panel} testID="control-help" nativeID={helpId} role="tooltip" onPointerEnter={controller.enter} onPointerLeave={() => controller.leave(help.anchor)}
          style={{ position: "absolute", left: frame.left, width: frame.width, ...(frame.bottom === null ? { top: frame.top ?? 0 } : { bottom: frame.bottom }), maxHeight: frame.maxHeight,
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
      ref: (node: View | null) => { anchor.current = node; if (typeof forwarded === "function") forwarded(node); else if (forwarded) forwarded.current = node; },
      onHoverIn: () => controller?.show(anchor, text, focused.current),
      onHoverOut: () => controller?.leave(anchor),
      onFocus: () => { focused.current = true; controller?.show(anchor, text, true); },
      onBlur: () => { focused.current = false; controller?.leave(anchor, true); },
    },
    dismiss: () => controller?.dismiss(),
  };
}
