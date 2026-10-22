import { Platform } from "react-native";

type EscapeKey = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyDocument = {
  addEventListener(type: "keydown", listener: (event: EscapeKey) => void): void;
  removeEventListener(type: "keydown", listener: (event: EscapeKey) => void): void;
};

/** Web Escape complements native Back; a nested disclosure consumes it first. */
export function onEscape(close: () => void): () => void {
  if (Platform.OS !== "web") return () => {};
  const document = (globalThis as { document?: KeyDocument }).document;
  const listener = (event: EscapeKey) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault(); event.stopPropagation(); close();
  };
  document?.addEventListener("keydown", listener);
  return () => document?.removeEventListener("keydown", listener);
}
