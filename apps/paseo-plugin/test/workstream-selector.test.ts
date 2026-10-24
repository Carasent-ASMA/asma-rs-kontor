import { beforeEach, describe, expect, it, vi } from "vitest";

const state = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0, back: [] as (() => boolean)[] }));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useState: (initial: unknown) => { const index = state.cursor++; if (!(index in state.values)) state.values[index] = initial; return [state.values[index], (next: unknown) => { state.values[index] = next; }]; },
  useRef: (current: unknown) => ({ current }),
  useEffect: (effect: () => void) => { effect(); },
  useContext: () => null,
}));
vi.mock("react-native", () => ({ View: "View", Text: "Text", Pressable: "Pressable", ScrollView: "ScrollView", Platform: { OS: "web" }, findNodeHandle: () => null,
  AccessibilityInfo: { setAccessibilityFocus: () => {} }, BackHandler: { addEventListener: (_type: string, handler: () => boolean) => { state.back.push(handler); return { remove: () => {} }; } } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", ScrollView: "ScrollView", Icon: "Icon" }));

import { normalizeChoice, selectionSummary, toggleSelection, WorkstreamSelector } from "../client/workstream-selector";
import { workstreamOptionLabel, type WorkstreamOption } from "../client/plan-workstreams";

type Element = { type: unknown; props: Record<string, unknown> & { children?: unknown } };
const walk = (node: unknown): Element[] => node === null || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children)];
const text = (node: unknown): string => node === null || node === undefined || typeof node === "boolean" ? "" : typeof node === "string" || typeof node === "number" ? String(node)
  : Array.isArray(node) ? node.map(text).join("") : text((node as Element).props.children);
const byTestID = (tree: unknown, testID: string) => walk(tree).find((element) => element.props.testID === testID);

const option = (key: string, label: string, extra: Partial<WorkstreamOption> = {}): WorkstreamOption =>
  ({ key, label, registered: true, plans: 0, sessions: 2, workspaces: 1, desk: false, shownUnder: {}, othersInDesk: 0, emptyInDesk: 0, ...extra });
const options = [option("beta", "Beta", { sessions: 0, workspaces: 0, plans: 1 }), option("alpha", "Alpha & Co"), option("gamma", "Gamma", { registered: false })];
const colors = new Proxy({}, { get: () => "#000" }) as never;

function render(props: Partial<Parameters<typeof WorkstreamSelector>[0]> = {}) {
  state.cursor = 0;
  const onChange = vi.fn(), onOpen = vi.fn();
  const tree = WorkstreamSelector({ options, selected: null, open: true, registryLoaded: true, about: "Counts are inventory facts.", colors, compact: false, onOpen, onChange, ...props });
  return { tree, onChange, onOpen };
}

beforeEach(() => { state.values = []; state.back = []; });

describe("association choice helpers", () => {
  it("finds choices regardless of case, punctuation, spacing and & versus and", () => {
    expect(normalizeChoice("Alpha & Co")).toBe(normalizeChoice("alpha  and co"));
    expect(normalizeChoice("mobile-app")).toBe(normalizeChoice("Mobile App"));
  });

  it("starts from every current choice when leaving All and never collapses back to All", () => {
    expect(toggleSelection(null, ["a", "b"], "a")).toEqual(["b"]);
    expect(toggleSelection(["a", "gone"], ["a", "b"], "b")).toEqual(["a", "gone", "b"]);
    expect(toggleSelection(["a"], ["a", "b"], "a")).toEqual([]);
  });

  it("summarises All, None and an explicit selection with an unavailable key", () => {
    expect(selectionSummary(null, options)).toEqual({ line: "Workstream associations · All · 3 available", names: null });
    expect(selectionSummary([], options)).toEqual({ line: "Workstream associations · None · 3 available", names: null });
    expect(selectionSummary(["alpha", "gone", "beta"], options)).toEqual({ line: "Workstream associations · 3 selected · 3 available", names: "Alpha & Co, Unavailable association +1" });
  });
});

describe("association selector", () => {
  it("shows the closed summary and its selected names as a way into the list", () => {
    const { tree, onOpen } = render({ open: false, selected: ["alpha"] });
    expect(byTestID(tree, "sessions-associations-toggle")!.props["aria-expanded"]).toBe(false);
    expect(byTestID(tree, "sessions-associations-search")).toBeUndefined();
    const names = byTestID(tree, "sessions-associations-selected")!;
    expect(text(names)).toBe("Alpha & Co");
    (names.props.onPress as () => void)();
    expect(onOpen).toHaveBeenCalledWith(true);
  });

  it("keeps selected and unavailable choices pinned while search narrows only the others", () => {
    state.values[0] = "alp";
    const { tree } = render({ selected: ["beta", "gone"] });
    const rows = walk(tree).filter((element) => element.props.accessibilityRole === "checkbox");
    expect(rows.map((row) => [row.props.testID, row.props["aria-checked"]])).toEqual([
      ["workstream-option-beta", true], ["workstream-option-gone", true], ["workstream-option-alpha", false]]);
    expect(rows.map((row) => row.props.accessibilityLabel)).toEqual(["Beta, 0 sessions", "Unavailable association, not in this context", "Alpha & Co, 2 sessions"]);
    expect(text(tree)).toContain("Selected (2)");
    expect(text(tree)).toContain("plan only");
  });

  it("toggles from All with the complete universe, and offers All and None", () => {
    state.values[0] = "gam";
    const { tree, onChange } = render();
    (byTestID(tree, "workstream-option-gamma")!.props.onPress as () => void)();
    expect(onChange).toHaveBeenLastCalledWith(["beta", "alpha"]);
    (byTestID(tree, "sessions-associations-none")!.props.onPress as () => void)();
    expect(onChange).toHaveBeenLastCalledWith([]);
    (byTestID(tree, "sessions-associations-all")!.props.onPress as () => void)();
    expect(onChange).toHaveBeenLastCalledWith(null);
    expect(text(tree)).toContain("unregistered");
  });

  it("shows every fact of a choice in its own Details, and the saved key of an unavailable one", () => {
    state.values[1] = "beta";
    let { tree } = render({ selected: ["gone"] });
    expect(text(byTestID(tree, "workstream-facts-beta"))).toBe(workstreamOptionLabel(options[0]!, true));
    expect(byTestID(tree, "workstream-details-beta")!.props["aria-expanded"]).toBe(true);
    state.values[1] = "gone";
    ({ tree } = render({ selected: ["gone"] }));
    expect(text(byTestID(tree, "workstream-facts-gone"))).toContain("Saved key gone is selected");
  });

  it("adds a person scope's own count beside each inventory count", () => {
    const { tree } = render({ scopeCounts: new Map([["alpha", 1]]), scopeLabel: "in My work" });
    expect(byTestID(tree, "workstream-option-alpha")!.props.accessibilityLabel).toBe("Alpha & Co, 2 sessions, 1 in My work");
    expect(byTestID(tree, "workstream-option-gamma")!.props.accessibilityLabel).toBe("Gamma, 2 sessions, 0 in My work");
  });

  it("counts the choices My work leaves out without listing them, and offers All work", () => {
    const hidden = [option("access", "Access & login", { sessions: 0, workspaces: 0 }), option("qnr-v2", "QNR v2", { sessions: 0, workspaces: 0 })];
    const onShowAll = vi.fn();
    let { tree } = render({ hidden, onShowAll });
    expect(text(byTestID(tree, "sessions-associations-hidden"))).toContain("2 workstreams have no sessions here.");
    expect(byTestID(tree, "workstream-option-qnr-v2")).toBeUndefined();
    (byTestID(tree, "sessions-associations-show-all")!.props.onPress as () => void)();
    expect(onShowAll).toHaveBeenCalledTimes(1);
    state.values = []; state.values[0] = "qnr";
    ({ tree } = render({ hidden, onShowAll }));
    expect(text(tree)).toContain("No other association matches.");
    expect(text(byTestID(tree, "sessions-associations-hidden"))).toContain("1 matching workstream has no sessions here.");
    state.values = []; state.values[0] = "alp";
    ({ tree } = render({ hidden, onShowAll }));
    expect(byTestID(tree, "sessions-associations-hidden")).toBeUndefined();
    state.values = [];
    expect(byTestID(render().tree, "sessions-associations-hidden")).toBeUndefined();
  });

  it("closes on compact Back before Filters handles it", () => {
    const { onOpen } = render({ compact: true });
    expect(state.back).toHaveLength(1);
    expect(state.back[0]!()).toBe(true);
    expect(onOpen).toHaveBeenCalledWith(false);
    state.back = [];
    render({ compact: false });
    expect(state.back).toHaveLength(0);
  });
});
