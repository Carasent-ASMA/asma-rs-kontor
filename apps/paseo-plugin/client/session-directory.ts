import type {
  OwnedSubscription,
  PaseoAgentListOptions,
  PaseoAgentListResult,
  PaseoAgentUpdate,
  PaseoApi,
} from "@getpaseo/client";

export const SESSION_STATUSES = [
  "initializing",
  "idle",
  "running",
  "error",
  "closed",
] as const;

export type SessionStatus = (typeof SESSION_STATUSES)[number];

export const ATTENTION_REASONS = ["finished", "error", "permission"] as const;

export type AttentionReason = (typeof ATTENTION_REASONS)[number];

export const DIRECTORY_PAGE_LIMIT = 200;
export const DIRECTORY_MAX_PAGES = 1_000;
export const PENDING_UPDATE_LIMIT = 256;

/**
 * Structured labels that carry workflow identifiers and PR references. They are
 * search text only; none of them creates hierarchy or changes native placement.
 */
export const SEARCH_LABEL_KEYS = [
  "epic",
  "workstream",
  "pr",
  "ticket",
  "jira.epic",
  "jira.issue",
  "origin.epic",
  "desk",
  "role",
] as const;

const AGENT_SORT: NonNullable<PaseoAgentListOptions["sort"]> = [
  { key: "updated_at", direction: "desc" },
];

export type DirectoryEntry = {
  readonly id: string;
  readonly title: string;
  /** Raw native lifecycle. Never replaced by the derived needs-input state. */
  readonly status: SessionStatus;
  readonly requiresAttention: boolean;
  readonly attentionReason: AttentionReason | null;
  readonly attentionTimestamp: string | null;
  /** Native pending permission requests; public `pendingPermissions.length`. */
  readonly pendingPermissionCount: number;
  /** Native needs-input parity: permission wait wins over raw lifecycle. */
  readonly needsInput: boolean;
  readonly archivedAt: string | null;
  readonly createdAt: string;
  readonly updatedAt: string;
  readonly cwd: string;
  readonly workspaceId: string | null;
  readonly provider: string;
  readonly model: string | null;
  readonly parentAgentId: string | null;
  readonly labels: Readonly<Record<string, string>>;
  readonly projectKey: string | null;
  readonly projectName: string | null;
  readonly workspaceName: string | null;
  /** Lower-cased haystack for search over names, workspace context and cwd. */
  readonly searchText: string;
};

export type DirectoryStatus = "idle" | "loading" | "ready" | "stale" | "error";

export type DirectoryState = {
  readonly entries: readonly DirectoryEntry[];
  readonly status: DirectoryStatus;
  readonly live: boolean;
  readonly error: string | null;
  readonly staleReason: string | null;
  readonly pages: number;
  readonly lastUpdatedAt: number | null;
  readonly includeArchived: boolean;
};

export type DirectoryClient = Pick<PaseoApi, "agents">;

export type AgentDirectoryPage = PaseoAgentListResult["entries"][number];

export interface CollectedDirectory {
  readonly entries: DirectoryEntry[];
  readonly pages: number;
}

export interface SessionDirectoryOptions {
  /** Resolved per operation so a reconnect can borrow the new host connection. */
  readonly client: () => DirectoryClient;
  readonly includeArchived: boolean;
  readonly onChange: (state: DirectoryState) => void;
}

export function emptyDirectoryState(includeArchived = false): DirectoryState {
  return {
    entries: [],
    status: "idle",
    live: false,
    error: null,
    staleReason: null,
    pages: 0,
    lastUpdatedAt: null,
    includeArchived,
  };
}

export function describeDirectoryError(error: unknown): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }
  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }
  return "Paseo did not return a usable session directory.";
}

/**
 * Installed Paseo 0.11.4 `deriveAgentStateBucket` gives `needs_input` first when
 * a pending permission exists or the attention reason is permission, regardless
 * of `requiresAttention` or the raw lifecycle status.
 */
export function deriveNeedsInput(input: {
  readonly pendingPermissionCount?: number | null;
  readonly attentionReason?: AttentionReason | null;
}): boolean {
  return (
    (input.pendingPermissionCount ?? 0) > 0 ||
    input.attentionReason === "permission"
  );
}

function text(value: string | null | undefined): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function searchTextOf(input: {
  readonly title: string;
  readonly workspaceName: string | null;
  readonly projectName: string | null;
  readonly projectKey: string | null;
  readonly cwd: string;
  readonly labels: Readonly<Record<string, string>>;
}): string {
  const labelValues = SEARCH_LABEL_KEYS.map((key) => input.labels[key]).filter(
    (value): value is string => typeof value === "string" && value.trim().length > 0,
  );
  return [
    input.title,
    input.workspaceName,
    input.projectName,
    input.projectKey,
    input.cwd,
    ...labelValues,
  ]
    .filter((part): part is string => part !== null)
    .join("\n")
    .toLowerCase();
}

export function toDirectoryEntry(
  entry: { readonly agent: AgentDirectoryPage["agent"]; readonly project: AgentDirectoryPage["project"] | null },
  previous?: DirectoryEntry,
): DirectoryEntry {
  const { agent, project } = entry;
  const title = text(agent.title) ?? "Untitled session";
  const workspaceName = text(project?.workspaceName) ?? previous?.workspaceName ?? null;
  const projectName = text(project?.projectName) ?? previous?.projectName ?? null;
  const projectKey = text(project?.projectKey) ?? previous?.projectKey ?? null;
  const cwd = agent.cwd || previous?.cwd || "";
  const labels = agent.labels ?? {};
  const pendingPermissionCount = agent.pendingPermissions?.length ?? 0;
  const attentionReason = agent.attentionReason ?? null;
  return {
    id: agent.id,
    title,
    status: agent.status,
    requiresAttention: agent.requiresAttention === true,
    attentionReason,
    attentionTimestamp: agent.attentionTimestamp ?? null,
    pendingPermissionCount,
    needsInput: deriveNeedsInput({ pendingPermissionCount, attentionReason }),
    archivedAt: agent.archivedAt ?? null,
    createdAt: agent.createdAt,
    updatedAt: agent.updatedAt,
    cwd,
    workspaceId: agent.workspaceId ?? null,
    provider: agent.provider,
    model: agent.model ?? null,
    parentAgentId: (agent as { parentAgentId?: string | null }).parentAgentId ?? null,
    labels,
    projectKey,
    projectName,
    workspaceName,
    searchText: searchTextOf({ title, workspaceName, projectName, projectKey, cwd, labels }),
  };
}

function timestampOf(value: string): number {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}

/**
 * Same order as the installed native `deriveAgentStateBucket` priority:
 * needs input, failed, running, attention, done. The raw lifecycle remains
 * untouched; this only chooses display order.
 */
function stateBucketRank(entry: DirectoryEntry): number {
  if (entry.needsInput) {
    return 0;
  }
  if (entry.status === "error" || entry.attentionReason === "error") {
    return 1;
  }
  if (entry.status === "running") {
    return 2;
  }
  if (entry.requiresAttention) {
    return 3;
  }
  return 4;
}

/** Display order only. The directory never changes a native session. */
export function compareDirectoryEntries(left: DirectoryEntry, right: DirectoryEntry): number {
  const rank = stateBucketRank(left) - stateBucketRank(right);
  if (rank !== 0) {
    return rank;
  }
  const updated = timestampOf(right.updatedAt) - timestampOf(left.updatedAt);
  if (updated !== 0) {
    return updated;
  }
  return left.id.localeCompare(right.id);
}

export function sortDirectoryEntries(entries: readonly DirectoryEntry[]): DirectoryEntry[] {
  return [...entries].sort(compareDirectoryEntries);
}

function filterOptions(includeArchived: boolean): NonNullable<PaseoAgentListOptions["filter"]> {
  return includeArchived ? { includeArchived: true } : { includeArchived: false };
}

/**
 * Reads every page of the native agent directory. The loop never stops on a
 * visible page count and rejects a repeated or malformed cursor instead of
 * returning a silently partial inventory.
 */
export async function collectDirectory(
  client: DirectoryClient,
  includeArchived: boolean,
): Promise<CollectedDirectory> {
  const entries = new Map<string, DirectoryEntry>();
  const seenCursors = new Set<string>();
  let cursor: string | undefined;
  let pages = 0;

  for (;;) {
    const result = await client.agents.list({
      filter: filterOptions(includeArchived),
      sort: AGENT_SORT,
      page: {
        limit: DIRECTORY_PAGE_LIMIT,
        ...(cursor === undefined ? {} : { cursor }),
      },
    });
    pages += 1;
    for (const entry of result.entries) {
      entries.set(entry.agent.id, toDirectoryEntry(entry, entries.get(entry.agent.id)));
    }

    if (!result.pageInfo.hasMore) {
      return { entries: [...entries.values()], pages };
    }
    const nextCursor = result.pageInfo.nextCursor;
    if (nextCursor === null || nextCursor.trim().length === 0) {
      throw new Error("Paseo reported more sessions but returned no next cursor.");
    }
    if (seenCursors.has(nextCursor)) {
      throw new Error("Paseo returned a repeated page cursor; the inventory is incomplete.");
    }
    if (pages >= DIRECTORY_MAX_PAGES) {
      throw new Error(`Paseo still reports more sessions after ${DIRECTORY_MAX_PAGES} pages.`);
    }
    seenCursors.add(nextCursor);
    cursor = nextCursor;
  }
}

/**
 * Maintains one host's complete native session directory.
 *
 * Updates are streamed from the host. While a full page walk is in flight,
 * updates are buffered and replayed over the fresh pages so a slow inventory
 * read cannot lose a native state change. A reconnect delivers a second
 * subscription snapshot; that snapshot triggers a fresh full walk before the
 * directory is current again.
 */
export interface SessionDirectory {
  getState(): DirectoryState;
  start(): Promise<void>;
  reconnect(): Promise<void>;
  markHostOffline(reason?: string): void;
  setIncludeArchived(includeArchived: boolean): Promise<void>;
  refresh(reason?: string): Promise<void>;
  dispose(): Promise<void>;
}

export type SessionDirectoryConstructor = new (
  options: SessionDirectoryOptions,
) => SessionDirectory;

type PendingUpdate = {
  readonly generation: number;
  readonly update: PaseoAgentUpdate;
};

/**
 * Native Paseo evaluates plugin client bundles with Hermes, where ES6 class
 * syntax is disabled. This constructor is deliberately class-free: the host
 * compiles the source with its fixed es2020 target, so classes would otherwise
 * survive into a bundle that native rejects. The returned object preserves the
 * previous `new SessionDirectory(...)` API and behavior.
 */
export const SessionDirectory: SessionDirectoryConstructor = function SessionDirectory(
  options: SessionDirectoryOptions,
): SessionDirectory {
  const entries = new Map<string, DirectoryEntry>();
  let status: DirectoryStatus = "idle";
  let live = false;
  let error: string | null = null;
  let staleReason: string | null = null;
  let pages = 0;
  let lastUpdatedAt: number | null = null;
  let includeArchived = options.includeArchived;
  let disposed = false;
  let generation = 0;
  let readEpoch = 0;
  let refreshing = false;
  let refreshQueued = false;
  let refreshPromise: Promise<void> | null = null;
  let subscription: OwnedSubscription<PaseoAgentListResult> | null = null;
  let unsubscribe: (() => void) | null = null;
  let subscriptionActive = false;
  let pendingUpdates: PendingUpdate[] = [];
  let pendingOverflow = false;
  let hostOnline = false;

  function getState(): DirectoryState {
    return {
      entries: sortDirectoryEntries([...entries.values()]),
      status,
      live,
      error,
      staleReason,
      pages,
      lastUpdatedAt,
      includeArchived,
    };
  }

  function emit(): void {
    if (disposed) {
      return;
    }
    options.onChange(getState());
  }

  function releaseSubscription(): void {
    const currentUnsubscribe = unsubscribe;
    unsubscribe = null;
    if (currentUnsubscribe) {
      currentUnsubscribe();
    }
    const currentSubscription = subscription;
    subscription = null;
    if (currentSubscription) {
      void currentSubscription.release().catch(() => undefined);
    }
    // Buffered updates belong to the subscription that just ended. They must
    // never be replayed over a newer connection's reconciled snapshot.
    pendingUpdates = [];
    pendingOverflow = false;
  }

  function applyUpdate(update: PaseoAgentUpdate): void {
    if (update.kind === "remove") {
      entries.delete(update.agentId);
      return;
    }
    if (!includeArchived && (update.agent.archivedAt ?? null) !== null) {
      entries.delete(update.agent.id);
      return;
    }
    const previous = entries.get(update.agent.id);
    entries.set(
      update.agent.id,
      toDirectoryEntry(
        {
          agent: update.agent,
          project: update.project ?? null,
        },
        previous,
      ),
    );
  }

  function applyPendingUpdates(): void {
    const pending = pendingUpdates;
    pendingUpdates = [];
    for (const entry of pending) {
      if (entry.generation === generation) {
        applyUpdate(entry.update);
      }
    }
  }

  function receiveUpdate(update: PaseoAgentUpdate, expectedGeneration: number): void {
    if (disposed || expectedGeneration !== generation) {
      return;
    }
    if (refreshing) {
      if (pendingUpdates.length < PENDING_UPDATE_LIMIT) {
        pendingUpdates.push({ generation: expectedGeneration, update });
      } else {
        pendingOverflow = true;
      }
      return;
    }
    applyUpdate(update);
    emit();
  }

  async function runRefresh(): Promise<void> {
    const runGeneration = generation;
    const runEpoch = readEpoch;
    let collected: CollectedDirectory;
    try {
      collected = await collectDirectory(options.client(), includeArchived);
    } catch (refreshError) {
      if (runGeneration !== generation || runEpoch !== readEpoch || disposed) {
        return;
      }
      applyPendingUpdates();
      error = describeDirectoryError(refreshError);
      staleReason = entries.size > 0 ? "Could not refresh the full directory." : null;
      status = entries.size > 0 ? "stale" : "error";
      live = false;
      emit();
      return;
    }
    if (runGeneration !== generation || runEpoch !== readEpoch || disposed) {
      return;
    }

    entries.clear();
    for (const entry of collected.entries) {
      entries.set(entry.id, entry);
    }
    applyPendingUpdates();
    pages = collected.pages;
    lastUpdatedAt = Date.now();
    error = null;
    staleReason = subscriptionActive
      ? null
      : "Live updates are unavailable; showing the last complete read.";
    status = "ready";
    live = subscriptionActive;
    if (pendingOverflow) {
      pendingOverflow = false;
      refreshQueued = true;
    }
    emit();
  }

  async function openSubscription(expectedGeneration: number): Promise<void> {
    if (disposed || expectedGeneration !== generation) {
      return;
    }
    releaseSubscription();
    let result: PaseoAgentListResult & {
      subscriptionId: string;
      subscription: OwnedSubscription<PaseoAgentListResult>;
    };
    try {
      result = await options.client().agents.list({
        filter: filterOptions(includeArchived),
        sort: AGENT_SORT,
        page: { limit: DIRECTORY_PAGE_LIMIT },
        subscribe: {},
      });
    } catch (subscriptionError) {
      if (expectedGeneration !== generation || disposed) {
        return;
      }
      subscriptionActive = false;
      live = false;
      error = describeDirectoryError(subscriptionError);
      status = entries.size > 0 ? "stale" : "error";
      staleReason = entries.size > 0 ? "Live updates are unavailable." : null;
      emit();
      return;
    }
    if (expectedGeneration !== generation || disposed) {
      await result.subscription.release().catch(() => undefined);
      return;
    }

    subscription = result.subscription;
    subscriptionActive = true;
    let snapshots = 0;
    try {
      unsubscribe = result.subscription.subscribe({
        snapshot: () => {
          if (disposed || expectedGeneration !== generation) {
            return;
          }
          snapshots += 1;
          if (snapshots === 1) {
            live = status === "ready";
            emit();
            return;
          }
          // The transport reconnected. Anything buffered from before the new
          // snapshot belongs to the previous connection and must not be replayed
          // over the reconciled inventory. The read epoch also fences a page
          // walk that started before this snapshot: it must not publish old
          // inventory as ready/live while the fresh reconciliation is pending.
          readEpoch += 1;
          pendingUpdates = [];
          pendingOverflow = false;
          status = "stale";
          staleReason = "Paseo reconnected; reconciling the full directory.";
          live = false;
          emit();
          void refresh("Reconciling the full session directory.");
        },
        update: (message) => {
          if (message.type === "agent_update") {
            receiveUpdate(message.payload, expectedGeneration);
          }
        },
        error: (subscriptionError) => {
          if (disposed || expectedGeneration !== generation) {
            return;
          }
          subscriptionActive = false;
          live = false;
          error = describeDirectoryError(subscriptionError);
          status = entries.size > 0 ? "stale" : "error";
          staleReason = entries.size > 0 ? "Live updates stopped." : null;
          emit();
        },
      });
    } catch (subscriptionError) {
      subscriptionActive = false;
      live = false;
      await result.subscription.release().catch(() => undefined);
      error = describeDirectoryError(subscriptionError);
      status = entries.size > 0 ? "stale" : "error";
      staleReason = entries.size > 0 ? "Live updates are unavailable." : null;
      emit();
    }
  }

  async function connect(): Promise<void> {
    if (disposed || !hostOnline) {
      return;
    }
    // Every connection attempt gets a new generation. A queued update from the
    // previous subscription and an in-flight page walk from before the
    // reconnect are both fenced by this value.
    generation += 1;
    const connectionGeneration = generation;
    if (entries.size === 0) {
      status = "loading";
      staleReason = null;
      error = null;
      emit();
    } else {
      status = "stale";
      staleReason = "Reconnected to the Paseo host; reconciling the full directory.";
      emit();
    }
    await openSubscription(connectionGeneration);
    await refresh("Reconciling the full session directory.");
  }

  async function refresh(reason = "Refreshing the session directory."): Promise<void> {
    if (disposed) {
      return;
    }
    if (!hostOnline) {
      markHostOffline();
      return;
    }
    if (refreshing) {
      refreshQueued = true;
      return refreshPromise ?? Promise.resolve();
    }
    if (!subscriptionActive) {
      await openSubscription(generation);
    }
    refreshing = true;
    if (entries.size > 0) {
      status = "stale";
      staleReason = staleReason ?? reason;
      emit();
    }
    refreshPromise = (async () => {
      do {
        refreshQueued = false;
        await runRefresh();
      } while (refreshQueued && !disposed);
    })();
    try {
      await refreshPromise;
    } finally {
      refreshing = false;
      refreshPromise = null;
    }
    if (refreshQueued && !disposed) {
      refreshQueued = false;
      await refresh(reason);
    }
  }

  async function start(): Promise<void> {
    if (disposed) {
      return;
    }
    hostOnline = true;
    await connect();
  }

  async function reconnect(): Promise<void> {
    if (disposed) {
      return;
    }
    hostOnline = true;
    await connect();
  }

  function markHostOffline(
    reason = "The Paseo host is disconnected. Showing the last inventory.",
  ): void {
    hostOnline = false;
    generation += 1;
    live = false;
    subscriptionActive = false;
    releaseSubscription();
    if (entries.size > 0) {
      status = "stale";
      staleReason = reason;
      error = null;
    } else {
      status = "error";
      staleReason = null;
      error = reason;
    }
    emit();
  }

  async function setIncludeArchived(nextIncludeArchived: boolean): Promise<void> {
    if (disposed || nextIncludeArchived === includeArchived) {
      return;
    }
    includeArchived = nextIncludeArchived;
    generation += 1;
    pendingUpdates = [];
    pendingOverflow = false;
    entries.clear();
    pages = 0;
    lastUpdatedAt = null;
    status = "loading";
    staleReason = null;
    error = null;
    live = false;
    subscriptionActive = false;
    releaseSubscription();
    emit();
    if (hostOnline) {
      await connect();
    }
  }

  async function dispose(): Promise<void> {
    if (disposed) {
      return;
    }
    disposed = true;
    generation += 1;
    live = false;
    subscriptionActive = false;
    await releaseSubscription();
  }

  const directory: SessionDirectory = {
    getState,
    start,
    reconnect,
    markHostOffline,
    setIncludeArchived,
    refresh,
    dispose,
  };
  Object.setPrototypeOf(directory, SessionDirectory.prototype);
  return directory;
} as unknown as SessionDirectoryConstructor;
