/** React Native Web review adapter; production never imports this module. */
import { createContext, createElement, forwardRef, useContext } from "react";
import { Linking as WebLinking, StyleSheet, Text as WebText, TextInput as WebTextInput, useWindowDimensions as webDimensions } from "react-native-web";
import type { Text as NativeText, TextProps, TextInput as NativeInput, TextInputProps } from "react-native";
import { logHostEvent } from "./event-log";

export * from "react-native-web";

const requested = Number(new URLSearchParams(globalThis.location?.search ?? "").get("textScale"));
export const simulatedTextScale = [1, 1.3, 1.5, 2].includes(requested) ? requested : 1;
type TextContext = { size: number; line?: number; allow: boolean; max?: number | null };
const TextParent = createContext<TextContext>({ size: 14, allow: true });

/** Inherited sizes remain unscaled so nested Text is never enlarged twice. */
export function simulatedTextLayout(style: { fontSize?: number; lineHeight?: number }, allow: boolean | undefined, max: number | null | undefined, inherited: TextContext, scale: number) {
  const next = { size: style.fontSize ?? inherited.size, line: style.lineHeight ?? inherited.line, allow: allow ?? inherited.allow, max: max ?? inherited.max };
  const factor = next.allow ? Math.min(scale, next.max && next.max > 0 ? next.max : Infinity) : 1;
  return { next, style: { fontSize: next.size * factor, ...(next.line !== undefined ? { lineHeight: next.line * factor } : {}) } };
}
export const Text = forwardRef<NativeText, TextProps>(function Text(props, ref) {
  const inherited = useContext(TextParent);
  if (simulatedTextScale === 1) return createElement(WebText, { ...props, ref });
  const layout = simulatedTextLayout(StyleSheet.flatten(props.style) ?? {}, props.allowFontScaling, props.maxFontSizeMultiplier, inherited, simulatedTextScale);
  return createElement(TextParent.Provider, { value: layout.next }, createElement(WebText, { ...props, ref, style: [props.style, layout.style] }));
});
export const TextInput = forwardRef<NativeInput, TextInputProps>(function TextInput(props, ref) {
  if (simulatedTextScale === 1) return createElement(WebTextInput, { ...props, ref });
  const layout = simulatedTextLayout(StyleSheet.flatten(props.style) ?? {}, props.allowFontScaling, props.maxFontSizeMultiplier, { size: 14, allow: true }, simulatedTextScale);
  return createElement(WebTextInput, { ...props, ref, style: [props.style, layout.style] });
});
export function useWindowDimensions() { return { ...webDimensions(), fontScale: simulatedTextScale }; }

export const Settings = undefined;
export const BackHandler = { addEventListener: () => ({ remove() {} }), exitApp() {} };
export const Linking = { ...WebLinking, openURL: async (url: string) => { logHostEvent("linking.openURL", url); }, canOpenURL: async () => true };
