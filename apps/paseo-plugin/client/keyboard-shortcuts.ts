import { useEffect, useRef } from "react";
import type { BackShortcut } from "../shared/keyboard";
import { anchoredPanelOpen } from "./anchored-menu";

/** A keydown as the web reports it; the plugin SDK has no keyboard contract, so Kontor reads DOM events. */
export type ShortcutEvent = {
  key: string; code?: string; metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean;
  repeat?: boolean; isComposing?: boolean; defaultPrevented?: boolean; target?: unknown; preventDefault(): void;
};
type Box = { left: number; top: number; width: number; height: number };
type ScreenRoot = { contains?(node: unknown): boolean; getClientRects?(): ArrayLike<unknown>; getBoundingClientRect?(): Box };
export type ShortcutPage = {
  activeElement?: unknown; body?: unknown; elementFromPoint?(x: number, y: number): unknown;
  addEventListener(type: "keydown", listener: (event: ShortcutEvent) => void, capture: boolean): void;
  removeEventListener(type: "keydown", listener: (event: ShortcutEvent) => void, capture: boolean): void;
};

/** How long Test shortcut waits for a key. */
export const SHORTCUT_TEST_MS = 10_000;
const MODIFIERS = new Set(["Meta", "Control", "Alt", "Shift", "OS", "AltGraph", "CapsLock", "Fn"]);

/** Cmd with the key at the Backquote position and no other modifier. The position, not the character, so a layout that types < there matches and Cmd+Shift+Comma does not. */
export function isBackShortcut(event: ShortcutEvent, shortcut: BackShortcut): boolean {
  return shortcut === "cmd-backquote" && event.code === "Backquote" && !!event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
}

/** A text field, select or editable region, where a key belongs to the typing. */
export function isEditable(target: unknown): boolean {
  const node = target as { tagName?: string; isContentEditable?: boolean } | null;
  return !!node && (node.isContentEditable === true || ["INPUT", "TEXTAREA", "SELECT"].includes(node.tagName ?? ""));
}

/** The modifiers and key of a keydown, as on a Mac keyboard: "Cmd+`". */
export function describeKey(event: ShortcutEvent): string {
  const keys = [event.metaKey ? "Cmd" : null, event.ctrlKey ? "Ctrl" : null, event.altKey ? "Option" : null, event.shiftKey ? "Shift" : null].filter(Boolean);
  const own = MODIFIERS.has(event.key) ? null : event.key === " " ? "Space" : event.key.length === 1 ? event.key.toLocaleUpperCase() : event.key;
  return [...keys, own].filter(Boolean).join("+") || event.key;
}

/** Kontor is the screen in front: drawn, with its middle not covered by another screen or a host overlay. The host's stack keeps it mounted under an opened session. */
function inFront(page: ShortcutPage, root: ScreenRoot | null): boolean {
  if (!root?.getBoundingClientRect || !page.elementFromPoint || !root.getClientRects?.().length) return false;
  const box = root.getBoundingClientRect();
  if (!box.width || !box.height) return false;
  return !!root.contains?.(page.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2));
}

export type BackShortcutOptions = { shortcut: BackShortcut; root(): unknown; canGoBack(): boolean; back(): void };

/**
 * Listens for the chosen Back shortcut on the page and goes back one Kontor view. It acts only while Kontor is in front
 * and has focus (or nothing has), never while typing, composing, repeating or with a Kontor menu or dialog open, and
 * leaves the key alone when there is nothing to go back to. It listens late (bubble phase), so the host's own shortcuts
 * and an open menu see the key first; a key they handled is skipped.
 */
export function installBackShortcut(page: ShortcutPage, options: BackShortcutOptions): () => void {
  const onKey = (event: ShortcutEvent) => {
    if (event.defaultPrevented || event.repeat || event.isComposing || !isBackShortcut(event, options.shortcut)) return;
    const root = options.root() as ScreenRoot | null;
    const active = page.activeElement;
    if (isEditable(event.target ?? active) || anchoredPanelOpen() || !inFront(page, root)) return;
    if (active != null && active !== page.body && !root?.contains?.(active)) return;
    if (!options.canGoBack()) return;
    event.preventDefault();
    options.back();
  };
  page.addEventListener("keydown", onKey, false);
  return () => page.removeEventListener("keydown", onKey, false);
}

/** The web page, when there is one; native has no DOM key events. */
export function shortcutPage(): ShortcutPage | null {
  return (globalThis as { document?: ShortcutPage }).document ?? null;
}

/** Installs the Back shortcut while the screen is mounted and one is chosen; the latest `back` and history are read at the key press. */
export function useBackShortcut(shortcut: BackShortcut, web: boolean, root: { readonly current: unknown }, canGoBack: () => boolean, back: () => void): void {
  const latest = useRef({ canGoBack, back });
  latest.current = { canGoBack, back };
  useEffect(() => {
    const page = shortcutPage();
    if (shortcut === "off" || !web || !page) return;
    return installBackShortcut(page, { shortcut, root: () => root.current, canGoBack: () => latest.current.canGoBack(), back: () => latest.current.back() });
  }, [shortcut, web]);
}

export type ShortcutTestResult =
  | { kind: "key"; label: string; code: string; back: boolean }
  | { kind: "modifiers"; label: string }
  | { kind: "nothing" };

/**
 * Waits once for a key and reports what reached Kontor, or that only modifiers or nothing did (macOS or the app may
 * take a key first). It listens early (capture phase) and claims the key it reports, so the Back shortcut does not
 * also act on it. The returned function stops waiting without a result.
 */
export function testShortcut(page: ShortcutPage, done: (result: ShortcutTestResult) => void, timeoutMs = SHORTCUT_TEST_MS): () => void {
  let modifiers: string | null = null;
  const stop = () => { clearTimeout(timer); page.removeEventListener("keydown", onKey, true); };
  const onKey = (event: ShortcutEvent) => {
    if (MODIFIERS.has(event.key)) { modifiers = describeKey(event); return; }
    event.preventDefault();
    stop();
    done({ kind: "key", label: describeKey(event), code: event.code ?? "", back: isBackShortcut(event, "cmd-backquote") });
  };
  const timer = setTimeout(() => { stop(); done(modifiers ? { kind: "modifiers", label: modifiers } : { kind: "nothing" }); }, timeoutMs);
  page.addEventListener("keydown", onKey, true);
  return stop;
}
