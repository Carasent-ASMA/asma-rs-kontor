import type { PluginTheme } from "@getpaseo/plugin";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import type {
  NativeScrollEvent,
  NativeSyntheticEvent,
} from "react-native";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";

type HookSlot = {
  initialized?: boolean;
  value?: unknown;
  deps?: readonly unknown[];
  cleanup?: () => void;
};

type HookQueueItem = {
  readonly slot: HookSlot;
  readonly fn: () => void | (() => void);
  readonly deps?: readonly unknown[];
};

const accessibility = vi.hoisted(() => ({ screenReader: false }));
const { harness, directory } = vi.hoisted(() => {
  const slots: HookSlot[] = [];
  let cursor = 0;
  let dirty = false;
  let component: (() => unknown) | null = null;
  let tree: unknown = null;
  let layoutQueue: HookQueueItem[] = [];
  let effectQueue: HookQueueItem[] = [];

  function sameDeps(
    left: readonly unknown[] | undefined,
    right: readonly unknown[] | undefined,
  ): boolean {
    if (left === undefined || right === undefined) {
      return false;
    }
    return (
      left.length === right.length &&
      left.every((value, index) => Object.is(value, right[index]))
    );
  }

  function nextSlot(): HookSlot {
    const index = cursor;
    cursor += 1;
    const existing = slots[index];
    if (existing) {
      return existing;
    }
    const slot: HookSlot = {};
    slots[index] = slot;
    return slot;
  }

  function useState(initial: unknown): [unknown, (next: unknown) => void] {
    const slot = nextSlot();
    if (!slot.initialized) {
      slot.value =
        typeof initial === "function" ? (initial as () => unknown)() : initial;
      slot.initialized = true;
    }
    const setValue = (next: unknown) => {
      const value =
        typeof next === "function"
          ? (next as (previous: unknown) => unknown)(slot.value)
          : next;
      if (!Object.is(value, slot.value)) {
        slot.value = value;
        dirty = true;
      }
    };
    return [slot.value, setValue];
  }

  function useRef(initial: unknown): { current: unknown } {
    const slot = nextSlot();
    if (!slot.initialized) {
      slot.value = { current: initial };
      slot.initialized = true;
    }
    return slot.value as { current: unknown };
  }

  function useMemo(
    factory: () => unknown,
    deps?: readonly unknown[],
  ): unknown {
    const slot = nextSlot();
    if (!slot.initialized || !sameDeps(slot.deps, deps)) {
      slot.value = factory();
      slot.deps = deps;
      slot.initialized = true;
    }
    return slot.value;
  }

  function useCallback<Fn>(fn: Fn, _deps?: readonly unknown[]): Fn {
    nextSlot();
    return fn;
  }

  function useEffect(
    fn: () => void | (() => void),
    deps?: readonly unknown[],
  ): void {
    effectQueue.push({ slot: nextSlot(), fn, deps });
  }

  function useLayoutEffect(
    fn: () => void | (() => void),
    deps?: readonly unknown[],
  ): void {
    layoutQueue.push({ slot: nextSlot(), fn, deps });
  }

  function runQueue(queue: HookQueueItem[]): void {
    for (const item of queue) {
      if (item.deps !== undefined && sameDeps(item.slot.deps, item.deps)) {
        continue;
      }
      item.slot.cleanup?.();
      const cleanup = item.fn();
      item.slot.deps = item.deps;
      item.slot.cleanup =
        typeof cleanup === "function" ? cleanup : undefined;
    }
  }

  function renderTree(): unknown {
    if (!component) {
      throw new Error("No component registered");
    }
    let guard = 0;
    do {
      dirty = false;
      cursor = 0;
      layoutQueue = [];
      effectQueue = [];
      tree = component();
      runQueue(layoutQueue);
      runQueue(effectQueue);
      guard += 1;
    } while (dirty && guard < 20);
    return tree;
  }

  return {
    harness: {
      render(next: () => unknown) {
        component = next;
        return renderTree();
      },
      flush() {
        return renderTree();
      },
      current() {
        return tree;
      },
      unmount() {
        for (const slot of slots) slot.cleanup?.();
        component = null;
      },
      reset() {
        slots.length = 0;
        cursor = 0;
        dirty = false;
        component = null;
        tree = null;
        layoutQueue = [];
        effectQueue = [];
      },
      useCallback,
      useEffect,
      useLayoutEffect,
      useMemo,
      useRef,
      useState,
    },
    directory: {
      state: null as unknown,
      inputs: [] as Parameters<typeof import("../client/use-session-directory").useSessionDirectory>[0][],
      getClient: vi.fn<(hostId: string) => unknown>(() => { throw new Error("No host in the sessions-screen event test"); }),
    },
  };
});

vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return {
    ...actual,
    memo: (component: unknown) => component,
    useCallback: harness.useCallback,
    useEffect: harness.useEffect,
    useLayoutEffect: harness.useLayoutEffect,
    useMemo: harness.useMemo,
    useRef: harness.useRef,
    useState: harness.useState,
  };
});

vi.mock("react-native", () => ({
  AccessibilityInfo: {
    addEventListener: () => ({ remove: () => undefined }),
    isReduceMotionEnabled: async () => false,
    isScreenReaderEnabled: async () => accessibility.screenReader,
  },
  ActivityIndicator: "ActivityIndicator",
  Animated: {
    Value: class {
      setValue() {}
      stopAnimation() {}
    },
    View: "Animated.View",
    timing: () => ({
      start: (callback?: () => void) => callback?.(),
      stop: () => undefined,
    }),
  },
  // Group rows create a drag responder; the row tests below only read what the row draws.
  PanResponder: { create: () => ({ panHandlers: {} }) },
  Pressable: "Pressable",
  Text: "Text",
  View: "View",
}));

vi.mock("@getpaseo/plugin/client", () => ({
  getPaseoClient: directory.getClient,
  useHosts: () => [
    { serverId: "host-a", status: "online" },
    { serverId: "host-b", status: "online" },
  ],
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: "FlatList",
  Icon: "Icon",
  ScrollView: "ScrollView",
  TextInput: "TextInput",
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

vi.mock("../client/use-session-directory", () => ({
  useSessionDirectory: (input: Parameters<typeof import("../client/use-session-directory").useSessionDirectory>[0]) => {
    directory.inputs.push(input);
    return { state: directory.state, refresh: () => undefined };
  },
}));

import { emptyDirectoryState } from "../client/session-directory";
import {
  ActiveFilterSummary,
  COMPACT_COLLAPSE_DISTANCE,
  COMPACT_SCROLL_SLACK,
  compactToolbarOpen,
  FILTER_SUMMARY_TEST_ID,
  FiltersButton,
  HIDE_FILTERS_TEST_ID,
  OPTIONS_MIN_HEIGHT,
  OPTIONS_SPACE_SHARE,
  SCOPE_SUMMARY_TEST_ID,
  sessionDetailLines,
  SessionsScreen,
  SHORT_PEEK_HEIGHT,
  SlimActionButton,
  SlimToolbar,
  toolbarOpenPatch,
} from "../client/sessions-screen";
import { StateBadge } from "../client/organization-row";
import { MenuItem } from "../client/anchored-menu";
import { nativeFreshnessLabel } from "../client/native-freshness";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
} from "../client/session-view-store";
import { TOOLBAR_LAYOUT_GAP } from "../client/toolbar-visibility";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";

const colors = {
  accent: "#0055ff",
  accentForeground: "#ffffff",
  border: "#cccccc",
  foreground: "#111111",
  foregroundMuted: "#666666",
  surface0: "#ffffff",
  surface1: "#f4f4f4",
  surface2: "#eeeeee",
  statusDanger: "#cc0000",
  statusSuccess: "#008800",
  statusWarning: "#cc8800",
} as unknown as PluginTheme["colors"];

type ElementLike = {
  readonly type?: unknown;
  readonly props: Record<string, unknown>;
};

type RefLike = { current: unknown };

function walk(node: unknown): ElementLike[] {
  if (node === null || node === undefined || typeof node !== "object") {
    return [];
  }
  if (Array.isArray(node)) {
    return node.flatMap(walk);
  }
  const element = node as ElementLike;
  const children = element.props.children;
  // A FlatList draws its header element inside its own scroll content.
  const header = element.props.ListHeaderComponent;
  return [element, ...walk(children), ...walk(header)];
}

function findByTestID(node: unknown, testID: string): ElementLike {
  const found = walk(node).filter(
    (element) => element.props.testID === testID,
  );
  expect(found).toHaveLength(1);
  return found[0]!;
}

function scrollEvent(
  offset: number,
  contentHeight = 2000,
  viewportHeight = 600,
): NativeSyntheticEvent<NativeScrollEvent> {
  return {
    nativeEvent: {
      contentOffset: { x: 0, y: offset },
      contentSize: { width: 400, height: contentHeight },
      layoutMeasurement: { width: 400, height: viewportHeight },
    },
  } as NativeSyntheticEvent<NativeScrollEvent>;
}

function renderScreen(organization?: Parameters<typeof SessionsScreen>[0]["organization"], compact = true, platform: "ios" | "web" = compact ? "ios" : "web") {
  const navigation = {
    openAgent: () => undefined,
    openBrowser: () => undefined,
    openWorkspace: () => undefined,
  };
  const tree = harness.render(() =>
    SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" },
      layout: { compact, platform },
      navigation,
      theme: { colors } as PluginTheme,
      params: {},
      organization,
    } as PluginScreenProps),
  );
  const list = findByTestID(tree, "sessions-list");
  const fullToolbar = findByTestID(tree, "sessions-full-toolbar");
  const slimSlot = findByTestID(tree, "sessions-slim-toolbar-slot");
  (fullToolbar.props.onLayout as (event: unknown) => void)({
    nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 220 } },
  });
  (slimSlot.props.onLayout as (event: unknown) => void)({
    nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 44 } },
  });
  const scrollToOffset = vi.fn();
  (list.props.ref as RefLike).current = { scrollToOffset };
  return { scrollToOffset };
}

function scroll(offset: number, contentHeight = 2000, viewportHeight = 600) {
  const list = findByTestID(harness.current(), "sessions-list");
  (list.props.onScroll as (event: unknown) => void)(
    scrollEvent(offset, contentHeight, viewportHeight),
  );
  harness.flush();
}

type GroupRow = Extract<import("../client/session-organization").OrganizationRow, { kind: "group" }>;

type ListRow = import("../client/session-organization").OrganizationRow | { readonly kind: "result"; readonly entry: import("../client/session-directory").DirectoryEntry };
function listRows(): ListRow[] {
  return findByTestID(harness.current(), "sessions-list").props.data as ListRow[];
}

function groupRows(): GroupRow[] {
  return listRows().filter((row): row is GroupRow => row.kind === "group");
}

/** Sessions the list draws, as tree rows or as flat results. */
function sessionRowIds(): string[] {
  return listRows().flatMap((row) => (row.kind === "session" || row.kind === "result" ? [row.entry.id] : []));
}

/** Presses the disclosure of the first group of a kind through the list's own row renderer. */
function toggle(kind: string) {
  const row = groupRows().find((candidate) => candidate.group.kind === kind)!;
  const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
  (renderItem({ item: row }).props.onToggle as () => void)();
  harness.flush();
}

function expandAll() {
  const options = walk(harness.current()).some((element) => element.props.testID === "sessions-options");
  if (!options) {
    const toggle = walk(findByTestID(harness.current(), "sessions-full-toolbar")).find((element) => element.props.expanded === false && "activeCount" in element.props)!;
    (toggle.props.onPress as () => void)();
    harness.flush();
  }
  (findByTestID(harness.current(), "sessions-expand-all").props.onPress as () => void)();
  harness.flush();
}

function fullToolbarShown(): boolean {
  const slots = walk(harness.current()).filter((element) => element.props.testID === "sessions-full-toolbar-slot");
  expect(slots.length).toBeLessThanOrEqual(1);
  return slots[0]?.props.accessibilityElementsHidden === false;
}

describe("SessionsScreen toolbar events", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    clearSessionViews();
    harness.reset();
    directory.inputs = [];
    directory.getClient.mockReset();
    directory.getClient.mockImplementation(() => { throw new Error("No host in the sessions-screen event test"); });
    directory.state = {
      entries: [sessionEntry("session-a", { status: "idle" })],
      status: "ready",
      live: true,
      error: null,
      staleReason: null,
      pages: 1,
      lastUpdatedAt: 0,
      inventoryComplete: true,
      includeArchived: true,
    };
    patchSessionView("host-a", {
      query: "session-a",
      status: "all",
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 0,
      // Compact opens with the slim line only; these cases use the opened toolbar.
      ...toolbarOpenPatch(true),
    });
  });

  afterEach(() => {
    harness.unmount();
    vi.useRealTimers();
  });

  it("shows the count line only for results or an incomplete read, and keeps the full count and read in Filters", () => {
    const list = () => findByTestID(harness.current(), "sessions-list");
    const header = () => list().props.ListHeaderComponent;
    renderScreen();
    const search = (query: string) => { (findByTestID(harness.current(), "sessions-search").props.onChangeText as (query: string) => void)(query); harness.flush(); };
    search("");
    expect(testIDs(header())).not.toContain("sessions-count");
    expect(textOf(findByTestID(harness.current(), "sessions-options-count"))).toMatch(/^1 session · 1 workspace · complete inventory read /);
    expect(list().props.ListFooterComponent).toBeNull();
    search("no such session");
    // No results: the empty state says so instead of a zero count.
    expect(testIDs(header())).not.toContain("sessions-count");
    expect(textOf(list().props.ListEmptyComponent)).toContain("No sessions match the current search and filters.");
    search("session-a");
    expect(textOf(findByTestID(header(), "sessions-count"))).toBe("1 matching session · 1 workspace");
    expect(findByTestID(header(), "sessions-count").props.accessibilityLiveRegion).toBe("polite");
    search("");
    expect(testIDs(header())).not.toContain("sessions-count");
    directory.state = { ...directory.state as object, inventoryComplete: false };
    renderScreen();
    expect(textOf(findByTestID(header(), "sessions-count"))).toMatch(/^1 session · partial inventory read /);
    directory.state = { ...directory.state as object, inventoryComplete: true, lastUpdatedAt: null };
    renderScreen();
    expect(textOf(findByTestID(header(), "sessions-count"))).toBe("1 session · not read yet");
  });

  it("shows No sessions without a 0 beside it, and keeps non-zero group counts", () => {
    patchSessionView("host-a", { query: "" });
    harness.render(() => SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" }, layout: { compact: true, platform: "ios" },
      navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
      theme: { colors } as PluginTheme, params: {}, workspaces: [{ id: "w-empty", name: "Empty desk", projectKey: null, projectName: null }],
    } as PluginScreenProps));
    const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
    const rendered = (row: GroupRow) => { const view = renderItem({ item: row }) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike }; return view.type(view.props); };
    const counts = (tree: ElementLike) => walk(tree).filter((element) => (element.props.style as { fontVariant?: string[] } | undefined)?.fontVariant?.includes("tabular-nums")).map(textOf);
    const empty = groupRows().find((row) => row.group.unitTotal === 0)!;
    expect(walk(rendered(empty)).some((element) => (element as { type?: unknown }).type === StateBadge && element.props.state === null)).toBe(true);
    expect(counts(rendered(empty))).toEqual([]);
    expect(String(walk(rendered(empty)).find((element) => element.props.accessibilityRole === "button")!.props.accessibilityLabel)).toContain(", 0 sessions.");
    const full = groupRows().find((row) => row.group.unitTotal > 0)!;
    expect(counts(rendered(full))).toEqual([String(full.group.unitTotal)]);
  });

  it("keeps the workspace read note in Filters and shows only a workspace list failure under the list", () => {
    const render = (workspaceNote: string) => harness.render(() => SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" }, layout: { compact: true, platform: "ios" },
      navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
      theme: { colors } as PluginTheme, params: {}, workspaceNote,
    } as PluginScreenProps));
    let tree = render("Empty workspaces from the workspace list read 10:00:00");
    expect(findByTestID(tree, "sessions-list").props.ListFooterComponent).toBeNull();
    expect(textOf(findByTestID(tree, "sessions-options-count"))).toContain(". Empty workspaces from the workspace list read 10:00:00.");
    tree = render("Workspace list unavailable: rejected");
    expect(textOf(findByTestID(tree, "sessions-list").props.ListFooterComponent)).toBe("Workspace list unavailable: rejected.");
    expect(textOf(findByTestID(tree, "sessions-list").props.ListFooterComponent)).not.toContain("Select a session");
  });

  it("restores the selected view when the screen host changes without remounting", () => {
    patchSessionView("host-a", {
      query: "host-a query",
      status: "all",
      includeArchived: true,
      advancedOpen: false,
      scrollOffset: 0,
    });
    patchSessionView("host-b", {
      query: "host-b query",
      status: "error",
      includeArchived: false,
      advancedOpen: true,
      scrollOffset: 0,
      ...toolbarOpenPatch(true),
    });
    let activeHost = "host-a";
    const render = () =>
      harness.render(() =>
        SessionsScreen({
          host: { id: activeHost, label: `srv_${activeHost}` },
          layout: { compact: true, platform: "ios" },
          navigation: {
            openAgent: () => undefined,
            openBrowser: () => undefined,
            openWorkspace: () => undefined,
          },
          theme: { colors } as PluginTheme,
          params: {},
        } as PluginScreenProps),
      );

    render();
    expect(findByTestID(harness.current(), "sessions-search").props.value).toBe(
      "host-a query",
    );

    activeHost = "host-b";
    harness.flush();
    expect(findByTestID(harness.current(), "sessions-search").props.value).toBe(
      "host-b query",
    );

    const search = findByTestID(harness.current(), "sessions-search");
    (search.props.onChangeText as (query: string) => void)("host-b edited");
    harness.flush();

    expect(readSessionView("host-b").query).toBe("host-b edited");
    expect(readSessionView("host-a").query).toBe("host-a query");
  });

  it.each([false, undefined])("labels an unavailable-live read as partial instead of complete (completeness=%s)", (inventoryComplete) => {
    directory.state = {
      entries: [sessionEntry("session-a", { status: "idle" })],
      status: "ready",
      live: false,
      error: null,
      staleReason:
        "Live updates are unavailable; the inventory may be incomplete.",
      pages: 1,
      lastUpdatedAt: 0,
      inventoryComplete,
      includeArchived: true,
    };
    renderScreen();
    const list = findByTestID(harness.current(), "sessions-list");
    const count = JSON.stringify(findByTestID(list.props.ListHeaderComponent, "sessions-count").props.children);

    expect(count).toContain("partial inventory read");
    expect(count).not.toContain("complete inventory read");
  });

  it("restores context-scoped filters and offsets without mixing them with host defaults", () => {
    patchSessionView("host-a", { query: "host default", scrollOffset: 0 });
    patchSessionView("host-a:epic-a", { query: "session-a", includeArchived: true, scrollOffset: 420, ...toolbarOpenPatch(true) });
    patchSessionView("host-a:epic-b", { query: "other", includeArchived: false, scrollOffset: 180, ...toolbarOpenPatch(true) });
    let viewScope = "host-a:epic-a";
    harness.render(() => SessionsScreen({
      host: { id: "host-a", label: "Host" }, layout: { compact: true, platform: "ios" },
      theme: { colors } as PluginTheme, params: {}, viewScope,
    }));
    const scrollToOffset = vi.fn();
    (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { scrollToOffset };
    vi.advanceTimersByTime(0);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 420, animated: false });

    viewScope = "host-a:epic-b"; harness.flush(); vi.advanceTimersByTime(0);
    expect(findByTestID(harness.current(), "sessions-search").props.value).toBe("other");
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 180, animated: false });
    expect(directory.inputs.at(-1)?.includeArchived).toBe(false);
    (findByTestID(harness.current(), "sessions-search").props.onChangeText as (query: string) => void)("edited b");
    harness.flush();
    expect(readSessionView("host-a:epic-b").query).toBe("edited b");
    expect(readSessionView("host-a:epic-a")).toMatchObject({ query: "session-a", scrollOffset: 420 });
    expect(readSessionView("host-a").query).toBe("host default");

    viewScope = "host-a:epic-a"; harness.flush(); vi.advanceTimersByTime(0);
    expect(findByTestID(harness.current(), "sessions-search").props.value).toBe("session-a");
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 420, animated: false });
    expect(directory.inputs.at(-1)?.includeArchived).toBe(true);
  });

  it("retains injected directory ownership, project/epic filtering and partial-read truth", () => {
    patchSessionView("host-a:linked", { query: "", scrollOffset: 0, ...toolbarOpenPatch(true) });
    const supplied = { state: { ...emptyDirectoryState(), status: "ready" as const,
      entries: [sessionEntry("match", { projectKey: "project-a", labels: { epic: "ASMA-1" } }),
        sessionEntry("other-project", { projectKey: "project-b", labels: { epic: "ASMA-1" } }),
        sessionEntry("other-epic", { projectKey: "project-a", labels: { epic: "ASMA-2" } })],
      pages: 1, lastUpdatedAt: 0, staleReason: "Live updates are unavailable; the inventory may be incomplete." }, refresh: vi.fn() };
    const onIncludeArchived = vi.fn();
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" },
      layout: { compact: true, platform: "ios" }, theme: { colors } as PluginTheme, params: {},
      viewScope: "host-a:linked", directory: supplied, projectId: "project-a", epicKey: "ASMA-1", onIncludeArchived,
    }));
    expandAll();
    expect(sessionRowIds()).toEqual(["match"]);
    expect(directory.inputs.at(-1)?.enabled).toBe(false);
    expect(onIncludeArchived).toHaveBeenCalledWith(false);
    const list = findByTestID(harness.current(), "sessions-list");
    const count = JSON.stringify(findByTestID(list.props.ListHeaderComponent, "sessions-count").props.children);
    expect(count).toContain('"1 session"'); expect(count).not.toContain("3");
    expect(count).toContain("partial inventory read");
    const banner = findByTestID(harness.current(), "sessions-source-banner");
    expect(banner.props.message).toContain("Inventory read attempt");
    expect(banner.props.message).not.toContain("Last complete read");
    (banner.props.onAction as () => void)(); expect(supplied.refresh).toHaveBeenCalledOnce();
  });

  it("keeps the actual directory hook disabled for an injected inventory and isolates late host reads", async () => {
    const { useSessionDirectory } = await vi.importActual<typeof import("../client/use-session-directory")>("../client/use-session-directory");
    let enabled = false;
    let hostId = "host-a";
    let releaseA: () => void = () => {};
    const pendingA = new Promise<void>((resolve) => { releaseA = resolve; });
    const calls: string[] = [];
    directory.getClient.mockImplementation((id) => ({ agents: { list: async () => {
      calls.push(id);
      if (id === "host-a") await pendingA;
      throw new Error(`${id} unavailable`);
    } } }));
    let current: ReturnType<typeof useSessionDirectory>;
    harness.render(() => { current = useSessionDirectory({ hostId, hostStatus: "online", includeArchived: false, enabled }); return null; });
    expect(calls).toEqual([]); expect(directory.getClient).not.toHaveBeenCalled();
    // Switching the host starts an owned directory after the injected one is removed.
    enabled = true; hostId = "host-b"; harness.flush();
    await vi.waitFor(() => { harness.flush(); expect(current!.state.error).toBe("host-b unavailable"); });
    hostId = "host-a"; harness.flush();
    expect(current!.state.entries).toEqual([]); expect(current!.state.error).toBeNull();
    hostId = "host-b"; harness.flush();
    await vi.waitFor(() => { harness.flush(); expect(current!.state.error).toBe("host-b unavailable"); });
    releaseA();
    await Promise.resolve(); await Promise.resolve(); harness.flush();
    expect(current!.state.error).toBe("host-b unavailable");
    current!.refresh();
    await vi.waitFor(() => { expect(calls.filter((id) => id === "host-b").length).toBeGreaterThan(4); });
    expect(current!.state.entries).toEqual([]);
  });

  it("closes the in-list compact toolbar once it has scrolled out of view, compensates the offset, and leaves reopening to the manual controls", () => {
    const { scrollToOffset } = renderScreen();
    // The open toolbar and its gap head the list content.
    const toolbarHeight = 220 + TOOLBAR_LAYOUT_GAP;
    const saved = readSessionView("host-a");

    // While any of it is in view it scrolls with the rows and stays open.
    scroll(100);
    scroll(toolbarHeight - 1);
    expect(fullToolbarShown()).toBe(true);
    expect(scrollToOffset).not.toHaveBeenCalled();

    // Out of view: it closes, and the offset drops by its height in the same
    // step, so a row at content y keeps its screen position (y + 228 - 300 = y - 72).
    scroll(300);
    expect(scrollToOffset).toHaveBeenLastCalledWith({
      offset: 300 - toolbarHeight,
      animated: false,
    });
    expect(fullToolbarShown()).toBe(false);
    expect(readSessionView("host-a").scrollOffset).toBe(72);
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(false);

    // A bottom bounce, a reverse scroll and the top all keep the space for sessions.
    vi.advanceTimersByTime(300);
    scroll(1400);
    scrollToOffset.mockClear();
    scroll(1405);
    expect(readSessionView("host-a").scrollOffset).toBe(1400);
    scroll(1389);
    scroll(4);
    expect(fullToolbarShown()).toBe(false);
    expect(scrollToOffset).not.toHaveBeenCalled();

    const after = readSessionView("host-a");
    expect(after.shortcut ?? null).toBe(saved.shortcut ?? null);
    expect(after).toMatchObject({
      query: saved.query,
      status: saved.status,
      includeArchived: saved.includeArchived,
      advancedOpen: saved.advancedOpen,
      scrollOffset: 4,
    });
  });

  it("compensates wide collapse and near-bottom reveal through the actual screen event path", () => {
    const { scrollToOffset } = renderScreen(undefined, false);
    // Wide layouts swap the full toolbar for the slim row in the same slot.
    const header = 220 - 44;
    const expandedListTop = 220 + TOOLBAR_LAYOUT_GAP;
    const collapsedListTop = 44 + TOOLBAR_LAYOUT_GAP;
    const saved = readSessionView("host-a");

    scroll(300);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 300 - header, animated: false });
    expect(fullToolbarShown()).toBe(false);
    expect(expandedListTop - 300).toBe(collapsedListTop - (300 - header));

    vi.advanceTimersByTime(300);
    scroll(1400);
    scroll(1389);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 1389 + header, animated: false });
    expect(fullToolbarShown()).toBe(true);
    expect(collapsedListTop - 1389).toBe(expandedListTop - (1389 + header));
    expect(1389 + header).toBeLessThanOrEqual(1400 + header);

    const after = readSessionView("host-a");
    expect(after.shortcut ?? null).toBe(saved.shortcut ?? null);
    expect(after).toMatchObject({
      query: saved.query,
      status: saved.status,
      includeArchived: saved.includeArchived,
      advancedOpen: saved.advancedOpen,
      scrollOffset: 1389 + header,
    });
  });

  it("retains Favorites and desk shortcuts across a screen remount", () => {
    const organization = {
      state: { status: "ready", values: { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["session-a"] }, revision: "revision-a", saving: false, saveError: null, save: async () => true, reload: async () => {}, reset: async () => true },
      save: async () => true,
    } as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    directory.state = { ...(directory.state as object), entries: [sessionEntry("session-a", { labels: { desk: "Adam" } })] };
    renderScreen(organization);
    const chip = (testID: string) => findByTestID(harness.current(), testID);
    expect([chip("sessions-filter-favorites").props.label, chip("sessions-shortcut-2").props.label]).toEqual(["★ Favorites", "Adam desk"]);
    expect([chip("sessions-filter-favorites").props.count, chip("sessions-shortcut-2").props.count]).toEqual([1, 1]);
    (chip("sessions-filter-favorites").props.onPress as () => void)(); harness.flush();
    (chip("sessions-shortcut-2").props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ status: "favorites", shortcut: "Adam" });
    expect(sessionRowIds()).toEqual(["session-a"]);
    harness.reset(); renderScreen(organization);
    expect(chip("sessions-filter-favorites").props.active).toBe(true);
    expect(chip("sessions-shortcut-2").props.active).toBe(true);
    (chip("sessions-shortcut-3").props.onPress as () => void)(); harness.flush();
    expect(sessionRowIds()).toEqual([]);
  });

  it.each([
    ["unavailable", { status: "unavailable", error: "Settings unreadable" }],
    ["saving", { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: true, saveError: null }],
  ])("opens and closes branches locally while organization settings are %s", (_, state) => {
    const save = vi.fn(async () => true);
    patchSessionView("host-a", { query: "" });
    renderScreen({ state: { ...state, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>);
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
    toggle("epic");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:true", "workspace:false"]);
    toggle("workstream");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:false"]);
    expect(save).not.toHaveBeenCalled();
  });

  it("saves a branch toggle through organization settings when they are writable", () => {
    const save = vi.fn(async () => true);
    patchSessionView("host-a", { query: "" });
    renderScreen({ state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>);
    const epic = groupRows().find((row) => row.group.kind === "epic")!;
    toggle("epic");
    expect(save).toHaveBeenLastCalledWith({ expandedGroups: { [epic.group.key]: true } });
  });

  it("lists a state filter's matches flat without opening or saving any branch", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running", labels: { epic: "ASMA-1" } }),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen();
    expect(sessionRowIds()).toEqual([]);
    expect(findByTestID(harness.current(), "sessions-collapse-all").props.disabled).toBe(false);
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    expect(sessionRowIds()).toEqual(["quiet"]);
    expect(findByTestID(harness.current(), "sessions-collapse-all").props.disabled).toBe(true);
    (findByTestID(harness.current(), "sessions-filter-all").props.onPress as () => void)(); harness.flush();
    expect(sessionRowIds()).toEqual([]);
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
  });

  it("shows a desk result in its workspace under the desk, and names a missing epic elsewhere", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("desk", { status: "idle", labels: { desk: "Adam" } }),
      sessionEntry("loose", { status: "running", title: "SWE • loose", labels: { workstream: "Kontor" } }),
      sessionEntry("loose-2", { status: "idle", title: "SWE • second", labels: { workstream: "Kontor" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen();
    expect(groupRows().map((row) => `${row.group.kind}:${row.group.label}`)).toEqual(["workstream:Adam", "workspace:Workspace", "workstream:Kontor", "epic:No linked epic"]);
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    const renderItem = (info: { item: unknown }) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)(info);
    const results = listRows() as Extract<ListRow, { kind: "result" }>[];
    expect(results.map((row) => (renderItem({ item: row }).props.provenance as { parts: string[] }).parts)).toEqual([["Adam desk", "Workspace"], ["Kontor", "No linked epic", "Workspace"]]);
    (renderItem({ item: results[0] }).props.onShowInTree as (entry: unknown) => void)(results[0]!.entry); harness.flush();
    expect(groupRows().map((row) => `${row.group.kind}:${row.group.label}:${row.expanded}`).slice(0, 2)).toEqual(["workstream:Adam:true", "workspace:Workspace:true"]);
    expect(sessionRowIds()).toContain("desk");
    // Role grouping names the missing epic in each session's context too.
    harness.reset(); patchSessionView("host-a", { grouping: "role", status: "all" }); renderScreen();
    expect(groupRows().map((row) => `${row.group.label}:${row.expanded}`)).toEqual(["SWE:false", "No role:false"]);
    expandAll();
    const roleSessions = listRows().filter((row): row is Extract<ListRow, { kind: "session" }> => row.kind === "session" && row.entry.id.startsWith("loose"));
    expect(roleSessions.map((row) => walk(renderItem({ item: row })).find((element) => "displayContext" in element.props)!.props.displayContext)).toEqual(["Kontor · No linked epic · Workspace", "Kontor · No linked epic · Workspace"]);
  });

  it("shows a result in the tree without saving the branches it opens, and Back returns to the results", () => {
    const save = vi.fn(async () => true);
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running", labels: { epic: "ASMA-1" } }),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen({ state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>);
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    // The current render's row renderer, since rows read the peek from its closure.
    const renderItem = (info: { item: unknown }) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)(info);
    const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
    expect((renderItem({ item: result }).props.provenance as { parts: string[] }).parts).toEqual(["Other work", "ASMA-1", "Workspace"]);
    (renderItem({ item: result }).props.onShowInTree as (entry: unknown) => void)(result.entry); harness.flush();
    expect(textOf(findByTestID(harness.current(), "sessions-peek"))).toContain("Branches opened here are not saved.");
    expect(walk(findByTestID(harness.current(), "sessions-peek")).some((element) => element.props.accessibilityLabel === "Showing “Session quiet” in the tree. Branches opened here are not saved.")).toBe(true);
    // Pinned above the list, not in its scrolling header, so the way back stays in view.
    expect(testIDs(findByTestID(harness.current(), "sessions-list").props.ListHeaderComponent)).not.toContain("sessions-peek");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:true", "workspace:true"]);
    expect(sessionRowIds()).toEqual(expect.arrayContaining(["quiet"]));
    const target = listRows().find((row) => row.kind === "session" && row.entry.id === "quiet")!;
    expect(walk(renderItem({ item: target })).find((element) => "highlighted" in element.props)!.props.highlighted).toBe(true);
    toggle("workspace");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:true", "workspace:false"]);
    const back = walk(findByTestID(harness.current(), "sessions-peek")).find((element) => element.props.label === "Back to 1 result")!;
    (back.props.onPress as () => void)(); harness.flush();
    expect(testIDs(harness.current())).not.toContain("sessions-peek");
    expect(sessionRowIds()).toEqual(["quiet"]);
    (findByTestID(harness.current(), "sessions-filter-all").props.onPress as () => void)(); harness.flush();
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
    expect(save).not.toHaveBeenCalled();
  });

  it("on a short compact screen, Show in tree keeps one Back row above the list, and Back brings the toolbar and filters back", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running", labels: { epic: "ASMA-1" } }),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen();
    const rootHeight = (height: number) => { ((harness.current() as ElementLike).props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: 390, height } } }); harness.flush(); };
    const header = () => testIDs(findByTestID(harness.current(), "sessions-list").props.ListHeaderComponent);
    const peekText = () => walk(findByTestID(harness.current(), "sessions-peek")).find((element) => "numberOfLines" in element.props)!;
    const showInTree = () => {
      const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
      const row = (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: result });
      (row.props.onShowInTree as (entry: unknown) => void)(result.entry); harness.flush();
    };
    const back = () => walk(findByTestID(harness.current(), "sessions-peek")).find((element) => element.props.label === "Back to 1 result")!;
    rootHeight(SHORT_PEEK_HEIGHT - 100);
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    // Results keep their toolbar and filters at any height; only the peek folds them.
    expect(testIDs(harness.current())).toContain("sessions-slim-toolbar-slot");
    expect(header()).toContain("sessions-full-toolbar");
    showInTree();
    expect(testIDs(harness.current())).not.toContain("sessions-slim-toolbar-slot");
    expect(header()).toEqual([]);
    expect([textOf(peekText()), peekText().props.numberOfLines, peekText().props.accessibilityLabel]).toEqual(["In the tree · not saved", 1, "Showing “Session quiet” in the tree. Branches opened here are not saved."]);
    // A full-size Back, the 44pt touch action.
    expect(back().props.small).toBe(false);
    expect(sessionRowIds()).toEqual(expect.arrayContaining(["quiet"]));
    (back().props.onPress as () => void)(); harness.flush();
    expect(testIDs(harness.current())).toContain("sessions-slim-toolbar-slot");
    expect(header()).toContain("sessions-full-toolbar");
    expect(sessionRowIds()).toEqual(["quiet"]);
    // A phone-height screen keeps the toolbar and the longer note during the peek.
    rootHeight(SHORT_PEEK_HEIGHT);
    showInTree();
    expect(testIDs(harness.current())).toContain("sessions-slim-toolbar-slot");
    expect(header()).toContain("sessions-full-toolbar");
    expect([textOf(peekText()), peekText().props.numberOfLines]).toEqual(["Shown in the tree. Branches opened here are not saved.", 2]);
  });

  it("places a session shown in the tree again once the tree rows are measured, so a list header cannot push it out of view", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running", labels: { epic: "ASMA-1" } }),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen();
    const scrollToIndex = vi.fn();
    (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { scrollToOffset: vi.fn(), scrollToIndex };
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
    ((findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: result }).props.onShowInTree as (entry: unknown) => void)(result.entry);
    harness.flush();
    const place = { index: listRows().findIndex((row) => row.kind === "session" && row.entry.id === "quiet"), viewPosition: 0.3, animated: false };
    vi.advanceTimersByTime(0);
    expect(scrollToIndex.mock.calls).toEqual([[place]]);
    vi.advanceTimersByTime(150);
    expect(scrollToIndex.mock.calls).toEqual([[place], [place]]);
    vi.runOnlyPendingTimers();
    expect(scrollToIndex).toHaveBeenCalledTimes(2);
  });

  /**
   * Forty running sessions and one idle one in a workspace. The Idle filter's one result, shown in the
   * tree, is tree row 43: past the rendered window, so its scroll fails and is retried 60ms later.
   */
  function showQuietInTree(render: () => void = () => { renderScreen(); }) {
    directory.state = { ...(directory.state as object), entries: [
      ...Array.from({ length: 40 }, (_, n) => sessionEntry(`busy-${String(n).padStart(2, "0")}`, { status: "running", labels: { epic: "ASMA-1" } })),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    render();
    const scrollToIndex = vi.fn();
    (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { scrollToOffset: vi.fn(), scrollToIndex };
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    const showInTree = () => {
      const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
      ((findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: result }).props.onShowInTree as (entry: unknown) => void)(result.entry);
      harness.flush();
    };
    showInTree();
    vi.runOnlyPendingTimers();
    scrollToIndex.mockClear();
    const quietRow = () => listRows().findIndex((row) => row.kind === "session" && row.entry.id === "quiet");
    const failed = () => (findByTestID(harness.current(), "sessions-list").props.onScrollToIndexFailed as (info: { index: number; averageItemLength: number }) => void)({ index: quietRow(), averageItemLength: 40 });
    const back = () => {
      (walk(findByTestID(harness.current(), "sessions-peek")).find((element) => element.props.label === "Back to 1 result")!.props.onPress as () => void)();
      harness.flush();
    };
    const indexes = () => scrollToIndex.mock.calls.map(([place]) => (place as { index: number }).index);
    return { failed, back, showInTree, quietRow, indexes };
  }

  it("retries a failed scroll to a tree row once, on that row", () => {
    const { failed, quietRow, indexes } = showQuietInTree();
    expect(quietRow()).toBe(43);
    failed();
    vi.advanceTimersByTime(60);
    expect(indexes()).toEqual([43]);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43]);
  });

  it("drops a scroll retry once Back has swapped the tree for the shorter results list", () => {
    const { failed, back, indexes } = showQuietInTree();
    failed();
    back();
    expect(listRows()).toHaveLength(1);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("drops a scroll retry whose row a collapse removed, though the list mode is unchanged", () => {
    const { failed, indexes } = showQuietInTree();
    failed();
    // The workspace closes before the retry: three rows remain, and the list still shows the same session's tree.
    toggle("workspace");
    expect(listRows()).toHaveLength(3);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("moves a scroll retry with its row when a live update shifts the tree", () => {
    const { failed, quietRow, indexes } = showQuietInTree();
    failed();
    addBusySibling();
    expect(quietRow()).toBe(44);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([44]);
  });

  it("drops a scroll retry when Back and Show in tree place the same session again first", () => {
    const { failed, back, showInTree, indexes } = showQuietInTree();
    failed();
    back();
    showInTree();
    // The new placement scrolls at once and again at 150ms; the old retry at 60ms adds nothing.
    vi.advanceTimersByTime(0);
    expect(indexes()).toEqual([43]);
    vi.advanceTimersByTime(60);
    expect(indexes()).toEqual([43]);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43, 43]);
  });

  it("drops a scroll retry when the screen moves to another host showing the same tree", () => {
    let activeHost = "host-a";
    const { failed, indexes } = showQuietInTree(() => {
      harness.render(() =>
        SessionsScreen({
          host: { id: activeHost, label: `srv_${activeHost}` },
          layout: { compact: true, platform: "ios" },
          navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
          theme: { colors } as PluginTheme,
          params: {},
        } as PluginScreenProps),
      );
    });
    // host-b reads the same sessions and shows the same session in its tree, so only the host differs.
    patchSessionView("host-b", { ...readSessionView("host-a") });
    failed();
    activeHost = "host-b";
    harness.flush();
    expect(readSessionView("host-b").peek?.sessionId).toBe("quiet");
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  /** Adds a running session that sorts before "quiet", so quiet's tree row moves from 43 to 44. */
  function addBusySibling() {
    const { entries } = directory.state as { entries: readonly unknown[] };
    directory.state = { ...(directory.state as object), entries: [sessionEntry("busy-40", { status: "running", labels: { epic: "ASMA-1" } }), ...entries] };
    harness.flush();
  }

  it("places a session shown in the tree on its row as that row stands when the first pass runs", () => {
    const { back, showInTree, quietRow, indexes } = showQuietInTree();
    back();
    showInTree();
    addBusySibling();
    expect(quietRow()).toBe(44);
    vi.advanceTimersByTime(0);
    expect(indexes()).toEqual([44]);
  });

  it("drops the settle pass when a collapse removes the session's row, though the list mode is unchanged", () => {
    const { back, showInTree, indexes } = showQuietInTree();
    back();
    showInTree();
    vi.advanceTimersByTime(0);
    expect(indexes()).toEqual([43]);
    toggle("workspace");
    expect(listRows()).toHaveLength(3);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43]);
  });

  it("moves the settle pass with the session's row when a live update shifts the tree", () => {
    const { back, showInTree, indexes } = showQuietInTree();
    back();
    showInTree();
    vi.advanceTimersByTime(0);
    addBusySibling();
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43, 44]);
  });

  it("drops a scroll retry in the Role view when a collapse removes its row", () => {
    const { failed, quietRow, indexes } = showQuietInTree(() => { patchSessionView("host-a", { grouping: "role" }); renderScreen(); });
    expect([groupRows().map((row) => `${row.group.label}:${row.expanded}`), quietRow()]).toEqual([["No role:true"], 41]);
    failed();
    toggle("role");
    expect(listRows()).toHaveLength(1);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("moves a scroll retry in the Role view with its row when a new session joins the role", () => {
    const { failed, quietRow, indexes } = showQuietInTree(() => { patchSessionView("host-a", { grouping: "role" }); renderScreen(); });
    failed();
    addBusySibling();
    expect(quietRow()).toBe(42);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([42]);
  });

  /** The Role view of a few sessions, with a renderer that draws a row as the list does. */
  function renderRoles(entries: readonly ReturnType<typeof sessionEntry>[], organization?: Parameters<typeof SessionsScreen>[0]["organization"], compact = true) {
    directory.state = { ...(directory.state as object), entries };
    patchSessionView("host-a", { query: "", grouping: "role", status: "all" });
    renderScreen(organization, compact);
    const renderItem = (row: unknown) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: row });
    const drawn = (row: unknown) => { const view = renderItem(row) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike }; return view.type(view.props); };
    return { renderItem, drawn };
  }

  /** The Role view with every group open, spying on session opens and preference saves. */
  function renderDetails(entries: readonly ReturnType<typeof sessionEntry>[], compact = true) {
    const openAgent = vi.fn();
    const save = vi.fn(async () => true);
    directory.state = { ...(directory.state as object), entries };
    patchSessionView("host-a", { query: "", grouping: "role", status: "all" });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: compact ? "ios" : "web" },
      navigation: { openAgent, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {},
      organization: { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save },
    } as unknown as PluginScreenProps));
    expandAll();
    save.mockClear();
    const row = (id: string) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: listRows().find((candidate) => candidate.kind === "session" && candidate.entry.id === id) });
    const details = (id: string) => row(id).props.details as ElementLike;
    const dialog = () => walk(harness.current()).find((element) => element.props.testID === "sessions-details");
    const lines = () => (findByTestID(dialog()!.props.children, "sessions-details-lines").props.children as ElementLike[])
      .map((line) => (line.props.children as ElementLike[]).map(textOf).join(": "));
    const close = () => walk(dialog()!.props.header).find((element) => element.props.label === "Close")!;
    const open = (id: string) => { (details(id).props.onPress as () => void)(); harness.flush(); };
    return { openAgent, save, row, details, dialog, lines, close, open };
  }

  const placeEntries = () => [
    sessionEntry("long", { title: "QA • Reconcile the delivery evidence for the second review round", workspaceName: "TSW • ASMA-8541 roles provenance correction workspace with a long name",
      labels: { "kontor.role": "QA", workstream: "Kontor plugin and prototype parity across every host", epic: "ASMA-8541" } }),
    sessionEntry("conflict", { title: "AUD • audit notes", labels: { role: "qa", workstream: "Review" } }),
    sessionEntry("desk", { title: "SWE • desk work", labels: { desk: "Adam" } }),
    sessionEntry("cwd", { title: "SWE • scratch", workspaceId: null, workspaceName: null, cwd: "/tmp/scratch" }),
    sessionEntry("nowhere", { title: "loose", workspaceId: null, workspaceName: null, cwd: "" }),
  ];

  it("reveals each Role row's full place from its own details button, with absent values named, and saves nothing", () => {
    const screen = renderDetails(placeEntries());
    // The row keeps its short context; the details hold what it leaves out.
    expect((screen.row("long").props.children as ElementLike).props.compactContext).toBe("ASMA-8541 · TSW • ASMA-8541 roles provenance correction workspace with a long name");
    expect(screen.dialog()).toBeUndefined();
    expect(screen.details("long").props).toMatchObject({ title: "QA • Reconcile the delivery evidence for the second review round", expanded: false });
    screen.open("long");
    expect(screen.dialog()!.props).toMatchObject({ dialog: true, label: "Details: QA • Reconcile the delivery evidence for the second review round", placement: { align: "stretch", width: 0, cover: true } });
    expect(textOf(findByTestID(screen.dialog()!.props.header, "sessions-details-title"))).toBe("QA • Reconcile the delivery evidence for the second review round");
    expect(screen.lines()).toEqual(["Role: QA, from the kontor.role label", "Other role declarations: None", "Workstream: Kontor plugin and prototype parity across every host", "Epic: ASMA-8541",
      "Workspace: TSW • ASMA-8541 roles provenance correction workspace with a long name"]);
    expect(screen.details("long").props.expanded).toBe(true);
    expect(screen.details("conflict").props.expanded).toBe(false);
    const expected: Record<string, string[]> = {
      conflict: ["Role: QA, from the role label", "Other role declarations: title says AUD", "Workstream: Review", "Epic: No linked epic", "Workspace: Workspace"],
      desk: ["Role: SWE, from the title", "Other role declarations: None", "Desk: Adam desk", "Workspace: Workspace"],
      cwd: ["Role: SWE, from the title", "Other role declarations: None", "Workstream: Other work: no workstream or desk label and no display mapping", "Epic: No linked epic", "Workspace: No workspace · /tmp/scratch"],
      nowhere: ["Role: No role: no kontor.role or role label, and no role prefix in the title", "Other role declarations: None", "Workstream: Other work: no workstream or desk label and no display mapping", "Epic: No linked epic", "Workspace: No workspace"],
    };
    for (const [id, lines] of Object.entries(expected)) {
      (screen.dialog()!.props.onClose as () => void)(); harness.flush();
      expect(screen.dialog()).toBeUndefined();
      screen.open(id);
      expect(screen.lines()).toEqual(lines);
    }
    expect(screen.openAgent).not.toHaveBeenCalled();
    expect(screen.save).not.toHaveBeenCalled();
  });

  it("moves focus to Close on open and back to the details button on Close, while the row itself still opens the session", () => {
    const screen = renderDetails(placeEntries());
    const button = { focus: vi.fn() };
    (screen.details("long").props.ref as (node: unknown) => void)(button);
    screen.open("long");
    const closeFocus = vi.fn();
    (screen.close().props.ref as RefLike).current = { focus: closeFocus };
    (screen.close().props.onPress as () => void)(); harness.flush();
    expect(screen.dialog()).toBeUndefined();
    expect(button.focus).toHaveBeenCalledTimes(1);
    screen.open("long");
    expect(closeFocus).toHaveBeenCalledTimes(1);
    (screen.dialog()!.props.onClose as () => void)(); harness.flush();
    // The row's own button opens the session and never the details.
    const sessionRow = screen.row("long").props.children as ElementLike;
    (sessionRow.props.onPress as (entry: unknown) => void)(sessionRow.props.entry);
    harness.flush();
    expect(screen.openAgent).toHaveBeenCalledTimes(1);
    expect(screen.openAgent.mock.calls[0]![0]).toMatchObject({ agentId: "long" });
    expect(screen.dialog()).toBeUndefined();
    expect(screen.save).not.toHaveBeenCalled();
  });

  it("anchors the details beside the row on a wide screen, closes them when the view leaves Role, and draws no details elsewhere", () => {
    const screen = renderDetails(placeEntries(), false);
    screen.open("desk");
    expect(screen.dialog()!.props.placement).toEqual({ align: "end", width: 360, maxHeight: 440 });
    (walk(harness.current()).find((element) => element.props.testID === "sessions-grouping")!.props.onPress as () => void)(); harness.flush();
    (walk(harness.current()).find((element) => element.type === MenuItem && element.props.label === "Workstreams")!.props.onPress as () => void)(); harness.flush();
    expect(screen.dialog()).toBeUndefined();
    expandAll();
    const sessions = listRows().filter((row) => row.kind === "session");
    expect(sessions).toHaveLength(5);
    expect(sessions.map((row) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: row }).props.details)).toEqual(sessions.map(() => undefined));
  });

  it("names a declared Other work workstream or Unassigned epic as declared, and only a missing one as absent", () => {
    const chain = (entry: ReturnType<typeof sessionEntry>, mappings: Parameters<typeof sessionDetailLines>[1] = {}) =>
      sessionDetailLines(entry, mappings).filter(({ label }) => label === "Workstream" || label === "Epic").map(({ label, value }) => `${label}: ${value}`);
    const absent = ["Workstream: Other work: no workstream or desk label and no display mapping", "Epic: No linked epic"];
    expect(chain(sessionEntry("label", { labels: { workstream: "Other work", epic: "Unassigned" } }))).toEqual(["Workstream: Other work", "Epic: Unassigned"]);
    expect(chain(sessionEntry("mapped"), { "session:mapped": { workstream: "Other work", epic: "Unassigned" } })).toEqual(["Workstream: Other work", "Epic: Unassigned"]);
    expect(chain(sessionEntry("jira", { labels: { "jira.epic": "Unassigned" } }))).toEqual([absent[0], "Epic: Unassigned"]);
    expect(chain(sessionEntry("bare"))).toEqual(absent);
    // A blank label declares nothing.
    expect(chain(sessionEntry("blank", { labels: { workstream: " ", epic: "" } }))).toEqual(absent);
  });

  it("makes the lines a named stop with a visible focus on web only, and returns focus to the list once the row has left", () => {
    const screen = renderDetails(placeEntries());
    const region = () => findByTestID(screen.dialog()!.props.children, "sessions-details-lines");
    const list = () => findByTestID(harness.current(), "sessions-list");
    screen.open("long");
    // Native keeps its own focus rules.
    expect([region().props.tabIndex, region().props.role, region().props.accessibilityLabel, list().props.tabIndex]).toEqual([undefined, undefined, undefined, undefined]);
    (screen.dialog()!.props.onClose as () => void)(); harness.flush();
    vi.stubGlobal("document", { activeElement: null, body: {} });
    try {
      screen.open("long");
      expect(region().props).toMatchObject({ tabIndex: 0, role: "region", accessibilityLabel: "Role and placement" });
      expect(list().props.tabIndex).toBe(-1);
      expect(region().props.style).not.toHaveProperty("outlineWidth");
      (region().props.onFocus as () => void)(); harness.flush();
      expect(region().props.style).toMatchObject({ outlineColor: colors.accent, outlineStyle: "solid", outlineWidth: 2 });
      (region().props.onBlur as () => void)(); harness.flush();
      expect(region().props.style).not.toHaveProperty("outlineWidth");
      (region().props.onFocus as () => void)(); harness.flush();
      (screen.dialog()!.props.onClose as () => void)(); harness.flush();
      screen.open("long");
      expect(region().props.style).not.toHaveProperty("outlineWidth");
      // Escape, a press outside and Close all dismiss through onClose; without the row's button, focus goes to the list.
      const scroller = { focus: vi.fn() };
      (list().props.ref as RefLike).current = { getScrollableNode: () => scroller };
      const button = { focus: vi.fn() };
      (screen.details("long").props.ref as (node: unknown) => void)(button);
      (screen.details("long").props.ref as (node: unknown) => void)(null);
      (screen.dialog()!.props.onClose as () => void)(); harness.flush();
      expect(screen.dialog()).toBeUndefined();
      expect([scroller.focus.mock.calls.length, button.focus.mock.calls.length]).toEqual([1, 0]);
      (screen.details("long").props.ref as (node: unknown) => void)(button);
      screen.open("long");
      (screen.close().props.onPress as () => void)(); harness.flush();
      expect([scroller.focus.mock.calls.length, button.focus.mock.calls.length]).toEqual([1, 1]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("closes details whose session has left or that leave Role, sending only focus the page dropped to the list", () => {
    const screen = renderDetails(placeEntries());
    const page = { activeElement: null as unknown, body: {} };
    vi.stubGlobal("document", page);
    try {
      const scroller = { focus: vi.fn() };
      (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { getScrollableNode: () => scroller };
      const without = (id: string) => { directory.state = { ...(directory.state as object), entries: placeEntries().filter((entry) => entry.id !== id) }; harness.flush(); };
      screen.open("long");
      without("long");
      expect(screen.dialog()).toBeUndefined();
      expect(scroller.focus).toHaveBeenCalledTimes(1);
      // They stay closed when the session comes back.
      without("none");
      expect(screen.dialog()).toBeUndefined();
      screen.open("conflict");
      page.activeElement = page.body;
      without("conflict");
      expect(screen.dialog()).toBeUndefined();
      expect(scroller.focus).toHaveBeenCalledTimes(2);
      // Focus that moved on is left where it is.
      without("none");
      screen.open("desk");
      page.activeElement = { elsewhere: true };
      without("desk");
      expect(screen.dialog()).toBeUndefined();
      expect(scroller.focus).toHaveBeenCalledTimes(2);
      without("none");
      screen.open("cwd");
      const choose = (label: string) => {
        (walk(harness.current()).find((element) => element.props.testID === "sessions-grouping")!.props.onPress as () => void)(); harness.flush();
        (walk(harness.current()).find((element) => element.type === MenuItem && element.props.label === label)!.props.onPress as () => void)(); harness.flush();
      };
      choose("Workstreams");
      choose("Role");
      expect(screen.dialog()).toBeUndefined();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  const qaEntries = () => [
    sessionEntry("n", { title: "QA • n", pendingPermissionCount: 1 }),
    sessionEntry("e", { title: "QA • e", status: "error" }),
    ...["r1", "r2", "r3"].map((id) => sessionEntry(id, { title: `QA • ${id}`, status: "running" })),
  ];

  it("narrows the Role headers to the sessions in the selected epic", () => {
    patchSessionView("host-a:epic-roles", { query: "", grouping: "role", status: "all", scrollOffset: 0, ...toolbarOpenPatch(true) });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" },
      layout: { compact: true, platform: "ios" }, theme: { colors } as PluginTheme, params: {}, viewScope: "host-a:epic-roles",
      directory: { state: { ...emptyDirectoryState(), status: "ready" as const, pages: 1, lastUpdatedAt: 0, entries: [
        sessionEntry("qa-in", { title: "QA • in", labels: { epic: "ASMA-9" } }),
        sessionEntry("swe-in", { title: "work", labels: { epic: "ASMA-9", role: "swe" } }),
        sessionEntry("aud-out", { title: "AUD • out", labels: { epic: "ASMA-1" } }),
        sessionEntry("qa-out", { title: "QA • out", labels: { epic: "ASMA-1" } }),
      ] }, refresh: vi.fn() },
      epicKey: "ASMA-9",
    } as Parameters<typeof SessionsScreen>[0]));
    // AUD has no session in ASMA-9, so it has no header; QA counts only its ASMA-9 session.
    expect(groupRows().map((row) => [row.group.label, row.group.aggregate.total])).toEqual([["SWE", 1], ["QA", 1]]);
    expandAll();
    expect(sessionRowIds()).toEqual(["swe-in", "qa-in"]);
  });

  it("draws a Role header with its two leading states and a separate total, calling them last known when stale", () => {
    const { drawn } = renderRoles(qaEntries());
    const [qa] = groupRows();
    expect([qa!.group.label, qa!.expanded]).toEqual(["QA", false]);
    const header = drawn(qa);
    const counts = walk(header).find((element) => "aggregate" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    const words = walk(counts.type(counts.props)).map((element) => element.props.children).filter((child) => typeof child === "string" || typeof child === "number");
    expect(words).toEqual(["▲", 1, "needs input", "✕", 1, "error"]);
    expect(walk(header).filter((element) => element.props.children === "5")).toHaveLength(1);
    const label = () => String(walk(drawn(groupRows()[0])).find((element) => element.props.accessibilityRole === "button")!.props.accessibilityLabel);
    expect(label()).toMatch(/^Role QA, 5 sessions\. 1 needs input · 1 error · 3 running\. Native counts: 0 initializing, 1 idle, 3 running, 1 errors, 0 closed\.$/);
    directory.state = { ...(directory.state as object), status: "stale", live: false, staleReason: "Host offline" };
    harness.flush();
    expect(label()).toContain("5 sessions. Last known: 1 needs input · 1 error · 3 running.");
    const staleCounts = walk(drawn(groupRows()[0])).find((element) => "aggregate" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    expect(walk(staleCounts.type(staleCounts.props)).map((element) => element.props.children).filter((child) => typeof child === "string" || typeof child === "number"))
      .toEqual(["Last known", "▲", 1, "needs input", "✕", 1, "error"]);
  });

  it("shows a Role row's conflict cue and place, keeping the place beside the title on compact", () => {
    const { renderItem, drawn } = renderRoles([
      sessionEntry("conflict", { title: "QA • check results", labels: { "kontor.role": "AUD", workstream: "Kontor", epic: "ASMA-1" } }),
      sessionEntry("plain", { title: "aud • sign off", labels: { desk: "PR Review" } }),
      sessionEntry("loose", { title: "Notes", workspaceId: null, workspaceName: null, cwd: "/tmp/notes" }),
    ]);
    expect(groupRows().map((row) => row.group.label)).toEqual(["AUD", "No role"]);
    const headerOf = (label: string) => walk(drawn(groupRows().find((row) => row.group.label === label)));
    expect(String(headerOf("No role").find((element) => element.props.accessibilityRole === "button")!.props.accessibilityLabel)).toMatch(/^No role, 1 sessions\. 1 idle\./);
    expect(["AUD", "No role"].map((label) => (headerOf(label).find((element) => element.props.children === label)!.props.style as { fontFamily?: string }).fontFamily)).toEqual(["monospace", undefined]);
    expandAll();
    const props = (id: string) => walk(renderItem(listRows().find((row) => row.kind === "session" && row.entry.id === id))).find((element) => "displayContext" in element.props)!.props;
    const shown = (id: string) => { const { displayTitle, displayContext, compactContext, accessibilityContext } = props(id); return { displayTitle, displayContext, compactContext, accessibilityContext }; };
    expect(shown("conflict")).toEqual({
      displayTitle: undefined,
      displayContext: "title says QA · Kontor · ASMA-1 · Workspace",
      compactContext: "title says QA · ASMA-1 · Workspace",
      accessibilityContext: "Role AUD from the kontor.role label. title says QA. Workstream Kontor, epic ASMA-1, workspace Workspace",
    });
    expect(shown("plain")).toEqual({
      displayTitle: "sign off",
      displayContext: "PR Review desk · Workspace",
      compactContext: "PR Review desk · Workspace",
      accessibilityContext: "Role AUD from the title. Desk PR Review desk, workspace Workspace",
    });
    expect(shown("loose")).toMatchObject({ displayTitle: undefined, displayContext: "Other work · No linked epic · No workspace · /tmp/notes", compactContext: "Other work · No workspace · /tmp/notes" });
    const row = walk(renderItem(listRows().find((candidate) => candidate.kind === "session" && candidate.entry.id === "conflict"))).find((element) => "displayContext" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    // The harness's memo returns the component itself.
    const drawnRow = row.type(row.props);
    expect([findByTestID(drawnRow, "sessions-row-title").props.children, findByTestID(drawnRow, "sessions-row-meta").props.children]).toEqual(["QA • check results", "title says QA · ASMA-1 · Workspace"]);
    expect((findByTestID(drawnRow, "sessions-row-meta").props.style as { maxWidth: string }).maxWidth).toBe("55%");
    expect(String(drawnRow.props.accessibilityLabel)).toContain("Role AUD from the kontor.role label. title says QA. Workstream Kontor, epic ASMA-1, workspace Workspace");
  });

  it("arranges Role headers among themselves and keeps No role last", () => {
    const save = vi.fn(async () => true);
    patchSessionView("host-a", { arranging: true });
    const { renderItem } = renderRoles([sessionEntry("q", { title: "QA • q" }), sessionEntry("s", { title: "SWE • s" }), sessionEntry("x", { title: "Notes" })],
      { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>);
    expect(groupRows().map((row) => row.group.key)).toEqual(["roles/role:SWE", "roles/role:QA", "roles/no-role"]);
    const move = (key: string, distance: number) => { (renderItem(groupRows().find((row) => row.group.key === key)).props.onMove as (distance: number) => void)(distance); harness.flush(); };
    move("roles/role:QA", -1);
    expect(save).toHaveBeenLastCalledWith({ siblingOrder: { roles: ["roles/role:QA", "roles/role:SWE", "roles/no-role"] } });
    move("roles/no-role", -2);
    expect(save).toHaveBeenLastCalledWith({ siblingOrder: { roles: ["roles/role:SWE", "roles/role:QA", "roles/no-role"] } });
  });

  it("opens a result's Role header for Show in tree without saving it, and the header is closed again after Back", () => {
    const save = vi.fn(async (_patch: object) => true);
    renderRoles(qaEntries(), { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>);
    (findByTestID(harness.current(), "sessions-filter-idle").props.onPress as () => void)(); harness.flush();
    const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
    expect(result.entry.id).toBe("n");
    ((findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: result }).props.onShowInTree as (entry: unknown) => void)(result.entry);
    harness.flush();
    expect([groupRows().map((row) => `${row.group.label}:${row.expanded}`), sessionRowIds()]).toEqual([["QA:true"], ["n", "e", "r1", "r2", "r3"]]);
    (walk(findByTestID(harness.current(), "sessions-peek")).find((element) => String(element.props.label).startsWith("Back to "))!.props.onPress as () => void)();
    harness.flush();
    expect(sessionRowIds()).toEqual(["n"]);
    (findByTestID(harness.current(), "sessions-filter-all").props.onPress as () => void)(); harness.flush();
    expect(groupRows().map((row) => `${row.group.label}:${row.expanded}`)).toEqual(["QA:false"]);
    expect(save.mock.calls.filter(([patch]) => "expandedGroups" in patch)).toEqual([]);
  });

  it("returns to the tree position it left when the search is cleared", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("busy", { status: "running" }), sessionEntry("quiet")] };
    patchSessionView("host-a", { query: "" });
    const { scrollToOffset } = renderScreen(undefined, false);
    scroll(480);
    const search = (query: string) => { (findByTestID(harness.current(), "sessions-search").props.onChangeText as (query: string) => void)(query); harness.flush(); };
    search("quiet");
    expect(readSessionView("host-a")).toMatchObject({ treeOffset: 480, scrollOffset: 0 });
    vi.runOnlyPendingTimers();
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 0, animated: false });
    scroll(40);
    search("");
    vi.runOnlyPendingTimers();
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 480, animated: false });
    expect(readSessionView("host-a")).toMatchObject({ scrollOffset: 480, treeOffset: undefined });
  });

  it("lists a Board task's sessions by exact label and clears the task from its chip", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("linked", { labels: { "jira.issue": "ASMA-7" } }),
      sessionEntry("ticket", { labels: { "kontor.ticket": "T-7" } }),
      sessionEntry("titled", { title: "SWE • ASMA-7" }),
    ] };
    patchSessionView("host-a", { query: "", task: { jira: "ASMA-7", kontorId: "T-7", label: "ASMA-7" } });
    renderScreen(undefined, false);
    expect(sessionRowIds()).toEqual(["linked", "ticket"]);
    const chip = findByTestID(harness.current(), "sessions-task-filter");
    expect(textOf(chip)).toContain("Task ASMA-7");
    (chip.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").task).toBeNull();
    expect(testIDs(harness.current())).not.toContain("sessions-task-filter");
    expect(listRows().every((row) => row.kind !== "result")).toBe(true);
  });

  it("scopes an epic through the display mapping the tree uses", () => {
    const values = { ...DEFAULT_ORGANIZATION_SETTINGS, displayMappings: { "workspace:wks_mapped": { epic: "asma-9" } } };
    patchSessionView("host-a:epic", { query: "", scrollOffset: 0, ...toolbarOpenPatch(true) });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" },
      layout: { compact: true, platform: "ios" }, theme: { colors } as PluginTheme, params: {}, viewScope: "host-a:epic",
      directory: { state: { ...emptyDirectoryState(), status: "ready" as const, pages: 1, lastUpdatedAt: 0, entries: [
        sessionEntry("mapped", { workspaceId: "wks_mapped", labels: { epic: "ASMA-1" } }),
        sessionEntry("labelled", { workspaceId: "wks_other", labels: { epic: "ASMA-9" } }),
        sessionEntry("unmapped", { workspaceId: "wks_other", labels: { epic: "ASMA-1" } }),
      ] }, refresh: vi.fn() },
      epicKey: "ASMA-9",
      organization: { state: { status: "ready", values, revision: "r", saving: false, saveError: null, save: async () => true, reload: async () => {}, reset: async () => true }, save: async () => true },
    } as Parameters<typeof SessionsScreen>[0]));
    expandAll();
    expect(sessionRowIds().sort()).toEqual(["labelled", "mapped"]);
  });

  it("offers Plan, Board and Graph under an epic only for a configured plan source", () => {
    const onEpicTab = vi.fn();
    const render = (planSources?: Record<string, readonly string[]>) => {
      harness.reset();
      directory.state = { ...(directory.state as object), entries: [sessionEntry("a", { labels: { epic: "asma-7" } }), sessionEntry("b", { labels: { epic: "ASMA-8" } })] };
      patchSessionView("host-a", { query: "" });
      harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact: false, platform: "web" },
        theme: { colors } as PluginTheme, params: {}, planSources, onEpicTab } as Parameters<typeof SessionsScreen>[0]));
      const list = findByTestID(harness.current(), "sessions-list");
      const renderItem = list.props.renderItem as (info: { item: unknown }) => ElementLike;
      return groupRows().filter((row) => row.group.kind === "epic").map((row) => renderItem({ item: { ...row, expanded: true } }));
    };
    expect(render().map((row) => row.props.actions)).toEqual([undefined, undefined]);
    const [seven, eight] = render({ "ASMA-7": ["plan-a"], "ASMA-8": ["plan-b", "plan-c"] });
    const actions = (row: ElementLike) => row.props.actions as ElementLike[];
    expect(actions(seven!).map((action) => action.props.label)).toEqual(["Plan", "Board", "Graph"]);
    (actions(seven!)[2]!.props.onPress as () => void)();
    expect(onEpicTab).toHaveBeenLastCalledWith("Graph", "asma-7", "plan-a");
    (actions(eight!)[0]!.props.onPress as () => void)();
    expect(onEpicTab).toHaveBeenLastCalledWith("Plan", "ASMA-8", null);
    expect(actions(eight!)[0]!.props.accessibilityLabel).toBe("Plan for ASMA-8, choose among 2 configured plans");
    expect(render({ "ASMA-9": ["plan-z"] }).map((row) => row.props.actions)).toEqual([undefined, undefined]);
  });

  it("ignores a collapsed bottom bounce and its return while preserving top reveal on wide layouts", () => {
    const { scrollToOffset } = renderScreen(undefined, false);
    scroll(300);
    vi.advanceTimersByTime(300);
    scroll(1400);
    scrollToOffset.mockClear();

    scroll(1405);
    expect(fullToolbarShown()).toBe(false);
    expect(scrollToOffset).not.toHaveBeenCalled();
    expect(readSessionView("host-a").scrollOffset).toBe(1400);

    scroll(1400);
    expect(fullToolbarShown()).toBe(false);
    expect(scrollToOffset).not.toHaveBeenCalled();

    scroll(4);
    expect(fullToolbarShown()).toBe(true);
    expect(scrollToOffset).toHaveBeenLastCalledWith({
      offset: 4,
      animated: false,
    });
  });
});

function testIDs(node: unknown): string[] {
  return walk(node).flatMap((element) => (typeof element.props.testID === "string" ? [element.props.testID] : []));
}

function textOf(node: unknown): string {
  if (node === null || node === undefined || typeof node === "boolean") return "";
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textOf).join("");
  return textOf((node as ElementLike).props.children);
}

/** Shallow rendering stops at components; expand the slim-line ones to reach their controls. */
const EXPANDED_COMPONENTS = new Set<unknown>([SlimToolbar, SlimActionButton, ActiveFilterSummary]);
function expand(node: unknown): unknown {
  if (node === null || node === undefined || typeof node !== "object") return node;
  if (Array.isArray(node)) return node.map(expand);
  const element = node as ElementLike & { readonly type?: unknown };
  if (EXPANDED_COMPONENTS.has(element.type)) return expand((element.type as (props: unknown) => unknown)(element.props));
  if (!("props" in element)) return node;
  return { ...element, props: { ...element.props, children: expand(element.props.children) } };
}

function screen(): unknown {
  return expand(harness.current());
}

function press(testID: string) {
  (findByTestID(screen(), testID).props.onPress as () => void)();
  harness.flush();
}

type ScreenOptions = { compact?: boolean; viewScope?: string; scopeLabel?: string | null; onClearScope?: () => void };

function renderCompact(options: ScreenOptions = {}) {
  return harness.render(() =>
    SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" },
      layout: options.compact === false ? { compact: false, platform: "web" } : { compact: true, platform: "ios" },
      navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
      theme: { colors } as PluginTheme,
      params: {},
      viewScope: options.viewScope,
      scopeLabel: options.scopeLabel,
      onClearScope: options.onClearScope,
    } as Parameters<typeof SessionsScreen>[0]),
  );
}

/** Renders the session leaf the list would draw for the first session row. */
function renderSessionLeaf(): ElementLike & { readonly type: unknown } {
  const row = listRows().find((candidate) => candidate.kind === "session")!;
  const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
  const leaf = renderItem({ item: row }).props.children as ElementLike & { readonly type: (props: unknown) => ElementLike };
  return leaf.type(leaf.props) as ElementLike & { readonly type: unknown };
}

describe("SessionsScreen compact toolbar", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    accessibility.screenReader = false;
    clearSessionViews();
    harness.reset();
    directory.inputs = [];
    directory.state = {
      entries: [sessionEntry("session-a", { status: "idle" })],
      status: "ready",
      live: true,
      error: null,
      staleReason: null,
      pages: 1,
      lastUpdatedAt: 0,
      inventoryComplete: true,
      includeArchived: false,
    };
  });

  afterEach(() => {
    harness.unmount();
    vi.useRealTimers();
  });

  it("opens on one slim line: no heading, shortcuts, search or state chips above the tree", () => {
    renderCompact();
    const tree = screen() as ElementLike;
    const ids = testIDs(tree);
    for (const hidden of ["sessions-full-toolbar-slot", "sessions-full-toolbar", "sessions-heading", "sessions-shortcut-0", "sessions-search", "sessions-filter-all", "sessions-filter-idle", "sessions-advanced-toggle", "sessions-filter-summary-row", HIDE_FILTERS_TEST_ID]) {
      expect(ids).not.toContain(hidden);
    }
    // Only the slim slot sits above the list.
    const above = (tree.props.children as unknown[]).filter((child) => child !== null && child !== undefined && child !== false);
    expect(above.map((child) => (child as ElementLike).props.testID)).toEqual(["sessions-slim-toolbar-slot", "sessions-list"]);
    // The host name on one line; its live state is the top bar's pill.
    const host = findByTestID(tree, "sessions-slim-host");
    expect(host.props.style).toMatchObject({ flexDirection: "row" });
    expect(walk(host).filter((element) => element !== host).every((element) => element.props.numberOfLines === 1)).toBe(true);
    expect(textOf(host)).toBe("srv_host-a");
    expect(findByTestID(tree, "sessions-slim-search").props.accessibilityState).toEqual({ expanded: false });
    expect(findByTestID(tree, "sessions-slim-filters").props.accessibilityState).toEqual({ expanded: false });
    expect([findByTestID(tree, "sessions-slim-search").props["aria-expanded"], findByTestID(tree, "sessions-slim-filters").props["aria-expanded"]]).toEqual([false, false]);
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(false);
  });

  it("opens from Search and Filters, closes with Hide filters, and keeps the choice across re-entry", () => {
    renderCompact();
    press("sessions-slim-search");
    expect(fullToolbarShown()).toBe(true);
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(true);
    const focus = vi.fn();
    const blur = vi.fn();
    (findByTestID(harness.current(), "sessions-search").props.ref as RefLike).current = { focus, blur };
    vi.advanceTimersByTime(0);
    expect(focus).toHaveBeenCalledOnce();
    expect(findByTestID(screen(), "sessions-slim-search").props.accessibilityState).toEqual({ expanded: true });
    expect(findByTestID(screen(), "sessions-slim-search").props["aria-expanded"]).toBe(true);
    const hide = findByTestID(screen(), HIDE_FILTERS_TEST_ID);
    expect(textOf(hide)).toBe("Hide filters");
    expect(hide.props.accessibilityLabel).toBe("Hide filters");
    expect(hide.props.accessibilityState).toEqual({ expanded: true });
    expect((hide.props.style as { minHeight: number }).minHeight).toBeGreaterThanOrEqual(44);
    expect(testIDs(screen())).not.toContain("sessions-slim-filters");

    press(HIDE_FILTERS_TEST_ID);
    expect(blur).toHaveBeenCalledOnce();
    expect(fullToolbarShown()).toBe(false);
    expect(testIDs(harness.current())).not.toContain("sessions-search");
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(false);

    // Back and re-entry remount the screen; the store keeps the choice.
    harness.reset(); renderCompact();
    expect(fullToolbarShown()).toBe(false);
    press("sessions-slim-filters");
    expect(fullToolbarShown()).toBe(true);
    expect(testIDs(harness.current())).toContain("sessions-options");
    expect(readSessionView("host-a")).toMatchObject({ advancedOpen: true });
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(true);
    harness.reset(); renderCompact();
    expect(fullToolbarShown()).toBe(true);
    expect(testIDs(harness.current())).toContain("sessions-options");
  });

  it("closes the open toolbar after a small downward scroll even when the list is shorter than the screen", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    const { scrollToOffset } = renderScreen();
    const list = () => findByTestID(harness.current(), "sessions-list");
    (list().props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 500 } } });
    harness.flush();
    // The short list keeps room for the closing drag on web and Android.
    expect(list().props.contentContainerStyle).toMatchObject({ minHeight: 500 + COMPACT_SCROLL_SLACK });

    scroll(COMPACT_COLLAPSE_DISTANCE - 1, 500 + COMPACT_SCROLL_SLACK, 500);
    expect(fullToolbarShown()).toBe(true);
    scroll(COMPACT_COLLAPSE_DISTANCE + 6, 500 + COMPACT_SCROLL_SLACK, 500);
    expect(fullToolbarShown()).toBe(false);
    expect(compactToolbarOpen(readSessionView("host-a"))).toBe(false);
    // The toolbar never left the view, so the rows take its place from the top.
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 0, animated: false });
    expect(list().props.contentContainerStyle).not.toHaveProperty("minHeight");

    // iOS: a rubber-band drag past a 300px list in a 500px viewport closes it too.
    press("sessions-slim-search");
    vi.advanceTimersByTime(2_000);
    scroll(0, 300, 500);
    scroll(COMPACT_COLLAPSE_DISTANCE + 6, 300, 500);
    expect(fullToolbarShown()).toBe(false);
  });

  it("does not close while the search input or a toolbar control has focus", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    renderScreen();
    const search = () => findByTestID(harness.current(), "sessions-search");
    (search().props.onFocus as () => void)();
    scroll(60, 300, 500);
    expect(fullToolbarShown()).toBe(true);
    (search().props.onBlur as () => void)();

    const chip = () => findByTestID(harness.current(), "sessions-filter-idle");
    (chip().props.onFocus as () => void)();
    vi.advanceTimersByTime(2_000);
    scroll(120, 300, 500);
    expect(fullToolbarShown()).toBe(true);
    (chip().props.onBlur as () => void)();

    // With focus gone, the next deliberate scroll closes it.
    scroll(120 + COMPACT_COLLAPSE_DISTANCE, 300, 500);
    expect(fullToolbarShown()).toBe(false);
  });

  it("on web keeps the toolbar for keyboard focus but not for a chip focused by a click or tap", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    renderScreen();
    const chip = () => findByTestID(harness.current(), "sessions-filter-idle");
    // The browser marks keyboard focus as :focus-visible; a pointer press focuses without it.
    const focus = (visible: boolean) => (chip().props.onFocus as (event: unknown) => void)({ target: { matches: (selector: string) => selector === ":focus-visible" && visible } });
    focus(true);
    vi.advanceTimersByTime(2_000);
    scroll(120, 300, 500);
    expect(fullToolbarShown()).toBe(true);
    (chip().props.onBlur as () => void)();

    focus(false);
    // Only the short interaction pin holds; once it lapses, a deliberate scroll closes the toolbar.
    vi.advanceTimersByTime(2_000);
    scroll(120 + COMPACT_COLLAPSE_DISTANCE, 300, 500);
    expect(fullToolbarShown()).toBe(false);
  });

  it("tells a web screen reader which chips are on, which workstreams are checked, what is open and which sessions are favorites", () => {
    const organization = {
      state: { status: "ready", values: { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["session-a"] }, revision: "revision-a", saving: false, saveError: null, save: async () => true, reload: async () => {}, reset: async () => {} },
      save: async () => true,
    } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    directory.state = { ...(directory.state as object), entries: [sessionEntry("session-a", { labels: { workstream: "Adapter" } }), sessionEntry("session-b", { labels: { workstream: "Board" } })] };
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true, status: "idle", streams: ["adapter"] });
    renderScreen(organization);
    // React Native Web drops accessibilityState, so the state reaches the DOM only as ARIA.
    const host = (element: ElementLike & { readonly type?: unknown }) => (element.type as (props: unknown) => ElementLike)(element.props).props;
    const tree = harness.current();
    expect([host(findByTestID(tree, "sessions-filter-idle"))["aria-pressed"], host(findByTestID(tree, "sessions-filter-all"))["aria-pressed"]]).toEqual([true, false]);
    const streams = walk(tree).filter((element) => element.props.checkbox === true).map((element) => [element.props.label, host(element)["aria-checked"], host(element)["aria-pressed"]]);
    expect(streams).toEqual([["Adapter", true, undefined], ["Board", false, undefined]]);
    expect(host(findByTestID(tree, "sessions-advanced-archived"))["aria-checked"]).toBe(false);
    expect(host(walk(tree).find((element) => (element as { type?: unknown }).type === FiltersButton)!)["aria-expanded"]).toBe(true);

    patchSessionView("host-a", { status: "all", streams: null });
    harness.reset(); renderScreen(organization);
    const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
    const drawn = (row: unknown) => { const view = renderItem({ item: row }) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike }; return view.type(view.props); };
    const group = groupRows()[0]!;
    const opened = (expanded: boolean) => walk(drawn({ ...group, expanded })).find((element) => element.props.accessibilityRole === "button")!.props["aria-expanded"];
    expect([opened(true), opened(false)]).toEqual([true, false]);
    // A session row as the list draws it under an opened group.
    const star = (id: string) => walk(drawn({ kind: "session", entry: sessionEntry(id, {}), depth: 1, parentKey: group.group.key })).find((element) => /^(Un)?favorite /i.test(String(element.props.accessibilityLabel)))!.props["aria-pressed"];
    expect([star("session-a"), star("session-b")]).toEqual([true, false]);
  });

  it("keeps the toolbar for a native screen reader but not for React Native Web's always-on report", async () => {
    accessibility.screenReader = true;
    for (const [platform, open] of [["ios", true], ["web", false]] as const) {
      harness.reset(); clearSessionViews();
      patchSessionView("host-a", toolbarOpenPatch(true));
      renderScreen(undefined, true, platform);
      await Promise.resolve(); await Promise.resolve();
      scroll(120, 300, 500);
      expect(fullToolbarShown()).toBe(open);
    }
  });

  it("does not read an offset shift from rows inserted above the viewport as a closing scroll", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running" }), sessionEntry("quiet", { status: "idle" }),
    ] };
    patchSessionView("host-a", { query: "", status: "running", grouping: "flat", ...toolbarOpenPatch(true) });
    renderScreen();
    // Too short to scroll the 228px toolbar out of view, so the small drag decides.
    scroll(10, 700, 600);
    // Same entry order, one more matching row: the native list keeps its visible row by moving the offset.
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running" }), sessionEntry("quiet", { status: "running" }),
    ] };
    harness.flush();
    scroll(10 + 48, 748, 600);
    expect(fullToolbarShown()).toBe(true);
    scroll(10 + 48 + COMPACT_COLLAPSE_DISTANCE, 748, 600);
    expect(fullToolbarShown()).toBe(false);
  });

  it("stays open while arranging, so Move up and Move down remain reachable", () => {
    patchSessionView("host-a", { ...toolbarOpenPatch(true), arranging: true, advancedOpen: true });
    renderScreen();
    vi.advanceTimersByTime(2_000);
    scroll(300);
    expect(fullToolbarShown()).toBe(true);
  });

  it("keeps active filters and the page scope visible as one-tap chips while collapsed", () => {
    patchSessionView("host-a:scoped", { query: "", status: "idle", shortcut: "PR Review", scrollOffset: 0 });
    const onClearScope = vi.fn();
    renderCompact({ viewScope: "host-a:scoped", scopeLabel: "ASMA-1", onClearScope });
    const summary = findByTestID(screen(), FILTER_SUMMARY_TEST_ID);
    expect(textOf(summary)).toBe("Idle · PR Review✕");
    expect(summary.props.accessibilityLabel).toBe("Active filters: Idle, PR Review. Clear filters");
    expect((summary.props.style as { minHeight: number }).minHeight).toBeGreaterThanOrEqual(44);
    const filters = findByTestID(screen(), "sessions-slim-filters");
    expect(textOf(filters)).toBe("Filters2");
    expect(filters.props.accessibilityLabel).toBe("Filters and view, 2 active");

    const scope = findByTestID(screen(), SCOPE_SUMMARY_TEST_ID);
    expect(textOf(scope)).toContain("ASMA-1");
    (scope.props.onPress as () => void)();
    expect(onClearScope).toHaveBeenCalledOnce();

    press(FILTER_SUMMARY_TEST_ID);
    expect(readSessionView("host-a:scoped")).toMatchObject({ query: "", status: "all", shortcut: null, streams: null, includeArchived: false });
    expect(testIDs(screen())).not.toContain(FILTER_SUMMARY_TEST_ID);
    expect(fullToolbarShown()).toBe(false);
  });

  it("keeps source warnings visible outside the closed compact toolbar", () => {
    directory.state = { ...(directory.state as object), status: "stale", live: false, staleReason: "Live updates are unavailable." };
    renderCompact();
    expect(fullToolbarShown()).toBe(false);
    const notices = findByTestID(harness.current(), "sessions-notices");
    const banner = findByTestID(notices, "sessions-source-banner");
    expect(banner.props.disabled).toBeUndefined();
    expect(banner.props.actionLabel).toBe("Retry");
  });

  it("renders a compact session row as one line: title, muted context and the state badge", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("0123456789abcdef", {
      title: "A deliberately long session title that would otherwise wrap onto a second line on a phone",
      status: "running", pendingPermissionCount: 1, archivedAt: "2026-10-18T11:00:00.000Z", workspaceName: "RSW • review",
    })] };
    patchSessionView("host-a", { grouping: "flat", includeArchived: true });
    renderCompact();
    const leaf = renderSessionLeaf();
    expect((leaf.props.style as (state: { pressed: boolean }) => { minHeight: number })({ pressed: false }).minHeight).toBe(44);
    const title = findByTestID(leaf, "sessions-row-title");
    expect(title.props.numberOfLines).toBe(1);
    // The context is nested inside the title's single truncating line, not a second line.
    expect(walk(title).some((element) => element.props?.testID === "sessions-row-meta")).toBe(true);
    expect(textOf(findByTestID(leaf, "sessions-row-meta"))).toBe("  Running · RSW • review · Project · Archived");
    const lines = walk(leaf).filter((element) => (element as { type?: unknown }).type === "Text" && element.props.numberOfLines === 1);
    expect(lines.filter((line) => !walk(title).includes(line) || line === title)).toEqual([title]);
    expect(walk(leaf).some((element) => (element as { type?: unknown }).type === StateBadge && element.props.state === "needs_input")).toBe(true);
    // ST-02: the native ID is in the accessible label, never in the visible row.
    expect(textOf(leaf)).not.toContain("01234567");
    expect(leaf.props.accessibilityLabel).toContain("Native ID 0123456789abcdef");
  });

  it("draws the open toolbar inside the list's scroll content, under the pinned slim line", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    renderCompact();
    const root = harness.current() as ElementLike;
    // A drag on the toolbar is a drag on the list: only the slim line sits outside it.
    const above = (root.props.children as unknown[]).filter((child) => child !== null && child !== undefined && child !== false);
    expect(above.map((child) => (child as ElementLike).props.testID)).toEqual(["sessions-slim-toolbar-slot", "sessions-list"]);
    const list = findByTestID(root, "sessions-list");
    const header = list.props.ListHeaderComponent as ElementLike;
    const ids = testIDs(header);
    expect(ids).toEqual(expect.arrayContaining(["sessions-full-toolbar-slot", "sessions-full-toolbar", "sessions-heading", "sessions-search", "sessions-filter-all"]));
    // A complete, unfiltered read has no count line; the group rows carry the counts.
    expect(ids).not.toContain("sessions-count");
    // Its gap is list content too, so closing removes exactly the compensated height.
    expect(findByTestID(header, "sessions-full-toolbar-slot").props.style).toContainEqual({ paddingBottom: TOOLBAR_LAYOUT_GAP });
    // After typing, the first tap on a chip applies it rather than only dismissing the keyboard.
    expect(list.props.keyboardShouldPersistTaps).toBe("handled");
    expect(testIDs(findByTestID(screen(), "sessions-slim-toolbar-slot"))).toContain(HIDE_FILTERS_TEST_ID);
  });

  it("gives the space back on Hide filters without moving the rows in view", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    const { scrollToOffset } = renderScreen();
    const toolbarHeight = 220 + TOOLBAR_LAYOUT_GAP;
    // Typing pins the toolbar open while the list scrolls past it.
    (findByTestID(harness.current(), "sessions-search").props.onFocus as () => void)();
    scroll(600);
    expect(fullToolbarShown()).toBe(true);
    expect(scrollToOffset).not.toHaveBeenCalled();
    press(HIDE_FILTERS_TEST_ID);
    expect(fullToolbarShown()).toBe(false);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 600 - toolbarHeight, animated: false });
    expect(readSessionView("host-a")).toMatchObject({ scrollOffset: 600 - toolbarHeight, toolbarOpen: false });

    // Partly in view: the rows move up only by the part that was showing.
    vi.advanceTimersByTime(2_000);
    press("sessions-slim-search");
    vi.advanceTimersByTime(2_000);
    scroll(100);
    expect(fullToolbarShown()).toBe(true);
    scrollToOffset.mockClear();
    press(HIDE_FILTERS_TEST_ID);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 0, animated: false });
  });

  it("opens from Search or Filters at the top of the list, where the toolbar lives, and never on a scroll up", () => {
    patchSessionView("host-a", toolbarOpenPatch(true));
    const { scrollToOffset } = renderScreen();
    scroll(300);
    expect(fullToolbarShown()).toBe(false);
    vi.advanceTimersByTime(300);
    scroll(900);
    scroll(20);
    scroll(900);
    expect(fullToolbarShown()).toBe(false);
    scrollToOffset.mockClear();

    press("sessions-slim-filters");
    expect(fullToolbarShown()).toBe(true);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 0, animated: false });
    expect(readSessionView("host-a")).toMatchObject({ scrollOffset: 0, toolbarOpen: true, advancedOpen: true });

    // From the top the same rules close it again.
    vi.advanceTimersByTime(2_000);
    scroll(100);
    expect(fullToolbarShown()).toBe(true);
    scroll(300);
    expect(fullToolbarShown()).toBe(false);
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 300 - (220 + TOOLBAR_LAYOUT_GAP), animated: false });
  });

  it("leaves the host's live state to the top bar; a stale or offline host still gets the source banner", () => {
    const labels = new Set<string>();
    for (const hostStatus of ["online", "connecting", "idle", "offline", "error"] as const) {
      for (const status of ["idle", "loading", "ready", "stale", "error"] as const) {
        for (const live of [true, false]) labels.add(nativeFreshnessLabel({ hostStatus, status, live }));
      }
    }
    expect([...labels]).toEqual(expect.arrayContaining(["Live", "Stale", "Host offline"]));
    directory.state = { ...(directory.state as object), status: "stale", live: false, staleReason: "Live updates are unavailable." };
    for (const [hostId, compact] of [["host-a", true], ["host-offline", true], ["host-offline", false]] as const) {
      harness.reset();
      harness.render(() => SessionsScreen({
        host: { id: hostId, label: `srv_${hostId}` },
        layout: compact ? { compact: true, platform: "ios" } : { compact: false, platform: "web" },
        navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
        theme: { colors } as PluginTheme,
        params: {},
      } as PluginScreenProps));
      const slim = textOf(findByTestID(screen(), "sessions-slim-toolbar-slot"));
      expect(slim).toContain(`srv_${hostId}`);
      for (const label of labels) {
        expect(slim).not.toContain(label);
      }
      const banner = findByTestID(harness.current(), "sessions-source-banner");
      expect(banner.props.message).toContain("Live updates are unavailable.");
      expect(banner.props.actionLabel).toBe("Retry");
    }
  });

  it("leaves wide layouts unchanged, including after a compact session", () => {
    patchSessionView("host-a", { query: "", grouping: "flat", ...toolbarOpenPatch(false) });
    let compact = false;
    harness.render(() => SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" },
      layout: compact ? { compact: true, platform: "ios" } : { compact: false, platform: "web" },
      navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined },
      theme: { colors } as PluginTheme,
      params: {},
    } as PluginScreenProps));
    expect(fullToolbarShown()).toBe(true);
    expect(findByTestID(harness.current(), "sessions-slim-toolbar-slot").props.accessibilityElementsHidden).toBe(true);
    expect(findByTestID(screen(), "sessions-slim-search").props.accessibilityState).toBeUndefined();
    const ids = testIDs(screen());
    for (const absent of [HIDE_FILTERS_TEST_ID, "sessions-filter-summary-row", "sessions-notices", "sessions-list-header"]) {
      expect(ids).not.toContain(absent);
    }
    expect(ids).toEqual(expect.arrayContaining(["sessions-search", "sessions-heading", "sessions-shortcut-0", "sessions-filter-idle"]));
    expect(findByTestID(harness.current(), "sessions-list").props.onLayout).toBeUndefined();
    expect(findByTestID(harness.current(), "sessions-list").props.keyboardShouldPersistTaps).toBeUndefined();
    // The full toolbar stays above the list, outside its scroll content.
    const root = harness.current() as ElementLike;
    expect((root.props.children as unknown[]).some((child) => (child as ElementLike | null)?.props?.testID === "sessions-full-toolbar-slot")).toBe(true);
    expect(findByTestID(root, "sessions-list").props.ListHeaderComponent).toBeNull();
    // Wide session rows keep the title and the right-hand state column.
    const leaf = renderSessionLeaf();
    expect(testIDs(leaf)).not.toContain("sessions-row-meta");
    const column = walk(leaf).find((element) => (element.props.style as { width?: number } | undefined)?.width === 95);
    expect(column && walk(column).some((element) => (element as { type?: unknown }).type === StateBadge)).toBe(true);

    compact = true; harness.flush();
    expect(fullToolbarShown()).toBe(false);
    compact = false; harness.flush();
    expect(fullToolbarShown()).toBe(true);
  });

  it("chooses Workspace scope and Group by from compact selects that keep every choice, mark the current one and return focus", () => {
    const save = vi.fn(async () => true);
    const organization = {
      state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true },
      save,
    } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    directory.state = { ...(directory.state as object), entries: [sessionEntry("desk", { labels: { desk: "Ops" } }), sessionEntry("epic-work")] };
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true });
    renderScreen(organization);
    const select = (id: string) => findByTestID(harness.current(), id).props;
    const menus = () => walk(harness.current()).filter((element) => /^sessions-(scope|grouping)-menu$/.test(String(element.props.testID))).map((element) => element.props.testID);
    const items = () => walk(harness.current()).filter((element) => typeof element.props.onPress === "function" && typeof element.props.selected === "boolean" && "tall" in element.props);
    const choose = (label: string) => { (items().find((item) => item.props.label === label)!.props.onPress as () => void)(); harness.flush(); };
    // React Native Web drops accessibilityState, so the open state reaches the DOM only as ARIA.
    const ariaExpanded = (id: string) => {
      const element = findByTestID(harness.current(), id) as ElementLike & { readonly type: (props: unknown) => unknown };
      return walk(element.type(element.props)).find((node) => node.props.accessibilityRole === "button")!.props["aria-expanded"];
    };
    const scopeFocus = vi.fn(), groupingFocus = vi.fn();
    (select("sessions-scope").anchor as RefLike).current = { focus: scopeFocus };
    (select("sessions-grouping").anchor as RefLike).current = { focus: groupingFocus };
    expect([select("sessions-scope").value, select("sessions-scope").expanded, menus()]).toEqual(["All Paseo workspaces", false, []]);
    expect(select("sessions-filter-all").count).toBe(2);

    (select("sessions-scope").onPress as () => void)(); harness.flush();
    expect([select("sessions-scope").expanded, ariaExpanded("sessions-scope"), menus()]).toEqual([true, true, ["sessions-scope-menu"]]);
    // The menu frame is clamped to the screen (390x320: 130pt for five 44pt choices), so the choices scroll inside it.
    const choices = (id: string) => {
      const menu = findByTestID(harness.current(), `sessions-${id}-menu`);
      const children = [menu.props.children].flat() as (ElementLike & { readonly type: unknown })[];
      expect(children.map((child) => child.type)).toEqual(["ScrollView"]);
      expect(children[0]!.props.style).toEqual({ flexGrow: 0, flexShrink: 1 });
      return walk(children[0]).filter((element) => "tall" in element.props).map((element) => element.props.label);
    };
    expect(choices("scope")).toEqual(["All Paseo workspaces", "Durable desks", "PR Review desk", "Adam desk", "Other workspaces"]);
    // Durable desks lives only here; the four others repeat the desk shortcuts.
    expect(items().map((item) => [item.props.label, item.props.selected])).toEqual([["All Paseo workspaces", true], ["Durable desks", false], ["PR Review desk", false], ["Adam desk", false], ["Other workspaces", false]]);
    choose("Durable desks");
    expect([select("sessions-scope").value, select("sessions-scope").expanded, ariaExpanded("sessions-scope"), menus(), scopeFocus.mock.calls.length]).toEqual(["Durable desks", false, false, [], 1]);
    expect(readSessionView("host-a").shortcut).toBe("Desks");
    expect(select("sessions-filter-all").count).toBe(1);
    expect([0, 1, 2, 3].map((index) => select(`sessions-shortcut-${index}`).active)).toEqual([false, false, false, false]);
    // A desk shortcut picks the same scope, and the select follows it.
    (select("sessions-shortcut-2").onPress as () => void)(); harness.flush();
    expect(select("sessions-scope").value).toBe("Adam desk");

    // Escape, a tap outside or Android back close through onClose without a choice.
    (select("sessions-scope").onPress as () => void)(); harness.flush();
    (findByTestID(harness.current(), "sessions-scope-menu").props.onClose as () => void)(); harness.flush();
    expect([select("sessions-scope").value, select("sessions-scope").expanded, menus()]).toEqual(["Adam desk", false, []]);

    (select("sessions-grouping").onPress as () => void)(); harness.flush();
    expect(items().map((item) => [item.props.label, item.props.selected])).toEqual([["Workstreams", true], ["Desks & workspaces", false], ["Role", false], ["Flat", false]]);
    expect(choices("grouping")).toEqual(["Workstreams", "Desks & workspaces", "Role", "Flat"]);
    choose("Role");
    expect([select("sessions-grouping").value, menus(), groupingFocus.mock.calls.length]).toEqual(["Role", [], 1]);
    expect(save).toHaveBeenCalledWith({ grouping: "role" });

    // A menu left open closes with the options panel and does not come back when it reopens.
    (select("sessions-scope").onPress as () => void)(); harness.flush();
    const filters = () => walk(harness.current()).find((element) => (element as { type?: unknown }).type === FiltersButton)!.props;
    (filters().onPress as () => void)(); harness.flush();
    expect(menus()).toEqual([]);
    (filters().onPress as () => void)(); harness.flush();
    expect([select("sessions-scope").expanded, menus()]).toEqual([false, []]);
  });

  it("bounds compact Filters & view to its own scroll frame, so the list keeps a share of the viewport and every control stays in the frame", () => {
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true });
    renderScreen();
    const options = () => findByTestID(harness.current(), "sessions-options") as ElementLike & { readonly type: unknown };
    const layout = (testID: string, values: { y?: number; height?: number }) => {
      (findByTestID(harness.current(), testID).props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: 390, height: 0, ...values } } });
      harness.flush();
    };
    expect([options().type, options().props.nestedScrollEnabled, options().props.keyboardShouldPersistTaps]).toEqual(["ScrollView", true, "handled"]);
    // Unmeasured, the frame is not guessed.
    expect((options().props.style as { maxHeight?: number }).maxHeight).toBeUndefined();
    // 390x844, touch: a 585pt list viewport, and the frame starts 278pt down the toolbar.
    layout("sessions-list", { height: 585 });
    layout("sessions-options", { y: 278 });
    const frame = () => (options().props.style as { maxHeight?: number; flexGrow?: number });
    expect(frame()).toMatchObject({ flexGrow: 0, maxHeight: 132 });
    // The toolbar ends at 70% of the viewport; 175pt below it still shows sessions.
    expect([OPTIONS_MIN_HEIGHT, OPTIONS_SPACE_SHARE, 585 - 278 - frame().maxHeight!]).toEqual([132, 0.7, 175]);
    // A taller viewport gives the frame the extra room; a shorter one keeps three 44pt controls.
    layout("sessions-list", { height: 760 });
    expect(frame().maxHeight).toBe(254);
    layout("sessions-list", { height: 400 });
    expect(frame().maxHeight).toBe(132);
    // Every control is still in the frame: nothing moved out or was dropped.
    const ids = walk(options()).map((element) => element.props.testID).filter(Boolean);
    expect(ids).toEqual(expect.arrayContaining(["sessions-grouping", "sessions-scope", "sessions-filter-initializing", "sessions-filter-closed", "sessions-advanced-archived", "sessions-arrange", "sessions-expand-all", "sessions-collapse-all", "sessions-options-count"]));
    expect(walk(options()).some((element) => element.props.label === "Legend")).toBe(true);
  });

  it("bounds wide Filters & view by the screen it shares with the list, so a short window keeps every control in the frame", () => {
    patchSessionView("host-a", { advancedOpen: true });
    renderScreen(undefined, false);
    const frame = () => findByTestID(harness.current(), "sessions-options").props.style as { maxHeight?: number };
    const layout = (target: ElementLike, values: { y?: number; height?: number }) => {
      (target.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: 1200, height: 0, ...values } } });
      harness.flush();
    };
    expect(frame().maxHeight).toBeUndefined();
    // 1440x900, mouse: a 752pt screen with the frame 120pt down the toolbar has room for the whole 354pt panel.
    layout(harness.current() as ElementLike, { height: 752 });
    layout(findByTestID(harness.current(), "sessions-options"), { y: 120 });
    expect(frame().maxHeight).toBe(384);
    // 1440x600, touch: the frame scrolls instead of pushing Arrange, Expand all, Collapse all and Legend off the screen.
    layout(harness.current() as ElementLike, { height: 452 });
    layout(findByTestID(harness.current(), "sessions-options"), { y: 156 });
    expect(frame().maxHeight).toBe(138);
    layout(harness.current() as ElementLike, { height: 300 });
    expect(frame().maxHeight).toBe(132);
  });
});
