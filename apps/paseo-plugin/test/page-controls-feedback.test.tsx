import { describe, expect, it, vi, beforeEach } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ ActivityIndicator: "ActivityIndicator", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/control-help", () => { const help = () => ({ bindings: {}, dismiss: () => {}, hold: () => {}, forget: () => {} }); return { useControlHelp: help, useActionHelp: help }; });

import { Action, Disclosure, IconButton } from "../client/page-controls";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#fafafa", surface2: "#eee", border: "#ccc", accent: "#20744A" } as never;
/** Draws a component through any wrapper to its Pressable, as React would on the next render. */
const pressable = (draw: () => unknown) => {
  const tree = harness.current.render(() => { let node = draw() as Element; while (typeof node.type === "function") node = (node.type as (props: unknown) => Element)(node.props); return node; });
  return walk(tree).find((element) => element.type === "Pressable")!;
};
const look = (element: Element, pressed: boolean) => (element.props.style as (state: { pressed: boolean }) => Record<string, unknown>)({ pressed });
const spinning = (element: Element) => walk(element).some((child) => child.type === "ActivityIndicator");

beforeEach(() => harness.fresh());

describe("shared controls acknowledge a press", () => {
  it("paint pressed on touch-down without the web's press-in delay, also on a selected control", () => {
    for (const element of [pressable(() => <Action label="Resume" colors={colors} onPress={() => {}} />), pressable(() => <Action label="Running" selected colors={colors} onPress={() => {}} />),
      pressable(() => <IconButton label="Back" icon="ChevronLeft" colors={colors} onPress={() => {}} />), pressable(() => <Disclosure label="Legend" colors={colors}>{null}</Disclosure>)]) {
      expect(element.props).toMatchObject({ delayPressIn: 0, delayLongPress: 500 });
      expect(look(element, true)).toMatchObject({ backgroundColor: "#eee", opacity: 0.7 });
      expect(look(element, false).opacity).toBe(1);
    }
    expect(look(pressable(() => <Action label="Resume" disabled colors={colors} onPress={() => {}} />), false).opacity).toBe(0.5);
  });

  it("shows pending for a returned promise, ignores a second tap before it renders, and frees the control when it settles", async () => {
    let finish!: () => void;
    const onPress = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
    const draw = () => <Action label="Refresh" icon="RefreshCw" colors={colors} onPress={onPress} />;
    let element = pressable(draw);
    expect(element.props["aria-busy"]).toBeUndefined();
    (element.props.onPress as () => void)(); (element.props.onPress as () => void)();
    expect(onPress).toHaveBeenCalledTimes(1);
    element = pressable(draw);
    expect(element.props).toMatchObject({ "aria-busy": true, accessibilityState: { busy: true } });
    expect(spinning(element)).toBe(true);
    (element.props.onPress as () => void)();
    expect(onPress).toHaveBeenCalledTimes(1);
    finish(); await Promise.resolve(); await Promise.resolve();
    element = pressable(draw);
    expect(element.props["aria-busy"]).toBeUndefined();
    expect(element.props.accessibilityState).not.toHaveProperty("busy");
    expect(spinning(element)).toBe(false);
    (element.props.onPress as () => void)();
    expect(onPress).toHaveBeenCalledTimes(2);
  });

  it("frees the control after a rejection and leaves the result to its owner", async () => {
    const onPress = vi.fn(() => Promise.reject(new Error("host refused")));
    const draw = () => <Action label="Refresh" colors={colors} onPress={onPress} />;
    (pressable(draw).props.onPress as () => void)();
    await Promise.resolve(); await Promise.resolve();
    const element = pressable(draw);
    expect(element.props["aria-busy"]).toBeUndefined();
    (element.props.onPress as () => void)();
    expect(onPress).toHaveBeenCalledTimes(2);
  });

  it("keeps a synchronous action immediate and repeatable", () => {
    const onPress = vi.fn();
    const element = pressable(() => <Action label="Close" colors={colors} onPress={onPress} />);
    (element.props.onPress as () => void)(); (element.props.onPress as () => void)();
    expect(onPress).toHaveBeenCalledTimes(2);
    expect(spinning(pressable(() => <Action label="Close" colors={colors} onPress={onPress} />))).toBe(false);
  });
});
