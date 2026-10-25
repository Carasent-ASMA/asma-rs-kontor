// Preserve QA1's two integrated Settings reproductions; catalog paths use local hashed fixtures.
import { fileURLToPath, URL } from 'node:url';
import { isValidElement, type ReactElement, type ReactNode } from 'react';
import type { PluginScreenProps } from '@getpaseo/plugin/client';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { sourceCatalog } from '../server/read-sources';
import { createTeamsSourceAdapter } from '../server/teams-source-adapter';
import { teamsSourceBindings } from '../shared/teams-source-bindings';
import type { TeamsSourceSnapshot } from '../shared/teams-source-snapshot';
import type { SourceRead } from '../shared/read-sources';
const model = vi.hoisted(() => ({ sources: [] as unknown[], snapshots: {} as Record<string, unknown>, errors: {} as Record<string, string>, reads: {} as Record<string, SourceRead>, available: true, sourceError: null as string | null, details: false }));
vi.mock('react', async (original) => ({ ...await original<typeof import('react')>(), useContext: () => null, useMemo: (factory: () => unknown) => factory(), useCallback: (value: unknown) => value, useEffect() {}, useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [typeof initial === 'boolean' ? model.details : typeof initial === 'function' ? initial() : initial, () => {}] }));
vi.mock('react-native', () => ({ ScrollView: 'ScrollView', SafeAreaView: 'SafeAreaView', KeyboardAvoidingView: 'KeyboardAvoidingView', View: 'View', Text: 'Text', Pressable: 'Pressable' }));
vi.mock('@getpaseo/plugin/client', () => ({ useHosts: () => [{ serverId: 'qa-settings-host', status: 'online' }] }));
vi.mock('@getpaseo/plugin/client/react-native', () => ({ TextInput: 'TextInput', ScrollView: 'ScrollView', FlatList: 'FlatList', Icon: 'Icon', Modal: Object.assign(() => null, { Content: () => null }), useToast: () => ({ error() {} }) }));
vi.mock('../client/use-responsibility', () => ({ useResponsibility: () => null }));
vi.mock('../client/use-kontor-status', () => ({ useKontorStatus: () => null }));
vi.mock('../client/use-work-catalog', () => ({ useWorkCatalog: () => null }));
vi.mock('../client/use-read-sources', () => ({ useReadSources: () => ({ sources: model.sources, reads: model.reads, teamsSnapshots: model.snapshots, teamsErrors: model.errors, teamsBindingDiagnostics: [], error: model.sourceError, available: model.available, loading: false, demand: () => {}, refresh: async () => {} }) }));
vi.mock('../client/use-session-directory', () => ({ useSessionDirectory: () => ({ state: { entries: [], status: 'ready', live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false } }) }));
vi.mock('../client/use-workspace-directory', () => ({ useWorkspaceDirectory: () => ({ entries: [], error: null, readAt: null }) }));
vi.mock('../client/use-organization-settings', () => ({ useOrganizationSettings: () => ({ state: { status: 'ready', values: DEFAULT_ORGANIZATION_SETTINGS, revision: 'qa-rev', saving: false, saveError: null } }) }));
vi.mock('../client/use-keyboard-settings', () => ({ useKeyboardSettings: () => ({ state: { status: 'loading' }, save: async () => true }) }));
import { DEFAULT_ORGANIZATION_SETTINGS } from '../shared/organization';
import { KontorScreen } from '../client/kontor-screen';
import { OrganizationSettingsPage } from '../client/organization-settings-page';
import { navigate } from '../client/navigation-state';
const catalog = sourceCatalog(JSON.stringify([
  ['authoring', 'authoring-fleet.yml', 'fa82d7a49d12a01a30f0a3b939d605aecf04d784fb80e53b88fcc99ff1f211f7'],
  ['operator', 'operator-fleet.yml', '5349f628b9de00e9f62ef9d69b4e8977d3c6a28c582f3b5497ed961c77060487'],
  ['roster', 'core-team.yml', '329f5fb70d34cb51409f32588533c1b200bd5bb434341a35d9d66fa142fd6cfc'],
  ['templates', 'mvp-team-pack.json', '2a5a0aed9c23450f984fd3d1f92c5824f4640a71b2bc9697349b0632fed8b3cc'],
].map(([id, filename, expectedSha256]) => ({ id, title: id, kind: 'teams', path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)), expectedSha256 }))));
let snapshots: Record<string, TeamsSourceSnapshot>;
beforeAll(async () => {
  const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
  snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
  expect(Object.values(snapshots).every((value) => value.status === 'ready')).toBe(true);
  model.sources = catalog.sources;
});
beforeEach(() => {
  model.sources = catalog.sources; model.snapshots = snapshots; model.errors = {}; model.reads = {};
  model.available = true; model.sourceError = null; model.details = false;
});
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  return isValidElement<Record<string, unknown>>(node) ? [node, ...elements(node.props.children as ReactNode)] : [];
}
function text(node: ReactNode): string {
  if (typeof node === 'string' || typeof node === 'number') return String(node);
  if (Array.isArray(node)) return node.map(text).join('');
  if (!isValidElement<Record<string, unknown>>(node)) return '';
  if (typeof node.type === 'function') return text((node.type as (props: object) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function labels(node: ReactNode): string[] {
  if (Array.isArray(node)) return node.flatMap(labels);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === 'function') return labels((node.type as (props: object) => ReactNode)(node.props));
  const own = typeof node.props.accessibilityLabel === 'string' ? [node.props.accessibilityLabel] : [];
  return [...own, ...labels(node.props.children as ReactNode)];
}
function settingsLabels() {
  const props: PluginScreenProps = { host: { id: 'qa-settings-host', label: 'QA' }, params: {}, layout: { compact: true, platform: 'ios' }, theme: { colors: {} } as PluginScreenProps['theme'] };
  navigate(props.host.id, { tab: 'Settings' });
  const outer = KontorScreen(props);
  const page = elements((outer.type as (props: PluginScreenProps) => ReactNode)(outer.props as PluginScreenProps)).find((element) => element.type === OrganizationSettingsPage)!;
  return labels(OrganizationSettingsPage(page.props as Parameters<typeof OrganizationSettingsPage>[0])).join('\n');
}
function settingsText() {
  const props: PluginScreenProps = { host: { id: 'qa-settings-host', label: 'QA' }, params: {}, layout: { compact: true, platform: 'ios' }, theme: { colors: {} } as PluginScreenProps['theme'] };
  navigate(props.host.id, { tab: 'Settings' });
  const outer = KontorScreen(props);
  const host = outer.type as (props: PluginScreenProps) => ReactNode;
  const page = elements(host(outer.props as PluginScreenProps)).find((element) => element.type === OrganizationSettingsPage)!;
  expect(page).toBeDefined();
  return text(OrganizationSettingsPage(page.props as Parameters<typeof OrganizationSettingsPage>[0]));
}
describe('QA Settings must display actual Teams source state', () => {
  it('does not label four successfully read authorized sources as Not read', () => {
    model.snapshots = snapshots; model.errors = {};
    const actual = settingsText();
    console.log('Snapshots:', Object.fromEntries(Object.entries(snapshots).map(([id,snapshot]) => [id,snapshot.status])));
    console.log('Settings visible source status:', actual.slice(actual.indexOf('Read-only source catalog available.')));
    expect(actual).not.toContain('authoring · Not read');
  });
  it('shows a failed adapter request instead of permanently reporting Not read', () => {
    model.snapshots = { ...snapshots }; delete model.snapshots.operator; model.errors = { operator: 'operator unavailable: source-read failure' };
    const actual = settingsText();
    console.log('Retained per-source request error:', model.errors.operator);
    console.log('Settings visible source status:', actual.slice(actual.indexOf('Read-only source catalog available.')));
    expect(actual).toContain('operator unavailable: source-read failure');
  });
  it('forwards all four real ready states and read times without inventing observed time or activation', () => {
    // One visible line per source; the state, read time, observed time and activation are in its label and Source details.
    const actual = settingsText(); const label = settingsLabels();
    for (const [id, snapshot] of Object.entries(snapshots)) {
      expect(label).toContain(`${id} · ready · ${snapshot.configurationState}. Read ${snapshot.readAt} · observed time Unknown · activation: not-attested`);
      expect(actual).not.toContain(`${id} · Not read`); expect(actual).not.toContain(snapshot.readAt);
      expect(actual).not.toContain(snapshot.source.path); expect(actual).not.toContain(snapshot.sha256);
    }
    expect(actual).not.toContain('retained snapshot stale'); expect(actual).not.toContain('kontord on this host');
    model.details = true;
    const expanded = settingsText();
    expect(expanded).toContain('team activation: not shown here. kontord on this host: unknown.');
    for (const [id, snapshot] of Object.entries(snapshots)) {
      expect(expanded).toContain(`${id} · ready · ${snapshot.configurationState}`); expect(expanded).toContain(snapshot.readAt);
    }
    expect(expanded.match(/observed time Unknown/g)).toHaveLength(4);
    expect(expanded.match(/activation: not-attested/g)).toHaveLength(4);
  });
  it.each(['partial', 'unavailable'] as const)('shows the actual %s snapshot and expandable provenance/diagnostics', (status) => {
    model.snapshots = { ...snapshots, operator: { ...snapshots.operator!, status, data: status === 'unavailable' ? null : snapshots.operator!.data, diagnostics: [{ code: 'read-probe', pointer: '/chains/probe', message: 'Definition unavailable in this snapshot' }] } };
    expect(settingsText()).toContain(`operator · ${status}`);
    expect(settingsText()).not.toContain('Definition unavailable in this snapshot'); model.details = true;
    const expanded = settingsText();
    expect(expanded).toContain(snapshots.operator!.source.path); expect(expanded).toContain(snapshots.operator!.sha256);
    expect(expanded).toContain('read-probe · /chains/probe: Definition unavailable in this snapshot');
  });
  it('shows request failure separately from a retained ready snapshot and marks offline snapshots stale', () => {
    model.errors = { operator: 'operator unavailable: rejected request' };
    let actual = settingsText();
    expect(actual).toContain('operator · ready · retained snapshot stale'); expect(actual).toContain('Adapter request failed: operator unavailable: rejected request');
    model.errors = {}; model.available = false; model.sourceError = 'Paseo host offline. Retained reads are stale.';
    actual = settingsText(); expect(actual).toContain(model.sourceError);
    for (const id of Object.keys(snapshots)) expect(actual).toContain(`${id} · ready · retained snapshot stale`);
  });
  it('preserves separate legacy JSON source states and a genuinely unread mapped source', () => {
    const legacy = { ...catalog.sources[0]!, id: 'legacy-json', title: 'Legacy JSON' };
    model.sources = [...catalog.sources, legacy]; model.snapshots = {};
    model.reads = { 'legacy-json': { source: legacy, status: 'unavailable', readAt: '2026-10-19T09:00:00Z', modifiedAt: null, revision: null, content: null, graph: null, records: null, diagnostics: ['JSON parse failed'] } };
    let actual = settingsText();
    expect(actual).toContain('Legacy JSON · unavailable'); expect(actual).toContain('authoring · Not read');
    expect(actual).not.toContain('JSON parse failed'); model.details = true;
    actual = settingsText(); expect(actual).toContain('JSON parse failed'); expect(actual).toContain('2026-10-19T09:00:00Z');
  });
});
