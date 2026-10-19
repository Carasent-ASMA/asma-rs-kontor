import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: <T,>(initial: T) => {
  const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
  return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
} }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
type Props = Parameters<typeof ReadOnlyPage>[0];
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
beforeEach(() => { hooks.values = []; hooks.cursor = 0; });
describe("formatted read-only Plan controls", () => {
  it("collapses compact contents, retains exact Source and search, and measures heading navigation", () => {
    const content = "# Start\n\nA paragraph.\n\n## End\nLast line.";
    const source: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: "/plans/a.md", projectId: null, workstream: null, epic: null, revisionKind: "working copy" }, status: "ready", revision: "hash", readAt: "now", modifiedAt: null, content, graph: null, records: null, diagnostics: [] };
    const saved = vi.fn();
    const props: Props = { tab: "Plan", compact: true, colors: {} as Props["colors"], sources: [source.source], reads: { plan: source }, sourceId: "plan", sourceError: null, entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, staleReason: null, error: null, includeArchived: false, pages: 1 }, query: "", setQuery: (value) => { props.query = value; }, onOpen: vi.fn(), onSource: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: 0, initialDocumentOffset: 0, onScroll: vi.fn(), onDocumentScroll: saved };
    const outer = ReadOnlyPage(props); const plan = elements(outer).find((node) => node.props.tab === "Plan")!;
    const render = () => { hooks.cursor = 0; return (plan.type as (props: Props) => ReactNode)(props); };
    let tree = render(); const click = (label: string) => { const action = elements(tree).find((node) => node.props.label === label)!; (action.props.onPress as () => void)(); tree = render(); };
    expect(elements(tree).some((node) => node.props.label === "End")).toBe(false);
    click("Contents (2)"); expect(elements(tree).some((node) => node.props.label === "End")).toBe(true);
    click("Exact Source"); expect(elements(tree).find((node) => node.props.testID === "plan-exact-source")?.props.children).toBe(content);
    const search = elements(tree).find((node) => node.props.accessibilityLabel === "Search this plan")!;
    (search.props.onChangeText as (value: string) => void)("End"); tree = render();
    expect(props.query).toBe("End"); expect(elements(tree).some((node) => node.props.testID === "plan-exact-source")).toBe(false);
    const heading = elements(tree).find((node) => node.props.testID === "plan-line-4")!;
    (heading.props.onLayout as (event: object) => void)({ nativeEvent: { layout: { y: 450 } } });
    const end = elements(tree).find((node) => node.props.label === "End")!; (end.props.onPress as () => void)();
    expect(saved).toHaveBeenLastCalledWith(450);
    expect(elements(tree).some((node) => /approve|execute|edit plan/i.test(String(node.props.label)))).toBe(false);
  });
});
