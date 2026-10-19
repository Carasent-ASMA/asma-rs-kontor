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
  HIDE_FILTERS_TEST_ID,
  SCOPE_SUMMARY_TEST_ID,
  SessionsScreen,
  SlimActionButton,
  SlimToolbar,
  toolbarOpenPatch,
} from "../client/sessions-screen";
import { StateBadge } from "../client/organization-row";
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

function listRows(): import("../client/session-organization").OrganizationRow[] {
  return findByTestID(harness.current(), "sessions-list").props.data as import("../client/session-organization").OrganizationRow[];
}

function groupRows(): GroupRow[] {
  return listRows().filter((row): row is GroupRow => row.kind === "group");
}

function sessionRowIds(): string[] {
  return listRows().flatMap((row) => (row.kind === "session" ? [row.entry.id] : []));
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
    expect(count).toContain('"1"'); expect(count).not.toContain("3");
    expect(count).toContain("session"); expect(count).toContain("partial inventory read");
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

  it("reveals matching descendants for a state filter without overwriting the saved disclosure", () => {
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
    expect(ids).toEqual(expect.arrayContaining(["sessions-full-toolbar-slot", "sessions-full-toolbar", "sessions-heading", "sessions-search", "sessions-filter-all", "sessions-count"]));
    expect(ids.indexOf("sessions-full-toolbar-slot")).toBeLessThan(ids.indexOf("sessions-count"));
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
    expect(testIDs(findByTestID(root, "sessions-list").props.ListHeaderComponent)).toEqual(["sessions-count"]);
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
});
