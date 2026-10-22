import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const state = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0, cleanup: [] as (() => void)[], back: [] as (() => boolean)[], platform: "web" }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useState: (initial: unknown) => { const index = state.cursor++; if (!(index in state.slots)) state.slots[index] = initial; return [state.slots[index], (next: unknown) => { state.slots[index] = next; }]; },
  useRef: (initial: unknown) => { const index = state.cursor++; if (!(index in state.slots)) state.slots[index] = { current: initial }; return state.slots[index]; },
  useEffect: (effect: () => (() => void) | undefined) => { const cleanup = effect(); if (cleanup) state.cleanup.push(cleanup); },
  useContext: () => null,
}));
vi.mock("react-native", () => ({ View: "View", Text: "Text", Pressable: "Pressable", Platform: { get OS() { return state.platform; } }, findNodeHandle: () => null,
  AccessibilityInfo: { setAccessibilityFocus: vi.fn() }, BackHandler: { addEventListener: (_type: string, handler: () => boolean) => { state.back.push(handler); return { remove: vi.fn() }; } } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", ScrollView: "ScrollView", Icon: "Icon" }));
import { SessionScopeSelector } from "../client/session-scope-selector";

type Element = { type: unknown; props: Record<string, any> & { children?: unknown } };
const walk = (node: unknown): Element[] => node === null || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const byId = (tree: unknown, id: string) => walk(tree).find((node) => node.props.testID === id)!;
const options = Array.from({ length: 100 }, (_, index) => ({ key: `desk-project:prj_${index}`, label: "Duplicate desk name", detail: `prj_${index}` }));
function render(extra: Partial<Parameters<typeof SessionScopeSelector>[0]> = {}) {
  state.cursor = 0;
  const onOpen = vi.fn(), onChange = vi.fn();
  const tree = SessionScopeSelector({ id: "scope", label: "Desk", options, selected: null, open: true, colors: {} as never, compact: true, onOpen, onChange, ...extra });
  return { tree, onOpen, onChange };
}
beforeEach(() => { state.slots = []; state.cursor = 0; state.back = []; state.cleanup = []; state.platform = "web"; });
afterEach(() => { state.cleanup.forEach((cleanup) => cleanup()); vi.unstubAllGlobals(); });

describe("searchable scope choice", () => {
  it.each([320, 390, 1280])("keeps selected/unavailable keys and exact native IDs while searching a long list at width %s", (width) => {
    render({ compact: width < 600 });
    let result = render({ compact: width < 600, selected: "desk-project:missing" });
    result.tree && byId(result.tree, "sessions-scope-search").props.onChangeText("prj_99");
    result = render({ compact: width < 600, selected: "desk-project:missing" });
    const rows = walk(result.tree).filter((node) => String(node.props.testID).startsWith("sessions-scope-choice-"));
    expect(rows.map((node) => node.props.testID)).toEqual(["sessions-scope-choice-all", "sessions-scope-choice-desk-project:missing", "sessions-scope-choice-desk-project:prj_99"]);
    expect(rows[1]!.props.accessibilityLabel).toBe("Unavailable selection. desk-project:missing");
    expect(rows[1]!.props.accessibilityState.selected).toBe(true);
    expect(rows[2]!.props.accessibilityLabel).toBe("Duplicate desk name. prj_99");
    expect(byId(result.tree, "sessions-scope-search").props.style.minHeight).toBeGreaterThanOrEqual(width < 600 ? 44 : 32);
    rows[2]!.props.onPress();
    expect(result.onChange).toHaveBeenCalledWith("desk-project:prj_99");
    rows[0]!.props.onPress();
    expect(result.onChange).toHaveBeenLastCalledWith(null);
  });

  it("keeps the chosen row when no available choice matches and hides choices when closed", () => {
    state.slots[0] = "no-such-desk";
    let { tree } = render({ selected: options[0]!.key });
    expect(byId(tree, `sessions-scope-choice-${options[0]!.key}`).props.accessibilityState.selected).toBe(true);
    expect(walk(tree).filter((node) => String(node.props.testID).startsWith("sessions-scope-choice-"))).toHaveLength(2);
    ({ tree } = render({ open: false, selected: options[0]!.key }));
    expect(byId(tree, "sessions-scope-search")).toBeUndefined();
    expect(byId(tree, "sessions-scope-toggle").props.accessibilityState.expanded).toBe(false);
  });

  it("consumes Escape and restores trigger focus, cleans up, and leaves handled keys alone", () => {
    let listener: (event: any) => void = () => {};
    const removeEventListener = vi.fn();
    vi.stubGlobal("document", { addEventListener: (_: string, handler: typeof listener) => { listener = handler; }, removeEventListener });
    const { tree, onOpen } = render();
    const focus = vi.fn(); byId(tree, "sessions-scope-toggle").props.ref.current = { focus };
    const event = { key: "Escape", defaultPrevented: true, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    listener(event); expect(onOpen).not.toHaveBeenCalled();
    event.defaultPrevented = false; listener(event);
    expect(onOpen).toHaveBeenCalledWith(false); expect(focus).toHaveBeenCalledOnce();
    expect(event.preventDefault).toHaveBeenCalledOnce(); expect(event.stopPropagation).toHaveBeenCalledOnce();
    state.cleanup.forEach((cleanup) => cleanup()); state.cleanup = [];
    expect(removeEventListener).toHaveBeenCalledWith("keydown", listener);
  });

  it("consumes native Back before the parent Filters page", () => {
    state.platform = "ios";
    const { tree, onOpen } = render();
    const focus = vi.fn(); byId(tree, "sessions-scope-toggle").props.ref.current = { focus };
    expect(state.back[0]!()).toBe(true);
    expect(onOpen).toHaveBeenCalledWith(false); expect(focus).toHaveBeenCalledOnce();
  });
});
