import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { deriveSessionProjection } from "../shared/session-lifecycle";
import type { DirectoryEntry } from "../client/session-directory";
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { CompactRetirementNotice, SessionCompactContent, compactRetirementNotice, sessionRowDetails } from "../client/session-compact-content";
import { SessionStateMarker } from "../client/session-state-marker";
import { sessionState } from "../client/session-organization";

const colors = { foreground: "#111", foregroundMuted: "#666", statusWarning: "#850" } as Parameters<typeof SessionCompactContent>[0]["colors"];
const entry = (patch: Partial<DirectoryEntry> = {}): DirectoryEntry => ({ id: "fixture", title: "SWE • ASMA-1 • Review the summary", status: "idle", requiresAttention: false,
  attentionReason: null, attentionTimestamp: null, pendingPermissionCount: 0, needsInput: false, archivedAt: null, createdAt: "2026-10-25T09:00:00Z", updatedAt: "2026-10-25T10:00:00Z",
  cwd: "/fixture", workspaceId: "workspace", provider: "codex", model: "gpt-6.1-sol", parentAgentId: null, labels: {}, projectKey: "project", projectName: "Example", workspaceName: "Review", searchText: "review", ...patch });
type Element = ReactElement<Record<string, unknown>>;
function nodes(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(nodes);
  return isValidElement<Record<string, unknown>>(node) ? [node, ...nodes(node.props.children as ReactNode)] : [];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join(" ");
  return isValidElement<Record<string, unknown>>(node) ? text(node.props.children as ReactNode) : "";
}
const projection = () => deriveSessionProjection({ hostId: "fixture-host", coverage: { archiveInventory: "complete", observedAt: "2026-10-25T10:00:00Z", detail: "fixture" },
  session: { id: "fixture", title: "Review", status: "idle", archived: false, archivedAt: null, activeTurn: false, pendingInteraction: false, workspaceId: "workspace", projectKey: "project", provider: "codex", model: null, updatedAt: "2026-10-25T10:00:00Z", observedAt: "2026-10-25T10:00:00Z" },
  intent: { state: "none", reviewId: null, sessionId: "fixture", observedAt: null, detail: "None" }, release: { state: "unavailable", reason: "fixture" }, now: "2026-10-25T10:00:00Z" });
afterEach(() => vi.useRealTimers());

describe("compact session row content", () => {
  it("keeps title/context apart from state, model and last activity without routine diagnostics", () => {
    const row = SessionCompactContent({ entry: entry({ requiresAttention: true, attentionReason: "finished" }), colors, context: "Example › Review", time: { compact: "2m ago", wide: "Updated 2 minutes ago", spoken: "Updated 2 minutes ago" } });
    expect(text(row)).toContain("Review the summary");
    expect(text(row)).toContain("Example › Review");
    expect(text(row)).toContain("sol"); expect(text(row)).toContain("2m ago");
    expect(text(row)).not.toMatch(/Availability unknown|Retirement|Not archived|Native running|configured|Input tokens/);
    expect(nodes(row).find(n => n.props.testID === "sessions-row-title")!.props.children).toBe(entry().title);
    expect(nodes(row).find(n => n.props.testID === "sessions-row-meta")!.props.numberOfLines).toBe(1);
  });
  it.each([
    { status: "running" as const }, { status: "running" as const, needsInput: true },
    { requiresAttention: true, attentionReason: "finished" as const }, { status: "error" as const }, { status: "closed" as const },
  ])("uses the canonical state marker with no repeated lifecycle prose: %j", patch => {
    const value = entry(patch), row = SessionCompactContent({ entry: value, colors, time: null });
    expect(nodes(row).filter(n => n.type === SessionStateMarker).map(n => n.props.state)).toEqual([sessionState(value)]);
    expect(text(row)).not.toMatch(/Native |Not archived|Availability/);
  });
  it("keeps Archived distinct even for an archived running session", () => {
    const row = SessionCompactContent({ entry: entry({ status: "running", archivedAt: "2026-10-25T10:00:00Z" }), colors, time: null });
    expect(text(row)).toContain("Archived");
    expect(nodes(row).find(n => n.type === SessionStateMarker)!.props.state).toBe("running");
  });
  it("uses existing configured/reported/missing/custom model semantics", () => {
    for (const model of [null, "custom-model-long-name", '["fixture-route","gpt-6.1-sol"]']) {
      const row = SessionCompactContent({ entry: entry({ model }), colors, time: null });
      expect(text(row)).toContain(model === null ? "—" : model.startsWith("[") ? "sol" : model);
      expect(nodes(row).find(n => n.type === "Icon")!.props.name).toBe(model === null ? "CircleHelp" : "SlidersHorizontal");
    }
    const row = SessionCompactContent({ entry: entry({ telemetry: { runtimeInfo: { provider: "codex", sessionId: "fixture", extra: { runtimeModel: "reported-model" } } } }), colors, time: null });
    expect(text(row)).toContain("reported-model"); expect(nodes(row).find(n => n.type === "Icon")!.props.name).toBe("Radio");
  });
  it("preserves caller-provided stale time, and does not invent a time when absent", () => {
    const time = { compact: "10:00", wide: "Updated today at 10:00", spoken: "Updated today at 10:00" };
    expect(text(SessionCompactContent({ entry: entry(), colors, time }))).toContain("10:00");
    expect(nodes(SessionCompactContent({ entry: entry(), colors, time: null })).some(n => n.props.testID === "sessions-row-age")).toBe(false);
  });
  it("shows a missing-record warning only for current exact-ID evidence", () => {
    vi.useFakeTimers(); vi.setSystemTime(Date.parse("2026-10-25T10:00:10Z"));
    const value = entry({ requiresAttention: true, availability: { agentId: "fixture", revision: entry().updatedAt, state: "missing", reason: "Exact read", checkedAt: "2026-10-25T10:00:00Z" } });
    expect(text(SessionCompactContent({ entry: value, colors, time: null }))).toContain("Native record absent");
    expect(text(SessionCompactContent({ entry: { ...value, id: "different" }, colors, time: null }))).not.toContain("Native record absent");
    vi.advanceTimersByTime(60_000);
    expect(text(SessionCompactContent({ entry: value, colors, time: null }))).not.toContain("Native record absent");
  });
  it("retains actionable archive feedback supplied by the owning row", () => {
    expect(text(SessionCompactContent({ entry: entry(), colors, time: null, children: <span>Archive blocked · active child</span> }))).toContain("Archive blocked · active child");
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
