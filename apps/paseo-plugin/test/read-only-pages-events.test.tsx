import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }) }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
import { goBack, navigate, readRouteView, writeRouteView } from "../client/navigation-state";
type Props = Parameters<typeof ReadOnlyPage>[0];
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
const source: SourceRead = { source: { id: "plan-a", title: "Plan A", kind: "plan", path: "/plans/a.md", workstream: null, epic: null, projectId: null, revisionKind: "working copy" }, status: "partial", revision: "sha-a", readAt: "now", modifiedAt: null, content: "# Start\nBody\n## End", graph: null, records: null, diagnostics: [] };
const event = (y: number) => ({ nativeEvent: { contentOffset: { y } } });
function mount(hostId: string) {
  const route = navigate(hostId, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
  const saved = readRouteView(hostId, route);
  const props: Props = { tab: "Plan", colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: [source.source], reads: { "plan-a": source }, sourceId: "plan-a", sourceError: null, query: saved.query, setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: saved.offset, onScroll: (offset) => writeRouteView(hostId, route, { offset }), initialDocumentOffset: saved.documentOffset, onDocumentScroll: (documentOffset) => writeRouteView(hostId, route, { documentOffset }) };
  const outer = ReadOnlyPage(props);
  const plan = elements(outer).find((element) => element.props.tab === "Plan")!;
  const tree = (plan.type as (props: Props) => ReactNode)(plan.props as Props);
  const document = elements(tree).find((element) => element.props.testID === "plan-document")!;
  const scrollTo = vi.fn(); (document.props.ref as { current: unknown }).current = { scrollTo };
  return { outer, tree, document, scrollTo,
    layout: () => (document.props.onLayout as (event: object) => void)({ nativeEvent: { layout: { height: 640 } } }),
    size: (height: number) => (document.props.onContentSizeChange as (width: number, height: number) => void)(400, height),
    scroll: (y: number) => (document.props.onScroll as (event: object) => void)(event(y)),
    drag: () => (document.props.onScrollBeginDrag as () => void)(),
  };
}
describe("inner Plan document position", () => {
  it("records a scrub to 900 separately from the outer page and restores it after Back", () => {
    const screen = mount("document-return"); screen.drag(); screen.scroll(900);
    (screen.outer.props.onScroll as (event: object) => void)(event(120));
    expect(readRouteView("document-return")).toMatchObject({ offset: 120, documentOffset: 900 });
    navigate("document-return", { tab: "Overview" }); goBack("document-return");
    const returned = mount("document-return"); returned.layout(); returned.size(5000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 900, animated: false });
  });
  it("ignores early/clamped events and releases restoration when the user drags", () => {
    const route = navigate("document-measurement", { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    writeRouteView("document-measurement", route, { documentOffset: 900 });
    const screen = mount("document-measurement"); screen.scroll(0); screen.layout(); screen.size(640); screen.scroll(0);
    expect(readRouteView("document-measurement").documentOffset).toBe(900);
    screen.size(5000); screen.scroll(900); screen.size(5000);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 0, animated: false }], [{ y: 900, animated: false }]]);
    screen.drag(); screen.scroll(1300); screen.size(6000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(2);
    expect(readRouteView("document-measurement").documentOffset).toBe(1300);
  });
  it("persists heading navigation immediately and stops pending restoration", () => {
    const route = navigate("document-heading", { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    writeRouteView("document-heading", route, { documentOffset: 900 });
    const screen = mount("document-heading");
    const endLine = elements(screen.tree).find((element) => element.props.children === "## End")!;
    (endLine.props.onLayout as (event: object) => void)({ nativeEvent: { layout: { y: 450 } } });
    const heading = elements(screen.tree).find((element) => element.props.label === "End")!;
    (heading.props.onPress as () => void)(); screen.layout(); screen.size(5000);
    expect(readRouteView("document-heading").documentOffset).toBe(450);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 450, animated: true }]]);
  });
});
