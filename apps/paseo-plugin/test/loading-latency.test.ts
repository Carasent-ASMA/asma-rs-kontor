import type {
  PaseoAgentListOptions,
  PaseoAgentListResult,
} from "@getpaseo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { OwnedSubscriptions } from "../node_modules/@getpaseo/client/dist/connection/owned.js";
import {
  SessionDirectory,
  type AgentDirectoryPage,
  type DirectoryClient,
  type DirectoryState,
} from "../client/session-directory";

const TIMEOUT_MS = 30_000;
const ROWS = 409;
const PAGE_SIZE = 200;

function entry(id: string): AgentDirectoryPage {
  return {
    agent: {
      id,
      provider: "opencode/deepseek/deepseek-flash",
      cwd: "/tmp/work",
      workspaceId: "wks_1",
      model: null,
      createdAt: "2026-10-18T10:00:00.000Z",
      updatedAt: "2026-10-18T10:00:00.000Z",
      lastUserMessageAt: null,
      status: "idle",
      capabilities: {
        supportsStreaming: true,
        supportsSessionPersistence: true,
        supportsDynamicModes: true,
        supportsMcpServers: true,
        supportsReasoningStream: true,
        supportsToolInvocations: true,
      },
      currentModeId: null,
      availableModes: [],
      pendingPermissions: [],
      persistence: null,
      title: `Session ${id}`,
      labels: {},
      requiresAttention: false,
      attentionReason: null,
      attentionTimestamp: null,
      archivedAt: null,
    },
    project: {
      projectKey: "prj_1",
      projectName: "Project",
      workspaceName: "Workspace",
      checkout: { kind: "not_git", projectRootPath: "/tmp/work" },
    },
  } as unknown as AgentDirectoryPage;
}

function pageInfo(index: number) {
  const pages = Math.ceil(ROWS / PAGE_SIZE);
  return {
    hasMore: index + 1 < pages,
    nextCursor: index + 1 < pages ? `cursor-${index + 1}` : null,
    prevCursor: index > 0 ? `cursor-${index - 1}` : null,
  };
}

type SubscriptionSnapshot = PaseoAgentListResult & { subscriptionId: string };

type SubscriptionObserver = {
  snapshot: (snapshot: SubscriptionSnapshot) => void;
  update: (message: { type: "agent_update"; payload: unknown }) => void;
  error?: (error: unknown) => void;
};

function scriptedDirectory(initialSubscriptionSucceeds: boolean) {
  let rows = Array.from({ length: ROWS }, (_, index) => entry(`session-${index}`));
  const subscribeStarts: number[] = [];
  const pageStarts: number[] = [];
  let hangSubscriptions = !initialSubscriptionSucceeds;
  let holdNextSubscription = false;
  let holdNextPage = false;
  let failNextPage = false;
  let releaseSubscription: (() => void) | null = null;
  let releasePage: (() => void) | null = null;
  let observer: SubscriptionObserver | null = null;

  const subscriptionResult = () => {
    const first = rows.slice(0, 25);
    const snapshot: SubscriptionSnapshot = {
      requestId: "subscription",
      subscriptionId: "subscription-1",
      entries: first,
      pageInfo: pageInfo(0),
    };
    return {
      ...snapshot,
      subscription: {
        subscriptionId: "subscription-1",
        ready: Promise.resolve(snapshot),
        subscribe: (next: SubscriptionObserver) => {
          observer = next;
          next.snapshot(snapshot);
          return () => {
            observer = null;
          };
        },
        release: async () => undefined,
      },
    };
  };

  const client = {
    agents: {
      list: async (options: PaseoAgentListOptions = {}) => {
        if (options.subscribe) {
          subscribeStarts.push(Date.now());
          if (hangSubscriptions) {
            return new Promise<PaseoAgentListResult>((_resolve, reject) => {
              setTimeout(
                () => reject(new Error("subscription timed out")),
                options.timeout ?? TIMEOUT_MS,
              );
            });
          }
          if (holdNextSubscription) {
            holdNextSubscription = false;
            await new Promise<void>((resolve) => {
              releaseSubscription = resolve;
            });
          }
          return subscriptionResult();
        }
        pageStarts.push(Date.now());
        if (holdNextPage) {
          holdNextPage = false;
          await new Promise<void>((resolve) => {
            releasePage = resolve;
          });
        }
        if (failNextPage) {
          failNextPage = false;
          throw new Error("inventory failed");
        }
        const index = options.page?.cursor
          ? Number.parseInt(options.page.cursor.replace("cursor-", ""), 10)
          : 0;
        return {
          requestId: `page-${index}`,
          entries: rows.slice(index * PAGE_SIZE, (index + 1) * PAGE_SIZE),
          pageInfo: pageInfo(index),
        };
      },
    },
  } as unknown as DirectoryClient;

  return {
    client,
    subscribeStarts,
    pageStarts,
    hangSubscriptions() {
      hangSubscriptions = true;
    },
    failSubscription() {
      observer?.error?.(new Error("live updates stopped"));
    },
    holdNextSubscription() {
      holdNextSubscription = true;
      hangSubscriptions = false;
    },
    releaseHeldSubscription() {
      releaseSubscription?.();
      releaseSubscription = null;
    },
    holdNextPage() {
      holdNextPage = true;
    },
    releaseHeldPage() {
      releasePage?.();
      releasePage = null;
    },
    failNextPage() {
      failNextPage = true;
    },
    setRows(next: AgentDirectoryPage[]) {
      rows = next;
    },
  };
}

async function waitForState(
  read: () => DirectoryState | null,
  predicate: (state: DirectoryState) => boolean,
  timeoutMs = 2_000,
): Promise<DirectoryState> {
  return vi.waitFor(
    () => {
      const state = read();
      if (!state || !predicate(state)) {
        throw new Error("state did not settle");
      }
      return state;
    },
    { timeout: timeoutMs },
  );
}

function loadingDirectory(fake: ReturnType<typeof scriptedDirectory>) {
  let latest: DirectoryState | null = null;
  const states: DirectoryState[] = [];
  const directory = new SessionDirectory({
    client: () => fake.client,
    includeArchived: false,
    bootstrapLimit: 25,
    onChange: (state) => {
      latest = state;
      states.push(state);
    },
  });
  return { directory, latest: () => latest, states };
}

type Deferred<T = void> = {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
};

function deferred<T = void>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

async function sdkBufferedUpdate(beforeActivation: boolean): Promise<DirectoryState> {
  const subscriptionGate = deferred();
  const firstPage = deferred();
  const secondPage = deferred();
  const owned = new OwnedSubscriptions({
    release: async () => undefined,
    failed: () => undefined,
  });
  owned.restore();
  const oldRow = entry("fixture-a");
  const freshRow = entry("fixture-a");
  freshRow.agent = {
    ...freshRow.agent,
    title: "New permission warning",
    status: "running",
    updatedAt: "2026-10-25T12:00:02.000Z",
    pendingPermissions: [
      {
        id: "permission-fixture",
        provider: "opencode",
        name: "fixture",
        kind: "tool",
      },
    ],
    requiresAttention: true,
    attentionReason: "permission",
  };
  const snapshot: SubscriptionSnapshot = {
    requestId: "fixture-subscription",
    subscriptionId: "fixture-sub",
    entries: [oldRow],
    pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
  };
  const update = {
    type: "agent_update" as const,
    payload: {
      subscriptionId: "fixture-sub",
      kind: "upsert" as const,
      agent: freshRow.agent,
      project: freshRow.project,
    },
  };
  const page = (row: AgentDirectoryPage): PaseoAgentListResult => ({
    requestId: "fixture-page",
    entries: [row],
    pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
  });
  let pages = 0;
  let latest: DirectoryState | null = null;
  const client = {
    agents: {
      list: async (options: PaseoAgentListOptions = {}) => {
        if (options.subscribe) {
          const subscription = owned.observe(
            async (accept) => {
              await subscriptionGate.promise;
              accept(snapshot);
              if (beforeActivation) owned.receive(update);
              return snapshot;
            },
            options.signal,
          );
          return { ...(await subscription.ready), subscription };
        }
        pages += 1;
        if (pages === 1) {
          await firstPage.promise;
          return page(oldRow);
        }
        await secondPage.promise;
        throw new Error("fixture reconciliation failed");
      },
    },
  } as unknown as DirectoryClient;
  const directory = new SessionDirectory({
    client: () => client,
    includeArchived: false,
    bootstrapLimit: 25,
    onChange: (state) => {
      latest = state;
    },
  });
  try {
    const starting = directory.start();
    await vi.waitFor(() => expect(pages).toBe(1));
    subscriptionGate.resolve();
    await vi.waitFor(() => expect(pages).toBe(2));
    if (!beforeActivation) owned.receive(update);
    secondPage.resolve();
    await starting;
    if (!latest) throw new Error("directory emitted no state");
    return latest;
  } finally {
    subscriptionGate.resolve();
    firstPage.resolve();
    secondPage.resolve();
    await directory.dispose();
    await owned.close();
  }
}

describe("session directory loading latency", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts the inventory immediately when the subscription is slow, then keeps one partial read", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const fake = scriptedDirectory(false);
    const { directory, latest } = loadingDirectory(fake);

    const startedAt = Date.now();
    const starting = directory.start();
    await vi.advanceTimersByTimeAsync(0);
    await starting;

    expect(fake.pageStarts).toHaveLength(3);
    expect(fake.pageStarts[0]! - startedAt).toBe(0);
    expect(fake.subscribeStarts).toHaveLength(1);
    expect(latest()).toMatchObject({
      status: "ready",
      live: false,
      inventoryComplete: false,
      pages: 3,
    });
    expect(latest()!.entries).toHaveLength(ROWS);

    await vi.advanceTimersByTimeAsync(TIMEOUT_MS);
    expect(fake.subscribeStarts).toHaveLength(1);
    expect(fake.pageStarts).toHaveLength(3);
    expect(latest()).toMatchObject({
      status: "ready",
      live: false,
      inventoryComplete: false,
      staleReason: "Live updates are unavailable; the inventory may be incomplete.",
    });

    await directory.dispose();
  });

  it.each(["reconnect", "retry"] as const)(
    "starts the %s inventory immediately when the subscription is slow",
    async (operation) => {
      vi.useFakeTimers();
      vi.setSystemTime(0);
      const fake = scriptedDirectory(true);
      const { directory, latest } = loadingDirectory(fake);

      await directory.start();
      expect(latest()).toMatchObject({ status: "ready", live: true, pages: 3 });

      if (operation === "reconnect") {
        directory.markHostOffline();
      } else {
        fake.failSubscription();
      }
      fake.hangSubscriptions();
      fake.subscribeStarts.length = 0;
      fake.pageStarts.length = 0;

      const startedAt = Date.now();
      const operationPromise = operation === "reconnect" ? directory.reconnect() : directory.retry();
      await vi.advanceTimersByTimeAsync(0);
      await operationPromise;

      expect(fake.pageStarts).toHaveLength(3);
      expect(fake.pageStarts[0]! - startedAt).toBe(0);
      expect(fake.subscribeStarts).toHaveLength(1);
      expect(latest()).toMatchObject({
        status: "ready",
        live: false,
        inventoryComplete: false,
      });
      expect(latest()!.entries).toHaveLength(ROWS);

      await vi.advanceTimersByTimeAsync(TIMEOUT_MS);
      expect(fake.subscribeStarts).toHaveLength(1);
      expect(fake.pageStarts).toHaveLength(3);

      await directory.dispose();
    },
  );

  it("reconciles once when a held subscription activates after the provisional walk", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const fake = scriptedDirectory(false);
    fake.hangSubscriptions();
    const { directory, latest, states } = loadingDirectory(fake);

    fake.holdNextSubscription();
    await directory.start();
    expect(latest()).toMatchObject({ status: "ready", live: false, inventoryComplete: false });
    expect(fake.pageStarts).toHaveLength(3);

    fake.holdNextPage();
    fake.releaseHeldSubscription();
    await vi.waitFor(() => expect(fake.pageStarts).toHaveLength(4));
    expect(latest()).toMatchObject({
      status: "stale",
      live: false,
      inventoryComplete: false,
    });
    expect(states.every((state) => !(state.live && !state.inventoryComplete))).toBe(true);

    fake.releaseHeldPage();
    const reconciled = await waitForState(
      () => latest(),
      (state) => state.status === "ready" && state.live === true && state.inventoryComplete === true,
    );

    expect(reconciled.entries).toHaveLength(ROWS);
    expect(reconciled.pages).toBe(3);
    expect(fake.subscribeStarts).toHaveLength(1);
    expect(fake.pageStarts).toHaveLength(6);

    await directory.dispose();
  });

  it("keeps newer provisional rows when a late bootstrap arrives and reconciliation fails", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const fake = scriptedDirectory(false);
    fake.hangSubscriptions();
    fake.setRows(
      Array.from({ length: ROWS }, (_, index) => {
        const row = entry(`session-${index}`);
        if (index !== 0) return row;
        return {
          ...row,
          agent: {
            ...row.agent,
            title: "Newer",
            status: "running",
            updatedAt: "2026-10-18T11:00:00.000Z",
          },
        } as AgentDirectoryPage;
      }),
    );
    const { directory, latest } = loadingDirectory(fake);

    fake.holdNextSubscription();
    await directory.start();
    expect(latest()!.entries.find((item) => item.id === "session-0")?.title).toBe("Newer");

    fake.setRows(
      Array.from({ length: ROWS }, (_, index) => {
        const row = entry(`session-${index}`);
        if (index !== 0) return row;
        return {
          ...row,
          agent: {
            ...row.agent,
            title: "Older bootstrap",
            status: "idle",
            updatedAt: "2026-10-18T10:00:00.000Z",
          },
        } as AgentDirectoryPage;
      }),
    );
    fake.holdNextPage();
    fake.releaseHeldSubscription();
    await vi.waitFor(() => expect(fake.pageStarts).toHaveLength(4));
    expect(latest()!.entries.find((item) => item.id === "session-0")?.title).toBe("Newer");

    fake.failNextPage();
    fake.releaseHeldPage();
    const failed = await waitForState(
      () => latest(),
      (state) =>
        state.status === "stale" &&
        state.entries.find((item) => item.id === "session-0")?.title === "Newer",
    );
    expect(failed.entries.find((item) => item.id === "session-0")?.status).toBe("running");
    expect(failed.entries.find((item) => item.id === "session-0")?.updatedAt).toBe(
      "2026-10-18T11:00:00.000Z",
    );

    await directory.dispose();
  });

  it("preserves a pinned-SDK bootstrap buffered update when reconciliation fails", async () => {
    const latest = await sdkBufferedUpdate(true);
    expect(latest).toMatchObject({
      status: "stale",
      live: false,
      inventoryComplete: false,
    });
    expect(latest.entries[0]).toMatchObject({
      title: "New permission warning",
      status: "running",
      pendingPermissionCount: 1,
      needsInput: true,
    });
  });

  it("preserves the same pinned-SDK update delivered during reconciliation", async () => {
    const latest = await sdkBufferedUpdate(false);
    expect(latest).toMatchObject({
      status: "stale",
      live: false,
      inventoryComplete: false,
    });
    expect(latest.entries[0]).toMatchObject({
      title: "New permission warning",
      status: "running",
      pendingPermissionCount: 1,
      needsInput: true,
    });
  });

  it("supersedes a provisional walk when the subscription activates during it", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const fake = scriptedDirectory(false);
    fake.hangSubscriptions();
    const { directory, latest } = loadingDirectory(fake);

    fake.holdNextSubscription();
    fake.holdNextPage();
    const starting = directory.start();
    await waitForState(
      () => latest(),
      (state) => state.status === "loading" && state.entries.length === 0,
    );
    await vi.advanceTimersByTimeAsync(0);
    expect(fake.pageStarts).toHaveLength(1);

    fake.releaseHeldSubscription();
    fake.releaseHeldPage();
    await starting;
    const settled = await waitForState(
      () => latest(),
      (state) => state.status === "ready" && state.live === true && state.inventoryComplete === true,
    );

    expect(settled.entries).toHaveLength(ROWS);
    expect(settled.pages).toBe(3);
    expect(fake.subscribeStarts).toHaveLength(1);
    expect(fake.pageStarts).toHaveLength(4);

    await directory.dispose();
  });
});
