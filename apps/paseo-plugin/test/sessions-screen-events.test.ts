import type { PluginTheme } from "@getpaseo/plugin";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import type {
  NativeScrollEvent,
  NativeSyntheticEvent,
} from "react-native";
import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { NATIVE_DESK_PROJECTS, NATIVE_DESK_WORKSPACES, nativeDeskSessions } from "./fixtures/native-desks";

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
  Platform: { OS: "web" },
  Pressable: "Pressable",
  Text: "Text",
  View: "View",
}));

vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: () => async () => { throw new Error("Lifecycle RPC unavailable in this fixture"); },
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
import { SessionLifecycleDialog } from "../client/session-lifecycle-view";
import {
  ActiveFilterSummary,
  COMPACT_COLLAPSE_DISTANCE,
  COMPACT_SCROLL_SLACK,
  compactToolbarOpen,
  FILTER_SUMMARY_TEST_ID,
  FiltersButton,
  ClearFiltersButton,
  HIDE_FILTERS_TEST_ID,
  OPTIONS_MIN_HEIGHT,
  OPTIONS_SPACE_SHARE,
  SCOPE_SUMMARY_TEST_ID,
  SessionDetailsButton,
  sessionDetailLines,
  SessionRow,
  resultCues,
  SessionsScreen,
  SessionTimeLines,
  SHORT_PEEK_HEIGHT,
  SlimActionButton,
  SlimToolbar,
  toolbarOpenPatch,
} from "../client/sessions-screen";
import { MobileFiltersPage } from "../client/mobile-filters";
import { MobileQuickToolbar } from "../client/mobile-quick-toolbar";
import { MobileQuickChooser } from "../client/mobile-quick-chooser";
import { StateBadge } from "../client/organization-row";
import { SessionUsage, SessionUsageLines } from "../client/session-usage-view";
import { AnchoredMenu, MenuItem } from "../client/anchored-menu";
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

function sessionSearch(): ElementLike {
  const toolbar = walk(harness.current()).find((node) => node.type === MobileQuickToolbar);
  return toolbar ? { props: { value: toolbar.props.query, onChangeText: toolbar.props.onQuery } } : findByTestID(harness.current(), "sessions-search");
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

function renderScreen(organization?: Parameters<typeof SessionsScreen>[0]["organization"], compact = false, platform: "ios" | "web" = compact ? "ios" : "web", native?: Pick<Parameters<typeof SessionsScreen>[0], "projects" | "workspaces" | "projectId" | "epicKey" | "workstreamSources" | "workstreamReads" | "workstreamRegistry" | "planMappings">) {
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
      ...native,
    } as PluginScreenProps),
  );
  const list = findByTestID(tree, "sessions-list");
  const fullToolbar = walk(tree).find((element) => element.props.testID === "sessions-full-toolbar");
  const slimSlot = walk(tree).find((element) => element.props.testID === "sessions-slim-toolbar-slot");
  if (fullToolbar) (fullToolbar.props.onLayout as (event: unknown) => void)({
    nativeEvent: { layout: { x: 0, y: 0, width: 400, height: 220 } },
  });
  if (slimSlot) (slimSlot.props.onLayout as (event: unknown) => void)({
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
  const mobile = walk(harness.current()).some((node) => node.type === MobileQuickToolbar);
  if (mobile) openMobileFilters();
  const options = walk(harness.current()).some((element) => element.props.testID === "sessions-options");
  if (!options) {
    const toggle = walk(findByTestID(harness.current(), "sessions-full-toolbar")).find((element) => element.props.expanded === false && "activeCount" in element.props)!;
    (toggle.props.onPress as () => void)(); harness.flush();
  }
  (findByTestID(harness.current(), "sessions-expand-all").props.onPress as () => void)(); harness.flush();
  if (mobile) closeMobileFilters();
}
function setStateFilter(status: string) {
  const mobile = walk(harness.current()).some((node) => node.type === MobileQuickToolbar);
  if (mobile) openMobileFilters();
  (findByTestID(harness.current(), `sessions-filter-${status}`).props.onPress as () => void)(); harness.flush();
  if (mobile) closeMobileFilters();
}

function fullToolbarShown(): boolean {
  const slots = walk(harness.current()).filter((element) => element.props.testID === "sessions-full-toolbar-slot");
  expect(slots.length).toBeLessThanOrEqual(1);
  return slots[0]?.props.accessibilityElementsHidden === false;
}

describe("SessionsScreen toolbar events", () => {
  it.each([true, false])("keeps exactly one enabled recovery entry on compact=%s", (compact) => {
    renderScreen(undefined, compact);
    const tree = walk(harness.current());
    const recovery = tree.filter((element) => element.props.accessibilityLabel === "Recover sessions");
    const slim = tree.find((element) => element.type === SlimToolbar);
    const inSlim = slim?.props.action as ElementLike | undefined;
    expect(recovery.length + (inSlim ? 1 : 0)).toBe(1);
    expect((inSlim ?? recovery[0]!)?.props.disabled).toBe(false);
    expect(tree.some((element) => element.type === MobileQuickToolbar)).toBe(compact);
    expect(inSlim).toBeUndefined();
  });
  it("opens recovery from the mobile quick toolbar and keeps the existing search after close", () => {
    renderScreen(undefined, true);
    const quick = walk(harness.current()).find((element) => element.type === MobileQuickToolbar)!;
    (quick.props.onQuery as (query: string) => void)("session-a");
    harness.flush();
    const recovery = walk(harness.current()).find((element) => element.props.accessibilityLabel === "Recover sessions")!;
    (recovery.props.onPress as () => void)();
    harness.flush();
    const dialog = walk(harness.current()).find((element) => element.type === SessionLifecycleDialog)!;
    expect(dialog.props.scopeIds).toEqual(["session-a"]);
    expect(walk(harness.current()).some((element) => element.type === MobileQuickToolbar)).toBe(true);
    (dialog.props.onClose as () => void)();
    harness.flush();
    expect(walk(harness.current()).some((element) => element.type === SessionLifecycleDialog)).toBe(false);
    expect(readSessionView("host-a").query).toBe("session-a");
  });
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

  it.each([true, false])("keeps history neutral in rows and filters on compact=%s", (compact) => {
    const history = Array.from({ length: 13 }, (_, n) => sessionEntry(`history-${n}`, {
      title: `Closed review ${n}`, status: "closed", requiresAttention: true, attentionReason: "error",
      workspaceName: n < 10 ? "Archived workspace" : "Current workspace",
    }));
    directory.state = { ...(directory.state as object), entries: [...history,
      sessionEntry("current", { status: "error", requiresAttention: true, attentionReason: "error" }),
      sessionEntry("unread", { requiresAttention: true, attentionReason: "finished" }),
    ], includeArchived: false };
    patchSessionView("host-a", { query: "", includeArchived: false });
    renderScreen(undefined, compact);
    if (compact) openMobileFilters();
    expect(findByTestID(harness.current(), "sessions-filter-error").props.count).toBe(1);
    expect(findByTestID(harness.current(), "sessions-filter-attention").props.count).toBe(2);
    expect(findByTestID(harness.current(), "sessions-filter-closed").props.count).toBe(13);
    const earlier = findByTestID(harness.current(), "sessions-filter-earlier_error");
    expect(earlier.props.label).toBe("Earlier errors");
    expect(earlier.props.count).toBe(13);
    expect(earlier.props.dot).not.toBe(colors.statusDanger);
    (earlier.props.onPress as () => void)(); harness.flush();
    if (compact) closeMobileFilters();
    expect(sessionRowIds().sort()).toEqual(history.map((entry) => entry.id).sort());
    expect(resultCues(history[0]!)).toContain("Earlier error · now closed");
    const row = SessionRow({ entry: history[0]!, colors, compact, onPress: () => {}, displayContext: "Archived workspace" });
    expect(textOf(row)).toContain("Earlier error · now closed");
    const pressable = walk(row).find((element) => element.props.testID === "sessions-row-history-0")!;
    expect(pressable.props.accessibilityLabel).toContain("Native status closed");
    expect(pressable.props.accessibilityLabel).toContain("Earlier error · now closed");
    expect(pressable.props.accessibilityLabel).not.toContain("Needs attention");
    expect(walk(row).find((element) => element.type === StateBadge)!.props.state).toBe("closed");
    // Archive remains an explicit independent option, and never hides closed-unarchived history.
    if (compact) openMobileFilters();
    (findByTestID(harness.current(), "sessions-advanced-archived").props.onPress as () => void)(); harness.flush();
    expect(directory.inputs.at(-1)?.includeArchived).toBe(true);
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toHaveLength(13);
    setStateFilter("error");
    expect(sessionRowIds()).toEqual(["current"]);
  });

  it("shows the count line only for results or an incomplete read, and keeps the full count and read in Filters", () => {
    const list = () => findByTestID(harness.current(), "sessions-list");
    const header = () => list().props.ListHeaderComponent;
    renderScreen();
    const search = (query: string) => { (sessionSearch().props.onChangeText as (query: string) => void)(query); harness.flush(); };
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
      host: { id: "host-a", label: "srv_host-a" }, layout: { compact: false, platform: "web" },
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
    expect(sessionSearch().props.value).toBe(
      "host-a query",
    );

    activeHost = "host-b";
    harness.flush();
    expect(sessionSearch().props.value).toBe(
      "host-b query",
    );

    const search = sessionSearch();
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
    expect(sessionSearch().props.value).toBe("other");
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 180, animated: false });
    expect(directory.inputs.at(-1)?.includeArchived).toBe(false);
    (sessionSearch().props.onChangeText as (query: string) => void)("edited b");
    harness.flush();
    expect(readSessionView("host-a:epic-b").query).toBe("edited b");
    expect(readSessionView("host-a:epic-a")).toMatchObject({ query: "session-a", scrollOffset: 420 });
    expect(readSessionView("host-a").query).toBe("host default");

    viewScope = "host-a:epic-a"; harness.flush(); vi.advanceTimersByTime(0);
    expect(sessionSearch().props.value).toBe("session-a");
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

  it("selects an empty native desk by ID, distinguishes duplicate names and retains selection across rename", () => {
    patchSessionView("host-a", { query: "", advancedOpen: true, shortcut: null, toolbarOpen: true });
    const native: Pick<Parameters<typeof SessionsScreen>[0], "projects" | "workspaces"> = { projects: [{ id: "prj_one", name: "DESK • RESEARCH" }, { id: "prj_two", name: "DESK • RESEARCH" }], workspaces: [] };
    renderScreen(undefined, false, "web", native);
    const openScope = () => { (findByTestID(harness.current(), "sessions-scope").props.onPress as () => void)(); harness.flush(); };
    openScope();
    const choices = walk(harness.current()).filter((element) => element.type === MenuItem && element.props.label === "DESK • RESEARCH");
    expect(choices.map((choice) => choice.props.detail)).toEqual(["prj_one", "prj_two"]);
    (choices[0]!.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").shortcut).toBe("desk-project:prj_one");
    expect(sessionRowIds()).toEqual([]);
    expect(findByTestID(harness.current(), "sessions-scope").props.value).toBe("DESK • RESEARCH");
    native.workspaces = [{ id: "wks_empty_desk", name: "CSW • empty native desk", projectKey: "prj_one", projectName: "DESK • RESEARCH" }];
    harness.flush();
    expect(groupRows().filter((row) => row.group.kind === "workspace").map((row) => row.group.label)).toEqual(["CSW • empty native desk"]);
    native.projects = [{ id: "prj_one", name: "DESK • DOCS" }, native.projects![1]!];
    harness.flush();
    expect(findByTestID(harness.current(), "sessions-scope").props.value).toBe("DESK • DOCS");
    expect(readSessionView("host-a").shortcut).toBe("desk-project:prj_one");
    openScope();
    expect(walk(harness.current()).find((element) => element.type === MenuItem && element.props.label === "DESK • DOCS")!.props.selected).toBe(true);
  });

  it.each([true, false])("keeps scoped native desk search, Details and Show in tree on one workspace (compact %s)", (compact) => {
    directory.state = { ...(directory.state as object), entries: nativeDeskSessions() };
    patchSessionView("host-a", { query: "asma-99102 desk-epic-b", grouping: "hierarchy", shortcut: "desk-project:prj_desk", status: "all", scrollOffset: 390, toolbarOpen: true });
    const { scrollToOffset } = renderScreen(undefined, compact, compact ? "ios" : "web", { projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES, projectId: "prj_desk" });
    vi.advanceTimersByTime(0);
    expect(sessionRowIds()).toEqual(["desk-epic-b"]);
    const result = listRows()[0]!;
    const draw = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
    const row = draw({ item: result });
    expect(row.props.provenance).toMatchObject({ parts: ["DESK • ADAM", "ADAM"], nativeDesk: { id: "prj_desk", workspaceId: "w_adam" } });
    expect((row.props.lifecycleAction as ElementLike).props.session).toMatchObject({ id: "desk-epic-b", workspaceId: "w_adam" });
    ((row.props.details as ElementLike).props.onPress as () => void)(); harness.flush();
    const details = findByTestID(harness.current(), "sessions-details");
    expect(JSON.stringify(details.props.children)).toContain("w_adam");
    expect(JSON.stringify(details.props.children)).toContain("Workstream association");
    (row.props.onShowInTree as (entry: unknown) => void)((result as Extract<ListRow, { kind: "result" }>).entry); harness.flush();
    expect(groupRows().filter((row) => row.group.kind === "project").map((row) => row.group.key)).toEqual(["project:prj_desk"]);
    expect(groupRows().some((row) => row.group.kind === "epic")).toBe(false);
    expect(groupRows().find((row) => row.group.key === "project:prj_desk/workspace:id%3Aw_adam")?.expanded).toBe(true);
    expect(sessionRowIds()).toContain("desk-epic-b");
    const nativeRow = listRows().find((item) => item.kind === "session" && item.entry.id === "desk-epic-b")!;
    expect((draw({ item: nativeRow }).props.lifecycleAction as ElementLike).props.session).toMatchObject({ id: "desk-epic-b", workspaceId: "w_adam" });
    const back = walk(findByTestID(harness.current(), "sessions-peek")).find((element) => element.props.label === "Back to 1 result")!;
    (back.props.onPress as () => void)(); harness.flush(); vi.advanceTimersByTime(0);
    expect(sessionRowIds()).toEqual(["desk-epic-b"]);
    expect(readSessionView("host-a")).toMatchObject({ query: "asma-99102 desk-epic-b", shortcut: "desk-project:prj_desk", grouping: "hierarchy", peek: null });
    expect(scrollToOffset).toHaveBeenCalled();
  });

  it.each([true, false])("explains incomplete workstream availability with zero, one and unregistered associations (compact=%s)", (compact) => {
    patchSessionView("host-a", { advancedOpen: true });
    directory.state = { ...(directory.state as object), entries: [] };
    renderScreen(undefined, compact);
    if (compact) openMobileFilters();
    expect(textOf(findByTestID(harness.current(), "sessions-workstream-availability"))).toContain("this list may be incomplete");
    const optionText = () => textOf(findByTestID(harness.current(), "sessions-options"));
    expect(optionText()).toContain("No workstream associations available in this context.");
    const choices = () => walk(harness.current()).filter((node) => node.props.checkbox === true);
    expect(choices()).toHaveLength(0);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {})] };
    harness.flush();
    expect(choices().map((node) => node.props.label)).toEqual(["Other work"]);
    expect(optionText()).not.toContain("No workstream associations available");
    (choices()[0]!.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").streams).toEqual([]);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {}), sessionEntry("unknown", { labels: { workstream: "unregistered-key" } })] };
    harness.flush();
    expect(choices().map((node) => node.props.label).sort()).toEqual(["Other work", "unregistered-key"]);
    expect(textOf(findByTestID(harness.current(), "sessions-workstream-availability"))).toContain("Names may be unregistered");
    expect(readSessionView("host-a").streams).toEqual([]);
  });

  it("does not insert unrelated empty native desks into a project or epic context", () => {
    directory.state = { ...(directory.state as object), entries: nativeDeskSessions() };
    patchSessionView("host-a", { query: "", grouping: "hierarchy", shortcut: null, status: "all" });
    renderScreen(undefined, true, "ios", { projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES, projectId: "prj_control" });
    expect(groupRows().some((row) => row.group.project)).toBe(false);
    harness.unmount(); harness.reset();
    renderScreen(undefined, true, "ios", { projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES, epicKey: "ASMA-99102" });
    expect(groupRows().filter((row) => row.group.project).map((row) => row.group.project!.id)).toEqual(["prj_desk"]);
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
    setStateFilter("idle");
    expect(sessionRowIds()).toEqual(["quiet"]);
    expect(findByTestID(harness.current(), "sessions-collapse-all").props.disabled).toBe(true);
    setStateFilter("all");
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
    setStateFilter("idle");
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
    setStateFilter("idle");
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
    setStateFilter("all");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
    expect(save).not.toHaveBeenCalled();
  });

  it("on a short compact screen, Show in tree keeps one Back row above the list, and Back brings the toolbar and filters back", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("busy", { status: "running", labels: { epic: "ASMA-1" } }),
      sessionEntry("quiet", { status: "idle", labels: { epic: "ASMA-1" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    renderScreen(undefined, true);
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
    setStateFilter("idle");
    // Results keep their toolbar and filters at any height; only the peek folds them.
    expect(walk(harness.current()).some((node) => node.type === MobileQuickToolbar)).toBe(true);
    expect(header()).not.toContain("sessions-full-toolbar");
    showInTree();
    expect(walk(harness.current()).some((node) => node.type === MobileQuickToolbar)).toBe(false);
    expect(header()).toEqual([]);
    expect([textOf(peekText()), peekText().props.numberOfLines, peekText().props.accessibilityLabel]).toEqual(["In the tree · not saved", 1, "Showing “Session quiet” in the tree. Branches opened here are not saved."]);
    // A full-size Back, the 44pt touch action.
    expect(back().props.small).toBe(false);
    expect(sessionRowIds()).toEqual(expect.arrayContaining(["quiet"]));
    (back().props.onPress as () => void)(); harness.flush();
    expect(walk(harness.current()).some((node) => node.type === MobileQuickToolbar)).toBe(true);
    expect(header()).not.toContain("sessions-full-toolbar");
    expect(sessionRowIds()).toEqual(["quiet"]);
    // A phone-height screen keeps the toolbar and the longer note during the peek.
    rootHeight(SHORT_PEEK_HEIGHT);
    showInTree();
    expect(walk(harness.current()).some((node) => node.type === MobileQuickToolbar)).toBe(true);
    expect(header()).not.toContain("sessions-full-toolbar");
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
    setStateFilter("idle");
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
    setStateFilter("idle");
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
    const content = () => (findByTestID(dialog()!.props.children, "sessions-details-lines").props.children as unknown[]).flat() as (ElementLike & { readonly type: unknown })[];
    // The times come first, drawn by their own clock-driven component; the role and place lines follow.
    const times = () => content()[0]!;
    const usage = () => content().find((line) => line.type === SessionUsageLines)!;
    const lines = () => content().filter((line) => line.type === "View")
      .map((line) => (line.props.children as ElementLike[]).map(textOf).join(": "));
    const close = () => walk(dialog()!.props.header).find((element) => element.props.label === "Close")!;
    const open = (id: string) => { (details(id).props.onPress as () => void)(); harness.flush(); };
    return { openAgent, save, row, details, dialog, times, usage, lines, close, open };
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

  it("anchors the details beside the row on a wide screen, closes them when the grouping changes, and gives every row its details", () => {
    const screen = renderDetails(placeEntries(), false);
    screen.open("desk");
    expect(screen.dialog()!.props.placement).toEqual({ align: "end", width: 360, maxHeight: 440 });
    (walk(harness.current()).find((element) => element.props.testID === "sessions-grouping")!.props.onPress as () => void)(); harness.flush();
    (walk(harness.current()).find((element) => element.type === MenuItem && element.props.label === "Desks & workstreams")!.props.onPress as () => void)(); harness.flush();
    expect(screen.dialog()).toBeUndefined();
    expandAll();
    const sessions = listRows().filter((row) => row.kind === "session");
    expect(sessions).toHaveLength(5);
    const buttons = sessions.map((row) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: row }).props.details as ElementLike);
    expect(buttons.map((button) => [button.type, button.props.title, button.props.expanded])).toEqual(sessions.map((row) => [SessionDetailsButton, row.kind === "session" ? row.entry.title : null, false]));
    // Workstreams details open the same way, with the same lines.
    (buttons[0]!.props.onPress as () => void)(); harness.flush();
    expect(screen.dialog()!.props.label).toBe(`Details: ${(sessions[0] as { entry: { title: string } }).entry.title}`);
    const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
    const result = renderItem({ item: { kind: "result", entry: placeEntries()[0]! } });
    expect((result.props.details as ElementLike).props.title).toBe(placeEntries()[0]!.title);
    ((result.props.details as ElementLike).props.onPress as () => void)(); harness.flush();
    expect(screen.dialog()).toBeDefined();
  });

  it("reports native parentage and independent declared associations in desk Details", () => {
    const session = sessionEntry("desk-with-epic", { projectKey: "prj_desk", workspaceId: "wks_desk", projectName: "DESK • ADAM", labels: { epic: "ASMA-99101" } });
    const lines = (mappings: Parameters<typeof sessionDetailLines>[1]) => Object.fromEntries(sessionDetailLines(session, mappings).map(({ label, value }) => [label, value]));
    expect(lines({})).toMatchObject({ "Native desk": "DESK • ADAM", "Native project ID": "prj_desk", "Native workspace ID": "wks_desk", "Workstream association": "Not declared", "Epic association": "ASMA-99101" });
    expect(lines({ "session:desk-with-epic": { workstream: "declared" } })["Workstream association"]).toBe("declared");
    expect(sessionDetailLines({ ...session, labels: { ...session.labels, workstream: "labelled" } }, {}).find((line) => line.label === "Workstream association")!.value).toBe("labelled");
    expect(lines({})).not.toHaveProperty("Workstream");
    expect(lines({})["Association verification"]).toBe("Workstream registry membership and epic/plan links are not checked here.");
    expect(lines({ "session:desk-with-epic": { workstream: "unregistered-association", epic: "ASMA-99999" } })).toMatchObject({
      "Native project ID": "prj_desk", "Native workspace ID": "wks_desk", "Workstream association": "unregistered-association",
      "Epic association": "ASMA-99999", "Association verification": lines({})["Association verification"],
    });
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

  it("keeps exact times and usage details on the same session, with each informed when the list becomes stale", () => {
    const screen = renderDetails(placeEntries());
    screen.open("long");
    expect(screen.times().type).toBe(SessionTimeLines);
    expect(screen.times().props).toMatchObject({ entry: { id: "long" }, freshness: { live: true, readAt: 0 } });
    expect(screen.usage().props).toMatchObject({ entry: { id: "long" }, stale: false });
    directory.state = { ...(directory.state as object), live: false, staleReason: "Live updates stopped.", lastUpdatedAt: 5 };
    harness.flush();
    expect(screen.times().props).toMatchObject({ entry: { id: "long" }, freshness: { live: false, readAt: 5 } });
    expect(screen.usage().props).toMatchObject({ entry: { id: "long" }, stale: true });
    directory.state = { ...(directory.state as object), status: "stale", live: true };
    harness.flush();
    expect(screen.times().props.freshness).toEqual({ live: false, readAt: 5 });
  });

  /** Search results for a few sessions, with the Updated time three hours back on a live list. */
  function renderResults(compact: boolean, query = "found") {
    vi.setSystemTime(Date.parse("2026-10-18T13:00:00.000Z"));
    const openAgent = vi.fn();
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("found", { title: "SWE • found", updatedAt: "2026-10-18T10:00:00.000Z" }),
      sessionEntry("found-too", { title: "QA • found too", status: "running", updatedAt: "2026-10-18T12:59:30.000Z" }),
    ] };
    patchSessionView("host-a", { query, grouping: "hierarchy", status: "all" });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: compact ? "ios" : "web" },
      navigation: { openAgent, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as unknown as PluginScreenProps));
    const result = (id: string) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike & { readonly type: (props: unknown) => ElementLike })({ item: listRows().find((row) => row.kind === "result" && row.entry.id === id) });
    return { openAgent, result, drawn: (id: string) => { const element = result(id); return element.type(element.props); } };
  }

  it.each([[true, "3h ago"], [false, "Updated 3 hours ago"]] as const)("gives each result its Updated time and its details beside the row, and keeps open and Show in tree (compact %s)", (compact, shown) => {
    const screen = renderResults(compact);
    expect(listRows().map((row) => row.kind)).toEqual(["result", "result"]);
    const drawn = screen.drawn("found");
    expect(textOf(findByTestID(drawn, "sessions-row-age"))).toBe(shown);
    expect(textOf(findByTestID(screen.drawn("found-too"), "sessions-row-age"))).toBe(compact ? "just now" : "Updated just now");
    const row = findByTestID(drawn, "sessions-row-found");
    expect(String(row.props.accessibilityLabel).endsWith(". Updated 3 hours ago")).toBe(true);
    // The details button sits between the row and Show in tree, and opens the details only.
    const parts = drawn.props.children as ElementLike[];
    const details = parts[1]!;
    expect((details as ElementLike & { type: unknown }).type).toBe(SessionDetailsButton);
    expect(details.props).toMatchObject({ title: "SWE • found", expanded: false });
    expect(String(parts[2]!.props.accessibilityLabel)).toBe("Show SWE • found in the tree");
    (details.props.onPress as () => void)(); harness.flush();
    const dialog = walk(harness.current()).find((element) => element.props.testID === "sessions-details")!;
    expect(dialog.props.label).toBe("Details: SWE • found");
    expect(screen.openAgent).not.toHaveBeenCalled();
    expect((screen.result("found").props.details as ElementLike).props.expanded).toBe(true);
    (row.props.onPress as () => void)(); harness.flush();
    expect(screen.openAgent).toHaveBeenCalledTimes(1);
    expect(screen.openAgent.mock.calls[0]![0]).toMatchObject({ agentId: "found" });
    // Show in tree leaves the results, and the details opened there close.
    (screen.result("found").props.onShowInTree as (entry: unknown) => void)(screen.result("found").props.entry); harness.flush();
    expect(walk(harness.current()).find((element) => element.props.testID === "sessions-details")).toBeUndefined();
  });

  it("gives rows the Updated time itself, not an age, while the list is not live", () => {
    const screen = renderResults(true);
    expect(screen.result("found").props.live).toBe(true);
    directory.state = { ...(directory.state as object), live: false, staleReason: "Live updates stopped." };
    harness.flush();
    expect(screen.result("found").props.live).toBe(false);
    const time = new Intl.DateTimeFormat([], { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).format(Date.parse("2026-10-18T10:00:00.000Z"));
    expect(textOf(findByTestID(screen.drawn("found"), "sessions-row-age"))).toBe(time);
    expect(String(findByTestID(screen.drawn("found"), "sessions-row-found").props.accessibilityLabel).endsWith(`. Updated today at ${time}`)).toBe(true);
    // A tree row is told the same, and its details button sits beside it.
    harness.unmount();
    harness.reset();
    renderResults(true, "");
    directory.state = { ...(directory.state as object), live: false, staleReason: "Live updates stopped." };
    harness.flush();
    expandAll();
    const session = listRows().find((row) => row.kind === "session" && row.entry.id === "found")!;
    const drawnRow = (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: session });
    expect((drawnRow.props.children as ElementLike).props.live).toBe(false);
    expect((drawnRow.props.details as ElementLike).props.title).toBe("SWE • found");
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
      expect(region().props).toMatchObject({ tabIndex: 0, role: "region", accessibilityLabel: "Times, role, placement, model and token report" });
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
        openMobileFilters();
        (walk(harness.current()).find((element) => element.props.testID === "sessions-grouping")!.props.onPress as () => void)(); harness.flush();
        (walk(harness.current()).find((element) => element.type === MenuItem && element.props.label === label)!.props.onPress as () => void)(); harness.flush();
        closeMobileFilters();
      };
      choose("Desks & workstreams");
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
    setStateFilter("idle");
    const [result] = listRows() as [Extract<ListRow, { kind: "result" }>];
    expect(result.entry.id).toBe("n");
    ((findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: result }).props.onShowInTree as (entry: unknown) => void)(result.entry);
    harness.flush();
    expect([groupRows().map((row) => `${row.group.label}:${row.expanded}`), sessionRowIds()]).toEqual([["QA:true"], ["n", "e", "r1", "r2", "r3"]]);
    (walk(findByTestID(harness.current(), "sessions-peek")).find((element) => String(element.props.label).startsWith("Back to "))!.props.onPress as () => void)();
    harness.flush();
    expect(sessionRowIds()).toEqual(["n"]);
    setStateFilter("all");
    expect(groupRows().map((row) => `${row.group.label}:${row.expanded}`)).toEqual(["QA:false"]);
    expect(save.mock.calls.filter(([patch]) => "expandedGroups" in patch)).toEqual([]);
  });

  it("returns to the tree position it left when the search is cleared", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("busy", { status: "running" }), sessionEntry("quiet")] };
    patchSessionView("host-a", { query: "" });
    const { scrollToOffset } = renderScreen(undefined, false);
    scroll(480);
    const search = (query: string) => { (sessionSearch().props.onChangeText as (query: string) => void)(query); harness.flush(); };
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

describe("SessionsScreen mobile filter page", () => {
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
  it("keeps a compact title and context together, with its Updated time, wrapped report metadata and state badge", () => {
    directory.state = { ...(directory.state as object), entries: [{ ...sessionEntry("0123456789abcdef", {
      title: "A deliberately long session title that would otherwise wrap onto a second line on a phone",
      status: "running", pendingPermissionCount: 1, archivedAt: "2026-10-18T11:00:00.000Z", workspaceName: "RSW • review",
    }), provider: "codex", model: "gpt-6.1-sol", telemetry: { lastUsage: { inputTokens: 100, outputTokens: 20 } } }] };
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
    // The Updated value stays untruncated; model/report metadata can wrap beneath the title.
    const age = findByTestID(leaf, "sessions-row-age");
    expect(lines.filter((line) => !walk(title).includes(line) || line === title)).toEqual([title, age]);
    const content = ((leaf.props.children as ElementLike).props.children as ElementLike[])[0]!;
    expect(walk(content)).toContain(age);
    expect(walk(content).some((element) => element.type === SessionUsage && (element.props.entry as { id: string }).id === "0123456789abcdef")).toBe(true);
    expect(leaf.props.accessibilityLabel).toContain("Reported input tokens: 100");
    expect(leaf.props.accessibilityLabel).toContain("Reported output tokens: 20");
    expect(age.props.style).toMatchObject({ flexShrink: 0 });
    expect(walk(leaf).some((element) => (element as { type?: unknown }).type === StateBadge && element.props.state === "needs_input")).toBe(true);
    // ST-02: the native ID is in the accessible label, never in the visible row.
    expect(textOf(leaf)).not.toContain("01234567");
    expect(leaf.props.accessibilityLabel).toContain("Native ID 0123456789abcdef");
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
  it("chooses Workspace scope and Group by from compact selects that keep every choice, mark the current one and return focus", () => {
    const save = vi.fn(async () => true);
    const organization = {
      state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true },
      save,
    } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    directory.state = { ...(directory.state as object), entries: [sessionEntry("desk", { labels: { desk: "Ops" } }), sessionEntry("epic-work")] };
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true });
    renderScreen(organization, true);
    openMobileFilters();
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
    expect(items().map((item) => [item.props.label, item.props.selected])).toEqual([["Desks & workstreams", true], ["Desks & workspaces", false], ["Role", false], ["Flat", false]]);
    expect(choices("grouping")).toEqual(["Desks & workstreams", "Desks & workspaces", "Role", "Flat"]);
    choose("Role");
    expect([select("sessions-grouping").value, menus(), groupingFocus.mock.calls.length]).toEqual(["Role", [], 1]);
    expect(save).toHaveBeenCalledWith({ grouping: "role" });

    // Menus keep no Tab, so the other select is reachable with one open; it swaps its menu in without a close, as a new
    // opening against its own trigger, and back again.
    const opening = (id: string) => AnchoredMenu(findByTestID(harness.current(), `sessions-${id}-menu`).props as Parameters<typeof AnchoredMenu>[0]);
    (select("sessions-grouping").onPress as () => void)(); harness.flush();
    const grouping = opening("grouping");
    (select("sessions-scope").onPress as () => void)(); harness.flush();
    expect([menus(), opening("scope").props.anchor]).toEqual([["sessions-scope-menu"], select("sessions-scope").anchor]);
    expect(opening("scope").key).not.toBe(grouping.key);
    (select("sessions-grouping").onPress as () => void)(); harness.flush();
    expect([menus(), opening("grouping").key]).toEqual([["sessions-grouping-menu"], grouping.key]);
    (findByTestID(harness.current(), "sessions-grouping-menu").props.onClose as () => void)(); harness.flush();

    // A menu left open closes with the options panel and does not come back when it reopens.
    (select("sessions-scope").onPress as () => void)(); harness.flush();
    closeMobileFilters();
    expect(menus()).toEqual([]);
    openMobileFilters();
    expect([select("sessions-scope").expanded, menus()]).toEqual([false, []]);
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

  it("keeps the quick toolbar and one full-page Filters route beside results, ignoring the old inline-open preference", () => {
    patchSessionView("host-a", { toolbarOpen: true, advancedOpen: true, status: "idle", grouping: "role" });
    renderCompact();
    expect(testIDs(harness.current())).toContain("sessions-list");
    expect(testIDs(harness.current())).not.toContain("sessions-full-toolbar");
    expect(testIDs(harness.current())).not.toContain("sessions-options");
    const triggers = walk(harness.current()).filter((node) => node.type === MobileQuickToolbar);
    expect(triggers).toHaveLength(1);
    expect(triggers[0]!.props.activeCount).toBe(1);
    expect(triggers[0]!.props.summary).toContain("Idle");
  });

  it.each([true, false])("explains incomplete workstream availability with zero, one and unregistered associations (compact=%s)", (compact) => {
    patchSessionView("host-a", { advancedOpen: true });
    directory.state = { ...(directory.state as object), entries: [] };
    renderScreen(undefined, compact);
    if (compact) openMobileFilters();
    expect(textOf(findByTestID(harness.current(), "sessions-workstream-availability"))).toContain("this list may be incomplete");
    expect(textOf(harness.current())).toContain("No workstream associations available in this context.");
    const choices = () => walk(harness.current()).filter((node) => node.props.checkbox === true);
    expect(choices()).toHaveLength(0);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {})] };
    harness.flush();
    expect(choices().map((node) => node.props.label)).toEqual(["Other work"]);
    expect(textOf(harness.current())).not.toContain("No workstream associations available");
    (choices()[0]!.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").streams).toEqual([]);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {}), sessionEntry("unknown", { labels: { workstream: "unregistered-key" } })] };
    harness.flush();
    expect(choices().map((node) => node.props.label).sort()).toEqual(["Other work", "unregistered-key"]);
    expect(textOf(findByTestID(harness.current(), "sessions-workstream-availability"))).toContain("Names may be unregistered");
    expect(readSessionView("host-a").streams).toEqual([]);
  });

  it("applies all seven quick actions to actual rows and updates counts with search, without a settings write", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("run", { title: "mobile run", status: "running" }),
      sessionEntry("idle", { title: "mobile idle" }),
      sessionEntry("error", { title: "mobile error", status: "error", requiresAttention: true, attentionReason: "error" }),
      sessionEntry("input", { title: "mobile input", needsInput: true, requiresAttention: true, attentionReason: "permission" }),
      sessionEntry("notice", { title: "mobile notice", requiresAttention: true, attentionReason: "finished" }),
      sessionEntry("other", { title: "unrelated" }),
    ] };
    const save = vi.fn(async () => true);
    const organization = { state: { status: "ready", values: { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["idle"] }, revision: "r", saving: false, saveError: null, save: async () => true, reset: async () => true, reload: async () => {} }, save } satisfies NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    patchSessionView("host-a", { query: "mobile", grouping: "flat" });
    renderScreen(organization, true);
    const toolbar = () => walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!;
    const expected = { all: ["error", "idle", "input", "notice", "run"], running: ["run"], idle: ["idle", "input", "notice"], error: ["error"], needs_input: ["input"], attention: ["error", "input", "notice"], favorites: ["idle"] };
    for (const [status, ids] of Object.entries(expected)) {
      (toolbar().props.onStatus as (status: string) => void)(status); harness.flush();
      expect(sessionRowIds().sort()).toEqual(ids);
      expect((toolbar().props.counts as Record<string, number>)[status]).toBe(ids.length);
      expect(readSessionView("host-a").query).toBe("mobile");
    }
    (toolbar().props.onChooser as () => void)(); harness.flush();
    expect(readSessionView("host-a").status).toBe("favorites");
    const chooser = walk(harness.current()).find((node) => node.type === MobileQuickChooser)!;
    (chooser.props.onClose as () => void)(); harness.flush();
    (toolbar().props.onQuery as (query: string) => void)("run"); harness.flush();
    expect(sessionRowIds()).toEqual([]);
    expect((toolbar().props.counts as Record<string, number>).all).toBe(1);
    (toolbar().props.onStatus as (status: string) => void)("all"); harness.flush();
    expect(sessionRowIds()).toEqual(["run"]);
    (toolbar().props.onQuery as (query: string) => void)(""); harness.flush();
    expect(sessionRowIds()).toHaveLength(6);
    expect(save).not.toHaveBeenCalled();
  });

  it("applies filters immediately and Back keeps search, grouping, task/scope choices and results", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("idle", { title: "SWE • mobile", status: "idle" }), sessionEntry("running", { title: "QA • mobile", status: "running" })] };
    patchSessionView("host-a", { query: "mobile", grouping: "role" });
    renderCompact();
    openMobileFilters();
    expect(testIDs(harness.current())).not.toContain("sessions-list");
    expect(testIDs(harness.current())).toContain("sessions-options");
    expect(textOf(harness.current())).toContain("Changes apply immediately. Back keeps your choices.");
    (findByTestID(harness.current(), "sessions-filter-running").props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ query: "mobile", status: "running", grouping: "role" });
    closeMobileFilters();
    expect(sessionRowIds()).toEqual(["running"]);
    openMobileFilters();
    expect(findByTestID(harness.current(), "sessions-filter-running").props.active).toBe(true);
    (walk(harness.current()).find((node) => node.type === ClearFiltersButton)!.props.onPress as () => void)(); harness.flush();
    closeMobileFilters();
    expect((walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.counts as Record<string, number>).all).toBe(2);
    expect(groupRows().map((row) => row.group.label).sort()).toEqual(["QA", "SWE"]);
    expect(readSessionView("host-a")).toMatchObject({ query: "", status: "all", grouping: "role", shortcut: null, includeArchived: false });
  });

  it.each(["quick", "full-page", "desktop", "collapsed", "empty"] as const)("clears compound filters from %s within the explicit project/epic and preserves preferences", (entry) => {
    const scope = "host-a:project-a:ASMA-1";
    const onClearScope = vi.fn(), save = vi.fn(async () => true), refresh = vi.fn(), archive = vi.fn();
    const values = { ...DEFAULT_ORGANIZATION_SETTINGS, grouping: "flat" as const, favorites: ["match"], expandedGroups: { keep: true } };
    const organization = { state: { status: "ready" as const, values, revision: "r", saving: false, saveError: null, save: async () => true, reset: async () => true, reload: async () => {} }, save };
    const supplied = { state: { ...emptyDirectoryState(), status: "ready" as const, entries: [
      sessionEntry("match", { projectKey: "project-a", labels: { epic: "ASMA-1" } }),
      sessionEntry("second", { projectKey: "project-a", labels: { epic: "ASMA-1" }, status: "running" }),
      sessionEntry("outside-project", { projectKey: "project-b", labels: { epic: "ASMA-1" } }),
      sessionEntry("outside-epic", { projectKey: "project-a", labels: { epic: "ASMA-2" } }),
    ], pages: 1, lastUpdatedAt: 0, inventoryComplete: true }, refresh };
    patchSessionView(scope, { query: "no match", status: "favorites", task: { jira: "ASMA-9", kontorId: null, label: "ASMA-9" },
      shortcut: "Adam", streams: [], includeArchived: true, grouping: "flat", arranging: true, advancedOpen: true, toolbarOpen: false, scrollOffset: entry === "collapsed" ? 300 : 0, treeOffset: 70 });
    const compact = entry === "quick" || entry === "full-page";
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: "web" }, theme: { colors } as PluginTheme, params: {},
      directory: supplied, organization, projectId: "project-a", epicKey: "ASMA-1", scopeLabel: "ASMA-1", onClearScope, onIncludeArchived: archive, viewScope: scope }));
    expect(sessionRowIds()).toEqual([]);
    let clear: () => void;
    if (entry === "quick") clear = walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.onClear as () => void;
    else if (entry === "collapsed") clear = walk(harness.current()).find((node) => node.type === ActiveFilterSummary)!.props.onClear as () => void;
    else if (entry === "empty") clear = walk(findByTestID(harness.current(), "sessions-list").props.ListEmptyComponent).find((node) => node.props.accessibilityLabel === "Clear filters")!.props.onPress as () => void;
    else {
      if (entry === "full-page") openMobileFilters();
      clear = walk(harness.current()).find((node) => node.type === ClearFiltersButton)!.props.onPress as () => void;
    }
    clear(); harness.flush();
    if (entry === "full-page") {
      expect(walk(harness.current()).some((node) => node.type === MobileFiltersPage)).toBe(true);
      expect(walk(harness.current()).some((node) => node.type === MobileQuickToolbar)).toBe(false);
      expect(testIDs(harness.current()).filter((id) => id === "sessions-search")).toHaveLength(1);
      closeMobileFilters();
    }
    expect(readSessionView(scope)).toMatchObject({ query: "", status: "all", task: null, shortcut: null, streams: null, includeArchived: false,
      grouping: "flat", advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 70, peek: null });
    expect(sessionRowIds().sort()).toEqual(["match", "second"]);
    const cleared = readSessionView(scope);
    clear(); harness.flush();
    expect(readSessionView(scope)).toEqual(cleared);
    expect(values.favorites).toEqual(["match"]);
    expect(values.expandedGroups).toEqual({ keep: true });
    expect(save).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled(); expect(onClearScope).not.toHaveBeenCalled();
    expect(archive).toHaveBeenLastCalledWith(false);
    expect(directory.inputs.at(-1)?.enabled).toBe(false);
  });

  it("returns keyboard focus to Filters and restores an unchanged list offset", () => {
    patchSessionView("host-a", { grouping: "flat", scrollOffset: 370 });
    renderCompact(); vi.runAllTimers();
    const ref = walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.filtersTriggerRef as RefLike;
    const calls: string[] = [];
    const focus = vi.fn(() => calls.push("focus")); ref.current = { focus };
    const dismiss = vi.fn(() => calls.push("dismiss"));
    const helpRef = walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.helpDismissRef as { current: (() => void) | null };
    helpRef.current = dismiss;
    openMobileFilters(); closeMobileFilters();
    const scrollToOffset = vi.fn();
    (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { scrollToOffset };
    vi.runAllTimers();
    expect(focus).toHaveBeenCalledOnce();
    expect(calls).toEqual(["focus", "dismiss"]);
    expect(scrollToOffset).toHaveBeenCalledWith({ offset: 370, animated: false });
  });

  it("retains choices across a resize and keeps filter pages isolated by context", () => {
    let compact = true;
    let scope = "project:a";
    patchSessionView(scope, { scrollOffset: 370 });
    const render = () => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: "web" }, theme: { colors } as PluginTheme, params: {}, viewScope: scope });
    harness.render(render); openMobileFilters();
    setStateFilter("idle");
    compact = false; harness.flush();
    expect(testIDs(harness.current())).not.toContain("sessions-mobile-filters");
    expect(fullToolbarShown()).toBe(true);
    expect(readSessionView(scope).status).toBe("idle");
    compact = true; harness.flush(); openMobileFilters();
    scope = "project:b"; harness.flush();
    expect(walk(harness.current()).some((node) => node.type === MobileFiltersPage)).toBe(false);
    expect(readSessionView(scope).status).toBe("all");
    expect(readSessionView("project:a").status).toBe("idle");
  });
});

function openMobileFilters() {
  const trigger = walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!;
  (trigger.props.onFilters as () => void)(); harness.flush();
}
function closeMobileFilters() {
  const page = walk(harness.current()).find((node) => node.type === MobileFiltersPage)!;
  (page.props.onBack as () => void)(); harness.flush();
}


describe("plan-derived visible workstream choices on the current mobile/lifecycle screen", () => {
  beforeEach(() => { vi.useFakeTimers(); clearSessionViews(); harness.reset(); });
  afterEach(() => { harness.unmount(); vi.useRealTimers(); });
  it.each([true, false])("renders graphless and registry-only choices, filters rows, and clears streams (compact=%s)", async (compact) => {
    const { source, read, registry } = await import("./plan-workstream-fixtures");
    const entries = [sessionEntry("linked", { labels: { epic: "ASMA-7575" } })];
    directory.state = { ...emptyDirectoryState(), entries, status: "ready", live: true, inventoryComplete: true };
    patchSessionView("host-a", { query: "", advancedOpen: true, includeArchived: false, streams: [], ...toolbarOpenPatch(true) });
    const { planWorkstreamMappings } = await import("../client/plan-workstreams");
    const mappings = planWorkstreamMappings([source], { plan: read }, entries, [], {}, {});
    renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [source], workstreamReads: { plan: read }, workstreamRegistry: registry, planMappings: mappings });
    if (compact) openMobileFilters();
    expect(findByTestID(harness.current(), "workstream-option-adopus-graphql").props.label).toContain("1 sessions");
    expect(findByTestID(harness.current(), "workstream-option-access").props.label).toContain("0 sessions");
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toEqual([]);
    if (compact) openMobileFilters();
    (findByTestID(harness.current(), "workstream-option-adopus-graphql").props.onPress as () => void)(); harness.flush();
    if (compact) closeMobileFilters();
    expandAll(); expect(sessionRowIds()).toEqual(["linked"]);
    if (compact) openMobileFilters();
    const clear = walk(harness.current()).find((node) => node.type === ClearFiltersButton)!;
    (clear.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").streams).toBeNull();
  });
  it.each([true, false])("counts only native workspaces in the current project and epic (compact=%s)", async (compact) => {
    const { source, read, registry } = await import("./plan-workstream-fixtures");
    directory.state = { ...emptyDirectoryState(), entries: [], status: "ready", live: true, inventoryComplete: true };
    patchSessionView("host-a", { query: "", advancedOpen: true, ...toolbarOpenPatch(true) });
    renderScreen(undefined, compact, compact ? "ios" : "web", {
      projectId: "included-project", epicKey: "ASMA-7575",
      projects: [{ id: "included-project", name: "Existing project" }, { id: "outside-project", name: "Other project" }],
      workspaces: [{ id: "empty", name: "Existing empty workspace", projectKey: "included-project" }, { id: "outside", name: "Outside workspace", projectKey: "outside-project" }],
      planMappings: { "workspace:empty": { epic: "ASMA-7575", workstream: "adopus-graphql" }, "workspace:outside": { epic: "ASMA-7575", workstream: "adopus-graphql" } },
      workstreamSources: [source], workstreamReads: { plan: read }, workstreamRegistry: registry,
    });
    if (compact) openMobileFilters();
    expect(findByTestID(harness.current(), "workstream-option-adopus-graphql").props.label).toContain("0 sessions · 1 workspaces · 1 plans");
  });
});
