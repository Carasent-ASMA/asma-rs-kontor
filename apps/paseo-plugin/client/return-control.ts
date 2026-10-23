import type {
  OwnedSubscription,
  PaseoAgentListResult,
  PaseoAgentUpdate,
} from "@getpaseo/client";
import type {
  PluginButtonRegistration,
  PluginClientContext,
} from "@getpaseo/plugin/client";
import { collectDirectory, type DirectoryEntry } from "./session-directory";
import { recordVisit } from "./recent-sessions";

export const RETURN_CONTROL_ID = "return-to-kontor";
export const RETURN_CONTROL_TITLE = "Open Kontor";
export const RETURN_CONTROL_LABEL = "Kontor";
export const RETURN_CONTROL_ICON = "ArrowLeft";

type AgentLike = {
  readonly id: string;
  readonly workspaceId?: string | null;
  readonly archivedAt?: string | null;
  readonly status?: string | null;
};

type RegisteredPill = {
  readonly workspaceId: string;
  readonly registration: PluginButtonRegistration;
};

export type ReturnControlOptions = {
  /** Delay before retrying a failed observation bootstrap; default 500 ms. */
  readonly retryDelayMs?: number;
  /** Maximum exponential retry delay; default 30 s. */
  readonly maxRetryDelayMs?: number;
};

const DEFAULT_RETRY_DELAY_MS = 500;
const DEFAULT_MAX_RETRY_DELAY_MS = 30_000;
const PENDING_UPDATE_LIMIT = 256;

/**
 * Adds one understated return pill to each live agent's composer track. The
 * host owns the pill's styling, spacing, touch target and ordering; this module
 * only supplies the Lucide arrow and the short Kontor label.
 *
 * The agent directory is walked in full so ad-hoc and older sessions beyond the
 * first subscription page also get a pill. An owned subscription then follows
 * upserts, removals, closes, archives and workspace changes. All registrations
 * are removed on cleanup, and a late bootstrap walk after cleanup registers
 * nothing.
 */
export function contributeReturnControl(
  client: PluginClientContext,
  screenId: string,
  options: ReturnControlOptions = {},
): () => void {
  const pills = new Map<string, RegisteredPill>();
  const pendingUpdates: PaseoAgentUpdate[] = [];
  let pendingOverflow = false;
  const lifetime = new AbortController();
  const initialRetryDelay = Math.max(0, options.retryDelayMs ?? DEFAULT_RETRY_DELAY_MS);
  const maxRetryDelay = Math.max(
    initialRetryDelay,
    options.maxRetryDelayMs ?? DEFAULT_MAX_RETRY_DELAY_MS,
  );
  let retryDelay = initialRetryDelay;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;
  let observing = false;
  let walking = false;
  let walkQueued = false;
  let walkRetryTimer: ReturnType<typeof setTimeout> | null = null;
  let walkRetryDelay = initialRetryDelay;
  let unsubscribe: (() => void) | null = null;
  let subscription: OwnedSubscription<PaseoAgentListResult> | null = null;

  function removePill(agentId: string): void {
    const current = pills.get(agentId);
    if (!current) {
      return;
    }
    pills.delete(agentId);
    current.registration.remove();
  }

  function removeAll(): void {
    for (const agentId of [...pills.keys()]) {
      removePill(agentId);
    }
  }

  function registerAgent(agent: AgentLike): void {
    if (stopped) {
      return;
    }
    const workspaceId =
      typeof agent.workspaceId === "string" ? agent.workspaceId.trim() : "";
    if (
      workspaceId.length === 0 ||
      (agent.archivedAt ?? null) !== null ||
      agent.status === "closed"
    ) {
      removePill(agent.id);
      return;
    }
    const current = pills.get(agent.id);
    if (current?.workspaceId === workspaceId) {
      return;
    }
    if (current) {
      removePill(agent.id);
    }
    try {
      const registration = client.addComposerPill({
        id: RETURN_CONTROL_ID,
        workspaceId,
        agentId: agent.id,
        button: {
          title: RETURN_CONTROL_TITLE,
          icon: RETURN_CONTROL_ICON,
          label: RETURN_CONTROL_LABEL,
          behavior: {
            kind: "action",
            onPress() {
              // Leaving a session through Kontor's return path is a Recent visit on this device.
              recordVisit(agent.id);
              client.openScreen({ screenId });
            },
          },
        },
      });
      pills.set(agent.id, { workspaceId, registration });
    } catch (error) {
      console.error("Kontor return control could not register a composer pill", error);
    }
  }

  function applyUpdate(update: PaseoAgentUpdate): void {
    if (update.kind === "remove") {
      removePill(update.agentId);
      return;
    }
    registerAgent(update.agent);
  }

  function observeUpdate(update: PaseoAgentUpdate): void {
    if (stopped) {
      return;
    }
    if (walking) {
      if (pendingUpdates.length < PENDING_UPDATE_LIMIT) {
        pendingUpdates.push(update);
      } else {
        pendingOverflow = true;
      }
      return;
    }
    applyUpdate(update);
  }

  function reconcile(entries: readonly DirectoryEntry[]): void {
    if (stopped) {
      return;
    }
    const seen = new Set<string>();
    for (const entry of entries) {
      seen.add(entry.id);
      registerAgent(entry);
    }
    for (const agentId of [...pills.keys()]) {
      if (!seen.has(agentId)) {
        removePill(agentId);
      }
    }
  }

  function clearPendingUpdates(): void {
    pendingUpdates.length = 0;
    pendingOverflow = false;
  }

  function flushPendingUpdates(): void {
    const pending = pendingUpdates.splice(0);
    for (const update of pending) {
      applyUpdate(update);
    }
    if (pendingOverflow) {
      pendingOverflow = false;
      walkQueued = true;
    }
  }

  async function walkDirectory(): Promise<void> {
    if (stopped) {
      return;
    }
    if (walking) {
      walkQueued = true;
      return;
    }
    walking = true;
    try {
      do {
        walkQueued = false;
        const collected = await collectDirectory(client.paseo, false, lifetime.signal);
        if (stopped) {
          return;
        }
        reconcile(collected.entries);
        flushPendingUpdates();
      } while (walkQueued && !stopped);
      clearWalkRetryTimer();
      walkRetryDelay = initialRetryDelay;
    } catch (error) {
      if (!stopped) {
        flushPendingUpdates();
        console.error("Kontor return control could not walk the agent directory", error);
        scheduleWalkRetry();
      }
    } finally {
      walking = false;
    }
  }

  function clearRetryTimer(): void {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }

  function clearWalkRetryTimer(): void {
    if (walkRetryTimer !== null) {
      clearTimeout(walkRetryTimer);
      walkRetryTimer = null;
    }
  }

  function scheduleWalkRetry(): void {
    if (stopped || subscription === null || walkRetryTimer !== null) {
      return;
    }
    const delay = walkRetryDelay;
    walkRetryDelay = Math.min(Math.max(walkRetryDelay * 2, 1), maxRetryDelay);
    walkRetryTimer = setTimeout(() => {
      walkRetryTimer = null;
      void walkDirectory();
    }, delay);
  }

  function releaseObservation(): void {
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    if (subscription) {
      void subscription.release().catch(() => undefined);
      subscription = null;
    }
    // Buffered updates belong to the released connection. A fresh snapshot
    // must not replay them over a newer reconciled inventory.
    clearPendingUpdates();
  }

  function scheduleObservationRetry(): void {
    if (stopped || subscription || retryTimer !== null) {
      return;
    }
    const delay = retryDelay;
    retryDelay = Math.min(Math.max(retryDelay * 2, 1), maxRetryDelay);
    retryTimer = setTimeout(() => {
      retryTimer = null;
      void observeAgents();
    }, delay);
  }

  async function observeAgents(): Promise<void> {
    if (stopped || observing || subscription) {
      return;
    }
    observing = true;
    clearRetryTimer();
    try {
      const ready = await client.paseo.agents.list({
        subscribe: {},
        signal: lifetime.signal,
      });
      if (stopped) {
        await ready.subscription?.release().catch(() => undefined);
        return;
      }
      subscription = ready.subscription ?? null;
      // Register the ready page before attaching the observer so a buffered
      // remove/upsert delivered during subscribe wins over this bootstrap page.
      for (const entry of ready.entries) {
        registerAgent(entry.agent);
      }
      if (subscription) {
        let snapshots = 0;
        unsubscribe = subscription.subscribe({
          snapshot: () => {
            snapshots += 1;
            if (snapshots === 1) {
              return;
            }
            clearPendingUpdates();
            void walkDirectory();
          },
          update: (message) => {
            if (message.type === "agent_update") {
              observeUpdate(message.payload);
            }
          },
          error: () => {
            if (stopped) {
              return;
            }
            releaseObservation();
            scheduleObservationRetry();
          },
        });
        retryDelay = initialRetryDelay;
      }
      void walkDirectory();
    } catch (error) {
      if (stopped) {
        return;
      }
      console.error("Kontor return control could not observe agents", error);
      releaseObservation();
      void walkDirectory();
      scheduleObservationRetry();
    } finally {
      observing = false;
    }
  }

  void observeAgents();

  return () => {
    stopped = true;
    lifetime.abort();
    clearRetryTimer();
    clearWalkRetryTimer();
    releaseObservation();
    pendingUpdates.length = 0;
    pendingOverflow = false;
    removeAll();
  };
}
