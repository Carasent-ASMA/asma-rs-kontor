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
import type { PageEvent, ShortcutEvent } from "../client/keyboard-shortcuts";
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
const listeners: { type: string; listener: Listener; capture: boolean }[] = [];
// Kontor's screen root fills the page; `hostField` is a field outside it.
const inside = { tagName: "DIV" }, hostField = { tagName: "INPUT" };
const screenRoot = { contains: (node: unknown) => node === inside, getClientRects: () => [{}], getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }) };
const page = {
  activeElement: null as unknown, body: {}, elementFromPoint: () => inside,
  addEventListener: (type: string, listener: Listener, capture: boolean) => { listeners.push({ type, listener, capture }); },
  removeEventListener: (type: string, listener: Listener, capture: boolean) => { const index = listeners.findIndex((entry) => entry.type === type && entry.listener === listener && entry.capture === capture); if (index >= 0) listeners.splice(index, 1); },
};
const fire = (type: string, event: PageEvent) => { for (const entry of [...listeners]) if (entry.type === type) entry.listener(event as ShortcutEvent); };
const press = (key: Partial<ShortcutEvent>) => { const event = { key: "`", code: "Backquote", metaKey: true, preventDefault: vi.fn(), ...key }; fire("keydown", event); return event; };

function mount(initial: KeyboardSettingsState, web = true) {
  const props = { state: initial, save: vi.fn(async () => true), colors: {} as never, web, root: { current: screenRoot }, copy: vi.fn() };
  hooks.values = []; hooks.effects = [];
  let cleanups: (() => void)[] = [];
  const render = () => { hooks.cursor = 0; const tree = KeyboardSettingsCard(props); return tree; };
  // Effects run once, after the first draw, as on mount.
  render(); cleanups = hooks.effects.splice(0).map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");
  hooks.effects.length = 0;
  const actions = () => { hooks.effects.length = 0; return walk(render(), (element) => element.type !== Action).filter((element) => element.type === Action); };
  const action = (label: string) => { const found = actions().find((element) => element.props.label === label); if (!found) throw new Error(`Missing ${label}`); return found; };
  const live = () => { const region = walk(render(), (element) => element.type !== Action).find((element) => element.props.accessibilityLiveRegion === "polite"); hooks.effects.length = 0; return region ? visible(region.props.children as ReactNode) : null; };
  return {
    props, actions, action, live, text: () => { const text = visible(render()); hooks.effects.length = 0; return text; },
    press(label: string) { (action(label).props.onPress as () => void)(); },
    unmount() { cleanups.forEach((cleanup) => cleanup()); },
  };
}

describe("Keyboard settings card", () => {
  beforeEach(() => { vi.stubGlobal("document", page); listeners.length = 0; page.activeElement = null; vi.useFakeTimers(); });
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
    expect(text).toContain("macOS can use Cmd+` for “Move focus to next window” (System Settings › Keyboard › Keyboard Shortcuts › Keyboard), so it may take the key before Kontor sees it.");
    expect(text).not.toContain(NATIVE);
    expect(card.actions().map((element) => [element.props.label, element.props.selected, element.props.disabled])).toEqual([
      ["Off", true, false], [BACK, false, false], ["Test shortcut", undefined, undefined],
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
    expect(card.action("Test shortcut").props.disabled ?? false).toBe(false);
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

  it("listens for one key on Test shortcut, says what reached Kontor, lets the result be copied and writes no setting", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    expect(listeners.map((entry) => [entry.type, entry.capture])).toEqual([["keydown", true], ["focusin", true], ["pointerdown", true], ["visibilitychange", false]]);
    // The same first button cancels, and there is no result to copy yet.
    expect(card.actions().map((element) => element.props.label)).toEqual(["Off", BACK, "Cancel test"]);
    expect(card.action("Cancel test").props.disabled ?? false).toBe(false);
    expect(card.live()).toBe("Listening for 10 seconds: press Cmd+` (Cmd+< on Norwegian) once, here in Kontor. Leaving Kontor stops the test.");
    const key = press({});
    expect(key.preventDefault).toHaveBeenCalledTimes(1);
    expect(listeners).toEqual([]);
    const received = "Kontor received Cmd+` (reported key position Backquote). That matches the Back shortcut.";
    expect(card.actions().map((element) => element.props.label)).toEqual(["Off", BACK, "Test shortcut", "Copy result"]);
    expect(card.live()).toBe(received);
    card.press("Copy result");
    expect(card.props.copy).toHaveBeenCalledExactlyOnceWith(received, "Test result");
    // A second test clears the last result, so there is nothing stale to copy, even once it is cancelled.
    card.press("Test shortcut");
    expect(card.text()).not.toContain("Kontor received");
    expect(card.actions().map((element) => element.props.label)).toEqual(["Off", BACK, "Cancel test"]);
    card.press("Cancel test");
    expect(card.actions().map((element) => element.props.label)).toEqual(["Off", BACK, "Test shortcut"]);
    expect(card.live()).toBe("");
    card.press("Test shortcut");
    vi.advanceTimersByTime(10_000);
    expect(card.live()).toContain("No key pressed with Cmd reached Kontor within 10 seconds.");
    expect(card.props.save).not.toHaveBeenCalled();
  });

  it("cancels a running test without a result", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    card.press("Cancel test");
    expect(listeners).toEqual([]);
    expect(card.actions().map((element) => element.props.label)).toEqual(["Off", BACK, "Test shortcut"]);
    expect(card.live()).toBe("");
    vi.advanceTimersByTime(10_000);
    expect(card.live()).toBe("");
    expect(press({}).preventDefault).not.toHaveBeenCalled();
  });

  it("stops when focus moves to a host field, and leaves a letter typed there alone", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    page.activeElement = hostField;
    fire("focusin", { target: hostField });
    expect(listeners).toEqual([]);
    expect(card.live()).toBe("The test stopped: focus or a click moved outside Kontor.");
    const letter = press({ key: "a", code: "KeyA", metaKey: false, target: hostField });
    expect(letter.preventDefault).not.toHaveBeenCalled();
    expect(card.live()).toBe("The test stopped: focus or a click moved outside Kontor.");
  });

  it("stops a running test when the card goes away", () => {
    const card = mount(state("ready"));
    card.press("Test shortcut");
    card.text();
    expect(listeners.length).toBe(4);
    card.unmount();
    expect(listeners).toEqual([]);
    vi.advanceTimersByTime(10_000);
    expect(card.text()).not.toContain("No key pressed with Cmd");
  });
});

describe("Test shortcut wording", () => {
  const unknown = "The test cannot tell why: focus may have been elsewhere, the key may not have been pressed fully, or macOS, the browser or the host may have taken it.";
  it("says what reached Kontor and names no cause it cannot see", () => {
    expect(testMessage({ kind: "nothing" })).toBe(`No key pressed with Cmd reached Kontor within 10 seconds. ${unknown}`);
    expect(testMessage({ kind: "cmd" })).toBe(`Only Cmd reached Kontor within 10 seconds, with no key pressed with it. ${unknown}`);
    expect(testMessage({ kind: "key", label: "Cmd+<", code: "Backquote", back: true })).toBe("Kontor received Cmd+< (reported key position Backquote). That matches the Back shortcut.");
    expect(testMessage({ kind: "key", label: "Cmd+A", code: "KeyA", back: false })).toBe("Kontor received Cmd+A (reported key position KeyA). That does not match the Back shortcut.");
    expect(testMessage({ kind: "key", label: "Cmd+A", code: "", back: false })).toBe("Kontor received Cmd+A. That does not match the Back shortcut.");
  });

  it.each([
    ["focus", "focus or a click moved outside Kontor"],
    ["front", "Kontor was no longer the screen in front"],
    ["window", "Kontor's window lost focus"],
    ["hidden", "the page was hidden"],
  ] as const)("says the test stopped when %s, and whether Cmd had reached Kontor", (reason, words) => {
    expect(testMessage({ kind: "left", reason, cmd: false })).toBe(`The test stopped: ${words}.`);
    expect(testMessage({ kind: "left", reason, cmd: true })).toBe(`The test stopped: ${words}. Only Cmd had reached Kontor.`);
  });
});
