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
): () => void {
  const pills = new Map<string, RegisteredPill>();
  const pendingUpdates: PaseoAgentUpdate[] = [];
  const lifetime = new AbortController();
  let stopped = false;
  let walking = false;
  let walkQueued = false;
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
      pendingUpdates.push(update);
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

  function flushPendingUpdates(): void {
    const pending = pendingUpdates.splice(0);
    for (const update of pending) {
      applyUpdate(update);
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
        const collected = await collectDirectory(client.paseo, false);
        if (stopped) {
          return;
        }
        reconcile(collected.entries);
        flushPendingUpdates();
      } while (walkQueued && !stopped);
    } catch (error) {
      if (!stopped) {
        flushPendingUpdates();
        console.error("Kontor return control could not walk the agent directory", error);
      }
    } finally {
      walking = false;
    }
  }

  void client.paseo.agents
    .list({ subscribe: {}, signal: lifetime.signal })
    .then((ready) => {
      if (stopped) {
        void ready.subscription?.release().catch(() => undefined);
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
            void walkDirectory();
          },
          update: (message) => {
            if (message.type === "agent_update") {
              observeUpdate(message.payload);
            }
          },
        });
      }
      void walkDirectory();
    })
    .catch((error) => {
      if (!stopped) {
        console.error("Kontor return control could not observe agents", error);
        void walkDirectory();
      }
    });

  return () => {
    stopped = true;
    lifetime.abort();
    if (unsubscribe) {
      unsubscribe();
      unsubscribe = null;
    }
    if (subscription) {
      void subscription.release().catch(() => undefined);
      subscription = null;
    }
    pendingUpdates.length = 0;
    removeAll();
  };
}
