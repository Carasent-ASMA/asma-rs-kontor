import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import type { ReadSourceSummary } from "../shared/read-sources";

const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, entries: [] as DirectoryEntry[], sources: [] as ReadSourceSummary[], values: {} as object,
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
  },
}));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(), useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useMemo: (factory: () => unknown) => factory(), useCallback: (callback: unknown) => callback,
  useEffect: () => {},
}));
vi.mock("react-native", () => ({
  SafeAreaView: "SafeAreaView", KeyboardAvoidingView: "KeyboardAvoidingView", ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable",
}));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => [] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  Modal: Object.assign(() => null, { Content: () => null }), ScrollView: "ScrollView", FlatList: "FlatList", TextInput: "TextInput", Icon: "Icon", useToast: () => ({ error: () => {} }),
}));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: { entries: model.entries, status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false }, refresh: () => {} }) }));
vi.mock("../client/use-organization-settings", () => ({ useOrganizationSettings: () => ({ state: { status: "ready", values: model.values, saving: false, saveError: null }, save: async () => true }) }));
vi.mock("../client/use-read-sources", () => ({ useReadSources: () => ({ sources: model.sources, reads: {}, error: null, loading: false, available: true }) }));
vi.mock("../client/use-workspace-directory", () => ({ useWorkspaceDirectory: () => ({ entries: [], error: null, readAt: null }) }));

import { KontorScreen } from "../client/kontor-screen";
import { DESTINATIONS, hostNavigation, navigate, type KontorRoute } from "../client/navigation-state";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
function elements(value: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  return [value, ...elements(value.props.children as ReactNode)];
}
let sequence = 0;
beforeEach(() => { model.slots = []; model.cursor = 0; model.values = DEFAULT_ORGANIZATION_SETTINGS; model.entries = [sessionEntry("native", { labels: { epic: "ASMA-1" } })]; model.sources = []; });
function mount(compact: boolean, initial?: Partial<KontorRoute>) {
  const props: PluginScreenProps = { host: { id: `shell-${++sequence}`, label: "Host" }, layout: { compact, platform: "ios" }, params: {}, navigation: { openAgent: vi.fn(), openWorkspace: vi.fn(), openBrowser: vi.fn() },
    theme: { colors: { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } } };
  if (initial) navigate(props.host.id, initial);
  const entry = KontorScreen(props);
  const component = entry.type as (props: PluginScreenProps) => ReactNode;
  let tree: ReactNode;
  const render = () => { model.cursor = 0; tree = component(props); };
  render();
  return {
    props, render,
    action(label: string) { const action = elements(tree).find((element) => element.props.label === label); if (!action) throw new Error(`Missing action ${label}`); return action; },
    click(label: string) { const action = this.action(label); const press = action.props.onPress; expect(typeof press).toBe("function"); if (typeof press === "function") press(); render(); },
    elements() { return elements(tree); },
  };
}
describe("Kontor shell native actions", () => {
  it("keeps the native conversation and its association boundary while choosing a linked plan", () => {
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-a", "linked-b"] } };
    model.sources = ["linked-a", "linked-b", "unrelated"].map((id) => ({ id, title: id, kind: "plan", workstream: null, epic: null, projectId: null, path: `/configured/${id}.md`, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", originAgentId: "native" });
    const page = () => screen.elements().find((element) => typeof element.props.onSource === "function")!;
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-a", "linked-b"]);
    (page().props.onSource as (id: string) => void)("linked-b"); screen.render();
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ sourceId: "linked-b", originAgentId: "native" });
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-a", "linked-b"]);
    screen.click("Return to conversation");
    expect(screen.props.navigation?.openAgent).toHaveBeenCalled();
  });
  it("wires the inner document position independently from the outer Plan page through Back", () => {
    const screen = mount(true, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    const page = () => screen.elements().find((element) => typeof element.props.onDocumentScroll === "function")!;
    (page().props.onDocumentScroll as (offset: number) => void)(900);
    (page().props.onScroll as (offset: number) => void)(120);
    screen.click("Overview"); screen.click("‹ Back");
    expect(page().props.initialDocumentOffset).toBe(900);
    expect(page().props.initialOffset).toBe(120);
    (page().props.onSource as (id: string) => void)("plan-b"); screen.render();
    expect(page().props.initialDocumentOffset).toBe(0);
    screen.click("‹ Back"); expect(page().props.initialDocumentOffset).toBe(900);
  });
  it("reaches every destination on compact layouts through four tabs and More", () => {
    const screen = mount(true);
    const primary = screen.elements().filter((element) => element.props.tab === true);
    expect(primary.map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board", "More"]);
    expect(primary.every((element) => element.props.stacked === true)).toBe(true);
    for (const tab of DESTINATIONS) {
      if (!["Sessions", "Overview", "Plan", "Board"].includes(tab)) screen.click("More");
      screen.click(tab);
      expect(hostNavigation(screen.props.host.id).route.tab).toBe(tab);
    }
  });
  it("keeps Graph and Delivery directly accessible on desktop with the remaining pages in More", () => {
    const screen = mount(false);
    expect(screen.elements().filter((element) => element.props.tab === true).map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery", "More"]);
    screen.click("Graph"); expect(hostNavigation(screen.props.host.id).route.tab).toBe("Graph");
    screen.click("More"); screen.click("Inbox"); expect(screen.action("More").props.selected).toBe(true);
  });
  it("filters by confirmed epic context, returns with Back, and keeps Overview global", () => {
    const screen = mount(true);
    screen.click("All projects ▾"); screen.click("ASMA-1");
    expect(hostNavigation(screen.props.host.id).route.epicKey).toBe("ASMA-1");
    screen.click("‹ Back"); expect(hostNavigation(screen.props.host.id).route.epicKey).toBeNull();
    screen.click("All projects ▾"); screen.click("ASMA-1"); screen.click("Overview");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ projectId: null, epicKey: null, tab: "Overview" });
  });
});
