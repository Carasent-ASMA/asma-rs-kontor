import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { mkdir, open, link, readFile, readdir, unlink } from "node:fs/promises";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { execFile } from "node:child_process";
import { promisify, isDeepStrictEqual } from "node:util";
import type { PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { z } from "zod";
import { readConfig } from "./config";
import { createSwitchFile, createSwitchProblem } from "./session-create";
import { hasKontorLabels, parent } from "./labels";
import { takeoverAdapter } from "./session-takeover";
import { lifecycleRevision } from "./session-lifecycle";
import { within } from "./placement";
import {
  proceduralAdmissionSchema,
  proceduralReceiptSchema,
  prepareProceduralTakeover,
  executeProceduralTakeover,
  checkProceduralTakeover,
  PROCEDURAL_LIMITS,
  type ProceduralAdmission,
  type ProceduralReceipt,
} from "../shared/procedural-takeover";

type Ref = ProceduralAdmission["handoff"];
type Agent = NonNullable<
  Awaited<ReturnType<ReturnType<PaseoApi["agents"]["ref"]>["refresh"]>>
>["agent"];
const exec = promisify(execFile);
export const takeoverHash = (value: string | Buffer) =>
  createHash("sha256").update(value).digest("hex");
function requireFact(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
const code = (error: unknown, value: string) =>
  error instanceof Error && "code" in error && error.code === value;
const id = z.string().uuid();
const lifecycleClaimSchema = z
  .object({
    reviewId: id,
    agentId: id,
    action: z.literal("takeover"),
    phase: z.literal("procedural"),
    fingerprint: z.string().length(64),
    token: id,
  })
  .strict();
const executionClaimSchema = z
  .object({
    operationId: id,
    admissionHash: z.string().length(64),
    lifecycleClaims: z.record(id, lifecycleClaimSchema),
  })
  .strict();
// One installed plugin worker owns these paths; serialize its concurrent check/cleanup calls.
const childSettlementSteps = new Map<string, Promise<void>>();

/** A procedural owner supplies bounded, hash-bound public evidence; this never reads Paseo's private store. */
async function referenced(ref: Ref): Promise<string> {
  requireFact(isAbsolute(ref.path), "Evidence requires an absolute path");
  const file = await open(ref.path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const stat = await file.stat();
    requireFact(
      stat.isFile() && stat.nlink === 1 && stat.size <= 8_000_000,
      "Evidence must be a bounded regular file",
    );
    const bytes = await file.readFile();
    requireFact(takeoverHash(bytes) === ref.sha256, "Prepared evidence changed");
    return bytes.toString();
  } finally {
    await file.close();
  }
}
async function immutable(path: string, value: unknown): Promise<void> {
  await mkdir(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${randomUUID()}.tmp`,
    file = await open(temporary, "wx", 0o600);
  try {
    await file.writeFile(JSON.stringify(value));
    await file.sync();
  } finally {
    await file.close();
  }
  try {
    await link(temporary, path);
  } finally {
    await unlink(temporary);
  }
  const directory = await open(dirname(path), "r");
  try {
    await directory.sync();
  } finally {
    await directory.close();
  }
}
export async function takeoverCheckout(path: string): Promise<ProceduralAdmission["checkout"]> {
  requireFact(isAbsolute(path), "Checkout requires an absolute path");
  const git = async (...args: string[]) =>
    (await exec("git", ["--no-optional-locks", "-C", path, ...args], { maxBuffer: 16_000_000 }))
      .stdout;
  const branch = (await git("symbolic-ref", "--short", "HEAD")).trim(),
    head = (await git("rev-parse", "HEAD")).trim();
  const parts: (string | Buffer)[] = [
    await git("diff", "--binary", "HEAD", "--"),
    await git("status", "--porcelain=v1", "-z", "--untracked-files=all"),
  ];
  for (const name of (await git("ls-files", "--others", "--exclude-standard", "-z"))
    .split("\0")
    .filter(Boolean)
    .sort()) {
    const file = await open(join(path, name), constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
      const stat = await file.stat();
      requireFact(
        stat.isFile() && stat.size <= 16_000_000,
        "Untracked checkout entry exceeds its bound or is not a regular file",
      );
      parts.push(name, await file.readFile());
    } finally {
      await file.close();
    }
  }
  const hash = createHash("sha256");
  for (const part of parts) {
    hash.update(String(Buffer.byteLength(part)));
    hash.update(":");
    hash.update(part);
  }
  return { path, branch, head, wip: hash.digest("hex") };
}

const accountingSchema = z
  .object({
    operationId: id,
    sourceId: id,
    mode: proceduralAdmissionSchema.shape.mode,
    reviewedBy: z.string().min(1),
    observedAt: z.iso.datetime(),
    expiresAt: z.iso.datetime(),
    coverage: z.literal("owner-reviewed-not-host-certified"),
    schedules: z.array(
      z
        .object({
          id: z.string().min(1),
          status: z.enum(["paused", "completed"]),
          runningRunIds: z.array(z.string()).max(0),
          source: z.object({ path: z.string(), sha256: z.string() }),
        })
        .strict(),
    ),
    workers: z.array(
      z
        .object({
          id: z.string(),
          state: z.enum(["finished", "checkpointed"]),
          checkpoint: z.object({ path: z.string(), sha256: z.string() }),
        })
        .strict(),
    ),
    reporting: z.object({ ownerId: z.string().min(1), deadline: z.iso.datetime() }).strict(),
    accountPrincipal: z.literal("unverified"),
    changedCondition: z.string().min(1),
  })
  .strict();
const ackSchema = z
  .object({
    operationId: id,
    successorId: id,
    stage: z.enum(["bootstrap", "continuation"]),
    admissionHash: z.string().length(64),
    authoritySha256: z.string().length(64),
    checkout: proceduralAdmissionSchema.shape.checkout,
    acknowledged: z.literal(true),
  })
  .strict();
const copyLabels = (agent: Agent) =>
  Object.fromEntries(
    Object.entries(agent.labels ?? {}).filter(([key]) =>
      ["role", "epic", "task", "ticket", "orchestration", "supervisor", "write.scope"].includes(
        key,
      ),
    ),
  );

/** One Kontor-local source claim and immutable operation history. Claims are never stolen by age. */
export function createProceduralTakeover(client: PaseoApi, root: string, hostId: string) {
  const directory = (operationId: string) => join(root, "operations", id.parse(operationId));
  const lifecycleClaim = (agentId: string) =>
    join(dirname(root), "session-lifecycle-v1", `intent-${takeoverHash(agentId)}.json`);
  async function read(operationId: string): Promise<ProceduralReceipt> {
    const names = (await readdir(directory(operationId)))
      .filter((name) => /^\d+\.json$/.test(name))
      .map((name) => Number(name.slice(0, -5)))
      .sort((a, b) => a - b);
    let last: ProceduralReceipt | undefined;
    for (const revision of names) {
      requireFact(revision === (last ? last.revision + 1 : 0), "Operation history gap");
      const next = proceduralReceiptSchema.parse(
        JSON.parse(await readFile(join(directory(operationId), `${revision}.json`), "utf8")),
      );
      requireFact(
        next.operationId === operationId &&
          next.admission.operationId === operationId &&
          next.sourceId === next.admission.sourceId &&
          next.revision === revision &&
          (!last ||
            (next.admissionHash === last.admissionHash &&
              next.sourceId === last.sourceId &&
              next.successorId === last.successorId &&
              isDeepStrictEqual(next.admission, last.admission) &&
              isDeepStrictEqual(next.nativeHistory, last.nativeHistory) &&
              isDeepStrictEqual(next.identityLabels, last.identityLabels))),
        "Operation identity changed",
      );
      last = next;
    }
    requireFact(last, "Operation not found");
    return last;
  }
  const save = (receipt: ProceduralReceipt) =>
    immutable(
      join(directory(receipt.operationId), `${receipt.revision}.json`),
      proceduralReceiptSchema.parse(receipt),
    );
  async function exact(agentId: string) {
    const result = await client.agents.ref(agentId).refresh();
    requireFact(result?.agent.id === agentId, "Exact native agent readback missing");
    return result;
  }
  async function inventory(): Promise<Agent[]> {
    const agents: Agent[] = [],
      seen = new Set<string>();
    let cursor: string | undefined;
    for (let page = 0; page < 1000; page++) {
      const result = await client.agents.list({
        filter: { includeArchived: false },
        page: { limit: 200, ...(cursor ? { cursor } : {}) },
      });
      agents.push(...result.entries.map((entry) => entry.agent));
      if (!result.pageInfo.hasMore) return agents;
      const next = result.pageInfo.nextCursor;
      requireFact(next && !seen.has(next), "Incomplete child/background inventory");
      seen.add(next);
      cursor = next;
    }
    throw new Error("Child/background inventory exceeded its bound");
  }
  async function evidence(admission: ProceduralAdmission) {
    requireFact(
      admission.hostId === hostId && Date.parse(admission.expiresAt) > Date.now(),
      "Host mismatch or expired admission",
    );
    requireFact(
      admission.mode.orchestration === "paseo",
      "Governed transitions belong to kontord; no direct Paseo effect is allowed",
    );
    const config = readConfig();
    requireFact(
      ![config.stateRoot, ...config.placementRoots].some((placement) =>
        within(resolve(admission.cwd), resolve(placement)),
      ),
      "This session runs in a Kontor placement without its seat labels, so governed ownership cannot be ruled out. Nothing is taken over here.",
    );
    requireFact(
      !createSwitchProblem(createSwitchFile()),
      "Session creation is switched off or unreadable",
    );
    const handle = client.agents.ref(admission.sourceId),
      workspace = client.workspaces.ref(admission.workspaceId);
    for (const method of [
      handle.refresh,
      handle.archive,
      handle.run,
      handle.timeline?.refetch,
      workspace.refresh,
      workspace.agents?.create,
      client.providers.snapshot,
      ...(admission.children.length ? [handle.detach] : []),
    ])
      requireFact(
        typeof method === "function",
        "Required stock public API capability is unavailable",
      );
    for (const ref of [
      admission.handoff,
      ...admission.history,
      admission.authority,
      admission.plan,
    ])
      requireFact((await referenced(ref)).trim(), "Required handoff/history/authority is empty");
    const plan = JSON.parse(await referenced(admission.plan));
    requireFact(
      isDeepStrictEqual(plan.mode, admission.mode) &&
        plan.sourceId === admission.sourceId &&
        plan.operationId === admission.operationId,
      "Plan mode/operation/source mismatch",
    );
    const authority = JSON.parse(await referenced(admission.authority));
    requireFact(
      authority.operationId === admission.operationId &&
        authority.sourceId === admission.sourceId &&
        authority.coordinatorId === admission.coordinatorId &&
        isDeepStrictEqual(authority.config, admission.config) &&
        isDeepStrictEqual(authority.mode, admission.mode),
      "Prepared successor authority/configuration mismatch",
    );
    const handoff = JSON.parse(await referenced(admission.handoff));
    requireFact(
      handoff.operationId === admission.operationId &&
        handoff.sourceId === admission.sourceId &&
        handoff.authorizedNextAction &&
        handoff.scope &&
        handoff.criteria?.length &&
        handoff.completeAvailableContext === true,
      "Complete prepared handoff is missing",
    );
    const accounting = accountingSchema.parse(JSON.parse(await referenced(admission.accounting)));
    requireFact(
      accounting.operationId === admission.operationId &&
        accounting.sourceId === admission.sourceId &&
        isDeepStrictEqual(accounting.mode, admission.mode),
      "Accounting identifies another operation/mode",
    );
    requireFact(
      Date.parse(accounting.observedAt) <= Date.now() &&
        Date.parse(accounting.expiresAt) > Date.now() &&
        Date.now() - Date.parse(accounting.observedAt) <= 300_000 &&
        Date.parse(accounting.reporting.deadline) > Date.now(),
      "Owner accounting/reporting has expired",
    );
    requireFact(
      new Set(accounting.schedules.map((entry) => entry.id)).size === accounting.schedules.length &&
        new Set(accounting.workers.map((entry) => entry.id)).size === accounting.workers.length,
      "Duplicate accounting identities",
    );
    for (const schedule of accounting.schedules) await referenced(schedule.source);
    for (const worker of accounting.workers) await referenced(worker.checkpoint);
    if (admission.coordinatorId) {
      const owner = (await exact(admission.coordinatorId)).agent;
      requireFact(
        !owner.archivedAt &&
          owner.status !== "closed" &&
          !owner.providerUnavailable &&
          !owner.pendingPermissions.length &&
          !hasKontorLabels(owner.labels) &&
          (!owner.labels?.orchestration || owner.labels.orchestration === "paseo-direct"),
        "Existing direct coordinator is unavailable or governed",
      );
      requireFact(
        accounting.reporting.ownerId === admission.coordinatorId,
        "Reporting owner differs from existing coordinator",
      );
    } else
      requireFact(
        admission.mode.workflow === "free",
        "Kontor/Paseo requires its existing coordinator",
      );
    requireFact(
      isDeepStrictEqual(await takeoverCheckout(admission.cwd), admission.checkout),
      "Checkout branch/HEAD/WIP changed",
    );
  }
  async function preflight(admission: ProceduralAdmission, detached: boolean) {
    await evidence(admission);
    const result = await exact(admission.sourceId),
      source = result.agent;
    requireFact(
      takeoverAdapter(source).adapter === "direct",
      "Governed or conflicting ownership cannot use the direct route",
    );
    requireFact(
      source.status === "error" &&
        !source.archivedAt &&
        !source.activeTurn &&
        !source.pendingPermissions.length &&
        source.attentionReason !== "permission",
      "Source resumed, retired, or is waiting for interaction",
    );
    // Child detach does not change the predecessor's own pinned native revision.
    requireFact(
      lifecycleRevision(source) === admission.sourceRevision &&
        source.title === admission.title &&
        source.cwd === admission.cwd &&
        source.workspaceId === admission.workspaceId &&
        result.project?.projectKey === admission.projectKey &&
        parent(source) === admission.coordinatorId,
      "Pinned source identity/placement/authority changed",
    );
    const children = (await inventory())
      .filter((agent) => parent(agent) === source.id)
      .map((agent) => agent.id)
      .sort();
    requireFact(
      isDeepStrictEqual(children, detached ? [] : [...admission.children].sort()),
      "Child inventory changed; archive cascade refused",
    );
    const workspace = await client.workspaces.ref(admission.workspaceId).refresh();
    requireFact(
      workspace?.workspaceDirectory === admission.cwd,
      "Existing workspace differs from the checkout",
    );
    const snapshot = await client.providers.snapshot({ cwd: admission.cwd });
    const provider = snapshot.entries.find((entry) => entry.provider === admission.config.provider);
    requireFact(
      provider?.enabled === true &&
        provider.status === "ready" &&
        provider.models?.some(
          (model) =>
            model.id === admission.config.model &&
            model.thinkingOptions?.some((option) => option.id === admission.config.effort),
        ) &&
        provider.modes?.some((mode) => mode.id === admission.config.mode),
      "Stock provider/model/effort/mode is unavailable; account principal remains unverified",
    );
    return source;
  }
  async function retired(receipt: ProceduralReceipt, afterContinuation = false) {
    if (!afterContinuation) await evidence(receipt.admission);
    const source = (await exact(receipt.sourceId)).agent;
    requireFact(
      takeoverAdapter(source).adapter === "direct" &&
        source.archivedAt &&
        source.status === "closed" &&
        !source.activeTurn &&
        !source.pendingPermissions.length,
      "Exact predecessor retirement is unconfirmed",
    );
    requireFact(
      source.persistence?.sessionId === receipt.sourcePersistence &&
        source.cwd === receipt.admission.cwd &&
        source.workspaceId === receipt.admission.workspaceId,
      "Retired source identity changed",
    );
    requireFact(
      !(await inventory()).some((agent) => parent(agent) === receipt.sourceId),
      "A child remains attached to the predecessor",
    );
    for (const childId of receipt.admission.children) {
      const child = (await exact(childId)).agent;
      requireFact(!child.archivedAt && parent(child) === null, "Child preservation is unconfirmed");
    }
  }
  async function successor(receipt: ProceduralReceipt, runtime: boolean) {
    const admission = receipt.admission,
      result = await exact(receipt.successorId),
      agent = result.agent;
    requireFact(
      !hasKontorLabels(agent.labels) &&
        !agent.archivedAt &&
        !agent.activeTurn &&
        agent.status === "idle" &&
        !agent.pendingPermissions.length &&
        !agent.providerUnavailable,
      "Exact successor is not idle/available",
    );
    requireFact(
      agent.provider === admission.config.provider &&
        (agent.runtimeInfo?.model ?? agent.model) === admission.config.model &&
        agent.currentModeId === admission.config.mode &&
        agent.effectiveThinkingOptionId === admission.config.effort,
      "Successor effective configuration mismatch",
    );
    requireFact(
      agent.cwd === admission.cwd &&
        agent.workspaceId === admission.workspaceId &&
        result.project?.projectKey === admission.projectKey &&
        parent(agent) === admission.coordinatorId &&
        agent.title === admission.title &&
        agent.labels?.["takeover.operation"] === receipt.operationId &&
        agent.labels?.["takeover.predecessor"] === receipt.sourceId,
      "Successor checkout/title/authority/correlation mismatch",
    );
    for (const [key, value] of Object.entries(receipt.identityLabels))
      requireFact(
        agent.labels?.[key] === value,
        "Successor workflow/task/role identity labels changed",
      );
    requireFact(
      takeoverAdapter(agent).adapter === "direct",
      "Successor acquired a conflicting orchestration claim",
    );
    if (runtime)
      requireFact(
        agent.runtimeInfo?.model === admission.config.model && agent.persistence?.sessionId,
        "Effective native successor identity is unreported",
      );
    if (receipt.successorPersistence)
      requireFact(
        agent.persistence?.sessionId === receipt.successorPersistence,
        "Successor native session was replaced",
      );
    return agent;
  }
  const acknowledgement = (receipt: ProceduralReceipt, stage: "bootstrap" | "continuation") => ({
    operationId: receipt.operationId,
    successorId: receipt.successorId,
    stage,
    admissionHash: receipt.admissionHash,
    authoritySha256: receipt.admission.authority.sha256,
    checkout: receipt.admission.checkout,
    acknowledged: true as const,
  });
  async function run(receipt: ProceduralReceipt, stage: "bootstrap" | "continuation") {
    const ack = acknowledgement(receipt, stage),
      admission = receipt.admission;
    const prompt = `${stage === "bootstrap" ? "BOOTSTRAP ONLY. Read the complete handoff, history, authority and plan; inspect checkout. Do not perform task effects." : "CONTINUE THE EXISTING TASK. Read the complete prepared handoff and authority. Perform only its authorizedNextAction within its scope and criteria. Preserve findings, retry counters and reporting ownership. Do not revive the predecessor or create another owner/heartbeat. This acknowledgement reports continuation, not task completion. Its checkout value names the verified pre-continuation base; authorized task edits may change it afterwards."}\n${JSON.stringify({ admission, nativeHistory: receipt.nativeHistory })}\nIf and only if all evidence matches, reply ONLY this exact JSON: ${JSON.stringify(ack)}. Otherwise stop and report the mismatch.`;
    const result = await client.agents
      .ref(receipt.successorId)
      .run(prompt, { messageId: `procedural-${stage}-${receipt.operationId}`, timeoutMs: 60_000 });
    requireFact(
      result.status === "idle" && result.final?.id === receipt.successorId && !result.error,
      "Exact successor response is unavailable",
    );
    requireFact(
      isDeepStrictEqual(ackSchema.parse(JSON.parse(result.lastMessage ?? "null")), ack),
      "Correlated successor acknowledgement mismatch",
    );
    const current = await successor(receipt, true);
    return correlatedResponse(
      { ...receipt, successorPersistence: current.persistence!.sessionId },
      stage,
    );
  }
  async function correlatedResponse(
    receipt: ProceduralReceipt,
    stage: "bootstrap" | "continuation",
  ) {
    const page = await client.agents
      .ref(receipt.successorId)
      .timeline.refetch({ direction: "tail", limit: 200 });
    requireFact(
      !page.error && page.agent?.id === receipt.successorId,
      "Exact successor timeline is unreadable",
    );
    const prompt = page.entries.find(
      (entry) =>
        entry.item.type === "user_message" &&
        entry.item.clientMessageId === `procedural-${stage}-${receipt.operationId}`,
    );
    requireFact(
      prompt?.turnId && prompt.provider === receipt.admission.config.provider,
      "Correlated prompt/turn identity missing",
    );
    const expected = acknowledgement(receipt, stage);
    for (const entry of page.entries) {
      if (
        entry.provider !== prompt.provider ||
        entry.turnId !== prompt.turnId ||
        entry.seqStart <= prompt.seqEnd ||
        entry.item.type !== "assistant_message"
      )
        continue;
      try {
        if (isDeepStrictEqual(ackSchema.parse(JSON.parse(entry.item.text)), expected))
          return takeoverHash(
            JSON.stringify({
              provider: entry.provider,
              sessionId: receipt.successorPersistence,
              turnId: entry.turnId,
              messageId: `procedural-${stage}-${receipt.operationId}`,
              response: entry.item.text,
            }),
          );
      } catch {
        /* Another response is not continuation evidence. */
      }
    }
    throw new Error("Matching successor response is not visible; acceptance remains unknown");
  }
  async function prepare(ref: Ref) {
    const text = await referenced(ref),
      admission = proceduralAdmissionSchema.parse(JSON.parse(text)),
      admissionHash = takeoverHash(text);
    try {
      const previous = await read(admission.operationId);
      requireFact(
        previous.admissionHash === admissionHash && previous.sourceId === admission.sourceId,
        "Operation ID was reused with another admission",
      );
      return { receipt: previous, limits: PROCEDURAL_LIMITS };
    } catch (error) {
      if (!code(error, "ENOENT")) throw error;
    }
    try {
      await readFile(lifecycleClaim(admission.sourceId));
      throw new Error("Another Kontor lifecycle action owns this source");
    } catch (error) {
      if (!code(error, "ENOENT")) throw error;
    }
    const source = await preflight(admission, false);
    const history = await client.agents
      .ref(admission.sourceId)
      .timeline.refetch({ direction: "tail", limit: 200 });
    requireFact(
      !history.error && history.agent?.id === admission.sourceId,
      "Prepared public source history is unreadable",
    );
    const historyValue = {
      sourceId: admission.sourceId,
      scope: "bounded-public-tail",
      entries: history.entries,
    };
    requireFact(
      Buffer.byteLength(JSON.stringify(historyValue)) <= 8_000_000,
      "Public history capture exceeds its bound",
    );
    const historyPath = join(directory(admission.operationId), "source-history.json");
    try {
      await immutable(historyPath, historyValue);
    } catch (error) {
      if (!code(error, "EEXIST")) throw error;
      requireFact(
        (await readFile(historyPath, "utf8")) === JSON.stringify(historyValue),
        "Prepared public history changed; reconcile this operation",
      );
    }
    const nativeHistory = { path: historyPath, sha256: takeoverHash(JSON.stringify(historyValue)) };
    const record: ProceduralReceipt = {
      version: 1,
      operationId: admission.operationId,
      sourceId: admission.sourceId,
      successorId: randomUUID(),
      admissionHash,
      revision: 0,
      phase: "prepared",
      uncertainPhase: null,
      message:
        "Prepared procedural handoff; no detach, archive, create or task prompt dispatched; timeline reads may hydrate history",
      admission,
      identityLabels: copyLabels(source),
      nativeHistory,
      sourcePersistence: source.persistence?.sessionId ?? null,
      successorPersistence: null,
      evidence: [],
    };
    try {
      await save(record);
    } catch (error) {
      if (!code(error, "EEXIST")) throw error;
    }
    const persisted = await read(admission.operationId);
    requireFact(
      persisted.admissionHash === admissionHash && persisted.sourceId === admission.sourceId,
      "Operation ID was reused with another admission",
    );
    const sourceClaim = join(root, "sources", `${takeoverHash(admission.sourceId)}.json`);
    try {
      await immutable(sourceClaim, { operationId: persisted.operationId, admissionHash });
    } catch (error) {
      if (!code(error, "EEXIST")) throw error;
      requireFact(
        isDeepStrictEqual(JSON.parse(await readFile(sourceClaim, "utf8")), {
          operationId: persisted.operationId,
          admissionHash,
        }),
        "Another operation already owns this source; reconcile it",
      );
    }
    return { receipt: persisted, limits: PROCEDURAL_LIMITS };
  }
  /** Settle only the exact persisted claims after correlated success; a restart needs no in-memory tokens. */
  async function settlePreservedChildren(receipt: ProceduralReceipt): Promise<void> {
    if (!receipt.admission.children.length) return;
    const key = directory(receipt.operationId);
    const pending = (childSettlementSteps.get(key) ?? Promise.resolve()).then(async () => {
      const execution = executionClaimSchema.parse(
        JSON.parse(await readFile(join(key, "execution.claim"), "utf8")),
      );
      requireFact(
        execution.operationId === receipt.operationId &&
          execution.admissionHash === receipt.admissionHash,
        "Child claim owner is unconfirmed",
      );
      for (const childId of receipt.admission.children) {
        const expected = execution.lifecycleClaims[childId];
        requireFact(
          expected?.reviewId === receipt.operationId &&
            expected.agentId === childId &&
            expected.fingerprint === receipt.admissionHash,
          "Child claim identity is unconfirmed",
        );
        const path = lifecycleClaim(childId);
        let bytes: string;
        try {
          bytes = await readFile(path, "utf8");
        } catch (error) {
          if (code(error, "ENOENT")) continue;
          throw error;
        }
        const observed = lifecycleClaimSchema.parse(JSON.parse(bytes));
        if (!isDeepStrictEqual(observed, expected)) continue; // A valid foreign claim is never ours to release.
        await unlink(path);
        const parent = await open(dirname(path), "r");
        try {
          await parent.sync();
        } finally {
          await parent.close();
        }
      }
    });
    const settled = pending.catch(() => {});
    childSettlementSteps.set(key, settled);
    try {
      await pending;
    } finally {
      if (childSettlementSteps.get(key) === settled) childSettlementSteps.delete(key);
    }
  }
  async function execute(operationId: string, admissionHash: string) {
    let receipt = await read(operationId);
    requireFact(receipt.admissionHash === admissionHash, "Admission hash mismatch");
    requireFact(
      isDeepStrictEqual(
        JSON.parse(
          await readFile(join(root, "sources", `${takeoverHash(receipt.sourceId)}.json`), "utf8"),
        ),
        { operationId, admissionHash },
      ),
      "Source claim mismatch",
    );
    if (receipt.phase !== "prepared") return receipt;
    const claims = new Map(
      [...new Set([receipt.sourceId, ...receipt.admission.children])].sort().map((agentId) => [
        agentId,
        {
          reviewId: operationId,
          agentId,
          action: "takeover" as const,
          phase: "procedural" as const,
          fingerprint: admissionHash,
          token: randomUUID(),
        },
      ]),
    );
    try {
      await immutable(join(directory(operationId), "execution.claim"), {
        operationId,
        admissionHash,
        lifecycleClaims: Object.fromEntries(claims),
      });
    } catch (error) {
      if (code(error, "EEXIST")) return read(operationId);
      throw error;
    }
    const advance = async (phase: ProceduralReceipt["phase"], message: string, proof?: string) => {
      receipt = {
        ...receipt,
        revision: receipt.revision + 1,
        phase,
        message,
        evidence: proof ? [...receipt.evidence, proof] : receipt.evidence,
      };
      await save(receipt);
    };
    try {
      for (const [agentId, claim] of claims) await immutable(lifecycleClaim(agentId), claim);
      await preflight(receipt.admission, false);
      await referenced(receipt.nativeHistory);
      await advance("preserving", "Child preservation intent persisted");
      for (const childId of receipt.admission.children) {
        const child = (await exact(childId)).agent;
        requireFact(
          parent(child) === receipt.sourceId && takeoverAdapter(child).adapter === "direct",
          "Child ownership changed",
        );
        await client.agents.ref(childId).detach();
        requireFact(parent((await exact(childId)).agent) === null, "Detach is unconfirmed");
      }
      await preflight(receipt.admission, true);
      await advance("retiring", "Archive intent persisted; unknown outcome will not be resent");
      await client.agents.ref(receipt.sourceId).archive();
      await retired(receipt);
      await advance("retired", "Exact source archived, closed, no turn; children accounted for");
      requireFact(
        (await client.agents.ref(receipt.successorId).refresh()) === null,
        "Recorded successor ID already exists; read-only reconciliation required",
      );
      await advance("creating", "Creation intent persisted for the one reserved successor ID");
      const created = await client.workspaces.ref(receipt.admission.workspaceId).agents.create({
        agentId: receipt.successorId,
        idempotencyKey: operationId,
        title: receipt.admission.title,
        ...(receipt.admission.coordinatorId ? { parent: receipt.admission.coordinatorId } : {}),
        config: {
          provider: `${receipt.admission.config.provider}/${receipt.admission.config.model}`,
          modeId: receipt.admission.config.mode,
          thinkingOptionId: receipt.admission.config.effort,
        },
        labels: {
          ...receipt.identityLabels,
          "takeover.operation": operationId,
          "takeover.predecessor": receipt.sourceId,
        },
      });
      requireFact(created.id === receipt.successorId, "Creation returned another ID");
      await advance("created", "Reserved successor created without a task prompt");
      await retired(receipt);
      await successor(receipt, false);
      await advance("bootstrapping", "Read-only bootstrap intent persisted");
      const bootstrap = await run(receipt, "bootstrap");
      await retired(receipt);
      const verified = await successor(receipt, true);
      receipt = { ...receipt, successorPersistence: verified.persistence!.sessionId };
      await advance(
        "verified",
        "Correlated brief/authority/checkout bootstrap observed",
        bootstrap,
      );
      await advance("activating", "Continuation intent persisted");
      await retired(receipt);
      await successor(receipt, true);
      const continuation = await run(receipt, "continuation");
      await retired(receipt, true);
      await successor(receipt, true);
      await advance(
        "active",
        "Correlated task continuation observed; task completion is not implied",
        continuation,
      );
      // Keep the predecessor tombstone. Child settlement is shared with restart reconciliation.
      await settlePreservedChildren(receipt);
    } catch {
      // A failed save may have committed. Read disk first; never replace it with an assumed earlier phase.
      const persisted = await read(operationId);
      receipt = {
        ...persisted,
        revision: persisted.revision + 1,
        uncertainPhase: persisted.phase,
        phase: "held",
        message:
          "Effect/readback uncertain or precondition changed; inspect the exact operation and successor. No automatic resend or revival",
      };
      await save(receipt);
    }
    return receipt;
  }
  async function check(operationId: string) {
    const receipt = await read(operationId);
    // Reconciliation reads native state and may persist an observed verdict; it never resumes native effects.
    if (receipt.phase !== "held" && receipt.phase !== "active") return receipt;
    try {
      const activation =
        receipt.phase === "active" ||
        receipt.uncertainPhase === "activating" ||
        receipt.uncertainPhase === "active";
      await retired(receipt, activation);
      try {
        await successor(receipt, true);
      } catch {
        return {
          ...receipt,
          phase: "held" as const,
          uncertainPhase: receipt.uncertainPhase ?? receipt.phase,
          message:
            "Exact predecessor retirement is confirmed; the reserved successor is not verified. Existing owner must reconcile it without creating another successor",
        };
      }
      if (activation) {
        const proof = await correlatedResponse(receipt, "continuation");
        await settlePreservedChildren(receipt);
        if (receipt.phase === "active") return receipt;
        const reconciled: ProceduralReceipt = {
          ...receipt,
          revision: receipt.revision + 1,
          phase: "active",
          uncertainPhase: null,
          message: "Reconciled exact correlated continuation response; no native effect was resent",
          evidence: [...receipt.evidence, proof],
        };
        try {
          await save(reconciled);
          return reconciled;
        } catch (error) {
          if (code(error, "EEXIST")) return read(operationId);
          throw error;
        }
      }
      return {
        ...receipt,
        message:
          "Exact retired source and successor are readable; continuation remains unconfirmed. Do not replay",
      };
    } catch {
      return {
        ...receipt,
        phase: "held" as const,
        uncertainPhase: receipt.uncertainPhase ?? receipt.phase,
        message:
          "Exact native state or prepared evidence is unavailable/changed; operation remains held. Do not replay",
      };
    }
  }
  return { prepare, execute, check, read };
}
export function contributeProceduralTakeover(server: PluginServerContext) {
  const root = join(
    dirname(readConfig().paseoWorktreesRoot),
    "plugin-state",
    "kontor",
    "procedural-takeover-v1",
  );
  // The selected host identity is pinned in the local admission. There is no new native host capability.
  server.handle(prepareProceduralTakeover, async (input, { paseo }) => {
    const admission = proceduralAdmissionSchema.parse(
      JSON.parse(await referenced(input.admission)),
    );
    return createProceduralTakeover(paseo, root, admission.hostId).prepare(input.admission);
  });
  server.handle(executeProceduralTakeover, async (input, { paseo }) => {
    const receipt = await createProceduralTakeover(paseo, root, "read-only").read(
      input.operationId,
    );
    return createProceduralTakeover(paseo, root, receipt.admission.hostId).execute(
      input.operationId,
      input.admissionHash,
    );
  });
  server.handle(checkProceduralTakeover, async (input, { paseo }) => {
    const receipt = await createProceduralTakeover(paseo, root, "read-only").read(
      input.operationId,
    );
    return createProceduralTakeover(paseo, root, receipt.admission.hostId).check(input.operationId);
  });
}
