import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, readdir, rename, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoApi, PaseoWorkspace } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { asmaJiraSearch, createStateRoot, createSwitchFile, createSwitchProblem, personaVersion, serial as sessionSerial, sourceSummary, type JiraSearch } from "./session-create";
import { readPromotion, promoteSession, type NativePromotionPlacement, type PromotionOutcome, type PromotionRecord, type PromoteSessionInput, promotionRecordSchema, promotionOutcomeSchema, nativePromotionPlacementSchema, logicalPromotionPlacementSchema } from "../shared/session-promotion";
import { TITLE_AUTO_LABEL, TITLE_SEPARATOR, type JiraIssue } from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type AgentRef = ReturnType<PaseoApi["agents"]["ref"]>;
type WorkspaceRef = ReturnType<PaseoApi["workspaces"]["ref"]>;
export type PromotionClient = {
  agents: {
    list(options?: PaseoAgentListOptions): Promise<PaseoAgentListResult>;
    ref(id: string): Pick<AgentRef, "refresh" | "timeline" | "send" | "run">;
  };
  workspaces: { ref(id: string): Pick<WorkspaceRef, "refresh"> };
};
type Page<T> = { entries: T[]; pageInfo: { hasMore: boolean; nextCursor?: string | null } };
type NativePlacement = NativePromotionPlacement;
type SourceState = {
  agent: Agent;
  workspace: PaseoWorkspace;
  native: NativePlacement;
  persona: { issued: string; effective: string };
};
type Discovery =
  | { kind: "none" }
  | { kind: "one"; agent: Agent }
  | { kind: "archived"; message: string }
  | { kind: "ambiguous"; message: string }
  | { kind: "inflight"; message: string };

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const digest = (value: unknown) => createHash("sha256").update(JSON.stringify(value)).digest("hex");
const now = () => new Date().toISOString();
async function allPages<T>(read: (cursor?: string) => Promise<Page<T>>): Promise<T[]> {
  const rows: T[] = [], cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1_000; page++) {
    const result = await read(cursor);
    rows.push(...result.entries);
    if (!result.pageInfo.hasMore) return rows;
    const next = result.pageInfo.nextCursor;
    if (!next || cursors.has(next)) throw new Error("The host returned an incomplete list.");
    cursors.add(next);
    cursor = next;
  }
  throw new Error("The host list exceeded the bounded page walk.");
}

function nativePlacementSchema(agent: Agent, workspace: PaseoWorkspace) {
  const titleAuto = agent.labels?.[TITLE_AUTO_LABEL] ?? "";
  return nativePromotionPlacementSchema.parse({
    workspaceId: workspace.id,
    workspace: (workspace.title ?? workspace.name ?? "").trim(),
    title: agent.title ?? titleAuto,
    titleAuto,
    observedAt: now(),
    revision: agent.updatedAt,
  });
}

function logicalPlacement(agentId: string, epic: string, persona: string, native: NativePlacement) {
  const base = { workspace: "LSA" as const, node: `epic:${epic}` };
  return logicalPromotionPlacementSchema.parse({ ...base, bindingRevision: digest({ agentId, epic, persona, native }) });
}

async function placement(client: PromotionClient, agentId: string): Promise<{ agent: Agent; workspace: PaseoWorkspace; native: NativePlacement } | null> {
  const found = await client.agents.ref(agentId).refresh().catch(() => null);
  if (!found) return null;
  const agent = found.agent;
  if (!agent.workspaceId) return null;
  const workspace = await client.workspaces.ref(agent.workspaceId).refresh().catch(() => null);
  if (!workspace || workspace.archivingAt) return null;
  return { agent, workspace, native: nativePlacementSchema(agent, workspace) };
}

function sourceState(current: { agent: Agent; workspace: PaseoWorkspace; native: NativePlacement }, epic: string): SourceState | string {
  const labels = current.agent.labels ?? {}, workspaceName = current.native.workspace.toUpperCase();
  if (current.agent.archivedAt) return "The source session is archived; promotion never revives an archived session.";
  if (workspaceName !== "ADHOC") return "Only a session in the native ADHOC lead workspace can be promoted in place.";
  if (labels.role !== "LSA") return "Only an ADHOC LSA can be promoted; this session has another role.";
  if (labels.ticket) return "A ticket-bound session is not an ADHOC LSA.";
  if (labels.epic && labels.epic !== epic) return `This session is already bound to ${labels.epic}; it cannot be rebound to ${epic}.`;
  if (!current.agent.cwd || current.agent.cwd !== current.workspace.workspaceDirectory) return "The source session's native working directory no longer matches its ADHOC workspace.";
  const [role, scope, topic] = current.native.titleAuto.split(TITLE_SEPARATOR);
  if (role !== "LSA" || scope !== "ADHOC" || !topic?.trim()) return "The source session's title.auto is not an LSA • ADHOC • <topic> title.";
  const issued = labels.persona ?? "";
  const effective = personaVersion("lsa-ad-hoc", true);
  if (issued !== effective) return issued.startsWith("kontor-lsa-adhoc@") || issued.startsWith("kontor-lsa-adhoc+bootstrap@")
    ? "This session carries a legacy fixed ADHOC persona. Supported same-ID instruction refresh is unavailable, so promotion would leave contradictory startup instructions in force; start a current-persona ADHOC session or report the legacy gap."
    : "This session does not carry a current ADHOC LSA persona.";
  return { agent: current.agent, workspace: current.workspace, native: current.native, persona: { issued, effective } };
}

function recordPath(root: string, requestId: string): string {
  return join(root, "promotion-v1", `${requestId}.json`);
}

async function readRecord(root: string, requestId: string): Promise<PromotionRecord | null> {
  try {
    return promotionRecordSchema.parse(JSON.parse(await readFile(recordPath(root, requestId), "utf8")));
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return null;
    throw error;
  }
}

async function records(root: string): Promise<PromotionRecord[]> {
  let names: string[];
  try { names = await readdir(join(root, "promotion-v1")); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return [];
    throw error;
  }
  const result: PromotionRecord[] = [];
  for (const name of names.filter((entry) => entry.endsWith(".json")).sort()) {
    result.push(promotionRecordSchema.parse(JSON.parse(await readFile(join(root, "promotion-v1", name), "utf8"))));
  }
  return result;
}

async function writeRecord(root: string, record: PromotionRecord): Promise<void> {
  const path = recordPath(root, record.requestId);
  await mkdir(dirname(path), { recursive: true });
  const temporary = `${path}.${randomUUID()}.tmp`;
  await writeFile(temporary, JSON.stringify(promotionRecordSchema.parse(record)));
  await rename(temporary, path);
}

function outcome(record: PromotionRecord): PromotionOutcome {
  const state = record.state === "promoted" ? "promoted" : record.state === "handoff" ? "handoff" : "unknown";
  return promotionOutcomeSchema.parse({
    requestId: record.requestId, state, agentId: record.agentId, epic: record.epic,
    canonicalAgentId: record.canonicalAgentId, native: record.native, logical: record.logical,
    summarySha256: record.summarySha256, acknowledgement: record.acknowledgement,
    message: record.message, checkedAt: now(),
  });
}

function refused(input: PromoteSessionInput, message: string): PromotionOutcome {
  return promotionOutcomeSchema.parse({ requestId: input.requestId, state: "refused", agentId: input.agentId, epic: input.epic,
    canonicalAgentId: null, native: null, logical: null, summarySha256: null, acknowledgement: "not-required", message, checkedAt: now() });
}

function unchanged(input: PromoteSessionInput, source: SourceState): PromotionOutcome {
  return promotionOutcomeSchema.parse({ requestId: input.requestId, state: "unchanged", agentId: input.agentId, epic: input.epic,
    canonicalAgentId: input.agentId, native: source.native, logical: logicalPlacement(input.agentId, input.epic, source.persona.effective, source.native),
    summarySha256: null, acknowledgement: "not-required", message: "This session is already the epic's canonical LSA; no promotion effect was dispatched.", checkedAt: now() });
}

async function validEpic(epic: string, jira: JiraSearch): Promise<JiraIssue | string> {
  let read;
  try { read = await jira({ query: epic, issueClass: "epic", openOnly: true }); }
  catch (error) { return `Jira could not be read: ${errorText(error)}.`; }
  if (read.failure) return `Jira could not be read: ${read.failure}.`;
  const issue = read.issues.find((entry) => entry.key === epic);
  if (!issue) return `${epic} was not found in Jira.`;
  if (!issue.epic) return `${epic} is not an epic.`;
  if (!issue.open) return `${epic} is ${issue.status}, not open.`;
  return issue;
}

async function discover(client: PromotionClient, root: string, input: PromoteSessionInput): Promise<Discovery> {
  let listed: PaseoAgentListResult["entries"];
  try {
    listed = await allPages((cursor) => client.agents.list({ filter: { labels: { role: "LSA", epic: input.epic }, includeArchived: true },
      page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
  } catch (error) { return { kind: "ambiguous", message: `The canonical lead inventory could not be read: ${errorText(error)}.` }; }
  const exact = listed.map((entry) => entry.agent).filter((agent) => agent.labels?.role === "LSA" && agent.labels?.epic === input.epic);
  const active = exact.filter((agent) => agent.archivedAt === null);
  const archived = exact.filter((agent) => agent.archivedAt !== null);
  let stored: PromotionRecord[];
  try { stored = await records(root); }
  catch (error) { return { kind: "ambiguous", message: `Promotion state could not be read: ${errorText(error)}.` }; }
  const promoting = stored.filter((entry) => entry.epic === input.epic && ["promoting", "unknown"].includes(entry.state) && entry.requestId !== input.requestId);
  if (promoting.length) return { kind: "inflight", message: `Another promotion request for ${input.epic} is not settled; no second admission is allowed.` };
  const bound = new Map(active.map((agent) => [agent.id, agent]));
  for (const entry of stored.filter((candidate) => candidate.epic === input.epic && candidate.state === "promoted")) {
    if (bound.has(entry.agentId)) continue;
    const current = await placement(client, entry.agentId).catch(() => null);
    if (!current) return { kind: "archived", message: `The promoted LSA for ${input.epic} is no longer readable. Its owner must reconcile it before another promotion.` };
    if (current.agent.archivedAt) return { kind: "archived", message: `The promoted LSA for ${input.epic} is archived. No fresh replacement is allowed.` };
    bound.set(current.agent.id, current.agent);
  }
  for (const entry of stored.filter((candidate) => candidate.epic === input.epic && candidate.state === "handoff" && candidate.canonicalAgentId)) {
    if (bound.has(entry.canonicalAgentId!)) continue;
    const current = await placement(client, entry.canonicalAgentId!).catch(() => null);
    if (!current || current.agent.archivedAt) return { kind: "archived", message: `The recorded canonical LSA for ${input.epic} is unreadable or archived; its owner must reconcile the handoff before another promotion.` };
    bound.set(current.agent.id, current.agent);
  }
  if (bound.size > 1) return { kind: "ambiguous", message: `Multiple active canonical LSAs exist for ${input.epic}; promotion is refused until the owner reconciles them.` };
  if (bound.size === 1) return { kind: "one", agent: [...bound.values()][0]! };
  if (archived.length) return { kind: "archived", message: `An archived canonical LSA exists for ${input.epic}; it is not revived and no fresh replacement is created.` };
  return { kind: "none" };
}

function recordFor(input: PromoteSessionInput, source: SourceState, branch: "promote" | "handoff", state: PromotionRecord["state"],
  canonicalAgentId: string | null, summarySha256: string | null, acknowledgement: PromotionRecord["acknowledgement"], message: string): PromotionRecord {
  const logical = logicalPlacement(input.agentId, input.epic, source.persona.effective, source.native);
  const stamp = now();
  return promotionRecordSchema.parse({ schemaVersion: 1, requestId: input.requestId, agentId: input.agentId, epic: input.epic, branch, state,
    native: source.native, logical, persona: source.persona, canonicalAgentId, summarySha256, acknowledgement, message, createdAt: stamp, updatedAt: stamp });
}

function contextMessage(source: SourceState, input: PromoteSessionInput, logical: PromotionRecord["logical"]): string {
  return [
    "Kontor promotion context (authoritative binding, not a task).",
    `Epic: ${input.epic}. Logical lead node: ${logical.node}.`,
    `Native origin: ${source.native.workspace} (${source.native.workspaceId}) is retained; do not claim a native workspace move.`,
    `Persona effective revision: ${source.persona.effective}. Issued revision: ${source.persona.issued}.`,
    `Binding revision: ${logical.bindingRevision}.`,
    "Treat this same session as the epic's canonical LSA for architecture, scope coherence and decisions. Keep the operator title, conversation, model, TPM ownership and credential boundaries. Do not create a replacement lead or rename yourself.",
  ].join("\n");
}

function handoffPrompt(source: SourceState, input: PromoteSessionInput, summary: string, summarySha256: string): string {
  return [
    `Kontor handoff to the canonical epic LSA ${input.epic}. Source ADHOC LSA: ${source.agent.id}.`,
    `Summary revision: ${summarySha256}.`,
    "",
    summary,
    "",
    `Acknowledge the exact revision by replying with only: HANDOFF-ACK ${input.epic} ${summarySha256}`,
    "Do not create or relabel a second lead.",
  ].join("\n");
}

export function createPromotionService(client: PromotionClient, root: string, switchFile = createSwitchFile(), jira: JiraSearch = asmaJiraSearch) {
  async function promote(input: PromoteSessionInput): Promise<PromotionOutcome> {
    return sessionSerial(async () => {
      try {
        const earlier = await readRecord(root, input.requestId);
        if (earlier) return earlier.agentId === input.agentId ? outcome(earlier) : refused(input, "This request id is already recorded for another session.");
        const prior = (await records(root)).filter((entry) => entry.agentId === input.agentId).sort((a, b) => a.updatedAt.localeCompare(b.updatedAt)).at(-1);
        if (prior) return prior.epic === input.epic ? outcome(prior) : refused(input, `This session already has a promotion record for ${prior.epic}; it cannot be rebound to ${input.epic}.`);
        const switched = createSwitchProblem(switchFile);
        if (switched) return refused(input, switched);
        const issue = await validEpic(input.epic, jira);
        if (typeof issue === "string") return refused(input, issue);
        const current = await placement(client, input.agentId);
        if (!current) return refused(input, "The source session could not be read at its exact native placement.");
        const source = sourceState(current, input.epic);
        if (typeof source === "string") return refused(input, source);
        const found = await discover(client, root, input);
        if (found.kind === "ambiguous" || found.kind === "archived" || found.kind === "inflight") return refused(input, found.message);
        if (found.kind === "one" && found.agent.id === input.agentId) return unchanged(input, source);
        if (found.kind === "one") {
          const canonical = await placement(client, found.agent.id).catch(() => null);
          if (!canonical || canonical.agent.archivedAt) return refused(input, "The existing canonical LSA could not be read as active; nothing was sent.");
          let summary: string | null;
          try { summary = await sourceSummary(client, input.agentId); }
          catch (error) { return refused(input, `The source handoff summary could not be read: ${errorText(error)}.`); }
          if (!summary) return refused(input, "The source session has no handoff summary to deliver; the existing lead was not changed.");
          const summarySha256 = createHash("sha256").update(summary).digest("hex");
          const pending = recordFor(input, source, "handoff", "promoting", canonical.agent.id, summarySha256, "unknown", "Handoff intent recorded; awaiting the exact receiver acknowledgement.");
          await writeRecord(root, pending);
          let acknowledgement: PromotionRecord["acknowledgement"] = "unknown";
          try {
            const result = await client.agents.ref(canonical.agent.id).run(handoffPrompt(source, input, summary, summarySha256), { timeoutMs: 120_000 });
            const expected = `HANDOFF-ACK ${input.epic} ${summarySha256}`;
            acknowledgement = result?.lastMessage?.trim().split("\n").at(-1)?.trim() === expected ? "verified" : "unknown";
          } catch {}
          const after = await placement(client, canonical.agent.id).catch(() => null);
          if (!after || after.agent.archivedAt) acknowledgement = "unknown";
          const state: PromotionRecord["state"] = acknowledgement === "verified" ? "handoff" : "unknown";
          const final = { ...pending, state, acknowledgement, updatedAt: now(),
            message: state === "handoff" ? "Handoff delivered to the exact existing canonical LSA and its exact-revision acknowledgement was read; source release remains an operator decision."
              : "Handoff delivery or its exact-revision acknowledgement is unconfirmed. Do not resend; reconcile the recorded intent first." };
          await writeRecord(root, final);
          return outcome(final);
        }
        const pending = recordFor(input, source, "promote", "promoting", input.agentId, null, "not-required", "Promotion intent recorded; binding context is being delivered.");
        await writeRecord(root, pending);
        let delivered = false;
        try { await client.agents.ref(input.agentId).send(contextMessage(source, input, pending.logical)); delivered = true; } catch {}
        const after = await placement(client, input.agentId).catch(() => null);
        if (!after || after.agent.id !== source.agent.id || after.agent.archivedAt || after.native.workspaceId !== source.native.workspaceId || after.agent.cwd !== source.agent.cwd) delivered = false;
        const final = { ...pending, state: delivered ? "promoted" as const : "unknown" as const, updatedAt: now(),
          message: delivered ? "Promoted in place: the native ADHOC origin is retained and the logical epic LSA binding is recorded; no native move or replacement was claimed."
            : "Binding context delivery is unconfirmed. Do not resend; reconcile the recorded intent first." };
        await writeRecord(root, final);
        return outcome(final);
      } catch (error) {
        return { requestId: input.requestId, state: "unknown", agentId: input.agentId, epic: input.epic, canonicalAgentId: null, native: null, logical: null,
          summarySha256: null, acknowledgement: "unknown", message: `Promotion state is unreadable or the host read failed: ${errorText(error)}. Do not retry blindly.`, checkedAt: now() };
      }
    });
  }

  async function read(agentId: string): Promise<{ record: PromotionRecord; native: NativePlacement | null } | null> {
    const found = (await records(root)).filter((entry) => entry.agentId === agentId).sort((a, b) => a.updatedAt.localeCompare(b.updatedAt)).at(-1);
    if (!found) return null;
    const current = await placement(client, agentId).catch(() => null);
    return { record: found, native: current?.native ?? null };
  }

  return { promote, read };
}

export function contributeSessionPromotion(server: PluginServerContext): void {
  const root = createStateRoot();
  server.handle(promoteSession, (input, { paseo }) => createPromotionService(paseo, root).promote(input));
  server.handle(readPromotion, (input, { paseo }) => createPromotionService(paseo, root).read(input.agentId));
}
