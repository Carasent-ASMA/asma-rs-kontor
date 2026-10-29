import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { attachSummary, readNavigatorCapabilities, UNSUPPORTED, latestSummaries, readSummary, saveSummary, summaryRevisionSchema, type NavigatorCapabilities, type Capability, type SummaryPin, type SummaryRevision, type SummaryHistoryBoundary } from "../shared/session-navigator";
import { searchSummaryAttachments, summaryAttachmentIdentifier } from "../shared/summary-attachments";
import { stockSummaryGenerator } from "./stock-summary-generator";
import { createSummaryAttachService, attachStateRoot } from "./session-summary-attach";
import { readConfig } from "./config";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

export const SUMMARY_MODEL_API_GAP = "This connection does not expose the stock agent creation and history APIs required for summary generation.";
export const SUMMARY_LIMITS_WITH_GENERATION = [
  "Generation runs in a separate read-only Codex session. It never prompts the source or destination; other providers are explicitly unsupported.",
  "Only a captured tail of up to 48 history items / 24,000 message characters is summarized; its epoch, sequence boundary and input hash are saved.",
  "Immutable revisions live in Kontor-owned plugin state. A single plugin worker is the writer; multi-process writers are not qualified.",
  "Clipboard write uses the stock client API. Paste to verify provides explicit readback; write success alone is not readback proof.",
];
const sectionsSchema = summaryRevisionSchema.shape.sections;
export interface CompactSummaryPayload { sections: z.infer<typeof sectionsSchema>; cutoff: string; historyBoundary?: SummaryHistoryBoundary }
export interface SessionSummaryGenerator { generate(agentId: string): Promise<CompactSummaryPayload> }
export function paseoSummaryGenerator(paseo: PaseoApi, root = summaryStateRoot()): SessionSummaryGenerator | undefined {
  return stockSummaryGenerator(paseo, root);
}
export function summariesInstalledCapability(paseo: PaseoApi): Capability {
  return paseoSummaryGenerator(paseo) ? { supported: true, limits: SUMMARY_LIMITS_WITH_GENERATION } : { supported: false, reason: SUMMARY_MODEL_API_GAP };
}

const storeSchema = z.object({
  summaryId: z.string().uuid(),
  sourceAgentId: z.string().min(1),
  sourceTitle: z.string(),
  latestRevision: z.number().int().positive(),
  revisions: z.record(z.string(), summaryRevisionSchema),
}).strict();
type SummaryStore = z.infer<typeof storeSchema>;

const storePath = (root: string, agentId: string) => join(root, `source-${createHash("sha256").update(agentId).digest("hex")}.json`);

/** Serializes read-modify-write per source on this worker; rename alone is not CAS across concurrent writers. */
const sourceStoreTails = new Map<string, Promise<unknown>>();

export function withSourceStoreLock<T>(root: string, agentId: string, work: () => Promise<T>): Promise<T> {
  const key = `${root}\0${agentId}`;
  const tail = sourceStoreTails.get(key) ?? Promise.resolve();
  const run = tail.catch(() => undefined).then(work);
  sourceStoreTails.set(key, run);
  return run.finally(() => {
    if (sourceStoreTails.get(key) === run) sourceStoreTails.delete(key);
  });
}

async function writeStore(root: string, agentId: string, store: SummaryStore) {
  await mkdir(root, { recursive: true, mode: 0o700 });
  const file = storePath(root, agentId), temporary = `${file}.${randomUUID()}.tmp`;
  const output = await open(temporary, "wx", 0o600);
  try { await output.writeFile(JSON.stringify(storeSchema.parse(store))); await output.sync(); } finally { await output.close(); }
  await rename(temporary, file);
}

async function readStore(root: string, agentId: string): Promise<SummaryStore | null> {
  try { return storeSchema.parse(JSON.parse(await readFile(storePath(root, agentId), "utf8"))); }
  catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
}

function formatSummaryText(sections: SummaryRevision["sections"]): string {
  const blocks = [
    ["Task", sections.task],
    ["Decisions", sections.decisions],
    ["Current state", sections.currentState],
    ["Next step", sections.nextStep],
  ].filter(([, body]) => (body ?? "").trim());
  return blocks.map(([heading, body]) => `${heading}\n${body}`).join("\n\n");
}

function contentKey(text: string, cutoff: string) {
  return createHash("sha256").update(JSON.stringify([text, cutoff])).digest("hex");
}

export function createSessionSummaryService(paseo: PaseoApi, root: string, generator: SessionSummaryGenerator | undefined = paseoSummaryGenerator(paseo, root)) {
  async function exact(agentId: string): Promise<Agent | null> {
    const found = await paseo.agents.ref(agentId).refresh();
    return found?.agent.id === agentId ? found.agent : null;
  }

  async function persistRevision(agent: Agent, agentId: string, sections: SummaryRevision["sections"], cutoff: string, savedAt: string, text: string, key: string, historyBoundary?: SummaryHistoryBoundary): Promise<SummaryRevision> {
    return withSourceStoreLock(root, agentId, async () => {
      const existing = await readStore(root, agentId);
      if (existing) {
        const latest = existing.revisions[String(existing.latestRevision)];
        if (latest && contentKey(latest.text, latest.cutoff) === key && JSON.stringify(latest.historyBoundary) === JSON.stringify(historyBoundary)) return latest;
        const revision = existing.latestRevision + 1;
        const next: SummaryRevision = { summaryId: existing.summaryId, revision, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, cutoff, savedAt, sections, text, ...(historyBoundary ? { historyBoundary } : {}) };
        const store: SummaryStore = { ...existing, sourceTitle: agent.title ?? agentId, latestRevision: revision, revisions: { ...existing.revisions, [String(revision)]: next } };
        await writeStore(root, agentId, store);
        return next;
      }
      const summaryId = randomUUID(), revision = 1;
      const created: SummaryRevision = { summaryId, revision, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, cutoff, savedAt, sections, text, ...(historyBoundary ? { historyBoundary } : {}) };
      await writeStore(root, agentId, { summaryId, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, latestRevision: revision, revisions: { "1": created } });
      return created;
    });
  }

  async function save(agentId: string): Promise<SummaryRevision> {
    const agent = await exact(agentId);
    if (!agent) throw new Error("This session is not on this host, so no summary was saved.");
    if (!generator) throw new Error(SUMMARY_MODEL_API_GAP);
    const generated = await generator.generate(agentId);
    const sections = sectionsSchema.parse(generated.sections);
    const cutoff = generated.cutoff.trim();
    if (!cutoff) throw new Error("The host returned no summary cutoff timestamp.");
    const savedAt = new Date().toISOString();
    const text = formatSummaryText(sections);
    return persistRevision(agent, agentId, sections, cutoff, savedAt, text, contentKey(text, cutoff), generated.historyBoundary);
  }

  async function latest(agentIds: string[]): Promise<Record<string, SummaryRevision | null>> {
    const out: Record<string, SummaryRevision | null> = {};
    for (const agentId of agentIds) {
      const store = await readStore(root, agentId);
      out[agentId] = store ? store.revisions[String(store.latestRevision)] ?? null : null;
    }
    return out;
  }

  return { save, latest };
}

export async function readPinnedSummary(root: string, pin: SummaryPin): Promise<SummaryRevision> {
  const { readdir } = await import("node:fs/promises");
  let names: string[];
  try { names = await readdir(root); }
  catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") throw new Error("That summary is not stored on this host."); throw error; }
  for (const name of names) {
    if (!name.startsWith("source-") || !name.endsWith(".json")) continue;
    const store = storeSchema.parse(JSON.parse(await readFile(join(root, name), "utf8")));
    if (store.summaryId !== pin.summaryId) continue;
    const revision = store.revisions[String(pin.revision)];
    if (revision) return revision;
  }
  throw new Error("That summary revision is not stored on this host.");
}

/** Search reads immutable revisions only; the stock picker supplies no destination or selection receipt. */
export async function savedSummaryResources(root: string, query: string) {
  const { readdir } = await import("node:fs/promises");
  let names: string[];
  try { names = await readdir(root); }
  catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return { items: [] }; throw error; }
  const revisions: SummaryRevision[] = [];
  const ids = new Set<string>();
  for (const name of names.sort()) {
    if (!name.startsWith("source-") || !name.endsWith(".json")) continue;
    let store: SummaryStore;
    try { store = storeSchema.parse(JSON.parse(await readFile(join(root, name), "utf8"))); }
    catch { throw new Error("A saved summary is unreadable. No attachment choices were returned."); }
    for (const [key, revision] of Object.entries(store.revisions)) {
      if (key !== String(revision.revision) || ids.has(summaryAttachmentIdentifier(revision.summaryId, revision.revision)) || revision.summaryId !== store.summaryId || revision.sourceAgentId !== store.sourceAgentId || !revision.text.trim() || Object.values(revision.sections).some(text => !text.trim()) || revision.text !== formatSummaryText(revision.sections)) throw new Error("A saved summary is empty or inconsistent. No attachment choices were returned.");
      ids.add(summaryAttachmentIdentifier(revision.summaryId, revision.revision));
      revisions.push(revision);
    }
  }
  const needle = query.trim().toLowerCase();
  const exactPin = /^[a-f0-9-]{36}:r[1-9][0-9]*$/.test(needle);
  const items = revisions.sort((a, b) => b.savedAt.localeCompare(a.savedAt) || b.revision - a.revision || a.summaryId.localeCompare(b.summaryId))
    .filter(revision => exactPin ? summaryAttachmentIdentifier(revision.summaryId, revision.revision).toLowerCase() === needle : `${summaryAttachmentIdentifier(revision.summaryId, revision.revision)} ${revision.sourceTitle} ${revision.sourceAgentId}`.toLowerCase().includes(needle)).slice(0, 50)
    .map(revision => ({ id: summaryAttachmentIdentifier(revision.summaryId, revision.revision), identifier: summaryAttachmentIdentifier(revision.summaryId, revision.revision),
      title: `${revision.sourceTitle} · r${revision.revision}`, subtitle: `Source ${revision.sourceAgentId} · until ${revision.cutoff}`,
      url: `urn:kontor:summary:${revision.summaryId}:${revision.revision}`, text: revision.text, resourceType: "kontor-summary-revision" as const }));
  return { items };
}

export function summaryStateRoot() {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-summaries-v1");
}

/** Standalone R28 capability seam; no attachment, takeover or usage owner is pulled in. */
export function summaryOnlyNavigatorCapabilities(paseo: PaseoApi): NavigatorCapabilities {
  return { ...UNSUPPORTED, summaries: summariesInstalledCapability(paseo) };
}

export function contributeSessionSummaries(server: PluginServerContext, options: {
  root?: string;
  capabilities?: (paseo: PaseoApi) => NavigatorCapabilities;
} = {}) {
  const root = options.root ?? summaryStateRoot();
  const capabilities = options.capabilities ?? summaryOnlyNavigatorCapabilities;
  server.handle(readNavigatorCapabilities, (_input, { paseo }) => capabilities(paseo));
  server.handle(searchSummaryAttachments, ({ query }) => savedSummaryResources(root, query));
  server.handle(saveSummary, (input, { paseo }) => sessionSummaryService(paseo, root).save(input.agentId));
  server.handle(attachSummary, (input, { paseo }) => createSummaryAttachService(paseo, root, attachStateRoot()).attach(input));
  server.handle(readSummary, (input) => readPinnedSummary(root, input));
  server.handle(latestSummaries, (input, { paseo }) => sessionSummaryService(paseo, root).latest(input.agentIds));
}

const sessionSummaryServices = new WeakMap<PaseoApi, ReturnType<typeof createSessionSummaryService>>();

function sessionSummaryService(paseo: PaseoApi, root: string) {
  let service = sessionSummaryServices.get(paseo);
  if (!service) {
    service = createSessionSummaryService(paseo, root);
    sessionSummaryServices.set(paseo, service);
  }
  return service;
}
