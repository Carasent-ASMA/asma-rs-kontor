import type { OwnedSubscription, PaseoAgentListOptions, PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import { collectDirectory, type DirectoryEntry } from "./session-directory";
import {
  nativeReadAgentSchema, nativeReadResultSchema, sessionReadTargetSchema, SESSION_READ_MESSAGES,
  type SessionReadOutcome, type SessionReadResult, type SessionReadTarget,
} from "../shared/session-read";

type Client = Pick<PaseoApi, "agents">;
type AgentHandle = ReturnType<Client["agents"]["ref"]>;
type ReadHandle = AgentHandle & {
  markSeen(input: { expectedUpdatedAt: string; expectedAttentionTimestamp: string | null }): Promise<unknown>;
};
type ObservedDirectory = PaseoAgentListResult & { subscriptionId: string; subscription: OwnedSubscription<PaseoAgentListResult> };

function hostDirectoryClient(native: Client): Client {
  function list(options: PaseoAgentListOptions & { subscribe: {} }): Promise<ObservedDirectory>;
  function list(options?: PaseoAgentListOptions): Promise<PaseoAgentListResult>;
  function list(options?: PaseoAgentListOptions) {
    const scoped = { ...options, filter: { ...options?.filter, includeBackground: true, includeUnavailablePersisted: true } };
    if (options?.subscribe) return native.agents.list({ ...scoped, subscribe: {} });
    return native.agents.list(scoped);
  }
  return { agents: { ...native.agents, list } };
}

/** The native extension gates its RPC on the host capability; legacy clear-attention is never used. */
function supportsReadAcknowledgement(handle: AgentHandle): handle is ReadHandle {
  return "markSeen" in handle && typeof handle.markSeen === "function";
}

function hostSupportsMarkSeen(client: Client): boolean {
  return "supportsMarkSeen" in client.agents && client.agents.supportsMarkSeen === true;
}

export function sessionReadSupported(client: Client, exactId: string): boolean {
  return hostSupportsMarkSeen(client) && supportsReadAcknowledgement(client.agents.ref(exactId));
}

export function sessionReadTarget(hostId: string, entry: Pick<DirectoryEntry, "id" | "updatedAt" | "attentionTimestamp">): SessionReadTarget {
  return sessionReadTargetSchema.parse({ hostId, id: entry.id, updatedAt: entry.updatedAt, attentionTimestamp: entry.attentionTimestamp });
}

export type HostReadPreview = {
  readonly hostId: string;
  readonly targets: readonly SessionReadTarget[];
  readonly needsInputCount: number;
  readonly recordCount: number;
  readonly pages: number;
};

function result(target: SessionReadTarget, outcome: SessionReadOutcome): SessionReadResult {
  return { hostId: target.hostId, agentId: target.id, outcome, message: SESSION_READ_MESSAGES[outcome] };
}

async function reconcile(handle: AgentHandle, target: SessionReadTarget): Promise<SessionReadResult> {
  try {
    const readback = await handle.refresh();
    if (readback === null) return result(target, "missing");
    const agent = nativeReadAgentSchema.parse(readback.agent);
    if (agent.id !== target.id) return result(target, "unknown");
    if (agent.pendingPermissions.length || agent.attentionReason === "permission") return result(target, "needs_input");
    if (!agent.requiresAttention) return result(target, "already_seen");
    if (agent.updatedAt !== target.updatedAt || (agent.attentionTimestamp ?? null) !== target.attentionTimestamp) return result(target, "changed");
  } catch {
    // A failed metadata read proves neither absence nor acknowledgement.
  }
  return result(target, "unknown");
}

/** Borrow one exact host's public API. This service keeps no local read state or hidden-row store. */
export function createSessionReadActions(hostId: string, client: () => Client) {
  function checkHost(target: { readonly hostId: string }) {
    if (target.hostId !== hostId) throw new Error("Read acknowledgement belongs to a different host. Nothing was sent.");
  }

  async function markOne(input: SessionReadTarget): Promise<SessionReadResult> {
    const target = sessionReadTargetSchema.parse(input);
    checkHost(target);
    const native = client();
    const handle = native.agents.ref(target.id);
    if (!hostSupportsMarkSeen(native) || !supportsReadAcknowledgement(handle)) return result(target, "unsupported");
    try {
      const response = nativeReadResultSchema.parse(await handle.markSeen({
        expectedUpdatedAt: target.updatedAt, expectedAttentionTimestamp: target.attentionTimestamp,
      }));
      if (response.agentId !== target.id || (response.agent && response.agent.id !== target.id)) return result(target, "unknown");
      if (response.outcome === "error") return reconcile(handle, target);
      if (response.outcome === "missing") return result(target, response.agent === null ? "missing" : "unknown");
      if (!response.agent) return result(target, "unknown");
      if (response.outcome === "seen" || response.outcome === "already_seen") {
        return result(target, response.agent.requiresAttention ? "new_unread" : response.outcome);
      }
      return result(target, response.outcome);
    } catch {
      // Never resend after a timeout/lost acknowledgement; read native metadata once.
      return reconcile(handle, target);
    }
  }

  async function prepareHost(signal?: AbortSignal): Promise<HostReadPreview> {
    const native = client();
    if (!hostSupportsMarkSeen(native)) throw new Error(SESSION_READ_MESSAGES.unsupported);
    const inventory = await collectDirectory(hostDirectoryClient(native), true, signal);
    const unread = inventory.entries.filter((entry) => entry.requiresAttention);
    return {
      hostId,
      targets: unread.filter((entry) => !entry.needsInput).map((entry) => sessionReadTarget(hostId, entry)),
      needsInputCount: unread.filter((entry) => entry.needsInput).length,
      recordCount: inventory.entries.length,
      pages: inventory.pages,
    };
  }

  async function* markHost(preview: HostReadPreview, signal?: AbortSignal): AsyncGenerator<SessionReadResult> {
    checkHost(preview);
    // Validate the entire frozen scope before the first write.
    const targets = preview.targets.map((input) => {
      const target = sessionReadTargetSchema.parse(input);
      checkHost(target);
      return target;
    });
    const seenIds = new Set<string>();
    for (const target of targets) {
      if (seenIds.has(target.id)) continue;
      seenIds.add(target.id);
      if (signal?.aborted) { yield result(target, "not_sent"); continue; }
      yield await markOne(target);
    }
  }

  return { prepareHost, markOne, markHost };
}
