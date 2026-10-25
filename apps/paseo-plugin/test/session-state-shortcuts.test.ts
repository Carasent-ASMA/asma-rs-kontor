import { afterEach, describe, expect, it, vi } from "vitest";
import { providerBlockedView, type ProviderRefusalReads } from "../client/provider-blocked-filter";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";
import { startSwitchCounts, stateShortcutPatch, STATUS_SHORTCUTS, STATUS_SHORTCUT_SCOPE } from "../client/session-state-shortcuts";
import { DEFAULT_SESSION_FILTERS, filterDirectoryEntries } from "../client/session-view";
import type { SessionViewState } from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";

const VIEW: SessionViewState = { ...DEFAULT_SESSION_FILTERS, scrollOffset: 0 };
const entries = [
  sessionEntry("running", { status: "running", projectKey: "project-a" }),
  sessionEntry("question", { status: "running", needsInput: true, attentionReason: "permission", projectKey: "project-b" }),
  sessionEntry("finished", { requiresAttention: true, attentionReason: "finished" }),
  sessionEntry("blocked", { status: "error", needsInput: true }),
  sessionEntry("error", { status: "error" }),
  sessionEntry("archived", { status: "running", needsInput: true, archivedAt: "2026-10-18T10:00:00Z" }),
];
const state: DirectoryState = { entries, status: "ready", live: true, inventoryComplete: true, pages: 1, lastUpdatedAt: 0, includeArchived: true, error: null, staleReason: null };
const read = (entry: DirectoryEntry, blocked: boolean) => ({ availability: { agentId: entry.id, checkedAt: "2026-10-18T10:00:10.000Z", revision: entry.updatedAt,
  state: "readable" as const, reason: "Exact read" }, refusal: blocked ? { state: "current" as const, read: "fresh" as const } : null });
const reads: ProviderRefusalReads = new Map([["blocked", read(entries[3]!, true)], ["error", read(entries[4]!, false)]]);
afterEach(() => vi.restoreAllMocks());

describe("Start/switch status counts and destination identity", () => {
  it("uses the same ordinary all-project set and canonical predicate as each destination, with overlapping categories", () => {
    vi.spyOn(Date, "now").mockReturnValue(Date.parse("2026-10-18T10:00:30Z"));
    const counts = startSwitchCounts(state, "Live", reads);
    const expected = { running: ["running", "question"], needs_input: ["question", "blocked"], provider_blocked: ["blocked"], error: ["blocked", "error"] };
    for (const { status } of STATUS_SHORTCUTS) {
      const destination = { ...VIEW, ...stateShortcutPatch(VIEW, { status, scope: STATUS_SHORTCUT_SCOPE }) };
      const rows = status === "provider_blocked" ? providerBlockedView({ state, includeArchived: false, candidates: entries, reads, freshness: "Live", blocked: null }).sessions
        : filterDirectoryEntries(entries, destination);
      expect(rows.map(entry => entry.id)).toEqual(expected[status]);
      expect(counts[status].text).toBe(String(new Set(rows.map(entry => entry.id)).size));
    }
    expect(counts.needs_input.text).toBe("2"); // finished-only Attention is not a question.
    expect(counts.provider_blocked.text).toBe("1"); // An unblocked native error remains only Error.
  });

  it("does not infer provider blocks from error/attention or claim zero for unchecked errors", () => {
    const counts = startSwitchCounts(state, "Live");
    expect(counts.error.text).toBe("2");
    expect(counts.provider_blocked.text).toBe("…");
    expect(counts.provider_blocked.label).toContain("2 not checked");
  });

  it("counts every ID once, across projects, regardless of the Recent eight-row cap", () => {
    const many = Array.from({ length: 19 }, (_, index) => sessionEntry(`run-${index}`, { status: "running", projectKey: index % 2 ? "a" : "b" }));
    expect(startSwitchCounts({ ...state, entries: [...many, many[0]!] }, "Live").running.text).toBe("19");
  });

  it.each([
    { name: "unread", patch: { lastUpdatedAt: null }, freshness: "Reading" },
    { name: "partial", patch: { inventoryComplete: false }, freshness: "Live" },
    { name: "unknown completeness", patch: { inventoryComplete: undefined }, freshness: "Live" },
    { name: "offline", patch: { live: false }, freshness: "Host offline" },
    { name: "stale", patch: { status: "stale" as const, live: false }, freshness: "Stale" },
    { name: "reconnecting", patch: { live: false }, freshness: "Reconnecting" },
  ])("does not present verified zero or Live for $name", ({ patch, freshness }) => {
    const counts = startSwitchCounts({ ...state, entries: [], ...patch }, freshness);
    for (const { status } of STATUS_SHORTCUTS) expect(counts[status].text).toBe("…");
  });

  it("moves from unknown through retained counts to verified zero only after a complete live read", () => {
    expect(startSwitchCounts({ ...state, entries: [], lastUpdatedAt: null, status: "error" }, "Unavailable").running.text).toBe("—");
    expect(startSwitchCounts(state, "Host offline").running.text).toBe("…");
    expect(startSwitchCounts({ ...state, entries: [] }, "Live").running.text).toBe("0");
    expect(startSwitchCounts({ ...state, entries: [] }, "Live").provider_blocked.text).toBe("0");
  });

  it("replaces incompatible Recent/search/archive/refinements, retaining presentation preferences", () => {
    const view: SessionViewState = { ...VIEW, status: "recent", query: "old search", membership: "archived", includeArchived: true,
      task: { jira: "ASMA-1", kontorId: null, label: "Task" }, epic: { kind: "known", key: "ASMA-2" }, streams: ["other"], shortcut: "other",
      assignment: "me", layout: "grouped", grouping: "role", sort: "title", sortDirection: "asc", peek: { sessionId: "old", resultsOffset: 8 } };
    const patch = stateShortcutPatch(view, { status: "needs_input", scope: STATUS_SHORTCUT_SCOPE });
    expect({ ...view, ...patch }).toMatchObject({ status: "needs_input", query: "", membership: "sessions", includeArchived: false, work: "mine",
      task: null, epic: null, streams: null, shortcut: null, assignment: "any", peek: null, layout: "grouped", grouping: "role", sort: "title", sortDirection: "asc" });
    for (const key of ["layout", "grouping", "sort"]) expect(patch).not.toHaveProperty(key);
    expect(view.query).toBe("old search");
  });
});
