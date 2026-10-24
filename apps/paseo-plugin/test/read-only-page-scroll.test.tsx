import { isValidElement, type ReactElement, type ReactNode } from "react";
import { fileURLToPath, URL } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";
const model = vi.hoisted(() => ({ view: null as unknown }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useContext: () => null, useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, vi.fn()] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
import { goBack, navigate, readRouteView, writeRouteView } from "../client/navigation-state";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { DEFAULT_TEAMS_VIEW, type TeamsView } from "../client/teams-source-page";
// The disclosure state lives in the saved Teams view; configured sources start open here.
const view = () => model.view as TeamsView;
const bindingsOpen = () => view().sources.authoring?.bindings ?? false;
const catalog = sourceCatalog(JSON.stringify([{ id: "authoring", title: "Authoring", kind: "teams", path: fileURLToPath(new URL("./fixtures/teams-source/authoring-fleet.yml", import.meta.url)), expectedSha256: "fa82d7a49d12a01a30f0a3b939d605aecf04d784fb80e53b88fcc99ff1f211f7" }]));
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
  const props: Props = { tab: route.tab, teamsSources: catalog.sources, teamsSnapshots: { authoring: snapshot }, teamsAvailable: true, colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: [], reads: {}, sourceId: route.sourceId, sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), teamsView: view(), onTeamsView: (next) => { model.view = next; }, initialOffset: saved.offset, onScroll: (offset) => writeRouteView(hostId, route, { offset })};
  const page = ReadOnlyPage(props);
  const scrollTo = vi.fn(); (page.props.ref as { current: unknown }).current = { scrollTo };
  const call = (key: string, ...args: unknown[]) => (page.props[key] as (...args: unknown[]) => void)?.(...args);
  return { page, props, scrollTo,
    layout: (height = 640) => call("onLayout", { nativeEvent: { layout: { height } } }),
    size: (height: number) => call("onContentSizeChange", 400, height),
    scroll: (y: number) => call("onScroll", { nativeEvent: { contentOffset: { y } } }),
    drag: () => call("onScrollBeginDrag"),
    toggle: () => {
      // Re-render for the current view; the outer scroller keeps its first instance.
      const action = expand(ReadOnlyPage({ ...props, teamsView: view() })).find((element) => element.props.accessibilityLabel === "Configured bindings (40)")!;
      expect(action).toBeDefined(); (action.props.onPress as () => void)();
    },
  };
}
beforeEach(async () => {
  model.view = { ...DEFAULT_TEAMS_VIEW, configured: true };
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
    screen.toggle(); expect(bindingsOpen()).toBe(true); screen.size(4500);
    screen.toggle(); expect(bindingsOpen()).toBe(false); screen.size(3000);
    expect(screen.scrollTo).not.toHaveBeenCalled();
    expect(readRouteView(hostId).offset).toBe(800);
    navigate(hostId, { tab: "Overview" }); goBack(hostId);
    const returned = mount(hostId); returned.layout(); returned.size(3000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 800, animated: false });
  });
  it("restores once after positive content measurement and saves the host-clamped outer offset", () => {
    const hostId = "outer-clamp-grow"; const first = mount(hostId);
    first.scroll(900); const screen = mount(hostId); screen.scroll(0); screen.size(0);
    expect(screen.scrollTo).not.toHaveBeenCalled(); screen.size(1000);
    expect(screen.scrollTo).toHaveBeenLastCalledWith({ y: 900, animated: false }); screen.scroll(360);
    expect(readRouteView(hostId).offset).toBe(360);
    screen.size(3000); screen.size(4000);
    expect(screen.scrollTo.mock.calls).toEqual([[{ y: 900, animated: false }]]);
    navigate(hostId, { tab: "Overview" }); goBack(hostId);
    const returned = mount(hostId); returned.size(1000);
    expect(returned.scrollTo).toHaveBeenLastCalledWith({ y: 360, animated: false });
    screen.scroll(800); screen.size(5000);
    expect(screen.scrollTo).toHaveBeenCalledTimes(1); expect(readRouteView(hostId).offset).toBe(800);
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
