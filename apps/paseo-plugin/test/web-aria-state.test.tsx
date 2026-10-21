import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: (initial: unknown) => [initial, () => {}] }));
vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { MenuItem } from "../client/anchored-menu";
import { Action, Disclosure, IconButton } from "../client/page-controls";

type Element = ReactElement<Record<string, unknown>>;
const colors = {} as never;
/** The pressable a control renders; React Native Web forwards its aria-* props and drops accessibilityState. */
function pressable(node: ReactNode): Record<string, unknown> {
  if (!isValidElement<Record<string, unknown>>(node)) throw new Error("not an element");
  if (typeof node.type === "function") return pressable((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  if (node.type === "Pressable") return (node as Element).props;
  const children = [(node as Element).props.children].flat() as ReactNode[];
  const found = children.find((child) => isValidElement(child));
  if (found === undefined) throw new Error(`no pressable under ${String(node.type)}`);
  return pressable(found);
}
const aria = (props: Record<string, unknown>) => Object.fromEntries(Object.entries(props).filter(([key, value]) => key.startsWith("aria-") && value !== undefined));

describe("control state on the web", () => {
  it("says whether a disclosure is open", () => {
    expect(aria(pressable(<Disclosure label="Legend" colors={colors}>{null}</Disclosure>))).toEqual({ "aria-expanded": false });
    expect(aria(pressable(<Disclosure label="Legend" colors={colors} open onToggle={() => {}}>{null}</Disclosure>))).toEqual({ "aria-expanded": true });
  });

  it("marks the selected tab, the pressed toggle and an open menu button, and nothing on a plain button", () => {
    const action = (props: Partial<Parameters<typeof Action>[0]>) => aria(pressable(<Action label="Overview" colors={colors} onPress={() => {}} {...props} />));
    expect([action({ tab: true, selected: true }), action({ tab: true })]).toEqual([{ "aria-selected": true }, { "aria-selected": false }]);
    expect([action({ selected: true }), action({ selected: false })]).toEqual([{ "aria-pressed": true }, { "aria-pressed": false }]);
    // More is a menu button: open or closed, never a toggle, even while one of its views is current.
    expect(action({ expanded: true, selected: true })).toEqual({ "aria-expanded": true });
    expect(action({})).toEqual({});
  });

  it("exposes Back as disabled until navigation history exists", () => {
    const back = (disabled: boolean) => pressable(<IconButton label="Back" icon="ChevronLeft" colors={colors} disabled={disabled} onPress={() => {}} />);
    expect(back(true)).toMatchObject({ disabled: true, accessibilityState: { disabled: true }, "aria-disabled": true });
    expect(back(false)).toMatchObject({ disabled: false, accessibilityState: { disabled: false }, "aria-disabled": false });
  });

  it("marks only the current menu item", () => {
    const item = (selected: boolean) => aria(pressable(<MenuItem label="Inbox" selected={selected} colors={colors} onPress={() => {}} />));
    expect([item(true), item(false)]).toEqual([{ "aria-current": true }, {}]);
  });
});
