import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { attachSummary, latestSummaries, readSummary, saveSummary, summaryRevisionSchema, type SummaryPin, type SummaryRevision } from "../shared/session-navigator";
import { readConfig } from "./config";
import { sourceSummary, type CreateClient } from "./session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

export const SUMMARY_LIMITS = [
  "Each revision is generated from the source session's last assistant handoff message read through the host SDK timeline.",
  "Revisions persist under the plugin state root and are read back by exact summaryId and revision number.",
  "Attach summary to another seat's draft is not installed on this host yet.",
];

const storeSchema = z.object({
  summaryId: z.string().uuid(),
  sourceAgentId: z.string().min(1),
  sourceTitle: z.string(),
  latestRevision: z.number().int().positive(),
  revisions: z.record(z.string(), summaryRevisionSchema),
}).strict();
type SummaryStore = z.infer<typeof storeSchema>;

const storePath = (root: string, agentId: string) => join(root, `source-${createHash("sha256").update(agentId).digest("hex")}.json`);

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

function sectionsFromHandoff(text: string): SummaryRevision["sections"] {
  return { task: text, decisions: "", currentState: "", nextStep: "" };
}

async function sourceCutoff(client: CreateClient, agentId: string, fallback: string): Promise<string> {
  const page = await client.agents.ref(agentId).timeline.refetch({ limit: 100, direction: "tail" });
  if ("error" in page && page.error) throw new Error("The source timeline could not be read for a summary cutoff.");
  for (let index = page.entries.length - 1; index >= 0; index--) {
    const item = page.entries[index]!.item as { createdAt?: string };
    if (item.createdAt?.trim()) return item.createdAt;
  }
  return fallback;
}

function contentKey(text: string, cutoff: string) {
  return createHash("sha256").update(JSON.stringify([text, cutoff])).digest("hex");
}

export function createSessionSummaryService(client: CreateClient, root: string) {
  async function exact(agentId: string): Promise<Agent | null> {
    const found = await client.agents.ref(agentId).refresh();
    return found?.agent.id === agentId ? found.agent : null;
  }

  async function save(agentId: string): Promise<SummaryRevision> {
    const agent = await exact(agentId);
    if (!agent) throw new Error("This session is not on this host, so no summary was saved.");
    const handoff = await sourceSummary(client, agentId);
    if (!handoff) throw new Error("This session has no assistant handoff message to summarize yet.");
    const cutoff = await sourceCutoff(client, agentId, agent.updatedAt ?? new Date().toISOString());
    const savedAt = new Date().toISOString();
    const sections = sectionsFromHandoff(handoff);
    const text = formatSummaryText(sections);
    const key = contentKey(text, cutoff);
    const existing = await readStore(root, agentId);
    if (existing) {
      const latest = existing.revisions[String(existing.latestRevision)];
      if (latest && contentKey(latest.text, latest.cutoff) === key) return latest;
      const revision = existing.latestRevision + 1;
      const next: SummaryRevision = { summaryId: existing.summaryId, revision, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, cutoff, savedAt, sections, text };
      const store: SummaryStore = { ...existing, sourceTitle: agent.title ?? agentId, latestRevision: revision, revisions: { ...existing.revisions, [String(revision)]: next } };
      await writeStore(root, agentId, store);
      return next;
    }
    const summaryId = randomUUID(), revision = 1;
    const created: SummaryRevision = { summaryId, revision, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, cutoff, savedAt, sections, text };
    await writeStore(root, agentId, { summaryId, sourceAgentId: agentId, sourceTitle: agent.title ?? agentId, latestRevision: revision, revisions: { "1": created } });
    return created;
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

/** Read pin by scanning stores (bounded file count per host). */
async function readPinned(root: string, pin: SummaryPin): Promise<SummaryRevision> {
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

export function summaryStateRoot() {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-summaries-v1");
}

export function contributeSessionSummaries(server: PluginServerContext) {
  const root = summaryStateRoot();
  const service = (paseo: PaseoApi) => createSessionSummaryService(paseo, root);
  server.handle(saveSummary, (input, { paseo }) => service(paseo).save(input.agentId));
  server.handle(readSummary, (input, { paseo }) => readPinned(root, input));
  server.handle(latestSummaries, (input, { paseo }) => service(paseo).latest(input.agentIds));
  server.handle(attachSummary, () => { throw new Error("Attach summary to another seat's draft is not installed on this host yet."); });
}
