import { Children, isValidElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: <T,>(value: T | (() => T)) => [typeof value === "function" ? (value as () => T)() : value, vi.fn()], useContext: () => null, useRef: <T,>(current: T) => ({ current }), useMemo: <T,>(compute: () => T) => compute(), useEffect: () => {}, useId: () => "test-help" }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View", Platform: { OS: "web" }, useWindowDimensions: () => ({ fontScale: 1 }) }));
vi.mock("@getpaseo/plugin/client/ui", () => ({ ScrollView: "ScrollView", TextInput: "TextInput", Icon: "Icon" }));
import { TokenomicsHelp } from "../client/tokenomics-help";
import { TokenomicsChoice, TokenomicsOptions } from "../client/tokenomics-choice";
import type { Colors } from "../client/page-controls";
const colors = {} as Colors;
function controls(node: ReactNode): Record<string, unknown>[] {
  return Children.toArray(node).flatMap((n) => isValidElement<{ children?: ReactNode }>(n) ? [...(n.type === "Pressable" ? [n.props as Record<string, unknown>] : []), ...controls(n.props.children)] : []);
}
describe("native Tokenomics selection semantics", () => {
  const options = [{ value: "a", label: "Person A" }, { value: "b", label: "Person B" }];
  it("exposes radio checked state through both supported native aliases and selects once", () => {
    const onChange = vi.fn(), done = vi.fn();
    const rows = controls(TokenomicsOptions({ label: "User", options, value: ["b"], onChange, onDone: done, colors }));
    expect(rows.map((r) => [r.accessibilityRole, r.accessibilityState, r["aria-checked"]])).toEqual([["radio", { checked: false }, false], ["radio", { checked: true }, true]]);
    (rows[0]!.onPress as () => void)(); expect(onChange).toHaveBeenCalledWith(["a"]); expect(done).toHaveBeenCalledOnce();
  });
  it("retains checkbox state, individual toggles and All/None semantics", () => {
    const onChange = vi.fn(); const rows = controls(TokenomicsOptions({ label: "Workstreams", multiple: true, options, value: ["b"], onChange, colors }));
    const boxes = rows.filter((r) => r.accessibilityRole === "checkbox").sort((a, b) => String(a.accessibilityLabel).localeCompare(String(b.accessibilityLabel)));
    expect(boxes.map((r) => [r.accessibilityState, r["aria-checked"]])).toEqual([[{ checked: false }, false], [{ checked: true }, true]]);
    (boxes[0]!.onPress as () => void)(); expect(onChange).toHaveBeenLastCalledWith(["b", "a"]);
    (boxes[1]!.onPress as () => void)(); expect(onChange).toHaveBeenLastCalledWith([]);
    (rows.find((r) => r.accessibilityLabel === "Workstreams: select All")!.onPress as () => void)(); expect(onChange).toHaveBeenLastCalledWith(["a", "b"]);
    (rows.find((r) => r.accessibilityLabel === "Workstreams: select None")!.onPress as () => void)(); expect(onChange).toHaveBeenLastCalledWith([]);
  });
  it("exposes the closed contextual-help state through both supported aliases", () => {
    const trigger = controls(TokenomicsHelp({ title: "cost basis", colors, children: "Details", short: true }))[0]!;
    expect(trigger.accessibilityLabel).toBe("About cost basis"); expect(trigger.accessibilityState).toEqual({ expanded: false }); expect(trigger["aria-expanded"]).toBe(false);
  });
  it("publishes an ordinary trigger's expanded state on web and native", () => {
    for (const row of [false, true]) {
      const trigger = controls(TokenomicsChoice({ label: "User", options, value: ["b"], onChange: vi.fn(), colors, row }))[0]!;
      expect(trigger.accessibilityState).toEqual({ expanded: false }); expect(trigger["aria-expanded"]).toBe(false);
    }
  });
});
