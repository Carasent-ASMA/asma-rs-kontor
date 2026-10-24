import { describe, expect, it } from "vitest";
import { providerBlock, providerBlockedView, type ProviderRefusalReads } from "../client/provider-blocked-filter";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";
import { matchesStatus } from "../client/session-view";
import { sessionEntry } from "./session-fixtures";

const now = Date.parse("2026-10-18T10:00:30.000Z");
const revision = "2026-10-18T10:00:00.000Z";
type Refusal = { state: "current" | "history"; read: "fresh" | "stale" | "unreadable" } | null;
const read = (agentId: string, refusal: Refusal, patch: { checkedAt?: string; revision?: string; state?: "readable" | "missing" | "unknown" } = {}) => ({
  availability: { agentId, state: "readable" as const, reason: "Exact read", checkedAt: "2026-10-18T10:00:10.000Z", revision, ...patch }, refusal });
const current = { state: "current", read: "fresh" } as const;
const error = (id: string, patch: Partial<DirectoryEntry> = {}) => ({ ...sessionEntry(id, { status: "error", requiresAttention: true, attentionReason: "error" }), ...patch });
const state: DirectoryState = { entries: [], status: "ready", live: true, inventoryComplete: true, pages: 1, lastUpdatedAt: 0, includeArchived: false, error: null, staleReason: null };
const settled = { state, includeArchived: false, freshness: "Live", blocked: null };
const view = (candidates: readonly DirectoryEntry[], reads: ProviderRefusalReads, at = now) => providerBlockedView({ ...settled, candidates, reads, now: at });

describe("provider-blocked predicate", () => {
  it("is blocked only on a fresh readable exact read of this ID and revision that saw a current refusal", () => {
    expect(providerBlock(error("a"), new Map([["a", read("a", current)]]), now)).toEqual({ state: "blocked" });
    expect(providerBlock(error("a"), new Map([["a", read("a", null)]]), now)).toEqual({ state: "none" });
  });

  it("keeps every other native error unknown, never blocked and never none", () => {
    const cases: [ProviderRefusalReads, string][] = [
      [new Map(), "not-read"],
      [new Map([["a", "failed"]]), "read-failed"],
      [new Map([["a", read("b", current)]]), "read-not-current"],
      [new Map([["a", read("a", null, { revision: "2026-10-18T10:00:20.000Z" })]]), "read-not-current"],
      [new Map([["a", read("a", current, { checkedAt: "2026-10-18T09:59:30.000Z" })]]), "read-not-current"],
      [new Map([["a", read("a", null, { checkedAt: "2026-10-18T10:00:40.000Z" })]]), "read-not-current"],
      [new Map([["a", read("a", null, { state: "missing" })]]), "not-readable"],
      [new Map([["a", read("a", null, { state: "unknown" })]]), "not-readable"],
      [new Map([["a", read("a", current, { state: "missing" })]]), "not-readable"],
      [new Map([["a", read("a", current, { state: "unknown" })]]), "not-readable"],
      [new Map([["a", read("a", { state: "history", read: "stale" })]]), "unresolved"],
      [new Map([["a", read("a", { state: "history", read: "unreadable" })]]), "unresolved"],
      [new Map([["a", read("a", { state: "current", read: "stale" })]]), "unresolved"],
    ];
    for (const [reads, reason] of cases) expect(providerBlock(error("a"), reads, now)).toEqual({ state: "unknown", reason });
  });

  it("leaves non-error and archived sessions outside the filter, even with a refusal read", () => {
    const reads = new Map(["idle", "closed", "run", "archived"].map((id) => [id, read(id, current)]));
    expect(providerBlock(sessionEntry("idle"), reads, now)).toBeNull();
    expect(providerBlock(sessionEntry("closed", { status: "closed" }), reads, now)).toBeNull();
    expect(providerBlock(sessionEntry("run", { status: "running" }), reads, now)).toBeNull();
    expect(providerBlock(error("archived", { archivedAt: revision }), reads, now)).toBeNull();
  });

  it("never lends one session's refusal to another on the same route", () => {
    const reads = new Map([["a", read("a", current)]]);
    expect(providerBlock(error("b"), reads, now)).toEqual({ state: "unknown", reason: "not-read" });
  });
});

describe("provider-blocked view", () => {
  const entries = [error("blocked"), sessionEntry("idle"), error("clean"), error("unread"), error("blocked")];
  const reads = new Map([["blocked", read("blocked", current)], ["clean", read("clean", null)]]);

  it("lists observed blocks only, in candidate order, and counts canonical IDs", () => {
    const result = view(entries, reads);
    expect(result.sessions.map((entry) => entry.id)).toEqual(["blocked", "blocked"]);
    expect(result).toMatchObject({ blocked: 1, unknown: 1, none: 1, empty: null });
    expect(result.count).toEqual({ text: "…", label: "1 provider blocked and 1 not checked of 3 error sessions. A session not checked is not counted as blocked." });
    expect(result.notice).toBe("1 error session not checked: provider state unknown, so not listed as blocked. Error shows it.");
  });

  it("states a number only when every error session has a current read", () => {
    const result = view(entries.filter((entry) => entry.id !== "unread"), reads);
    expect(result.count).toEqual({ text: "1", label: "1 provider blocked (observed) of 2 error sessions, each read by exact ID within the last minute" });
    expect(result.notice).toBeNull();
  });

  it("moves an expired read to not checked, never to free of refusals", () => {
    const later = view(entries, reads, now + 60_000);
    expect(later).toMatchObject({ sessions: [], blocked: 0, unknown: 3, none: 0 });
    expect(later.notice).toBe("3 error sessions not checked: provider state unknown, so not listed as blocked. Error shows them.");
    expect(later.empty).toBe("No provider block observed among the 3 error sessions here; 3 not checked. This is not a provider health check.");
  });

  it("counts a fresh missing or unknown read as not checked, never free of refusals", () => {
    for (const state of ["missing", "unknown"] as const) {
      const result = view([error("a")], new Map([["a", read("a", null, { state })]]));
      expect(result).toMatchObject({ sessions: [], blocked: 0, unknown: 1, none: 0, count: { text: "…" } });
      expect(result.notice).toBe("1 error session not checked: provider state unknown, so not listed as blocked. Error shows it.");
    }
  });

  it("is a subset of Error that Current never shows, and follows search and scope through its candidates", () => {
    const shown = view(entries, reads).sessions;
    expect(shown.length).toBeGreaterThan(0);
    expect(shown.every((entry) => matchesStatus(entry, "error") && !matchesStatus(entry, "current"))).toBe(true);
    expect(view(entries.filter((entry) => entry.id === "unread"), reads)).toMatchObject({ sessions: [], blocked: 0, unknown: 1 });
  });

  it("uses the shared count gate for a partial, retained or unevaluated read", () => {
    expect(providerBlockedView({ ...settled, state: { ...state, inventoryComplete: false }, candidates: entries, reads, now }).count)
      .toEqual({ text: "…", label: "1 provider blocked and 1 not checked of 3 error sessions in the partial read. Total updating; live." });
    expect(providerBlockedView({ ...settled, blocked: "Assigned to me", candidates: entries, reads, now }).count.label).toContain("not evaluated");
  });

  it("says why it is empty over candidates that are not", () => {
    expect(view([error("clean")], reads).empty).toBe("No provider block observed among the 1 error session here. This is not a provider health check.");
    expect(view([sessionEntry("idle")], reads).empty).toBe("No session here is in native error, and only a native error can carry a current provider refusal.");
    expect(view([error("old", { archivedAt: revision })], reads).empty).toBe("Archived sessions are history: a refusal on one is never current.");
    expect(view([], reads).empty).toBeNull();
  });

  it("never words absence or expiry as clear or available", () => {
    const words = [entries, [error("clean")], [sessionEntry("idle")], [error("old", { archivedAt: revision })]]
      .flatMap((candidates) => [now, now + 60_000].flatMap((at) => { const result = view(candidates, reads, at); return [result.count.label, result.notice ?? "", result.empty ?? ""]; }));
    for (const text of words) expect(text).not.toMatch(/\b(clear|available|healthy)\b/i);
  });
});
