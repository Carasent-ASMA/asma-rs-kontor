import type {
  PaseoAgentListOptions,
  PaseoAgentListResult,
} from "@getpaseo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
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
  const rows = Array.from({ length: ROWS }, (_, index) => entry(`session-${index}`));
  const subscribeStarts: number[] = [];
  const pageStarts: number[] = [];
  let hangSubscriptions = !initialSubscriptionSucceeds;
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
          return subscriptionResult();
        }
        pageStarts.push(Date.now());
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
  };
}

async function settleAtTimeout(promise: Promise<void>) {
  let settled = false;
  void promise.finally(() => {
    settled = true;
  });
  for (let attempt = 0; attempt < 3; attempt += 1) {
    await vi.advanceTimersByTimeAsync(TIMEOUT_MS);
    await Promise.resolve();
    if (settled) {
      break;
    }
  }
  await promise;
}

function loadingDirectory(fake: ReturnType<typeof scriptedDirectory>) {
  let latest: DirectoryState | null = null;
  const directory = new SessionDirectory({
    client: () => fake.client,
    includeArchived: false,
    bootstrapLimit: 25,
    onChange: (state) => {
      latest = state;
    },
  });
  return { directory, latest: () => latest };
}

describe("session directory loading latency", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not serialize a second subscription timeout before a cold first page", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const fake = scriptedDirectory(false);
    const { directory, latest } = loadingDirectory(fake);

    const startedAt = Date.now();
    await settleAtTimeout(directory.start());

    expect(fake.pageStarts).toHaveLength(3);
    expect(fake.pageStarts[0]! - startedAt).toBe(TIMEOUT_MS);
    expect(fake.subscribeStarts.filter((at) => at <= fake.pageStarts[0]!)).toHaveLength(1);
    expect(latest()).toMatchObject({
      status: "ready",
      live: false,
      inventoryComplete: false,
      pages: 3,
    });
    expect(latest()!.entries).toHaveLength(ROWS);

    await directory.dispose();
  });

  it.each(["reconnect", "retry"] as const)(
    "does not serialize a second subscription timeout before the %s first page",
    async (operation) => {
      vi.useFakeTimers();
      vi.setSystemTime(0);
      const fake = scriptedDirectory(true);
      const { directory, latest } = loadingDirectory(fake);

      await directory.start();
      expect(latest()).toMatchObject({ status: "ready", live: true, pages: 3 });
      expect(latest()!.entries).toHaveLength(ROWS);

      if (operation === "reconnect") {
        directory.markHostOffline();
      } else {
        fake.failSubscription();
      }
      expect(latest()).toMatchObject({ status: "stale" });
      expect(latest()!.entries).toHaveLength(ROWS);

      fake.hangSubscriptions();
      fake.subscribeStarts.length = 0;
      fake.pageStarts.length = 0;
      const startedAt = Date.now();
      await settleAtTimeout(operation === "reconnect" ? directory.reconnect() : directory.retry());

      expect(fake.pageStarts).toHaveLength(3);
      expect(fake.pageStarts[0]! - startedAt).toBe(TIMEOUT_MS);
      expect(fake.subscribeStarts.filter((at) => at <= fake.pageStarts[0]!)).toHaveLength(1);
      expect(latest()).toMatchObject({
        status: "ready",
        live: false,
        inventoryComplete: false,
        pages: 3,
        staleReason: "Live updates are unavailable; the inventory may be incomplete.",
      });
      expect(latest()!.entries).toHaveLength(ROWS);

      await directory.dispose();
    },
  );
});
