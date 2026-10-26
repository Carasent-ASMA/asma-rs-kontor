import { isValidElement } from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView",
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

import { OperationSheet, SheetFooter } from "../client/operation-sheet";

type Element = { type: unknown; props: Record<string, unknown> };
/** The footer's buttons are hook-free and drawn in place. */
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? []
  : typeof node.type === "function" && node.type.name === "FooterButton" ? [node, ...walk((node.type as (props: object) => unknown)(node.props))] : [node, ...walk(node.props.children)];
const buttons = (tree: Element[]) => tree.filter((node) => node.type === "Pressable");
const style = (button: Element) => (button.props.style as (state: { pressed: boolean }) => Record<string, unknown>)({ pressed: false });

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const footer = (primary: Parameters<typeof SheetFooter>[0]["primary"], note: string | null = null) =>
  walk(SheetFooter({ colors: COLORS, note, secondary: { testID: "sheet-close", label: "Close", onPress: vi.fn() }, primary }));

describe("SheetFooter", () => {
  it("puts the secondary action left and the primary right", () => {
    const tree = footer({ testID: "sheet-confirm", label: "Archive 3", onPress: vi.fn() });
    expect(buttons(tree).map((node) => node.props.testID)).toEqual(["sheet-close", "sheet-confirm"]);
  });

  it("keeps a disabled primary's label, speaks why with it and keeps it in Tab", () => {
    const primary = buttons(footer({ testID: "sheet-confirm", label: "Take over 3", disabled: true, hint: "Select sessions first.", onPress: vi.fn() }))[1]!;
    expect(primary.props).toMatchObject({ accessibilityLabel: "Take over 3. Select sessions first.", tabIndex: 0, disabled: true, accessibilityState: { disabled: true }, "aria-disabled": true });
    expect(buttons(footer({ label: "Take over 3", hint: "Select sessions first.", onPress: vi.fn() }))[1]!.props.accessibilityLabel).toBe("Take over 3");
    expect(walk(primary).find((node) => node.type === "Text")!.props.children).toBe("Take over 3");
  });

  it("gives the primary the wider share of the row", () => {
    const [secondary, primary] = buttons(footer({ testID: "sheet-confirm", label: "Retry remaining 2", onPress: vi.fn() }));
    expect(style(secondary!).flex).toBe(1);
    expect(style(primary!).flex).toBe(1.5);
  });

  it("writes the note under the buttons only when there is one", () => {
    const texts = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => node.props.children);
    expect(texts(footer({ label: "Retry remaining 1", onPress: vi.fn() }, "Completed seats are not restarted."))).toEqual(["Close", "Retry remaining 1", "Completed seats are not restarted."]);
    expect(texts(footer({ label: "Retry remaining 1", onPress: vi.fn() }))).toEqual(["Close", "Retry remaining 1"]);
  });
});

describe("OperationSheet", () => {
  it("treats the host's close as the footer's dismissal", () => {
    const onClose = vi.fn();
    const modal = walk(OperationSheet({ testID: "sheet", title: "Takeover results", colors: COLORS, onClose, footer: null, children: null }))[0]!;
    (modal.props.onOpenChange as (open: boolean) => void)(true);
    expect(onClose).not.toHaveBeenCalled();
    (modal.props.onOpenChange as (open: boolean) => void)(false);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
