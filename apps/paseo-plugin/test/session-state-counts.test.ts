import { describe, expect, it } from "vitest";
import { compactSessionCount, sessionStateCounts } from "../client/session-state-counts";
import type { DirectoryState } from "../client/session-directory";
import { sessionEntry } from "./session-fixtures";

const entries = [
  sessionEntry("run", { status: "running" }), sessionEntry("idle"),
  sessionEntry("error", { status: "error", requiresAttention: true, attentionReason: "error" }),
  sessionEntry("input", { needsInput: true, requiresAttention: true, attentionReason: "permission" }),
  sessionEntry("start", { status: "initializing" }),
  sessionEntry("closed", { status: "closed" }),
  sessionEntry("earlier", { status: "idle", requiresAttention: true, attentionReason: "error" }),
];
const state: DirectoryState = { entries, status: "ready", live: true, inventoryComplete: true, pages: 1, lastUpdatedAt: 0, includeArchived: false, error: null, staleReason: null };
const input = { universe: entries, candidates: entries, favorites: ["idle"], refined: false, state, includeArchived: false, freshness: "Live", blocked: null };

describe("same-state counts", () => {
  it("counts every canonical state once per ID, independent of the selected state or layout", () => {
    const counts = sessionStateCounts({ ...input, universe: [...entries, entries[0]!] });
    const expected = { current: 5, all: 7, running: 1, idle: 3, error: 1, needs_input: 1, attention: 2, favorites: 1, initializing: 1, closed: 1, earlier_error: 1 };
    for (const [status, value] of Object.entries(expected)) expect(counts[status as keyof typeof counts].text).toBe(String(value));
  });

  it("keeps zero and same-set ratios when refinements are applied", () => {
    const counts = sessionStateCounts({ ...input, refined: true, candidates: entries.filter((entry) => entry.id === "run") });
    expect(counts.running).toMatchObject({ text: "1/1", label: expect.stringContaining("1 of 1 sessions") });
    expect(counts.idle.text).toBe("0/3");
    expect(counts.all.text).toBe("1/7");
    expect(sessionStateCounts({ ...input, refined: true }).all.text).toBe("7/7");
    expect(sessionStateCounts({ ...input, universe: [], candidates: [], refined: true }).all.text).toBe("0/0");
    expect(sessionStateCounts({ ...input, universe: [], candidates: [] }).all.text).toBe("0");
  });

  it.each([
    { state: { ...state, inventoryComplete: false }, label: "partial read" },
    { state: { ...state, inventoryComplete: undefined }, label: "partial read" },
    { state: { ...state, status: "loading" as const }, freshness: "Reading", label: "retained read" },
    { state, freshness: "Host offline", label: "retained read" },
    { state: { ...state, status: "stale" as const, live: false }, freshness: "Stale", label: "retained read" },
  ])("never presents a partial or retained page as a current total: $label", ({ label, ...patch }) => {
    const count = sessionStateCounts({ ...input, refined: true, candidates: [], ...patch }).running;
    expect(count.text).toBe("…"); expect(count.label).toContain("0 of 1 sessions"); expect(count.label).toContain(label);
    expect(count.numerator).toBeUndefined(); expect(count.denominator).toBeUndefined();
  });

  it("does not reuse another membership's read, failed first read, or a blocked assignment as zero", () => {
    expect(sessionStateCounts({ ...input, includeArchived: true }).all.label).toContain("Reading");
    expect(sessionStateCounts({ ...input, state: { ...state, status: "error", lastUpdatedAt: null } }).all.text).toBe("—");
    expect(sessionStateCounts({ ...input, blocked: "Assigned to me", candidates: [] }).all.label).toContain("not evaluated");
  });
});

it("fits compact counts into three characters without changing the exact accessible values", () => {
  const cases = [[999, "999"], [1_000, "1K"], [9_999, "10K"], [99_499, "99K"], [99_500, ".1M"], [999_999, "1M"], [1_000_000, "1M"], [99_500_000, ".1B"]] as const;
  for (const [value, text] of cases) { expect(compactSessionCount(value)).toBe(text); expect(text.length).toBeLessThanOrEqual(3); }
});
