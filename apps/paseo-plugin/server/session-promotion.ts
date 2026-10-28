import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoApi, PaseoWorkspace } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { asmaJiraSearch, createStateRoot, createSwitchFile, createSwitchProblem, personaVersion, serial as sessionSerial, type JiraSearch } from "./session-create";
import { canonicalBindingProblem, discoverCanonical, placement, readRecord, records, type Discovery } from "./session-canonical";
import { readPromotion, readPromotionList, promoteSession, type NativePromotionPlacement, type PromotionListEntry, type PromotionOutcome, type PromotionRecord, type PromoteSessionInput, promotionRecordSchema, promotionOutcomeSchema, logicalPromotionPlacementSchema } from "../shared/session-promotion";
import { TITLE_SEPARATOR, type JiraIssue } from "../shared/session-create";

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
type NativePlacement = NativePromotionPlacement;
type SourceState = {
  agent: Agent;
  workspace: PaseoWorkspace;
  native: NativePlacement;
  persona: { issued: string; effective: string };
  legacy: boolean;
};

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const digest = (value: unknown) => createHash("sha256").update(JSON.stringify(value)).digest("hex");
const now = () => new Date().toISOString();

function logicalPlacement(agentId: string, epic: string, persona: string, native: NativePlacement) {
  const base = { workspace: "LSA" as const, node: `epic:${epic}` };
  return logicalPromotionPlacementSchema.parse({ ...base, bindingRevision: digest({ agentId, epic, persona, native }) });
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
  const legacy = issued !== effective && (issued.startsWith("kontor-lsa-adhoc@") || issued.startsWith("kontor-lsa-adhoc+bootstrap@"));
  if (issued !== effective && !legacy) return "This session does not carry a current ADHOC LSA persona.";
  return { agent: current.agent, workspace: current.workspace, native: current.native, persona: { issued, effective }, legacy };
}

/** Exact binding, role, epic and placement revalidation after any awaited read and immediately before/after an effect. */
function sourceBindingProblem(after: { agent: Agent; workspace: PaseoWorkspace; native: NativePlacement } | null, source: SourceState, epic: string): string | null {
  if (!after || after.agent.id !== source.agent.id) return "The source session's exact native identity changed.";
  if (after.agent.archivedAt) return "The source session became archived before the effect.";
  if (after.native.workspaceId !== source.native.workspaceId || after.agent.cwd !== source.agent.cwd) return "The source session's native placement changed before the effect.";
  if (after.native.titleAuto !== source.native.titleAuto) return "The source session's title.auto changed before the effect.";
  const labels = after.agent.labels ?? {};
  if (labels.role !== "LSA" || labels.ticket || (labels.epic && labels.epic !== epic)) return "The source session's role or epic binding changed before the effect.";
  if ((labels.persona ?? "") !== source.persona.issued) return "The source session's persona revision changed before the effect.";
  return null;
}

function recordPath(root: string, requestId: string): string {
  return join(root, "promotion-v1", `${requestId}.json`);
}

async function writeRecord(root: string, record: PromotionRecord): Promise<void> {
  const path = recordPath(root, record.requestId);
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${randomUUID()}.tmp`;
  const file = await open(temporary, "w", 0o600);
  try { await file.writeFile(JSON.stringify(promotionRecordSchema.parse(record))); await file.sync(); } finally { await file.close(); }
  await rename(temporary, path);
}

function outcome(record: PromotionRecord): PromotionOutcome {
  const state = record.state === "promoted" ? "promoted" : record.state === "handoff" ? "handoff" : "unknown";
  return promotionOutcomeSchema.parse({
    requestId: record.requestId, state, agentId: record.agentId, epic: record.epic,
    canonicalAgentId: record.canonicalAgentId, native: record.native, logical: record.logical,
    summarySha256: record.summarySha256, summaryRevision: record.summaryRevision, acknowledgement: record.acknowledgement,
    message: record.message, checkedAt: now(),
  });
}

function refused(input: PromoteSessionInput, message: string): PromotionOutcome {
  return promotionOutcomeSchema.parse({ requestId: input.requestId, state: "refused", agentId: input.agentId, epic: input.epic,
    canonicalAgentId: null, native: null, logical: null, summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message, checkedAt: now() });
}

function unchanged(input: PromoteSessionInput, source: SourceState): PromotionOutcome {
  return promotionOutcomeSchema.parse({ requestId: input.requestId, state: "unchanged", agentId: input.agentId, epic: input.epic,
    canonicalAgentId: input.agentId, native: source.native, logical: logicalPlacement(input.agentId, input.epic, source.persona.effective, source.native),
    summarySha256: null, summaryRevision: null, acknowledgement: "not-required", message: "This session is already the epic's canonical LSA; no promotion effect was dispatched.", checkedAt: now() });
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

function summaryProblem(text: string): string | null {
  const missing = ["Scope", "Artifacts", "Verified", "Unverified", "Next action"].filter((heading) => !new RegExp(`^${heading}:`, "m").test(text));
  return missing.length ? `The handoff summary is missing required section${missing.length === 1 ? "" : "s"}: ${missing.join(", ")}.` : null;
}

function recordFor(input: PromoteSessionInput, source: SourceState, branch: "promote" | "handoff", state: PromotionRecord["state"],
  canonicalAgentId: string | null, summarySha256: string | null, summaryRevision: string | null, acknowledgement: PromotionRecord["acknowledgement"], message: string): PromotionRecord {
  const logical = logicalPlacement(input.agentId, input.epic, source.persona.effective, source.native);
  const stamp = now();
  return promotionRecordSchema.parse({ schemaVersion: 1, requestId: input.requestId, agentId: input.agentId, epic: input.epic, branch, state,
    native: source.native, logical, persona: source.persona, canonicalAgentId, summarySha256, summaryRevision, acknowledgement, message, createdAt: stamp, updatedAt: stamp });
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

function handoffPrompt(source: SourceState, input: PromoteSessionInput, summary: string, summarySha256: string, summaryRevision: string): string {
  return [
    `Kontor handoff to the canonical epic LSA ${input.epic}. Source ADHOC LSA: ${source.agent.id}.`,
    `Summary revision: ${summaryRevision}. Summary sha256: ${summarySha256}.`,
    "",
    summary,
    "",
    `Acknowledge the exact revision and content by replying with only: HANDOFF-ACK ${input.epic} ${summaryRevision} ${summarySha256}`,
    "Do not create or relabel a second lead.",
  ].join("\n");
}

export function createPromotionService(client: PromotionClient, root: string, switchFile = createSwitchFile(), jira: JiraSearch = asmaJiraSearch) {
  async function promote(input: PromoteSessionInput): Promise<PromotionOutcome> {
    return sessionSerial(async () => {
      try {
        const earlier = await readRecord(root, input.requestId);
        const summaryMatches = !earlier || earlier.branch !== "handoff" || (input.summary !== undefined
          && earlier.summaryRevision === input.summary.revision && earlier.summarySha256 === createHash("sha256").update(input.summary.text).digest("hex"));
        if (earlier) return earlier.agentId === input.agentId && earlier.epic === input.epic && summaryMatches ? outcome(earlier) : refused(input, "This request id is already recorded for another session, epic or summary payload.");
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
        const found: Discovery = await discoverCanonical(client, root, input.epic, input.requestId);
        if (found.kind === "ambiguous" || found.kind === "archived" || found.kind === "inflight") return refused(input, found.message);
        if (found.kind === "one" && found.agent.id === input.agentId) return unchanged(input, source);
        if (found.kind === "one") {
          const canonicalProblem = await canonicalBindingProblem(client, root, input.epic, found.agent.id);
          if (canonicalProblem) return refused(input, canonicalProblem);
          if (!input.summary) return refused(input, "An existing canonical epic LSA requires an explicitly authored handoff summary and its exact artifact revision.");
          const summaryProblemText = summaryProblem(input.summary.text);
          if (summaryProblemText) return refused(input, summaryProblemText);
          const before = await placement(client, input.agentId).catch(() => null);
          const drift = sourceBindingProblem(before, source, input.epic);
          if (drift) return refused(input, drift);
          const summarySha256 = createHash("sha256").update(input.summary.text).digest("hex");
          const pending = recordFor(input, source, "handoff", "promoting", found.agent.id, summarySha256, input.summary.revision, "unknown", "Handoff intent recorded; awaiting the exact receiver acknowledgement.");
          await writeRecord(root, pending);
          let acknowledgement: PromotionRecord["acknowledgement"] = "unknown";
          try {
            const result = await client.agents.ref(found.agent.id).run(handoffPrompt(source, input, input.summary.text, summarySha256, input.summary.revision), { timeoutMs: 120_000 });
            const expected = `HANDOFF-ACK ${input.epic} ${input.summary.revision} ${summarySha256}`;
            acknowledgement = result?.status === "idle" && result.lastMessage?.trim().split("\n").at(-1)?.trim() === expected ? "verified" : "unknown";
          } catch {}
          const afterProblem = await canonicalBindingProblem(client, root, input.epic, found.agent.id);
          const sourceAfter = await placement(client, input.agentId).catch(() => null);
          if (afterProblem || sourceBindingProblem(sourceAfter, source, input.epic)) acknowledgement = "unknown";
          const state: PromotionRecord["state"] = acknowledgement === "verified" ? "handoff" : "unknown";
          const final = { ...pending, state, acknowledgement, updatedAt: now(),
            message: state === "handoff" ? "Handoff delivered to the exact existing canonical LSA and its exact-revision acknowledgement was read; source release remains an operator decision."
              : "Handoff delivery or its exact-revision acknowledgement is unconfirmed. Do not resend; reconcile the recorded intent first." };
          await writeRecord(root, final);
          return outcome(final);
        }
        if (source.legacy) return refused(input, "This session carries a legacy fixed ADHOC persona. Supported same-ID instruction refresh is unavailable, so it cannot become a new canonical lead; report the legacy gap.");
        const before = await placement(client, input.agentId).catch(() => null);
        const drift = sourceBindingProblem(before, source, input.epic);
        if (drift) return refused(input, drift);
        const pending = recordFor(input, source, "promote", "promoting", input.agentId, null, null, "not-required", "Promotion intent recorded; binding context is being delivered.");
        await writeRecord(root, pending);
        let delivered = false;
        try { await client.agents.ref(input.agentId).send(contextMessage(source, input, pending.logical)); delivered = true; } catch {}
        const after = await placement(client, input.agentId).catch(() => null);
        if (sourceBindingProblem(after, source, input.epic)) delivered = false;
        const final = { ...pending, state: delivered ? "promoted" as const : "unknown" as const, updatedAt: now(),
          message: delivered ? "Promoted in place: the native ADHOC origin is retained and the logical epic LSA binding is recorded; no native move or replacement was claimed."
            : "Binding context delivery is unconfirmed. Do not resend; reconcile the recorded intent first." };
        await writeRecord(root, final);
        return outcome(final);
      } catch (error) {
        return { requestId: input.requestId, state: "unknown", agentId: input.agentId, epic: input.epic, canonicalAgentId: null, native: null, logical: null,
          summarySha256: null, summaryRevision: null, acknowledgement: "unknown", message: `Promotion state is unreadable or the host read failed: ${errorText(error)}. Do not retry blindly.`, checkedAt: now() };
      }
    });
  }

  async function read(agentId: string): Promise<{ record: PromotionRecord; native: NativePlacement | null } | null> {
    const found = (await records(root)).filter((entry) => entry.agentId === agentId).sort((a, b) => a.updatedAt.localeCompare(b.updatedAt)).at(-1);
    if (!found) return null;
    const current = await placement(client, agentId).catch(() => null);
    return { record: found, native: current?.native ?? null };
  }

  async function list(): Promise<PromotionListEntry[]> {
    const all = await records(root);
    return Promise.all(all.map(async (record): Promise<PromotionListEntry> => {
      if (record.state !== "promoted" && record.state !== "handoff") return { record, validation: { state: "unknown", message: "The promotion is not a settled canonical binding." } };
      const owner = record.state === "promoted" ? record.agentId : record.canonicalAgentId;
      if (!owner) return { record, validation: { state: "unknown", message: "The record names no canonical owner." } };
      const problem = await canonicalBindingProblem(client, root, record.epic, owner);
      return { record, validation: problem ? { state: "stale", message: problem } : { state: "current", message: "The canonical binding matches its exact native owner evidence." } };
    }));
  }

  return { promote, read, list };
}

export function contributeSessionPromotion(server: PluginServerContext): void {
  const root = createStateRoot();
  server.handle(promoteSession, (input, { paseo }) => createPromotionService(paseo, root).promote(input));
  server.handle(readPromotion, (input, { paseo }) => createPromotionService(paseo, root).read(input.agentId));
  server.handle(readPromotionList, (_input, { paseo }) => createPromotionService(paseo, root).list());
}
