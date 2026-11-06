import { type ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Easing: { linear: (t: number) => t }, View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import type { PluginButtonIconProps } from "@getpaseo/plugin/client";
import { composerSessionPresentation } from "../client/composer-session-state";
import { deriveNeedsInput, type DirectoryEntry } from "../client/session-directory";
import { sessionState, STATE_LABELS } from "../client/session-organization";
import { composerRunningInk, sessionStatusInk } from "../client/session-status-ink";
import { ComposerPulse } from "../client/composer-pulse";
import { observePulseEnvironment, type PulseEnvironment } from "../client/composer-pulse-environment";

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
  ] as const)("draws the Kontor building in canonical $state ink, including precedence and retained earlier error", ({ patch, state }) => {
    const record = entry(patch);
    expect(sessionState(record)).toBe(state);
    const value = composerSessionPresentation(record, { live: true }, "Start or switch session");
    expect(value.title).toBe(`Start or switch session · ${STATE_LABELS[state]}`);
    const root = value.icon(props);
    const icon = root.props.children as ReactElement<{ name: string; color: string }>;
    if (state === "running") {
      expect(icon.type).toBe(ComposerPulse);
      expect(icon.props).toEqual({ colors: props.theme.colors, size: props.size });
    } else {
      expect(icon.props).toMatchObject({ name: "Building2", size: props.size, color: sessionStatusInk(state === "failed" ? "error" : state, props.theme.colors) });
    }
    expect(root.props.accessible).toBe(false);
  });

  it("keeps a pending permission visible even without requiresAttention or a permission reason", () => {
    const record = entry({ status: "running", pendingPermissionCount: 1,
      needsInput: deriveNeedsInput({ pendingPermissionCount: 1, attentionReason: null }) });
    expect(composerSessionPresentation(record, { live: true }, "Kontor").title).toBe("Kontor · Needs input");
  });

  it("mutes the building to last known offline or unverified, without claiming Live or Offline", () => {
    const record = entry({ status: "running" });
    const value = composerSessionPresentation(record, { live: false }, "Kontor");
    expect(value.title).toBe("Kontor · Last known: Running");
    const icon = value.icon(props).props.children as ReactElement<{ name: string; color: string }>;
    expect(icon.props).toMatchObject({ name: "Building2", color: props.theme.colors.foregroundMuted });
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

function environment(initialState: string | null = "active") {
  let resolveMotion!: (reduced: boolean) => void;
  let rejectMotion!: (error: unknown) => void;
  const pending = new Promise<boolean>((resolve, reject) => { resolveMotion = resolve; rejectMotion = reject; });
  const motions = new Set<(reduced: boolean) => void>();
  const states = new Set<(state: string) => void>();
  const errors: unknown[] = [];
  const port: PulseEnvironment = {
    currentState: initialState,
    readReducedMotion: () => pending,
    onReducedMotion: listener => { motions.add(listener); return () => { motions.delete(listener); }; },
    onAppState: listener => { states.add(listener); return () => { states.delete(listener); }; },
    onError: error => { errors.push(error); },
  };
  return { port, resolveMotion, rejectMotion, motions, states, errors,
    motion: (reduced: boolean) => { for (const listener of motions) listener(reduced); },
    state: (state: string) => { for (const listener of states) listener(state); },
  };
}

describe("composer pulse accessibility and lifetime", () => {
  const fg = (foreground: boolean, reducedMotion: boolean) => ({ foreground, reducedMotion });
  it("waits for the OS preference and stops/resumes for live reduced motion and background changes", async () => {
    const fixture = environment();
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    const cleanup = observePulseEnvironment(fixture.port, value => { published.push(value); });
    // N2/N3: the first publish is synchronous at subscribe — foreground from AppState.currentState,
    // Reduce Motion still unknown and therefore reduced.
    expect(published).toEqual([fg(true, true)]);
    fixture.resolveMotion(false);
    await Promise.resolve();
    fixture.state("inactive");
    fixture.state("background"); // already backgrounded: no new facts
    fixture.state("active");
    fixture.motion(true);
    fixture.motion(false);
    expect(published).toEqual([fg(true, true), fg(true, false), fg(false, false), fg(true, false), fg(true, true), fg(true, false)]);
    cleanup();
    expect([fixture.motions.size, fixture.states.size]).toEqual([0, 0]);
  });

  it("does not let an older preference read undo a live accessibility event", async () => {
    const fixture = environment();
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    const cleanup = observePulseEnvironment(fixture.port, value => { published.push(value); });
    expect(published).toEqual([fg(true, true)]); // the synchronous subscribe publish
    fixture.motion(true);
    fixture.resolveMotion(false);
    await Promise.resolve();
    expect(published).toEqual([fg(true, true)]); // the live event beat the late read; nothing new
    fixture.motion(false);
    expect(published).toEqual([fg(true, true), fg(true, false)]);
    cleanup();
  });

  it("ignores preference completion after host switch or unmount", async () => {
    const fixture = environment();
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    const cleanup = observePulseEnvironment(fixture.port, value => { published.push(value); });
    expect(published).toEqual([fg(true, true)]); // the synchronous subscribe publish
    cleanup();
    fixture.resolveMotion(false);
    await Promise.resolve();
    fixture.motion(false);
    fixture.state("active");
    expect(published).toEqual([fg(true, true)]);
    expect([fixture.motions.size, fixture.states.size]).toEqual([0, 0]);
  });

  it("keeps motion disabled when the native preference read fails", async () => {
    const fixture = environment();
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    const cleanup = observePulseEnvironment(fixture.port, value => { published.push(value); });
    // N2: even a rejected read leaves the synchronous subscribe facts in place — foreground flow
    // without the pulse; "motion disabled" now means the pulse stays off, not that the flow stops.
    expect(published).toEqual([fg(true, true)]);
    fixture.rejectMotion(new Error("preference unavailable"));
    await Promise.resolve();
    await Promise.resolve();
    expect(published).toEqual([fg(true, true)]);
    expect(fixture.errors.map(error => String(error))).toEqual(["Error: preference unavailable"]);
    cleanup();
  });

  it("waits for foreground when the initial native app state is unknown", async () => {
    const fixture = environment(null);
    const published: { foreground: boolean; reducedMotion: boolean }[] = [];
    const cleanup = observePulseEnvironment(fixture.port, value => { published.push(value); });
    // Unknown app state: the synchronous publish is foreground=false, Reduce Motion unknown-reduced.
    expect(published).toEqual([fg(false, true)]);
    fixture.resolveMotion(false);
    await Promise.resolve();
    expect(published).toEqual([fg(false, true), fg(false, false)]);
    fixture.state("active");
    expect(published).toEqual([fg(false, true), fg(false, false), fg(true, false)]);
    cleanup();
  });

  it("uses Kontor's blue appearance ink with a green host accent in either theme", () => {
    expect(composerRunningInk(props.theme.colors)).toBe("#9cbfff");
    expect(composerRunningInk({ ...props.theme.colors, surface0: "#ffffff", surface1: "#f5f6f8", surface2: "#e6ebf2", foreground: "#20242c" })).toBe("#245bba");
  });
});
