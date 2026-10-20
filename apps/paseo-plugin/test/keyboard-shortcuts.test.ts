import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hook = vi.hoisted(() => ({ ref: null as null | { current: unknown }, effects: [] as (() => void | (() => void))[] }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useRef: (initial: unknown) => (hook.ref ??= { current: initial }),
  useEffect: (effect: () => void | (() => void)) => { hook.effects.push(effect); },
}));
vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { describeKey, installBackShortcut, isBackShortcut, isEditable, SHORTCUT_TEST_MS, testShortcut, useBackShortcut, type PageEvent, type ShortcutEvent, type ShortcutPage } from "../client/keyboard-shortcuts";

type Listener = (event: ShortcutEvent) => void;
type Pressed = ShortcutEvent & { preventDefault: ReturnType<typeof vi.fn> };

/** A page with Kontor's screen root: 800×600 at the origin, drawn and in front unless told otherwise. */
function fakePage() {
  const listeners: { type: string; listener: Listener; capture: boolean }[] = [];
  const blurs: (() => void)[] = [];
  const inside = { tagName: "DIV" }, elsewhere = { tagName: "DIV" }, field = { tagName: "DIV", isContentEditable: true };
  const box = { left: 0, top: 0, width: 800, height: 600 };
  const root = { drawn: true, contains: (node: unknown) => node === root || node === inside || node === field, getClientRects: () => (root.drawn ? [{}] : []), getBoundingClientRect: () => box };
  const view = { covered: false };
  const page = {
    activeElement: null as unknown, body: { tagName: "BODY" }, visibilityState: "visible",
    elementFromPoint: vi.fn((_x: number, _y: number): unknown => (view.covered ? elsewhere : inside)),
    defaultView: {
      addEventListener: (_type: "blur", listener: () => void) => { blurs.push(listener); },
      removeEventListener: (_type: "blur", listener: () => void) => { const index = blurs.indexOf(listener); if (index >= 0) blurs.splice(index, 1); },
    },
    addEventListener: (type: string, listener: Listener, capture: boolean) => { listeners.push({ type, listener, capture }); },
    removeEventListener: (type: string, listener: Listener, capture: boolean) => {
      const index = listeners.findIndex((entry) => entry.type === type && entry.listener === listener && entry.capture === capture);
      if (index >= 0) listeners.splice(index, 1);
    },
  } satisfies ShortcutPage;
  /** Delivers an event as the page does: capture listeners first, then the bubble phase. */
  const fire = (type: string, event: PageEvent) => {
    for (const capture of [true, false]) for (const entry of [...listeners]) if (entry.type === type && entry.capture === capture) entry.listener(event as ShortcutEvent);
  };
  /** Cmd+` unless overridden; claiming it marks it handled for later listeners. */
  const press = (key: Partial<ShortcutEvent> = {}): Pressed => {
    const event = { key: "`", code: "Backquote", metaKey: true, ...key } as Pressed;
    event.preventDefault = vi.fn(() => { event.defaultPrevented = true; });
    fire("keydown", event);
    return event;
  };
  const blurWindow = () => { for (const listener of [...blurs]) listener(); };
  return { page, view, root, box, inside, elsewhere, field, listeners, blurs, fire, press, blurWindow };
}

describe("Back shortcut keys", () => {
  it("matches Cmd at the Backquote position only, whichever character the layout types there", () => {
    const key = (event: Partial<ShortcutEvent>) => ({ key: "`", code: "Backquote", metaKey: true, preventDefault() {}, ...event });
    expect(isBackShortcut(key({}), "cmd-backquote")).toBe(true);
    // A Norwegian layout types < at that position.
    expect(isBackShortcut(key({ key: "<" }), "cmd-backquote")).toBe(true);
    expect(isBackShortcut(key({}), "off")).toBe(false);
    expect(isBackShortcut(key({ metaKey: false }), "cmd-backquote")).toBe(false);
    for (const extra of [{ ctrlKey: true }, { altKey: true }, { shiftKey: true }]) expect(isBackShortcut(key(extra), "cmd-backquote")).toBe(false);
    // The same character at another position (US Cmd+Shift+Comma) is not the shortcut.
    expect(isBackShortcut(key({ key: "<", code: "Comma", shiftKey: true }), "cmd-backquote")).toBe(false);
    expect(isBackShortcut(key({ code: undefined }), "cmd-backquote")).toBe(false);
  });

  it("treats text fields, selects and editable regions as typing", () => {
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) expect(isEditable({ tagName })).toBe(true);
    expect(isEditable({ tagName: "DIV", isContentEditable: true })).toBe(true);
    expect(isEditable({ tagName: "DIV", isContentEditable: false })).toBe(false);
    expect(isEditable({ tagName: "BUTTON" })).toBe(false);
    expect(isEditable(null)).toBe(false);
    expect(isEditable(undefined)).toBe(false);
  });

  it("names a key press as a Mac keyboard does", () => {
    const named = (event: Partial<ShortcutEvent>) => describeKey({ key: "", preventDefault() {}, ...event });
    expect(named({ key: "`", metaKey: true })).toBe("Cmd+`");
    expect(named({ key: "a", metaKey: true, ctrlKey: true, altKey: true, shiftKey: true })).toBe("Cmd+Ctrl+Option+Shift+A");
    expect(named({ key: " ", ctrlKey: true })).toBe("Ctrl+Space");
    expect(named({ key: "ArrowLeft" })).toBe("ArrowLeft");
    // A modifier on its own is named once, not as "Cmd+Meta".
    expect(named({ key: "Meta", metaKey: true })).toBe("Cmd");
    expect(named({ key: "Alt", altKey: true, ctrlKey: true })).toBe("Ctrl+Option");
    // A modifier key without its flag still has a name.
    expect(named({ key: "Meta" })).toBe("Meta");
  });
});

describe("Back shortcut listener", () => {
  function install(shortcut: "off" | "cmd-backquote" = "cmd-backquote") {
    const world = fakePage();
    const options = { shortcut, root: vi.fn((): unknown => world.root), canGoBack: vi.fn(() => true), back: vi.fn() };
    const remove = installBackShortcut(world.page, options);
    return { ...world, options, remove };
  }
  /** The press claimed its key and went back, `backs` times in all. */
  const handled = (event: Pressed, back: ReturnType<typeof vi.fn>, backs = 1) => {
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(back).toHaveBeenCalledTimes(backs);
  };

  it("goes back once for Cmd+` while Kontor is in front, late in the bubble phase, and stops when removed", () => {
    const world = install();
    expect(world.listeners.map((entry) => entry.capture)).toEqual([false]);
    handled(world.press(), world.options.back);
    expect(world.page.elementFromPoint).toHaveBeenCalledWith(400, 300);
    handled(world.press({ key: "<" }), world.options.back, 2);
    world.page.activeElement = world.inside;
    world.press();
    expect(world.options.back).toHaveBeenCalledTimes(3);
    world.page.activeElement = world.page.body;
    world.press();
    expect(world.options.back).toHaveBeenCalledTimes(4);
    world.remove();
    expect(world.listeners).toEqual([]);
    const after = world.press();
    expect(after.preventDefault).not.toHaveBeenCalled();
    expect(world.options.back).toHaveBeenCalledTimes(4);
  });

  it("finds the box from the root's own position", () => {
    const world = install();
    Object.assign(world.box, { left: 100, top: 40, width: 200, height: 60 });
    world.press();
    expect(world.page.elementFromPoint).toHaveBeenLastCalledWith(200, 70);
  });

  it.each([
    ["the shortcut is off", (world: ReturnType<typeof install>) => world, { off: true }],
    ["the host or a menu already handled the key", (world: ReturnType<typeof install>) => world, { defaultPrevented: true }],
    ["the key repeats", (world: ReturnType<typeof install>) => world, { repeat: true }],
    ["an input method is composing", (world: ReturnType<typeof install>) => world, { isComposing: true }],
    ["another modifier is held", (world: ReturnType<typeof install>) => world, { shiftKey: true }],
    ["the key is typed into a field", (world: ReturnType<typeof install>) => world, { target: { tagName: "INPUT" } }],
    // Inside Kontor, so only the typing check can refuse it.
    ["focus is in an editable region of Kontor and the event names no target", (world: ReturnType<typeof install>) => { world.page.activeElement = world.field; return world; }, {}],
    ["another screen covers Kontor", (world: ReturnType<typeof install>) => { world.view.covered = true; return world; }, {}],
    ["Kontor is not drawn", (world: ReturnType<typeof install>) => { world.root.drawn = false; return world; }, {}],
    ["Kontor has no size", (world: ReturnType<typeof install>) => { world.box.width = 0; return world; }, {}],
    ["Kontor has no height", (world: ReturnType<typeof install>) => { world.box.height = 0; return world; }, {}],
    ["the screen root is gone", (world: ReturnType<typeof install>) => { world.options.root.mockReturnValue(null); return world; }, {}],
    ["focus is in the host outside Kontor", (world: ReturnType<typeof install>) => { world.page.activeElement = world.elsewhere; return world; }, {}],
    ["there is nothing to go back to", (world: ReturnType<typeof install>) => { world.options.canGoBack.mockReturnValue(false); return world; }, {}],
  ] as const)("leaves the key alone when %s", (_name, arrange, key) => {
    const { off, ...event } = key as Partial<ShortcutEvent> & { off?: boolean };
    const world = arrange(install(off ? "off" : "cmd-backquote"));
    const pressed = world.press(event);
    expect(pressed.preventDefault).not.toHaveBeenCalled();
    expect(world.options.back).not.toHaveBeenCalled();
  });

  it("reads focus from the event target first, so a field keeps its key even with focus reported on the body", () => {
    const world = install();
    world.page.activeElement = world.page.body;
    const typed = world.press({ target: { tagName: "TEXTAREA" } });
    expect(typed.preventDefault).not.toHaveBeenCalled();
    handled(world.press({ target: world.inside }), world.options.back);
  });

  it("asks for history only once every other check passed", () => {
    const world = install();
    world.view.covered = true;
    world.press();
    expect(world.options.canGoBack).not.toHaveBeenCalled();
  });
});

describe("Back shortcut hook", () => {
  beforeEach(() => { hook.ref = null; hook.effects = []; });
  afterEach(() => { vi.unstubAllGlobals(); });
  const run = () => hook.effects.splice(0).map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");

  it("installs only on web, with a page, and with a shortcut chosen", () => {
    const world = fakePage();
    vi.stubGlobal("document", world.page);
    for (const [shortcut, web] of [["off", true], ["cmd-backquote", false]] as const) {
      useBackShortcut(shortcut, web, { current: world.root }, () => true, vi.fn());
      expect(run()).toEqual([]);
      expect(world.listeners).toEqual([]);
    }
    vi.unstubAllGlobals();
    useBackShortcut("cmd-backquote", true, { current: world.root }, () => true, vi.fn());
    expect(run()).toEqual([]);
  });

  it("reads the latest Back, history and root at the key press, and removes its listener on cleanup", () => {
    const world = fakePage();
    vi.stubGlobal("document", world.page);
    const root: { current: unknown } = { current: null };
    const first = vi.fn(), second = vi.fn();
    useBackShortcut("cmd-backquote", true, root, () => true, first);
    const [cleanup] = run();
    expect(world.listeners.length).toBe(1);
    // The root ref is read at the press, after the screen attached it.
    root.current = world.root;
    // A later render hands new callbacks without installing again.
    useBackShortcut("cmd-backquote", true, root, () => true, second);
    hook.effects = [];
    world.press();
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    useBackShortcut("cmd-backquote", true, root, () => false, second);
    hook.effects = [];
    expect(world.press().preventDefault).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
    cleanup!();
    expect(world.listeners).toEqual([]);
  });
});

describe("Test shortcut", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });
  function start(timeoutMs?: number) {
    const world = fakePage();
    const done = vi.fn();
    const stop = testShortcut(world.page, () => world.root, done, timeoutMs);
    return { ...world, done, stop };
  }
  /** Every page and window listener the test added is gone. */
  const removed = (world: ReturnType<typeof start>) => { expect(world.listeners).toEqual([]); expect(world.blurs).toEqual([]); };

  it("waits early in the capture phase for one key with Cmd, claims the Back shortcut's own key and stops listening", () => {
    const world = start();
    expect(world.listeners.map((entry) => [entry.type, entry.capture])).toEqual([["keydown", true], ["focusin", true], ["pointerdown", true], ["visibilitychange", false]]);
    expect(world.blurs.length).toBe(1);
    const cmd = world.press({ key: "Meta", code: "MetaLeft" });
    expect(cmd.preventDefault).not.toHaveBeenCalled();
    expect(world.done).not.toHaveBeenCalled();
    const key = world.press();
    expect(key.preventDefault).toHaveBeenCalledTimes(1);
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "key", label: "Cmd+`", code: "Backquote", back: true });
    removed(world);
    vi.advanceTimersByTime(SHORTCUT_TEST_MS);
    expect(world.done).toHaveBeenCalledTimes(1);
  });

  it("reports another key with Cmd without claiming it, and says when the browser gave no position", () => {
    const world = start();
    const other = world.press({ key: "<", code: "Comma", shiftKey: true });
    expect(other.preventDefault).not.toHaveBeenCalled();
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "key", label: "Cmd+Shift+<", code: "Comma", back: false });
    const again = start();
    again.press({ key: "c", code: undefined });
    expect(again.done).toHaveBeenCalledExactlyOnceWith({ kind: "key", label: "Cmd+C", code: "", back: false });
  });

  it("goes on listening when focus or a click stays in Kontor, or the page stays visible", () => {
    const world = start();
    world.fire("focusin", { target: world.inside });
    world.fire("pointerdown", { target: world.field });
    world.fire("visibilitychange", {});
    expect(world.done).not.toHaveBeenCalled();
    expect(world.listeners.length).toBe(4);
  });

  it("takes a key with focus on the page body, as after a click on Kontor's background", () => {
    const world = start();
    world.page.activeElement = world.page.body;
    world.press();
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "key", label: "Cmd+`", code: "Backquote", back: true });
  });

  it.each([
    ["a key typed without Cmd", (_world: ReturnType<typeof start>) => {}, { key: "a", code: "KeyA", metaKey: false }],
    ["a repeat", (_world: ReturnType<typeof start>) => {}, { repeat: true }],
    ["an input method composing", (_world: ReturnType<typeof start>) => {}, { isComposing: true }],
    ["typing in a field", (_world: ReturnType<typeof start>) => {}, { target: { tagName: "INPUT" } }],
    ["typing in an editable region of Kontor that the event does not name", (world: ReturnType<typeof start>) => { world.page.activeElement = world.field; }, {}],
    ["another modifier held with Cmd", (_world: ReturnType<typeof start>) => {}, { key: "Shift", code: "ShiftLeft", shiftKey: true }],
  ] as const)("leaves %s alone and goes on listening", (_name, arrange, key) => {
    const world = start();
    arrange(world);
    const pressed = world.press(key);
    expect(pressed.preventDefault).not.toHaveBeenCalled();
    expect(world.done).not.toHaveBeenCalled();
    expect(world.listeners.length).toBe(4);
    vi.advanceTimersByTime(SHORTCUT_TEST_MS);
    // None of them counts as Cmd reaching Kontor.
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "nothing" });
  });

  it.each([
    ["focus moves outside Kontor", "focus", (world: ReturnType<typeof start>) => { world.fire("focusin", { target: world.elsewhere }); }],
    ["a click lands outside Kontor", "focus", (world: ReturnType<typeof start>) => { world.fire("pointerdown", { target: world.elsewhere }); }],
    ["a key arrives with focus outside Kontor", "focus", (world: ReturnType<typeof start>) => { world.page.activeElement = world.elsewhere; expect(world.press({ key: "a", metaKey: false }).preventDefault).not.toHaveBeenCalled(); }],
    ["a key arrives with Kontor covered", "front", (world: ReturnType<typeof start>) => { world.view.covered = true; expect(world.press().preventDefault).not.toHaveBeenCalled(); }],
    ["the time runs out with Kontor covered", "front", (world: ReturnType<typeof start>) => { world.view.covered = true; vi.advanceTimersByTime(SHORTCUT_TEST_MS); }],
    ["the window loses focus", "window", (world: ReturnType<typeof start>) => { world.blurWindow(); }],
    ["the page is hidden", "hidden", (world: ReturnType<typeof start>) => { world.page.visibilityState = "hidden"; world.fire("visibilitychange", {}); }],
  ] as const)("stops when %s, saying whether Cmd had reached Kontor", (_name, reason, leave) => {
    const world = start();
    world.press({ key: "Meta", code: "MetaLeft" });
    leave(world);
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "left", reason, cmd: true });
    removed(world);
    const bare = start();
    leave(bare);
    expect(bare.done).toHaveBeenCalledExactlyOnceWith({ kind: "left", reason, cmd: false });
  });

  it("reports Cmd alone when no key came with it", () => {
    const world = start(50);
    world.press({ key: "Meta", code: "MetaLeft" });
    vi.advanceTimersByTime(49);
    expect(world.done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "cmd" });
    removed(world);
  });

  it("reports nothing after ten seconds without a key", () => {
    const world = start();
    vi.advanceTimersByTime(SHORTCUT_TEST_MS - 1);
    expect(world.done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(world.done).toHaveBeenCalledExactlyOnceWith({ kind: "nothing" });
    expect(SHORTCUT_TEST_MS).toBe(10_000);
  });

  it("stops without a result", () => {
    const world = start();
    world.stop();
    removed(world);
    expect(world.press().preventDefault).not.toHaveBeenCalled();
    world.blurWindow();
    vi.advanceTimersByTime(SHORTCUT_TEST_MS);
    expect(world.done).not.toHaveBeenCalled();
  });

  it("keeps Back from acting on the press it reports, and Back acts again once the test is over", () => {
    const world = fakePage();
    const back = vi.fn(), done = vi.fn();
    installBackShortcut(world.page, { shortcut: "cmd-backquote", root: () => world.root, canGoBack: () => true, back });
    testShortcut(world.page, () => world.root, done);
    expect(world.press().preventDefault).toHaveBeenCalledTimes(1);
    expect(done).toHaveBeenCalledExactlyOnceWith({ kind: "key", label: "Cmd+`", code: "Backquote", back: true });
    expect(back).not.toHaveBeenCalled();
    world.press();
    expect(back).toHaveBeenCalledTimes(1);
  });
});
