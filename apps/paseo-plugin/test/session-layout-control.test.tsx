import { isValidElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginTheme } from "@getpaseo/plugin";

const fixture = vi.hoisted(() => ({ effects: [] as Array<() => void | (() => void)>, native: false, helpTexts: [] as string[], dismiss: vi.fn(), hold: vi.fn() }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useRef: (current: unknown) => ({ current }), useEffect: (effect: () => void | (() => void)) => fixture.effects.push(effect),
}));
vi.mock("react-native", () => ({ Platform: { get OS() { return fixture.native ? "ios" : "web"; } }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("../client/control-help", () => ({ useActionHelp: (text: string, ref: { current: unknown }) => { fixture.helpTexts.push(text); return { bindings: { ref: (node: unknown) => { ref.current = node; } }, dismiss: fixture.dismiss, hold: fixture.hold }; } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { SessionLayoutControl } from "../client/session-layout-control";

type Element = { props: Record<string, unknown> };
const walk = (node: ReactNode): Element[] => Array.isArray(node) ? node.flatMap(walk)
  : isValidElement<{ children?: ReactNode }>(node) ? [node, ...walk(node.props.children)] : [];
const colors = { foreground: "#fafafa", surface1: "#181b1a", surface2: "#272a29", accent: "#20744a", border: "#252b2a" } as PluginTheme["colors"];
afterEach(() => { fixture.effects = []; fixture.native = false; fixture.helpTexts = []; vi.clearAllMocks(); });

function mount(value: "grouped" | "flat" = "grouped", disabled?: string) {
  const onChange = vi.fn();
  const tree = SessionLayoutControl({ value, colors, onChange, disabled });
  const segments = walk(tree).filter((node) => node.props.accessibilityRole === "radio");
  const targets = segments.map((node) => {
    const target = { focus: vi.fn() }; (node.props.ref as (target: unknown) => void)(target); return target;
  });
  const root = { addEventListener: vi.fn(), removeEventListener: vi.fn() };
  (tree.props.ref as { current: unknown }).current = root;
  const cleanup = fixture.effects.map((effect) => effect());
  const key = (key: string, extra = {}) => {
    const event = { key, target: targets[value === "grouped" ? 0 : 1], preventDefault: vi.fn(), stopPropagation: vi.fn(), ...extra };
    root.addEventListener.mock.calls[0]![1](event); return event;
  };
  return { tree, segments, targets, root, onChange, key, cleanup };
}

describe("visible session layout segments", () => {
  it.each(["grouped", "flat"] as const)("labels both choices, checks only %s and exposes one Tab stop with a touch target and help", (value) => {
    const { tree, segments, onChange } = mount(value);
    expect(tree.props).toMatchObject({ accessibilityRole: "radiogroup", accessibilityLabel: "Session layout" });
    expect(segments.map((node) => node.props.accessibilityLabel)).toEqual(["Grouped", "Flat"]);
    expect(walk(tree).filter(node => typeof node.props.children === "string")).toEqual([]);
    expect(walk(tree).filter(node => node.props.name === "Check")).toHaveLength(1);
    expect(segments.map((node) => node.props["aria-checked"])).toEqual([value === "grouped", value === "flat"]);
    expect(segments.map((node) => node.props.accessibilityState)).toEqual([{ checked: value === "grouped" }, { checked: value === "flat" }]);
    expect(segments.map((node) => node.props.tabIndex)).toEqual(value === "grouped" ? [0, -1] : [-1, 0]);
    for (const node of segments) {
      const style = (node.props.style as (state: object) => Record<string, unknown>)({});
      expect(style.minHeight).toBeGreaterThanOrEqual(44); expect(style.minWidth).toBeGreaterThanOrEqual(44);
      expect(node.props.accessibilityHint).toContain("Search, filters and sort stay the same");
      expect((node.props.style as (state: object) => object)({ focused: true })).toMatchObject({ outlineColor: colors.foreground, outlineWidth: 2 });
    }
    (segments[value === "grouped" ? 0 : 1]!.props.onPress as () => void)();
    expect(onChange).not.toHaveBeenCalled();
    (segments[value === "grouped" ? 1 : 0]!.props.onPress as () => void)();
    expect(onChange).toHaveBeenCalledExactlyOnceWith(value === "grouped" ? "flat" : "grouped");
    expect(fixture.dismiss).toHaveBeenCalledTimes(2);
  });

  it("keeps showing the saved choice while a view ignores layout, and says why instead of changing it", () => {
    const why = "Recent lists sessions in the order you opened them.";
    const { tree, segments, onChange, key } = mount("grouped", why);
    expect(tree.props["aria-disabled"]).toBe(true);
    expect(segments.map((node) => node.props.accessibilityState)).toEqual([{ checked: true, disabled: true }, { checked: false, disabled: true }]);
    expect(segments.map((node) => node.props.accessibilityHint)).toEqual([why, why]);
    (segments[1]!.props.onPress as () => void)();
    expect(fixture.hold).toHaveBeenCalledOnce();
    expect(key("ArrowRight").preventDefault).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
    expect((segments[1]!.props.style as (state: object) => Record<string, unknown>)({}).opacity).toBe(0.5);
  });

  it.each(["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"])("%s selects and focuses the other choice, including wrapping", (key) => {
    for (const value of ["grouped", "flat"] as const) {
      fixture.effects = [];
      const control = mount(value), event = control.key(key);
      expect(control.onChange).toHaveBeenCalledExactlyOnceWith(value === "grouped" ? "flat" : "grouped");
      expect(control.targets[value === "grouped" ? 1 : 0]!.focus).toHaveBeenCalledOnce();
      expect(event.preventDefault).toHaveBeenCalledOnce(); expect(event.stopPropagation).toHaveBeenCalledOnce();
      control.cleanup.forEach((cleanup) => cleanup?.());
      expect(control.root.removeEventListener).toHaveBeenCalledWith("keydown", control.root.addEventListener.mock.calls[0]![1]);
    }
  });

  it("supports Home/End and Space, leaves Tab, Enter, host shortcuts and handled keys to their owners", () => {
    const control = mount();
    control.key("Home"); expect(control.onChange).not.toHaveBeenCalled();
    control.key("End"); expect(control.onChange).toHaveBeenLastCalledWith("flat");
    control.onChange.mockClear();
    control.key(" ", { target: control.targets[1] }); expect(control.onChange).toHaveBeenCalledExactlyOnceWith("flat");
    control.onChange.mockClear();
    for (const [key, extra] of [["Tab", {}], ["Enter", {}], ["ArrowRight", { metaKey: true }], ["ArrowDown", { ctrlKey: true }], ["ArrowUp", { altKey: true }], ["ArrowLeft", { defaultPrevented: true }]] as const) {
      expect(control.key(key, extra).preventDefault).not.toHaveBeenCalled();
    }
    expect(control.onChange).not.toHaveBeenCalled();
  });

  it.each(["grouped", "flat"] as const)("native disabled %s keeps its reason and never requests tooltip help", (value) => {
    fixture.native = true;
    const why = "Recent lists sessions in the order you opened them.";
    const { segments, onChange } = mount(value, why);
    expect(fixture.helpTexts).toEqual(["", ""]);
    for (const node of segments) {
      expect(node.props.accessibilityHint).toBe(why);
      expect(node.props.accessibilityState).toMatchObject({ disabled: true });
      (node.props.onPress as () => void)();
    }
    expect(segments.map((node) => node.props.accessibilityLabel)).toEqual(["Grouped", "Flat"]);
    expect(fixture.hold).not.toHaveBeenCalled();
    expect(onChange).not.toHaveBeenCalled();
  });
  it.each(["grouped", "flat"] as const)("native selected %s suppresses help while the other radio still switches", (value) => {
    fixture.native = true;
    const { segments, onChange } = mount(value);
    const selected = value === "grouped" ? 0 : 1;
    expect(fixture.helpTexts[selected]).toBe("");
    expect(fixture.helpTexts[1 - selected]).toContain("Search, filters and sort stay the same.");
    expect(segments[selected]!.props.accessibilityHint).toContain("Search, filters and sort stay the same.");
    (segments[selected]!.props.onPress as () => void)();
    expect(onChange).not.toHaveBeenCalled(); expect(fixture.hold).not.toHaveBeenCalled();
    (segments[1 - selected]!.props.onPress as () => void)();
    expect(onChange).toHaveBeenCalledExactlyOnceWith(value === "grouped" ? "flat" : "grouped");
  });
  it("keeps native checked semantics and hints without web listeners or Tab overrides", () => {
    fixture.native = true;
    const { segments, root } = mount("flat");
    expect(root.addEventListener).not.toHaveBeenCalled();
    expect(segments.every((node) => node.props.tabIndex === undefined)).toBe(true);
    expect(segments[1]!.props.accessibilityState).toEqual({ checked: true });
  });
});
