// QA1's frozen-c835 reproduction, ported unchanged except the local fixture catalog.
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
import { TeamsSourcePage } from '../client/teams-source-page';
import { goBack, navigate, readRouteView, writeRouteView } from '../client/navigation-state';
type Props = Parameters<typeof ReadOnlyPage>[0];
type Element = ReactElement<Record<string, unknown>>;
const catalog = sourceCatalog(JSON.stringify([
  ['authoring', 'authoring-fleet.yml', 'f7ddfff7a0db716e3356d43fe209c9c73f4fe51866de5ebbc26f641968fbea54'],
  ['operator', 'operator-fleet.yml', 'e5040c5d74dbc131338b1568d0c0ada499eb31e55bafc512b4b1d883e578d3a3'],
  ['roster', 'core-team.yml', '329f5fb70d34cb51409f32588533c1b200bd5bb434341a35d9d66fa142fd6cfc'],
  ['templates', 'mvp-team-pack.json', 'd4b0965db9be03de42cf9aec25f0dd3c43c9c0493d6b2378b6fb8e4d82f8ee5d'],
].map(([id, filename, expectedSha256]) => ({ id, title: id, kind: 'teams', path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)), expectedSha256 }))));
let snapshots: Record<string, TeamsSourceSnapshot>;
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
    if (node.type.name === 'Disclosure') return [node];
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
    initialOffset: initial, onScroll: (offset) => writeRouteView(hostId, route, { offset }), initialDocumentOffset: 0, onDocumentScroll() {},
  };
  const outer = ReadOnlyPage(props);
  let current = 0, height = 2400;
  const viewport = 640;
  const emitScroll = (y: number) => { current = Math.max(0, Math.min(y, Math.max(0, height - viewport))); (outer.props.onScroll as (event: object) => void)({ nativeEvent: { contentOffset: { y: current } } }); };
  const scrollTo = vi.fn(({ y }: { y: number }) => emitScroll(y));
  (outer.props.ref as { current: unknown }).current = { scrollTo };
  const teams = elements(outer).find((element) => element.type === TeamsSourcePage)!;
  const teamTree = TeamsSourcePage(teams.props as Parameters<typeof TeamsSourcePage>[0]);
  const chain = disclosures(teamTree).find((element) => element.props.label === 'Source details')!;
  expect(chain).toBeDefined();
  function renderDisclosure() {
    hooks.active = hostId + ':authoring-source-details';
    return (chain.type as (props: object) => ReactNode)(chain.props);
  }
  return { initial, scrollTo, current: () => current, scroll: emitScroll,
    size(next: number) { height = next; (outer.props.onContentSizeChange as (width: number, height: number) => void)(400, next); },
    setOpen(value: boolean) { hooks.states.set(hostId + ':authoring-source-details', value); },
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
