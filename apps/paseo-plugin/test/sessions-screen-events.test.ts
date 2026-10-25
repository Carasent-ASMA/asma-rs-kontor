import { SessionScopeSelector } from "../client/session-scope-selector";
import { SessionScopeChip } from "../client/session-scope-chip";
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
const controlHelp = vi.hoisted(() => ({ current: null as null | { dismiss(): void } }));
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
    ...actual, useContext: () => controlHelp.current,
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
  FilterChip,
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
import { SessionsLegend } from "../client/sessions-legend";
import { MobileFiltersPage } from "../client/mobile-filters";
import { SessionLayoutControl } from "../client/session-layout-control";
import { MobileQuickToolbar } from "../client/mobile-quick-toolbar";
import { MobileQuickChooser } from "../client/mobile-quick-chooser";
import { indentOf, StateBadge } from "../client/organization-row";
import { SessionStateMarker, SessionStatusIcon } from "../client/session-state-marker";
import { QUICK_FILTER_ICONS } from "../client/mobile-quick-filter-options";
import type { StatusFilter } from "../client/session-view";
import { SessionDetailSection } from "../client/session-detail-section";
import { SessionUsage, SessionUsageLines } from "../client/session-usage-view";
import { AnchoredMenu, MenuItem } from "../client/anchored-menu";
import { nativeFreshnessLabel } from "../client/native-freshness";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
  workScopeOf,
  writeWorkingAs,
} from "../client/session-view-store";
import { responsibilitySchema, type ResponsibilityRead } from "../shared/responsibility";
import { TOOLBAR_LAYOUT_GAP } from "../client/toolbar-visibility";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
import type { SessionStateCount } from "../client/session-state-counts";
import { toggleSelection, WorkstreamSelector } from "../client/workstream-selector";
import { workstreamOptionLabel } from "../client/plan-workstreams";

/** The association selector as the screen configures it; its own rendering is covered in workstream-selector.test.ts. */
function selector(): Parameters<typeof WorkstreamSelector>[0] {
  return walk(harness.current()).find((node) => (node as { type?: unknown }).type === WorkstreamSelector)!.props as unknown as Parameters<typeof WorkstreamSelector>[0];
}
function toggleChoice(key: string) {
  const props = selector();
  props.onChange(toggleSelection(props.selected, props.options.map((option) => option.key), key)); harness.flush();
}
/** The facts the selector's Details for a choice show. */
function choiceFacts(key: string): string {
  const props = selector();
  return workstreamOptionLabel(props.options.find((option) => option.key === key)!, props.registryLoaded);
}

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

function chooseLayout(value: "grouped" | "flat") {
  const control = walk(harness.current()).find((node) => node.type === SessionLayoutControl)!;
  (control.props.onChange as (value: "grouped" | "flat") => void)(value); harness.flush();
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
    const recovery = tree.filter((element) => element.props.accessibilityLabel === "Resume sessions");
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
    const recovery = walk(harness.current()).find((element) => element.props.accessibilityLabel === "Resume sessions")!;
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
    clearSessionViews({ work: "all" });
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
    expect(findByTestID(harness.current(), "sessions-filter-error").props.count).toMatchObject({ text: "1" });
    expect(findByTestID(harness.current(), "sessions-filter-attention").props.count).toMatchObject({ text: "2" });
    expect(findByTestID(harness.current(), "sessions-filter-closed").props.count).toMatchObject({ text: "13" });
    const earlier = findByTestID(harness.current(), "sessions-filter-earlier_error");
    expect(earlier.props.label).toBe("Earlier errors");
    expect(earlier.props.count).toMatchObject({ text: "13" });
    expect(earlier.props.status).toBe("earlier_error");
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
    expect(walk(row).find((element) => element.type === SessionStateMarker)!.props.state).toBe("closed");
    // Archived history is disjoint from closed-but-unarchived records, without changing the state choice.
    if (compact) openMobileFilters();
    (findByTestID(harness.current(), "sessions-advanced-archived").props.onPress as () => void)(); harness.flush();
    expect(directory.inputs.at(-1)?.includeArchived).toBe(true);
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toHaveLength(0);
    if (compact) openMobileFilters();
    (findByTestID(harness.current(), "sessions-advanced-archived").props.onPress as () => void)(); harness.flush();
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toHaveLength(13);
    setStateFilter("error");
    expect(sessionRowIds()).toEqual(["current"]);
  });

  it.each([true, false])("keeps exact status icons, counts, selection and activation in quick/expanded filter choices, compact=%s", compact => {
    for (const status of Object.keys(QUICK_FILTER_ICONS) as StatusFilter[]) {
      const onPress = vi.fn(), onFocus = vi.fn(), onBlur = vi.fn();
      for (const active of [false, true]) {
        const chip = FilterChip({ status, label: status, count: 3, colors, compact, active, onPress, onFocus, onBlur });
        const marker = walk(chip).find(element => element.type === SessionStatusIcon)!;
        const icon = walk(SessionStatusIcon(marker.props as Parameters<typeof SessionStatusIcon>[0])).find(element => element.type === "Icon")!;
        expect(icon.props.name).toBe(QUICK_FILTER_ICONS[status]);
        expect(chip.props.accessibilityLabel).toBe(`${status}, 3`);
        expect(chip.props.accessibilityState).toEqual({ selected: active, disabled: false });
        expect(chip.props.style).toMatchObject({ borderColor: active ? colors.accent : colors.border, backgroundColor: active ? colors.surface2 : colors.surface1 });
        chip.props.onFocus?.(); chip.props.onBlur?.(); chip.props.onPress();
      }
      expect(onPress).toHaveBeenCalledTimes(2);
      expect(onFocus).toHaveBeenCalledTimes(2);
      expect(onBlur).toHaveBeenCalledTimes(2);
    }
  });

  it.each([true, false])("updates the same row marker through native transitions without changing its accessible name or adding controls, compact=%s", compact => {
    patchSessionView("host-a", { layout: "flat" });
    patchSessionView("host-a", { query: "", grouping: "flat", status: "all", includeArchived: false });
    renderScreen(undefined, compact);
    const transitions = [
      [sessionEntry("live", { status: "initializing" }), "initializing"],
      [sessionEntry("live"), "idle"],
      [sessionEntry("live", { status: "running" }), "running"],
      [sessionEntry("live", { status: "running", pendingPermissionCount: 1 }), "needs_input"],
      [sessionEntry("live", { status: "error", requiresAttention: true, attentionReason: "error" }), "failed"],
      [sessionEntry("live", { status: "closed", requiresAttention: true, attentionReason: "error" }), "closed"],
      [sessionEntry("live", { requiresAttention: true, attentionReason: "finished" }), "attention"],
      [sessionEntry("live"), "idle"],
    ] as const;
    for (const [entry, state] of transitions) {
      directory.state = { ...(directory.state as object), entries: [entry] };
      harness.flush();
      const row = renderSessionLeaf();
      expect(walk(row).filter(element => element.type === SessionStateMarker).map(element => element.props.state)).toEqual([state]);
      expect(walk(row).filter(element => element.type === "Pressable")).toHaveLength(1);
      expect(row.props.accessibilityLabel).toContain(`Native status ${entry.status}`);
      expect(row.props.accessibilityLabel).toContain("Session live");
      expect(row.props["aria-describedby"]).toBeUndefined();
      expect(walk(row).some(element => element.type === StateBadge)).toBe(false);
    }
    // Labeled filters teach the same shapes, keeping their labels and exact counts.
    if (compact) openMobileFilters();
    expect(findByTestID(harness.current(), "sessions-filter-idle").props).toMatchObject({ status: "idle", label: "Idle", count: { text: "1" } });
    expect(findByTestID(harness.current(), "sessions-filter-error").props).toMatchObject({ status: "error", label: "Error", count: { text: "0" } });
    if (compact) closeMobileFilters();
  });

  it("shows applied filters with the matching count accessible and inventory units/read status subordinate in Filters", () => {
    const header = () => findByTestID(harness.current(), "sessions-layout-bar");
    directory.state = { ...directory.state as object, includeArchived: false };
    patchSessionView("host-a", { includeArchived: false });
    renderScreen();
    const search = (query: string) => { (sessionSearch().props.onChangeText as (query: string) => void)(query); harness.flush(); };
    search("Session");
    expect(String(findByTestID(header(), "sessions-applied-filters").props.accessibilityLabel)).toContain("1 matching session");
    expect(textOf(findByTestID(harness.current(), "sessions-options-count"))).toMatch(/^Inventory: 1 session in this host context, excluding archived\. Results span 1 workspace \/ folder group/);
    search("no such session");
    expect(String(findByTestID(header(), "sessions-applied-filters").props.accessibilityLabel)).toContain("0 matching sessions");
    search("session-a");
    expect(String(findByTestID(header(), "sessions-applied-filters").props.accessibilityLabel)).toContain("1 matching session");
    expect(findByTestID(header(), "sessions-applied-filters").props.accessibilityLiveRegion).toBe("polite");
    directory.state = { ...directory.state as object, inventoryComplete: false };
    renderScreen();
    expect(String(findByTestID(header(), "sessions-applied-filters").props.accessibilityLabel)).toContain("partial read");
    directory.state = { ...directory.state as object, inventoryComplete: true, lastUpdatedAt: null };
    renderScreen();
    expect(String(findByTestID(header(), "sessions-applied-filters").props.accessibilityLabel)).toContain("Reading matching sessions");
    expect(textOf(findByTestID(harness.current(), "sessions-options-count"))).toBe("Inventory for this membership has not been read yet.");
  });

  it("counts ordinary sessions from an archive-inclusive read without claiming including archived", () => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("ordinary", { status: "idle" }),
      sessionEntry("retired", { status: "idle", archivedAt: "2026-10-18T11:00:00Z" }),
    ], includeArchived: true };
    patchSessionView("host-a", { query: "ordinary", status: "all", includeArchived: false, membership: "sessions" });
    renderScreen();
    const header = findByTestID(harness.current(), "sessions-applied-filters");
    expect(String(header.props.accessibilityLabel)).toContain("1 matching session");
    expect(textOf(findByTestID(harness.current(), "sessions-options-count"))).toMatch(/^Inventory: 1 session in this host context, excluding archived/);
    expect(directory.inputs.at(-1)?.includeArchived).toBe(true);
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
    expect(counts(rendered(full))).toEqual([]);
    expect(walk(rendered(full)).find((element) => element.type === StateBadge)!.props.aggregate).toMatchObject({ total: full.group.visibleCount });
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
      includeArchived: false,
    };
    patchSessionView("host-a", { includeArchived: false });
    renderScreen();
    const list = findByTestID(harness.current(), "sessions-list");
    const count = JSON.stringify(findByTestID(harness.current(), "sessions-options-count").props.children);

    expect(count).toContain("partial inventory read");
    expect(count).not.toContain("complete inventory read");
  });

  it("restores context-scoped filters and offsets without mixing them with host defaults", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("session-a", { archivedAt: "2026-10-18T11:00:00Z" }), sessionEntry("other")] };
    patchSessionView("host-a", { query: "host default", scrollOffset: 0 });
    patchSessionView("host-a:epic-a", { query: "session-a", membership: "archived", scrollOffset: 420, ...toolbarOpenPatch(true) });
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
    expect(directory.inputs.at(-1)?.includeArchived).toBe(true);
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
    // Compact says the Filters matching count: one session, qualified because the read is not live and complete.
    expect(walk(harness.current()).some((node) => node.props.testID === "sessions-mobile-count")).toBe(false);
    expect((walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.counts as Record<string, SessionStateCount>).all!.label).toContain("partial read");
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
    patchSessionView("host-a", { query: "", layout: "grouped", advancedOpen: true, shortcut: null, toolbarOpen: true });
    const native: Pick<Parameters<typeof SessionsScreen>[0], "projects" | "workspaces"> = { projects: [{ id: "prj_one", name: "DESK • RESEARCH" }, { id: "prj_two", name: "DESK • RESEARCH" }], workspaces: [] };
    renderScreen(undefined, false, "web", native);
    const selector = () => walk(harness.current()).find((node) => node.type === SessionScopeSelector && node.props.id === "scope")!.props as Parameters<typeof SessionScopeSelector>[0];
    expect(selector().options.filter((choice) => choice.label === "DESK • RESEARCH").map((choice) => choice.detail)).toEqual(["prj_one", "prj_two"]);
    selector().onChange("desk-project:prj_one"); harness.flush();
    expect(readSessionView("host-a").shortcut).toBe("desk-project:prj_one");
    expect(sessionRowIds()).toEqual([]);
    native.workspaces = [{ id: "wks_empty_desk", name: "CSW • empty native desk", projectKey: "prj_one", projectName: "DESK • RESEARCH" }];
    harness.flush();
    expect(groupRows().filter((row) => row.group.kind === "workspace").map((row) => row.group.label)).toEqual(["CSW • empty native desk"]);
    native.projects = [{ id: "prj_one", name: "DESK • DOCS" }, native.projects![1]!]; harness.flush();
    expect(selector().options.find((choice) => choice.key === selector().selected)?.label).toBe("DESK • DOCS");
    expect(readSessionView("host-a").shortcut).toBe("desk-project:prj_one");
  });

  it.each([true, false])("keeps scoped native desk search, Details and Show in tree on one workspace (compact %s)", (compact) => {
    patchSessionView("host-a", { layout: "flat" });
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
    expect(selector().about).toContain("this list may be incomplete");
    const choices = () => selector().options.map((option) => option.label);
    expect(choices()).toHaveLength(0);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {})] };
    harness.flush();
    expect(choices()).toEqual(["Other work"]);
    toggleChoice("other work");
    expect(readSessionView("host-a").streams).toEqual([]);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {}), sessionEntry("unknown", { labels: { workstream: "unregistered-key" } })] };
    harness.flush();
    expect(choices().sort()).toEqual(["Other work", "unregistered-key"]);
    expect(selector().about).toContain("Names may be unregistered");
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
    expect([chip("sessions-filter-favorites").props.label, (chip("sessions-shortcut-2").props.choice as { label: string }).label]).toEqual(["★ Favorites", "Adam desk"]);
    expect(chip("sessions-shortcut-2").type).toBe(SessionScopeChip);
    expect((chip("sessions-filter-favorites").props.count as SessionStateCount).text).toBe("1/1");
    expect((chip("sessions-filter-favorites").props.count as SessionStateCount).label).toContain("1 of 1 sessions after refinements");
    expect(chip("sessions-shortcut-2").props.count).toBe(1);
    (chip("sessions-filter-favorites").props.onPress as () => void)(); harness.flush();
    (chip("sessions-shortcut-2").props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ status: "favorites", shortcut: "Adam" });
    expect(sessionRowIds()).toEqual(["session-a"]);
    harness.reset(); renderScreen(organization);
    expect(chip("sessions-filter-favorites").props.active).toBe(true);
    expect(chip("sessions-shortcut-2").props.active).toBe(true);
    (chip("sessions-shortcut-4").props.onPress as () => void)(); harness.flush();
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

  it("filters Grouped without saving the temporary matching branches", () => {
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
    expect(findByTestID(harness.current(), "sessions-collapse-all").props.disabled).toBe(false);
    setStateFilter("all");
    expect(sessionRowIds()).toEqual([]);
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
  });

  it("shows a desk result in its workspace under the desk, and names a missing epic elsewhere", () => {
    patchSessionView("host-a", { layout: "flat" });
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("desk", { status: "idle", labels: { desk: "Adam" } }),
      sessionEntry("loose", { status: "running", title: "SWE • loose", labels: { workstream: "Kontor" } }),
      sessionEntry("loose-2", { status: "idle", title: "SWE • second", labels: { workstream: "Kontor" } }),
    ] };
    patchSessionView("host-a", { query: "" });
    patchSessionView("host-a", { layout: "grouped" });
    renderScreen();
    expect(groupRows().map((row) => `${row.group.kind}:${row.group.label}`)).toEqual(["workstream:Adam", "workspace:Workspace", "workstream:Kontor", "epic:No epic linked"]);
    chooseLayout("flat");
    setStateFilter("idle");
    const renderItem = (info: { item: unknown }) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)(info);
    const results = listRows() as Extract<ListRow, { kind: "result" }>[];
    expect(results.map((row) => (renderItem({ item: row }).props.provenance as { parts: string[] }).parts)).toEqual([["Adam desk", "Workspace"], ["Kontor", "No epic linked", "Workspace"]]);
    (renderItem({ item: results[0] }).props.onShowInTree as (entry: unknown) => void)(results[0]!.entry); harness.flush();
    expect(groupRows().map((row) => `${row.group.kind}:${row.group.label}:${row.expanded}`).slice(0, 2)).toEqual(["workstream:Adam:true", "workspace:Workspace:true"]);
    expect(sessionRowIds()).toContain("desk");
    // Role grouping names the missing epic in each session's context too.
    harness.reset(); patchSessionView("host-a", { layout: "grouped", grouping: "role", status: "all" }); renderScreen();
    expect(groupRows().map((row) => `${row.group.label}:${row.expanded}`)).toEqual(["SWE:false", "No role:false"]);
    expandAll();
    const roleSessions = listRows().filter((row): row is Extract<ListRow, { kind: "session" }> => row.kind === "session" && row.entry.id.startsWith("loose"));
    expect(roleSessions.map((row) => walk(renderItem({ item: row })).find((element) => "displayContext" in element.props)!.props.displayContext)).toEqual(["Kontor · No epic linked · Workspace", "Kontor · No epic linked · Workspace"]);
  });

  it.each([
    { compact: false, filter: "search" }, { compact: true, filter: "search" },
    { compact: false, filter: "state" }, { compact: true, filter: "state" },
  ])("keeps denominator help truthful during contextual peek, compact=$compact filter=$filter", ({ compact, filter }) => {
    directory.state = { ...(directory.state as object), entries: [
      sessionEntry("quiet", { title: "Unique quiet session", status: "idle", labels: { workstream: "alpha", epic: "ASMA-1" } }),
      sessionEntry("busy", { status: "running", labels: { workstream: "alpha", epic: "ASMA-1" } }),
      sessionEntry("outside", { status: "idle", labels: { workstream: "beta", epic: "ASMA-2" } }),
    ] };
    const query = filter === "search" ? "Unique quiet" : "";
    const status = filter === "state" ? "idle" : "all";
    patchSessionView("host-a", { layout: "grouped", query, status, streams: ["alpha"], legendOpen: true });
    renderScreen(undefined, compact);
    const renderItem = (item: ListRow) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: ListRow }) => ElementLike)({ item });
    const header = (row: GroupRow) => {
      const element = renderItem(row) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
      return walk(element.type(element.props)).find((node) => node.props.accessibilityRole === "button")!.props;
    };
    const legendText = () => {
      if (compact) openMobileFilters();
      const element = walk(harness.current()).find((node) => node.type === SessionsLegend)!;
      const text = textOf(SessionsLegend(element.props as Parameters<typeof SessionsLegend>[0]));
      if (compact) closeMobileFilters();
      return text;
    };
    const filteredKeys = groupRows().map((row) => row.group.key);
    expect(groupRows().every((row) => row.group.aggregate.total === 1)).toBe(true);
    expect(String(header(groupRows()[0]!).accessibilityLabel)).toContain("Idle: 1 of 1 session");
    chooseLayout("flat");
    expect(sessionRowIds()).toEqual(["quiet"]);
    const result = listRows()[0] as Extract<ListRow, { kind: "result" }>;
    (renderItem(result).props.onShowInTree as (entry: typeof result.entry) => void)(result.entry); harness.flush();
    expect(sessionRowIds().sort()).toEqual(["busy", "quiet"]);
    expect(groupRows().every((row) => row.group.aggregate.total === 2)).toBe(true);
    for (const row of groupRows()) {
      expect(String(header(row).accessibilityLabel)).toContain("Running: 1 of 2 sessions. Idle: 1 of 2 sessions");
      expect(String(header(row).accessibilityHint)).toContain("retained surrounding context");
      expect(String(header(row).accessibilityHint)).toContain("outside the results' search, task or explicit state filter");
      expect(String(header(row).accessibilityHint)).toContain("Current still applies when selected");
    }
    expect(legendText()).toContain("retained surrounding context");
    toggle("workspace");
    expect(sessionRowIds()).toEqual([]);
    expect(groupRows().every((row) => row.group.aggregate.total === 2)).toBe(true);
    expect(String(header(groupRows()[0]!).accessibilityHint)).toContain("including collapsed branches");
    const back = walk(findByTestID(harness.current(), "sessions-peek")).find((node) => node.props.label === "Back to 1 result")!;
    (back.props.onPress as () => void)(); harness.flush();
    expect(sessionRowIds()).toEqual(["quiet"]);
    expect(readSessionView("host-a")).toMatchObject({ query, status, streams: ["alpha"], peek: null });
    chooseLayout("grouped");
    expect(groupRows().map((row) => row.group.key)).toEqual(filteredKeys);
    expect(groupRows().every((row) => row.group.aggregate.total === 1)).toBe(true);
    expect(String(header(groupRows()[0]!).accessibilityHint)).toContain("matching the current search and filters");
    expect(legendText()).toContain("matching the current search and filters");
    expect(legendText()).not.toContain("retained surrounding context");
  });

  it("shows a result in the tree without saving the branches it opens, and Back returns to the results", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    chooseLayout("grouped");
    expect(groupRows().map((row) => `${row.group.kind}:${row.expanded}`)).toEqual(["workstream:true", "epic:false"]);
    expect(save.mock.calls).toEqual([[{ layout: "grouped", grouping: "hierarchy" }]]);
  });

  it("on a short compact screen, Show in tree keeps one Back row above the list, and Back brings the toolbar and filters back", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    expect(header()).toEqual(["sessions-layout-bar", "sessions-view-controls"]);
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
    patchSessionView("host-a", { layout: "flat" });
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
    patchSessionView("host-a", { layout: "flat" });
    const { failed, quietRow, indexes } = showQuietInTree();
    expect(quietRow()).toBe(43);
    failed();
    vi.advanceTimersByTime(60);
    expect(indexes()).toEqual([43]);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43]);
  });

  it("drops a scroll retry once Back has swapped the tree for the shorter results list", () => {
    patchSessionView("host-a", { layout: "flat" });
    const { failed, back, indexes } = showQuietInTree();
    failed();
    back();
    expect(listRows()).toHaveLength(1);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("drops a scroll retry whose row a collapse removed, though the list mode is unchanged", () => {
    patchSessionView("host-a", { layout: "flat" });
    const { failed, indexes } = showQuietInTree();
    failed();
    // The workspace closes before the retry: three rows remain, and the list still shows the same session's tree.
    toggle("workspace");
    expect(listRows()).toHaveLength(3);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("moves a scroll retry with its row when a live update shifts the tree", () => {
    patchSessionView("host-a", { layout: "flat" });
    const { failed, quietRow, indexes } = showQuietInTree();
    failed();
    addBusySibling();
    expect(quietRow()).toBe(44);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([44]);
  });

  it("drops a scroll retry when Back and Show in tree place the same session again first", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    patchSessionView("host-a", { layout: "flat" });
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
    patchSessionView("host-a", { layout: "flat" });
    const { back, showInTree, quietRow, indexes } = showQuietInTree();
    back();
    showInTree();
    addBusySibling();
    expect(quietRow()).toBe(44);
    vi.advanceTimersByTime(0);
    expect(indexes()).toEqual([44]);
  });

  it("drops the settle pass when a collapse removes the session's row, though the list mode is unchanged", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    patchSessionView("host-a", { layout: "flat" });
    const { back, showInTree, indexes } = showQuietInTree();
    back();
    showInTree();
    vi.advanceTimersByTime(0);
    addBusySibling();
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([43, 44]);
  });

  it("drops a scroll retry in the Role view when a collapse removes its row", () => {
    patchSessionView("host-a", { layout: "flat" });
    const { failed, quietRow, indexes } = showQuietInTree(() => { patchSessionView("host-a", { grouping: "role" }); renderScreen(); });
    expect([groupRows().map((row) => `${row.group.label}:${row.expanded}`), quietRow()]).toEqual([["No role:true"], 41]);
    failed();
    toggle("role");
    expect(listRows()).toHaveLength(1);
    vi.runOnlyPendingTimers();
    expect(indexes()).toEqual([]);
  });

  it("moves a scroll retry in the Role view with its row when a new session joins the role", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    // Timing keeps its clock-driven component inside the named section.
    const times = () => walk(content()).find(line => (line as ElementLike & { type: unknown }).type === SessionTimeLines)! as ElementLike & { type: unknown };
    const usage = () => content().find((line) => line.type === SessionUsageLines)!;
    const lines = () => (content().find(line => line.type === SessionDetailSection && line.props.title === "Role and placement")!.props.values as { label: string; value: string }[])
      .map(({ label, value }) => `${label}: ${value}`);
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
    expect(screen.lines()).toEqual(["Role: QA, from the kontor.role label", "Other role declarations: None", "Workstream: Kontor plugin and prototype parity across every host", "Epic: ASMA-8541, from the epic label",
      "Workspace: TSW • ASMA-8541 roles provenance correction workspace with a long name"]);
    expect(screen.details("long").props.expanded).toBe(true);
    expect(screen.details("conflict").props.expanded).toBe(false);
    const expected: Record<string, string[]> = {
      conflict: ["Role: QA, from the role label", "Other role declarations: title says AUD", "Workstream: Review", "Epic: Unknown, no epic or task label", "Workspace: Workspace"],
      desk: ["Role: SWE, from the title", "Other role declarations: None", "Desk: Adam desk", "Workspace: Workspace"],
      cwd: ["Role: SWE, from the title", "Other role declarations: None", "Workstream: Other work: no workstream or desk label and no display mapping", "Epic: Unknown, no epic or task label", "Linking: Settings, under Presentation and associations, can map this session or link it to a plan. Project and session names never link it.", "Workspace: No workspace · /tmp/scratch"],
      nowhere: ["Role: No role: no kontor.role or role label, and no role prefix in the title", "Other role declarations: None", "Workstream: Other work: no workstream or desk label and no display mapping", "Epic: Unknown, no epic or task label", "Linking: Settings, under Presentation and associations, can map this session or link it to a plan. Project and session names never link it.", "Workspace: No workspace"],
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

  it("keeps restored Details focus without help blocking the next session action", () => {
    let helpVisible = false;
    controlHelp.current = { dismiss: () => { helpVisible = false; } };
    try {
      const screen = renderDetails(placeEntries());
      const button = { focus: vi.fn(() => { helpVisible = true; }) };
      (screen.details("long").props.ref as (node: unknown) => void)(button);
      screen.open("long");
      (screen.close().props.onPress as () => void)(); harness.flush();
      expect(button.focus).toHaveBeenCalledOnce();
      expect(helpVisible).toBe(false);
      const next = screen.row("conflict").props.children as ElementLike;
      (next.props.onPress as (entry: unknown) => void)(next.props.entry);
      expect(screen.openAgent).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ agentId: "conflict" }));

      screen.open("long");
      const scroller = { focus: vi.fn(() => { helpVisible = true; }) };
      (findByTestID(harness.current(), "sessions-list").props.ref as RefLike).current = { getScrollableNode: () => scroller };
      (screen.details("long").props.ref as (node: unknown) => void)(null);
      vi.stubGlobal("document", { activeElement: null, body: {} });
      directory.state = { ...(directory.state as object), entries: placeEntries().filter(entry => entry.id !== "long") };
      harness.flush();
      expect(scroller.focus).toHaveBeenCalledOnce();
      expect(helpVisible).toBe(false);
    } finally {
      controlHelp.current = null;
      vi.unstubAllGlobals();
    }
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
    expect(lines({})).toMatchObject({ "Native desk": "DESK • ADAM", "Native project ID": "prj_desk", "Native workspace ID": "wks_desk", "Workstream association": "Not declared", "Epic association": "ASMA-99101, from the epic label" });
    expect(lines({ "session:desk-with-epic": { workstream: "declared" } })["Workstream association"]).toBe("declared");
    expect(sessionDetailLines({ ...session, labels: { ...session.labels, workstream: "labelled" } }, {}).find((line) => line.label === "Workstream association")!.value).toBe("labelled");
    expect(lines({})).not.toHaveProperty("Workstream");
    expect(lines({})).not.toHaveProperty("Workstream source");
    const derived = Object.fromEntries(sessionDetailLines(session, { "session:desk-with-epic": { workstream: "kontor" } }, null, "Workstream from the Jira ws:kontor label of ASMA-99101.").map(({ label, value }) => [label, value]));
    expect(derived).toMatchObject({ "Native desk": "DESK • ADAM", "Workstream association": "kontor", "Workstream source": "Workstream from the Jira ws:kontor label of ASMA-99101." });
    expect(lines({})["Association verification"]).toBe("Workstream registry membership and epic/plan links are not checked here.");
    expect(lines({ "session:desk-with-epic": { workstream: "unregistered-association", epic: "ASMA-99999" } })).toMatchObject({
      "Native project ID": "prj_desk", "Native workspace ID": "wks_desk", "Workstream association": "unregistered-association",
      "Epic association": "ASMA-99999, from a display mapping", "Association verification": lines({})["Association verification"],
    });
  });

  it("names a declared Other work workstream or Unassigned epic as declared, and only a missing one as absent", () => {
    const chain = (entry: ReturnType<typeof sessionEntry>, mappings: Parameters<typeof sessionDetailLines>[1] = {}) =>
      sessionDetailLines(entry, mappings).filter(({ label }) => label === "Workstream" || label === "Epic").map(({ label, value }) => `${label}: ${value}`);
    const absent = ["Workstream: Other work: no workstream or desk label and no display mapping", "Epic: Unknown, no epic or task label"];
    expect(chain(sessionEntry("label", { labels: { workstream: "Other work", epic: "Unassigned" } }))).toEqual(["Workstream: Other work", "Epic: Unassigned, from the epic label"]);
    expect(chain(sessionEntry("mapped"), { "session:mapped": { workstream: "Other work", epic: "Unassigned" } })).toEqual(["Workstream: Other work", "Epic: Unassigned, from a display mapping"]);
    expect(chain(sessionEntry("jira", { labels: { "jira.epic": "Unassigned" } }))).toEqual([absent[0], "Epic: Unassigned, from the jira.epic label"]);
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
  function renderResults(compact: boolean, query = "found", entries = [
    sessionEntry("found", { title: "SWE • found", updatedAt: "2026-10-18T10:00:00.000Z" }),
    sessionEntry("found-too", { title: "QA • found too", status: "running", updatedAt: "2026-10-18T12:59:30.000Z" }),
  ]) {
    vi.setSystemTime(Date.parse("2026-10-18T13:00:00.000Z"));
    const openAgent = vi.fn();
    directory.state = { ...(directory.state as object), entries };
    patchSessionView("host-a", { query, grouping: "hierarchy", status: "all" });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: compact ? "ios" : "web" },
      navigation: { openAgent, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as unknown as PluginScreenProps));
    const result = (id: string) => (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike & { readonly type: (props: unknown) => ElementLike })({ item: listRows().find((row) => row.kind === "result" && row.entry.id === id) });
    return { openAgent, result, drawn: (id: string) => { const element = result(id); return element.type(element.props); } };
  }

  it.each([[true, "3h ago"], [false, "Updated 3 hours ago"]] as const)("gives each result its Updated time and its details beside the row, and keeps open and Show in tree (compact %s)", (compact, shown) => {
    patchSessionView("host-a", { layout: "flat" });
    const screen = renderResults(compact);
    expect(listRows().map((row) => row.kind)).toEqual(["result", "result"]);
    const drawn = screen.drawn("found");
    expect(textOf(findByTestID(drawn, "sessions-row-age"))).toBe(shown);
    expect(textOf(findByTestID(screen.drawn("found-too"), "sessions-row-age"))).toBe(compact ? "just now" : "Updated just now");
    // Compact gives the title its line to itself, and the state and the age lead the usage line; desktop keeps both by the title.
    const lineOf = (testID: string) => walk(drawn).filter((element) => (element.props.style as { flexDirection?: string } | undefined)?.flexDirection === "row" && testIDs(element.props.children).includes(testID)).at(-1)!;
    const kinds = (line: ElementLike) => [line.props.children].flat(2).filter(Boolean).map((child) => {
      const element = child as ElementLike & { type: unknown };
      return element.type === SessionUsage ? "usage" : element.type === SessionStateMarker ? "state" : String(element.props.testID ?? "other");
    });
    expect(kinds(lineOf("sessions-row-title"))).toEqual(compact ? ["sessions-row-title"] : ["sessions-row-title", "sessions-row-age", "other", "state"]);
    if (compact) expect(kinds(lineOf("sessions-row-age"))).toEqual(["state", "sessions-row-age", "usage"]);
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

  it.each([true, false])("lets compact results that share a role and task wrap their titles to where they differ, and keeps desktop titles on one line (compact %s)", (compact) => {
    patchSessionView("host-a", { layout: "flat" });
    // Synthetic sessions whose titles differ only after the shared role and task.
    const titles = { first: "SWE • ASMA-1 • first subject", second: "SWE • ASMA-1 • second subject" };
    const screen = renderResults(compact, "subject", Object.entries(titles).map(([id, title]) => sessionEntry(id, { title })));
    expect(listRows().map((row) => row.kind)).toEqual(["result", "result"]);
    for (const [id, title] of Object.entries(titles)) {
      const drawn = screen.drawn(id);
      const shown = findByTestID(drawn, "sessions-row-title");
      // The whole title is drawn; compact gives it a line of its own and three lines to reach the subject.
      expect(textOf(shown)).toBe(title);
      expect(shown.props.numberOfLines).toBe(compact ? 3 : 1);
      const line = walk(drawn).filter((element) => (element.props.style as { flexDirection?: string } | undefined)?.flexDirection === "row" && testIDs(element.props.children).includes("sessions-row-title")).at(-1)!;
      expect([line.props.children].flat(2).filter(Boolean)).toHaveLength(compact ? 1 : 4);
      // The row and Show in tree keep the full title in their names.
      expect(String(findByTestID(drawn, `sessions-row-${id}`).props.accessibilityLabel).startsWith(`${title}. `)).toBe(true);
      expect(String((drawn.props.children as ElementLike[])[2]!.props.accessibilityLabel)).toBe(`Show ${title} in the tree`);
    }
  });

  it("gives rows the Updated time itself, not an age, while the list is not live", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    patchSessionView("host-a", { layout: "grouped" });
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
      expect(region().props).toMatchObject({ tabIndex: 0, role: "region", accessibilityLabel: "Model, reported usage, timing and placement" });
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

  it.each([true, false])("labels the primary Role total once with its state subset and stale cue, compact=%s", compact => {
    const { drawn } = renderRoles(qaEntries(), undefined, compact);
    const [qa] = groupRows();
    expect([qa!.group.label, qa!.expanded]).toEqual(["QA", false]);
    const header = drawn(qa);
    const counts = walk(header).find((element) => "aggregate" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    const words = walk(counts.type(counts.props)).map((element) => element.props.children).filter((child) => typeof child === "string" || typeof child === "number");
    expect(words).toEqual(["5 sessions", "Needs input 1"]);
    expect(walk(counts.type(counts.props)).filter(element => element.type === SessionStateMarker).map(element => element.props.state)).toEqual(["needs_input"]);
    expect(walk(header).filter((element) => element.props.children === "5")).toHaveLength(0);
    const label = () => String(walk(drawn(groupRows()[0])).find((element) => element.props.accessibilityRole === "button")!.props.accessibilityLabel);
    expect(label()).toContain("Role QA, 5 sessions. Needs input: 1 of 5 sessions. Error: 1 of 5 sessions. Running: 3 of 5 sessions.");
    directory.state = { ...(directory.state as object), status: "stale", live: false, staleReason: "Host offline" };
    harness.flush();
    expect(label()).toContain("5 sessions. Last known: Needs input: 1 of 5 sessions.");
    const staleCounts = walk(drawn(groupRows()[0])).find((element) => "aggregate" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    expect(walk(staleCounts.type(staleCounts.props)).map((element) => element.props.children).filter((child) => typeof child === "string" || typeof child === "number"))
      .toEqual(["Last known", "5 sessions", "Needs input 1"]);
  });

  it("shows a Role row's conflict cue and place, giving the compact title its whole line above the place and age", () => {
    const { renderItem, drawn } = renderRoles([
      sessionEntry("conflict", { title: "QA • check results", labels: { "kontor.role": "AUD", workstream: "Kontor", epic: "ASMA-1" } }),
      sessionEntry("plain", { title: "aud • sign off", labels: { desk: "PR Review" } }),
      sessionEntry("loose", { title: "Notes", workspaceId: null, workspaceName: null, cwd: "/tmp/notes" }),
    ]);
    expect(groupRows().map((row) => row.group.label)).toEqual(["AUD", "No role"]);
    const headerOf = (label: string) => walk(drawn(groupRows().find((row) => row.group.label === label)));
    expect(String(headerOf("No role").find((element) => element.props.accessibilityRole === "button")!.props.accessibilityLabel)).toMatch(/^No role, 1 session\. Idle: 1 of 1 session\./);
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
    expect(shown("loose")).toMatchObject({ displayTitle: undefined, displayContext: "Other work · No epic linked · No workspace · /tmp/notes", compactContext: "Other work · No workspace · /tmp/notes" });
    const row = walk(renderItem(listRows().find((candidate) => candidate.kind === "session" && candidate.entry.id === "conflict"))).find((element) => "displayContext" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
    // The harness's memo returns the component itself.
    const drawnRow = row.type(row.props);
    expect([findByTestID(drawnRow, "sessions-row-title").props.children, findByTestID(drawnRow, "sessions-row-meta").props.children]).toEqual(["QA • check results", "title says QA · ASMA-1 · Workspace"]);
    // No line holds the title beside anything else; the place and the age share the line below, the place in up to two lines.
    const lineOf = (testID: string) => walk(drawnRow).filter((element) => (element.props.style as { flexDirection?: string } | undefined)?.flexDirection === "row" && testIDs(element.props.children).includes(testID)).at(-1);
    expect(lineOf("sessions-row-title")).toBeUndefined();
    expect(testIDs(lineOf("sessions-row-meta")!.props.children)).toEqual(["sessions-row-meta", "sessions-row-age"]);
    expect(findByTestID(drawnRow, "sessions-row-meta").props).toMatchObject({ numberOfLines: 2, style: { flex: 1, minWidth: 0 } });
    expect(String(drawnRow.props.accessibilityLabel)).toContain("Role AUD from the kontor.role label. title says QA. Workstream Kontor, epic ASMA-1, workspace Workspace");
  });

  it("gives same-prefix Role subjects three compact title lines while preserving scoped IDs and full accessible titles", () => {
    const subjects = ["Collect scope fixtures", "Review layout choices", "Check keyboard focus", "Verify native IDs", "Inspect group sorting", "Exercise empty scopes", "Confirm mobile wrapping"];
    const entries = subjects.map((subject, n) => sessionEntry(`role-subject-${n}`, {
      title: `SWE • ASMA-1234 • ${subject}`, labels: { epic: "ASMA-1234" },
    }));
    patchSessionView("host-a", { layout: "grouped", epic: { kind: "known", key: "ASMA-1234" } });
    const { renderItem } = renderRoles([...entries,
      sessionEntry("outside-epic", { title: entries[0]!.title, labels: { epic: "ASMA-12340" } }),
    ]);
    expandAll();
    expect(sessionRowIds().sort()).toEqual(entries.map(entry => entry.id).sort());
    for (const entry of entries) {
      const wrapper = renderItem(listRows().find(row => row.kind === "session" && row.entry.id === entry.id));
      const row = walk(wrapper).find(element => "compactContext" in element.props) as ElementLike & { type: (props: Record<string, unknown>) => ElementLike };
      expect(row.props.compactContext).toBeTypeOf("string");
      expect(row.props.displayTitle).toBe(entry.title.replace(/^SWE • /, ""));
      const drawn = row.type(row.props);
      expect(findByTestID(drawn, "sessions-row-title").props).toMatchObject({
        children: row.props.displayTitle, numberOfLines: 3, style: { fontSize: 14, lineHeight: 20 },
      });
      expect(drawn.props.testID).toBe(`sessions-row-${entry.id}`);
      expect(drawn.props.accessibilityLabel).toContain(entry.title);
      expect(drawn.props.accessibilityLabel).toContain(`Native ID ${entry.id}`);
    }
    // This is a component contract; actual 320/390 text fitting needs rendered evidence.
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
    patchSessionView("host-a", { layout: "flat" });
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
    chooseLayout("grouped");
    expect(groupRows().map((row) => `${row.group.label}:${row.expanded}`)).toEqual(["QA:false"]);
    expect(save.mock.calls.filter(([patch]) => "expandedGroups" in patch)).toEqual([]);
  });

  it("returns to the remembered tree position when layout returns to Grouped", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("busy", { status: "running" }), sessionEntry("quiet")] };
    patchSessionView("host-a", { query: "" });
    const { scrollToOffset } = renderScreen(undefined, false);
    scroll(480);
    const search = (query: string) => { (sessionSearch().props.onChangeText as (query: string) => void)(query); harness.flush(); };
    search("quiet");
    chooseLayout("flat");
    expect(readSessionView("host-a")).toMatchObject({ treeOffset: 480, scrollOffset: 0 });
    vi.runOnlyPendingTimers();
    expect(scrollToOffset).toHaveBeenLastCalledWith({ offset: 0, animated: false });
    scroll(40);
    search("");
    expect(readSessionView("host-a").layout).toBe("flat");
    chooseLayout("grouped");
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

/** Exercise the protected native leaf renderer; Flat result presentation has separate cases. */
function renderSessionLeaf(): ElementLike & { readonly type: unknown } {
  const found = listRows().find((candidate) => candidate.kind === "session" || candidate.kind === "result")!;
  const row = found.kind === "result" ? { kind: "session", entry: found.entry, parentKey: "flat", depth: 0 } : found;
  const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike;
  const leaf = renderItem({ item: row }).props.children as ElementLike & { readonly type: (props: unknown) => ElementLike };
  return leaf.type(leaf.props) as ElementLike & { readonly type: unknown };
}

describe("SessionsScreen mobile filter page", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    accessibility.screenReader = false;
    clearSessionViews({ work: "all" });
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
    // Checkbox ARIA inside the association selector is covered in workstream-selector.test.ts.
    expect([selector().options.map((option) => option.label), selector().selected]).toEqual([["Adapter", "Board"], ["adapter"]]);
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
  it("Current filters actual tree rows while All restores closed/error rows without discarding old idle records", () => {
    directory.state = { ...(directory.state as object), entries: [sessionEntry("old", {status:"idle",updatedAt:"2001-01-01"}), sessionEntry("error",{status:"error"}),sessionEntry("closed",{status:"closed"})] };
    patchSessionView("host-a", { grouping: "flat", status: "current" }); renderCompact();
    expect(sessionRowIds()).toEqual(["old"]);
    (walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.onStatus as (status: string) => void)("all");harness.flush();
    expect(sessionRowIds().sort()).toEqual(["closed","error","old"]);
    (walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.onStatus as (status: string) => void)("current");harness.flush();
    const toolbar=walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!;
    expect(toolbar.props.activeCount).toBe(1);
    (findByTestID(findByTestID(harness.current(), "sessions-list").props.ListHeaderComponent, "sessions-mobile-clear-filters").props.onPress as () => void)();harness.flush();
    expect(readSessionView("host-a").status).toBe("all");
    expect(sessionRowIds().sort()).toEqual(["closed","error","old"]);
  });
  it("keeps a compact title and context together, with its Updated time, wrapped report metadata and state badge", () => {
    patchSessionView("host-a", { layout: "flat" });
    directory.state = { ...(directory.state as object), entries: [{ ...sessionEntry("0123456789abcdef", {
      title: "A deliberately long session title that would otherwise wrap onto a second line on a phone",
      status: "running", pendingPermissionCount: 1, archivedAt: "2026-10-18T11:00:00.000Z", workspaceName: "RSW • review",
    }), provider: "codex", model: "gpt-6.1-sol", telemetry: { lastUsage: { inputTokens: 100, outputTokens: 20 } } }] };
    patchSessionView("host-a", { grouping: "flat", membership: "archived", status: "all" });
    renderCompact();
    const leaf = renderSessionLeaf();
    expect((leaf.props.style as (state: { pressed: boolean }) => { minHeight: number })({ pressed: false }).minHeight).toBe(44);
    const title = findByTestID(leaf, "sessions-row-title");
    expect(title.props.numberOfLines).toBe(3);
    // The context is nested inside the title's text, which wraps to three lines, not a line of its own.
    expect(walk(title).some((element) => element.props?.testID === "sessions-row-meta")).toBe(true);
    expect(textOf(findByTestID(leaf, "sessions-row-meta"))).toBe("  Running · Other work › No epic linked › RSW • review · Archived");
    const lines = walk(leaf).filter((element) => (element as { type?: unknown }).type === "Text" && element.props.numberOfLines === 1);
    // The Updated value stays untruncated; model/report metadata can wrap beneath the title.
    const age = findByTestID(leaf, "sessions-row-age");
    expect(lines.filter((line) => !walk(title).includes(line))).toEqual([age]);
    // The title has the first line to itself; the state and the age lead the usage line beneath it.
    const content = leaf.props.children as ElementLike;
    const [first, usageLine] = [content.props.children].flat().filter(Boolean) as ElementLike[];
    expect(first).toBe(title);
    expect(([usageLine!.props.children].flat(2).filter(Boolean) as (ElementLike & { type: unknown })[])
      .map((child) => child.type === SessionStateMarker ? "state" : child.type === SessionUsage ? "usage" : child.props.testID)).toEqual(["state", "sessions-row-age", "usage"]);
    expect(walk(content)).toContain(age);
    expect(walk(content).some((element) => element.type === SessionUsage && (element.props.entry as { id: string }).id === "0123456789abcdef")).toBe(true);
    expect(leaf.props.accessibilityLabel).toContain("Last usage report, not a session total. Input tokens: 100.");
    expect(leaf.props.accessibilityLabel).toContain("Output tokens: 20.");
    expect(age.props.style).toMatchObject({ flexShrink: 0 });
    expect(walk(leaf).some((element) => (element as { type?: unknown }).type === SessionStateMarker && element.props.state === "needs_input")).toBe(true);
    // ST-02: the native ID is in the accessible label, never in the visible row.
    expect(textOf(leaf)).not.toContain("01234567");
    expect(leaf.props.accessibilityLabel).toContain("Native ID 0123456789abcdef");
  });
  it.each([320, 390])("lets a depth-4 compact tree row wrap its title to where titles that share a role and task differ at %ipx", (viewport) => {
    // Synthetic titles that differ only after the shared role and task.
    const titles = ["SWE • ASMA-1234 • first subject", "SWE • ASMA-1234 • second subject"];
    const entries = titles.map((title, n) => sessionEntry(`nested-${n}`, { title }));
    directory.state = { ...(directory.state as object), entries };
    patchSessionView("host-a", { grouping: "hierarchy" });
    renderCompact();
    const renderItem = findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike & { type: (props: unknown) => ElementLike };
    // The title's room: page padding 12 + 12, the depth-4 indent, Details, Archive and Favorite at 44 each, and the row's padding 8 + 4.
    const room = viewport - 24 - indentOf(4, true) - 3 * 44 - 12;
    // What the title's lines show at 7.41 px a character, the measured 78d mean at 14 px; a model, not a render. Lines break at spaces only.
    const perLine = Math.floor(room / 7.41);
    const visible = (text: string, count: number) => {
      const words = text.split(" "), lines: string[] = [];
      while (words.length > 0 && lines.length < count - 1) {
        let line = words.shift()!;
        while (words.length > 0 && `${line} ${words[0]}`.length <= perLine) line += ` ${words.shift()}`;
        lines.push(line);
      }
      return [...lines, words.join(" ").slice(0, perLine)].filter(Boolean).join(" / ");
    };
    const shown = entries.map((entry) => {
      const row = renderItem({ item: { kind: "session", entry, depth: 4, parentKey: "project:prj_1/workspace:wks_1" } });
      expect(row.type(row.props).props.style).toMatchObject({ paddingLeft: indentOf(4, true) });
      const leaf = row.props.children as ElementLike & { type: (props: unknown) => ElementLike };
      const title = findByTestID(leaf.type(leaf.props), "sessions-row-title");
      expect(title.props.numberOfLines).toBe(3);
      return visible(textOf(title), title.props.numberOfLines as number);
    });
    expect(shown[0]).not.toBe(shown[1]);
  });
  it("leaves wide layouts unchanged, including after a compact session", () => {
    patchSessionView("host-a", { layout: "flat" });
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
    // The list header holds only the work line, never the toolbar.
    expect(testIDs(findByTestID(root, "sessions-list").props.ListHeaderComponent)).not.toContain("sessions-full-toolbar");
    // Wide session rows keep their status in a compact trailing marker.
    const leaf = renderSessionLeaf();
    expect(testIDs(leaf)).not.toContain("sessions-row-meta");
    expect(walk(leaf).some((element) => element.type === SessionStateMarker)).toBe(true);
    expect(walk(leaf).some((element) => (element.props.style as { width?: number } | undefined)?.width === 95)).toBe(false);

    compact = true; harness.flush();
    expect(fullToolbarShown()).toBe(false);
    compact = false; harness.flush();
    expect(fullToolbarShown()).toBe(true);
  });
  it.each([true, false])("keeps Legend local across full-page Filters, resize and navigation without changing results (compact=%s)", (compact) => {
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true, query: "session-a", status: "idle" });
    renderScreen(undefined, compact);
    const before = readSessionView("host-a");
    const rows = listRows();
    const legend = () => walk(harness.current()).find((element) => element.type === SessionsLegend)!.props;
    const toggleDesktopFilters = () => { (walk(harness.current()).find((element) => element.type === FiltersButton)!.props.onPress as () => void)(); harness.flush(); };
    if (compact) openMobileFilters();
    expect(legend().open).toBe(false);
    (legend().onToggle as (open: boolean) => void)(true); harness.flush();
    expect(readSessionView("host-a")).toEqual({ ...before, legendOpen: true });
    if (compact) closeMobileFilters(); else toggleDesktopFilters();
    expect(walk(harness.current()).some((element) => element.type === SessionsLegend)).toBe(false);
    expect(listRows()).toEqual(rows);
    if (compact) openMobileFilters(); else toggleDesktopFilters();
    expect(legend().open).toBe(true);
    renderScreen(undefined, false); harness.flush();
    expect(legend().open).toBe(true);
    // Leaving Sessions unmounts it; the same scoped view restores disclosure state on return.
    harness.unmount(); harness.reset(); renderScreen(undefined, compact); harness.flush();
    if (compact) openMobileFilters();
    expect(legend().open).toBe(true);
    expect(readSessionView("host-a")).toMatchObject({ query: before.query, status: before.status, includeArchived: before.includeArchived, legendOpen: true });
    (legend().onToggle as (open: boolean) => void)(false); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ query: before.query, status: before.status, legendOpen: false });
    if (compact) closeMobileFilters();
    expect(listRows()).toEqual(rows);
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
    expect(walk(findByTestID(harness.current(), "sessions-options")).some((element) => element.type === SessionsLegend)).toBe(true);
  });
  it("keeps searchable scope independent from conditional Group by and Layout on compact", () => {
    const save = vi.fn(async () => true);
    const organization = { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "revision-a", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    patchSessionView("host-a", { ...toolbarOpenPatch(true), advancedOpen: true, status: "all" });
    renderScreen(organization, true); openMobileFilters();
    const selector = () => walk(harness.current()).find((node) => node.type === SessionScopeSelector && node.props.id === "scope")!.props as Parameters<typeof SessionScopeSelector>[0];
    expect(selector().options.map((choice) => choice.label)).toEqual(["Durable desks", "PR Review desk", "Adam desk", "Tickets desk", "Outside desks"]);
    selector().onOpen(true); harness.flush();
    expect(readSessionView("host-a").scopeOpen).toBe("desk");
    selector().onChange("Desks"); harness.flush();
    expect(readSessionView("host-a").shortcut).toBe("Desks");
    (findByTestID(harness.current(), "sessions-grouping").props.onPress as () => void)(); harness.flush();
    const items = walk(harness.current()).filter((node) => node.type === MenuItem);
    expect(items.map((node) => node.props.label)).toEqual(["Desks & workstreams", "Desks & workspaces", "Role"]);
    (items.find((node) => node.props.label === "Role")!.props.onPress as () => void)(); harness.flush();
    expect(save).toHaveBeenCalledWith({ grouping: "role", layout: "grouped" });
    closeMobileFilters();
    chooseLayout("flat");
    openMobileFilters();
    expect(testIDs(harness.current())).not.toContain("sessions-grouping");
    expect(readSessionView("host-a")).toMatchObject({ layout: "flat", grouping: "role", shortcut: "Desks" });
    closeMobileFilters();
    chooseLayout("grouped");
    openMobileFilters();
    expect(findByTestID(harness.current(), "sessions-grouping").props.value).toBe("Role");
    closeMobileFilters();
    expect(readSessionView("host-a")).toMatchObject({ layout: "grouped", grouping: "role", shortcut: "Desks" });
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
    // The selected chip says the state; there is no summary row repeating it.
    expect(triggers[0]!.props.status).toBe("idle"); expect(triggers[0]!.props).not.toHaveProperty("summary");
  });

  it("shows compact applied-filter values, leaves the count accessible and keeps Clear filters at 44pt", () => {
    patchSessionView("host-a", { status: "idle", query: "session", layout: "grouped", grouping: "role" });
    renderCompact({ scopeLabel: "ASMA-1" });
    openMobileFilters();
    const filters = walk(harness.current()).find((node) => node.type === MobileFiltersPage)!.props.count as { text: string; label: string };
    closeMobileFilters();
    expect(testIDs(harness.current())).not.toContain("sessions-mobile-selected-filter");
    const bar = findByTestID(harness.current(), "sessions-layout-bar");
    // Only what no visible control says: not the state chip, the work line or the layout control.
    expect(textOf(findByTestID(bar, "sessions-applied-filters"))).toBe("ASMA-1 • “session”");
    expect(findByTestID(bar, "sessions-applied-filters").props.accessibilityLabel).toBe(`ASMA-1. “session”. ${filters.label}`);
    expect(walk(bar).some((node) => node.type === SessionLayoutControl)).toBe(true);
    const clear = () => findByTestID(findByTestID(findByTestID(harness.current(), "sessions-list").props.ListHeaderComponent, "sessions-work"), "sessions-mobile-clear-filters");
    expect(clear().props).toMatchObject({ disabled: false, style: expect.objectContaining({ minHeight: 44 }) });
    (clear().props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ status: "all", query: "", layout: "grouped", grouping: "role" });
    expect(clear().props.disabled).toBe(true);
    expect(textOf(findByTestID(harness.current(), "sessions-applied-filters"))).not.toContain("session”");

    renderCompact({ compact: false, scopeLabel: "ASMA-1" });
    expect(testIDs(harness.current())).not.toContain("sessions-mobile-count");
    expect(testIDs(harness.current())).not.toContain("sessions-mobile-clear-filters");
  });

  it.each([true, false])("explains incomplete workstream availability with zero, one and unregistered associations (compact=%s)", (compact) => {
    patchSessionView("host-a", { advancedOpen: true });
    directory.state = { ...(directory.state as object), entries: [] };
    renderScreen(undefined, compact);
    if (compact) openMobileFilters();
    expect(selector().about).toContain("this list may be incomplete");
    const choices = () => selector().options.map((option) => option.label);
    expect(choices()).toHaveLength(0);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {})] };
    harness.flush();
    expect(choices()).toEqual(["Other work"]);
    toggleChoice("other work");
    expect(readSessionView("host-a").streams).toEqual([]);

    directory.state = { ...(directory.state as object), entries: [sessionEntry("missing", {}), sessionEntry("unknown", { labels: { workstream: "unregistered-key" } })] };
    harness.flush();
    expect(choices().sort()).toEqual(["Other work", "unregistered-key"]);
    expect(selector().about).toContain("Names may be unregistered");
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
      expect((toolbar().props.counts as Record<string, SessionStateCount>)[status]!.label).toContain(`${ids.length} of`);
      expect(readSessionView("host-a").query).toBe("mobile");
    }
    (toolbar().props.onChooser as () => void)(); harness.flush();
    expect(readSessionView("host-a").status).toBe("favorites");
    const chooser = walk(harness.current()).find((node) => node.type === MobileQuickChooser)!;
    (chooser.props.onClose as () => void)(); harness.flush();
    (toolbar().props.onQuery as (query: string) => void)("run"); harness.flush();
    expect(sessionRowIds()).toEqual([]);
    expect((toolbar().props.counts as Record<string, SessionStateCount>).all!.text).toBe("1/6");
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
    expect((walk(harness.current()).find((node) => node.type === MobileQuickToolbar)!.props.counts as Record<string, SessionStateCount>).all!.text).toBe("2");
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
      shortcut: "Adam", streams: [], includeArchived: true, membership: "archived", grouping: "flat", arranging: true, advancedOpen: true, toolbarOpen: false, scrollOffset: entry === "collapsed" ? 300 : 0, treeOffset: 70 });
    const compact = entry === "quick" || entry === "full-page";
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: "web" }, theme: { colors } as PluginTheme, params: {},
      directory: supplied, organization, projectId: "project-a", epicKey: "ASMA-1", scopeLabel: "ASMA-1", onClearScope, onIncludeArchived: archive, viewScope: scope }));
    expect(sessionRowIds()).toEqual([]);
    let clear: () => void;
    if (entry === "quick") clear = findByTestID(findByTestID(harness.current(), "sessions-list").props.ListHeaderComponent, "sessions-mobile-clear-filters").props.onPress as () => void;
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
    expect(readSessionView(scope)).toMatchObject({ query: "", status: "all", task: null, shortcut: null, streams: null, includeArchived: false, membership: "sessions",
      grouping: "flat", advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 0, peek: null });
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
    expect(readSessionView(scope).status).toBe("current");
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
  beforeEach(() => { vi.useFakeTimers(); clearSessionViews({ work: "all" }); harness.reset(); });
  afterEach(() => { harness.unmount(); vi.useRealTimers(); });
  it.each([true, false])("renders zero-session registered choices from operator configuration (compact=%s)", async (compact) => {
    const { mkdtemp, mkdir, writeFile, rm } = await import("node:fs/promises");
    const { join } = await import("node:path"); const { tmpdir } = await import("node:os");
    const { readWorkstreamRegistry } = await import("../server/read-sources");
    const root = await mkdtemp(join(tmpdir(), "kontor-registry-options-"));
    try {
      await mkdir(join(root, "plugin-config/kontor"), { recursive: true });
      await mkdir(join(root, "config/orchestration"), { recursive: true });
      await writeFile(join(root, "plugin-config/kontor/registry.json"), JSON.stringify({ workspaceRoot: root }));
      await writeFile(join(root, "config/orchestration/workstreams.json"), JSON.stringify({ schema_version: 1, workstreams: { access: { name: "Access" }, reporting: { name: "Reporting" } }, desks: { "pr-review-desk": { name: "Review" }, "adam-adhoc": { name: "Adam" } } }));
      vi.stubEnv("PASEO_HOME", root); vi.stubEnv("KONTOR_PLUGIN_ASMA_WORKSPACE", "");
      const registry = await readWorkstreamRegistry();
      directory.state = { ...emptyDirectoryState(), entries: [], status: "ready", live: true, inventoryComplete: true };
      patchSessionView("host-a", { query: "", advancedOpen: true, ...toolbarOpenPatch(true) });
      renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [], workstreamReads: {}, workstreamRegistry: registry });
      if (compact) openMobileFilters();
      for (const key of ["access", "reporting"]) expect(choiceFacts(key)).toContain("0 sessions");
      expect(selector().options.map((option) => option.key)).toEqual(["access", "reporting"]);
      expect(Object.keys(registry.desks)).toHaveLength(2);
    } finally { vi.unstubAllEnvs(); await rm(root, { recursive: true, force: true }); }
  });
  it.each([true, false])("renders graphless and registry-only choices, filters rows, and clears streams (compact=%s)", async (compact) => {
    const { source, read, registry } = await import("./plan-workstream-fixtures");
    const entries = [sessionEntry("linked", { labels: { epic: "ASMA-7575" } })];
    directory.state = { ...emptyDirectoryState(), entries, status: "ready", live: true, inventoryComplete: true };
    patchSessionView("host-a", { query: "", advancedOpen: true, includeArchived: false, streams: [], ...toolbarOpenPatch(true) });
    const { planWorkstreamMappings } = await import("../client/plan-workstreams");
    const mappings = planWorkstreamMappings([source], { plan: read }, entries, [], {}, {});
    renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [source], workstreamReads: { plan: read }, workstreamRegistry: registry, planMappings: mappings });
    if (compact) openMobileFilters();
    expect(choiceFacts("adopus-graphql")).toContain("1 session · 1 workspace · 1 plan · registered");
    expect(choiceFacts("access")).toContain("0 sessions");
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toEqual([]);
    if (compact) openMobileFilters();
    toggleChoice("adopus-graphql");
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
    expect(choiceFacts("adopus-graphql")).toContain("0 sessions · 1 workspace · 1 plan · registered");
  });
  it.each([true, false])("names desk-held, plan-only and unlinked choices, an unloaded registry once, and the outside-desks shortcut (compact=%s)", async (compact) => {
    const { source, read } = await import("./plan-workstream-fixtures");
    const plan = { ...source, id: "beta", path: "/docs/beta.md", epic: "ASMA-2" };
    const planRead = { ...read, source: plan, planMetadata: { workstream: "beta", epic: "ASMA-2", diagnostics: [] } };
    const desk = { projectKey: "prj_desk", projectName: "DESK • ADAM", workspaceId: "wks_desk" };
    const entries = [sessionEntry("desk-own", desk), sessionEntry("desk-filed", { ...desk, labels: { workstream: "alpha" } }),
      sessionEntry("esw-filed", { projectKey: "prj_esw", projectName: "ESW • ASMA-1", workspaceId: "wks_esw", labels: { workstream: "alpha" } }),
      sessionEntry("esw-bare", { projectKey: "prj_plan", projectName: "ESW • ASMA-2", workspaceId: "wks_plan" })];
    directory.state = { ...emptyDirectoryState(), entries, status: "ready", live: true, inventoryComplete: true };
    patchSessionView("host-a", { query: "", advancedOpen: true, ...toolbarOpenPatch(true) });
    renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [plan], workstreamReads: { beta: planRead }, planMappings: {} });
    if (compact) openMobileFilters();
    expect(choiceFacts("alpha")).toBe("alpha · 2 sessions (1 shown under DESK • ADAM) · 2 workspaces · 0 plans");
    expect(choiceFacts("adam")).toBe("adam · desk category · 1 session · 1 workspace · 0 plans · desk shows 1 more session filed under other workstreams");
    expect(choiceFacts("beta")).toBe("beta · plan only · 0 sessions · 0 workspaces · 1 plan");
    expect(choiceFacts("other work")).toBe("Other work · no workstream link · 1 session · 1 workspace · 0 plans");
    // All is selected; Details show every fact without hover.
    expect(selector().selected).toBeNull();
    const footer = selector().about;
    expect(footer).toContain("The workstream registry is not loaded, so no choice says whether it is registered.");
    expect(footer).toContain("a choice names the desk where it counts sessions shown there");
    toggleChoice("alpha");
    expect(readSessionView("host-a").streams).toEqual(["adam", "beta", "other work"]);
    if (compact) closeMobileFilters();
    if (!compact) {
      const shortcut = findByTestID(harness.current(), "sessions-shortcut-4").props;
      expect([(shortcut.choice as { label: string }).label, shortcut.count]).toEqual(["Outside desks", 2]);
    }
    // A partial read keeps the same association counts, and Filters says beside them that the read is partial.
    directory.state = { ...emptyDirectoryState(), entries, status: "ready", live: false, inventoryComplete: false, lastUpdatedAt: 0 };
    renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [plan], workstreamReads: { beta: planRead }, planMappings: {} });
    if (compact) openMobileFilters();
    expect(choiceFacts("alpha")).toBe("alpha · 2 sessions (1 shown under DESK • ADAM) · 2 workspaces · 0 plans");
    expect(choiceFacts("other work")).toBe("Other work · no workstream link · 1 session · 1 workspace · 0 plans");
    expect(textOf(findByTestID(harness.current(), "sessions-options-count"))).toContain("partial inventory read");
    if (compact) closeMobileFilters();
    const { planFallbackNote } = await import("../client/plan-workstreams");
    const note = planFallbackNote([plan], { beta: planRead }, null, "prj_plan");
    expect(sessionDetailLines(entries[3]!, {}, note).find((line) => line.label === "Linking")!.value).toBe("No known ASMA epic, so no plan applies. Settings, under Presentation and associations, can map this session or link it to a plan. Project and session names never link it.");
    // Epic sources that disagree leave no known epic, and the note agrees with the Epic line rather than claiming no label.
    const { declaredScope } = await import("../client/session-organization");
    const disputed = sessionEntry("disputed", { projectKey: "prj_plan", projectName: "ESW • ASMA-2", workspaceId: "wks_plan", labels: { epic: "ASMA-2", "jira.epic": "ASMA-3" } });
    const disputedLines = sessionDetailLines(disputed, {}, planFallbackNote([plan], { beta: planRead }, declaredScope(disputed, {}).epic, "prj_plan"));
    expect(disputedLines.find((line) => line.label === "Epic")!.value).toBe("Unknown, sources disagree: the epic label says ASMA-2; the jira.epic label says ASMA-3");
    expect(disputedLines.find((line) => line.label === "Linking")!.value).toMatch(/^No known ASMA epic, so no plan applies\. Settings/);
    expect(sessionDetailLines(entries[1]!, {}, note).some((line) => line.label === "Linking")).toBe(false);
  });
  // The reported "Epic unknown · ESW • ASMA-7869/8101/8108" rows beside a recognized "ASMA-8101 · ESW • ASMA-8101" row, as the
  // 2026-10-21 reads shape them, with neutral native IDs. A session's own labels, or the Jira capture for its task, settle its
  // epic; an unlabelled session or an unmapped empty workspace stays unknown; no project name settles an epic.
  const esw = (key: string) => ({ projectKey: `prj_${key}`, projectName: `ESW • ASMA-${key}` });
  const linking = "Settings, under Presentation and associations, can map this session or link it to a plan. Project and session names never link it.";
  type Plan = typeof import("./plan-workstream-fixtures");
  function renderEsw(compact: boolean, plan: Plan, entries: readonly ReturnType<typeof sessionEntry>[], emptyWorkspaces: readonly { id: string; name: string }[] = [],
    projects?: Parameters<typeof SessionsScreen>[0]["projects"]) {
    directory.state = { ...emptyDirectoryState(), entries, status: "ready", live: true, inventoryComplete: true };
    patchSessionView("host-a", { query: "", grouping: "hierarchy", status: "all", ...toolbarOpenPatch(true) });
    renderScreen(undefined, compact, compact ? "ios" : "web", { workstreamSources: [plan.source], workstreamReads: { plan: plan.read }, planMappings: {}, projects,
      workspaces: [...entries.map((entry) => ({ id: entry.workspaceId!, name: entry.workspaceName!, projectKey: entry.projectKey, projectName: entry.projectName })),
        ...emptyWorkspaces.map((workspace) => ({ ...workspace, ...esw("8101") }))] });
    expandAll();
  }
  // A row reads its label, then its project on the secondary line.
  const epicRows = () => listRows().flatMap((row) => row.kind === "group" && row.group.kind === "epic" ? [[row.group.context ? `${row.group.label} · ${row.group.context}` : row.group.label, row.group.totalCount] as const] : []).sort();
  /** A session's Epic and Linking lines from its own Details; Close and focus return stay with the R5 Details tests above. */
  function placement(id: string) {
    const row = listRows().find((candidate) => candidate.kind === "session" && candidate.entry.id === id);
    (((findByTestID(harness.current(), "sessions-list").props.renderItem as (info: { item: unknown }) => ElementLike)({ item: row }).props.details as ElementLike).props.onPress as () => void)();
    harness.flush();
    const dialog = walk(harness.current()).find((element) => element.props.testID === "sessions-details")!;
    const content = (findByTestID(dialog.props.children, "sessions-details-lines").props.children as unknown[]).flat() as (ElementLike & { readonly type: unknown })[];
    const values = content.find((line) => line.type === SessionDetailSection && line.props.title === "Role and placement")!.props.values as { label: string; value: string }[];
    return values.filter(({ label }) => label === "Epic" || label === "Linking").map(({ label, value }) => `${label}: ${value}`);
  }

  it.each([true, false])("keeps an empty-workspace unknown row apart from unknown sessions and the recognized epic, and says why in Details (compact=%s)", async (compact) => {
    const entries = [sessionEntry("labelled-8101", { ...esw("8101"), workspaceId: "wks_8101", labels: { "jira.epic": "ASMA-8101" } }),
      sessionEntry("bare-7869", { ...esw("7869"), workspaceId: "wks_7869" }), sessionEntry("bare-8108", { ...esw("8108"), workspaceId: "wks_8108" })];
    renderEsw(compact, await import("./plan-workstream-fixtures"), entries,
      [{ id: "wks_8101_empty_a", name: "ASMA-8107 • Merged producer provenance" }, { id: "wks_8101_empty_b", name: "TSW • ASMA-8107 • Lifecycle merged provenance" }]);
    // ESW • ASMA-8101's unknown row holds only its two empty workspaces, so no session or Details sits behind it.
    expect(epicRows()).toEqual([["ASMA-8101 · ESW • ASMA-8101", 1], ["No epic linked · ESW • ASMA-7869", 1], ["No epic linked · ESW • ASMA-8101", 0], ["No epic linked · ESW • ASMA-8108", 1]]);
    expect(listRows().flatMap((row) => row.kind === "session" && row.entry.projectKey === "prj_8101" ? [row.entry.id] : [])).toEqual(["labelled-8101"]);
    if (compact) openMobileFilters();
    expect(choiceFacts("other work")).toBe("Other work · no workstream link · 3 sessions · 3 workspaces · 0 plans");
    if (compact) closeMobileFilters();
    // The screen passes each session's own known epic to the note, so a recognized epic is named, not reported unknown.
    expect(placement("labelled-8101")).toEqual(["Epic: ASMA-8101, from the jira.epic label", `Linking: No configured plan declares ASMA-8101. ${linking}`]);
    for (const id of ["bare-7869", "bare-8108"]) expect(placement(id)).toEqual(["Epic: Unknown, no epic or task label", `Linking: No known ASMA epic, so no plan applies. ${linking}`]);
  });

  it.each([true, false])("names the epic a task-only or dual-epic session settles on, and shows a task-only epic as unknown without a Jira capture (compact=%s)", async (compact) => {
    const plan = await import("./plan-workstream-fixtures");
    const { jiraCapture } = await import("../client/page-data");
    const generated = "2026-10-20T16:03:00+00:00";
    const issue = (key: string) => ({ key, status: "In Progress", cat: "indeterminate" as const });
    const captured = jiraCapture([{ ...plan.read, source: { ...plan.source, id: "jira", kind: "jira" }, jiraHistory: { schema: "consumption/data@2", generated,
      provenance: { path: "/configured/capture.json", sha256: "b".repeat(64), capturedEpics: 1 }, selection: ["ASMA-8108"], epics: [{ ...issue("ASMA-8108"), children: [issue("ASMA-8343")] }] } }]);
    // Only the capture names task ASMA-8343's parent. jira.parent is not an epic label, so the jira.epic label alone settles the second.
    const shapes = [sessionEntry("task-8343", { ...esw("8108"), workspaceId: "wks_8343", labels: { "asma.task": "ASMA-8343" } }),
      sessionEntry("dual-8001", { ...esw("7869"), workspaceId: "wks_8001", labels: { "jira.epic": "ASMA-8001", "jira.parent": "ASMA-7869" } })];
    const dual = ["Epic: ASMA-8001, from the jira.epic label", `Linking: No configured plan declares ASMA-8001. ${linking}`];
    renderEsw(compact, plan, shapes.map((entry) => ({ ...entry, jira: captured })));
    expect(epicRows()).toEqual([["ASMA-8001 · ESW • ASMA-7869", 1], ["ASMA-8108 · ESW • ASMA-8108", 1]]);
    expect(placement("task-8343")).toEqual([`Epic: ASMA-8108, from the Jira capture of ${generated} for ASMA-8343`, `Linking: No configured plan declares ASMA-8108. ${linking}`]);
    expect(placement("dual-8001")).toEqual(dual);
    // With no Jira capture read, the same task-only session is unknown and says why; the labelled one is unchanged.
    renderEsw(compact, plan, shapes.map((entry) => ({ ...entry, jira: jiraCapture([]) })));
    expect(epicRows()).toEqual([["ASMA-8001 · ESW • ASMA-7869", 1], ["No epic linked · ESW • ASMA-8108", 1]]);
    expect(placement("task-8343")).toEqual(["Epic: Unknown, no Jira capture read for ASMA-8343", `Linking: No known ASMA epic, so no plan applies. ${linking}`]);
    expect(placement("dual-8001")).toEqual(dual);
  });

  // A native project split by its sessions' own labels: its No epic linked row sits under its recognised one and says why,
  // on a phone too; same-named native projects read apart by one folder of their root. No session moves or merges.
  it.each([true, false])("shows a split project's No epic linked row under its recognised row and tells same-named projects apart (compact=%s)", async (compact) => {
    const folders = ["asma-8564", "asma-8574"];
    const entries = [sessionEntry("labelled-7869", { ...esw("7869"), workspaceId: "wks_7869_a", labels: { "jira.epic": "ASMA-7869" } }), sessionEntry("bare-7869", { ...esw("7869"), workspaceId: "wks_7869_b" }),
      sessionEntry("labelled-8101", { ...esw("8101"), workspaceId: "wks_8101", labels: { "jira.epic": "ASMA-8101" } }),
      ...folders.map((folder) => sessionEntry(`kontor-${folder}`, { projectKey: `prj_${folder}`, projectName: "asma-rs-kontor", workspaceId: `wks_${folder}` }))];
    renderEsw(compact, await import("./plan-workstream-fixtures"), entries, [], folders.map((folder) => ({ id: `prj_${folder}`, name: "asma-rs-kontor", root: `/checkouts/.worktrees/${folder}/asma-rs-kontor` })));
    expect(listRows().flatMap((row) => row.kind === "group" && row.group.kind === "epic" ? [[row.group.label, row.group.totalCount, row.group.detail]] : [])).toEqual([
      ["ASMA-7869", 1, "ESW • ASMA-7869 · 1 more session of this project has no known epic"],
      ["No epic linked", 1, "ESW • ASMA-7869 · 1 session with no known epic · same project as ASMA-7869"],
      ["ASMA-8101", 1, "ESW • ASMA-8101"],
      ["No epic linked", 1, "asma-rs-kontor (asma-8564) · 1 session with no known epic"],
      ["No epic linked", 1, "asma-rs-kontor (asma-8574) · 1 session with no known epic"],
    ]);
    expect(listRows().flatMap((row) => row.kind === "session" ? [row.entry.id] : []).sort()).toEqual(entries.map((entry) => entry.id).sort());
  });
});

describe("My work, explicit order and Reset view on the Sessions screen", () => {
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(Date.parse("2026-10-21T13:00:00Z")); clearSessionViews(); harness.reset(); });
  afterEach(() => { harness.unmount(); vi.useRealTimers(); });

  /** Synthetic realm and people only. */
  const projection = responsibilitySchema.parse({
    schema: "kontor/responsibility@1", realm: "tracker.example", generatedAt: "2026-10-21T12:00:00Z", expiresAt: "2026-10-22T12:00:00Z",
    provenance: { collector: "collect-responsibility@1", selection: ["ASMA-100", "ASMA-300"], complete: true, workstreams: null },
    people: [{ accountId: "account-a", displayName: "Alex Example", active: true, accountType: "atlassian" }, { accountId: "account-b", displayName: "Blair Example", active: true, accountType: "atlassian" }],
    issues: [{ key: "ASMA-100", type: "Epic", level: 1, parent: null, assignee: "account-a", workstreams: [] },
      { key: "ASMA-101", type: "Story", level: 0, parent: "ASMA-100", assignee: "account-b" },
      { key: "ASMA-300", type: "Epic", level: 1, parent: null, assignee: null, workstreams: [] }],
    workstreams: [],
  });
  const ready = (expiresAt = projection.expiresAt): ResponsibilityRead => ({ status: "ready", projection: { ...projection, expiresAt }, revision: "r1", readAt: "2026-10-21T12:59:00Z", diagnostics: [] });
  const entries = [
    sessionEntry("alpha-old", { title: "SWE • b alpha", labels: { epic: "ASMA-100" }, updatedAt: "2026-10-19T10:00:00Z", workspaceId: "w1", workspaceName: "one" }),
    sessionEntry("alpha-task", { title: "QA • c task", labels: { epic: "ASMA-100", "asma.task": "ASMA-101" }, updatedAt: "2026-10-21T10:00:00Z", workspaceId: "w1", workspaceName: "one" }),
    sessionEntry("other", { title: "SWE • a other", labels: { epic: "ASMA-300" }, updatedAt: "2026-10-20T10:00:00Z", workspaceId: "w2", workspaceName: "two" }),
    sessionEntry("unread", { title: "AUD • d unread", labels: { epic: "ASMA-999" }, updatedAt: "2026-10-18T10:00:00Z", workspaceId: "w3", workspaceName: "three" }),
  ];
  function render(responsibility: ResponsibilityRead | null) {
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact: false, platform: "web" }, theme: { colors } as PluginTheme, params: {}, responsibility,
      directory: { state: { ...emptyDirectoryState(), status: "ready" as const, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, entries }, refresh: vi.fn() },
    } as Parameters<typeof SessionsScreen>[0]));
  }
  const press = (testID: string) => { (findByTestID(harness.current(), testID).props.onPress as () => void)(); harness.flush(); };
  const message = () => textOf(findByTestID(harness.current(), "sessions-work-message"));
  const empty = () => findByTestID(harness.current(), "sessions-list").props.ListEmptyComponent as ElementLike;
  function openFilters() {
    if (walk(harness.current()).some((element) => element.props.testID === "sessions-options")) return;
    const toggle = walk(findByTestID(harness.current(), "sessions-full-toolbar")).find((element) => element.props.expanded === false && "activeCount" in element.props)!;
    (toggle.props.onPress as () => void)(); harness.flush();
  }
  function choose(menu: "sort" | "person" | "workingAs", label: string) {
    openFilters(); press(menu === "workingAs" ? "sessions-working-as" : `sessions-${menu}`);
    const item = walk(findByTestID(harness.current(), `sessions-${menu}-choices`)).find((element) => element.type === MenuItem && element.props.label === label)!;
    (item.props.onPress as () => void)(); harness.flush();
  }

  /** Every native state and every kind of Jira relation: missing labels, unassigned, other assignee, conflicting sources, not in the projection. */
  const local = [
    sessionEntry("running-mine", { status: "running", labels: { epic: "ASMA-100" }, workspaceId: "w1", workspaceName: "one" }),
    sessionEntry("starting-other", { status: "initializing", labels: { epic: "ASMA-100", "asma.task": "ASMA-101" }, workspaceId: "w1", workspaceName: "one" }),
    sessionEntry("idle-unassigned", { labels: { epic: "ASMA-300" }, workspaceId: "w2", workspaceName: "two" }),
    sessionEntry("error-missing", { status: "error", workspaceId: "w2", workspaceName: "two" }),
    sessionEntry("input-unread", { needsInput: true, labels: { epic: "ASMA-999" }, workspaceId: "w3", workspaceName: "three" }),
    sessionEntry("idle-conflict", { labels: { epic: "ASMA-300", "asma.task": "ASMA-101" }, workspaceId: "w3", workspaceName: "three" }),
  ];
  const localIds = local.map((entry) => entry.id).sort();
  function renderLocal(responsibility: ResponsibilityRead | null, hostId = "host-a") {
    harness.render(() => SessionsScreen({ host: { id: hostId, label: "Host" }, layout: { compact: false, platform: "web" }, theme: { colors } as PluginTheme, params: {}, responsibility,
      directory: { state: { ...emptyDirectoryState(), status: "ready" as const, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, entries: local }, refresh: vi.fn() },
    } as Parameters<typeof SessionsScreen>[0]));
  }

  it("defaults to Current within My work and restores the error session through All visibility", () => {
    patchSessionView("host-a", { grouping: "flat" });
    renderLocal(ready());
    expect(readSessionView("host-a").status).toBe("current");
    expect(sessionRowIds().sort()).toEqual(localIds.filter((id) => id !== "error-missing"));
    const all = walk(harness.current()).find((element) => element.type === FilterChip && element.props.status === "all")!;
    (all.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a").status).toBe("all");
    expect(sessionRowIds().sort()).toEqual(localIds);
    expect(message()).toBe("My work: every session on this host.");
  });

  it.each([true, false])("keeps archive membership disjoint through state, search, native scope, assignment and order (compact=%s)", (compact) => {
    const archives = local.map((entry) => ({ ...entry, id: `archive-${entry.id}`, archivedAt: "2026-10-18T11:00:00Z" }));
    writeWorkingAs("host-a", "tracker.example", "account-a");
    patchSessionView("host-a", { grouping: "flat", status: "all", query: "Session", assignment: "me", sort: "title", sortDirection: "desc", advancedOpen: true, ...toolbarOpenPatch(true) });
    const onIncludeArchived = vi.fn();
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: "web" }, theme: { colors } as PluginTheme,
      params: {}, projectId: "prj_1", epicKey: "ASMA-100", responsibility: ready(), onIncludeArchived,
      directory: { state: { ...emptyDirectoryState(), status: "ready" as const, entries: [...local, ...archives], pages: 1, lastUpdatedAt: 0, inventoryComplete: true }, refresh: vi.fn() },
    }));
    expect(sessionRowIds()).toEqual(["starting-other", "running-mine"]);
    if (compact) openMobileFilters();
    const toggle = findByTestID(harness.current(), "sessions-advanced-archived");
    expect(toggle.props.label).toBe("Archived history");
    (toggle.props.onPress as () => void)(); harness.flush();
    for (const state of ["all", "running", "current", "attention"]) {
      expect(findByTestID(harness.current(), `sessions-filter-${state}`).props.count).toMatchObject({ text: "…", label: "Reading sessions for this membership" });
    }
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toEqual(["archive-starting-other", "archive-running-mine"]);
    expect(readSessionView("host-a")).toMatchObject({ membership: "archived", status: "all", query: "Session", assignment: "me", grouping: "flat", sort: "title", sortDirection: "desc" });
    expect(onIncludeArchived).toHaveBeenLastCalledWith(true);
    setStateFilter("running");
    expect(sessionRowIds()).toEqual(["archive-running-mine"]);
    setStateFilter("idle"); expect(sessionRowIds()).toEqual([]);
    setStateFilter("attention"); expect(sessionRowIds()).toEqual([]);
    setStateFilter("current"); expect(sessionRowIds()).toEqual([]);
    setStateFilter("all");
    expect(sessionRowIds()).toEqual(["archive-starting-other", "archive-running-mine"]);
    if (compact) openMobileFilters();
    (findByTestID(harness.current(), "sessions-advanced-archived").props.onPress as () => void)(); harness.flush();
    if (compact) closeMobileFilters();
    expect(sessionRowIds()).toEqual(["starting-other", "running-mine"]);
    expect(onIncludeArchived).toHaveBeenLastCalledWith(false);
  });

  it("shows every local session in My work with All visibility, whatever the projection or Working as", () => {
    patchSessionView("host-a", { grouping: "flat", status: "all" });
    const reads: (ResponsibilityRead | null)[] = [null,
      { status: "unavailable", projection: null, revision: null, readAt: "2026-10-21T12:59:00Z", diagnostics: ["No responsibility projection is installed on this host."] },
      ready("2026-10-21T12:30:00Z"), ready()];
    for (const read of reads) {
      renderLocal(read);
      expect(sessionRowIds().sort()).toEqual(localIds);
      expect(message()).toBe("My work: every session on this host.");
      press("sessions-work-show-all");
      expect(sessionRowIds().sort()).toEqual(localIds);
      expect(message()).toBe("All work: every session on this host, with empty workspaces and plan-only choices.");
      press("sessions-work-show-mine");
    }
    expect(walk(harness.current()).some((element) => element.props.testID === "sessions-work-choose")).toBe(false);
    // Choosing who you are working as filters nothing by itself.
    choose("workingAs", "Alex Example");
    expect(sessionRowIds().sort()).toEqual(localIds);
    expect(message()).toBe("My work: every session on this host.");
    openFilters();
    expect(textOf(findByTestID(harness.current(), "sessions-responsibility-source"))).toContain("not remembered on this device, which offers Kontor no store");
  });

  it("narrows by assignment only when chosen, and says what is unknown without calling it someone else's", () => {
    patchSessionView("host-a", { grouping: "flat" });
    render(ready());
    openFilters(); press("sessions-assignment-me");
    expect(sessionRowIds()).toEqual([]);
    expect(message()).toBe("My work · Assigned to me needs Working as: choose who you are working as in Filters. It is not sign-in.");
    expect(findByTestID(harness.current(), "sessions-work-choose")).toBeDefined();
    expect(walk(empty()).some((element) => element.props.testID === "sessions-empty-any-assignment")).toBe(true);
    choose("workingAs", "Alex Example");
    expect(sessionRowIds().sort()).toEqual(["alpha-old", "alpha-task"]);
    expect(message()).toBe("My work · Assigned to me, working as Alex Example: 2 sessions here (2 epic assignee) · 1 with unknown assignment.");
    press("sessions-assignment-show-unknown");
    expect(sessionRowIds()).toEqual(["unread"]);
    expect(message()).toBe("My work · Unknown assignment for Alex Example: 1 session here with missing or unknown assignment data.");
    // A named person replaces the Working-as person; the task assignee matches only the task's session.
    choose("person", "Blair Example");
    expect(sessionRowIds()).toEqual(["alpha-task"]);
    expect(message()).toBe("My work · Assigned to Blair Example: 1 session here (1 task assignee) · 1 with unknown assignment.");
    press("sessions-assignment-show-any");
    expect(sessionRowIds().sort()).toEqual(["alpha-old", "alpha-task", "other", "unread"]);
  });

  it("keeps Working as per host: the same session IDs on another host stay unfiltered and unset", () => {
    patchSessionView("host-a", { grouping: "flat", status: "all", assignment: "me" });
    patchSessionView("host-b", { grouping: "flat", status: "all", assignment: "me" });
    renderLocal(ready());
    choose("workingAs", "Alex Example");
    // The epic assignee owns both ASMA-100 sessions; the conflicting labels stay unknown, not someone else's.
    expect(sessionRowIds().sort()).toEqual(["running-mine", "starting-other"]);
    renderLocal(ready(), "host-b");
    expect(message()).toBe("My work · Assigned to me needs Working as: choose who you are working as in Filters. It is not sign-in.");
    press("sessions-assignment-show-any");
    expect(sessionRowIds().sort()).toEqual(localIds);
  });

  it("never shows Not set while the projection is unread or unavailable", () => {
    render(null);
    openFilters();
    expect(findByTestID(harness.current(), "sessions-working-as").props.value).toBe("Loading…");
    render({ status: "unavailable", projection: null, revision: null, readAt: "2026-10-21T12:59:00Z", diagnostics: [] });
    expect(findByTestID(harness.current(), "sessions-working-as").props.value).toBe("Unavailable");
    render(ready());
    expect(findByTestID(harness.current(), "sessions-working-as").props.value).toBe("Not set");
  });

  it("prunes groups with nothing in a person scope", () => {
    render(ready());
    press("sessions-work-show-all");
    const all = groupRows().map((row) => row.group.label);
    choose("person", "Blair Example");
    const scoped = groupRows();
    expect(scoped.length).toBeLessThan(all.length);
    expect(scoped.every((row) => row.group.aggregate.total > 0)).toBe(true);
  });

  it("ends an Assigned to me peek when Working as changes", () => {
    render(ready());
    patchSessionView("host-a", { assignment: "me", query: "alpha", peek: { sessionId: "alpha-old", resultsOffset: 0 } });
    harness.flush();
    expect(readSessionView("host-a").peek).not.toBeNull();
    choose("workingAs", "Blair Example");
    expect(readSessionView("host-a").peek).toBeNull();
  });

  it("sorts the list and the results, and disables Arrange until Current order", () => {
    patchSessionView("host-a", { grouping: "flat", work: "all" });
    render(ready());
    choose("sort", "Last changed at");
    expect(sessionRowIds()).toEqual(["alpha-task", "other", "alpha-old", "unread"]);
    press("sessions-sort-asc");
    expect(sessionRowIds()).toEqual(["unread", "alpha-old", "other", "alpha-task"]);
    expect(findByTestID(harness.current(), "sessions-arrange").props.disabled).toBe(true);
    expect(textOf(findByTestID(harness.current(), "sessions-sort-help"))).toContain("Choose Current order to arrange.");
    choose("sort", "Title");
    const search = (query: string) => { (sessionSearch().props.onChangeText as (query: string) => void)(query); harness.flush(); };
    search("•");
    expect(listRows().map((row) => row.kind)).toEqual(["result", "result", "result", "result"]);
    expect(sessionRowIds()).toEqual(["unread", "alpha-task", "other", "alpha-old"]);
    choose("sort", "Current order");
    expect(textOf(findByTestID(harness.current(), "sessions-sort-help"))).toBe("Current order keeps state order, your saved arrangement and Role order.");
  });

  it("clears filters to All work and Any assignment keeping order, grouping and Working as; Reset view returns to My work and Any assignment", () => {
    patchSessionView("host-a", { grouping: "flat", assignment: "person:account-b", sort: "title", sortDirection: "desc" });
    render(ready());
    choose("workingAs", "Alex Example");
    (walk(harness.current()).find((node) => node.type === ClearFiltersButton)!.props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ work: "all", assignment: "any", sort: "title", sortDirection: "desc", grouping: "flat" });
    openFilters();
    expect(findByTestID(harness.current(), "sessions-working-as").props.value).toBe("Alex Example");
    openFilters(); press("sessions-reset-view");
    const view = readSessionView("host-a");
    expect(workScopeOf(view)).toBe("mine");
    expect(view.assignment).toBe("any");
    openFilters();
    expect(findByTestID(harness.current(), "sessions-working-as").props.value).toBe("Alex Example");
    expect([view.sort, view.sortDirection, view.grouping]).toEqual([undefined, undefined, undefined]);
  });
});

describe("independent Layout, Group by and exact scope", () => {
  beforeEach(() => {
    vi.useFakeTimers(); vi.setSystemTime(Date.parse("2026-10-21T13:00:00Z"));
    clearSessionViews({ work: "all", status: "all" }); harness.reset();
    patchSessionView("host-a", { advancedOpen: true, toolbarOpen: true });
  });
  afterEach(() => { harness.unmount(); vi.useRealTimers(); });
  const press = (id: string) => { (findByTestID(harness.current(), id).props.onPress as () => void)(); harness.flush(); };
  const scopeControl = (id: string) => walk(harness.current()).find((node) => node.type === SessionScopeSelector && node.props.id === id)!;
  const chooseGroup = (label: string) => {
    press("sessions-grouping");
    const menu = findByTestID(harness.current(), "sessions-grouping-choices");
    (walk(menu).find((node) => node.props.label === label)!.props.onPress as () => void)(); harness.flush();
  };

  it.each([320, 360])("scrolls the utility row inside the list at 390×%s and pins it again after resize without losing filters", (height) => {
    directory.state = { ...emptyDirectoryState(), status: "ready", entries: [sessionEntry("match", { title: "SWE topic" })], inventoryComplete: true, lastUpdatedAt: 0 };
    patchSessionView("host-a", { layout: "grouped", grouping: "role", query: "topic", status: "idle", sort: "title" });
    renderScreen(undefined, true);
    const resize = (height: number) => {
      ((harness.current() as ElementLike).props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: 390, height } } }); harness.flush();
    };
    const list = () => findByTestID(harness.current(), "sessions-list");
    const headerControls = () => walk(list().props.ListHeaderComponent).filter((node) => node.type === SessionLayoutControl);
    const pinnedBars = () => ((harness.current() as ElementLike).props.children as ElementLike[]).filter((node) => node?.props?.testID === "sessions-layout-bar");
    resize(height);
    expect(headerControls()).toHaveLength(1);
    expect(pinnedBars()).toHaveLength(0);
    chooseLayout("flat");
    expect(headerControls()[0]!.props.value).toBe("flat");
    expect(listRows().map((row) => row.kind)).toEqual(["result"]);
    resize(800);
    expect(headerControls()).toHaveLength(0);
    expect(pinnedBars()).toHaveLength(1);
    expect(walk(harness.current()).filter((node) => node.type === SessionLayoutControl)).toHaveLength(1);
    expect(readSessionView("host-a")).toMatchObject({ layout: "flat", grouping: "role", query: "topic", status: "idle", sort: "title" });
    resize(height);
    expect(headerControls()).toHaveLength(1);
    expect(pinnedBars()).toHaveLength(0);
    chooseLayout("grouped");
    expect(headerControls()[0]!.props.value).toBe("grouped");
  });

  it.each([320, 390, 1440])("keeps one layout control outside the list and collapsing filters at %s, and restores its scoped state on remount", (width) => {
    const compact = width < 600;
    directory.state = { ...emptyDirectoryState(), status: "ready", entries: [sessionEntry("match", { title: "SWE topic" })], inventoryComplete: true, lastUpdatedAt: 0 };
    patchSessionView("host-a", { layout: "grouped", grouping: "role", query: "topic", status: "idle", sort: "title", sortDirection: "desc" });
    const save = vi.fn(async (_patch: object) => true);
    const organization = { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "r", saving: false, saveError: null, save, reload: async () => {}, reset: async () => true }, save } as unknown as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    renderScreen(organization, compact);
    ((harness.current() as ElementLike).props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width, height: 800 } } }); harness.flush();
    scroll(320);
    const controls = () => walk(harness.current()).filter((node) => node.type === SessionLayoutControl);
    expect(controls()).toHaveLength(1);
    expect(walk(findByTestID(harness.current(), "sessions-list")).some((node) => node.type === SessionLayoutControl)).toBe(false);
    expect(walk(findByTestID(harness.current(), "sessions-layout-bar")).some((node) => node.type === SessionLayoutControl)).toBe(true);
    const original = readSessionView("host-a");
    chooseLayout("flat");
    expect(save).toHaveBeenLastCalledWith({ layout: "flat", grouping: "role" });
    expect(readSessionView("host-a")).toMatchObject({ query: original.query, status: original.status, sort: original.sort, sortDirection: original.sortDirection, grouping: "role" });
    expect(listRows().map((row) => row.kind)).toEqual(["result"]);
    if (compact) {
      openMobileFilters();
      expect(controls()).toHaveLength(0);
      expect(testIDs(harness.current())).not.toContain("sessions-layout-flat");
      expect(testIDs(harness.current())).not.toContain("sessions-grouping");
      closeMobileFilters();
    }
    harness.unmount(); harness.reset(); renderScreen(organization, compact);
    expect(controls()[0]!.props.value).toBe("flat");
    chooseLayout("grouped");
    expect(controls()[0]!.props.value).toBe("grouped");
    expect(readSessionView("host-a").grouping).toBe("role");
    expect(readSessionView("host-b").layout).toBeUndefined();
    expect(save.mock.calls.every(([patch]) => Object.keys(patch).every((key) => key === "layout" || key === "grouping"))).toBe(true);
  });

  it.each([320, 390, 1280])("retains the exact desk/workstream/epic + search/status/task/person intersection across layouts at %s", (width) => {
    const compact = width < 600;
    const projection = responsibilitySchema.parse({ schema: "kontor/responsibility@1", realm: "tracker.example", generatedAt: "2026-10-21T12:00:00Z", expiresAt: "2026-10-22T12:00:00Z",
      provenance: { collector: "collect-responsibility@1", selection: ["ASMA-10", "ASMA-100"], complete: true, workstreams: null },
      people: [{ accountId: "person-a", displayName: "Person Example", active: true, accountType: "atlassian" }],
      issues: [{ key: "ASMA-10", type: "Epic", level: 1, parent: null, assignee: "person-a", workstreams: [] },
        { key: "ASMA-100", type: "Epic", level: 1, parent: null, assignee: "person-a", workstreams: [] }], workstreams: [] });
    const base = { projectKey: "prj_desk", projectName: "DESK • ADAM", workspaceId: "w_adam", workspaceName: "ADAM", title: "QA • selected topic", labels: { workstream: "alpha", epic: "ASMA-10", "asma.task": "ASMA-11" } };
    const selected = sessionEntry("selected", base);
    const entries = [selected, sessionEntry("twin", { ...base, projectKey: "prj_twin" }),
      sessionEntry("wrong-stream", { ...base, labels: { ...base.labels, workstream: "alphabet" } }),
      sessionEntry("wrong-epic", { ...base, labels: { ...base.labels, epic: "ASMA-100" } }),
      sessionEntry("wrong-state", { ...base, status: "running" }), sessionEntry("wrong-search", { ...base, title: "QA • different" }),
      sessionEntry("wrong-task", { ...base, labels: { ...base.labels, "asma.task": "ASMA-110" } })];
    const save = vi.fn(async (_patch: object) => true);
    patchSessionView("host-a", { layout: "grouped", shortcut: "desk-project:prj_desk", streams: ["alpha"], epic: { kind: "known", key: "ASMA-10" },
      query: "selected topic", status: "idle", task: { jira: "ASMA-11", kontorId: null, label: "ASMA-11" }, assignment: "person:person-a", sort: "title", sortDirection: "desc" });
    harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Host" }, layout: { compact, platform: compact ? "ios" : "web" }, theme: { colors } as PluginTheme, params: {},
      projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES,
      directory: { state: { ...emptyDirectoryState(), status: "ready", entries, inventoryComplete: true, lastUpdatedAt: 0 }, refresh: vi.fn() },
      responsibility: { status: "ready", projection, revision: "r1", readAt: "2026-10-21T12:59:00Z", diagnostics: [] },
      organization: { state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "r1", saving: false, saveError: null, save: async () => true, reset: async () => true, reload: async () => {} }, save },
    } as Parameters<typeof SessionsScreen>[0]));
    ((harness.current() as ElementLike).props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width, height: 800 } } }); harness.flush();
    const original = readSessionView("host-a");
    expect(sessionRowIds()).toEqual(["selected"]);
    if (compact) openMobileFilters();
    for (const grouping of ["Role", "Desks & workspaces", "Desks & workstreams"]) {
      chooseGroup(grouping);
      if (compact) closeMobileFilters();
      expect(sessionRowIds()).toEqual(["selected"]);
      chooseLayout("flat");
      if (compact) openMobileFilters();
      expect(testIDs(harness.current())).not.toContain("sessions-grouping");
      if (compact) closeMobileFilters();
      expect(sessionRowIds()).toEqual(["selected"]);
      expect(listRows().map((row) => row.kind)).toEqual(["result"]);
      const drawn = (findByTestID(harness.current(), "sessions-list").props.renderItem as (info: unknown) => ElementLike)({ item: listRows()[0] });
      expect(drawn.props.entry).toMatchObject(selected);
      expect(drawn.props.provenance).toMatchObject({ parts: ["DESK • ADAM", "ADAM"], nativeDesk: { id: "prj_desk", workspaceId: "w_adam" }, association: "Association alpha · ASMA-10" });
      chooseLayout("grouped");
      if (compact) openMobileFilters();
      expect(readSessionView("host-a")).toMatchObject({ query: original.query, status: original.status, shortcut: original.shortcut, streams: original.streams, epic: original.epic, task: original.task, assignment: original.assignment, sort: original.sort, sortDirection: original.sortDirection });
    }
    if (compact) closeMobileFilters();
    expect(save.mock.calls.every(([patch]) => Object.keys(patch).every((key) => key === "layout" || key === "grouping"))).toBe(true);
    expect(readSessionView("host-b").epic).toBeUndefined();
  });

  it("sorts the whole Flat list, while Grouped keeps identities and sorts its own leaves with stable ties and missing times", () => {
    const entries = [sessionEntry("z", { labels: { epic: "ASMA-1" }, updatedAt: "2026-10-20T00:00:00Z" }),
      sessionEntry("a", { labels: { epic: "ASMA-1" }, updatedAt: "2026-10-20T00:00:00Z" }),
      sessionEntry("newest", { labels: { epic: "ASMA-2" }, updatedAt: "2026-10-21T00:00:00Z" }),
      sessionEntry("missing", { labels: { epic: "ASMA-2" }, updatedAt: "invalid" })];
    directory.state = { ...emptyDirectoryState(), status: "ready", entries, inventoryComplete: true, lastUpdatedAt: 0 };
    patchSessionView("host-a", { layout: "grouped", sort: "updated", sortDirection: "desc" });
    renderScreen(); expandAll();
    const groups = groupRows().map((row) => row.group.key);
    expect(sessionRowIds()).toEqual(["a", "z", "newest", "missing"]);
    chooseLayout("flat"); expect(sessionRowIds()).toEqual(["newest", "a", "z", "missing"]);
    press("sessions-sort-asc"); expect(sessionRowIds()).toEqual(["a", "z", "newest", "missing"]);
    chooseLayout("grouped");
    expect(groupRows().map((row) => row.group.key)).toEqual(groups);
    expect(sessionRowIds()).toEqual(["a", "z", "newest", "missing"]);
  });

  it("keeps unavailable scopes explicit, removes one dimension, and clears scope without clearing filters or presentation", () => {
    directory.state = { ...emptyDirectoryState(), status: "ready", entries: nativeDeskSessions(), inventoryComplete: true, lastUpdatedAt: 0 };
    patchSessionView("host-a", { layout: "flat", grouping: "role", query: "LSA", sort: "title", shortcut: "desk-project:prj_gone", streams: ["gone"], epic: { kind: "known", key: "ASMA-404" } });
    renderScreen(undefined, false, "web", { projects: NATIVE_DESK_PROJECTS, workspaces: NATIVE_DESK_WORKSPACES });
    expect(sessionRowIds()).toEqual([]);
    expect(findByTestID(harness.current(), "sessions-remove-desk").props.label).toContain("Unavailable native desk");
    expect(findByTestID(harness.current(), "sessions-remove-streams").props.label).toContain("Unavailable (gone)");
    expect(findByTestID(harness.current(), "sessions-remove-epic").props.label).toContain("ASMA-404 (unavailable)");
    expect(scopeControl("epic").props.selected).toBe("known:ASMA-404");
    press("sessions-remove-epic"); expect(readSessionView("host-a").epic).toBeNull();
    expect(readSessionView("host-a").streams).toEqual(["gone"]);
    press("sessions-clear-scope");
    expect(readSessionView("host-a")).toMatchObject({ shortcut: null, streams: null, epic: null, query: "LSA", sort: "title", layout: "flat", grouping: "role" });
    expect(sessionRowIds().length).toBeGreaterThan(0);
    (scopeControl("epic").props.onChange as (key: string) => void)("unknown"); harness.flush();
    expect(readSessionView("host-a").epic).toEqual({ kind: "unknown" });
    expect(sessionRowIds().sort()).toEqual(["desk-asw", "desk-csw", "desk-plan", "desk-unlabelled", "twin"]);
  });
});
