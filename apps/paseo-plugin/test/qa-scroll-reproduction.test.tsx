// QA1's frozen-c835 reproduction, ported unchanged except the local fixture catalog and
// the Teams view store: configured sources now open from a saved TeamsView, so the
// disclosure state lives there instead of in Disclosure-local state.
// Native layout is not observed: this harness drives real disclosure callbacks
// and explicitly supplies content-size events plus a range-clamping scroll model.
import { fileURLToPath, URL } from 'node:url';
import { isValidElement, type ReactElement, type ReactNode } from 'react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { sourceCatalog } from '../server/read-sources';
import { createTeamsSourceAdapter } from '../server/teams-source-adapter';
import { teamsSourceBindings } from '../shared/teams-source-bindings';
import type { TeamsSourceSnapshot } from '../shared/teams-source-snapshot';
const hooks = vi.hoisted(() => ({ active: '', states: new Map<string, unknown>() }));
vi.mock('react', async (original) => ({ ...await original<typeof import('react')>(),
  useRef: (current: unknown) => ({ current }), useMemo: (factory: () => unknown) => factory(),
  useState: (initial: unknown) => {
    const key = hooks.active;
    if (!hooks.states.has(key)) hooks.states.set(key, initial);
    return [hooks.states.get(key), (value: unknown) => hooks.states.set(key, value)];
  },
}));
vi.mock('react-native', () => ({ ScrollView: 'ScrollView', View: 'View', Text: 'Text', Pressable: 'Pressable' }));
vi.mock('@getpaseo/plugin/client/react-native', () => ({ TextInput: 'TextInput', Icon: 'Icon' }));
import { ReadOnlyPage } from '../client/read-only-pages';
import { DEFAULT_TEAMS_SOURCE_VIEW, DEFAULT_TEAMS_VIEW, TeamsSourcePage, type TeamsView } from '../client/teams-source-page';
import { goBack, navigate, readRouteView, writeRouteView } from '../client/navigation-state';
type Props = Parameters<typeof ReadOnlyPage>[0];
type Element = ReactElement<Record<string, unknown>>;
const catalog = sourceCatalog(JSON.stringify([
  ['authoring', 'authoring-fleet.yml', '5e49c621a5ed28a181f7e5f7c5c7c67f7bdc09c8d1258e94d48c60988fa5cff4'],
  ['operator', 'operator-fleet.yml', 'd0fd421cab74df1346a6fb3c96ae2d85814af7f6336c40fdcd4c95906dab2101'],
  ['roster', 'core-team.yml', '329f5fb70d34cb51409f32588533c1b200bd5bb434341a35d9d66fa142fd6cfc'],
  ['templates', 'mvp-team-pack.json', '2a5a0aed9c23450f984fd3d1f92c5824f4640a71b2bc9697349b0632fed8b3cc'],
].map(([id, filename, expectedSha256]) => ({ id, title: id, kind: 'teams', path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)), expectedSha256 }))));
let snapshots: Record<string, TeamsSourceSnapshot>;
const views = new Map<string, TeamsView>();
beforeAll(async () => {
  const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
  snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
  expect(Object.values(snapshots).every((snapshot) => snapshot.status === 'ready')).toBe(true);
});
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  return isValidElement<Record<string, unknown>>(node) ? [node, ...elements(node.props.children as ReactNode)] : [];
}
function disclosures(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(disclosures);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === 'function') {
    if (node.type.name === 'Disclosure') return [node, ...disclosures(node.props.children as ReactNode)];
    return disclosures((node.type as (props: object) => ReactNode)(node.props));
  }
  return disclosures(node.props.children as ReactNode);
}
function mount(hostId: string) {
  const route = navigate(hostId, { tab: 'AI teams & sessions' });
  const initial = readRouteView(hostId, route).offset;
  const props: Props = { tab: 'AI teams & sessions', colors: {} as Props['colors'], entries: [],
    directory: { entries: [], status: 'ready', live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false },
    sources: catalog.sources, reads: {}, sourceId: null, sourceError: null, query: '', setQuery() {}, onSource() {}, onOpen() {}, onTab() {}, onEpicTab() {},
    teamsSources: catalog.sources, teamsSnapshots: snapshots, teamsAvailable: true,
    initialOffset: initial, onScroll: (offset) => writeRouteView(hostId, route, { offset }),
    teamsView: views.get(hostId) ?? { ...DEFAULT_TEAMS_VIEW, configured: true }, onTeamsView: (view) => views.set(hostId, view),
  };
  const outer = ReadOnlyPage(props);
  let current = 0, height = 2400;
  const viewport = 640;
  const emitScroll = (y: number) => { current = Math.max(0, Math.min(y, Math.max(0, height - viewport))); (outer.props.onScroll as (event: object) => void)({ nativeEvent: { contentOffset: { y: current } } }); };
  const scrollTo = vi.fn(({ y }: { y: number }) => emitScroll(y));
  (outer.props.ref as { current: unknown }).current = { scrollTo };
  // Re-render only the Teams page for each toggle; the outer scroller keeps its first instance.
  const chain = () => {
    const teams = elements(ReadOnlyPage({ ...props, teamsView: views.get(hostId) ?? props.teamsView })).find((element) => element.type === TeamsSourcePage)!;
    const found = disclosures(TeamsSourcePage(teams.props as Parameters<typeof TeamsSourcePage>[0])).find((element) => element.props.label === 'Source details')!;
    expect(found).toBeDefined(); return found;
  };
  chain();
  function renderDisclosure() {
    hooks.active = hostId + ':authoring-source-details';
    const disclosure = chain();
    return (disclosure.type as (props: object) => ReactNode)(disclosure.props);
  }
  return { initial, scrollTo, current: () => current, scroll: emitScroll,
    size(next: number) { height = next; (outer.props.onContentSizeChange as (width: number, height: number) => void)(400, next); },
    setOpen(value: boolean) {
      const view = views.get(hostId) ?? props.teamsView!;
      views.set(hostId, { ...view, sources: { ...view.sources, authoring: { ...DEFAULT_TEAMS_SOURCE_VIEW, ...view.sources.authoring, details: value } } });
    },
    toggle() {
      const before = renderDisclosure();
      const button = elements(before).find((element) => element.props.accessibilityLabel === 'Source details')!;
      (button.props.onPress as () => void)();
      const after = renderDisclosure();
      const changed = elements(after).find((element) => element.props.accessibilityLabel === 'Source details')!;
      return (changed.props.accessibilityState as { expanded: boolean }).expanded;
    },
  };
}
describe('QA1 LSA-SCROLL-07 reproduction on integration head', () => {
  it.each([{initial:0, collapsing:false}, {initial:180, collapsing:false}, {initial:0, collapsing:true}, {initial:180, collapsing:true}])
    ('keeps a reachable user offset after disclosure resize: %j', ({ initial, collapsing }) => {
      const host = 'lsa-scroll-' + initial + '-' + collapsing;
      const route = navigate(host, { tab: 'AI teams & sessions' }); writeRouteView(host, route, { offset: initial });
      const screen = mount(host); screen.setOpen(collapsing); screen.size(collapsing ? 4000 : 2400);
      screen.scroll(800); expect(readRouteView(host).offset).toBe(800);
      screen.scrollTo.mockClear();
      expect(screen.toggle()).toBe(!collapsing);
      screen.size(collapsing ? 2400 : 4000); // 800 remains reachable in both geometries.
      console.log(JSON.stringify({ initialOffset:screen.initial, savedBeforeResize:800, action:collapsing?'collapse':'expand', requestedAfterResize:screen.scrollTo.mock.calls, modeledAfterResize:screen.current(), savedAfterResize:readRouteView(host).offset }));
      expect(screen.current()).toBe(800);
      expect(readRouteView(host).offset).toBe(800);
    });
  it('restores initial and Back/re-entry offsets when the saved offset is reachable', () => {
    const host = 'lsa-scroll-reentry'; const route = navigate(host, { tab: 'AI teams & sessions' }); writeRouteView(host, route, { offset: 250 });
    const first = mount(host); expect(first.current()).toBe(0); first.size(2400);
    expect(first.scrollTo).toHaveBeenLastCalledWith({ y:250, animated:false }); expect(first.current()).toBe(250);
    first.scroll(800); navigate(host, { tab:'Overview' }); goBack(host);
    const returned = mount(host); returned.size(2400);
    expect(returned.initial).toBe(800); expect(returned.current()).toBe(800); expect(readRouteView(host).offset).toBe(800);
  });
  it('lets the simulated host clamp an oversized outer restore, preserving a valid re-entry offset', () => {
    const host = 'lsa-scroll-range'; const route = navigate(host, { tab:'AI teams & sessions' }); writeRouteView(host, route, { offset:900 });
    const first = mount(host); first.size(1000);
    expect(first.scrollTo).toHaveBeenLastCalledWith({ y:900, animated:false });
    expect(first.current()).toBe(360); expect(readRouteView(host).offset).toBe(360);
    navigate(host, { tab:'Overview' }); goBack(host);
    const returned = mount(host); returned.size(1000);
    expect(returned.initial).toBe(360); expect(returned.current()).toBe(360);
  });
});
