/**
 * Host chrome for a composer pill's registered popover, after Paseo's `ButtonControl` and `PluginPopoverSurface`
 * (packages/app/src/plugins/buttons/view.tsx and popover.tsx, components/ui/menu/menu-surface.tsx and menu-overlay.tsx,
 * components/ui/anchor.ts, at local source fed19f3d, not claimed to be the installed build). Below Paseo's compact form
 * factor, 600 points, it is a bottom sheet headed by the button's title; wider, a surface above the pill, aligned to its
 * start 12 points off it, 280 to 420 wide and at most 440 tall, flipped and kept 8 points inside the window, over a
 * transparent backdrop that closes it. Not Paseo's pixels: no animation or pan-down, and the pill is measured again
 * when the window resizes, where the source measures it once on opening.
 */
import { useEffect, useState, type ReactNode, type RefObject } from "react";
import { Modal, Pressable, ScrollView, Text, View, useWindowDimensions } from "react-native-web";
import { shellColors, themeName } from "./theme";

/** Paseo's compact form factor ends with its sm breakpoint; the plugin's `layout.compact` follows the same width. */
export const SHEET_BELOW_WIDTH = 600;
const EDGE = 8, OFFSET = 12, MIN_WIDTH = 280, MAX_WIDTH = 420, MAX_HEIGHT = 440;
// The host's own borderAccent and md shadow, which the plugin theme does not carry.
const BORDER_ACCENT = themeName === "light" ? "#ececf1" : "#2F3534";
const SHADOW = themeName === "light" ? "0 4px 16px rgba(0,0,0,0.04)" : "0 4px 8px rgba(0,0,0,0.20)";

type Rect = { x: number; y: number; width: number; height: number };
type Size = { width: number; height: number };

/**
 * `computePosition` for side top, align start: above the trigger unless it does not fit there and below has more room,
 * then pulled back inside the window.
 */
export function placeAbove(trigger: Rect, content: Size, window: Size): { x: number; y: number; side: "top" | "bottom" } {
  const above = trigger.y, below = window.height - trigger.y - trigger.height;
  const side = above < content.height && below > above ? "bottom" : "top";
  const y = side === "top" ? trigger.y - content.height - OFFSET : trigger.y + trigger.height + OFFSET;
  return { side, x: Math.max(EDGE, Math.min(window.width - content.width - EDGE, trigger.x)), y: Math.max(EDGE, Math.min(window.height - content.height - EDGE, y)) };
}

/** The surface's visible height: its content, up to 440 and the window less 16. */
export const surfaceHeight = (content: number, window: number) => Math.min(content, MAX_HEIGHT, Math.max(window - 16, 0));

export function PluginPopover({ title, anchor, onClose, children }: { title: string; anchor: RefObject<HTMLElement | null>; onClose(): void; children: ReactNode }) {
  const colors = shellColors(), window = useWindowDimensions();
  const [trigger, setTrigger] = useState<Rect | null>(null);
  const [content, setContent] = useState<Size | null>(null);
  // As MenuOverlay's restoreFocusRef: closing hands focus back to the pill, wherever the body had moved it.
  useEffect(() => () => anchor.current?.focus(), [anchor]);
  useEffect(() => {
    const box = anchor.current?.getBoundingClientRect();
    setTrigger(box ? { x: box.left, y: box.top, width: box.width, height: box.height } : null);
  }, [anchor, window.width, window.height]);
  if (window.width < SHEET_BELOW_WIDTH) return <Modal transparent visible onRequestClose={onClose}>
    <View style={{ flex: 1, justifyContent: "flex-end" }}>
      <Pressable testID="host-popover-backdrop" accessibilityRole="button" accessibilityLabel="Bottom sheet backdrop" onPress={onClose}
        style={{ position: "absolute", top: 0, right: 0, bottom: 0, left: 0, backgroundColor: "rgba(0,0,0,0.45)" }} />
      <View testID="host-popover-sheet" style={{ maxHeight: window.height * 0.8, backgroundColor: colors.surface0, borderWidth: 1, borderColor: colors.border, borderTopLeftRadius: 12, borderTopRightRadius: 12 }}>
        <View style={{ paddingVertical: 10, alignItems: "center" }}><View style={{ width: 36, height: 4, borderRadius: 2, backgroundColor: colors.surface2 }} /></View>
        <ScrollView keyboardShouldPersistTaps="handled" style={{ flexShrink: 1 }} contentContainerStyle={{ paddingBottom: 4 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 12, paddingTop: 4, paddingBottom: 12 }}>
            <Text testID="host-popover-sheet-title" numberOfLines={1} style={{ flexShrink: 1, color: colors.foreground, fontSize: 14, fontWeight: "500" }}>{title}</Text>
          </View>
          <View style={{ padding: 12, gap: 8 }}>{children}</View>
        </ScrollView>
      </View>
    </View>
  </Modal>;
  const height = content ? surfaceHeight(content.height, window.height) : null;
  const place = trigger && content && height !== null ? placeAbove(trigger, { width: content.width, height }, window) : null;
  return <Modal transparent visible onRequestClose={onClose}>
    <Pressable testID="host-popover-backdrop" accessibilityRole="button" accessibilityLabel="Menu backdrop" onPress={onClose} style={{ position: "absolute", top: 0, right: 0, bottom: 0, left: 0 }} />
    <View testID="host-popover" style={{ position: "absolute", left: place?.x ?? -9999, top: place?.y ?? -9999, minWidth: MIN_WIDTH, maxWidth: MAX_WIDTH,
      backgroundColor: colors.surface1, borderWidth: 1, borderColor: BORDER_ACCENT, borderRadius: 8, overflow: "hidden", boxShadow: SHADOW }}>
      <ScrollView keyboardShouldPersistTaps="handled" style={height === null ? undefined : { height }} contentContainerStyle={{ flexGrow: 1 }}>
        <View onLayout={(event: { nativeEvent: { layout: Size } }) => { const { width, height: next } = event.nativeEvent.layout; setContent((current) => current?.width === width && current.height === next ? current : { width, height: next }); }}
          style={{ padding: 12, gap: 8 }}>{children}</View>
      </ScrollView>
    </View>
  </Modal>;
}
