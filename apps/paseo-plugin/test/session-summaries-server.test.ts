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
  readPinnedSummary,
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
    supportsHostFeature: (feature: "agentCompactSummary") => feature === "agentCompactSummary",
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

  it("qualifies summaries and store limits when the host advertises agentCompactSummary", () => {
    const paseo = {
      supportsHostFeature: (feature: "agentCompactSummary") => feature === "agentCompactSummary",
      agents: { generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) },
    } as never;
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

describe("session summary store serialization", () => {
  const agent = { id: "agent-1", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;

  async function service(root: string, generate: () => Promise<{ sections: typeof SECTIONS; cutoff: string }>) {
    const paseo = paseoWithGenerator(agent, generate);
    return { service: createSessionSummaryService(paseo as never, root), root };
  }

  it("concurrent first saves with identical content dedupe to one summaryId and revision 1", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    const payload = { sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" };
    const { service: s } = await service(root, async () => payload);
    const [left, right] = await Promise.all([s.save("agent-1"), s.save("agent-1")]);
    expect(left.summaryId).toBe(right.summaryId);
    expect(left.revision).toBe(1);
    expect(right.revision).toBe(1);
    expect(left.text).toBe(right.text);
    await expect(readPinnedSummary(root, { summaryId: left.summaryId, revision: 1 })).resolves.toEqual(left);
  });

  it("concurrent saves with different content yield sequential revisions under one summaryId", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    let calls = 0;
    const { service: s } = await service(root, async () => {
      const n = ++calls;
      if (n === 1) await new Promise((resolve) => setTimeout(resolve, 5));
      return n === 1
        ? { sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }
        : { sections: { ...SECTIONS, task: "Ship the desk layout" }, cutoff: "2026-10-25T11:00:00Z" };
    });
    const [first, second] = await Promise.all([s.save("agent-1"), s.save("agent-1")]);
    expect(first.summaryId).toBe(second.summaryId);
    expect(new Set([first.revision, second.revision])).toEqual(new Set([1, 2]));
    expect(first.text).not.toBe(second.text);
    await expect(readPinnedSummary(root, { summaryId: first.summaryId, revision: first.revision })).resolves.toMatchObject({ text: first.text });
    await expect(readPinnedSummary(root, { summaryId: second.summaryId, revision: second.revision })).resolves.toMatchObject({ text: second.text });
  });

  it("does not assign the same revision number to different text under one summaryId", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    let calls = 0;
    const { service: s } = await service(root, async () => {
      calls++;
      return calls % 2 === 1
        ? { sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }
        : { sections: { ...SECTIONS, nextStep: "Merge" }, cutoff: "2026-10-25T10:30:00Z" };
    });
    const results = await Promise.all(Array.from({ length: 4 }, () => s.save("agent-1")));
    for (const result of results) {
      const stored = await readPinnedSummary(root, { summaryId: result.summaryId, revision: result.revision });
      expect(stored.text).toBe(result.text);
      expect(stored.cutoff).toBe(result.cutoff);
    }
    const slot = new Map<string, string>();
    for (const result of results) {
      const key = `${result.summaryId}:${result.revision}`;
      const prior = slot.get(key);
      if (prior !== undefined) expect(prior).toBe(result.text);
      else slot.set(key, result.text);
    }
  });
});
