import { mkdtemp, readdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, describe, expect, it } from "vitest";
import { stockSummaryGenerator, SUMMARY_SOURCE_CHARACTERS } from "../server/stock-summary-generator";
import { createSessionSummaryService, readPinnedSummary } from "../server/session-summaries";

const SECTIONS = { task: "Review the parser", decisions: "Keep the grammar", currentState: "Ready", nextStep: "Review" };
const SOURCE = "00000000-0000-4000-8000-000000000001";
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

/** In-memory stock API adapter, not a real provider or installed-host qualification. Files remain real. */
async function harness() {
  const root = await mkdtemp(join(tmpdir(), "stock-summary-")); dirs.push(root);
  const source = { id: SOURCE, title: "Parser", provider: "codex", model: "model-a", status: "idle", activeTurn: null, persistence: { sessionId: "source-session" } };
  const page = { agentId: SOURCE, error: null, epoch: "epoch-1", gap: false, staleCursor: false, hasNewer: false, hasOlder: true,
    entries: [{ seqStart: 7, seqEnd: 7, timestamp: "2026-10-28T10:00:00Z", item: { type: "user_message", text: "Review the parser" } },
      { seqStart: 8, seqEnd: 8, timestamp: "2026-10-28T10:01:00Z", item: { type: "assistant_message", text: "Keep the grammar" } }] };
  const created: Array<Record<string, any>> = [];
  const historyCalls: unknown[] = [];
  const workers = new Set<string>();
  let result = { status: "idle", lastMessage: JSON.stringify(SECTIONS), error: null, final: null };
  let waits = 0, sends = 0, refuseCreate = false;
  const forbidden = async () => { sends++; throw new Error("Must not send a source/destination turn"); };
  const api = { agents: {
    ref: (id: string) => id === SOURCE ? {
      refresh: async () => ({ agent: source }), timeline: { refetch: async (options: unknown) => { historyCalls.push(options); return page; } }, run: forbidden, send: forbidden,
    } : {
      id, refresh: async () => workers.has(id) ? { agent: { id } } : null,
      waitForFinish: async () => { waits++; return result; }, run: forbidden, send: forbidden,
    },
    create: async (options: Record<string, any>) => { created.push(options); workers.add(options.agentId); if (refuseCreate) { refuseCreate = false; throw new Error("Creation answer lost"); } return api.agents.ref(options.agentId); },
  } };
  const generator = () => stockSummaryGenerator(api as never, root)!;
  return { root, api, source, page, created, historyCalls, generator, counts: () => ({ waits, sends }), setResult: (next: Partial<typeof result>) => { result = { ...result, ...next }; }, loseCreateAnswer: () => { refuseCreate = true; } };
}

describe("stock summary generator", () => {
  it("uses stock structured creation in a separate read-only worker, saves the exact source boundary and reopens the same revision", async () => {
    const h = await harness();
    const service = createSessionSummaryService(h.api as never, h.root);
    const saved = await service.save(SOURCE);
    expect(saved).toMatchObject({ sourceAgentId: SOURCE, revision: 1, cutoff: "2026-10-28T10:01:00Z", sections: SECTIONS,
      historyBoundary: { epoch: "epoch-1", fromSeq: 7, toSeq: 8, partial: true, providerSessionId: "source-session" } });
    expect(saved.historyBoundary?.snapshotHash).toMatch(/^[a-f0-9]{64}$/);
    expect(h.historyCalls).toEqual([{ direction: "tail", projection: "canonical", limit: 48 }]);
    expect(h.created).toHaveLength(1);
    expect(h.created[0]).toMatchObject({ config: { provider: "codex/model-a", modeId: "read-only" }, outputSchema: { required: ["task", "decisions", "currentState", "nextStep"] } });
    expect(h.created[0]!.agentId).not.toBe(SOURCE);
    expect(h.created[0]!.prompt).toContain('"sourceAgentId":"' + SOURCE + '"');
    expect(h.counts()).toEqual({ waits: 1, sends: 0 });
    const reopened = createSessionSummaryService(h.api as never, h.root);
    expect(await reopened.save(SOURCE)).toEqual(saved);
    expect(await readPinnedSummary(h.root, saved)).toEqual(saved);
    expect(h.created).toHaveLength(1);
    expect(await readdir(join(h.root, "generation-jobs"))).toContainEqual(expect.stringMatching(/\.json$/));
  });

  it("bounds message characters even for one oversized row", async () => {
    const h = await harness(); h.page.entries[1]!.item.text = "X".repeat(SUMMARY_SOURCE_CHARACTERS + 1000);
    const result = await h.generator().generate(SOURCE);
    const excerpt = JSON.parse(h.created[0]!.prompt.split("\n\n").at(-1));
    expect(excerpt).toEqual([{ seqStart: 8, seqEnd: 8, role: "assistant_message", text: "X".repeat(SUMMARY_SOURCE_CHARACTERS) }]);
    expect(result.historyBoundary?.fromSeq).toBe(8);
  });

  it("refuses another source returned by refresh without creating a worker", async () => {
    const h = await harness(); h.source.id = "another-source";
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("exact source");
    expect(h.created).toEqual([]);
  });

  it.each(["claude", "opencode"])("truthfully refuses unqualified provider %s", async provider => {
    const h = await harness(); h.source.provider = provider;
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("Codex sessions only");
    expect(h.created).toEqual([]);
  });

  it("refuses a running source and a history gap before generation", async () => {
    const h = await harness(); h.source.status = "running";
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("still working");
    h.source.status = "idle"; h.page.gap = true;
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("consistently");
    expect(h.created).toEqual([]);
  });

  it("recovers an uncertain creation answer using the recorded worker, never a second prompt", async () => {
    const h = await harness(); h.loseCreateAnswer();
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("Creation answer lost");
    expect((await h.generator().generate(SOURCE)).sections).toEqual(SECTIONS);
    expect(h.created).toHaveLength(1);
    expect(h.counts().sends).toBe(0);
  });

  it("rechecks a timed-out worker on retry and only persists a completed result", async () => {
    const h = await harness(); h.setResult({ status: "timeout" });
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("retry reads its result");
    h.setResult({ status: "idle" });
    expect((await h.generator().generate(SOURCE)).sections).toEqual(SECTIONS);
    expect(h.created).toHaveLength(1);
    expect(h.counts()).toEqual({ waits: 2, sends: 0 });
  });

  it("does not save malformed provider output and permits an explicit retry after a terminal failure", async () => {
    const h = await harness(); h.setResult({ lastMessage: "{}" });
    const service = createSessionSummaryService(h.api as never, h.root);
    await expect(service.save(SOURCE)).rejects.toThrow("four nonempty");
    expect(await service.latest([SOURCE])).toEqual({ [SOURCE]: null });
    h.setResult({ lastMessage: JSON.stringify(SECTIONS) });
    expect((await service.save(SOURCE)).revision).toBe(1);
    expect(h.created).toHaveLength(2);
  });

  it("a new epoch makes a new revision without replacing the old saved pin", async () => {
    const h = await harness(); const service = createSessionSummaryService(h.api as never, h.root);
    const first = await service.save(SOURCE);
    h.page.epoch = "epoch-2";
    const second = await service.save(SOURCE);
    expect(second.revision).toBe(2);
    expect(await readPinnedSummary(h.root, first)).toEqual(first);
  });
});
