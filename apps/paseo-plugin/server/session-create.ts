import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { lstatSync, statSync } from "node:fs";
import { mkdir, open, readdir, readFile, rename, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { z } from "zod";
import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoApi, PaseoWorkspace, PaseoWorkspaceCreateOptions, PaseoWorkspaceListOptions, PaseoWorkspaceListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readConfig } from "./config";
import { readFloor } from "./floor";
import {
  CREATE_REQUEST_LABEL, CREATE_ROLES, CREATE_STARTS, JIRA_KEY_PATTERN, PERSONA_PROVIDERS, TITLE_AUTO_LABEL, checkCreate, createOutcomeSchema, createRequestSchema,
  deskIssueProblem, deskKind, deskName, executeCreate, issueTopic, jiraIssueSchema, leadTitle, leadWorkspaceName, normalizeTopic, readCreateOptions, topicProblem,
  type CreateDesk, type CreateOptions, type CreateOutcome, type CreateProvider, type CreateRequest, type CreateRole, type CreateScope, type CreateStart, type JiraIssue, type JiraIssueClass, type LeadWorkspace,
} from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Page<T> = { entries: T[]; pageInfo: { hasMore: boolean; nextCursor?: string | null } };
type WorkspaceHandle = ReturnType<PaseoApi["workspaces"]["ref"]>;
type AgentHandle = ReturnType<PaseoApi["agents"]["ref"]>;
export type CreateClient = {
  workspaces: {
    list(options?: PaseoWorkspaceListOptions): Promise<PaseoWorkspaceListResult>;
    ref(id: string): Pick<WorkspaceHandle, "refresh" | "agents">;
    create(options: PaseoWorkspaceCreateOptions): Promise<Pick<WorkspaceHandle, "id" | "refresh">>;
  };
  agents: { list(options?: PaseoAgentListOptions): Promise<PaseoAgentListResult>; ref(id: string): Pick<AgentHandle, "refresh" | "timeline"> };
  providers: Pick<PaseoApi["providers"], "snapshot">;
  config: Pick<PaseoApi["config"], "get">;
};

/** How long a dispatched create may stay unanswered before its outcome is reported unknown. */
export const CREATE_ANSWER_MS = 120_000;
const MAX_BYTES = 2 * 1024 * 1024;

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
  if (mode.reason === "explicit-off") return "New session is switched off on this host.";
  if (mode.reason === "not-configured") {
    // Missing descendants are valid only when the nearest existing ancestor resolves to a directory.
    try {
      if (!lstatSync(file, { throwIfNoEntry: false })) {
        let path = dirname(file);
        while (!lstatSync(path, { throwIfNoEntry: false })) {
          const parent = dirname(path);
          if (parent === path) break;
          path = parent;
        }
        if (statSync(path).isDirectory()) return null;
      }
    } catch {}
  }
  return "New session is off: the host's create switch is not valid. The operator corrects it on the host computer.";
}

/** The one rule every persona carries: a created session labels its own confirmed scope, never a sibling's. */
export const SCOPE_LABEL_RULE = "A session you create gets the labels of its own confirmed scope: epic=<KEY>, task=<KEY> when it works one task, and role=<code>. Never copy yours or a sibling's; omit a key nothing confirms.";

const JIRA_DUTY = "Jira duty: you may create ASMA issues when the work needs them, following the Jira content contracts, and you must move the issues you own through their workflow states as work progresses. Use `asma jira create` and `asma jira transition`. A state change follows observed progress, never a plan alone.";

const WALL_PROTOCOL = [
  "Wall protocol. Stop this ticket's delivery and ask the operator when you observe any of these triggers:",
  "1. acceptance needs a change to a module, public contract, schema or shared library that the ticket's Delivery context does not name;",
  "2. an accepted ADR or architecture rule would need a deviation;",
  "3. the same blocking review finding returns after two fix rounds;",
  "4. the work splits into two or more independently acceptable outcomes.",
  "Then comment on the Jira issue with the trigger, evidence, options and your recommendation, record a hold (reason, owner, lifts-when, review date at most 30 days ahead), and ask the operator one question: A widen this ticket and refactor now; B split into new tickets; C plan an epic. Other tickets continue. Record the operator's answer verbatim on the issue and continue without further prompts. A: rewrite the issue's scope and continue in the same TSW through the same gates. B: create the new issues, link them, narrow the current one and continue it. C: hand off to a new LSA in the desk's LSA workspace; that LSA plans and creates the epic and re-parents the ticket, which continues in a TSW of the new ESW, and the old TSW retires after the handoff readback. Without an answer by the review date, raise attention again; never drop the ticket.",
].join("\n");

const PERSONA_HEAD = [
  "You are Adam, playing {role} in a Kontor desk session.",
  "Placement: desk {desk}, workspace {workspace} ({workspaceId}). {scope} Topic: {topic}.",
  "Tool profile: read and inspect documentation and code. Stay read-only on tracker and runtime state unless the owning workflow grants a specific write. No package installs, hook installs or scheduler writes.",
  "This workspace is a shared checkout: do not edit files in it. Make any change in an isolated worktree created through the project's own workflow.",
  "Before acting, load the orchestration instructions and skills this repository declares (start with AGENTS.md).",
  "Stop and record the blocker when a decision belongs to the operator, evidence cannot be read, or a gate cannot be satisfied.",
  "Title rules. Your Paseo title is \"{title}\"; your label title.auto holds the title last written for you. Do not rename yourself. If your title differs from title.auto, the operator renamed you: keep that title. Never write a kontor.* label or an orchestration label.",
  SCOPE_LABEL_RULE,
].join("\n");

const ADHOC_LSA_TEMPLATE = [
  PERSONA_HEAD.replace("{role}", "LSA (Lead Software Architect)").replace("{scope}", "Scope: ADHOC."),
  "Purpose: own architecture, scope coherence, cross-task decisions and remediation design for the question at hand. Your outputs are decisions, designs, decision-record proposals, review verdicts and named gaps, never delivery code that belongs to a task.",
  "Promotion is a handoff. This session stays ad hoc: do not rename yourself to a key and do not create the successor yourself. When the work gets a confirmed epic key, write the handoff summary through the seat-handoff mechanism (scope, artifacts, fulfilled and unverified criteria, findings, next action and artifact revision) and ask the operator to start the bound lead in the desk's LSA workspace with this session named as the source.",
].join("\n");

const BOUND_LSA_TEMPLATE = [
  PERSONA_HEAD.replace("{role}", "LSA (Lead Software Architect)").replace("{scope}", "Scope: epic {key}."),
  "Purpose: own architecture, scope coherence, cross-task decisions and remediation design for this epic. Your outputs are decisions, designs, decision-record proposals, review verdicts and named gaps, never delivery code that belongs to a task.",
  JIRA_DUTY,
  WALL_PROTOCOL,
].join("\n");

const BOUND_TPM_TEMPLATE = [
  PERSONA_HEAD.replace("{role}", "TPM (Technical Program Manager)").replace("{scope}", "Scope: epic {key}."),
  "Purpose: own coordination, admission, dispatch, supervision, gates and closeout for this epic. Keep the plan, the task graph and the evidence aligned; the LSA owns architecture and the delivery seats own implementation.",
  JIRA_DUTY,
  WALL_PROTOCOL,
].join("\n");

const PERSONAS = {
  "lsa-ad-hoc": { template: ADHOC_LSA_TEMPLATE, version: `kontor-lsa-adhoc@${hash(ADHOC_LSA_TEMPLATE)}` },
  "lsa-bound": { template: BOUND_LSA_TEMPLATE, version: `kontor-lsa-bound@${hash(BOUND_LSA_TEMPLATE)}` },
  "tpm-bound": { template: BOUND_TPM_TEMPLATE, version: `kontor-tpm-bound@${hash(BOUND_TPM_TEMPLATE)}` },
} as const;
export type PersonaKind = keyof typeof PERSONAS;
function hash(text: string): string { return createHash("sha256").update(text).digest("hex").slice(0, 12); }
export function personaVersion(kind: PersonaKind, supported: boolean): string {
  return supported ? PERSONAS[kind].version : PERSONAS[kind].version.replace("@", "+bootstrap@");
}
/** The ad-hoc LSA persona, kept for readers of the previous single-persona contract. */
export const PERSONA_VERSION = PERSONAS["lsa-ad-hoc"].version;
const personaSupport = (provider: string) => PERSONA_PROVIDERS.includes(provider) ? "supported" as const : "unverified" as const;

export function personaFor(role: CreateRole, start: CreateStart): PersonaKind {
  return start === "ad-hoc" ? "lsa-ad-hoc" : role === "TPM" ? "tpm-bound" : "lsa-bound";
}
export function personaPrompt(kind: PersonaKind, fields: { desk: string; workspace: string; workspaceId: string; topic: string; title: string; key: string | null }): string {
  return PERSONAS[kind].template.replace(/\{(desk|workspace|workspaceId|topic|title|key)\}/g, (_, name: keyof typeof fields) => fields[name] ?? "");
}
/** The persona text with no task: for an unverified provider it rides here, and a source summary always does. */
export function bootstrapPrompt(persona: string, source: string | null, supported: boolean): string | null {
  const parts: string[] = [];
  if (!supported) parts.push(persona);
  if (source) parts.push(`Handoff summary from the source session (read this before your first task):\n${source}`);
  if (!parts.length) return null;
  parts.push("This is a bootstrap message, not a task. Read it and wait for the operator's first task.");
  return parts.join("\n\n");
}

/**
 * One lead seat, described by its caller. The core never chooses a title, a label set or a lead-workspace
 * allowlist; ASMA-8641's takeover caller reuses it with its own spec.
 */
export type SeatCreateSpec = {
  requestId: string;
  title: string;
  labels: Record<string, string>;
  provider: string;
  model: string;
  effort: string | null;
  modeId: string | null;
  /** The lead workspace the seat belongs in; null asks the core to create it after the intent. */
  workspaceId: string | null;
  /** The desk's anchor workspace, recorded before a missing lead workspace has an id. */
  anchorWorkspaceId: string;
  workspaceName: string;
  /** The desk checkout that scopes the provider catalog. */
  cwd: string;
  /** Creates the missing lead workspace and reads it back; null when it already exists. */
  createWorkspace: (() => Promise<{ id: string; cwd: string }>) | null;
  /** Re-validates the target at create time; null when it still matches. */
  revalidate: (() => Promise<string | null>) | null;
  persona: { text: string; supported: boolean };
  /** A bootstrap-only first message, never a task. */
  bootstrap: string | null;
};
export type SeatCreateOutcome = CreateOutcome;
export type SeatCreatePlan = { spec: SeatCreateSpec } | { refused: string; requestId: string; title: string };
export type SeatCreateBuilder = () => Promise<SeatCreatePlan>;
export type SeatCreate = (requestId: string, build: SeatCreateBuilder) => Promise<SeatCreateOutcome>;

const recordSchema = z.object({
  request: z.object({
    requestId: z.string().uuid(), title: z.string(), cwd: z.string(), workspaceId: z.string().min(1).max(200),
    labels: z.record(z.string(), z.string()), provider: z.string(), model: z.string(), effort: z.string().nullable(), modeId: z.string().nullable(), at: z.string(),
  }).passthrough(),
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

export type DeskInventory = Omit<CreateDesk, "cwd"> & { cwd: string; anchorWorkspaceId: string };
type Checkout = { cwd: string; id: string };
type DeskGroup = { projectId: string; project: string; desk: string; adam: Checkout | null; leadCheckout: Checkout | null; lead: Record<LeadWorkspace, string | null> };

/** The desks' workspaces grouped by project; a project root is never a checkout. */
function deskGroups(workspaces: readonly PaseoWorkspace[]): DeskGroup[] {
  const groups = new Map<string, DeskGroup>();
  for (const workspace of workspaces) {
    if (workspace.archivingAt || !workspace.projectId) continue;
    const project = workspace.projectCustomName ?? workspace.projectDisplayName ?? "", desk = deskName(project);
    if (!desk) continue;
    const group = groups.get(workspace.projectId) ?? { projectId: workspace.projectId, project, desk, adam: null, leadCheckout: null, lead: { ADHOC: null, LSA: null, TPM: null } };
    const name = (workspace.title ?? workspace.name ?? "").trim().toUpperCase();
    const lead = name === "ADHOC" || name === "LSA" || name === "TPM";
    if (workspace.workspaceDirectory) {
      const checkout = { cwd: workspace.workspaceDirectory, id: workspace.id };
      if (name === "ADAM") group.adam ??= checkout;
      if (lead) group.leadCheckout ??= checkout;
    }
    if (lead) group.lead[name as LeadWorkspace] = workspace.id;
    groups.set(workspace.projectId, group);
  }
  return [...groups.values()];
}

/**
 * Every native desk project, its checkout, and its lead workspaces by kind; `DESK • ADAM` first.
 * The checkout is the ADAM workspace when one exists, else a lead workspace. A ticket `TSW` worktree and the
 * non-git project root are never a checkout, so a desk without either is not offered for creation.
 */
export function desksOf(workspaces: readonly PaseoWorkspace[]): DeskInventory[] {
  return deskGroups(workspaces).flatMap((group) => {
    const checkout = group.adam ?? group.leadCheckout;
    return checkout ? [{ projectId: group.projectId, project: group.project, desk: group.desk, kind: deskKind(group.desk), cwd: checkout.cwd, anchorWorkspaceId: checkout.id, lead: group.lead }] : [];
  }).sort((left, right) => Number(right.kind === "adam") - Number(left.kind === "adam") || left.project.localeCompare(right.project));
}

/** A desk with neither an ADAM nor a lead checkout cannot host a new session; adoption creates its lead workspaces first. */
export function deskCheckoutProblem(workspaces: readonly PaseoWorkspace[], projectId: string): string | null {
  const group = deskGroups(workspaces).find((entry) => entry.projectId === projectId);
  return group && !group.adam && !group.leadCheckout ? "This desk has no checkout; create the lead workspaces through adoption first." : null;
}

/** The desks a form can offer, without the internal anchor identity. */
export function deskProjects(workspaces: readonly PaseoWorkspace[]): CreateOptions["desks"] {
  return desksOf(workspaces).map(({ anchorWorkspaceId: _anchor, ...desk }) => desk);
}

export function createProviders(snapshot: Awaited<ReturnType<PaseoApi["providers"]["snapshot"]>>): CreateProvider[] {
  return snapshot.entries.map((entry) => {
    const selectable = (entry.models ?? []).filter((model) => model.isSelectable !== false);
    const models = selectable.map((model) => ({ id: model.id, label: model.label, efforts: (model.thinkingOptions ?? []).map((option) => ({ id: option.id, label: option.label })),
      defaultEffort: model.defaultThinkingOptionId ?? model.thinkingOptions?.find((option) => option.isDefault)?.id ?? null }));
    const status: CreateProvider["status"] = entry.enabled === false ? "disabled" : entry.enabled !== true ? "unknown"
      : ["ready", "loading", "error", "unavailable"].includes(entry.status) ? entry.status : "unknown";
    let note: string | null = null;
    if (status === "disabled") note = "Disabled on this host.";
    else if (status === "unknown") note = "Provider availability is unknown.";
    else if (status !== "ready") note = `Provider ${status}${entry.error ? `: ${entry.error}` : ""}.`;
    else if (!models.length) note = "No selectable model on this host.";
    return { id: entry.provider, label: entry.label ?? entry.provider, available: note === null, status, note, persona: personaSupport(entry.provider), models,
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
  if (agent.workspaceId !== request.workspaceId) differences.push("Placed outside the chosen workspace.");
  if (agent.cwd !== request.cwd) differences.push("Working directory differs from the chosen workspace.");
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

/** One live Jira search through asma-cli; a failure is distinct from an empty result. */
export type JiraSearchInput = { query: string; issueClass: JiraIssueClass | null; openOnly: boolean };
export type JiraSearchResult = { issues: JiraIssue[]; hasMore: boolean; nextPageToken: string | null; failure: string | null };
export type JiraSearch = (input: JiraSearchInput) => Promise<JiraSearchResult>;
const jiraSearchOutput = z.object({
  issues: z.array(jiraIssueSchema),
  hasMore: z.boolean(),
  nextPageToken: z.string().nullable().optional(),
}).strict();
const executeFile = promisify(execFile);

/** The CLI arguments for one search; the 50 cap is applied before the result ever reaches the picker. */
export function jiraSearchArgs(input: JiraSearchInput): string[] {
  return ["jira", "search", "--query", input.query, "--limit", "50",
    ...(input.issueClass ? ["--class", input.issueClass] : []), ...(input.openOnly ? [] : ["--all"])];
}

/** Parse the CLI page without dropping its identity, timestamp or continuation fields. */
export function parseJiraSearchOutput(stdout: string): Omit<JiraSearchResult, "failure"> {
  const parsed = jiraSearchOutput.parse(JSON.parse(stdout));
  return { issues: parsed.issues, hasMore: parsed.hasMore, nextPageToken: parsed.nextPageToken ?? null };
}

type JiraExecute = (file: string, args: readonly string[], options: { timeout: number; maxBuffer: number; encoding: "utf8" }) => Promise<{ stdout: string }>;

/** The CLI boundary, injectable so the contract can be tested without a live Jira or a child process. */
export async function runJiraSearch(input: JiraSearchInput, execute: JiraExecute = executeFile): Promise<JiraSearchResult> {
  try {
    const { stdout } = await execute("asma", jiraSearchArgs(input), { timeout: 15_000, maxBuffer: MAX_BYTES, encoding: "utf8" });
    return { ...parseJiraSearchOutput(stdout), failure: null };
  } catch (error) { return { issues: [], hasMore: false, nextPageToken: null, failure: errorText(error) }; }
}

export const asmaJiraSearch: JiraSearch = (input) => runJiraSearch(input);

/** The source session's last assistant message is its handoff summary; null when it wrote none. */
export async function sourceSummary(client: CreateClient, agentId: string): Promise<string | null> {
  const page = await client.agents.ref(agentId).timeline.refetch({ limit: 100, direction: "tail" });
  for (let index = page.entries.length - 1; index >= 0; index--) {
    const item = page.entries[index]!.item;
    if (item.type === "assistant_message" && item.text.trim()) return item.text.trim();
  }
  return null;
}

class WorkspaceCreateError extends Error {
  constructor(message: string, readonly mayExist: boolean) { super(message); }
}

/** Create a missing lead workspace on the desk and read back its id, name, project and directory. */
async function createLeadWorkspace(client: CreateClient, desk: DeskInventory, name: LeadWorkspace): Promise<{ id: string; cwd: string }> {
  const matches = async () => (await allPages((cursor) => client.workspaces.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } })))
    .find((workspace) => workspace.projectId === desk.projectId && !workspace.archivingAt && (workspace.title ?? workspace.name ?? "").trim().toUpperCase() === name);
  const verify = (snapshot: PaseoWorkspace | null): { id: string; cwd: string } => {
    if (!snapshot || snapshot.projectId !== desk.projectId || (snapshot.title ?? snapshot.name ?? "").trim().toUpperCase() !== name || !snapshot.workspaceDirectory) {
      throw new WorkspaceCreateError(`The ${name} workspace on ${desk.project} did not read back with its name, project and working directory.`, true);
    }
    return { id: snapshot.id, cwd: snapshot.workspaceDirectory };
  };
  let handle;
  try { handle = await client.workspaces.create({ source: { kind: "directory", path: desk.cwd!, projectId: desk.projectId }, title: name }); }
  catch (error) {
    let found;
    try { found = await matches(); } catch { throw new WorkspaceCreateError(`The ${name} workspace could not be created: ${errorText(error)}.`, true); }
    if (found) return verify(found);
    throw new WorkspaceCreateError(`The ${name} workspace could not be created: ${errorText(error)}.`, false);
  }
  return verify(await handle.refresh());
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
export function createSessionCreateService(client: CreateClient, root: string, switchFile: string, answerMs = CREATE_ANSWER_MS, jira: JiraSearch = asmaJiraSearch) {
  const path = (requestId: string) => join(root, `${requestId}.json`);
  async function readRecord(requestId: string): Promise<CreateRecord | null> {
    try { return recordSchema.parse(JSON.parse(await readFile(path(requestId), "utf8"))); }
    catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
  }
  async function writeRecord(record: CreateRecord) {
    await mkdir(root, { recursive: true, mode: 0o700 });
    const temporary = `${path(record.request.requestId)}.tmp`;
    const file = await open(temporary, "w", 0o600);
    try { await file.writeFile(JSON.stringify(record)); await file.sync(); } finally { await file.close(); }
    await rename(temporary, path(record.request.requestId));
  }
  async function settle(record: CreateRecord, result: CreateOutcome): Promise<CreateOutcome> {
    await writeRecord({ ...record, outcome: result });
    return result;
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
  function refusedOutcome(requestId: string, title: string, message: string): CreateOutcome {
    return { requestId, state: "refused", title, agentId: null, workspaceId: null, message: `${message} Nothing was created.`, differences: [],
      persona: "supported", checkedAt: new Date().toISOString() };
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

  async function options(scope: CreateScope = {}): Promise<CreateOptions> {
    const notes: string[] = [];
    const disabledReason = createSwitchProblem(switchFile);
    const [workspaces, config] = await Promise.all([
      allPages((cursor) => client.workspaces.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } })),
      client.config.get().catch((error: unknown) => { notes.push(`Agent profiles unavailable: ${errorText(error)}.`); return null; }),
    ]);
    const projectId = scope.projectId ?? (scope.workspaceId ? workspaces.find((entry) => entry.id === scope.workspaceId)?.projectId : undefined);
    const desks = deskProjects(workspaces), desk = (projectId ? desks.find((entry) => entry.projectId === projectId) : desks[0]) ?? null;
    const role = scope.role ?? CREATE_ROLES[0], start = scope.start ?? CREATE_STARTS[0];
    const providers = desk?.cwd ? createProviders(await client.providers.snapshot({ cwd: desk.cwd })) : [];
    const profile = config?.config.agentProfiles?.find((entry) => [entry.name, entry.id].some((value) => value.trim().toUpperCase() === role));
    let pending: CreateOutcome | null = null;
    try { pending = await unsettled(); } catch (error) { return { enabled: false, disabledReason: errorText(error), desks: [], catalogProjectId: null, providers: [], profile: null, issues: [], jiraProblem: null, pending: null, notes }; }
    // The issue chooses its desk, so the search spans every type; an ineligible issue stays listed with its reason.
    let issues: JiraIssue[] = [], jiraProblem: string | null = null;
    if (desk && start === "from-jira") {
      const issueClass = scope.issueClass ?? (desk.kind === "adam" ? "epic" : desk.kind === "tickets" ? "non-epic" : "eligible");
      const result = await jira({ query: scope.issueKey ?? "", issueClass, openOnly: true });
      if (result.failure) jiraProblem = `Jira could not be read: ${result.failure}`;
      else issues = result.issues;
    }
    return { enabled: disabledReason === null, disabledReason, desks, catalogProjectId: desk?.projectId ?? null, providers,
      profile: profile ? { name: profile.name, provider: profile.provider, model: profile.model ?? null, modeId: profile.modeId ?? null, effort: profile.thinkingOptionId ?? null } : null,
      issues, jiraProblem, pending, notes };
  }

  /** The one internal creation core: spec, validate, durable intent, one idempotent create, readback. */
  const createSeat: SeatCreate = (requestId, build) => serial(async () => {
    const existing = await readRecord(requestId);
    // The same request again (a lost reply on the client) answers from its record and never dispatches twice.
    if (existing) return existing.outcome ?? outcome(existing, "unknown", null, "This request was dispatched and its effect is not settled. Do not retry; Check reads the host for it.");
    const off = createSwitchProblem(switchFile);
    if (off) return refusedOutcome(requestId, "", off);
    let plan: SeatCreatePlan;
    try { plan = await build(); } catch (error) { return refusedOutcome(requestId, "", `The host could not be read: ${errorText(error)}.`); }
    if ("refused" in plan) return refusedOutcome(requestId, plan.title, plan.refused);
    const spec = plan.spec;
    const failure = (message: string) => refusedOutcome(requestId, spec.title, message);
    let cwd = spec.cwd, workspaceId = spec.workspaceId;
    try {
      const earlier = await unsettled();
      if (earlier) return failure(`An earlier creation, “${earlier.title}”, is not settled. Check it first.`);
      if (workspaceId) {
        const snapshot = await client.workspaces.ref(workspaceId).refresh();
        if (!snapshot || snapshot.archivingAt || !snapshot.workspaceDirectory || (snapshot.title ?? snapshot.name ?? "").trim().toUpperCase() !== spec.workspaceName.toUpperCase()) {
          return failure(`That workspace is no longer a ${spec.workspaceName} workspace on this host.`);
        }
        cwd = snapshot.workspaceDirectory;
      }
      const choice = choiceProblem(createProviders(await client.providers.snapshot({ cwd })), spec);
      if (choice) return failure(choice);
      const target = await spec.revalidate?.();
      if (target) return failure(target);
      if (workspaceId) {
        const agents = await allPages((cursor) => client.agents.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
        if (agents.some(({ agent }) => !agent.archivedAt && agent.workspaceId === workspaceId && agent.title === spec.title)) return failure(`“${spec.title}” already names a session in ${spec.workspaceName}. Choose another topic.`);
      }
    } catch (error) { return failure(`The host could not be read: ${errorText(error)}.`); }
    const record: CreateRecord = { request: { requestId, title: spec.title, cwd, workspaceId: workspaceId ?? spec.anchorWorkspaceId,
      labels: spec.labels, provider: spec.provider, model: spec.model, effort: spec.effort, modeId: spec.modeId, at: new Date().toISOString() }, outcome: null };
    try { await writeRecord(record); } catch (error) {
      // Not dispatched; a partial intent is removed so it does not block, and stays blocking if it cannot be.
      await rm(path(requestId), { force: true }).catch(() => {});
      return failure(`Kontor could not record the request on the host: ${errorText(error)}.`);
    }
    if (!workspaceId) {
      try {
        const created = await spec.createWorkspace!();
        workspaceId = created.id;
        record.request.workspaceId = created.id;
        record.request.cwd = created.cwd;
        await writeRecord(record);
      } catch (error) {
        const mayExist = error instanceof WorkspaceCreateError ? error.mayExist : true;
        return settle(record, outcome(record, mayExist ? "unknown" : "refused", null, mayExist
          ? `${errorText(error)} The effect is not settled. Do not retry; Check reads the host for it.`
          : `${errorText(error)} Nothing was created.`));
      }
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const handle = await Promise.race([
        client.workspaces.ref(record.request.workspaceId).agents.create({ idempotencyKey: requestId, title: spec.title, labels: spec.labels,
          ...(spec.persona.supported ? { config: { provider: `${spec.provider}/${spec.model}`, ...(spec.modeId ? { modeId: spec.modeId } : {}), ...(spec.effort ? { thinkingOptionId: spec.effort } : {}), systemPrompt: spec.persona.text } }
            : { config: { provider: `${spec.provider}/${spec.model}`, ...(spec.modeId ? { modeId: spec.modeId } : {}), ...(spec.effort ? { thinkingOptionId: spec.effort } : {}) } }),
          ...(spec.bootstrap ? { prompt: spec.bootstrap } : {}) }),
        new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error(`no answer within ${Math.round(answerMs / 1000)} s`)), answerMs); }),
      ]);
      return settle(record, await readback(record, handle.id));
    } catch (error) {
      return settle(record, outcome(record, "unknown", null, `The host did not confirm the creation (${errorText(error)}). Do not retry; Check reads the host for this request.`));
    } finally { clearTimeout(timer); }
  });

  /** New session's caller: role, desk and start choose the title, labels, persona and lead workspace. */
  async function buildNewSession(input: CreateRequest): Promise<SeatCreatePlan> {
    const refusal = (message: string, title = "") => ({ refused: message, requestId: input.requestId, title }) as const;
    if (input.role === "TPM" && input.start === "ad-hoc") return refusal("Ad hoc creates an LSA only. Choose LSA, or start from a Jira ticket.");
    const workspaces = await allPages((cursor) => client.workspaces.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
    const desk = desksOf(workspaces).find((entry) => entry.projectId === input.projectId);
    if (!desk) return refusal(deskCheckoutProblem(workspaces, input.projectId) ?? "That desk is no longer available on this host.");
    if (input.start === "from-jira" && (desk.kind === "pr-review" || desk.kind === "other")) return refusal(`${desk.project} offers ad hoc sessions only; From Jira is not available there.`);
    let key: string | null = null, topic: string;
    if (input.start === "from-jira") {
      const wanted = (input.issueKey ?? "").trim().toUpperCase();
      if (!JIRA_KEY_PATTERN.test(wanted)) return refusal("Choose an ASMA issue key, for example ASMA-1234.");
      const read = await jira({ query: wanted, issueClass: null, openOnly: false });
      if (read.failure) return refusal(`Jira could not be read: ${read.failure}.`);
      const issue = read.issues.find((entry) => entry.key === wanted);
      if (!issue) return refusal(`${wanted} was not found in Jira.`);
      const gate = deskIssueProblem(desk, issue);
      if (gate) return refusal(gate);
      key = wanted;
      topic = input.topic === undefined ? issueTopic(issue.summary) : normalizeTopic(input.topic);
    } else {
      topic = normalizeTopic(input.topic ?? "");
    }
    const problem = topicProblem(topic);
    if (problem) return refusal(problem);
    const title = leadTitle(input.role, key, topic);
    let source: string | null = null;
    if (input.sourceAgentId?.trim()) {
      try { source = await sourceSummary(client, input.sourceAgentId.trim()); }
      catch (error) { return refusal(`The source session could not be read: ${errorText(error)}.`, title); }
      if (!source) return refusal(`The source session ${input.sourceAgentId.trim()} has no handoff summary to read.`, title);
    }
    const workspaceName = leadWorkspaceName(input.role, input.start), personaKind = personaFor(input.role, input.start), supported = PERSONA_PROVIDERS.includes(input.provider);
    const workspaceId = desk.lead[workspaceName];
    const persona = personaPrompt(personaKind, { desk: desk.project, workspace: workspaceName, workspaceId: workspaceId ?? "id assigned at creation", topic, title, key });
    const labels: Record<string, string> = { role: input.role, [TITLE_AUTO_LABEL]: title, persona: personaVersion(personaKind, supported), [CREATE_REQUEST_LABEL]: input.requestId,
      ...(key && desk.kind === "adam" ? { epic: key } : key && desk.kind === "tickets" ? { ticket: key } : {}) };
    return { spec: {
      requestId: input.requestId, title, labels, provider: input.provider, model: input.model, effort: input.effort, modeId: input.modeId,
      workspaceId, anchorWorkspaceId: desk.anchorWorkspaceId, workspaceName, cwd: desk.cwd,
      createWorkspace: workspaceId ? null : () => createLeadWorkspace(client, desk, workspaceName),
      revalidate: key ? async () => {
        const current = await jira({ query: key!, issueClass: null, openOnly: false });
        if (current.failure) return `Jira could not be read: ${current.failure}.`;
        const issue = current.issues.find((entry) => entry.key === key);
        return issue ? deskIssueProblem(desk, issue) : `${key} is no longer in Jira.`;
      } : null,
      persona: { text: persona, supported }, bootstrap: bootstrapPrompt(persona, source, supported),
    } };
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

  return { options, create: (input: CreateRequest) => createSeat(input.requestId, () => buildNewSession(input)), check, createSeat };
}

export function contributeSessionCreate(server: PluginServerContext): void {
  const root = createStateRoot(), file = createSwitchFile();
  server.handle(readCreateOptions, (input, { paseo }) => createSessionCreateService(paseo, root, file).options(input));
  server.handle(executeCreate, (input, { paseo }) => createSessionCreateService(paseo, root, file).create(input));
  server.handle(checkCreate, (input, { paseo }) => createSessionCreateService(paseo, root, file).check(input.requestId));
}
