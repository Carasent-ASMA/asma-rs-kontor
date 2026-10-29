import { describe, expect, it, vi } from "vitest";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { UNSUPPORTED } from "../shared/session-navigator";
import { applyTakeover } from "../shared/session-lifecycle";
import { contributeSessionLifecycle } from "../server/session-lifecycle";
import { SUMMARY_MODEL_API_GAP } from "../server/session-summaries";
import { asTakeoverClient, paseoWakeScheduleReader, readInstalledNavigatorCapabilities } from "../server/session-navigator";

describe("installed navigator capabilities", () => {
  const paseo = { agents: {}, workspaces: {} } as never;

  it("keeps draft attachment unsupported even when a host advertises legacy draft APIs", () => {
    const host = { supportsHostFeature: () => true, agents: { create: vi.fn(), ref: vi.fn(), attachSavedSummaryToDraft: vi.fn() }, workspaces: {} } as never;
    const caps = readInstalledNavigatorCapabilities(host);
    expect(caps.draftAttach).toEqual(UNSUPPORTED.draftAttach);
    expect(caps.takeover.supported).toBe(true);
    expect(caps.takeover.supported && caps.takeover.limits?.join(" ")).toMatch(/stop/i);
    expect(caps.usageTotals.supported).toBe(true);
    expect(caps.summaries.supported).toBe(true);
  });

  it("gates takeover on the installed provider-independent summarizer", () => {
    const caps = readInstalledNavigatorCapabilities(paseo);
    expect(caps.takeover).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
    expect(caps.takeoverRoute).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
    expect(caps.summaries).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
    expect(caps.takeoverBulk).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
  });

  it("advertises model override and bulk only with the summarizer installed", () => {
    const withCreation = { agents: { create: vi.fn(), ref: vi.fn() }, workspaces: {} } as never;
    const caps = readInstalledNavigatorCapabilities(withCreation);
    expect(caps.takeoverRoute.supported).toBe(true);
    expect(caps.takeoverBulk.supported).toBe(true);
    expect(caps.takeover.supported).toBe(true);
  });

  it("reports the optional wake reader without changing the takeover capability", () => {
    const withSchedules = { agents: { create: vi.fn(), ref: vi.fn() }, workspaces: {}, schedules: { forAgent: async () => ({ entries: [] }) } } as never;
    const caps = readInstalledNavigatorCapabilities(withSchedules);
    expect(caps.takeover.supported).toBe(true);
    expect(paseoWakeScheduleReader(withSchedules)).toBeDefined();
    expect(asTakeoverClient(withSchedules).schedules).toBeDefined();
  });

  it("rejects a direct or stale apply even with no scheduled wakes and performs no takeover effects", async () => {
    const handlers = new Map<string, (...args: any[]) => unknown>();
    contributeSessionLifecycle({ handle: (rpc: { name: string }, handler: (...args: any[]) => unknown) => handlers.set(rpc.name, handler) } as unknown as PluginServerContext);
    const archive = vi.fn(), create = vi.fn(), send = vi.fn();
    const host = { schedules: { forAgent: async () => ({ entries: [] }) }, agents: { ref: () => ({ archive, send }) }, workspaces: { ref: () => ({ agents: { create } }) } };
    const apply = handlers.get(applyTakeover.name)!;
    await expect(apply({ reviewId: "33333333-3333-4333-8333-333333333333", revision: 1, digest: "a".repeat(64) }, { paseo: host })).rejects.toThrow("This review is not on this host");
    expect(archive).not.toHaveBeenCalled();
    expect(create).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });
});
