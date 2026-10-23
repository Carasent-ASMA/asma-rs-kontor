import { createHash } from "node:crypto";
import { lstatSync } from "node:fs";
import { mkdir, open, readdir, readFile, rename, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoApi, PaseoWorkspace, PaseoWorkspaceListOptions, PaseoWorkspaceListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readConfig } from "./config";
import { readFloor } from "./floor";
import {
  CREATE_REQUEST_LABEL, CREATE_ROLE, PERSONA_PROVIDERS, TITLE_AUTO_LABEL, adhocTitle, checkCreate, createOutcomeSchema, createRequestSchema,
  deskName, executeCreate, isAdamWorkspace, normalizeTopic, readCreateOptions, topicProblem,
  type CreateOptions, type CreateOutcome, type CreateProvider, type CreateRequest,
} from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Page<T> = { entries: T[]; pageInfo: { hasMore: boolean; nextCursor?: string | null } };
export type CreateClient = {
  workspaces: {
    list(options?: PaseoWorkspaceListOptions): Promise<PaseoWorkspaceListResult>;
    ref(id: string): { refresh(): Promise<PaseoWorkspace | null>; agents: Pick<ReturnType<PaseoApi["workspaces"]["ref"]>["agents"], "create"> };
  };
  agents: { list(options?: PaseoAgentListOptions): Promise<PaseoAgentListResult>; ref(id: string): Pick<ReturnType<PaseoApi["agents"]["ref"]>, "refresh"> };
  providers: Pick<PaseoApi["providers"], "snapshot">;
  config: Pick<PaseoApi["config"], "get">;
};

/** How long a dispatched create may stay unanswered before its outcome is reported unknown. */
export const CREATE_ANSWER_MS = 120_000;

/** The operator-owned switch on the host, read on every request: `<PASEO_HOME or ~/.paseo>/plugin-config/kontor/create.json`. */
export function createSwitchFile(): string {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "create.json");
}

function createStateRoot(): string {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-create-v1");
}

/** Available by default when absent; explicit off, unreadable or malformed switches keep creation off. */
export function createSwitchProblem(file: string): string | null {
  const mode = readFloor(file, "create");
  if (mode.reason === "explicit-on") return null;
  if (mode.reason === "explicit-off") return "New LSA is switched off on this host.";
  if (mode.reason === "not-configured") {
    // The shared reader follows symlinks; a dangling switch is unreadable, not absent.
    try { if (!lstatSync(file, { throwIfNoEntry: false })) return null; } catch {}
  }
  return "New LSA is off: the host's create switch is not valid. The operator corrects it on the host computer.";
}

const PERSONA_TEMPLATE = [
  "You are Adam, playing LSA (Lead Software Architect) in a Kontor desk session.",
  "Placement: desk {desk}, workspace ADAM ({workspaceId}). Scope: ADHOC. Topic: {topic}.",
  "Purpose: own architecture, scope coherence, cross-task decisions and remediation design for the question at hand. Your outputs are decisions, designs, decision-record proposals, review verdicts and named gaps, never delivery code that belongs to a task.",
  "Tool profile: read and inspect documentation and code. Stay read-only on tracker and runtime state unless the owning workflow grants a specific write. No package installs, hook installs or scheduler writes.",
  "This workspace is a shared checkout: do not edit files in it. Make any change in an isolated worktree created through the project's own workflow.",
  "Before acting, load the orchestration instructions and skills this repository declares (start with AGENTS.md).",
  "Stop and record the blocker when a decision belongs to the operator, evidence cannot be read, or a gate cannot be satisfied.",
  "",
  "Title rules. Your Paseo title is \"{title}\"; your label title.auto holds the title last written for you. Rename yourself only with the Paseo MCP update_agent tool, and only while your current title equals title.auto:",
  "- When the work gets a confirmed epic key (a Jira readback you made, an approved plan carrying it, or the operator's explicit confirmation), rename once to \"LSA • <KEY> • <topic>\" and set the labels epic=<KEY> and title.auto=<the new title>.",
  "- Change the topic only at a checkpoint (key promotion, a plan title set or changed, or an explicit instruction), at most once per checkpoint. A topic has 3 to 48 characters and never contains • or ·.",
  "- If your title differs from title.auto, the operator renamed you: never change the title again. You may still set epic.",
  "- Never write a kontor.* label or an orchestration label.",
].join("\n");
/** Identifies the persona text a session was given; recorded in its `persona` label. */
export const PERSONA_VERSION = `kontor-lsa-adhoc@${createHash("sha256").update(PERSONA_TEMPLATE).digest("hex").slice(0, 12)}`;
export function personaPrompt(fields: { desk: string; workspaceId: string; topic: string; title: string }): string {
  return PERSONA_TEMPLATE.replace(/\{(desk|workspaceId|topic|title)\}/g, (_, key: keyof typeof fields) => fields[key]);
}
const personaSupport = (provider: string) => PERSONA_PROVIDERS.includes(provider) ? "supported" as const : "unverified" as const;

const recordSchema = z.object({
  request: createRequestSchema.extend({ title: z.string(), cwd: z.string(), labels: z.record(z.string(), z.string()), at: z.string() }),
  outcome: createOutcomeSchema.nullable(),
});
type CreateRecord = z.infer<typeof recordSchema>;

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

async function allPages<T>(read: (cursor: string | undefined) => Promise<Page<T>>): Promise<T[]> {
  const rows: T[] = [], cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1_000; page++) {
    const result = await read(cursor);
    rows.push(...result.entries);
    if (!result.pageInfo.hasMore) return rows;
    const next = result.pageInfo.nextCursor;
    if (!next || cursors.has(next)) throw new Error("The host returned an incomplete list.");
    cursors.add(next); cursor = next;
  }
  throw new Error("The host list exceeded the bounded page walk.");
}

/** Desk `ADAM` workspaces that can take a new session, `DESK • ADAM` first. */
export function adamDesks(workspaces: readonly PaseoWorkspace[]): CreateOptions["desks"] {
  return workspaces.flatMap((workspace) => {
    const project = workspace.projectCustomName ?? workspace.projectDisplayName, desk = deskName(project);
    return desk && isAdamWorkspace(workspace.title ?? workspace.name) && !workspace.archivingAt && workspace.workspaceDirectory
      ? [{ workspaceId: workspace.id, projectId: workspace.projectId, desk, project }] : [];
  }).sort((a, b) => Number(b.desk.toUpperCase() === "ADAM") - Number(a.desk.toUpperCase() === "ADAM") || a.project.localeCompare(b.project));
}

export function createProviders(snapshot: Awaited<ReturnType<PaseoApi["providers"]["snapshot"]>>): CreateProvider[] {
  return snapshot.entries.map((entry) => {
    const selectable = (entry.models ?? []).filter((model) => model.isSelectable !== false);
    const models = selectable.map((model) => ({ id: model.id, label: model.label, efforts: (model.thinkingOptions ?? []).map((option) => ({ id: option.id, label: option.label })),
      defaultEffort: model.defaultThinkingOptionId ?? model.thinkingOptions?.find((option) => option.isDefault)?.id ?? null }));
    const enabled = entry.enabled !== false, ready = entry.status === "ready";
    let note: string | null = null;
    if (!enabled) note = "Disabled on this host.";
    else if (!ready) note = `Provider ${entry.status}${entry.error ? `: ${entry.error}` : ""}.`;
    else if (!models.length) note = "No selectable model on this host.";
    return { id: entry.provider, label: entry.label ?? entry.provider, available: note === null, note, persona: personaSupport(entry.provider), models,
      defaultModel: selectable.find((model) => model.isDefault)?.id ?? models[0]?.id ?? null,
      modes: (entry.modes ?? []).map((mode) => ({ id: mode.id, label: mode.label, dangerous: mode.colorTier === "dangerous" })),
      defaultMode: entry.defaultModeId ?? null };
  });
}

/** What the request asks for that a choice from these providers cannot give, or null. */
function choiceProblem(providers: readonly CreateProvider[], request: Pick<CreateRequest, "provider" | "model" | "effort" | "modeId">): string | null {
  const provider = providers.find((entry) => entry.id === request.provider);
  if (!provider) return `Provider ${request.provider} is not on this host.`;
  if (!provider.available) return `${provider.label}: ${provider.note}`;
  const model = provider.models.find((entry) => entry.id === request.model);
  if (!model) return `Model ${request.model} is not selectable for ${provider.label}.`;
  if (model.efforts.length ? !model.efforts.some((effort) => effort.id === request.effort) : request.effort !== null) return `Effort ${request.effort ?? "none"} is not offered for ${model.label}.`;
  if (provider.modes.length ? !provider.modes.some((mode) => mode.id === request.modeId) : request.modeId !== null) return `Mode ${request.modeId ?? "none"} is not offered by ${provider.label}.`;
  return null;
}

/** Each requested field the host's readback does not show; empty when it matches. */
export function readbackDifferences(agent: Agent, request: CreateRecord["request"]): string[] {
  const differences: string[] = [];
  if (agent.title !== request.title) differences.push(`Title reads “${agent.title ?? "none"}”.`);
  if (agent.workspaceId !== request.workspaceId) differences.push("Placed outside the chosen ADAM workspace.");
  if (agent.cwd !== request.cwd) differences.push("Working directory differs from the chosen ADAM workspace.");
  for (const [key, value] of Object.entries(request.labels)) if (agent.labels?.[key] !== value) differences.push(`Label ${key} reads ${agent.labels?.[key] ?? "missing"}.`);
  if (agent.provider !== request.provider) differences.push(`Provider reads ${agent.provider}.`);
  const model = agent.model ?? agent.runtimeInfo?.model ?? null;
  if (model !== request.model) differences.push(`Model reads ${model ?? "not reported yet"}.`);
  const mode = agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null;
  if (request.modeId !== null && mode !== request.modeId) differences.push(`Mode reads ${mode ?? "not reported yet"}.`);
  const effort = agent.effectiveThinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null;
  if (request.effort !== null && effort !== request.effort) differences.push(`Effort reads ${effort ?? "not reported yet"}.`);
  if (agent.archivedAt) differences.push("Already archived.");
  return differences;
}

// shortcut: one plugin worker owns the state root; add an inter-process fence if the host supports concurrent workers.
let queue: Promise<unknown> = Promise.resolve();
function serial<T>(step: () => Promise<T>): Promise<T> {
  const next = queue.then(step);
  queue = next.catch(() => {});
  return next;
}

/**
 * Host-local durable intents: a request is written before its dispatch, and its outcome after. An unsettled
 * request blocks the next creation until a Check settles it; nothing is ever re-dispatched.
 */
export function createSessionCreateService(client: CreateClient, root: string, switchFile: string, answerMs = CREATE_ANSWER_MS) {
  const path = (requestId: string) => join(root, `${requestId}.json`);
  async function readRecord(requestId: string): Promise<CreateRecord | null> {
    try { return recordSchema.parse(JSON.parse(await readFile(path(requestId), "utf8"))); }
    catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
  }
  async function writeIntent(record: CreateRecord) {
    await mkdir(root, { recursive: true, mode: 0o700 });
    const file = await open(path(record.request.requestId), "wx", 0o600);
    try { await file.writeFile(JSON.stringify(record)); await file.sync(); } finally { await file.close(); }
  }
  async function settle(record: CreateRecord, outcome: CreateOutcome): Promise<CreateOutcome> {
    const temporary = `${path(record.request.requestId)}.tmp`;
    const file = await open(temporary, "w", 0o600);
    try { await file.writeFile(JSON.stringify({ ...record, outcome })); await file.sync(); } finally { await file.close(); }
    await rename(temporary, path(record.request.requestId));
    return outcome;
  }
  /** The first request whose effect is not settled; an unreadable record also blocks, since it may be one. */
  async function unsettled(): Promise<CreateOutcome | null> {
    let names: string[];
    try { names = (await readdir(root)).filter((name) => /^[0-9a-f-]{36}\.json$/i.test(name)).sort(); }
    catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
    for (const name of names) {
      const record = await readRecord(name.slice(0, -5)).catch(() => null);
      if (!record) throw new Error("A creation record on the host cannot be read, so no new creation is allowed. The operator inspects Kontor's session-create state on the host.");
      if (!record.outcome || record.outcome.state === "unknown") return record.outcome ?? outcome(record, "unknown", null, "This creation was dispatched and its effect is not settled. Do not retry; Check reads the host for it.");
    }
    return null;
  }
  function outcome(record: Pick<CreateRecord, "request">, state: CreateOutcome["state"], agentId: string | null, message: string, differences: string[] = []): CreateOutcome {
    return { requestId: record.request.requestId, state, title: record.request.title, agentId, workspaceId: record.request.workspaceId, message, differences,
      persona: personaSupport(record.request.provider), checkedAt: new Date().toISOString() };
  }
  async function readback(record: CreateRecord, agentId: string): Promise<CreateOutcome> {
    let found;
    try { found = await client.agents.ref(agentId).refresh(); }
    catch (error) { return outcome(record, "unknown", agentId, `The host replied that it created this session, but reading it back failed: ${errorText(error)}. Check reads it again.`); }
    if (!found || found.agent.id !== agentId) return outcome(record, "unknown", agentId, "The host replied that it created this session, but an exact read returned no such session. Check reads it again.");
    const differences = readbackDifferences(found.agent, record.request);
    return differences.length
      ? outcome(record, "differences", agentId, "Created with differences: the host's readback does not match every requested field.", differences)
      : outcome(record, "verified", agentId, "Created and verified: the host's readback matches the request.");
  }

  async function options(): Promise<CreateOptions> {
    const notes: string[] = [];
    const disabledReason = createSwitchProblem(switchFile);
    const [workspaces, snapshot, config] = await Promise.all([
      allPages((cursor) => client.workspaces.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } })),
      client.providers.snapshot(),
      client.config.get().catch((error: unknown) => { notes.push(`Agent profiles unavailable: ${errorText(error)}.`); return null; }),
    ]);
    const profile = config?.config.agentProfiles?.find((entry) => [entry.name, entry.id].some((value) => value.trim().toUpperCase() === CREATE_ROLE));
    let pending: CreateOutcome | null = null;
    try { pending = await unsettled(); } catch (error) { return { enabled: false, disabledReason: errorText(error), desks: [], providers: [], profile: null, pending: null, notes }; }
    return { enabled: disabledReason === null, disabledReason, desks: adamDesks(workspaces), providers: createProviders(snapshot),
      profile: profile ? { name: profile.name, provider: profile.provider, model: profile.model ?? null, modeId: profile.modeId ?? null, effort: profile.thinkingOptionId ?? null } : null,
      pending, notes };
  }

  function create(input: CreateRequest): Promise<CreateOutcome> {
    const topic = normalizeTopic(input.topic), title = adhocTitle(topic);
    const labels = { role: CREATE_ROLE, [TITLE_AUTO_LABEL]: title, persona: PERSONA_VERSION, [CREATE_REQUEST_LABEL]: input.requestId };
    const record: CreateRecord = { request: { ...input, topic, title, cwd: "", labels, at: new Date().toISOString() }, outcome: null };
    const refuse = (message: string) => outcome(record, "refused", null, `${message} Nothing was created.`);
    return serial(async () => {
      const existing = await readRecord(input.requestId);
      // The same request again (a lost reply on the client) answers from its record and never dispatches twice.
      if (existing) return existing.outcome ?? outcome(existing, "unknown", null, "This request was dispatched and its effect is not settled. Do not retry; Check reads the host for it.");
      const problem = topicProblem(topic);
      if (problem) return refuse(problem);
      const off = createSwitchProblem(switchFile);
      if (off) return refuse(off);
      let workspace, desk;
      try {
        const earlier = await unsettled();
        if (earlier) return refuse(`An earlier creation, “${earlier.title}”, is not settled. Check it first.`);
        workspace = client.workspaces.ref(input.workspaceId);
        const snapshot = await workspace.refresh();
        desk = snapshot ? adamDesks([snapshot])[0] : undefined;
        if (!snapshot || !desk) return refuse("That workspace is no longer a desk's ADAM workspace on this host.");
        record.request.cwd = snapshot.workspaceDirectory!;
        const choice = choiceProblem(createProviders(await client.providers.snapshot({ cwd: snapshot.workspaceDirectory })), input);
        if (choice) return refuse(choice);
        const agents = await allPages((cursor) => client.agents.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
        if (agents.some(({ agent }) => !agent.archivedAt && agent.workspaceId === input.workspaceId && agent.title === title)) return refuse(`“${title}” already names a session in ${desk.project} / ADAM. Choose another topic.`);
      } catch (error) { return refuse(`The host could not be read: ${errorText(error)}.`); }
      try { await writeIntent(record); } catch (error) {
        // Not dispatched; a partial intent is removed so it does not block, and stays blocking if it cannot be.
        await rm(path(input.requestId), { force: true }).catch(() => {});
        return refuse(`Kontor could not record the request on the host: ${errorText(error)}.`);
      }
      let timer: ReturnType<typeof setTimeout> | undefined;
      try {
        const handle = await Promise.race([
          workspace.agents.create({ idempotencyKey: input.requestId, title, labels, config: {
            provider: `${input.provider}/${input.model}`, ...(input.modeId ? { modeId: input.modeId } : {}), ...(input.effort ? { thinkingOptionId: input.effort } : {}),
            systemPrompt: personaPrompt({ desk: desk.project, workspaceId: input.workspaceId, topic, title }),
          } }),
          new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error(`no answer within ${Math.round(answerMs / 1000)} s`)), answerMs); }),
        ]);
        return settle(record, await readback(record, handle.id));
      } catch (error) {
        return settle(record, outcome(record, "unknown", null, `The host did not confirm the creation (${errorText(error)}). Do not retry; Check reads the host for this request.`));
      } finally { clearTimeout(timer); }
    });
  }

  /** Settles an unknown outcome from the host: an exact read, or the request's label. Never dispatches. */
  function check(requestId: string): Promise<CreateOutcome> {
    return serial(async () => {
      const record = await readRecord(requestId);
      if (!record) throw new Error("This host has no record of that creation request. Its effect cannot be confirmed; do not retry creation.");
      if (record.outcome && record.outcome.state !== "unknown") return record.outcome;
      let agentId = record.outcome?.agentId ?? null;
      if (!agentId) {
        try {
          const found = await allPages((cursor) => client.agents.list({ filter: { labels: { [CREATE_REQUEST_LABEL]: requestId }, includeArchived: true }, page: { limit: 10, ...(cursor ? { cursor } : {}) } }));
          const matches = found.filter(({ agent }) => agent.labels?.[CREATE_REQUEST_LABEL] === requestId);
          if (matches.length > 1) return settle(record, outcome(record, "unknown", null, "Multiple sessions carry this request's label. The operator must reconcile them on the host; no new creation is allowed."));
          agentId = matches[0]?.agent.id ?? null;
        } catch (error) { return settle(record, outcome(record, "unknown", null, `The host could not be read: ${errorText(error)}. The outcome stays unknown; Check again.`)); }
      }
      if (!agentId) return settle(record, outcome(record, "unknown", null, "No session from this request is visible on the host yet. Creation may still complete; do not retry. Check again, or ask the operator to reconcile it on the host."));
      return settle(record, await readback(record, agentId));
    });
  }

  return { options, create, check };
}

export function contributeSessionCreate(server: PluginServerContext): void {
  const root = createStateRoot(), file = createSwitchFile();
  server.handle(readCreateOptions, (_input, { paseo }) => createSessionCreateService(paseo, root, file).options());
  server.handle(executeCreate, (input, { paseo }) => createSessionCreateService(paseo, root, file).create(input));
  server.handle(checkCreate, (input, { paseo }) => createSessionCreateService(paseo, root, file).check(input.requestId));
}
