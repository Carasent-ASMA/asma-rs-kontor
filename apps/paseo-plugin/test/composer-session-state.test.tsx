import { type ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import type { PluginButtonIconProps } from "@getpaseo/plugin/client";
import { composerSessionPresentation } from "../client/composer-session-state";
import { deriveNeedsInput, type DirectoryEntry } from "../client/session-directory";
import { sessionState, STATE_LABELS } from "../client/session-organization";
import { QUICK_FILTER_ICONS } from "../client/mobile-quick-filter-options";

function entry(patch: Partial<DirectoryEntry> = {}): DirectoryEntry {
  return {
    id: "session-a", title: "Synthetic session", workspaceId: "workspace-a",
    provider: "fixture", cwd: "/fixture", model: null, labels: {},
    status: "idle", requiresAttention: false, attentionReason: null,
    attentionTimestamp: null, pendingPermissionCount: 0, needsInput: false,
    archivedAt: null, createdAt: "2026-10-25T10:00:00Z", updatedAt: "2026-10-25T10:00:00Z",
    parentAgentId: null, projectKey: null, projectName: null, workspaceName: null, searchText: "synthetic session",
    ...patch,
  };
}

const props: PluginButtonIconProps = {
  context: "agent", agentId: "session-a", workspaceId: "workspace-a", size: 16, color: "#ffffff",
  host: { id: "host-a", label: "Synthetic host" }, layout: { compact: true, platform: "web" },
  theme: { colors: { surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A",
    foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff",
    statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b" } },
};

describe("composer exact-session state presentation", () => {
  it.each([
    { patch: { status: "running", attentionReason: "permission", needsInput: true }, state: "needs_input" },
    { patch: { status: "error" }, state: "failed" },
    { patch: { status: "running", requiresAttention: true, attentionReason: "finished" }, state: "running" },
    { patch: { requiresAttention: true, attentionReason: "finished" }, state: "attention" },
    { patch: {}, state: "idle" },
    { patch: { status: "initializing" }, state: "initializing" },
    { patch: { status: "closed" }, state: "closed" },
    { patch: { status: "idle", requiresAttention: true, attentionReason: "error" }, state: "idle" },
  ] as const)("uses canonical $state, including precedence and retained earlier error", ({ patch, state }) => {
    const record = entry(patch);
    expect(sessionState(record)).toBe(state);
    const value = composerSessionPresentation(record, { live: true }, "Start or switch session");
    expect(value.title).toBe(`Start or switch session · ${STATE_LABELS[state]}`);
    const root = value.icon(props);
    const icon = root.props.children as ReactElement<{ name: string }>;
    expect(icon.props.name).toBe(QUICK_FILTER_ICONS[state === "failed" ? "error" : state]);
    expect(root.props.accessible).toBe(false);
  });

  it("keeps a pending permission visible even without requiresAttention or a permission reason", () => {
    const record = entry({ status: "running", pendingPermissionCount: 1,
      needsInput: deriveNeedsInput({ pendingPermissionCount: 1, attentionReason: null }) });
    expect(composerSessionPresentation(record, { live: true }, "Kontor").title).toBe("Kontor · Needs input");
  });

  it("retains a muted last-known shape offline or unverified, without claiming Live or Offline", () => {
    const record = entry({ status: "running" });
    const value = composerSessionPresentation(record, { live: false }, "Kontor");
    expect(value.title).toBe("Kontor · Last known: Running");
    const icon = value.icon(props).props.children as ReactElement<{ name: string; color: string }>;
    expect(icon.props).toMatchObject({ name: "Play", color: props.theme.colors.foregroundMuted });
    expect(value.title).not.toMatch(/Live|Offline/);
  });

  it("has stable component identities across metadata changes and restores the current icon on reconnect", () => {
    const initial = composerSessionPresentation(entry(), { live: true }, "Kontor");
    const metadata = composerSessionPresentation(entry({ title: "Renamed", model: "synthetic-model" }), { live: true }, "Kontor");
    expect(metadata).toEqual(initial);
    expect(composerSessionPresentation(entry(), { live: false }, "Kontor").icon).not.toBe(initial.icon);
    expect(composerSessionPresentation(entry(), { live: true }, "Kontor").icon).toBe(initial.icon);
  });
});
