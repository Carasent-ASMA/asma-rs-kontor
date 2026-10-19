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
    isScreenReaderEnabled: async () => false,
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
  getPaseoClient: () => {
    throw new Error("No host in the sessions-screen event test");
  },
  useHosts: () => [{ serverId: "host-a", status: "online" }],
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: "FlatList",
  TextInput: "TextInput",
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

vi.mock("../client/use-session-directory", () => ({
  useSessionDirectory: () => ({
    state: directory.state,
    refresh: () => undefined,
  }),
}));

import { SessionsScreen } from "../client/sessions-screen";
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
  return [element, ...walk(children)];
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

function renderScreen(organization?: Parameters<typeof SessionsScreen>[0]["organization"]) {
  const navigation = {
    openAgent: () => undefined,
    openBrowser: () => undefined,
    openWorkspace: () => undefined,
  };
  const tree = harness.render(() =>
    SessionsScreen({
      host: { id: "host-a", label: "srv_host-a" },
      layout: { compact: true, platform: "ios" },
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

function fullToolbarShown(): boolean {
  const slot = findByTestID(harness.current(), "sessions-full-toolbar-slot");
  return slot.props.accessibilityElementsHidden === false;
}

describe("SessionsScreen toolbar events", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    clearSessionViews();
    harness.reset();
    directory.state = {
      entries: [sessionEntry("session-a", { status: "idle" })],
      status: "ready",
      live: true,
      error: null,
      staleReason: null,
      pages: 1,
      lastUpdatedAt: 0,
      includeArchived: true,
    };
    patchSessionView("host-a", {
      query: "session-a",
      status: "all",
      attentionOnly: false,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 0,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("compensates compact collapse and near-bottom reveal through the actual screen event path", () => {
    const { scrollToOffset } = renderScreen();
    const expandedListTop =
      44 + TOOLBAR_LAYOUT_GAP + 220 + TOOLBAR_LAYOUT_GAP;
    const collapsedListTop = 44 + TOOLBAR_LAYOUT_GAP;
    const saved = readSessionView("host-a");

    scroll(300);
    expect(scrollToOffset).toHaveBeenLastCalledWith({
      offset: 300 - (220 + TOOLBAR_LAYOUT_GAP),
      animated: false,
    });
    expect(fullToolbarShown()).toBe(false);
    expect(expandedListTop - 300).toBe(
      collapsedListTop - (300 - (220 + TOOLBAR_LAYOUT_GAP)),
    );
    expect(readSessionView("host-a").scrollOffset).toBe(72);

    vi.advanceTimersByTime(300);
    scroll(1400);
    scroll(1389);
    expect(scrollToOffset).toHaveBeenLastCalledWith({
      offset: 1389 + (220 + TOOLBAR_LAYOUT_GAP),
      animated: false,
    });
    expect(fullToolbarShown()).toBe(true);
    expect(collapsedListTop - 1389).toBe(
      expandedListTop - (1389 + (220 + TOOLBAR_LAYOUT_GAP)),
    );
    expect(1389 + (220 + TOOLBAR_LAYOUT_GAP)).toBeLessThanOrEqual(
      1400 + (220 + TOOLBAR_LAYOUT_GAP),
    );

    const after = readSessionView("host-a");
    expect(after).toMatchObject({
      query: saved.query,
      status: saved.status,
      attentionOnly: saved.attentionOnly,
      includeArchived: saved.includeArchived,
      advancedOpen: saved.advancedOpen,
      scrollOffset: 1389 + (220 + TOOLBAR_LAYOUT_GAP),
    });
  });

  it("retains Favorites and desk shortcuts across a screen remount", () => {
    const organization = {
      state: { status: "ready", values: { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["session-a"] }, revision: "revision-a", saving: false, saveError: null, save: async () => true, reload: async () => {}, reset: async () => true },
      save: async () => true,
    } as NonNullable<Parameters<typeof SessionsScreen>[0]["organization"]>;
    directory.state = { ...(directory.state as object), entries: [sessionEntry("session-a", { labels: { desk: "Adam" } })] };
    renderScreen(organization);
    const chip = (label: string) => walk(harness.current()).find((element) => element.props.label === label)!;
    (chip("Favorites").props.onPress as () => void)(); harness.flush();
    (chip("Adam").props.onPress as () => void)(); harness.flush();
    expect(readSessionView("host-a")).toMatchObject({ favoritesOnly: true, shortcut: "Adam" });
    harness.reset(); renderScreen(organization);
    expect(chip("Favorites").props.active).toBe(true);
    expect(chip("Adam").props.active).toBe(true);
  });

  it("ignores a collapsed bottom bounce and its return while preserving top reveal", () => {
    const { scrollToOffset } = renderScreen();
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
