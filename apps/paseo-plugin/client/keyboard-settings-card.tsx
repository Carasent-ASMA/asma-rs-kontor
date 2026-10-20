import { useEffect, useRef, useState } from "react";
import { View } from "react-native";
import { BACK_SHORTCUTS, type BackShortcut, type KeyboardSettings } from "../shared/keyboard";
import { SHORTCUT_TEST_MS, shortcutPage, testShortcut, type ShortcutTestExit, type ShortcutTestResult } from "./keyboard-shortcuts";
import { Action, Card, Copy, type Colors } from "./page-controls";
import type { KeyboardSettingsState } from "./use-keyboard-settings";

const CHOICES: Record<BackShortcut, string> = { off: "Off", "cmd-backquote": "Cmd+` (Cmd+< on Norwegian)" };
const MAC_NOTE = "macOS can use Cmd+` for “Move focus to next window” (System Settings › Keyboard › Keyboard Shortcuts › Keyboard), so it may take the key before Kontor sees it.";
const SECONDS = SHORTCUT_TEST_MS / 1000;
// What an empty result leaves open; the test only sees what reached Kontor.
const UNKNOWN = "The test cannot tell why: focus may have been elsewhere, the key may not have been pressed fully, or macOS, the browser or the host may have taken it.";
const EXITS: Record<ShortcutTestExit, string> = {
  focus: "focus or a click moved outside Kontor", front: "Kontor was no longer the screen in front", window: "Kontor's window lost focus", hidden: "the page was hidden",
};

/** What Test shortcut observed, in words; it names no cause it cannot see. */
export function testMessage(result: ShortcutTestResult): string {
  if (result.kind === "nothing") return `No key pressed with Cmd reached Kontor within ${SECONDS} seconds. ${UNKNOWN}`;
  if (result.kind === "cmd") return `Only Cmd reached Kontor within ${SECONDS} seconds, with no key pressed with it. ${UNKNOWN}`;
  if (result.kind === "left") return `The test stopped: ${EXITS[result.reason]}.${result.cmd ? " Only Cmd had reached Kontor." : ""}`;
  const position = result.code ? ` (reported key position ${result.code})` : "";
  return `Kontor received ${result.label}${position}. ${result.back ? "That matches the Back shortcut." : "That does not match the Back shortcut."}`;
}

/**
 * The Back shortcut choice and an in-Kontor test of what a key press delivers; native clients have no key events.
 * The test watches the Kontor screen under `root`, writes no setting, and its result can be copied.
 */
export function KeyboardSettingsCard({ state, save, colors, web, root, copy }: {
  state: KeyboardSettingsState; save(patch: Partial<KeyboardSettings>): Promise<boolean>; colors: Colors; web: boolean;
  root: { readonly current: unknown }; copy(text: string, what: string): void;
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
    stop.current = testShortcut(page, () => root.current, (found) => { stop.current = null; setTesting(false); setResult(testMessage(found)); });
  };
  const cancel = () => { stop.current?.(); stop.current = null; setTesting(false); };
  return <Card title="Keyboard" colors={colors}>
    <Copy colors={colors}>Back shortcut: goes back one Kontor view, like the Back button. It acts while Kontor is the open screen, not while you type or while a menu is open.</Copy>
    <Copy colors={colors} muted small>Shared by every client of this host. {MAC_NOTE}</Copy>
    {note ? <Copy colors={colors}>{note}</Copy> : null}
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      {BACK_SHORTCUTS.map((choice) => <Action key={choice} colors={colors} label={CHOICES[choice]} selected={current === choice} disabled={state.status !== "ready" || state.saving} onPress={() => { void save({ backShortcut: choice }); }} />)}
      {state.status === "invalid" ? <Action colors={colors} label="Reset the keyboard setting" disabled={state.saving} onPress={() => { void state.reset(); }} /> : null}
    </View>
    {/* One button for Test and Cancel, so focus stays on it while the test runs. */}
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      <Action colors={colors} label={testing ? "Cancel test" : "Test shortcut"} onPress={testing ? cancel : test} />
      {result ? <Action colors={colors} label="Copy result" onPress={() => copy(result, "Test result")} /> : null}
    </View>
    <View accessibilityLiveRegion="polite">
      {testing ? <Copy colors={colors}>Listening for {SECONDS} seconds: press {CHOICES["cmd-backquote"]} once, here in Kontor. Leaving Kontor stops the test.</Copy> : result ? <Copy colors={colors}>{result}</Copy> : null}
    </View>
  </Card>;
}
