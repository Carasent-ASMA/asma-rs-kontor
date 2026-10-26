/**
 * Fixture stand-in for the host-supplied `@getpaseo/plugin/client/react-native`.
 * Paseo resolves icons from lucide-react-native by PascalCase name; the web
 * build draws the same lucide geometry with lucide-react. Toasts, clipboard and
 * the modal are host chrome, so they are approximations, not Paseo pixels.
 */
import { forwardRef, useSyncExternalStore, type ReactNode } from "react";
import * as Lucide from "lucide-react";
import { FlatList as WebFlatList, Modal as WebModal, Pressable, ScrollView as WebScrollView, Text, TextInput as WebTextInput, useWindowDimensions, View, type StyleProp, type ViewStyle } from "react-native-web";
import { shellColors } from "./theme";
import { logHostEvent } from "./event-log";

export function Icon({ name, size = 16, color }: { name: string; size?: number; color?: string }) {
  // Canonical names first, then lucide's exported aliases (Building2 is an alias of Building in 1.50).
  const Glyph = (Lucide.icons as Record<string, typeof Lucide.Building>)[name] ?? (Lucide as unknown as Record<string, typeof Lucide.Building | undefined>)[name];
  if (!Glyph) return <View style={{ width: size, height: size }} />;
  return <Glyph size={size} color={color} strokeWidth={2} aria-hidden />;
}

type ModalProps = { title: string; icon?: ReactNode; open: boolean; onOpenChange(open: boolean): void; children: ReactNode };
/**
 * Host chrome approximation: a bottom sheet on a phone-width window and a centred dialog above it, in the shell's
 * theme, with the title and a close X in its header. Paseo draws its own; these are not its pixels.
 */
function ModalImpl({ title, open, onOpenChange, children }: ModalProps) {
  const colors = shellColors(), { width, height } = useWindowDimensions(), sheet = width < 600;
  return <WebModal transparent visible={open} onRequestClose={() => onOpenChange(false)}>
    <View style={{ flex: 1, backgroundColor: "rgba(0,0,0,0.45)", justifyContent: sheet ? "flex-end" : "center", alignItems: "center" }}>
      <View role="dialog" aria-label={title} style={{ width: sheet ? "100%" : 520, maxHeight: height * 0.92, backgroundColor: colors.surface0, borderColor: colors.border, borderWidth: 1,
        borderTopLeftRadius: 14, borderTopRightRadius: 14, borderBottomLeftRadius: sheet ? 0 : 14, borderBottomRightRadius: sheet ? 0 : 14, overflow: "hidden" }}>
        {sheet ? <View style={{ alignSelf: "center", width: 36, height: 4, borderRadius: 2, marginTop: 6, backgroundColor: colors.border }} /> : null}
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingLeft: 16, paddingRight: 4, minHeight: 48 }}>
          <Text accessibilityRole="header" numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 17, fontWeight: "600" }}>{title}</Text>
          <Pressable accessibilityRole="button" accessibilityLabel="Close" onPress={() => onOpenChange(false)}
            style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ width: 44, height: 44, alignItems: "center", justifyContent: "center", borderRadius: 8, borderWidth: 2, borderColor: focused ? colors.accent : "transparent", outlineStyle: "solid", outlineWidth: 0 })}>
            <Icon name="X" size={20} color={colors.foreground} />
          </Pressable>
        </View>
        {children}
      </View>
    </View>
  </WebModal>;
}
type ContentProps = { children: ReactNode; style?: StyleProp<ViewStyle>; contentContainerStyle?: StyleProp<ViewStyle>; scrollable?: boolean };
function ModalContent({ children, style, contentContainerStyle, scrollable = true }: ContentProps) {
  const padded = [{ padding: 24, gap: 16 }, contentContainerStyle];
  return scrollable ? <WebScrollView style={[{ flexShrink: 1 }, style]} contentContainerStyle={padded}>{children}</WebScrollView>
    : <View style={[{ flexShrink: 1, minHeight: 0 }, style, ...padded]}>{children}</View>;
}
export const Modal = Object.assign(ModalImpl, { Content: ModalContent });

type Toast = { id: number; message: string; variant: string };
let toasts: Toast[] = [];
let toastId = 0;
const toastListeners = new Set<() => void>();
function pushToast(message: string, variant: string, durationMs = 3500) {
  toastId += 1;
  const id = toastId;
  toasts = [...toasts, { id, message, variant }];
  logHostEvent("toast", `${variant}: ${message}`);
  for (const listener of toastListeners) listener();
  setTimeout(() => { toasts = toasts.filter((toast) => toast.id !== id); for (const listener of toastListeners) listener(); }, durationMs);
}
const toastApi = { show: (message: string, options?: { variant?: string; durationMs?: number }) => pushToast(message, options?.variant ?? "default", options?.durationMs), error: (message: string) => pushToast(message, "error") };
export function useToast() { return toastApi; }

/** Host chrome: the toast stack the shell renders above the plugin screen. */
export function ToastLayer() {
  const current = useSyncExternalStore((listener) => { toastListeners.add(listener); return () => toastListeners.delete(listener); }, () => toasts);
  return <View pointerEvents="none" accessibilityLiveRegion="polite" style={{ position: "absolute", left: 0, right: 0, bottom: 72, alignItems: "center", gap: 6 }}>
    {current.map((toast) => <View key={toast.id} style={{ maxWidth: 420, paddingHorizontal: 14, paddingVertical: 10, borderRadius: 10, backgroundColor: toast.variant === "error" ? "#5b2a26" : "#272A29", borderWidth: 1, borderColor: "#3a3f3e" }}>
      <Text style={{ color: "#fafafa", fontSize: 13 }}>{toast.message}</Text>
    </View>)}
  </View>;
}

export function useRevealedText(text: string): string { return text; }
export const ScrollView = WebScrollView;
export const FlatList = WebFlatList;
export const TextInput = forwardRef<unknown, Record<string, unknown>>(function TextInput(props, ref) {
  return <WebTextInput ref={ref as never} {...props} />;
});

/** Logs the write; with `clipboard=denied` in the page URL it is refused, as a host whose clipboard refuses writes would. */
export async function copyText(text: string): Promise<void> {
  if (new URLSearchParams(globalThis.location?.search ?? "").get("clipboard") === "denied") {
    logHostEvent("copyText", `SIMULATED: the clipboard refused a write of ${text.length} characters`);
    throw new Error("SIMULATED: the clipboard refused the write");
  }
  logHostEvent("copyText", `${text.length} characters`);
}
