import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({
  AccessibilityInfo: {
    addEventListener: () => ({ remove: () => undefined }),
    isReduceMotionEnabled: async () => false,
    isScreenReaderEnabled: async () => false,
  },
  ActivityIndicator: () => null,
  Animated: {
    Value: class {
      constructor(_value: number) {}
      setValue() {}
      stopAnimation() {}
    },
    View: () => null,
    timing: () => ({ start: () => undefined, stop: () => undefined }),
  },
  Pressable: () => null,
  Text: () => null,
  View: () => null,
}));

vi.mock("@getpaseo/plugin/client", () => ({
  getPaseoClient: () => {
    throw new Error("No host in the sessions-screen component test");
  },
  useHosts: () => [],
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: () => null,
  TextInput: () => null,
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

import {
  CLEAR_FILTERS_TEST_ID,
  ClearFiltersButton,
  FILTERS_TOGGLE_TEST_ID,
  FILTERS_TOUCH_TARGET,
  FiltersButton,
  filtersAccessibilityLabel,
  filtersDisclosureGlyph,
  indicatorColor,
  SlimActionButton,
  SlimToolbar,
} from "../client/sessions-screen";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
} from "../client/session-view-store";
import { DEFAULT_SESSION_FILTERS } from "../client/session-view";
import {
  reduceToolbarVisibility,
  toolbarLayout,
} from "../client/toolbar-visibility";
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

function collectText(node: unknown): string {
  if (node === null || node === undefined || node === false) {
    return "";
  }
  if (typeof node === "string" || typeof node === "number") {
    return String(node);
  }
  if (Array.isArray(node)) {
    return node.map(collectText).join("");
  }
  if (typeof node === "object" && "props" in node) {
    return collectText((node as ElementLike).props.children);
  }
  return "";
}

function styleOf(element: ElementLike): Record<string, unknown> {
  return element.props.style as Record<string, unknown>;
}

function findAll(
  node: unknown,
  predicate: (element: ElementLike) => boolean,
): ElementLike[] {
  if (node === null || node === undefined || typeof node !== "object") {
    return [];
  }
  if (Array.isArray(node)) {
    return node.flatMap((child) => findAll(child, predicate));
  }
  const element = node as ElementLike;
  return [
    ...(predicate(element) ? [element] : []),
    ...findAll(element.props.children, predicate),
  ];
}

function findByTestID(node: unknown, testID: string): ElementLike {
  const found = findAll(node, (element) => element.props.testID === testID);
  expect(found).toHaveLength(1);
  return found[0]!;
}

describe("Kontor filters affordance", () => {
  beforeEach(() => {
    clearSessionViews();
    vi.restoreAllMocks();
  });

  it("renders a collapsed, labelled Filters button with a chevron and touch target", () => {
    const element = FiltersButton({
      activeCount: 0,
      expanded: false,
      colors,
      compact: true,
      onPress: () => undefined,
    });

    expect(element.props.testID).toBe(FILTERS_TOGGLE_TEST_ID);
    expect(element.props.accessibilityRole).toBe("button");
    expect(element.props.accessibilityState).toEqual({ expanded: false });
    expect(element.props.accessibilityLabel).toBe("Filters");
    expect(styleOf(element).minHeight).toBeGreaterThanOrEqual(
      FILTERS_TOUCH_TARGET,
    );
    expect(styleOf(element).minWidth).toBeGreaterThanOrEqual(FILTERS_TOUCH_TARGET);
    expect(collectText(element)).toContain("Filters");
    expect(collectText(element)).toContain(filtersDisclosureGlyph(false));
    expect(collectText(element)).not.toContain("Clear filters");
  });

  it("shows the active count and expanded state on the Filters button", () => {
    const element = FiltersButton({
      activeCount: 2,
      expanded: true,
      colors,
      compact: true,
      onPress: () => undefined,
    });

    expect(element.props.accessibilityState).toEqual({ expanded: true });
    expect(element.props.accessibilityLabel).toBe(
      filtersAccessibilityLabel(2),
    );
    expect(element.props.accessibilityLabel).toContain("2 active");
    expect(collectText(element)).toContain("Filters");
    expect(collectText(element)).toContain("2");
    expect(collectText(element)).toContain(filtersDisclosureGlyph(true));
  });

  it("keeps Clear filters separate and gives it its own touch target", () => {
    const element = ClearFiltersButton({
      colors,
      compact: true,
      onPress: () => undefined,
    });

    expect(element.props.testID).toBe(CLEAR_FILTERS_TEST_ID);
    expect(element.props.testID).not.toBe(FILTERS_TOGGLE_TEST_ID);
    expect(element.props.accessibilityRole).toBe("button");
    expect(element.props.accessibilityLabel).toBe("Clear filters");
    expect(styleOf(element).minHeight).toBeGreaterThanOrEqual(
      FILTERS_TOUCH_TARGET,
    );
    expect(styleOf(element).minWidth).toBeGreaterThanOrEqual(FILTERS_TOUCH_TARGET);
    expect(collectText(element)).toContain("Clear filters");
  });

  it("keeps one slim host/state row with Search and Filters actions", () => {
    const opened: string[] = [];
    const element = SlimToolbar({
      hostLabel: "srv_tWq3jTzpOlc9",
      statusLabel: "Stale",
      statusColor: colors.statusWarning,
      activeCount: 2,
      colors,
      compact: true,
      onSearch: () => opened.push("search"),
      onFilters: () => opened.push("filters"),
    });

    const text = collectText(element);
    expect(element.props.testID).toBe("sessions-slim-toolbar");
    expect(text).toContain("srv_tWq3jTzpOlc9");
    expect(text).toContain("Stale");
    expect(text).not.toContain("Kontor");

    const search = findByTestID(element, "sessions-slim-search");
    const filters = findByTestID(element, "sessions-slim-filters");
    expect(search.props.label).toBe("Search");
    expect(search.props.accessibilityLabel).toBe("Search sessions");
    expect(filters.props.label).toBe("Filters");
    expect(filters.props.count).toBe(2);
    expect(filters.props.accessibilityLabel).toBe(filtersAccessibilityLabel(2));
    expect(filters.props.accessibilityLabel).toContain("2 active");

    (search.props.onPress as () => void)();
    (filters.props.onPress as () => void)();
    expect(opened).toEqual(["search", "filters"]);
  });

  it("gives both slim actions a 44x44 usable target", () => {
    const search = SlimActionButton({
      testID: "sessions-slim-search",
      label: "Search",
      accessibilityLabel: "Search sessions",
      accessibilityHint: "Shows the search and filter controls",
      colors,
      onPress: () => undefined,
    });
    const filters = SlimActionButton({
      testID: "sessions-slim-filters",
      label: "Filters",
      accessibilityLabel: filtersAccessibilityLabel(1),
      accessibilityHint: "Shows the filter options",
      count: 1,
      colors,
      onPress: () => undefined,
    });

    for (const element of [search, filters]) {
      expect(element.props.accessibilityRole).toBe("button");
      expect(styleOf(element).minHeight).toBeGreaterThanOrEqual(
        FILTERS_TOUCH_TARGET,
      );
      expect(styleOf(element).minWidth).toBeGreaterThanOrEqual(
        FILTERS_TOUCH_TARGET,
      );
    }
    expect(collectText(filters)).toContain("1");
  });

  it("keeps collapsed default and saved advanced expansion independent", () => {
    expect(DEFAULT_SESSION_FILTERS.advancedOpen).toBe(false);
    expect(readSessionView("host-a").advancedOpen).toBe(false);

    patchSessionView("host-a", { advancedOpen: true });

    expect(readSessionView("host-a").advancedOpen).toBe(true);
  });

  it("leaves the saved view untouched when only pure toolbar helpers run", () => {
    const saved = patchSessionView("host-a", {
      query: "ASMA-8540",
      status: "running",
      attentionOnly: true,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });
    const before = { ...saved };

    reduceToolbarVisibility("expanded", {
      offset: 400,
      delta: 40,
      contentHeight: 2000,
      viewportHeight: 600,
      headerHeight: 200,
      pinned: false,
    });
    toolbarLayout({ collapsed: true, compact: true });

    expect(readSessionView("host-a")).toEqual(before);
  });

  it("uses the warning indicator for a needs-input session and lifecycle colour otherwise", () => {
    const waiting = sessionEntry("waiting", {
      status: "running",
      pendingPermissionCount: 1,
    });
    const running = sessionEntry("running", { status: "running" });
    const failed = sessionEntry("failed", {
      status: "idle",
      requiresAttention: false,
      attentionReason: "error",
    });

    expect(indicatorColor(waiting, colors)).toBe(colors.statusWarning);
    expect(indicatorColor(running, colors)).toBe(colors.accent);
    expect(indicatorColor(failed, colors)).toBe(colors.statusDanger);
  });
});
