import { beforeEach, describe, expect, it } from "vitest";
import {
  clearSessionViews,
  isResultMode,
  sessionPresentation,
  clearSessionScopePatch,
  patchSessionView,
  readSessionView,
  resetSessionView,
  sessionViewTransition,
  clearSessionFilterPatch,
  sessionFilterCount,
  resetSessionViewPatch,
  readWorkingAs,
  writeWorkingAs,
  configureWorkingAsStorage,
  readRememberedWorkingAs,
} from "../client/session-view-store";
import type { ProfileStorage } from "../client/viewer-profile";
import { DEFAULT_SESSION_FILTERS } from "../client/session-view";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";

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

  it("resolves legacy Flat and keeps the last grouping independently of saved layout", () => {
    const saved = { ...DEFAULT_ORGANIZATION_SETTINGS, layout: "flat" as const, grouping: "role" as const };
    expect(sessionPresentation({}, saved)).toEqual({ layout: "flat", grouping: "role" });
    expect(sessionPresentation({ grouping: "flat" })).toEqual({ layout: "flat", grouping: "hierarchy" });
    expect(sessionPresentation({ layout: "grouped" }, { ...saved, grouping: "flat" })).toEqual({ layout: "grouped", grouping: "hierarchy" });
    expect(sessionPresentation({ layout: "flat", grouping: "role" })).toEqual({ layout: "flat", grouping: "role" });
    expect(sessionPresentation({ grouping: "placement" }, saved)).toEqual({ layout: "grouped", grouping: "placement" });
  });

  it("clears only scope, preserving layout, sort, search, task and assignment; isolates hosts", () => {
    patchSessionView("host-a", { layout: "flat", grouping: "role", epic: { kind: "unknown" }, shortcut: "desk-project:prj_a", streams: [],
      sort: "updated", sortDirection: "asc", query: "keep", status: "idle", assignment: "me" });
    patchSessionView("host-a", clearSessionScopePatch());
    expect(readSessionView("host-a")).toMatchObject({ shortcut: null, streams: null, epic: null, layout: "flat", grouping: "role", sort: "updated", sortDirection: "asc", query: "keep", status: "idle", assignment: "me" });
    expect(readSessionView("host-b").layout).toBeUndefined();
    const reset = { ...readSessionView("host-a"), ...resetSessionViewPatch(readSessionView("host-a")) };
    expect(sessionPresentation(reset, { ...DEFAULT_ORGANIZATION_SETTINGS, layout: "grouped", grouping: "role" })).toEqual({ layout: "grouped", grouping: "role" });
  });
});

describe("filtered results and the tree position", () => {
  beforeEach(() => {
    // These cases start from the unscoped inventory; the My work default is covered below.
    clearSessionViews({ work: "all" });
  });

  const task = { jira: "ASMA-7", kontorId: null, label: "ASMA-7" };

  it.each([
    { query: "missing" }, { status: "favorites" as const }, { task },
    { shortcut: "desk-project:example" }, { streams: [] }, { epic: { kind: "unknown" as const } }, { membership: "archived" as const },
  ])("counts and clears a single visible or advanced-only criterion %j", (patch) => {
    const view = { ...readSessionView("host-a"), status: "all" as const, grouping: "flat" as const, advancedOpen: true, ...patch };
    expect(sessionFilterCount(view)).toBe(1);
    const cleared = { ...view, ...sessionViewTransition(view, clearSessionFilterPatch(view)) };
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(cleared).toMatchObject({ grouping: "flat", advancedOpen: true });
  });

  it("clears every filter idempotently while keeping Flat and its remembered tree anchor", () => {
    const current = { ...readSessionView("host-a"), query: "missing", status: "favorites" as const, task, shortcut: "Adam", streams: [], includeArchived: true, membership: "archived" as const,
      grouping: "flat" as const, advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 40, treeOffset: 600 };
    expect(sessionFilterCount(current)).toBe(6);
    const cleared = { ...current, ...sessionViewTransition(current, clearSessionFilterPatch(current)) };
    expect(cleared).toMatchObject({ query: "", status: "all", task: null, shortcut: null, streams: null, includeArchived: false, membership: "sessions",
      grouping: "flat", advancedOpen: true, toolbarOpen: false, arranging: true, scrollOffset: 0, treeOffset: 600, peek: null });
    expect(sessionFilterCount(cleared)).toBe(0);
    const twice = { ...cleared, ...sessionViewTransition(cleared, clearSessionFilterPatch(cleared)) };
    expect(sessionFilterCount(twice)).toBe(0);
    expect(twice).toEqual(cleared);
  });

  it("keeps layout independent from query, state, task and scope", () => {
    const base = readSessionView("host-a");
    expect(isResultMode(base)).toBe(false);
    expect(isResultMode({ ...base, query: "  " })).toBe(false);
    patchSessionView("host-a", { shortcut: "Adam" });
    expect(isResultMode(readSessionView("host-a"))).toBe(false);
    expect(isResultMode({ ...base, query: "qa" })).toBe(false);
    expect(isResultMode({ ...base, status: "idle" })).toBe(false);
    expect(isResultMode({ ...base, task })).toBe(false);
    expect(isResultMode({ ...base, layout: "flat", query: "qa", task })).toBe(true);
  });

  it("remembers the tree position on entering results and restores it on leaving", () => {
    patchSessionView("host-a", { scrollOffset: 640 });
    const enter = sessionViewTransition(readSessionView("host-a"), { layout: "flat", query: "qa" }, 600);
    expect(enter).toEqual({ layout: "flat", query: "qa", treeOffset: 600, peek: null, scrollOffset: 0 });
    patchSessionView("host-a", { ...enter, scrollOffset: 90 });
    // Narrowing within results keeps the remembered tree position.
    const narrow = sessionViewTransition(readSessionView("host-a"), { status: "running" });
    expect(narrow).toEqual({ status: "running", peek: null });
    patchSessionView("host-a", narrow);
    const leave = sessionViewTransition(readSessionView("host-a"), { layout: "grouped", query: "", status: "all" });
    expect(leave).toEqual({ layout: "grouped", query: "", status: "all", treeOffset: undefined, peek: null, scrollOffset: 600 });
  });

  it("ends a tree peek when the results change, but not on a scroll", () => {
    patchSessionView("host-a", { query: "qa", treeOffset: 0, peek: { sessionId: "s", resultsOffset: 40 } });
    const current = readSessionView("host-a");
    expect(sessionViewTransition(current, { scrollOffset: 300 })).toEqual({ scrollOffset: 300 });
    expect(sessionViewTransition(current, { query: "qa swe" })).toEqual({ query: "qa swe", peek: null });
    expect(sessionViewTransition(current, { task })).toEqual({ task, peek: null });
  });
});

describe("work scope, order and reset", () => {
  beforeEach(() => {
    clearSessionViews();
  });

  it("starts in My work and Current order, counting native Current visibility separately", () => {
    const view = readSessionView("host-a");
    expect(view.work).toBeUndefined();
    expect(view.sort).toBeUndefined();
    expect(sessionFilterCount(view)).toBe(2);
    expect(sessionFilterCount({ ...view, status: "all" })).toBe(1);
    expect(sessionFilterCount({ ...view, status: "all", work: "all", sort: "title" })).toBe(0);
  });

  it("counts an assignment filter as its own choice", () => {
    const all = { ...readSessionView("host-a"), status: "all" as const };
    expect(sessionFilterCount({ ...all, work: "all", assignment: "me" })).toBe(1);
    expect(sessionFilterCount({ ...all, assignment: "person:account-b" })).toBe(2);
    expect(sessionFilterCount({ ...all, status: "current", work: "all", assignment: "me" })).toBe(2);
  });

  it("clears archive membership and resets to ordinary Current without changing viewing identity or presentation on Clear", () => {
    writeWorkingAs("host-a", "tracker.example", "account-a");
    const view = { ...readSessionView("host-a"), status: "all" as const, work: "all" as const, membership: "archived" as const,
      includeArchived: true, assignment: "person:account-b" as const, sort: "title" as const, grouping: "role" as const };
    expect(sessionFilterCount(view)).toBe(2);
    const cleared = { ...view, ...clearSessionFilterPatch(view) };
    expect(cleared).toMatchObject({ membership: "sessions", status: "all", work: "all", assignment: "any", sort: "title", grouping: "role" });
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(sessionFilterCount({ ...cleared, includeArchived: true })).toBe(0);
    expect({ ...view, ...resetSessionViewPatch(view) }).toMatchObject({ membership: "sessions", status: "current", work: "mine", assignment: "any", sort: undefined, grouping: undefined });
    expect(readWorkingAs("host-a", "tracker.example")).toBe("account-a");
  });

  it("clears to All work and Any assignment and keeps sort, grouping and Working as", () => {
    writeWorkingAs("host-a", "tracker.example", "account-a");
    const view = { ...readSessionView("host-a"), query: "qa", assignment: "me" as const, sort: "created" as const, sortDirection: "asc" as const, grouping: "role" as const };
    const cleared = { ...view, ...sessionViewTransition(view, clearSessionFilterPatch(view)) };
    expect(cleared).toMatchObject({ work: "all", assignment: "any", query: "", sort: "created", sortDirection: "asc", grouping: "role" });
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(readWorkingAs("host-a", "tracker.example")).toBe("account-a");
  });

  it("resets to what a new app session shows without touching other views", () => {
    patchSessionView("host-b", { work: "all", sort: "title" });
    writeWorkingAs("host-a", "tracker.example", "account-a");
    const view = { ...readSessionView("host-a"), work: "all" as const, assignment: "person:account-b" as const, sort: "updated" as const, sortDirection: "asc" as const,
      grouping: "flat" as const, streams: ["kontor"], arranging: true, peek: { sessionId: "s", resultsOffset: 4 } };
    const reset = { ...view, ...sessionViewTransition(view, resetSessionViewPatch(view)) };
    expect(reset).toMatchObject({ work: "mine", assignment: "any", sort: undefined, sortDirection: undefined, grouping: undefined, streams: null, arranging: false, peek: null });
    expect(readWorkingAs("host-a", "tracker.example")).toBe("account-a");
    expect(readSessionView("host-b")).toMatchObject({ work: "all", sort: "title" });
  });

  it("ends a peek when the work scope or the order changes", () => {
    patchSessionView("host-a", { query: "qa", treeOffset: 0, peek: { sessionId: "s", resultsOffset: 40 } });
    const current = readSessionView("host-a");
    expect(sessionViewTransition(current, { work: "all" })).toEqual({ work: "all", peek: null });
    expect(sessionViewTransition(current, { sort: "title" })).toEqual({ sort: "title", peek: null });
    expect(sessionViewTransition(current, { sortDirection: "asc" })).toEqual({ sortDirection: "asc", peek: null });
  });

  it("keeps Working as per host and tracker realm, and a change ends this host's Assigned to me peek", () => {
    patchSessionView("host-a", { query: "qa", assignment: "me", peek: { sessionId: "s", resultsOffset: 40 } });
    writeWorkingAs("host-a", "tracker.example", "account-a");
    expect(readWorkingAs("host-a", "tracker.example")).toBe("account-a");
    expect(readWorkingAs("host-a", "other.example")).toBeNull();
    expect(readWorkingAs("host-b", "tracker.example")).toBeNull();
    expect(readWorkingAs("host-a", null)).toBeNull();
    expect(readSessionView("host-a").peek).toBeNull();
    writeWorkingAs("host-a", "tracker.example", null);
    expect(readWorkingAs("host-a", "tracker.example")).toBeNull();
    writeWorkingAs("host-a", "tracker.example", "account-a");
    clearSessionViews();
    expect(readWorkingAs("host-a", "tracker.example")).toBeNull();
  });

  it("ends only the peeks the changed profile affects: this host, filtered to Assigned to me or Unknown assignment", () => {
    const scope = (host: string, epic: string) => JSON.stringify([host, "sessions", null, epic]);
    const peek = (resultsOffset: number) => ({ query: "qa", peek: { sessionId: `s-${resultsOffset}`, resultsOffset } });
    patchSessionView(scope("host-a", "ASMA-1"), { ...peek(1), assignment: "me" });
    patchSessionView(scope("host-a", "ASMA-2"), { ...peek(2), assignment: "unknown" });
    patchSessionView(scope("host-a", "ASMA-3"), { ...peek(3), work: "all" });
    patchSessionView(scope("host-a", "ASMA-4"), { ...peek(4), assignment: "person:account-b" });
    patchSessionView(scope("host-b", "ASMA-1"), { ...peek(700), assignment: "me" });
    patchSessionView("host-b", { ...peek(5), assignment: "me" });
    patchSessionView(scope("host-a", "ASMA-5"), { ...peek(6), work: "mine" });
    writeWorkingAs("host-a", "tracker.example", "account-a");
    const peeks = () => [scope("host-a", "ASMA-1"), scope("host-a", "ASMA-2"), scope("host-a", "ASMA-3"), scope("host-a", "ASMA-4"), scope("host-b", "ASMA-1"), "host-b", scope("host-a", "ASMA-5")]
      .map((key) => readSessionView(key).peek?.resultsOffset ?? null);
    // My work never depends on Working as, so its peek stays.
    expect(peeks()).toEqual([null, null, 3, 4, 700, 5, 6]);
    // Choosing the same person again changes nothing.
    patchSessionView(scope("host-a", "ASMA-1"), peek(1));
    writeWorkingAs("host-a", "tracker.example", "account-a");
    expect(readSessionView(scope("host-a", "ASMA-1")).peek?.resultsOffset).toBe(1);
  });

  it("reads Working as from the configured store once, writes it through, and forgets only on request", () => {
    const stored = new Map<string, unknown>();
    const reads: string[] = [];
    const storage: ProfileStorage = { read: (key) => { reads.push(key); return stored.get(key) ?? null; }, write: (key, value) => { if (value === null) stored.delete(key); else stored.set(key, value); } };
    configureWorkingAsStorage(storage);
    expect(readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: null, persistence: "device" });
    writeWorkingAs("host-a", "tracker.example", "account-a");
    writeWorkingAs("host-a", "other.example", "account-b");
    expect([...stored.values()]).toEqual(["account-a", "account-b"]);
    readWorkingAs("host-a", "tracker.example");
    expect(reads).toHaveLength(2);
    writeWorkingAs("host-a", "tracker.example", null);
    expect([...stored.values()]).toEqual(["account-b"]);
    expect(readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: null, persistence: "device" });
    configureWorkingAsStorage(null);
  });

  it("keeps a choice for this app session when the store fails, and retries the same choice", () => {
    let failing = true;
    const stored = new Map<string, string | null>();
    configureWorkingAsStorage({ read: () => null, write: (key, value) => { if (failing) throw new Error("quota"); stored.set(key, value); } });
    writeWorkingAs("host-a", "tracker.example", "account-a");
    expect(readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: "account-a", persistence: "failed" });
    failing = false;
    writeWorkingAs("host-a", "tracker.example", "account-a");
    expect(readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: "account-a", persistence: "device" });
    expect(stored.size).toBe(1);
    configureWorkingAsStorage(null);
  });
});
