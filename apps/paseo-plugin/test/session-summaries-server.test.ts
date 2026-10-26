import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import {
  SUMMARY_LIMITS_WITH_GENERATION,
  SUMMARY_MODEL_API_GAP,
  createSessionSummaryService,
  paseoSummaryGenerator,
  summariesInstalledCapability,
  summaryStateRoot,
} from "../server/session-summaries";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

const SECTIONS = { task: "Fix the parser", decisions: "Use ArkType", currentState: "Tests green", nextStep: "Open PR" };

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

function paseoWithGenerator(agent: Agent, generate: () => Promise<{ sections: typeof SECTIONS; cutoff: string }>) {
  return {
    agents: {
      ref: (id: string) => ({ refresh: async () => id === agent.id ? { agent, project: null } : null }),
      generateCompactSummary: async () => generate(),
    },
  };
}

describe("session summary host route", () => {
  it("reports summaries unsupported with the exact model API gap on the installed SDK", () => {
    const paseo = { agents: { ref: () => ({ refresh: async () => null }) } } as never;
    expect(summariesInstalledCapability(paseo)).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
    expect(paseoSummaryGenerator(paseo)).toBeUndefined();
    const caps = readInstalledNavigatorCapabilities(paseo);
    expect(caps.summaries).toEqual({ supported: false, reason: SUMMARY_MODEL_API_GAP });
    expect(caps.takeover.supported).toBe(true);
  });

  it("qualifies summaries and store limits when generateCompactSummary is on the SDK", () => {
    const paseo = { agents: { generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) } } as never;
    expect(summariesInstalledCapability(paseo)).toEqual({ supported: true, limits: SUMMARY_LIMITS_WITH_GENERATION });
  });

  it("persists model-generated sections and bumps revision when cutoff changes", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    const agent = { id: "agent-1", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
    let cutoff = "2026-10-25T10:00:00Z";
    const paseo = paseoWithGenerator(agent, async () => ({ sections: SECTIONS, cutoff }));
    const service = createSessionSummaryService(paseo as never, root);
    const first = await service.save("agent-1");
    expect(first).toMatchObject({ revision: 1, sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" });
    expect(first.text).toContain("Fix the parser");
    expect(await service.save("agent-1")).toEqual(first);
    cutoff = "2026-10-25T11:00:00Z";
    const second = await service.save("agent-1");
    expect(second.revision).toBe(2);
    expect(await service.latest(["agent-1"])).toEqual({ "agent-1": second });
  });

  it("uses the configured plugin host-backed state root", () => {
    expect(summaryStateRoot()).toMatch(/plugin-state\/kontor\/session-summaries-v1$/);
  });
});
