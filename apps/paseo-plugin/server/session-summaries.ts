import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { attachSummary, latestSummaries, readSummary, saveSummary, summaryRevisionSchema, type Capability, type SummaryPin, type SummaryRevision } from "../shared/session-navigator";
import { readConfig } from "./config";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

/** Exact gap on the installed @getpaseo/client surface (PaseoAgentHandle has timeline/send/run only). */
export const SUMMARY_MODEL_API_GAP =
  "The host SDK exposes no compact session-summary generation API on PaseoAgentHandle or PaseoAgentActions (only timeline read, send and run); Kontor cannot request a structured model summary without prompting the live session.";

const PLUGIN_STATE_STORE_LIMIT =
  "Saved revisions persist under the Kontor plugin host-backed state root (plugin-state/kontor/session-summaries-v1); this plugin is the sole writer for summary revisions on this host.";

export const SUMMARY_ATTACH_GAP = "Attach summary to another seat's draft is not installed on this host yet.";

export const SUMMARY_LIMITS_WITH_GENERATION = [
  "Each revision is model-generated through the host SDK compact-summary route with task, decisions, current state and next step sections.",
  PLUGIN_STATE_STORE_LIMIT,
  SUMMARY_ATTACH_GAP,
];

const sectionsSchema = summaryRevisionSchema.shape.sections;

export type CompactSummaryPayload = {
  sections: z.infer<typeof sectionsSchema>;
  cutoff: string;
};

export type SessionSummaryGenerator = {
  generate(agentId: string): Promise<CompactSummaryPayload>;
};

/** Optional host SDK route when the installed client adds agents.generateCompactSummary. */
export function paseoSummaryGenerator(paseo: PaseoApi): SessionSummaryGenerator | undefined {
  const host = paseo as {
    agents?: typeof paseo.agents & {
      generateCompactSummary?(agentId: string): Promise<CompactSummaryPayload>;
    };
  };
  if (typeof host.agents?.generateCompactSummary !== "function") return undefined;
  const generate = host.agents.generateCompactSummary.bind(host.agents);
  return { generate: (agentId: string) => generate(agentId) };
}

export function summariesInstalledCapability(paseo: PaseoApi): Capability {
  if (!paseoSummaryGenerator(paseo)) return { supported: false, reason: SUMMARY_MODEL_API_GAP };
  return { supported: true, limits: SUMMARY_LIMITS_WITH_GENERATION };
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

function assertSupported(paseo: PaseoApi): SessionSummaryGenerator {
  const generator = paseoSummaryGenerator(paseo);
  if (!generator) throw new Error(SUMMARY_MODEL_API_GAP);
  return generator;
}

export function createSessionSummaryService(paseo: PaseoApi, root: string, generator: SessionSummaryGenerator = assertSupported(paseo)) {
  async function exact(agentId: string): Promise<Agent | null> {
    const found = await paseo.agents.ref(agentId).refresh();
    return found?.agent.id === agentId ? found.agent : null;
  }

  async function save(agentId: string): Promise<SummaryRevision> {
    const agent = await exact(agentId);
    if (!agent) throw new Error("This session is not on this host, so no summary was saved.");
    const generated = await generator.generate(agentId);
    const sections = sectionsSchema.parse(generated.sections);
    const cutoff = generated.cutoff.trim();
    if (!cutoff) throw new Error("The host returned no summary cutoff timestamp.");
    const savedAt = new Date().toISOString();
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
  server.handle(saveSummary, (input, { paseo }) => createSessionSummaryService(paseo, root).save(input.agentId));
  server.handle(readSummary, (input, { paseo }) => {
    assertSupported(paseo);
    return readPinned(root, input);
  });
  server.handle(latestSummaries, (input, { paseo }) => {
    assertSupported(paseo);
    return createSessionSummaryService(paseo, root).latest(input.agentIds);
  });
  server.handle(attachSummary, (_input, { paseo }) => {
    assertSupported(paseo);
    throw new Error(SUMMARY_ATTACH_GAP);
  });
}
