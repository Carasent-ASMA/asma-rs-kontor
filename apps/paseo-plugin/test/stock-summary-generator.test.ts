import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, describe, expect, it } from "vitest";
import { stockSummaryGenerator, SUMMARY_SOURCE_CHARACTERS } from "../server/stock-summary-generator";
import { contributeSessionSummaries, createSessionSummaryService, readPinnedSummary } from "../server/session-summaries";

import { UNSUPPORTED } from "../shared/session-navigator";
import type { PluginServerContext } from "@getpaseo/plugin/server";

const SECTIONS = { task: "Review the parser", decisions: "Keep the grammar", currentState: "Ready", nextStep: "Review" };
const SOURCE = "00000000-0000-4000-8000-000000000001";
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

/** In-memory stock API adapter, not a real provider or installed-host qualification. Files remain real. */
async function harness() {
  const root = await mkdtemp(join(tmpdir(), "stock-summary-")); dirs.push(root);
  const source = { id: SOURCE, title: "Parser", provider: "codex", model: "model-a", status: "idle", activeTurn: null, persistence: { sessionId: "source-session" } };
  const page = { agent: { ...source }, agentId: SOURCE, error: null, epoch: "epoch-1", gap: false, staleCursor: false, hasNewer: false, hasOlder: true,
    entries: [{ seqStart: 7, seqEnd: 7, timestamp: "2026-10-28T10:00:00Z", item: { type: "user_message", text: "Review the parser" } },
      { seqStart: 8, seqEnd: 8, timestamp: "2026-10-28T10:01:00Z", item: { type: "assistant_message", text: "Keep the grammar" } }] };
  const created: Array<Record<string, any>> = [];
  const historyCalls: unknown[] = [];
  const workers = new Set<string>();
  let result = { status: "idle", lastMessage: JSON.stringify(SECTIONS), error: null, final: null };
  let waits = 0, sends = 0, refuseCreate = false;
  let foreignTurn = false, missingCorrelation = false, workerGap = false;
  const workerOverrides: Record<string, unknown> = {};
  const workerAgent = (id: string) => {
    const options = created.find(item => item.agentId === id)!;
    return { id, provider: "codex", model: "model-a", cwd: options.cwd, currentModeId: "read-only", status: result.status === "timeout" || result.status === "permission" ? "running" : result.status, activeTurn: null, persistence: { sessionId: "worker-session" }, labels: options.labels, ...workerOverrides };
  };
  const forbidden = async () => { sends++; throw new Error("Must not send a source/destination turn"); };
  const api = { agents: {
    ref: (id: string) => id === SOURCE ? {
      refresh: async () => ({ agent: source }), timeline: { refetch: async (options: unknown) => { historyCalls.push(options); return page; } }, run: forbidden, send: forbidden,
    } : {
      id, refresh: async () => workers.has(id) ? { agent: workerAgent(id) } : null,
      timeline: { refetch: async () => {
        const options = created.find(item => item.agentId === id)!;
        return { agentId: id, agent: workerAgent(id), epoch: "worker-epoch", error: null, gap: workerGap, staleCursor: false, hasNewer: false, hasOlder: false,
          entries: [{ seqStart: 1, seqEnd: 1, item: { type: "user_message", text: options.prompt, clientMessageId: missingCorrelation ? undefined : options.clientMessageId } },
            ...(foreignTurn ? [{ seqStart: 3, seqEnd: 3, item: { type: "user_message", text: "Unrelated payroll task", clientMessageId: "foreign-turn" } }] : []),
            { seqStart: 4, seqEnd: 4, item: { type: "assistant_message", text: result.lastMessage } }] };
      } },
      waitForFinish: async () => { waits++; return result; }, run: forbidden, send: forbidden,
    },
    create: async (options: Record<string, any>) => { created.push(options); workers.add(options.agentId); if (refuseCreate) { refuseCreate = false; throw new Error("Creation answer lost"); } return api.agents.ref(options.agentId); },
  } };
  const generator = () => stockSummaryGenerator(api as never, root)!;
  return { root, api, source, page, created, historyCalls, generator, counts: () => ({ waits, sends }), setResult: (next: Partial<typeof result>) => { result = { ...result, ...next }; }, loseCreateAnswer: () => { refuseCreate = true; }, foreignTurn: () => { foreignTurn = true; }, missingCorrelation: () => { missingCorrelation = true; }, workerGap: () => { workerGap = true; }, workerOverrides };
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

  it("holds unrelated later valid JSON after timeout across service restart without resending", async () => {
    const h = await harness(); const service = createSessionSummaryService(h.api as never, h.root);
    h.setResult({ status: "timeout" });
    await expect(service.save(SOURCE)).rejects.toThrow("retry reads its result");
    h.foreignTurn(); h.setResult({ status: "idle", lastMessage: JSON.stringify({ ...SECTIONS, task: "Unrelated payroll job" }) });
    const resumed = createSessionSummaryService(h.api as never, h.root);
    await expect(resumed.save(SOURCE)).rejects.toThrow("correlation is unconfirmed");
    expect(await resumed.latest([SOURCE])).toEqual({ [SOURCE]: null });
    expect(h.created).toHaveLength(1);
    expect(h.counts().sends).toBe(0);
  });

  it("persists only the same issued prompt response after timeout and service restart", async () => {
    const h = await harness(); h.setResult({ status: "timeout" });
    await expect(createSessionSummaryService(h.api as never, h.root).save(SOURCE)).rejects.toThrow("retry reads its result");
    h.setResult({ status: "idle" });
    const saved = await createSessionSummaryService(h.api as never, h.root).save(SOURCE);
    expect(saved.sections).toEqual(SECTIONS);
    expect(h.created).toHaveLength(1);
    expect(h.created[0]!.clientMessageId).toMatch(/^[a-f0-9-]{36}$/);
  });

  it.each(["missing prompt identity", "history gap", "different effective provider", "different effective mode", "changed prompt"])("holds output with %s", async reason => {
    const h = await harness();
    if (reason === "changed prompt") {
      h.setResult({ status: "timeout" });
      await expect(h.generator().generate(SOURCE)).rejects.toThrow("retry reads its result");
      h.created[0]!.prompt = "A different prompt"; h.setResult({ status: "idle" });
    }
    if (reason === "missing prompt identity") h.missingCorrelation();
    if (reason === "history gap") h.workerGap();
    if (reason === "different effective provider") h.workerOverrides.provider = "opencode";
    if (reason === "different effective mode") h.workerOverrides.currentModeId = "full-access";
    const service = createSessionSummaryService(h.api as never, h.root);
    await expect(service.save(SOURCE)).rejects.toThrow("correlation is unconfirmed");
    expect(await service.latest([SOURCE])).toEqual({ [SOURCE]: null });
    expect(h.created).toHaveLength(1);
  });

  it("holds a replaced worker provider session after timeout", async () => {
    const h = await harness(); h.setResult({ status: "timeout" });
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("retry reads its result");
    h.setResult({ status: "idle" }); h.workerOverrides.persistence = { sessionId: "replacement-worker-session" };
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("provider session changed");
    expect(h.created).toHaveLength(1);
  });

  it("refuses same-agent history from a replacement source provider session before creating a worker", async () => {
    const h = await harness(); h.page.agent.persistence = { sessionId: "replacement-source-session" };
    const service = createSessionSummaryService(h.api as never, h.root);
    await expect(service.save(SOURCE)).rejects.toThrow("source provider session changed");
    expect(h.created).toEqual([]);
    expect(await service.latest([SOURCE])).toEqual({ [SOURCE]: null });
  });


  it("holds an older job without persisted prompt correlation instead of accepting or resending", async () => {
    const h = await harness(); h.setResult({ status: "timeout" });
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("retry reads its result");
    const jobs = join(h.root, "generation-jobs");
    const file = join(jobs, (await readdir(jobs)).find(name => name.endsWith(".json"))!);
    const job = JSON.parse(await readFile(file, "utf8")); delete job.clientMessageId; delete job.promptHash;
    await writeFile(file, JSON.stringify(job));
    h.setResult({ status: "idle" });
    await expect(h.generator().generate(SOURCE)).rejects.toThrow("no matching prompt correlation");
    expect(h.created).toHaveLength(1);
  });


  it("registers the R28-only capability/save/read/latest closure and persists through its RPCs", async () => {
    const h = await harness();
    const handlers = new Map<string, (...args: any[]) => any>();
    contributeSessionSummaries({ handle: (rpc: { name: string }, handler: (...args: any[]) => any) => {
      expect(handlers.has(rpc.name)).toBe(false); handlers.set(rpc.name, handler);
    } } as unknown as PluginServerContext, { root: h.root });
    expect([...handlers.keys()].sort()).toEqual(["sessions.navigator-capabilities", "sessions.summary-latest", "sessions.summary-read", "sessions.summary-save"].sort());
    const context = { paseo: h.api };
    const caps = handlers.get("sessions.navigator-capabilities")!({}, context);
    expect(caps).toMatchObject({ summaries: { supported: true }, usageTotals: UNSUPPORTED.usageTotals, draftAttach: UNSUPPORTED.draftAttach,
      takeover: UNSUPPORTED.takeover, takeoverBulk: UNSUPPORTED.takeoverBulk, takeoverRoute: UNSUPPORTED.takeoverRoute });
    expect(h.created).toHaveLength(0);
    const saved = await handlers.get("sessions.summary-save")!({ agentId: SOURCE }, context);
    expect(saved.historyBoundary.providerSessionId).toBe("source-session");
    expect(await handlers.get("sessions.summary-read")!({ summaryId: saved.summaryId, revision: saved.revision }, context)).toEqual(saved);
    expect(await handlers.get("sessions.summary-latest")!({ agentIds: [SOURCE] }, context)).toEqual({ [SOURCE]: saved });
    expect(h.created).toHaveLength(1);
    expect(h.counts().sends).toBe(0);
  });

});
