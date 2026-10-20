import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0, effects: [] as (() => void | (() => void))[] }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useState: <T,>(initial: T) => {
    const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
    return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
  },
  useRef: <T,>(initial: T) => {
    const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = { current: initial };
    return hooks.values[index];
  },
  useEffect: (effect: () => void | (() => void)) => { hooks.effects.push(effect); },
}));
vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, View: "View", Text: "Text", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { KeyboardSettingsCard, testMessage } from "../client/keyboard-settings-card";
import type { ShortcutEvent } from "../client/keyboard-shortcuts";
import { Action } from "../client/page-controls";
import type { KeyboardSettingsState } from "../client/use-keyboard-settings";

type Element = ReactElement<Record<string, unknown>>;
type Listener = (event: ShortcutEvent) => void;
const BACK = "Cmd+` (Cmd+< on Norwegian)";
const NATIVE = "Keyboard shortcuts work in the web and desktop apps only.";

function walk(node: ReactNode, expand: (element: Element) => boolean): Element[] {
  if (Array.isArray(node)) return node.flatMap((child) => walk(child, expand));
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function" && expand(node)) return walk((node.type as (props: Record<string, unknown>) => ReactNode)(node.props), expand);
  return [node, ...walk(node.props.children as ReactNode, expand)];
}
function visible(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(visible).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return visible((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return visible(node.props.children as ReactNode);
}
function state(status: KeyboardSettingsState["status"], extra: Record<string, unknown> = {}): KeyboardSettingsState {
  const base = { saving: false, saveError: null, save: vi.fn(async () => true), reset: vi.fn(async () => true), reload: vi.fn(async () => {}) };
  const shape = status === "loading" ? { status } : status === "error" ? { status, error: "settings RPC failed" } : status === "invalid" ? { status, error: "backShortcut: invalid enum", revision: "rev-bad" } : { status, values: { backShortcut: "off" }, revision: "rev-a" };
  return { ...base, ...shape, ...extra } as KeyboardSettingsState;
}
const listeners: { listener: Listener; capture: boolean }[] = [];
const page = {
  activeElement: null, body: {},
  addEventListener: (_type: string, listener: Listener, capture: boolean) => { listeners.push({ listener, capture }); },
  removeEventListener: (_type: string, listener: Listener, capture: boolean) => { const index = listeners.findIndex((entry) => entry.listener === listener && entry.capture === capture); if (index >= 0) listeners.splice(index, 1); },
};
const press = (key: Partial<ShortcutEvent>) => { const event = { key: "`", code: "Backquote", metaKey: true, preventDefault: vi.fn(), ...key }; for (const { listener } of [...listeners]) listener(event); return event; };

function mount(initial: KeyboardSettingsState, web = true) {
  const props = { state: initial, save: vi.fn(async () => true), colors: {} as never, web };
  hooks.values = []; hooks.effects = [];
  let cleanups: (() => void)[] = [];
  const render = () => { hooks.cursor = 0; const tree = KeyboardSettingsCard(props); return tree; };
  // Effects run once, after the first draw, as on mount.
  render(); cleanups = hooks.effects.splice(0).map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");
  hooks.effects.length = 0;
  const actions = () => { hooks.effects.length = 0; return walk(render(), (element) => element.type !== Action).filter((element) => element.type === Action); };
  const action = (label: string) => { const found = actions().find((element) => element.props.label === label); if (!found) throw new Error(`Missing ${label}`); return found; };
  return {
    props, actions, action, text: () => { const text = visible(render()); hooks.effects.length = 0; return text; },
    press(label: string) { (action(label).props.onPress as () => void)(); },
    unmount() { cleanups.forEach((cleanup) => cleanup()); },
  };
}

describe("Keyboard settings card", () => {
  beforeEach(() => { vi.stubGlobal("document", page); listeners.length = 0; vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

  it("tells native clients and pages without a document that shortcuts are web and desktop only", () => {
    const native = mount(state("ready"), false);
    expect(native.text()).toBe(`Keyboard${NATIVE}`);
    expect(native.actions()).toEqual([]);
    vi.unstubAllGlobals();
    const bare = mount(state("ready"));
    expect(bare.text()).toBe(`Keyboard${NATIVE}`);
  });

  it("offers Off and Cmd+` with the saved one selected, saves a choice and names who shares it", () => {
    const card = mount(state("ready"));
    const text = card.text();
    expect(text).toContain("Back shortcut: goes back one Kontor view, like the Back button.");
    expect(text).toContain("Shared by every client of this host.");
    expect(text).toContain("System Settings › Keyboard › Keyboard Shortcuts › Keyboard");
    expect(text).not.toContain(NATIVE);
    expect(card.actions().map((element) => [element.props.label, element.props.selected, element.props.disabled])).toEqual([
      ["Off", true, false], [BACK, false, false], ["Test shortcut", undefined, false],
    ]);
    card.press(BACK);
    expect(card.props.save).toHaveBeenCalledExactlyOnceWith({ backShortcut: "cmd-backquote" });
    const chosen = mount(state("ready", { values: { backShortcut: "cmd-backquote" } }));
    expect(chosen.action(BACK).props.selected).toBe(true);
    expect(chosen.action("Off").props.selected).toBe(false);
  });

  it.each([
    ["loading", state("loading"), "Reading the keyboard setting…"],
    ["unavailable", state("error"), "Keyboard setting unavailable: settings RPC failed"],
    ["invalid", state("invalid"), "The saved keyboard setting is invalid and not applied: backShortcut: invalid enum"],
    ["saving", state("ready", { saving: true }), "Saving…"],
  ] as const)("shows a %s setting and offers no choice it cannot save", (_name, current, note) => {
    const card = mount(current);
    expect(card.text()).toContain(note);
    for (const label of ["Off", BACK]) {
      expect(card.action(label).props.disabled).toBe(true);
      expect(card.action(label).props.selected).toBe(current.status === "ready" && label === "Off");
    }
    // Testing never writes, so it stays available; only an invalid document can be reset.
    expect(card.action("Test shortcut").props.disabled).toBe(false);
    expect(card.actions().some((element) => element.props.label === "Reset the keyboard setting")).toBe(current.status === "invalid");
  });

  it("offers Reset only for an invalid setting, not while saving", () => {
    expect(mount(state("ready")).actions().some((element) => element.props.label === "Reset the keyboard setting")).toBe(false);
    const current = state("invalid");
    const card = mount(current);
    expect(card.action("Reset the keyboard setting").props.disabled).toBe(false);
    card.press("Reset the keyboard setting");
    expect(current.reset).toHaveBeenCalledTimes(1);
    expect(mount(state("invalid", { saving: true })).action("Reset the keyboard setting").props.disabled).toBe(true);
  });

  it("keeps a failed save visible beside the choices", () => {
    expect(mount(state("ready", { saveError: "revision conflict" })).text()).toContain("Not saved: revision conflict");
    expect(mount(state("ready", { saveError: "denied", saving: true })).text()).toContain("Not saved: denied · Saving…");
  });

  it("listens for one key on Test shortcut and says what reached Kontor", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    expect(listeners.map((entry) => entry.capture)).toEqual([true]);
    const waiting = card.action("Listening for 10 seconds: press the shortcut");
    expect(waiting.props.disabled).toBe(true);
    const key = press({});
    expect(key.preventDefault).toHaveBeenCalledTimes(1);
    expect(listeners).toEqual([]);
    expect(card.action("Test shortcut").props.disabled).toBe(false);
    expect(card.text()).toContain("Kontor received Cmd+` (key position Backquote). The Back shortcut reaches Kontor here.");
    // A second test clears the last result until a key arrives.
    card.press("Test shortcut");
    expect(card.text()).not.toContain("Kontor received");
    vi.advanceTimersByTime(10_000);
    expect(card.text()).toContain("No key reached Kontor within 10 seconds.");
  });

  it("stops a running test when the card goes away", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    card.text();
    expect(listeners.length).toBe(1);
    card.unmount();
    expect(listeners).toEqual([]);
    vi.advanceTimersByTime(10_000);
    expect(card.text()).not.toContain("No key reached Kontor");
  });
});

describe("Test shortcut wording", () => {
  it("names what reached Kontor, or that nothing or only modifiers did", () => {
    expect(testMessage({ kind: "nothing" })).toBe("No key reached Kontor within 10 seconds.");
    expect(testMessage({ kind: "modifiers", label: "Cmd" })).toBe("Only Cmd reached Kontor. macOS or the app took the rest of the key first.");
    expect(testMessage({ kind: "key", label: "Cmd+<", code: "Backquote", back: true })).toBe("Kontor received Cmd+< (key position Backquote). The Back shortcut reaches Kontor here.");
    expect(testMessage({ kind: "key", label: "A", code: "KeyA", back: false })).toBe("Kontor received A (key position KeyA). That is not the Back shortcut.");
    expect(testMessage({ kind: "key", label: "A", code: "", back: false })).toBe("Kontor received A. That is not the Back shortcut.");
  });
});
