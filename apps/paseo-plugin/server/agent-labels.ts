import type { AgentLabels } from "./labels";

interface AgentSnapshotLike {
  readonly labels?: Record<string, string>;
}

interface AgentHandleLike {
  current(): AgentSnapshotLike | null;
  refresh(): Promise<{ agent: AgentSnapshotLike } | null>;
}

interface AgentApiLike {
  ref(agentId: string): AgentHandleLike;
}

interface PaseoLike {
  readonly agents: AgentApiLike;
}

export interface LabelReadOptions {
  readonly timeoutMs: number;
  readonly signal?: AbortSignal;
}

function bounded<T>(
  promise: Promise<T>,
  ms: number,
  signal?: AbortSignal,
): Promise<T | null> {
  return new Promise<T | null>((resolve) => {
    let settled = false;
    const finish = (value: T | null): void => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener("abort", onAbort);
      resolve(value);
    };
    const timer = setTimeout(() => finish(null), ms);
    const onAbort = (): void => finish(null);
    signal?.addEventListener("abort", onAbort, { once: true });
    promise.then(finish, () => finish(null));
  });
}

/**
 * Read one agent's labels from the local daemon, bounded by the hook budget.
 *
 * Every failure mode returns `null`: a timeout, an abort, an unknown agent
 * (the agent is not registered yet during creation) or a daemon error. The
 * caller treats `null` as "cannot resolve" and fails open.
 */
export async function readAgentLabels(
  paseo: PaseoLike,
  agentId: string,
  options: LabelReadOptions,
): Promise<AgentLabels> {
  if (options.signal?.aborted) {
    return null;
  }
  try {
    const handle = paseo.agents.ref(agentId);
    const current = handle.current();
    if (current?.labels && Object.keys(current.labels).length > 0) {
      return current.labels;
    }
    return await bounded(
      handle.refresh().then((result) => result?.agent.labels ?? null),
      options.timeoutMs,
      options.signal,
    );
  } catch {
    return null;
  }
}
