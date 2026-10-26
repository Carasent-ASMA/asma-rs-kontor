import { afterEach, describe, expect, it, vi } from "vitest";
import { deriveSessionProjection } from "../shared/session-lifecycle";
import type { DirectoryEntry } from "../client/session-directory";
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { CompactRetirementNotice, compactRetirementNotice, sessionRowDetails } from "../client/session-compact-content";
import { sessionAvailabilityLabel } from "../client/session-view";

const colors = { foreground: "#111", foregroundMuted: "#666", statusWarning: "#850" } as Parameters<typeof CompactRetirementNotice>[0]["colors"];
const entry = (patch: Partial<DirectoryEntry> = {}): DirectoryEntry => ({ id: "fixture", title: "SWE • ASMA-1 • Review the summary", status: "idle", requiresAttention: false,
  attentionReason: null, attentionTimestamp: null, pendingPermissionCount: 0, needsInput: false, archivedAt: null, createdAt: "2026-10-25T09:00:00Z", updatedAt: "2026-10-25T10:00:00Z",
  cwd: "/fixture", workspaceId: "workspace", provider: "codex", model: "gpt-6.1-sol", parentAgentId: null, labels: {}, projectKey: "project", projectName: "Example", workspaceName: "Review", searchText: "review", ...patch });
const projection = () => deriveSessionProjection({ hostId: "fixture-host", coverage: { archiveInventory: "complete", observedAt: "2026-10-25T10:00:00Z", detail: "fixture" },
  session: { id: "fixture", title: "Review", status: "idle", archived: false, archivedAt: null, activeTurn: false, pendingInteraction: false, workspaceId: "workspace", projectKey: "project", provider: "codex", model: null, updatedAt: "2026-10-25T10:00:00Z", observedAt: "2026-10-25T10:00:00Z" },
  intent: { state: "none", reviewId: null, sessionId: "fixture", observedAt: null, detail: "None" }, release: { state: "unavailable", reason: "fixture" }, now: "2026-10-25T10:00:00Z" });
afterEach(() => vi.useRealTimers());

describe("compact session row content", () => {
  // The row note carries sessionAvailabilityLabel; the warning needs current exact-ID evidence.
  it("shows a missing-record warning only for current exact-ID evidence", () => {
    vi.useFakeTimers(); vi.setSystemTime(Date.parse("2026-10-25T10:00:10Z"));
    const value = entry({ requiresAttention: true, availability: { agentId: "fixture", revision: entry().updatedAt, state: "missing", reason: "Exact read", checkedAt: "2026-10-25T10:00:00Z" } });
    expect(sessionAvailabilityLabel(value)).toContain("Native record absent");
    expect(sessionAvailabilityLabel({ ...value, id: "different" })).not.toContain("Native record absent");
    vi.advanceTimersByTime(60_000);
    expect(sessionAvailabilityLabel(value)).not.toContain("Native record absent");
  });
  it("moves routine retain/ready/unknown retirement prose behind Details", () => {
    const value = projection();
    for (const disposition of ["retain", "ready", "unknown"] as const) {
      const current = { ...value, retirement: { ...value.retirement, disposition, attention: false }, conflicts: [] };
      expect(compactRetirementNotice(current)).toBeNull(); expect(CompactRetirementNotice({ projection: current, colors })).toBeNull();
    }
    expect(compactRetirementNotice({ ...value, retirement: { ...value.retirement, attention: true } })).toBe("Needs review");
    expect(compactRetirementNotice({ ...value, conflicts: [{ kind: "archived-with-active-execution", detail: "fixture", sources: [] }] })).toBe("Archived status conflict");
  });
  it("keeps hidden status, membership, earlier-error and availability facts in Details", () => {
    const value = entry({ status: "idle", requiresAttention: true, attentionReason: "error" });
    expect(sessionRowDetails(value)).toContainEqual({ label: "Availability", value: "Availability unknown" });
    expect(sessionRowDetails(value)).toContainEqual({ label: "Earlier error", value: "Earlier error · now idle" });
    expect(sessionRowDetails(value)).toContainEqual({ label: "Archive membership", value: "Not archived" });
    expect(sessionRowDetails({ ...value, archivedAt: "2026-10-25T10:00:00Z" })).toContainEqual({ label: "Archive membership", value: "Archived · 2026-10-25T10:00:00Z" });
  });
});
