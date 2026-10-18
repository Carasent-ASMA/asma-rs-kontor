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

const AGENT_SORT: NonNullable<PaseoAgentListOptions["sort"]> = [
  { key: "updated_at", direction: "desc" },
];

export type DirectoryEntry = {
  readonly id: string;
  readonly title: string;
  readonly status: SessionStatus;
  readonly requiresAttention: boolean;
  readonly attentionReason: AttentionReason | null;
  readonly attentionTimestamp: string | null;
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
}): string {
  return [input.title, input.workspaceName, input.projectName, input.projectKey, input.cwd]
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
  return {
    id: agent.id,
    title,
    status: agent.status,
    requiresAttention: agent.requiresAttention === true,
    attentionReason: agent.attentionReason ?? null,
    attentionTimestamp: agent.attentionTimestamp ?? null,
    archivedAt: agent.archivedAt ?? null,
    createdAt: agent.createdAt,
    updatedAt: agent.updatedAt,
    cwd,
    workspaceId: agent.workspaceId ?? null,
    provider: agent.provider,
    model: agent.model ?? null,
    parentAgentId: (agent as { parentAgentId?: string | null }).parentAgentId ?? null,
    labels: agent.labels ?? {},
    projectKey,
    projectName,
    workspaceName,
    searchText: searchTextOf({ title, workspaceName, projectName, projectKey, cwd }),
  };
}

function timestampOf(value: string): number {
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? 0 : parsed;
}

function statusRank(status: SessionStatus): number {
  if (status === "error") {
    return 0;
  }
  if (status === "running") {
    return 1;
  }
  if (status === "initializing") {
    return 2;
  }
  if (status === "idle") {
    return 3;
  }
  return 4;
}

/** Display order only. The directory never changes a native session. */
export function compareDirectoryEntries(left: DirectoryEntry, right: DirectoryEntry): number {
  const attention = Number(right.requiresAttention) - Number(left.requiresAttention);
  if (attention !== 0) {
    return attention;
  }
  const status = statusRank(left.status) - statusRank(right.status);
  if (status !== 0) {
    return status;
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
export class SessionDirectory {
  private readonly options: SessionDirectoryOptions;
  private readonly entries = new Map<string, DirectoryEntry>();
  private status: DirectoryStatus = "idle";
  private live = false;
  private error: string | null = null;
  private staleReason: string | null = null;
  private pages = 0;
  private lastUpdatedAt: number | null = null;
  private includeArchived: boolean;
  private disposed = false;
  private generation = 0;
  private refreshing = false;
  private refreshQueued = false;
  private refreshPromise: Promise<void> | null = null;
  private subscription: OwnedSubscription<PaseoAgentListResult> | null = null;
  private unsubscribe: (() => void) | null = null;
  private subscriptionActive = false;
  private pendingUpdates: PaseoAgentUpdate[] = [];
  private pendingOverflow = false;
  private hostOnline = false;

  constructor(options: SessionDirectoryOptions) {
    this.options = options;
    this.includeArchived = options.includeArchived;
  }

  getState(): DirectoryState {
    return {
      entries: sortDirectoryEntries([...this.entries.values()]),
      status: this.status,
      live: this.live,
      error: this.error,
      staleReason: this.staleReason,
      pages: this.pages,
      lastUpdatedAt: this.lastUpdatedAt,
      includeArchived: this.includeArchived,
    };
  }

  async start(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.hostOnline = true;
    await this.connect();
  }

  async reconnect(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.hostOnline = true;
    await this.connect();
  }

  markHostOffline(reason = "The Paseo host is disconnected. Showing the last inventory."): void {
    this.hostOnline = false;
    this.generation += 1;
    this.live = false;
    this.subscriptionActive = false;
    this.releaseSubscription();
    if (this.entries.size > 0) {
      this.status = "stale";
      this.staleReason = reason;
      this.error = null;
    } else {
      this.status = "error";
      this.staleReason = null;
      this.error = reason;
    }
    this.emit();
  }

  async setIncludeArchived(includeArchived: boolean): Promise<void> {
    if (this.disposed || includeArchived === this.includeArchived) {
      return;
    }
    this.includeArchived = includeArchived;
    this.generation += 1;
    this.pendingUpdates = [];
    this.pendingOverflow = false;
    this.entries.clear();
    this.pages = 0;
    this.lastUpdatedAt = null;
    this.status = "loading";
    this.staleReason = null;
    this.error = null;
    this.live = false;
    this.subscriptionActive = false;
    this.releaseSubscription();
    this.emit();
    if (this.hostOnline) {
      await this.connect();
    }
  }

  async refresh(reason = "Refreshing the session directory."): Promise<void> {
    if (this.disposed) {
      return;
    }
    if (!this.hostOnline) {
      this.markHostOffline();
      return;
    }
    if (this.refreshing) {
      this.refreshQueued = true;
      return this.refreshPromise ?? Promise.resolve();
    }
    if (!this.subscriptionActive) {
      await this.openSubscription(this.generation);
    }
    this.refreshing = true;
    if (this.entries.size > 0) {
      this.status = "stale";
      this.staleReason = this.staleReason ?? reason;
      this.emit();
    }
    this.refreshPromise = (async () => {
      do {
        this.refreshQueued = false;
        await this.runRefresh();
      } while (this.refreshQueued && !this.disposed);
    })();
    try {
      await this.refreshPromise;
    } finally {
      this.refreshing = false;
      this.refreshPromise = null;
    }
    if (this.refreshQueued && !this.disposed) {
      this.refreshQueued = false;
      await this.refresh(reason);
    }
  }

  async dispose(): Promise<void> {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    this.generation += 1;
    this.live = false;
    this.subscriptionActive = false;
    await this.releaseSubscription();
  }

  private async connect(): Promise<void> {
    if (this.disposed || !this.hostOnline) {
      return;
    }
    const generation = this.generation;
    if (this.entries.size === 0) {
      this.status = "loading";
      this.staleReason = null;
      this.error = null;
      this.emit();
    } else {
      this.status = "stale";
      this.staleReason = "Reconnected to the Paseo host; reconciling the full directory.";
      this.emit();
    }
    await this.openSubscription(generation);
    await this.refresh("Reconciling the full session directory.");
  }

  private async openSubscription(generation: number): Promise<void> {
    if (this.disposed || generation !== this.generation) {
      return;
    }
    this.releaseSubscription();
    let result: PaseoAgentListResult & {
      subscriptionId: string;
      subscription: OwnedSubscription<PaseoAgentListResult>;
    };
    try {
      result = await this.options.client().agents.list({
        filter: filterOptions(this.includeArchived),
        sort: AGENT_SORT,
        page: { limit: DIRECTORY_PAGE_LIMIT },
        subscribe: {},
      });
    } catch (error) {
      if (generation !== this.generation || this.disposed) {
        return;
      }
      this.subscriptionActive = false;
      this.live = false;
      this.error = describeDirectoryError(error);
      this.status = this.entries.size > 0 ? "stale" : "error";
      this.staleReason = this.entries.size > 0 ? "Live updates are unavailable." : null;
      this.emit();
      return;
    }
    if (generation !== this.generation || this.disposed) {
      await result.subscription.release().catch(() => undefined);
      return;
    }

    this.subscription = result.subscription;
    this.subscriptionActive = true;
    let snapshots = 0;
    try {
      this.unsubscribe = result.subscription.subscribe({
        snapshot: () => {
          snapshots += 1;
          if (snapshots === 1) {
            this.live = this.status === "ready";
            this.emit();
            return;
          }
          this.status = "stale";
          this.staleReason = "Paseo reconnected; reconciling the full directory.";
          this.live = false;
          this.emit();
          void this.refresh("Reconciling the full session directory.");
        },
        update: (message) => {
          if (message.type === "agent_update") {
            this.receiveUpdate(message.payload);
          }
        },
        error: (error) => {
          this.subscriptionActive = false;
          this.live = false;
          if (this.disposed) {
            return;
          }
          this.error = describeDirectoryError(error);
          this.status = this.entries.size > 0 ? "stale" : "error";
          this.staleReason = this.entries.size > 0 ? "Live updates stopped." : null;
          this.emit();
        },
      });
    } catch (error) {
      this.subscriptionActive = false;
      this.live = false;
      await result.subscription.release().catch(() => undefined);
      this.error = describeDirectoryError(error);
      this.status = this.entries.size > 0 ? "stale" : "error";
      this.staleReason = this.entries.size > 0 ? "Live updates are unavailable." : null;
      this.emit();
    }
  }

  private releaseSubscription(): void {
    const unsubscribe = this.unsubscribe;
    this.unsubscribe = null;
    if (unsubscribe) {
      unsubscribe();
    }
    const subscription = this.subscription;
    this.subscription = null;
    if (subscription) {
      void subscription.release().catch(() => undefined);
    }
  }

  private async runRefresh(): Promise<void> {
    const generation = this.generation;
    let collected: CollectedDirectory;
    try {
      collected = await collectDirectory(this.options.client(), this.includeArchived);
    } catch (error) {
      if (generation !== this.generation || this.disposed) {
        return;
      }
      this.applyPendingUpdates();
      this.error = describeDirectoryError(error);
      this.staleReason =
        this.entries.size > 0 ? "Could not refresh the full directory." : null;
      this.status = this.entries.size > 0 ? "stale" : "error";
      this.live = false;
      this.emit();
      return;
    }
    if (generation !== this.generation || this.disposed) {
      return;
    }

    this.entries.clear();
    for (const entry of collected.entries) {
      this.entries.set(entry.id, entry);
    }
    this.applyPendingUpdates();
    this.pages = collected.pages;
    this.lastUpdatedAt = Date.now();
    this.error = null;
    this.staleReason = this.subscriptionActive
      ? null
      : "Live updates are unavailable; showing the last complete read.";
    this.status = "ready";
    this.live = this.subscriptionActive;
    if (this.pendingOverflow) {
      this.pendingOverflow = false;
      this.refreshQueued = true;
    }
    this.emit();
  }

  private receiveUpdate(update: PaseoAgentUpdate): void {
    if (this.disposed) {
      return;
    }
    if (this.refreshing) {
      if (this.pendingUpdates.length < PENDING_UPDATE_LIMIT) {
        this.pendingUpdates.push(update);
      } else {
        this.pendingOverflow = true;
      }
      return;
    }
    this.applyUpdate(update);
    this.emit();
  }

  private applyPendingUpdates(): void {
    const pending = this.pendingUpdates;
    this.pendingUpdates = [];
    for (const update of pending) {
      this.applyUpdate(update);
    }
  }

  private applyUpdate(update: PaseoAgentUpdate): void {
    if (update.kind === "remove") {
      this.entries.delete(update.agentId);
      return;
    }
    if (!this.includeArchived && (update.agent.archivedAt ?? null) !== null) {
      this.entries.delete(update.agent.id);
      return;
    }
    const previous = this.entries.get(update.agent.id);
    this.entries.set(
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

  private emit(): void {
    if (this.disposed) {
      return;
    }
    this.options.onChange(this.getState());
  }
}
