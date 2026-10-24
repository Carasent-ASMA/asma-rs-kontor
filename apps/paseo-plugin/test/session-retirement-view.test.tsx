import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { deriveSessionProjection, MISSING_OWNER_ADAPTER, READY_IS_DESCRIPTIVE, type SessionProjection, type SessionProjectionInput } from "../shared/session-lifecycle";

vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Platform: { OS: "web" }, Pressable: "Pressable", ScrollView: "ScrollView", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", ScrollView: "ScrollView" }));

import { SessionRetirementConflict, SessionRetirementDetails, SessionRetirementSummary, retirementDispositionLabel } from "../client/session-lifecycle-view";

const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as Parameters<typeof SessionRetirementSummary>[0]["colors"];
const source = (id: string, kind: "native" | "label" | "archive" | "owner" = "label") => ({ id, kind, detail: `fixture ${id}`, observedAt: "2026-10-24T08:00:00.000Z", revision: "rev-fixture", fresh: "current" as const });
const input = (patch: Partial<SessionProjectionInput> = {}): SessionProjectionInput => ({
  hostId: "host", coverage: { archiveInventory: "complete", observedAt: "2026-10-24T08:00:02.000Z", detail: "complete fixture inventory" },
  intent: { state: "none", reviewId: null, sessionId: "fix-session", observedAt: null, detail: "No unresolved archive receipt." },
  release: { state: "unavailable", reason: MISSING_OWNER_ADAPTER },
  session: { id: "fix-session", status: "idle", archived: false, archivedAt: null, activeTurn: false, pendingInteraction: false, title: "SWE • ASMA-9 • fixture", workspaceId: "wks", projectKey: "prj", provider: "fixture", model: null, updatedAt: "2026-10-24T08:00:00.000Z", observedAt: "2026-10-24T08:00:01.000Z" },
  now: "2026-10-24T08:05:00.000Z", ...patch,
});
const conflicts = (projection: SessionProjection) => deriveSessionProjection(input({ session: { ...input().session, status: "running", archived: true, archivedAt: "2026-10-24T08:02:00.000Z", activeTurn: true } }));
function elements(value: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  if (typeof value.type === "function") return elements((value.type as (props: Record<string, unknown>) => ReactNode)(value.props));
  return [value, ...elements(value.props.children as ReactNode)];
}
function textOf(value: ReactNode): string {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(textOf).join(" ");
  if (!isValidElement<Record<string, unknown>>(value)) return "";
  if (typeof value.type === "function") return textOf((value.type as (props: Record<string, unknown>) => ReactNode)(value.props));
  return textOf(value.props.children as ReactNode);
}

describe("read-only retirement view", () => {
  it.each([["retain", "Retain"], ["ready", "Ready"], ["unknown", "Unknown"]] as const)("labels %s as %s", (disposition, label) => {
    expect(retirementDispositionLabel(disposition)).toBe(label);
  });

  it("shows the disposition, exact reason, native status and archived conflict separately", () => {
    const projection = conflicts(deriveSessionProjection(input()));
    const drawn = SessionRetirementSummary({ projection, colors });
    const all = textOf(drawn);
    expect(all).toContain("Retirement: Retain");
    expect(all).toContain("Native active execution is present.");
    expect(all).toContain("Native running · Archived");
    expect(all).toContain("Archived conflict");
    expect(all).toContain("archived-with-active-execution");
    expect(elements(drawn).find((element) => element.props.testID === "session-retirement-conflicts")).toBeTruthy();
  });

  it("lists missing facts, role responsibility and each evidence source with its own time", () => {
    const base = input({ roleClaims: [{ kind: "role", value: "SWE", binding: "claimed", source: source("title:fixture") }], workClaims: [{ kind: "epic", value: "ASMA-9", binding: "claimed", source: source("label:epic") }] });
    const projection = deriveSessionProjection(base);
    const drawn = SessionRetirementDetails({ projection, colors });
    const all = textOf(drawn);
    expect(all).toContain("Retirement Unknown");
    expect(all).toContain("Native status idle");
    expect(all).toContain("Archive membership Not archived");
    expect(all).toContain("Claimed role: SWE. Responsibility unknown");
    expect(all).toContain("epic: ASMA-9 (claimed; label)");
    expect(all).toContain(MISSING_OWNER_ADAPTER);
    expect(all).toContain("Evidence sources");
    expect(all).toContain("label · fixture title:fixture · 2026-10-24T08:00:00.000Z · revision rev-fixture · current");
  });

  it("shows a ready projection as descriptive and keeps native facts unchanged", () => {
    const projection = deriveSessionProjection(input({
      workClaims: [{ kind: "epic", value: "ASMA-9", binding: "verified", source: source("owner:binding", "owner") }],
      responsibilities: [{ owner: "fixture owner", sessionId: "fix-session", role: "SWE", responsibility: "fixture duty", source: source("responsibility:fixture", "owner") }],
      release: { state: "available", owner: "fixture owner", sessionId: "fix-session", accepted: true, dutiesSettled: true, descendantsSettled: true, wakesSettled: true, executor: "verified", source: source("owner:fixture", "owner") },
    }));
    const drawn = SessionRetirementDetails({ projection, colors });
    expect(textOf(drawn)).toContain("Ready");
    expect(textOf(drawn)).toContain(READY_IS_DESCRIPTIVE);
    expect(textOf(drawn)).toContain("Not archived");
    expect(textOf(drawn)).toContain("No missing evidence recorded.");
  });

  it("renders nothing for a conflict-free projection", () => {
    expect(SessionRetirementConflict({ conflicts: [], colors })).toBeNull();
  });
});
