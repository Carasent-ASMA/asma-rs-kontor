import { createHash } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { createSessionSummaryService, summaryStateRoot } from "../server/session-summaries";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator-capabilities";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
const HANDOFF = "Summary: fix the parser; migration is next.";

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

function mockClient(agent: Agent, timeline: { item: Record<string, unknown> }[] = [{ item: { type: "assistant_message", text: HANDOFF, createdAt: "2026-10-25T10:00:00Z" } }]) {
  return {
    agents: {
      ref: (id: string) => ({
        refresh: async () => id === agent.id ? { agent, project: null } : null,
        timeline: { refetch: async () => ({ entries: timeline, error: null }) },
      }),
    },
  };
}

describe("session summary store", () => {
  it("reports summaries supported with explicit attach gap in navigator capabilities", () => {
    const caps = readInstalledNavigatorCapabilities();
    expect(caps.summaries.supported).toBe(true);
    expect(caps.draftAttach).toMatchObject({ supported: false });
    expect(caps.takeover).toMatchObject({ supported: false });
  });

  it("saves a durable revision and returns the same revision when the handoff is unchanged", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    const agent = { id: "agent-1", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
    const service = createSessionSummaryService(mockClient(agent) as never, root);
    const saved = await service.save("agent-1");
    expect(saved).toMatchObject({ sourceAgentId: "agent-1", revision: 1, cutoff: "2026-10-25T10:00:00Z", text: expect.stringContaining(HANDOFF) });
    expect(await service.latest(["agent-1"])).toEqual({ "agent-1": saved });
    expect((await service.save("agent-1")).revision).toBe(1);
  });

  it("bumps revision when the handoff text changes", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    const agent = { id: "agent-1", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
    let timeline = [{ item: { type: "assistant_message", text: HANDOFF, createdAt: "2026-10-25T10:00:00Z" } }];
    const service = createSessionSummaryService({ agents: { ref: (id: string) => ({
      refresh: async () => id === agent.id ? { agent, project: null } : null,
      timeline: { refetch: async () => ({ entries: timeline, error: null }) },
    }) } } as never, root);
    expect((await service.save("agent-1")).revision).toBe(1);
    timeline = [{ item: { type: "assistant_message", text: "Summary: new handoff.", createdAt: "2026-10-25T10:05:00Z" } }];
    expect((await service.save("agent-1")).revision).toBe(2);
  });

  it("refuses save when the session wrote no assistant handoff", async () => {
    const root = await mkdtemp(join(tmpdir(), "kontor-summary-")); dirs.push(root);
    const agent = { id: "agent-2", title: "Empty", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
    const service = createSessionSummaryService(mockClient(agent, []) as never, root);
    await expect(service.save("agent-2")).rejects.toThrow(/no assistant handoff/i);
  });

  it("uses the configured plugin state root path", () => {
    expect(summaryStateRoot()).toMatch(/session-summaries-v1$/);
    expect(createHash("sha256").update("agent-1").digest("hex").length).toBe(64);
  });
});
