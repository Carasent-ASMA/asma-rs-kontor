import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
const hooks = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(),
  useRef: (current: unknown) => { const index = hooks.cursor++; if (!(index in hooks.slots)) hooks.slots[index] = { current }; return hooks.slots[index]; },
  useState: <T,>(initial: T | (() => T)) => {
    const index = hooks.cursor++; if (!(index in hooks.slots)) hooks.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [hooks.slots[index] as T, (next: T) => { hooks.slots[index] = next; }];
  } }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
import { PlanReader } from "../client/plan-reader";
import { goBack, navigate, readRouteView, writeRouteView } from "../client/navigation-state";
type Props = Parameters<typeof ReadOnlyPage>[0];
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
const source: SourceRead = { source: { id: "plan-a", title: "Plan A", kind: "plan", path: "/plans/a.md", workstream: null, epic: null, projectId: null, revisionKind: "working copy" }, status: "partial", revision: "sha-a", readAt: "now", modifiedAt: null, content: "# Start\nBody\n## End", graph: null, records: null, diagnostics: [] };
const event = (y: number) => ({ nativeEvent: { contentOffset: { y } } });
const layout = (values: object) => ({ nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 0, ...values } } });
beforeEach(() => { hooks.slots = []; hooks.cursor = 0; });
function mount(hostId: string) {
  hooks.slots = [];
  const route = navigate(hostId, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
  const saved = readRouteView(hostId, route);
  const props: Props = { tab: "Plan", colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: [source.source], reads: { "plan-a": source }, sourceId: "plan-a", sourceError: null, query: saved.query, setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: saved.offset, onScroll: (offset) => writeRouteView(hostId, route, { offset }) };
  const reader = ReadOnlyPage(props) as ReactElement<Parameters<typeof PlanReader>[0]>;
  expect(reader.type).toBe(PlanReader);
  // ReadOnlyPage's own hooks ran first; the reader starts with fresh slots.
  hooks.slots = [];
  const render = () => { hooks.cursor = 0; return PlanReader(reader.props); };
  let tree = render();
  const find = (test: (element: ReactElement<Record<string, unknown>>) => boolean) => elements(tree).find(test)!;
  const document = find((element) => element.props.testID === "plan-document");
  const scrollTo = vi.fn(); (document.props.ref as { current: unknown }).current = { scrollTo };
  return { find, scrollTo, rerender: () => { tree = render(); },
    tree: () => tree,
    layout: () => (document.props.onLayout as (event: object) => void)(layout({ height: 640 })),
    size: (height: number) => (document.props.onContentSizeChange as (width: number, height: number) => void)(400, height),
    scroll: (y: number) => (document.props.onScroll as (event: object) => void)(event(y)),
    drag: () => (document.props.onScrollBeginDrag as () => void)(),
  };
}
describe("Plan reader position", () => {
  it("keeps one vertical scroller with the intro, a sticky reader bar and the article", () => {
    const screen = mount("document-shape");
    const document = screen.find((element) => element.props.testID === "plan-document");
    expect(document.props.stickyHeaderIndices).toEqual([1]);
    const children = (document.props.children as ReactNode[]).filter(isValidElement) as ReactElement<Record<string, unknown>>[];
    expect(children).toHaveLength(3);
    expect(children[1]!.props.testID).toBe("plan-reader-bar");
    expect(elements(screen.tree()).filter((element) => element.type === "ScrollView" && element.props.horizontal !== true)).toHaveLength(1);
  });
  it("records a scrub to 900 in the route and restores it after Back", () => {
    const screen = mount("document-return"); screen.drag(); screen.scroll(900);
    expect(readRouteView("document-return")).toEqual({ query: "", offset: 900 });
    navigate("document-return", { tab: "Overview" }); goBack("document-return");
    const returned = mount("document-return"); returned.layout(); returned.size(5000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 900, animated: false });
  });
  it("ignores early/clamped events and releases restoration when the user drags", () => {
    const route = navigate("document-measurement", { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    writeRouteView("document-measurement", route, { offset: 900 });
    const screen = mount("document-measurement"); screen.scroll(0); screen.layout(); screen.size(640); screen.scroll(0);
    expect(readRouteView("document-measurement").offset).toBe(900);
    screen.size(5000); screen.scroll(900); screen.size(5000);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 0, animated: false }], [{ y: 900, animated: false }]]);
    screen.drag(); screen.scroll(1300); screen.size(6000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(2);
    expect(readRouteView("document-measurement").offset).toBe(1300);
  });
  it.each([true, false])("persists wheel scrolling after a clamped restore without drag callbacks (clamp event: %s)", (emitClampEvent) => {
    const hostId = `document-clamped-wheel-${emitClampEvent}`;
    const route = navigate(hostId, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    writeRouteView(hostId, route, { offset: 900 });
    const screen = mount(hostId); screen.layout(); screen.size(1000);
    expect(screen.scrollTo).toHaveBeenLastCalledWith({ y: 360, animated: false });
    if (emitClampEvent) screen.scroll(360);
    // Web wheel events arrive through onScroll without onScrollBeginDrag.
    screen.scroll(120);
    expect(readRouteView(hostId).offset).toBe(120);
    screen.size(5000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(1);
    navigate(hostId, { tab: "Overview" }); goBack(hostId);
    const returned = mount(hostId); returned.layout(); returned.size(1000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 120, animated: false });
    navigate(hostId, { sourceRevision: "sha-b" });
    expect(readRouteView(hostId).offset).toBe(0);
    expect(readRouteView(`${hostId}-other-host`, route).offset).toBe(0);
    goBack(hostId);
    expect(readRouteView(hostId).offset).toBe(120);
  });
  it("persists Contents heading navigation below the sticky bar and stops pending restoration", () => {
    const route = navigate("document-heading", { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    writeRouteView("document-heading", route, { offset: 900 });
    const screen = mount("document-heading");
    (screen.find((element) => element.props.testID === "plan-reader-bar").props.onLayout as (event: object) => void)(layout({ height: 50 }));
    (screen.find((element) => element.props.testID === "plan-line-2").props.onLayout as (event: object) => void)(layout({ y: 450 }));
    (screen.find((element) => element.props.testID === "plan-article").props.onLayout as (event: object) => void)(layout({ y: 200 }));
    (screen.find((element) => element.props.label === "Contents (2)").props.onPress as () => void)(); screen.rerender();
    (screen.find((element) => element.props.label === "End").props.onPress as () => void)();
    screen.layout(); screen.size(5000);
    expect(readRouteView("document-heading").offset).toBe(200 + 450 - 50 - 6);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 594, animated: true }]]);
  });
});
