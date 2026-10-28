import { beforeEach, expect, it, vi } from "vitest";

const state = vi.hoisted(() => ({ open: false }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), useState: () => [state.open, (value: boolean) => { state.open = value; }] }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", Modal: Object.assign("Modal", { Content: "ModalContent" }) }));
vi.mock("../client/control-help", () => ({ useActionHelp: () => ({ bindings: {}, dismiss() {} }) }));
vi.mock("../client/dense-session-row", () => ({ cancelDelayedOpen() {} }));
import { SessionActionsMenu, type SessionAction } from "../client/session-actions-menu";

type Element = { type: unknown; props: Record<string, any> };
const walk = (value: any): Element[] => !value || typeof value !== "object" ? [] : Array.isArray(value) ? value.flatMap(walk)
  : [value, ...walk(value.props?.children)];
beforeEach(() => { state.open = true; });

it("attaches takeover's disclosure to its own row before Archive without changing actions", () => {
  const takeover = vi.fn(), archive = vi.fn();
  const actions: SessionAction[] = [
    { key: "archive", label: "Archive", icon: "Archive", perform: archive },
    { key: "takeover", label: "Take over", icon: "Play", footer: "Creates a successor.", perform: takeover },
  ];
  const tree = SessionActionsMenu({ entry: { id: "fixture", title: "Fixture" }, colors: {} as never, actions });
  const menu = walk(tree).find((element) => element.props.role === "menu")!;
  const rows = menu.props.children as Element[];
  expect(rows.map((row) => row.props.action.key)).toEqual(["takeover", "archive"]);
  const draw = (row: Element) => (row.type as (props: any) => any)(row.props);
  const taken = draw(rows[0]!), archived = draw(rows[1]!);
  expect(walk(taken).find((element) => element.props.testID === "session-action-takeover-disclosure")?.props.children).toBe("Creates a successor.");
  expect(walk(archived).some((element) => element.props.children === "Creates a successor.")).toBe(false);
  expect(walk(tree).some((element) => element.type === "Text" && element.props.children === "Creates a successor.")).toBe(false);
  walk(archived).find((element) => element.props.testID === "session-action-archive")!.props.onPress();
  expect(archive).toHaveBeenCalledOnce(); expect(takeover).not.toHaveBeenCalled();
});
