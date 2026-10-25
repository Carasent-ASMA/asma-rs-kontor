import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => vi.fn() }));
import { bindAvailability, withLocalDispositions, withSessionAvailability } from "../client/use-session-availability";
import { hasSessionAttention } from "../client/session-directory";
import { sessionAvailabilityLabel } from "../client/session-view";
import { sessionEntry } from "./session-fixtures";
import { AVAILABILITY_TTL_MS, freshAvailability, type SessionAvailability } from "../shared/session-lifecycle";
const entry = () => sessionEntry("native-a", { status: "closed", requiresAttention: true, attentionReason: "finished" });
const evidence = (state: SessionAvailability["state"]): SessionAvailability => ({ agentId: "native-a", state, reason: "fixture exact read", checkedAt: new Date().toISOString(), revision: entry().updatedAt });
afterEach(() => vi.useRealTimers());
describe("P1 availability cache truth", () => {
  it("keeps older native records and their unread finished Attention", () => {
    const [row] = withSessionAvailability([entry()], { "native-a": evidence("readable") });
    expect(hasSessionAttention(row!)).toBe(true); expect(sessionAvailabilityLabel(row!)).toContain("Unread result"); expect(sessionAvailabilityLabel(row!)).toContain("provider history unverified");
  });
  it("does not apply evidence for another exact ID even if the map key matches", () => {
    const bad = { ...evidence("missing"), agentId: "other" };
    expect(bindAvailability(entry(), bad).state).toBe("unknown");
    expect(hasSessionAttention(withSessionAvailability([entry()], { "native-a": bad })[0]!)).toBe(true);
  });
  it("expires missing evidence without requiring a changed native timestamp", () => {
    vi.useFakeTimers(); const missing = evidence("missing");
    expect(hasSessionAttention(withSessionAvailability([entry()], { "native-a": missing })[0]!)).toBe(false);
    vi.advanceTimersByTime(AVAILABILITY_TTL_MS);
    const row = withSessionAvailability([entry()], { "native-a": missing })[0]!;
    expect(freshAvailability(row)).toBeUndefined(); expect(hasSessionAttention(row)).toBe(true);
  });
  it("lets a fresh readable check restore the count and never hides an unknown reply", () => {
    const missing = withSessionAvailability([entry()], { "native-a": evidence("missing") });
    expect(missing.filter(hasSessionAttention)).toHaveLength(0);
    for (const state of ["readable", "unknown"] as const) expect(withSessionAvailability([entry()], { "native-a": evidence(state) }).filter(hasSessionAttention)).toHaveLength(1);
  });
  it("reconciles native archive metadata while a directory snapshot remains stale", () => {
    const rows = withSessionAvailability([entry()], { "native-a": { ...evidence("readable"), archivedAt: "2026-10-21" } });
    expect(rows[0]!.archivedAt).toBe("2026-10-21"); expect(rows.filter(hasSessionAttention)).toHaveLength(0);
  });
  it("invalidates old-revision and future-dated claims", () => {
    for (const patch of [{ revision: "old" }, { checkedAt: new Date(Date.now() + 60_000).toISOString() }]) expect(hasSessionAttention(withSessionAvailability([entry()], { "native-a": { ...evidence("missing"), ...patch } })[0]!)).toBe(true);
  });
  it("does not relabel an older archive reply as the newer native revision", () => {
    const original = entry(), newer = { ...original, updatedAt: "2026-10-25T12:02:00Z", archivedAt: null };
    const reply = { ...evidence("readable"), archivedAt: "2026-10-25T12:01:00Z" };
    const bound = bindAvailability(newer, reply);
    expect(withLocalDispositions([newer], { [newer.id]: bound })[0]!.archivedAt).toBeNull();
    expect(bound.revision).toBe(reply.revision);
  });
});
