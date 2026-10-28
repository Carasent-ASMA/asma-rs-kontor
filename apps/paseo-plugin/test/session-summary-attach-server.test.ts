import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import {
  DRAFT_ATTACH_API_GAP,
  DRAFT_READ_API_GAP,
  createSummaryAttachService,
  draftAttachInstalledCapability,
  parseHostSummaryDraftRead,
  paseoSummaryDraftAttacher,
  readDestinationAttachState,
} from "../server/session-summary-attach";
import { createSessionSummaryService, readPinnedSummary } from "../server/session-summaries";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

const SECTIONS = { task: "Fix the parser", decisions: "Use ArkType", currentState: "Tests green", nextStep: "Open PR" };
const SOURCE = { id: "00000000-0000-4000-8000-000000000001", title: "Route review", updatedAt: "2026-10-25T10:00:00Z" } as Agent;
const DEST = { id: "00000000-0000-4000-8000-000000000002", title: "Billing", updatedAt: "2026-10-25T10:00:00Z", status: "idle" } as Agent;

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

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

type Draft = { summaryId: string; revision: number; text: string; sourceAgentId: string };
const OTHER: Draft = { summaryId: "00000000-0000-4000-8000-000000000099", revision: 4, text: "Existing task context", sourceAgentId: SOURCE.id };

async function harness(patch: Partial<Agent> = {}) {
  const summaryRoot = await mkdtemp(join(tmpdir(), "kontor-attach-")); dirs.push(summaryRoot);
  const attachRoot = await mkdtemp(join(tmpdir(), "kontor-attach-meta-")); dirs.push(attachRoot);
  const agents: Record<string, Agent> = { [SOURCE.id]: { ...SOURCE }, [DEST.id]: { ...DEST, ...patch } };
  const drafts = new Map<string, Draft>();
  const calls: { id: string; draft: Draft }[] = [];
  const paseo = {
    supportsHostFeature: (_feature: string) => true,
    agents: {
      create: async () => { throw new Error("not used by legacy attach fixture"); },
      ref: (id: string) => ({ refresh: async () => agents[id] ? { agent: agents[id], project: null } : null }),
      generateCompactSummary: async (_agentId: string) => ({ sections: SECTIONS, cutoff: "2026-10-25T10:00:00Z" }),
      attachSummaryDraft: async (id: string, draft: Draft): Promise<unknown> => {
        calls.push({ id, draft });
        const old = drafts.get(id);
        if (old) return { state: old.summaryId === draft.summaryId && old.revision === draft.revision ? "already" : "refused", draft: { summaryId: old.summaryId, revision: old.revision } };
        drafts.set(id, draft);
        return { state: "drafted", draft: { summaryId: draft.summaryId, revision: draft.revision } };
      },
      readSummaryDraft: async (id: string): Promise<unknown> => {
        const draft = drafts.get(id);
        return draft ? { summaryId: draft.summaryId, revision: draft.revision } : null;
      },
    },
  };
  const revision = await createSessionSummaryService(paseo as never, summaryRoot, { generate: paseo.agents.generateCompactSummary }).save(SOURCE.id);
  const pin = { summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: DEST.id };
  const service = () => createSummaryAttachService(paseo as never, summaryRoot, attachRoot);
  return { paseo, revision, pin, summaryRoot, attachRoot, service, drafts, calls, agents };
}

describe("atomic summary attach capability", () => {
  it("requires the advertised host feature and helper, with no legacy placement fallback", async () => {
    const h = await harness();
    const legacy = { ...h.paseo, agents: { ...h.paseo.agents, attachSummaryDraft: undefined, placeSummaryDraft: async () => { throw new Error("legacy must not be used"); } } };
    expect(paseoSummaryDraftAttacher(legacy as never)).toBeUndefined();
    expect(draftAttachInstalledCapability(legacy as never)).toEqual({ supported: false, reason: DRAFT_ATTACH_API_GAP });
    await expect(createSummaryAttachService(legacy as never, h.summaryRoot, h.attachRoot).attach(h.pin)).rejects.toThrow(DRAFT_ATTACH_API_GAP);
    h.paseo.supportsHostFeature = feature => feature !== "agentSummaryDraft";
    expect(draftAttachInstalledCapability(h.paseo as never)).toEqual({ supported: false, reason: DRAFT_ATTACH_API_GAP });
    expect(readInstalledNavigatorCapabilities(h.paseo as never).draftAttach.supported).toBe(false);
  });

  it("requires host readback, and exposes atomic no-replace limits only when qualified", async () => {
    const h = await harness();
    expect(draftAttachInstalledCapability(h.paseo as never)).toMatchObject({ supported: true });
    const noRead = { ...h.paseo, agents: { ...h.paseo.agents, readSummaryDraft: undefined } };
    expect(draftAttachInstalledCapability(noRead as never)).toEqual({ supported: false, reason: DRAFT_READ_API_GAP });
    await expect(createSummaryAttachService(noRead as never, h.summaryRoot, h.attachRoot).attach(h.pin)).rejects.toThrow(DRAFT_READ_API_GAP);
    expect(h.calls).toHaveLength(0);
  });
});

describe("one-shot atomic saved-revision attachment", () => {
  it("attaches the exact saved revision once, with no prompt or source modification", async () => {
    const h = await harness();
    h.paseo.agents.generateCompactSummary = async () => ({ sections: { ...SECTIONS, nextStep: "Newer next step" }, cutoff: "2026-10-25T11:00:00Z" });
    const newer = await createSessionSummaryService(h.paseo as never, h.summaryRoot, { generate: h.paseo.agents.generateCompactSummary }).save(SOURCE.id);
    expect(newer.revision).toBe(2);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "drafted", ...h.pin });
    expect(h.calls).toEqual([{ id: DEST.id, draft: { summaryId: h.revision.summaryId, revision: 1, text: h.revision.text, sourceAgentId: SOURCE.id } }]);
    expect(await readPinnedSummary(h.summaryRoot, h.pin)).toEqual(h.revision);
    expect(h.agents[DEST.id]).toEqual(DEST);
  });

  it("recognizes a host already result and confirms its exact pin by readback", async () => {
    const h = await harness();
    h.paseo.agents.attachSummaryDraft = async (id, draft) => {
      h.calls.push({ id, draft }); h.drafts.set(id, draft);
      return { state: "already", draft: { summaryId: draft.summaryId, revision: draft.revision } };
    };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "drafted", message: expect.stringContaining("already") });
    expect(h.calls).toHaveLength(1);
  });

  it("preserves an occupied pin, including occupancy that appears during atomic attachment", async () => {
    const h = await harness();
    h.paseo.agents.attachSummaryDraft = async (id, draft) => {
      h.calls.push({ id, draft }); h.drafts.set(id, OTHER);
      return { state: "refused", draft: { summaryId: OTHER.summaryId, revision: OTHER.revision } };
    };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused", message: expect.stringContaining(`${OTHER.summaryId} r4`) });
    expect(h.drafts.get(DEST.id)).toEqual(OTHER);
    expect((await readDestinationAttachState(h.attachRoot, DEST.id)).intents).toEqual({});
  });

  it("shows an occupied pin even when the accepted SDK throws for a host refusal", async () => {
    const h = await harness();
    h.paseo.agents.attachSummaryDraft = async (id, draft) => {
      h.calls.push({ id, draft }); h.drafts.set(id, OTHER); throw new Error("host occupied refusal");
    };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown", message: expect.stringContaining(`${OTHER.summaryId} r4`) });
    expect(h.drafts.get(DEST.id)).toEqual(OTHER);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(h.calls).toHaveLength(1);
  });

  it("never dispatches when the initial host read already shows another draft", async () => {
    const h = await harness(); h.drafts.set(DEST.id, OTHER);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused" });
    expect(h.calls).toHaveLength(0); expect(h.drafts.get(DEST.id)).toEqual(OTHER);
  });

  it.each([
    { status: "running" }, { status: "initializing" }, { activeTurn: { turnId: "turn", startedAt: "now" } }, { archivedAt: "2026-10-25T10:00:00Z" },
  ] as Partial<Agent>[])("validates destination state before any attach: %j", async patch => {
    const h = await harness(patch);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused" }); expect(h.calls).toHaveLength(0);
  });

  it("refuses the source, missing destination and missing saved revision", async () => {
    const h = await harness();
    expect(await h.service().attach({ ...h.pin, destinationAgentId: SOURCE.id })).toMatchObject({ state: "refused" });
    delete h.agents[DEST.id];
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused" });
    expect(await h.service().attach({ ...h.pin, revision: 99 })).toMatchObject({ state: "refused" });
    expect(h.calls).toHaveLength(0);
  });

  it("revalidates destination identity and busy state immediately before dispatch", async () => {
    const h = await harness(); let refreshes = 0;
    h.paseo.agents.ref = id => ({ refresh: async () => ({ agent: { ...h.agents[id]!, ...( ++refreshes >= 3 ? { status: "running" as const } : {}) }, project: null }) });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused" });
    expect(h.calls).toHaveLength(0);
    expect((await readDestinationAttachState(h.attachRoot, DEST.id)).intents).toEqual({});
    h.paseo.agents.ref = () => ({ refresh: async () => ({ agent: { ...DEST, id: SOURCE.id }, project: null }) });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused" });
    expect(h.calls).toHaveLength(0);
  });

  it("refuses a saved record whose embedded revision no longer matches the exact pin", async () => {
    const h = await harness();
    const path = join(h.summaryRoot, `source-${createHash("sha256").update(SOURCE.id).digest("hex")}.json`);
    const store = JSON.parse(await readFile(path, "utf8")); store.revisions["1"].revision = 9;
    await writeFile(path, JSON.stringify(store));
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused", message: expect.stringContaining("does not match") });
    expect(h.calls).toHaveLength(0);
  });

  it("shows a definitive host refusal without an occupied pin", async () => {
    const h = await harness(); h.paseo.agents.attachSummaryDraft = async () => ({ state: "refused", draft: null });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "refused", message: expect.stringContaining("existing work was preserved") });
    expect((await readDestinationAttachState(h.attachRoot, DEST.id)).intents).toEqual({});
  });

  it("keeps concurrent and restarted duplicate requests one-shot and rechecks persisted success", async () => {
    const h = await harness();
    const outcomes = await Promise.all([h.service().attach(h.pin), h.service().attach(h.pin)]);
    expect(outcomes.map(value => value.state)).toEqual(["drafted", "drafted"]);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "drafted", message: expect.stringContaining("already") });
    expect(h.calls).toHaveLength(1);
    h.drafts.delete(DEST.id);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(h.calls).toHaveLength(1);
  });

  it.each(["reply-failure", "confirmed-success"] as const)("reconciles %s after restart even when destination refresh is null", async history => {
    const h = await harness(); const attach = h.paseo.agents.attachSummaryDraft, read = h.paseo.agents.readSummaryDraft;
    if (history === "reply-failure") {
      h.paseo.agents.attachSummaryDraft = async (id, draft) => { await attach(id, draft); throw new Error("reply lost"); };
      h.paseo.agents.readSummaryDraft = async id => { if (h.calls.length) throw new Error("readback lost"); return read(id); };
    }
    expect((await h.service().attach(h.pin)).state).toBe(history === "reply-failure" ? "unknown" : "drafted");
    delete h.agents[DEST.id]; h.paseo.agents.readSummaryDraft = read;
    const resumed = await h.service().attach(h.pin);
    expect(resumed).toMatchObject({ state: "drafted", message: expect.stringContaining(`${h.pin.summaryId} r${h.pin.revision}`) });
    expect(resumed.message).toContain("already"); expect(resumed.message).not.toMatch(/nothing was placed|No session/);
    expect(h.calls).toHaveLength(1); expect(h.drafts.get(DEST.id)?.text).toBe(h.revision.text);
  });

  it.each(["reply-failure", "confirmed-success"] as const)("keeps %s history unknown with an occupied pin when the destination disappears", async history => {
    const h = await harness(); const attach = h.paseo.agents.attachSummaryDraft;
    if (history === "reply-failure") h.paseo.agents.attachSummaryDraft = async (id, draft) => { await attach(id, draft); throw new Error("reply lost"); };
    await h.service().attach(h.pin); delete h.agents[DEST.id]; h.drafts.set(DEST.id, OTHER);
    const resumed = await h.service().attach(h.pin);
    expect(resumed).toMatchObject({ state: "unknown", message: expect.stringContaining(`${OTHER.summaryId} r${OTHER.revision}`) });
    expect(resumed.message).not.toMatch(/nothing was placed|No session/);
    expect(h.calls).toHaveLength(1); expect(h.drafts.get(DEST.id)).toEqual(OTHER);
  });

  it.each(["unreadable", "empty"] as const)("retains durable uncertainty when destination refresh is null and host readback is %s", async readback => {
    const h = await harness(); await h.service().attach(h.pin); delete h.agents[DEST.id];
    h.paseo.agents.readSummaryDraft = async () => { if (readback === "unreadable") throw new Error("readback lost"); return null; };
    const resumed = await h.service().attach(h.pin);
    expect(resumed.state).toBe("unknown"); expect(resumed.message).toContain(h.pin.summaryId);
    expect(resumed.message).not.toMatch(/nothing was placed|No session/); expect(h.calls).toHaveLength(1);
  });

  it("retains an uncertain intent after host failure and never automatically reattaches after restart", async () => {
    const h = await harness();
    h.paseo.agents.attachSummaryDraft = async (id, draft) => { h.calls.push({ id, draft }); throw new Error("host refused or transport disconnected"); };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(h.calls).toHaveLength(1);
    expect(Object.keys((await readDestinationAttachState(h.attachRoot, DEST.id)).intents)).toHaveLength(1);
  });

  it("does not trust success without readback; restart reconciles it without another attach", async () => {
    const h = await harness(); const read = h.paseo.agents.readSummaryDraft;
    h.paseo.agents.readSummaryDraft = async id => { if (h.calls.length) throw new Error("readback failed"); return read(id); };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    h.paseo.agents.readSummaryDraft = read;
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "drafted" });
    expect(h.calls).toHaveLength(1);
  });

  it.each([undefined, { summaryId: OTHER.summaryId, revision: OTHER.revision }])("keeps missing or mismatched post-attach readback unknown: %j", async result => {
    const h = await harness(); const read = h.paseo.agents.readSummaryDraft;
    h.paseo.agents.readSummaryDraft = async id => h.calls.length ? result : read(id);
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(h.calls).toHaveLength(1);
  });

  it.each(["initial", "pre-attach", "post-attach", "restart", "destination-refresh"] as const)("redacts malformed-store host read errors during %s", async phase => {
    const h = await harness(); const read = h.paseo.agents.readSummaryDraft; let reads = 0;
    const failure = new Error(`Invalid summary-drafts-v1 JSON at /synthetic/private-store: body preview AUD_SECRET ${"private-body".repeat(10_000)}`);
    if (phase === "restart") await h.service().attach(h.pin);
    if (phase === "destination-refresh") h.paseo.agents.ref = () => ({ refresh: async () => { throw failure; } });
    else h.paseo.agents.readSummaryDraft = async id => {
      reads++;
      if (phase === "initial" || phase === "restart" || phase === "pre-attach" && reads >= 2 || phase === "post-attach" && h.calls.length) throw failure;
      return read(id);
    };
    const result = await h.service().attach(h.pin);
    expect(result.state).toBe("unknown");
    expect(result.message).not.toMatch(/AUD_SECRET|private-body|body preview|private-store|Invalid summary-drafts-v1/);
    expect(result.message.length).toBeLessThan(600);
    expect(h.calls).toHaveLength(phase === "post-attach" || phase === "restart" ? 1 : 0);
    if (phase === "post-attach" || phase === "restart") {
      const again = await h.service().attach(h.pin); expect(again.state).toBe("unknown");
      expect(again.message).not.toContain("AUD_SECRET"); expect(h.calls).toHaveLength(1);
    }
  });

  it.each([undefined, false, { state: "drafted", draft: null }, { state: "already", draft: { summaryId: OTHER.summaryId, revision: 4 } }])("keeps malformed or mismatched attach confirmation unknown: %j", async result => {
    const h = await harness();
    h.paseo.agents.attachSummaryDraft = async (id, draft) => { h.calls.push({ id, draft }); return result; };
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" });
    expect(h.calls).toHaveLength(1);
  });

  it.each([undefined, false, { revision: 1 }])("never treats malformed host draft read %j as empty", async raw => {
    const h = await harness(); h.paseo.agents.readSummaryDraft = async () => raw;
    expect(await h.service().attach(h.pin)).toMatchObject({ state: "unknown" }); expect(h.calls).toHaveLength(0);
  });
});
