import { describe, expect, it } from "vitest";
import { SESSION_STATUSES, sortDirectoryEntries } from "../client/session-directory";
import { buildOrganizationTree, sessionState } from "../client/session-organization";
import { inboxEntries, nativeSummary } from "../client/page-data";
import { chainLabel, chainState } from "../client/session-results";
import { archivedLabel, attentionLabel, earlierErrorLabel, matchesStatus, MORE_STATUS_FILTERS } from "../client/session-view";
import { sessionEntry } from "./session-fixtures";

describe("current runtime and retained error history", () => {
  it.each(SESSION_STATUSES)("keeps native %s separate from a retained error notice", (status) => {
    for (const requiresAttention of [false, true]) {
      const entry = sessionEntry("retained", { status, requiresAttention, attentionReason: "error" });
      const currentError = status === "error";
      expect(matchesStatus(entry, "error")).toBe(currentError);
      expect(matchesStatus(entry, "attention")).toBe(currentError && requiresAttention);
      expect(matchesStatus(entry, "earlier_error")).toBe(!currentError);
      expect(sessionState(entry)).toBe(currentError ? "failed" : status);
      expect(attentionLabel(entry)).toBe(currentError ? "Error" : null);
      expect(earlierErrorLabel(entry)).toBe(currentError ? null : `Earlier error · now ${status}`);
      expect(archivedLabel(entry)).toBeNull();
      expect(entry.status).toBe(status);
      expect(entry.requiresAttention).toBe(requiresAttention);
      expect(entry.attentionReason).toBe("error");
    }
  });

  it("keeps pending permissions actionable even while an earlier error notice is retained", () => {
    const entry = sessionEntry("permission", { status: "running", pendingPermissionCount: 1, requiresAttention: true, attentionReason: "error" });
    expect(sessionState(entry)).toBe("needs_input");
    expect(matchesStatus(entry, "needs_input")).toBe(true);
    expect(attentionLabel(entry)).toBe("Needs input");
    expect(inboxEntries([entry])).toEqual([entry]);
    expect(earlierErrorLabel(entry)).toBe("Earlier error · now running");
  });

  it("does not promote cleared finished notices to Attention and preserves unread finished signals", () => {
    const cleared = sessionEntry("cleared", { status: "idle", attentionReason: "finished", requiresAttention: false });
    const unread = sessionEntry("unread", { status: "closed", attentionReason: "finished", requiresAttention: true });
    expect(sessionState(cleared)).toBe("idle");
    expect(sessionState(unread)).toBe("attention");
    expect(inboxEntries([cleared, unread])).toEqual([unread]);
  });

  it("keeps the thirteen closed rows findable without counting them as current errors anywhere", () => {
    const earlier = Array.from({ length: 13 }, (_, n) => sessionEntry(`earlier-${n}`, {
      status: "closed", requiresAttention: true, attentionReason: "error", archivedAt: null,
    }));
    const current = sessionEntry("current", { status: "error", requiresAttention: true, attentionReason: "error" });
    const entries = [...earlier, current];
    expect(MORE_STATUS_FILTERS).toContain("earlier_error");
    expect(entries.filter((entry) => matchesStatus(entry, "all"))).toHaveLength(14);
    expect(entries.filter((entry) => matchesStatus(entry, "closed"))).toEqual(earlier);
    expect(entries.filter((entry) => matchesStatus(entry, "earlier_error"))).toEqual(earlier);
    expect(entries.filter((entry) => matchesStatus(entry, "error"))).toEqual([current]);
    expect(nativeSummary(entries)).toMatchObject({ total: 14, closed: 13, errors: 1, attention: 1 });
    expect(inboxEntries(entries)).toEqual([current]);
    expect(buildOrganizationTree({ entries })[0]!.aggregate).toMatchObject({ total: 14, failed: 1, attention: 0, closed: 13 });
    expect(chainState(earlier[0]!)).toBe("closed");
    expect(chainLabel(earlier[0]!)).toBe("Closed · Earlier error · now closed");
  });

  it("does not sort a recent retained error above a real error, running session or unread result", () => {
    const earlier = sessionEntry("history", { status: "closed", requiresAttention: true, attentionReason: "error", updatedAt: "2026-10-20T20:00:00Z" });
    const current = sessionEntry("current", { status: "error", updatedAt: "2026-10-18T10:00:00Z" });
    const running = sessionEntry("running", { status: "running" });
    const unread = sessionEntry("unread", { requiresAttention: true, attentionReason: "finished" });
    expect(sortDirectoryEntries([earlier, unread, running, current]).map((entry) => entry.id)).toEqual(["current", "running", "unread", "history"]);
  });

  it("does not infer archive or completion from a closed runtime or an earlier error", () => {
    const closed = sessionEntry("closed", { status: "closed", attentionReason: "error" });
    const archived = sessionEntry("archived", { status: "closed", attentionReason: "error", archivedAt: "2026-10-20T20:00:00Z" });
    expect(archivedLabel(closed)).toBeNull();
    expect(archivedLabel(archived)).toBe("Archived");
    expect(chainState(archived)).toBe("archived");
    expect(chainLabel(archived)).toBe("Archived · Earlier error · now closed");
    expect(matchesStatus(archived, "earlier_error")).toBe(true);
  });
});
