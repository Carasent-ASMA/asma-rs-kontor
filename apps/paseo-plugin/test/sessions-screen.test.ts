import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({
  AccessibilityInfo: {
    addEventListener: () => ({ remove: () => undefined }),
    isScreenReaderEnabled: async () => false,
  },
  ActivityIndicator: () => null,
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
} from "../client/sessions-screen";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
} from "../client/session-view-store";
import { DEFAULT_SESSION_FILTERS } from "../client/session-view";
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

  it("keeps collapsed default and saved advanced expansion independent", () => {
    expect(DEFAULT_SESSION_FILTERS.advancedOpen).toBe(false);
    expect(readSessionView("host-a").advancedOpen).toBe(false);

    patchSessionView("host-a", { advancedOpen: true });

    expect(readSessionView("host-a").advancedOpen).toBe(true);
  });

  it("uses the warning indicator for a needs-input session and lifecycle colour otherwise", () => {
    const waiting = sessionEntry("waiting", {
      status: "running",
      pendingPermissionCount: 1,
    });
    const running = sessionEntry("running", { status: "running" });

    expect(indicatorColor(waiting, colors)).toBe(colors.statusWarning);
    expect(indicatorColor(running, colors)).toBe(colors.accent);
  });
});
