import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { UNSUPPORTED } from "../shared/session-navigator";
import {
  DRAFT_PLACEMENT_API_GAP,
  DRAFT_READ_API_GAP,
  MALFORMED_HOST_DRAFT_READ_POST,
  MALFORMED_HOST_DRAFT_READ_PRE,
  createSummaryAttachService,
  draftAttachInstalledCapability,
  parseHostSummaryDraftRead,
  paseoSummaryDraftPlacer,
  readDestinationAttachState,
} from "../server/session-summary-attach";
import { createSessionSummaryService, readPinnedSummary } from "../server/session-summaries";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

const SECTIONS = { task: "Fix the parser", decisions: "Use ArkType", currentState: "Tests green", nextStep: "Open PR" };
const SOURCE = { id: "agent-src", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
const DEST = { id: "agent-dest", title: "Billing", updatedAt: "2026-10-25T10:00:00Z", status: "idle" } as Agent;

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, title: id, status: "idle", archivedAt: null, updatedAt: "2026-10-25T10:00:00Z", ...patch } as Agent;
}

function paseoWithDraft(
  agents: Record<string, Agent>,
  drafts: Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>,
  hooks?: { onPlace?: () => void | Promise<void> },
) {
  const placed: { summaryId: string; revision: number; text: string; destinationAgentId: string }[] = [];
  return {
    paseo: {
      agents: {
        ref: (id: string) => ({ refresh: async () => { const a = agents[id]; return a ? { agent: a, project: null } : null; } }),
        generateCompactSummary: async (agentId: string) => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
          await hooks?.onPlace?.();
          if (drafts.has(destinationAgentId) && (drafts.get(destinationAgentId)!.summaryId !== draft.summaryId || drafts.get(destinationAgentId)!.revision !== draft.revision)) {
            throw new Error("draft occupied");
          }
          drafts.set(destinationAgentId, draft);
          placed.push({ ...draft, destinationAgentId });
        },
        readSummaryDraft: async (destinationAgentId: string) => {
          const draft = drafts.get(destinationAgentId);
          return draft ? { summaryId: draft.summaryId, revision: draft.revision } : null;
        },
      },
    },
    placed,
    drafts,
  };
}

async function seedSummary(summaryRoot: string, source: Agent, text: string) {
  const { paseo } = paseoWithDraft({ [source.id]: source }, new Map());
  const saved = await createSessionSummaryService(paseo as never, summaryRoot).save(source.id);
  if (saved.text === text) return saved;
  const file = join(summaryRoot, `source-${createHash("sha256").update(source.id).digest("hex")}.json`);
  const store = JSON.parse(await import("node:fs/promises").then((m) => m.readFile(file, "utf8")));
  store.revisions["1"] = { ...store.revisions["1"], text };
  await writeFile(file, JSON.stringify(store));
  return readPinnedSummary(summaryRoot, { summaryId: store.summaryId, revision: 1 });
}

describe("parseHostSummaryDraftRead", () => {
  it.each([
    [null, "empty"],
    [undefined, "malformed"],
    ["", "malformed"],
    [false, "malformed"],
    [{ summaryId: "00000000-0000-4000-8000-000000000099", revision: 1 }, "occupied"],
    [{ summaryId: "not-a-uuid", revision: 1 }, "malformed"],
    [{ summaryId: "00000000-0000-4000-8000-000000000099", revision: 0 }, "malformed"],
    [{ revision: 1 }, "malformed"],
  ] as const)("classifies %j as %s", (raw, state) => {
    expect(parseHostSummaryDraftRead(raw).state).toBe(state);
  });
});

describe("summary attach host route", () => {
  it("keeps draftAttach unsupported with the exact gap when placeSummaryDraft is absent", () => {
    const paseo = { agents: { generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) } } as never;
    expect(paseoSummaryDraftPlacer(paseo)).toBeUndefined();
    expect(draftAttachInstalledCapability(paseo)).toEqual(UNSUPPORTED.draftAttach);
    expect(readInstalledNavigatorCapabilities(paseo).draftAttach).toEqual(UNSUPPORTED.draftAttach);
  });

  it("keeps draftAttach unsupported with the read gap when place exists without readSummaryDraft", () => {
    const paseo = {
      agents: {
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async () => {},
      },
    } as never;
    expect(paseoSummaryDraftPlacer(paseo)).toBeUndefined();
    expect(draftAttachInstalledCapability(paseo)).toEqual({ supported: false, reason: DRAFT_READ_API_GAP });
    expect(readInstalledNavigatorCapabilities(paseo).draftAttach).toEqual({ supported: false, reason: DRAFT_READ_API_GAP });
  });

  it("attach throws the draft placement gap when the SDK route is missing", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const revision = await seedSummary(summaryRoot, SOURCE, "Pinned summary text");
    const paseo = { agents: { ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }), generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) } };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    await expect(service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id })).rejects.toThrow(DRAFT_PLACEMENT_API_GAP);
  });

  it("attach throws the read gap when placement exists without readSummaryDraft", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const revision = await seedSummary(summaryRoot, SOURCE, "Pinned summary text");
    const paseo = {
      agents: {
        ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async () => {},
      },
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    await expect(service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id })).rejects.toThrow(DRAFT_READ_API_GAP);
  });
});

describe("summary attach with qualified SDK", () => {
  async function harness(extra?: Partial<Agent>, hooks?: { onPlace?: () => void | Promise<void> }) {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
    const dest = { ...DEST, ...extra };
    const { paseo, placed } = paseoWithDraft({ [SOURCE.id]: SOURCE, [dest.id]: dest }, drafts, hooks);
    const revision = await seedSummary(summaryRoot, SOURCE, "Attach this summary body");
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    return { service, summaryRoot, attachRoot, revision, placed, drafts, paseo, dest };
  }

  it("drafts an exact pin to a live destination and readPinned returns the same revision", async () => {
    const { service, summaryRoot, revision } = await harness();
    const outcome = await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id });
    expect(outcome).toMatchObject({ state: "drafted", destinationAgentId: DEST.id, summaryId: revision.summaryId, revision: revision.revision });
    await expect(readPinnedSummary(summaryRoot, { summaryId: revision.summaryId, revision: revision.revision })).resolves.toMatchObject({ text: "Attach this summary body" });
  });

  it("refuses source=destination, archived destination, and missing revision", async () => {
    const { service, revision } = await harness();
    expect(await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: SOURCE.id }))
      .toMatchObject({ state: "refused", message: expect.stringMatching(/not back to its own source/) });
    const archived = agent("agent-archived", { archivedAt: "2026-10-20T09:00:00Z", title: "Archived seat" });
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const rev = await seedSummary(summaryRoot, SOURCE, "Attach this summary body");
    const { paseo } = paseoWithDraft({ [SOURCE.id]: SOURCE, [archived.id]: archived }, new Map());
    const archivedService = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    expect(await archivedService.attach({ summaryId: rev.summaryId, revision: rev.revision, destinationAgentId: archived.id }))
      .toMatchObject({ state: "refused", message: expect.stringMatching(/archived/) });
    expect(await service.attach({ summaryId: revision.summaryId, revision: 9, destinationAgentId: DEST.id }))
      .toMatchObject({ state: "refused", message: expect.stringMatching(/no saved summary/i) });
  });

  it.each([
    ["running", { status: "running" as const, title: "Busy seat" }, /running/i],
    ["initializing", { status: "initializing" as const, title: "Boot seat" }, /initializing/i],
    ["active turn", { activeTurn: { turnId: "turn-1", startedAt: "2026-10-25T10:00:00Z" }, title: "Turn seat" }, /active turn/i],
  ] as const)("refuses a %s destination before placement with no host place", async (_label, patch, pattern) => {
    const { service, revision, placed, attachRoot } = await harness(patch);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    expect(await service.attach(pin)).toMatchObject({ state: "refused", message: expect.stringMatching(pattern) });
    expect(placed).toHaveLength(0);
    expect((await readDestinationAttachState(attachRoot, DEST.id)).intents).toEqual({});
  });

  it("refuses an archived destination on the first in-lock read with no intent or place", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const archived = agent(DEST.id, { archivedAt: "2026-10-20T09:00:00Z", title: "Archived billing" });
    const revision = await seedSummary(summaryRoot, SOURCE, "Pre-place archived");
    const placed: unknown[] = [];
    const paseo = {
      agents: {
        ref: () => ({ refresh: async () => ({ agent: archived, project: null }) }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (...args: unknown[]) => { placed.push(args); },
        readSummaryDraft: async () => null,
      },
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    expect(await service.attach(pin)).toMatchObject({ state: "refused", message: expect.stringMatching(/archived/) });
    expect(placed).toHaveLength(0);
    expect((await readDestinationAttachState(attachRoot, DEST.id)).intents).toEqual({});
  });

  it("returns the same drafted outcome on duplicate attach without a second host place", async () => {
    const { service, revision, placed } = await harness();
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    const first = await service.attach(pin);
    const second = await service.attach(pin);
    expect(second).toEqual(first);
    expect(placed).toHaveLength(1);
  });

  it("refuses when the host read shows a different draft", async () => {
    const { service, revision, drafts, placed } = await harness();
    drafts.set(DEST.id, { summaryId: "00000000-0000-4000-8000-000000000099", revision: 1, text: "other", sourceAgentId: "agent-other" });
    const outcome = await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringMatching(/never replaces a draft/) });
    expect(placed).toHaveLength(0);
  });

  it("reconciles from host read when the draft is already present without calling place", async () => {
    const { service, revision, drafts, placed } = await harness();
    drafts.set(DEST.id, { summaryId: revision.summaryId, revision: revision.revision, text: revision.text, sourceAgentId: revision.sourceAgentId });
    const outcome = await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id });
    expect(outcome).toMatchObject({ state: "drafted" });
    expect(placed).toHaveLength(0);
  });

  it("records unknown after host failure and does not place again on retry", async () => {
    let placeCalls = 0;
    const { service, revision, placed, attachRoot } = await harness(undefined, {
      onPlace: async () => { placeCalls++; throw new Error("host down"); },
    });
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    expect(await service.attach(pin)).toMatchObject({ state: "unknown", message: expect.stringMatching(/will not place it again/i) });
    expect(placeCalls).toBe(1);
    const state = await readDestinationAttachState(attachRoot, DEST.id);
    expect(state.intents[`${pin.summaryId}:${pin.revision}:${DEST.id}`]).toBeDefined();
    expect(await service.attach(pin)).toMatchObject({ state: "unknown", message: expect.stringMatching(/attach intent is recorded/i) });
    expect(placed).toHaveLength(0);
    expect(placeCalls).toBe(1);
  });

  it("refuses when the destination is archived after the lock starts and never records intent or place", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
    const revision = await seedSummary(summaryRoot, SOURCE, "Race body");
    let refreshCalls = 0;
    const archived = agent(DEST.id, { archivedAt: "2026-10-20T09:00:00Z", title: "Billing" });
    const paseo = {
      agents: {
        ref: (id: string) => ({
          refresh: async () => {
            refreshCalls++;
            if (id !== DEST.id) return null;
            if (refreshCalls === 1) return { agent: DEST, project: null };
            return { agent: archived, project: null };
          },
        }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
          drafts.set(destinationAgentId, draft);
        },
        readSummaryDraft: async () => null,
      },
    };
    const placed: unknown[] = [];
    paseo.agents.placeSummaryDraft = async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
      placed.push({ destinationAgentId, draft });
      drafts.set(destinationAgentId, draft);
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    const outcome = await service.attach(pin);
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringMatching(/archived/) });
    expect(placed).toHaveLength(0);
    const state = await readDestinationAttachState(attachRoot, DEST.id);
    expect(state.intents).toEqual({});
  });

  it.each([undefined, "", false, { revision: 1 }, { summaryId: "00000000-0000-4000-8000-000000000099" }])(
    "returns unknown without placement when readSummaryDraft returns %j",
    async (badRead) => {
      const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
      const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
      const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
      const revision = await seedSummary(summaryRoot, SOURCE, "Adversarial attach body");
      const placed: unknown[] = [];
      const paseo = {
        agents: {
          ref: (id: string) => ({ refresh: async () => ({ agent: DEST, project: null }) }),
          generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
          placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
            placed.push({ destinationAgentId, draft });
            drafts.set(destinationAgentId, draft);
          },
          readSummaryDraft: async () => badRead as never,
        },
      };
      const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
      const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
      const outcome = await service.attach(pin);
      expect(outcome).toMatchObject({ state: "unknown", message: expect.stringContaining(MALFORMED_HOST_DRAFT_READ_PRE) });
      expect(placed).toHaveLength(0);
    },
  );

  it.each([undefined, "", false])(
    "returns unknown after one place when post-place readSummaryDraft returns %j, retains intent, and does not place again",
    async (badRead) => {
      const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
      const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
      const revision = await seedSummary(summaryRoot, SOURCE, "Post-place malformed body");
      let readCalls = 0;
      let placeCalls = 0;
      const paseo = {
        agents: {
          ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }),
          generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
          placeSummaryDraft: async () => { placeCalls++; },
          readSummaryDraft: async () => { readCalls++; return readCalls <= 2 ? null : (badRead as never); },
        },
      };
      const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
      const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
      const first = await service.attach(pin);
      expect(first).toMatchObject({ state: "unknown", message: expect.stringContaining(MALFORMED_HOST_DRAFT_READ_POST) });
      expect(first.message).not.toMatch(/Nothing was placed/i);
      expect(placeCalls).toBe(1);
      const state = await readDestinationAttachState(attachRoot, DEST.id);
      expect(state.intents[`${pin.summaryId}:${pin.revision}:${DEST.id}`]).toBeDefined();
      expect(await service.attach(pin)).toMatchObject({ state: "unknown", message: expect.stringMatching(/attach intent is recorded/i) });
      expect(placeCalls).toBe(1);
    },
  );

  it.each([undefined, "", false] as const)(
    "clears a recorded intent when the final pre-place readSummaryDraft returns %j, then allows a later attach",
    async (badRead) => {
      const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
      const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
      const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
      const revision = await seedSummary(summaryRoot, SOURCE, "Final fence cleanup body");
      let readCalls = 0;
      let placeCalls = 0;
      let failFinalOnce = true;
      const paseo = {
        agents: {
          ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }),
          generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
          placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
            placeCalls++;
            drafts.set(destinationAgentId, draft);
          },
          readSummaryDraft: async (destinationAgentId: string) => {
            readCalls++;
            if (failFinalOnce && readCalls === 2) return badRead as never;
            const stored = drafts.get(destinationAgentId);
            return stored ? { summaryId: stored.summaryId, revision: stored.revision } : null;
          },
        },
      };
      const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
      const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
      const blocked = await service.attach(pin);
      expect(blocked).toMatchObject({ state: "unknown", message: expect.stringContaining(MALFORMED_HOST_DRAFT_READ_PRE) });
      expect(placeCalls).toBe(0);
      expect((await readDestinationAttachState(attachRoot, DEST.id)).intents).toEqual({});
      failFinalOnce = false;
      readCalls = 0;
      const ok = await service.attach(pin);
      expect(ok).toMatchObject({ state: "drafted" });
      expect(placeCalls).toBe(1);
    },
  );

  it("clears a recorded intent when the final pre-place read throws, then allows a later attach", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
    const revision = await seedSummary(summaryRoot, SOURCE, "Final fence throw body");
    let readCalls = 0;
    let placeCalls = 0;
    let failFinalOnce = true;
    const paseo = {
      agents: {
        ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
          placeCalls++;
          drafts.set(destinationAgentId, draft);
        },
        readSummaryDraft: async (destinationAgentId: string) => {
          readCalls++;
          if (failFinalOnce && readCalls === 2) throw new Error("host reader down");
          const stored = drafts.get(destinationAgentId);
          return stored ? { summaryId: stored.summaryId, revision: stored.revision } : null;
        },
      },
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    expect(await service.attach(pin)).toMatchObject({ state: "unknown", message: expect.stringMatching(/Nothing was placed/i) });
    expect(placeCalls).toBe(0);
    expect((await readDestinationAttachState(attachRoot, DEST.id)).intents).toEqual({});
    failFinalOnce = false;
    readCalls = 0;
    expect(await service.attach(pin)).toMatchObject({ state: "drafted" });
    expect(placeCalls).toBe(1);
  });

  it("refuses without placement when a different draft appears on the pre-place occupancy re-read", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const revision = await seedSummary(summaryRoot, SOURCE, "Occupancy race body");
    let readCalls = 0;
    let placeCalls = 0;
    let refreshCalls = 0;
    const other = { summaryId: "00000000-0000-4000-8000-000000000099", revision: 2 };
    const paseo = {
      agents: {
        ref: () => ({
          refresh: async () => {
            refreshCalls++;
            return { agent: DEST, project: null };
          },
        }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async () => { placeCalls++; },
        readSummaryDraft: async () => {
          readCalls++;
          return readCalls === 1 ? null : other;
        },
      },
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    const outcome = await service.attach(pin);
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringMatching(/never replaces a draft/) });
    expect(placeCalls).toBe(0);
    expect(readCalls).toBeGreaterThanOrEqual(2);
    expect(refreshCalls).toBeGreaterThanOrEqual(2);
    expect((await readDestinationAttachState(attachRoot, DEST.id)).intents).toEqual({});
  });

  it("does not overwrite an existing different draft when readSummaryDraft returns undefined", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const existing = { summaryId: "00000000-0000-4000-8000-000000000099", revision: 2, text: "keep me", sourceAgentId: "agent-other" };
    const drafts = new Map<string, typeof existing>([[DEST.id, existing]]);
    const revision = await seedSummary(summaryRoot, SOURCE, "Adversarial attach body");
    const placed: unknown[] = [];
    const paseo = {
      agents: {
        ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }),
        generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
          placed.push({ destinationAgentId, draft });
          drafts.set(destinationAgentId, draft);
        },
        readSummaryDraft: async () => undefined as never,
      },
    };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id });
    expect(placed).toHaveLength(0);
    expect(drafts.get(DEST.id)).toEqual(existing);
  });

  it("concurrent attaches to one destination serialize without splitting drafts", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
    const { paseo, placed } = paseoWithDraft({ [SOURCE.id]: SOURCE, [DEST.id]: DEST }, drafts);
    const revision = await seedSummary(summaryRoot, SOURCE, "Concurrent attach body");
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    const [a, b] = await Promise.all([service.attach(pin), service.attach(pin)]);
    expect(a).toEqual(b);
    expect(placed).toHaveLength(1);
  });
});
