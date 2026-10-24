import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";

const effects: (() => void | (() => void))[] = [];
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useEffect: (effect: () => void | (() => void)) => { effects.push(effect); },
  useRef: (initial: unknown) => ({ current: initial }),
  useState: (initial: unknown) => [initial, () => {}],
}));
vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", ScrollView: "ScrollView" }));

import { AnchoredMenu, anchoredPanelOpen, type FocusTarget } from "../client/anchored-menu";
import { BulkArchiveDialog } from "../client/bulk-archive";
import { installBackShortcut, type ShortcutEvent } from "../client/keyboard-shortcuts";

type Key = { key: string; shiftKey?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean };
type Event = Key & { preventDefault: ReturnType<typeof vi.fn>; stopPropagation: ReturnType<typeof vi.fn> };
type Node = FocusTarget & { focus: Mock<() => void> };

const listeners = new Set<(event: Event) => void>();
const page = {
  activeElement: null as unknown,
  body: {},
  addEventListener: (_type: string, listener: (event: Event) => void) => { listeners.add(listener); },
  removeEventListener: (_type: string, listener: (event: Event) => void) => { listeners.delete(listener); },
};
const node = (): Node => ({ focus: vi.fn<() => void>(), getClientRects: () => [{}] });
/** A control in the host app, outside the plugin's screen; the page body is outside it too. */
const host = node();
function panelOf(stops: Node[]): Node {
  const panel: Node = { focus: vi.fn<() => void>(), getClientRects: () => [{}], querySelectorAll: () => stops, contains: (target) => target === panel || stops.includes(target as Node) };
  return panel;
}
const press = (key: Key): Event => {
  const event = { ...key, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  for (const listener of [...listeners]) listener(event);
  return event;
};
const mounted: (() => void)[] = [];

type Options = { panel: Node; dialog?: boolean; anchor?: Node; onClose?: () => void };
/** Each trigger keeps one ref across draws, as `useRef` does. */
const refs = new WeakMap<Node, { current: Node }>();
const refOf = (anchor: Node) => { let ref = refs.get(anchor); if (!ref) refs.set(anchor, ref = { current: anchor }); return ref; };
const menuOf = ({ dialog, anchor = node(), onClose = vi.fn() }: Options) => AnchoredMenu({ anchor: refOf(anchor) as never,
  root: { current: { contains: (target: unknown) => target !== page.body && target !== host, measureInWindow: () => {} } } as never, rootSize: { width: 800, height: 600 },
  placement: { align: "start", width: 200 }, onClose, colors: {} as never, label: "Choices", dialog, children: null }) as ReactElement<Record<string, unknown>>;

/** Draws the menu once, attaches its panel and runs its mount effects; the returned function unmounts it. */
function mount(options: Options) {
  effects.length = 0;
  const menu = menuOf(options);
  const tree = (menu.type as (props: Record<string, unknown>) => ReactElement<{ children: ReactElement<Record<string, unknown>>[] }>)(menu.props);
  const shown = tree.props.children[1]!;
  const { panel } = options;
  (shown.props.ref as { current: unknown }).current = panel;
  const cleanups = effects.map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");
  let done = false;
  const unmount = () => { if (!done) { done = true; cleanups.forEach((cleanup) => cleanup()); } };
  mounted.push(unmount);
  return { key: menu.key, panel: shown.props, backdrop: tree.props.children[0]!.props, unmount };
}
/** One conditional slot drawn again with other props: as React does, the same key keeps the mounted panel and its effects, and a new key mounts afresh. */
function slot() {
  let current: ReturnType<typeof mount> | null = null;
  return (options: Options) => {
    if (current?.key === menuOf(options).key) return current;
    current?.unmount();
    return current = mount(options);
  };
}

describe("anchored menu keys on web", () => {
  beforeEach(() => { vi.stubGlobal("document", page); page.activeElement = null; });
  afterEach(() => { mounted.splice(0).forEach((unmount) => unmount()); vi.unstubAllGlobals(); });

  it.each(["Close", "Escape", "backdrop"])("dismisses the actual bulk confirmation through %s without executing archive", (dismiss) => {
    effects.length = 0;
    const onClose = vi.fn(), run = vi.fn(), stop = vi.fn(), anchor = node();
    const dialog = BulkArchiveDialog({ lifecycle: { review: { rows: [{ id: "fixture-a", title: "A", outcome: "pending", eligible: true, children: [] }] }, run, stop } as never,
      root: { current: { measureInWindow: () => {} } } as never, anchor: refOf(anchor) as never, rootSize: { width: 390, height: 844 }, colors: {} as never,
      compact: true, onClose, onFinished: vi.fn() }) as ReactElement<Record<string, unknown>>;
    expect(dialog.props.anchor).toBe(refOf(anchor));
    const menu = AnchoredMenu(dialog.props as never) as ReactElement<Record<string, unknown>>;
    const tree = (menu.type as (props: Record<string, unknown>) => ReactElement<{ children: ReactElement<Record<string, unknown>>[] }>)(menu.props);
    const cleanups = effects.map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");
    mounted.push(() => cleanups.forEach((cleanup) => cleanup()));
    if (dismiss === "Escape") press({ key: "Escape" });
    else if (dismiss === "backdrop") (tree.props.children[0]!.props.onPress as () => void)();
    else {
      const header = dialog.props.header as ReactElement<{ children: ReactElement<Record<string, unknown>>[] }>;
      (header.props.children.find((child) => child.props.label === "Close")!.props.onPress as () => void)();
    }
    expect(onClose).toHaveBeenCalledTimes(1); expect(stop).toHaveBeenCalledTimes(1); expect(run).not.toHaveBeenCalled();
  });

  it("keeps Tab and Shift+Tab inside an open dialog, and still closes it with Escape or a press outside", () => {
    const [close, lines] = [node(), node()];
    const anchor = node();
    const onClose = vi.fn();
    const menu = mount({ panel: panelOf([close, lines]), dialog: true, anchor, onClose });
    expect(menu.panel).toMatchObject({ role: "dialog", "aria-modal": true, tabIndex: -1 });
    page.activeElement = lines;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect(close.focus).toHaveBeenCalledTimes(1);
    page.activeElement = close;
    expect(press({ key: "Tab", shiftKey: true }).preventDefault).toHaveBeenCalledTimes(1);
    expect(lines.focus).toHaveBeenCalledTimes(1);
    // Between stops, and with a modifier, the browser and the host keep the key.
    expect(press({ key: "Tab" }).preventDefault).not.toHaveBeenCalled();
    page.activeElement = lines;
    for (const modifier of [{ altKey: true }, { ctrlKey: true }, { metaKey: true }]) expect(press({ key: "Tab", ...modifier }).preventDefault).not.toHaveBeenCalled();
    expect([close.focus.mock.calls.length, lines.focus.mock.calls.length]).toEqual([1, 1]);
    // Focus behind the panel in the screen comes back in; focus in the host app is left alone.
    page.activeElement = anchor;
    expect(press({ key: "Tab", shiftKey: true }).preventDefault).toHaveBeenCalledTimes(1);
    page.activeElement = host;
    expect(press({ key: "Tab" }).preventDefault).not.toHaveBeenCalled();
    expect([close.focus.mock.calls.length, lines.focus.mock.calls.length]).toEqual([1, 2]);
    press({ key: "Escape" });
    (menu.backdrop.onPress as () => void)();
    expect([onClose.mock.calls.length, anchor.focus.mock.calls.length]).toEqual([2, 2]);
  });

  it("leaves Tab to the browser in a menu and keeps its arrow keys", () => {
    const items = [node(), node(), node()];
    const menu = mount({ panel: panelOf(items) });
    expect([menu.panel.role, menu.panel["aria-modal"], menu.panel.tabIndex]).toEqual(["menu", undefined, undefined]);
    page.activeElement = items[2];
    expect(press({ key: "Tab" }).preventDefault).not.toHaveBeenCalled();
    expect(items.map((item) => item.focus.mock.calls.length)).toEqual([0, 0, 0]);
    expect(press({ key: "ArrowDown" }).preventDefault).toHaveBeenCalledTimes(1);
    expect(items[0]!.focus).toHaveBeenCalledTimes(1);
    page.activeElement = items[0];
    press({ key: "ArrowUp" });
    press({ key: "End" });
    expect(items[2]!.focus).toHaveBeenCalledTimes(2);
  });

  it("lets only the top open dialog keep Tab, hands it back when that one closes, and lets go on unmount", () => {
    const [lower, upper] = [node(), node()];
    const below = mount({ panel: panelOf([lower]), dialog: true });
    const above = mount({ panel: panelOf([upper]), dialog: true });
    page.activeElement = page.body;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect([lower.focus.mock.calls.length, upper.focus.mock.calls.length]).toEqual([0, 1]);
    above.unmount();
    press({ key: "Tab" });
    expect([lower.focus.mock.calls.length, upper.focus.mock.calls.length]).toEqual([1, 1]);
    // Closing a lower dialog leaves the top one in charge.
    const top = node();
    mount({ panel: panelOf([top]), dialog: true });
    below.unmount();
    press({ key: "Tab", shiftKey: true });
    expect([lower.focus.mock.calls.length, top.focus.mock.calls.length]).toEqual([1, 1]);
    mounted.splice(0).forEach((unmount) => unmount());
    expect(listeners.size).toBe(0);
    expect(press({ key: "Tab" }).preventDefault).not.toHaveBeenCalled();
  });

  it("opens a dialog afresh when its slot swaps a menu for it, and a menu afresh the other way", () => {
    const draw = slot();
    const [more, picker, item, input, last, below] = [node(), node(), node(), node(), node(), node()];
    // A dialog open lower down (Details, say) keeps Tab while the slot holds a menu.
    mount({ panel: panelOf([below]), dialog: true });
    draw({ panel: panelOf([item]), anchor: more });
    page.activeElement = page.body;
    press({ key: "Tab" });
    expect([below.focus.mock.calls.length, listeners.size]).toEqual([1, 2]);
    // From the open menu, Tab reaches the picker behind it, which opens in the same slot and takes Tab over.
    const dialog = draw({ panel: panelOf([input, last]), dialog: true, anchor: picker });
    expect(dialog.panel).toMatchObject({ role: "dialog", "aria-modal": true });
    expect(listeners.size).toBe(2);
    page.activeElement = last;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect([input.focus.mock.calls.length, below.focus.mock.calls.length]).toEqual([1, 1]);
    press({ key: "Escape" });
    expect([picker.focus.mock.calls.length, more.focus.mock.calls.length]).toEqual([1, 0]);
    // Back to the menu: the lower dialog has Tab again, as under any menu, and one handler remains each.
    draw({ panel: panelOf([item]), anchor: more });
    expect(listeners.size).toBe(2);
    page.activeElement = item;
    press({ key: "Tab" });
    expect([below.focus.mock.calls.length, input.focus.mock.calls.length]).toEqual([2, 1]);
    press({ key: "Escape" });
    expect([more.focus.mock.calls.length, picker.focus.mock.calls.length]).toEqual([1, 1]);
  });

  it("opens afresh when one trigger's opening changes mode, or the slot moves to another trigger", () => {
    const draw = slot();
    const [trigger, other, item, stop] = [node(), node(), node(), node()];
    const menu = draw({ panel: panelOf([item]), anchor: trigger });
    // Drawn again as it was (the parent redraws while it is open), it stays the same opening.
    expect(draw({ panel: panelOf([item]), anchor: trigger })).toBe(menu);
    draw({ panel: panelOf([stop]), dialog: true, anchor: trigger });
    page.activeElement = page.body;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect(stop.focus).toHaveBeenCalledTimes(1);
    draw({ panel: panelOf([item]), anchor: trigger });
    // Group by open, then the workspace scope: Escape returns focus to the scope, not to Group by.
    draw({ panel: panelOf([item]), anchor: other });
    expect(listeners.size).toBe(1);
    press({ key: "Escape" });
    expect([other.focus.mock.calls.length, trigger.focus.mock.calls.length]).toEqual([1, 0]);
  });

  it("counts each open menu and dialog until it unmounts, so the Back shortcut leaves the key to them", () => {
    let shortcut: ((event: ShortcutEvent) => void) | null = null;
    const root = { contains: () => true, getClientRects: () => [{}], getBoundingClientRect: () => ({ left: 0, top: 0, width: 800, height: 600 }) };
    const back = vi.fn();
    installBackShortcut({ elementFromPoint: () => ({}), addEventListener: (_type, listener) => { shortcut = listener; }, removeEventListener: () => {} },
      { shortcut: "cmd-backquote", root: () => root, canGoBack: () => true, back });
    const backKey = () => { const event = { key: "`", code: "Backquote", metaKey: true, preventDefault: vi.fn() }; shortcut!(event); return event; };
    expect(anchoredPanelOpen()).toBe(false);
    const dialog = mount({ panel: panelOf([node()]), dialog: true });
    const menu = mount({ panel: panelOf([node()]) });
    expect(anchoredPanelOpen()).toBe(true);
    expect(backKey().preventDefault).not.toHaveBeenCalled();
    menu.unmount();
    expect(anchoredPanelOpen()).toBe(true);
    expect(backKey().preventDefault).not.toHaveBeenCalled();
    dialog.unmount();
    expect(anchoredPanelOpen()).toBe(false);
    expect(back).not.toHaveBeenCalled();
    expect(backKey().preventDefault).toHaveBeenCalledTimes(1);
    expect(back).toHaveBeenCalledTimes(1);
    // A slot that swaps one opening for another stays counted once.
    const draw = slot(), trigger = node(), other = node();
    draw({ panel: panelOf([node()]), anchor: trigger });
    draw({ panel: panelOf([node()]), dialog: true, anchor: other });
    expect(anchoredPanelOpen()).toBe(true);
    mounted.splice(0).forEach((unmount) => unmount());
    expect(anchoredPanelOpen()).toBe(false);
  });

  it("keeps Tab with a dialog when a menu opens over it", () => {
    const close = node();
    mount({ panel: panelOf([close]), dialog: true });
    mount({ panel: panelOf([node()]) });
    page.activeElement = page.body;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect(close.focus).toHaveBeenCalledTimes(1);
  });
});
