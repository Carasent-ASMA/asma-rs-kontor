import { readFile, readdir } from "node:fs/promises";
import { join } from "node:path";
import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { nativePromotionPlacementSchema, promotionRecordSchema, type NativePromotionPlacement, type PromotionRecord } from "../shared/session-promotion";
import { TITLE_AUTO_LABEL } from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Page<T> = { entries: T[]; pageInfo: { hasMore: boolean; nextCursor?: string | null } };

/** The exact host reads shared by the promotion writer and the New-session writer. */
export type CanonicalClient = {
  agents: {
    list(options?: PaseoAgentListOptions): Promise<PaseoAgentListResult>;
    ref(id: string): { refresh(): Promise<{ agent: Agent } | null> };
  };
  workspaces: { ref(id: string): { refresh(): Promise<PaseoWorkspace | null> } };
};

export type Discovery =
  | { kind: "none" }
  | { kind: "one"; agent: Agent }
  | { kind: "archived"; message: string }
  | { kind: "ambiguous"; message: string }
  | { kind: "inflight"; message: string };

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

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
    observedAt: new Date().toISOString(),
    revision: agent.updatedAt,
  });
}

export async function placement(client: CanonicalClient, agentId: string): Promise<{ agent: Agent; workspace: PaseoWorkspace; native: NativePromotionPlacement } | null> {
  const found = await client.agents.ref(agentId).refresh().catch(() => null);
  if (!found || found.agent.id !== agentId) return null;
  const agent = found.agent;
  if (!agent.workspaceId) return null;
  const workspace = await client.workspaces.ref(agent.workspaceId).refresh().catch(() => null);
  if (!workspace || workspace.id !== agent.workspaceId || workspace.archivingAt) return null;
  return { agent, workspace, native: nativePlacementSchema(agent, workspace) };
}

function recordPath(root: string, requestId: string): string {
  return join(root, "promotion-v1", `${requestId}.json`);
}

export async function readRecord(root: string, requestId: string): Promise<PromotionRecord | null> {
  try {
    return promotionRecordSchema.parse(JSON.parse(await readFile(recordPath(root, requestId), "utf8")));
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return null;
    throw error;
  }
}

export async function records(root: string): Promise<PromotionRecord[]> {
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

function promotedBindingProblem(record: PromotionRecord, current: { agent: Agent; native: NativePromotionPlacement }): string | null {
  if (record.state !== "promoted" || record.agentId !== current.agent.id) return null;
  if (current.native.workspaceId !== record.native.workspaceId) return "The promoted session moved out of its recorded native workspace.";
  if ((current.agent.labels?.persona ?? "") !== record.persona.effective) return "The promoted session's persona revision drifted after binding.";
  return null;
}

/** Exact canonical provenance for a receiver: a matching record, or an LSA-workspace label and title for an existing lead. */
export async function canonicalBindingProblem(client: CanonicalClient, root: string, epic: string, agentId: string): Promise<string | null> {
  const current = await placement(client, agentId);
  if (!current) return "The canonical LSA could not be read at its exact native placement.";
  if (current.agent.archivedAt) return "The canonical LSA is archived.";
  const labels = current.agent.labels ?? {};
  if (labels.role !== "LSA") return "The recorded canonical session no longer has the LSA role.";
  if (labels.ticket) return "The recorded canonical session is ticket-bound, not an epic LSA.";
  if (labels.epic && labels.epic !== epic) return `The recorded canonical session is bound to ${labels.epic}, not ${epic}.`;
  let stored: PromotionRecord[];
  try { stored = await records(root); }
  catch (error) { return `Promotion state could not be revalidated: ${errorText(error)}.`; }
  const record = stored.filter((entry) => entry.epic === epic
    && (entry.agentId === agentId && entry.state === "promoted" || entry.canonicalAgentId === agentId && entry.state === "handoff"))
    .sort((a, b) => a.updatedAt.localeCompare(b.updatedAt)).at(-1);
  if (record) return promotedBindingProblem(record, current);
  if (labels.epic !== epic) return "A label-only canonical LSA must retain its exact epic label.";
  if (current.native.workspace.toUpperCase() !== "LSA") return "A label-only canonical LSA must live in the native LSA workspace.";
  const title = labels[TITLE_AUTO_LABEL] ?? current.agent.title ?? "";
  const [role, key] = title.split(" • ").map((part) => part.trim());
  if (role !== "LSA" || key !== epic) return "A label-only canonical LSA must have an LSA • <epic> • <topic> title.auto.";
  return null;
}


/** A durable New-session intent for the same epic epic LSA is a writer in flight, even before it gets labels. */
export async function unsettledCreateProblem(root: string, epic: string): Promise<string | null> {
  let names: string[];
  try { names = (await readdir(root)).filter((name) => /^[0-9a-f-]{36}\.json$/i.test(name)).sort(); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return null;
    return `New-session state could not be read: ${errorText(error)}.`;
  }
  for (const name of names) {
    let parsed: unknown;
    try { parsed = JSON.parse(await readFile(join(root, name), "utf8")); }
    catch { return "A New-session record on the host cannot be read; no canonical admission is allowed."; }
    if (!parsed || typeof parsed !== "object") return "A New-session record on the host is malformed; no canonical admission is allowed.";
    const record = parsed as { request?: { labels?: Record<string, string> }; outcome?: { state?: string } | null };
    const labels = record.request?.labels;
    if (!labels || typeof labels !== "object") return "A New-session record has no readable scope; no canonical admission is allowed.";
    const state = record.outcome?.state;
    if (state && state !== "unknown") continue;
    if (labels.role === "LSA" && labels.epic === epic) return `A New-session request for ${epic} is not settled; no promotion or duplicate lead admission is allowed.`;
  }
  return null;
}

export async function discoverCanonical(client: CanonicalClient, root: string, epic: string, excludeRequestId?: string): Promise<Discovery> {
  let listed: PaseoAgentListResult["entries"];
  try {
    listed = await allPages((cursor) => client.agents.list({ filter: { labels: { role: "LSA", epic }, includeArchived: true },
      page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
  } catch (error) { return { kind: "ambiguous", message: `The canonical lead inventory could not be read: ${errorText(error)}.` }; }
  const exact = listed.map((entry) => entry.agent).filter((agent) => agent.labels?.role === "LSA" && agent.labels?.epic === epic);
  const active = exact.filter((agent) => agent.archivedAt === null);
  const archived = exact.filter((agent) => agent.archivedAt !== null);
  let stored: PromotionRecord[];
  try { stored = await records(root); }
  catch (error) { return { kind: "ambiguous", message: `Promotion state could not be read: ${errorText(error)}.` }; }
  const createProblem = await unsettledCreateProblem(root, epic);
  if (createProblem) return { kind: "inflight", message: createProblem };
  const promoting = stored.filter((entry) => entry.epic === epic && ["promoting", "unknown"].includes(entry.state) && entry.requestId !== excludeRequestId);
  if (promoting.length) return { kind: "inflight", message: `Another promotion request for ${epic} is not settled; no second admission is allowed.` };
  if (active.length > 1) return { kind: "ambiguous", message: `Multiple active canonical LSAs exist for ${epic}; promotion is refused until the owner reconciles them.` };
  const bound = new Map(active.map((agent) => [agent.id, agent]));
  for (const agent of active) {
    const problem = await canonicalBindingProblem(client, root, epic, agent.id);
    if (problem) return { kind: "ambiguous", message: problem };
  }
  for (const entry of stored.filter((candidate) => candidate.epic === epic && candidate.state === "promoted")) {
    if (bound.has(entry.agentId)) continue;
    const current = await placement(client, entry.agentId).catch(() => null);
    if (!current) return { kind: "archived", message: `The promoted LSA for ${epic} is no longer readable. Its owner must reconcile it before another promotion.` };
    if (current.agent.archivedAt) return { kind: "archived", message: `The promoted LSA for ${epic} is archived. No fresh replacement is allowed.` };
    const problem = await canonicalBindingProblem(client, root, epic, current.agent.id);
    if (problem) return { kind: "ambiguous", message: problem };
    bound.set(current.agent.id, current.agent);
  }
  for (const entry of stored.filter((candidate) => candidate.epic === epic && candidate.state === "handoff" && candidate.canonicalAgentId)) {
    if (bound.has(entry.canonicalAgentId!)) continue;
    const current = await placement(client, entry.canonicalAgentId!).catch(() => null);
    if (!current || current.agent.archivedAt) return { kind: "archived", message: `The recorded canonical LSA for ${epic} is unreadable or archived; its owner must reconcile the handoff before another promotion.` };
    const problem = await canonicalBindingProblem(client, root, epic, current.agent.id);
    if (problem) return { kind: "ambiguous", message: problem };
    bound.set(current.agent.id, current.agent);
  }
  if (bound.size > 1) return { kind: "ambiguous", message: `Multiple active canonical LSAs exist for ${epic}; promotion is refused until the owner reconciles them.` };
  if (bound.size === 1) return { kind: "one", agent: [...bound.values()][0]! };
  if (archived.length) return { kind: "archived", message: `An archived canonical LSA exists for ${epic}; it is not revived and no fresh replacement is created.` };
  return { kind: "none" };
}

/** New-session admission: a From-Jira epic LSA may only be created when no canonical owner exists. */
export async function canonicalLeadProblem(client: CanonicalClient, root: string, epic: string): Promise<string | null> {
  const found = await discoverCanonical(client, root, epic);
  if (found.kind === "none") return null;
  if (found.kind === "one") return `Epic ${epic} already has a canonical LSA (${found.agent.id}); bind through that lead instead of creating a duplicate.`;
  return found.message;
}
