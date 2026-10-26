import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { UNSUPPORTED } from "../shared/session-navigator";
import {
  DRAFT_PLACEMENT_API_GAP,
  createSummaryAttachService,
  draftAttachInstalledCapability,
  paseoSummaryDraftPlacer,
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

function paseoWithDraft(agents: Record<string, Agent>, drafts: Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>) {
  const placed: { summaryId: string; revision: number; text: string; destinationAgentId: string }[] = [];
  return {
    paseo: {
      agents: {
        ref: (id: string) => ({ refresh: async () => { const a = agents[id]; return a ? { agent: a, project: null } : null; } }),
        generateCompactSummary: async (agentId: string) => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
        placeSummaryDraft: async (destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }) => {
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

describe("summary attach host route", () => {
  it("keeps draftAttach unsupported with the exact gap when placeSummaryDraft is absent", () => {
    const paseo = { agents: { generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) } } as never;
    expect(paseoSummaryDraftPlacer(paseo)).toBeUndefined();
    expect(draftAttachInstalledCapability(paseo)).toEqual(UNSUPPORTED.draftAttach);
    expect(readInstalledNavigatorCapabilities(paseo).draftAttach).toEqual(UNSUPPORTED.draftAttach);
  });

  it("attach throws the draft placement gap when the SDK route is missing", async () => {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const revision = await seedSummary(summaryRoot, SOURCE, "Pinned summary text");
    const paseo = { agents: { ref: () => ({ refresh: async () => ({ agent: DEST, project: null }) }), generateCompactSummary: async () => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }) } };
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    await expect(service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id })).rejects.toThrow(DRAFT_PLACEMENT_API_GAP);
  });
});

describe("summary attach with qualified SDK", () => {
  async function harness() {
    const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
    const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
    const drafts = new Map<string, { summaryId: string; revision: number; text: string; sourceAgentId: string }>();
    const { paseo, placed } = paseoWithDraft({ [SOURCE.id]: SOURCE, [DEST.id]: DEST }, drafts);
    const revision = await seedSummary(summaryRoot, SOURCE, "Attach this summary body");
    const service = createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
    return { service, summaryRoot, attachRoot, revision, placed, drafts, paseo };
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

  it("returns the same drafted outcome on duplicate attach without a second host place", async () => {
    const { service, revision, placed } = await harness();
    const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
    const first = await service.attach(pin);
    const second = await service.attach(pin);
    expect(second).toEqual(first);
    expect(placed).toHaveLength(1);
  });

  it("refuses when the destination already holds a different draft", async () => {
    const { service, revision, drafts } = await harness();
    drafts.set(DEST.id, { summaryId: "00000000-0000-4000-8000-000000000099", revision: 1, text: "other", sourceAgentId: "agent-other" });
    const outcome = await service.attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringMatching(/never replaces a draft/) });
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
