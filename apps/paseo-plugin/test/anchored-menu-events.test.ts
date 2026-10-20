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
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { AnchoredMenu, type FocusTarget } from "../client/anchored-menu";

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

/** Draws the menu once, attaches its panel and runs its mount effects; the returned function unmounts it. */
function mount({ panel, dialog, anchor = node(), onClose = vi.fn() }: { panel: Node; dialog?: boolean; anchor?: Node; onClose?: () => void }) {
  effects.length = 0;
  const tree = AnchoredMenu({ anchor: { current: anchor } as never, root: { current: { contains: (target: unknown) => target !== page.body && target !== host, measureInWindow: () => {} } } as never, rootSize: { width: 800, height: 600 },
    placement: { align: "start", width: 200 }, onClose, colors: {} as never, label: "Choices", dialog, children: null }) as ReactElement<{ children: ReactElement<Record<string, unknown>>[] }>;
  const shown = tree.props.children[1]!;
  (shown.props.ref as { current: unknown }).current = panel;
  const cleanups = effects.map((effect) => effect()).filter((cleanup): cleanup is () => void => typeof cleanup === "function");
  let done = false;
  const unmount = () => { if (!done) { done = true; cleanups.forEach((cleanup) => cleanup()); } };
  mounted.push(unmount);
  return { panel: shown.props, backdrop: tree.props.children[0]!.props, unmount };
}

describe("anchored menu keys on web", () => {
  beforeEach(() => { vi.stubGlobal("document", page); page.activeElement = null; });
  afterEach(() => { mounted.splice(0).forEach((unmount) => unmount()); vi.unstubAllGlobals(); });

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

  it("keeps Tab with a dialog when a menu opens over it", () => {
    const close = node();
    mount({ panel: panelOf([close]), dialog: true });
    mount({ panel: panelOf([node()]) });
    page.activeElement = page.body;
    expect(press({ key: "Tab" }).preventDefault).toHaveBeenCalledTimes(1);
    expect(close.focus).toHaveBeenCalledTimes(1);
  });
});
