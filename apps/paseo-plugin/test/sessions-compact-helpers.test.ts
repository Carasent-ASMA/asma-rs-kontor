import type { PluginTheme } from "@getpaseo/plugin";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({
  AccessibilityInfo: {
    addEventListener: () => ({ remove: () => undefined }),
    isReduceMotionEnabled: async () => false,
    isScreenReaderEnabled: async () => false,
  },
  ActivityIndicator: () => null,
  Animated: { Value: class { setValue() {} stopAnimation() {} }, View: () => null, timing: () => ({ start: () => undefined, stop: () => undefined }) },
  Pressable: () => null,
  Text: () => null,
  View: () => null,
}));

vi.mock("@getpaseo/plugin/client", () => ({
  getPaseoClient: () => { throw new Error("No host in the compact helper test"); },
  useHosts: () => [],
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: () => null,
  TextInput: () => null,
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

import {
  ActiveFilterSummary,
  activeFilterParts,
  COMPACT_COLLAPSE_DISTANCE,
  compactToolbarCompensation,
  compactToolbarOpen,
  compactToolbarScroll,
  toolbarOpenPatch,
} from "../client/sessions-screen";
import { patchSessionView, readSessionView, resetSessionView } from "../client/session-view-store";

const colors = { accent: "#05f", foreground: "#111", foregroundMuted: "#666", surface2: "#eee" } as unknown as PluginTheme["colors"];

/** A toolbar that cannot leave the viewport: the content is no taller than it. */
const SHORT = { toolbarHeight: 228, maxOffset: 48 };
/** A list long enough to scroll the 228px toolbar out of view. */
const LONG = { toolbarHeight: 228, maxOffset: 1400 };

describe("compact toolbar helpers", () => {
  it("closes a short list's toolbar after a deliberate downward drag from the lowest offset since opening", () => {
    const step = (anchor: number, offset: number, pinned = false, suppressed = false) => compactToolbarScroll({ anchor, offset, pinned, suppressed, ...SHORT });
    expect(step(300, 300 + COMPACT_COLLAPSE_DISTANCE - 1)).toEqual({ anchor: 300, collapse: false });
    expect(step(300, 300 + COMPACT_COLLAPSE_DISTANCE)).toEqual({ anchor: 300, collapse: true });
    // Scrolling up first lowers the anchor; the distance counts from there.
    expect(step(300, 120)).toEqual({ anchor: 120, collapse: false });
    expect(step(120, 140)).toEqual({ anchor: 120, collapse: false });
    // An iOS top pull is the top, not a negative anchor.
    expect(step(0, -40)).toEqual({ anchor: 0, collapse: false });
    // A rubber-band past the end of a short list is still a downward scroll.
    expect(step(0, 30)).toEqual({ anchor: 0, collapse: true });
    // Until the toolbar is measured, the small drag decides.
    expect(compactToolbarScroll({ anchor: 0, offset: 30, pinned: false, suppressed: false, toolbarHeight: 0, maxOffset: 1400 })).toEqual({ anchor: 0, collapse: true });
  });

  it("keeps a long list's toolbar open while any of it is in view and closes it once it has left", () => {
    const step = (anchor: number, offset: number) => compactToolbarScroll({ anchor, offset, pinned: false, suppressed: false, ...LONG });
    expect(step(0, 100)).toEqual({ anchor: 0, collapse: false });
    expect(step(0, LONG.toolbarHeight - 1)).toEqual({ anchor: 0, collapse: false });
    expect(step(0, LONG.toolbarHeight)).toEqual({ anchor: 0, collapse: true });
    // Out of view but only just re-anchored there: the hysteresis distance still applies.
    expect(step(400, 400 + COMPACT_COLLAPSE_DISTANCE - 1)).toEqual({ anchor: 400, collapse: false });
    expect(step(400, 400 + COMPACT_COLLAPSE_DISTANCE)).toEqual({ anchor: 400, collapse: true });
    // A scroll range that only just reaches the toolbar's height counts as long.
    expect(compactToolbarScroll({ anchor: 0, offset: 100, pinned: false, suppressed: false, toolbarHeight: 228, maxOffset: 228 })).toEqual({ anchor: 0, collapse: false });
  });

  it("compensates a close by the toolbar height and opens from the top", () => {
    expect(compactToolbarCompensation({ collapsed: true, offset: 260, toolbarHeight: 228 })).toBe(32);
    // Partly in view: the rows give back only the visible part.
    expect(compactToolbarCompensation({ collapsed: true, offset: 100, toolbarHeight: 228 })).toBe(0);
    expect(compactToolbarCompensation({ collapsed: true, offset: 0, toolbarHeight: 228 })).toBeNull();
    expect(compactToolbarCompensation({ collapsed: true, offset: 260, toolbarHeight: 0 })).toBeNull();
    expect(compactToolbarCompensation({ collapsed: false, offset: 900, toolbarHeight: 228 })).toBe(0);
    expect(compactToolbarCompensation({ collapsed: false, offset: 0, toolbarHeight: 228 })).toBeNull();
    expect(compactToolbarCompensation({ collapsed: true, offset: Number.NaN, toolbarHeight: 228 })).toBeNull();
  });

  it("re-anchors instead of closing while pinned, during programmatic scrolls and after a content shift", () => {
    expect(compactToolbarScroll({ anchor: 0, offset: 400, pinned: true, suppressed: false , ...LONG })).toEqual({ anchor: 400, collapse: false });
    expect(compactToolbarScroll({ anchor: 0, offset: 400, pinned: false, suppressed: true , ...LONG })).toEqual({ anchor: 400, collapse: false });
    expect(compactToolbarScroll({ anchor: Number.NaN, offset: 400, pinned: false, suppressed: false , ...LONG })).toEqual({ anchor: 400, collapse: false });
    expect(compactToolbarScroll({ anchor: 10, offset: Number.NaN, pinned: false, suppressed: false , ...LONG })).toEqual({ anchor: 10, collapse: false });
  });

  it("keeps the open choice in the local view store and treats absence as closed", () => {
    resetSessionView("helper-scope");
    expect(compactToolbarOpen(readSessionView("helper-scope"))).toBe(false);
    patchSessionView("helper-scope", toolbarOpenPatch(true));
    patchSessionView("helper-scope", { query: "kept" });
    expect(compactToolbarOpen(readSessionView("helper-scope"))).toBe(true);
    expect(readSessionView("helper-scope").query).toBe("kept");
  });

  it("names every active choice for the collapsed summary, and nothing at rest", () => {
    expect(activeFilterParts({ query: "  ", status: "all", shortcut: null, streams: null, includeArchived: false })).toEqual([]);
    expect(activeFilterParts({ query: " ASMA-1 ", status: "needs_input", shortcut: "Adam", streams: ["pr review"], includeArchived: true }))
      .toEqual(["“ASMA-1”", "Questions", "Adam desk", "1 workstream", "Archived history"]);
    expect(activeFilterParts({ query: "", status: "favorites", shortcut: "Desks", streams: ["a", "b"], includeArchived: false }))
      .toEqual(["★ Favorites", "Durable desks", "2 workstreams"]);
  });

  it("draws no summary row when nothing narrows the list", () => {
    expect(ActiveFilterSummary({ parts: [], scopeLabel: null, colors, onClear: () => undefined })).toBeNull();
    expect(ActiveFilterSummary({ parts: [], scopeLabel: "ASMA-1", colors, onClear: () => undefined })).not.toBeNull();
  });
});
