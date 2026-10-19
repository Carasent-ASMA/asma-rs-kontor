import { isValidElement, type ReactElement, type ReactNode } from "react";
import { fileURLToPath, URL } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
const model = vi.hoisted(() => ({ open: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => typeof initial === "number" ? [initial, vi.fn()] : [model.open, (next: boolean) => { model.open = next; }] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
import { goBack, navigate, readRouteView, writeRouteView } from "../client/navigation-state";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
const catalog = sourceCatalog(JSON.stringify([{ id: "authoring", title: "Authoring", kind: "teams", path: fileURLToPath(new URL("./fixtures/teams-source/authoring-fleet.yml", import.meta.url)), expectedSha256: "f7ddfff7a0db716e3356d43fe209c9c73f4fe51866de5ebbc26f641968fbea54" }]));
let snapshot: TeamsSourceSnapshot;
type Props = Parameters<typeof ReadOnlyPage>[0];
type Element = ReactElement<Record<string, unknown>>;
function expand(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(expand);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return expand((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...expand(node.props.children as ReactNode)];
}
function mount(hostId: string) {
  const route = navigate(hostId, { tab: "AI teams & sessions", projectId: "project-a", sourceId: "plan-a", sourceRevision: "sha-a" });
  const saved = readRouteView(hostId, route);
  const props: Props = { tab: route.tab, teamsSources: catalog.sources, teamsSnapshots: { authoring: snapshot }, teamsAvailable: true, colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: [], reads: {}, sourceId: route.sourceId, sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: saved.offset, onScroll: (offset) => writeRouteView(hostId, route, { offset }), initialDocumentOffset: 0, onDocumentScroll: vi.fn() };
  const page = ReadOnlyPage(props);
  const scrollTo = vi.fn(); (page.props.ref as { current: unknown }).current = { scrollTo };
  const call = (key: string, ...args: unknown[]) => (page.props[key] as (...args: unknown[]) => void)?.(...args);
  return { page, props, scrollTo,
    layout: (height = 640) => call("onLayout", { nativeEvent: { layout: { height } } }),
    size: (height: number) => call("onContentSizeChange", 400, height),
    scroll: (y: number) => call("onScroll", { nativeEvent: { contentOffset: { y } } }),
    drag: () => call("onScrollBeginDrag"),
    toggle: () => {
      const action = expand(page).find((element) => element.props.accessibilityLabel === "Configured bindings (38)")!;
      expect(action).toBeDefined(); (action.props.onPress as () => void)();
    },
  };
}
beforeEach(async () => {
  model.open = false;
  snapshot = await createTeamsSourceAdapter(catalog, { authoring: "fleet-authoring" }).read({ sourceId: "authoring" });
  expect(snapshot.status).toBe("ready");
});
describe("outer read-only page disclosure scrolling (callback interaction, not native QA)", () => {
  it.each([[0, false], [0, true], [120, false], [120, true]] as const)("does not restore stale initial%s after disclosure expansion/collapse at800 (drag: %s)", (initial, drag) => {
    const hostId = `outer-disclosure-${initial}-${drag}`;
    mount(hostId).scroll(initial);
    const screen = mount(hostId); screen.layout(); screen.size(3000); screen.scroll(initial); screen.scrollTo.mockClear();
    if (drag) screen.drag();
    screen.scroll(800); expect(readRouteView(hostId).offset).toBe(800);
    screen.toggle(); expect(model.open).toBe(true); screen.size(4500);
    screen.toggle(); expect(model.open).toBe(false); screen.size(3000);
    expect(screen.scrollTo).not.toHaveBeenCalled();
    expect(readRouteView(hostId).offset).toBe(800);
    navigate(hostId, { tab: "Overview" }); goBack(hostId);
    const returned = mount(hostId); returned.layout(); returned.size(3000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 800, animated: false });
  });
  it("clamps initial/re-entry restoration, waits for measurement and restores the reachable saved target", () => {
    const hostId = "outer-clamp-grow"; const first = mount(hostId);
    first.scroll(900); const screen = mount(hostId); screen.scroll(0); screen.size(1000);
    expect(screen.scrollTo).not.toHaveBeenCalled(); screen.layout();
    expect(screen.scrollTo).toHaveBeenLastCalledWith({ y: 360, animated: false }); screen.scroll(360);
    expect(readRouteView(hostId).offset).toBe(900);
    screen.size(3000); screen.scroll(900); screen.size(4000);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 360, animated: false }], [{ y: 900, animated: false }]]);
    screen.scroll(800); screen.size(5000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(2); expect(readRouteView(hostId).offset).toBe(800);
  });
  it("releases a clamped restoration for wheel scrolling without a drag callback and retains scope", () => {
    const hostId = "outer-clamp-wheel"; mount(hostId).scroll(900);
    const screen = mount(hostId); screen.layout(); screen.size(1000); screen.scroll(360); screen.scroll(120);
    expect(readRouteView(hostId).offset).toBe(120); screen.size(5000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(1);
    navigate(hostId, { tab: "Overview" }); goBack(hostId);
    const returned = mount(hostId); returned.layout(); returned.size(1000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 120, animated: false });
    navigate(hostId, { sourceRevision: "sha-b" }); expect(readRouteView(hostId).offset).toBe(0);
    goBack(hostId); expect(readRouteView(hostId).offset).toBe(120);
    expect(mount(`${hostId}-other`).props.initialOffset).toBe(0);
  });
});
