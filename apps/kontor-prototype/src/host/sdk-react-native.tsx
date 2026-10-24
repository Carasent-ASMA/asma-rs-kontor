/**
 * Fixture stand-in for the host-supplied `@getpaseo/plugin/client/react-native`.
 * Paseo resolves icons from lucide-react-native by PascalCase name; the web
 * build draws the same lucide geometry with lucide-react. Toasts, clipboard and
 * the modal are host chrome, so they are approximations, not Paseo pixels.
 */
import { forwardRef, useSyncExternalStore, type ReactNode } from "react";
import * as Lucide from "lucide-react";
import { FlatList as WebFlatList, Modal as WebModal, Pressable, ScrollView as WebScrollView, Text, TextInput as WebTextInput, View } from "react-native-web";
import { logHostEvent } from "./event-log";

export function Icon({ name, size = 16, color }: { name: string; size?: number; color?: string }) {
  // Canonical names first, then lucide's exported aliases (Building2 is an alias of Building in 1.50).
  const Glyph = (Lucide.icons as Record<string, typeof Lucide.Building>)[name] ?? (Lucide as unknown as Record<string, typeof Lucide.Building | undefined>)[name];
  if (!Glyph) return <View style={{ width: size, height: size }} />;
  return <Glyph size={size} color={color} strokeWidth={2} aria-hidden />;
}

type ModalProps = { title: string; icon?: ReactNode; open: boolean; onOpenChange(open: boolean): void; children: ReactNode };
function ModalImpl({ title, open, onOpenChange, children }: ModalProps) {
  return <WebModal transparent visible={open} onRequestClose={() => onOpenChange(false)}>
    <View style={{ flex: 1, backgroundColor: "rgba(0,0,0,0.4)", justifyContent: "center", alignItems: "center" }}>
      <View style={{ minWidth: 320, maxWidth: 560, backgroundColor: "#1E2120", borderRadius: 12, padding: 16, gap: 12 }}>
        <Text style={{ color: "#fafafa", fontWeight: "600" }}>{title}</Text>
        {children}
        <Pressable accessibilityRole="button" onPress={() => onOpenChange(false)}><Text style={{ color: "#A1A5A4" }}>Close</Text></Pressable>
      </View>
    </View>
  </WebModal>;
}
export const Modal = Object.assign(ModalImpl, { Content: ({ children }: { children: ReactNode }) => <View style={{ gap: 16 }}>{children}</View> });

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

export async function copyText(text: string): Promise<void> {
  logHostEvent("copyText", `${text.length} characters`);
}
