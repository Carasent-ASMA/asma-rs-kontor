import { useEffect, useRef, useState } from "react";
import { View } from "react-native";
import { BACK_SHORTCUTS, type BackShortcut, type KeyboardSettings } from "../shared/keyboard";
import { SHORTCUT_TEST_MS, shortcutPage, testShortcut, type ShortcutTestResult } from "./keyboard-shortcuts";
import { Action, Card, Copy, type Colors } from "./page-controls";
import type { KeyboardSettingsState } from "./use-keyboard-settings";

const CHOICES: Record<BackShortcut, string> = { off: "Off", "cmd-backquote": "Cmd+` (Cmd+< on Norwegian)" };
const MAC_NOTE = "macOS uses the same keys for “Move focus to next window”. If the test shows only Cmd, macOS took the key: change that shortcut in System Settings › Keyboard › Keyboard Shortcuts › Keyboard.";

/** What Test shortcut found, in words. */
export function testMessage(result: ShortcutTestResult): string {
  if (result.kind === "nothing") return `No key reached Kontor within ${SHORTCUT_TEST_MS / 1000} seconds.`;
  if (result.kind === "modifiers") return `Only ${result.label} reached Kontor. macOS or the app took the rest of the key first.`;
  const position = result.code ? ` (key position ${result.code})` : "";
  return result.back ? `Kontor received ${result.label}${position}. The Back shortcut reaches Kontor here.` : `Kontor received ${result.label}${position}. That is not the Back shortcut.`;
}

/** The Back shortcut choice and an in-Kontor test of what a key press delivers; native clients have no key events. */
export function KeyboardSettingsCard({ state, save, colors, web }: {
  state: KeyboardSettingsState; save(patch: Partial<KeyboardSettings>): Promise<boolean>; colors: Colors; web: boolean;
}) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const stop = useRef<(() => void) | null>(null);
  useEffect(() => () => { stop.current?.(); }, []);
  const page = web ? shortcutPage() : null;
  if (!page) return <Card title="Keyboard" colors={colors}><Copy colors={colors} muted small>Keyboard shortcuts work in the web and desktop apps only.</Copy></Card>;
  const current = state.status === "ready" ? state.values.backShortcut : null;
  const status = state.status === "loading" ? "Reading the keyboard setting…" : state.status === "invalid" ? `The saved keyboard setting is invalid and not applied: ${state.error}` : state.status === "error" ? `Keyboard setting unavailable: ${state.error}` : null;
  const note = [status, state.saveError ? `Not saved: ${state.saveError}` : null, state.saving ? "Saving…" : null].filter(Boolean).join(" · ");
  const test = () => {
    setResult(null); setTesting(true);
    stop.current = testShortcut(page, (found) => { stop.current = null; setTesting(false); setResult(testMessage(found)); });
  };
  return <Card title="Keyboard" colors={colors}>
    <Copy colors={colors}>Back shortcut: goes back one Kontor view, like the Back button. It acts while Kontor is the open screen, not while you type or while a menu is open.</Copy>
    <Copy colors={colors} muted small>Shared by every client of this host. {MAC_NOTE}</Copy>
    {note ? <Copy colors={colors}>{note}</Copy> : null}
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      {BACK_SHORTCUTS.map((choice) => <Action key={choice} colors={colors} label={CHOICES[choice]} selected={current === choice} disabled={state.status !== "ready" || state.saving} onPress={() => { void save({ backShortcut: choice }); }} />)}
      {state.status === "invalid" ? <Action colors={colors} label="Reset the keyboard setting" disabled={state.saving} onPress={() => { void state.reset(); }} /> : null}
    </View>
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      <Action colors={colors} label={testing ? `Listening for ${SHORTCUT_TEST_MS / 1000} seconds: press the shortcut` : "Test shortcut"} disabled={testing} onPress={test} />
    </View>
    {result ? <Copy colors={colors}>{result}</Copy> : null}
  </Card>;
}
