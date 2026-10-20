import { beforeEach, describe, expect, it } from "vitest";
import {
  clearSessionViews,
  isResultMode,
  patchSessionView,
  readSessionView,
  resetSessionView,
  sessionViewTransition,
  clearSessionFilterPatch,
  sessionFilterCount,
} from "../client/session-view-store";
import { DEFAULT_SESSION_FILTERS } from "../client/session-view";

describe("host-scoped session view state", () => {
  beforeEach(() => {
    clearSessionViews();
  });

  it("restores query, filters, expansion and scroll for the same host", () => {
    patchSessionView("host-a", {
      query: "ASMA-8540",
      status: "running",
      shortcut: "Adam",
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });

    expect(readSessionView("host-a")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      query: "ASMA-8540",
      status: "running",
      shortcut: "Adam",
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });
    expect(readSessionView("host-b")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    });
  });

  it("resets only the selected host", () => {
    patchSessionView("host-a", { query: "keep", scrollOffset: 80 });
    patchSessionView("host-b", { query: "other" });

    resetSessionView("host-a");

    expect(readSessionView("host-a")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    });
    expect(readSessionView("host-b").query).toBe("other");
  });
});

describe("filtered results and the tree position", () => {
  beforeEach(() => {
    clearSessionViews();
  });

  const task = { jira: "ASMA-7", kontorId: null, label: "ASMA-7" };

  it.each([
    { query: "missing" }, { status: "favorites" as const }, { task },
    { shortcut: "desk-project:example" }, { streams: [] }, { includeArchived: true },
  ])("counts and clears a single visible or advanced-only criterion %j", (patch) => {
    const view = { ...readSessionView("host-a"), grouping: "flat" as const, advancedOpen: true, ...patch };
    expect(sessionFilterCount(view)).toBe(1);
    const cleared = { ...view, ...sessionViewTransition(view, clearSessionFilterPatch(view)) };
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(cleared).toMatchObject({ grouping: "flat", advancedOpen: true });
  });

  it("clears every filter idempotently while keeping view preferences and the existing tree anchor", () => {
    const current = { ...readSessionView("host-a"), query: "missing", status: "favorites" as const, task, shortcut: "Adam", streams: [], includeArchived: true,
      grouping: "flat" as const, advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 40, treeOffset: 600 };
    expect(sessionFilterCount(current)).toBe(6);
    const cleared = { ...current, ...sessionViewTransition(current, clearSessionFilterPatch(current)) };
    expect(cleared).toMatchObject({ query: "", status: "all", task: null, shortcut: null, streams: null, includeArchived: false,
      grouping: "flat", advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 600, peek: null });
    expect(sessionFilterCount(cleared)).toBe(0);
    const twice = { ...cleared, ...sessionViewTransition(cleared, clearSessionFilterPatch(cleared)) };
    expect(sessionFilterCount(twice)).toBe(0);
    expect(twice).toEqual(cleared);
  });

  it("treats a query, a state filter or a task as results; a shortcut alone keeps the tree", () => {
    const base = readSessionView("host-a");
    expect(isResultMode(base)).toBe(false);
    expect(isResultMode({ ...base, query: "  " })).toBe(false);
    patchSessionView("host-a", { shortcut: "Adam" });
    expect(isResultMode(readSessionView("host-a"))).toBe(false);
    expect(isResultMode({ ...base, query: "qa" })).toBe(true);
    expect(isResultMode({ ...base, status: "idle" })).toBe(true);
    expect(isResultMode({ ...base, task })).toBe(true);
  });

  it("remembers the tree position on entering results and restores it on leaving", () => {
    patchSessionView("host-a", { scrollOffset: 640 });
    const enter = sessionViewTransition(readSessionView("host-a"), { query: "qa" }, 600);
    expect(enter).toEqual({ query: "qa", treeOffset: 600, peek: null, scrollOffset: 0 });
    patchSessionView("host-a", { ...enter, scrollOffset: 90 });
    // Narrowing within results keeps the remembered tree position.
    const narrow = sessionViewTransition(readSessionView("host-a"), { status: "running" });
    expect(narrow).toEqual({ status: "running", peek: null });
    patchSessionView("host-a", narrow);
    const leave = sessionViewTransition(readSessionView("host-a"), { query: "", status: "all" });
    expect(leave).toEqual({ query: "", status: "all", treeOffset: undefined, peek: null, scrollOffset: 600 });
  });

  it("ends a tree peek when the results change, but not on a scroll", () => {
    patchSessionView("host-a", { query: "qa", treeOffset: 0, peek: { sessionId: "s", resultsOffset: 40 } });
    const current = readSessionView("host-a");
    expect(sessionViewTransition(current, { scrollOffset: 300 })).toEqual({ scrollOffset: 300 });
    expect(sessionViewTransition(current, { query: "qa swe" })).toEqual({ query: "qa swe", peek: null });
    expect(sessionViewTransition(current, { task })).toEqual({ task, peek: null });
  });
});
