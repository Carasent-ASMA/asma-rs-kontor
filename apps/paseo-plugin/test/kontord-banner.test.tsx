import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useState: <T,>(initial: T) => {
    const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
    return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
  },
}));
vi.mock("react-native", () => ({ View: "View", Text: "Text", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { KontordBanner } from "../client/kontord-banner";
import { NoticeAction } from "../client/notice-tray";

type Element = ReactElement<Record<string, unknown>>;
const elements = (node: ReactNode): Element[] => Array.isArray(node) ? node.flatMap(elements) : isValidElement<Record<string, unknown>>(node) ? [node, ...elements(node.props.children as ReactNode)] : [];
const text = (node: ReactNode): string => typeof node === "string" ? node : Array.isArray(node) ? node.map(text).join("") : isValidElement<Record<string, unknown>>(node) ? text(node.props.children as ReactNode) : "";

describe("Kontor status banner", () => {
  it("keeps details closed until its info button is pressed, and reports the expanded state", () => {
    hooks.values = [];
    const draw = () => { hooks.cursor = 0; return KontordBanner({ status: { observation: "ready", composition: "floor-off", checkedAt: new Date(0).toISOString() }, compact: true, colors: {} as never }); };
    const toggle = () => elements(draw()).find((node) => node.type === NoticeAction)!;
    const details = "You can browse sessions and project information. Automatic agent setup and workspace checks are turned off.";
    expect(text(draw())).toBe(`Kontor connected · Navigation-only mode\nLast checked ${new Date(0).toLocaleTimeString()}.`);
    expect(toggle().props).toMatchObject({ icon: "Info", accessibilityLabel: "Kontor status details", expanded: false });
    // Pressable fires onPress for a tap and for Enter/Space on a focused button; hover only shows help.
    (toggle().props.onPress as () => void)();
    expect(toggle().props.expanded).toBe(true);
    expect(text(draw())).toContain(details);
    (toggle().props.onPress as () => void)();
    expect(text(draw())).not.toContain(details);
  });

  it("shows no check time when there is no status", () => {
    hooks.values = []; hooks.cursor = 0;
    expect(text(KontordBanner({ status: null, compact: false, colors: {} as never }))).toBe("Kontor status unknown");
  });
});
