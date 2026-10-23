import type { PluginTheme } from "@getpaseo/plugin";
import { afterEach, describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => ({ effects: [] as (() => void | (() => void))[], focus: vi.fn(), nativeFocus: vi.fn(), native: false, back: vi.fn(), removeBack: vi.fn() }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useRef: () => ({ current: { focus: hooks.focus } }),
  useEffect: (effect: () => void | (() => void)) => hooks.effects.push(effect),
}));
vi.mock("react-native", () => ({
  AccessibilityInfo: { setAccessibilityFocus: hooks.nativeFocus },
  findNodeHandle: () => 42,
  Platform: { get OS() { return hooks.native ? "android" : "web"; } },
  BackHandler: { addEventListener: hooks.back },
  Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));

import { focusControl, MobileFiltersPage, MobileFiltersTrigger } from "../client/mobile-filters";

type Key = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyListener = (event: Key) => void;
const colors = {} as PluginTheme["colors"];

afterEach(() => { hooks.effects = []; hooks.native = false; vi.clearAllMocks(); vi.unstubAllGlobals(); });

function mount(menuOpen = false) {
  const onBack = vi.fn();
  const addEventListener = vi.fn<(type: string, listener: KeyListener) => void>();
  const removeEventListener = vi.fn();
  vi.stubGlobal("document", { addEventListener, removeEventListener });
  hooks.back.mockReturnValue({ remove: hooks.removeBack });
  const tree = MobileFiltersPage({ colors, onBack, menuOpen, children: "Controls" });
  const cleanups = hooks.effects.map((effect) => effect()).filter((value): value is () => void => typeof value === "function");
  return { tree, onBack, addEventListener, removeEventListener, unmount: () => cleanups.forEach((cleanup) => cleanup()) };
}

describe("mobile Filters navigation", () => {
  it("focuses the visible Back, closes on Escape/Android Back, and releases every listener on leaving", () => {
    const page = mount();
    expect(hooks.focus).toHaveBeenCalledOnce();
    const listener = page.addEventListener.mock.calls[0]![1];
    const event = { key: "Escape", preventDefault: vi.fn(), stopPropagation: vi.fn() };
    listener({ ...event, key: "Enter" });
    listener({ ...event, defaultPrevented: true });
    expect(page.onBack).not.toHaveBeenCalled();
    listener(event);
    expect(page.onBack).toHaveBeenCalledOnce();
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(event.stopPropagation).toHaveBeenCalledOnce();
    const androidBack = hooks.back.mock.calls[0]![1] as () => boolean;
    expect(androidBack()).toBe(true);
    expect(page.onBack).toHaveBeenCalledTimes(2);
    page.unmount();
    expect(hooks.removeBack).toHaveBeenCalledOnce();
    expect(page.removeEventListener).toHaveBeenCalledWith("keydown", listener);
  });

  it("lets a nested select handle Escape/Android Back before the filter page", () => {
    const page = mount(true);
    expect(hooks.back).not.toHaveBeenCalled();
    expect(page.addEventListener).not.toHaveBeenCalled();
    expect(page.onBack).not.toHaveBeenCalled();
    page.unmount();
  });

  it("restores native screen-reader focus using the native node handle", () => {
    hooks.native = true;
    mount();
    expect(hooks.nativeFocus).toHaveBeenCalledWith(42);
    focusControl(null);
    expect(hooks.nativeFocus).toHaveBeenCalledTimes(1);
  });

  it("keeps the matching count beside the heading, outside the scroller, as one polite status that may wrap", () => {
    type Node = { props: { testID?: string; children: unknown; [key: string]: unknown } };
    const page = (count?: { text: string; label: string }) => MobileFiltersPage({ colors, onBack: () => {}, menuOpen: false, count, children: "Controls" }) as Node;
    const [header, scroller] = page({ text: "284 sessions", label: "284 matching sessions" }).props.children as [Node, Node];
    const [back, heading, count] = header.props.children as [Node, Node, Node];
    expect(back.props.label).toBe("Back");
    expect(header.props.style).toMatchObject({ flexDirection: "row", flexWrap: "wrap" });
    expect(heading.props).toMatchObject({ accessibilityRole: "header", children: "Filters & view", style: { flexShrink: 1 } });
    expect(count.props).toMatchObject({ testID: "sessions-mobile-filters-count", children: "284 sessions",
      accessibilityLabel: "284 matching sessions", accessibilityLiveRegion: "polite", style: { flexGrow: 1, flexShrink: 1, textAlign: "right" } });
    expect(count.props.numberOfLines).toBeUndefined();
    expect(scroller.props.testID).toBe("sessions-mobile-filters-scroll");
    expect(JSON.stringify(scroller.props.children)).not.toContain("284");
    expect(((page().props.children as Node[])[0]!.props.children as unknown[])[2]).toBeNull();
  });

  it("names the badge as active filters or view choices, with a 44pt touch target", () => {
    const tree = MobileFiltersTrigger({ colors, count: 2, onPress: () => {} });
    expect(tree.props.accessibilityLabel).toBe("Filters and view, 2 active filters or view choices");
    expect(tree.props.style).toMatchObject({ minHeight: 44, minWidth: 44 });
  });
});
