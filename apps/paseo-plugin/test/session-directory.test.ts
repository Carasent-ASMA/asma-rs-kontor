import type {
  PaseoAgentUpdate,
  PaseoAgentListOptions,
  PaseoAgentListResult,
} from "@getpaseo/client";
import { describe, expect, it, vi } from "vitest";
import {
  collectDirectory,
  emptyDirectoryState,
  SessionDirectory,
  toDirectoryEntry,
  type AgentDirectoryPage,
  type DirectoryClient,
  type DirectoryEntry,
  type DirectoryState,
} from "../client/session-directory";
import { matchesQuery } from "../client/session-view";

type Observer = {
  snapshot: (snapshot: PaseoAgentListResult & { subscriptionId: string }) => void;
  update: (message: { type: "agent_update"; payload: PaseoAgentUpdate }) => void;
  error?: (error: unknown) => void;
};

type EntryOverrides = {
  title?: string;
  status?: "initializing" | "idle" | "running" | "error" | "closed";
  requiresAttention?: boolean;
  attentionReason?: "finished" | "error" | "permission" | null;
  archivedAt?: string | null;
  workspaceName?: string;
  projectName?: string;
  updatedAt?: string;
  labels?: Record<string, string>;
};

function entry(id: string, overrides: EntryOverrides = {}): AgentDirectoryPage {
  return {
    agent: {
      id,
      provider: "opencode/deepseek/deepseek-flash",
      cwd: "/tmp/work",
      workspaceId: "wks_1",
      model: null,
      createdAt: "2026-10-18T10:00:00.000Z",
      updatedAt: overrides.updatedAt ?? "2026-10-18T10:00:00.000Z",
      lastUserMessageAt: null,
      status: overrides.status ?? "idle",
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
      title: overrides.title ?? `Session ${id}`,
      labels: overrides.labels ?? {},
      requiresAttention: overrides.requiresAttention ?? false,
      attentionReason: overrides.attentionReason ?? null,
      attentionTimestamp: null,
      archivedAt: overrides.archivedAt ?? null,
    },
    project: {
      projectKey: "prj_1",
      projectName: overrides.projectName ?? "Project",
      workspaceName: overrides.workspaceName ?? "Workspace",
      checkout: { kind: "not_git", projectRootPath: "/tmp/work" },
    },
  } as unknown as AgentDirectoryPage;
}

function cursorIndex(cursor: string | undefined): number {
  if (!cursor) {
    return 0;
  }
  const parsed = Number.parseInt(cursor.replace("cursor-", ""), 10);
  return Number.isFinite(parsed) ? parsed : 0;
}

function pageInfo(index: number, count: number) {
  return {
    hasMore: index + 1 < count,
    nextCursor: index + 1 < count ? `cursor-${index + 1}` : null,
    prevCursor: index > 0 ? `cursor-${index - 1}` : null,
  };
}

function fakeDirectory(initialPages: AgentDirectoryPage[][]) {
  let pages = initialPages;
  let observer: Observer | null = null;
  const observers: Observer[] = [];
  let heldPage: {
    readonly started: Promise<void>;
    readonly markStarted: () => void;
    readonly gate: Promise<void>;
    readonly release: () => void;
  } | null = null;
  let released = false;
  let failure: "none" | "next" | "always" = "none";
  let subscribeFailure = false;
  let pageInfoOverride: ((index: number, count: number) => ReturnType<typeof pageInfo>) | null = null;
  const listCalls: PaseoAgentListOptions[] = [];

  const visiblePages = (options: PaseoAgentListOptions): AgentDirectoryPage[][] => {
    const includeArchived = options.filter?.includeArchived === true;
    return pages.map((page) =>
      page.filter((item) => includeArchived || (item.agent.archivedAt ?? null) === null),
    );
  };

  const list = async (options: PaseoAgentListOptions = {}) => {
    listCalls.push(options);
    if (failure === "always" || failure === "next") {
      if (failure === "next") {
        failure = "none";
      }
      throw new Error("daemon unavailable");
    }
    const visible = visiblePages(options);
    if (options.subscribe) {
      if (subscribeFailure) {
        throw new Error("subscription unavailable");
      }
      const first = visible[0] ?? [];
      const snapshot: PaseoAgentListResult & { subscriptionId: string } = {
        requestId: "request-subscribe",
        subscriptionId: "sub-1",
        entries: first,
        pageInfo: pageInfo(0, visible.length),
      };
      const subscription = {
        subscriptionId: "sub-1",
        ready: Promise.resolve(snapshot),
        subscribe: (next: Observer) => {
          observer = next;
          observers.push(next);
          next.snapshot(snapshot);
          return () => {
            if (observer === next) {
              observer = null;
            }
          };
        },
        release: async () => {
          released = true;
        },
      };
      return { ...snapshot, subscription };
    }
    if (heldPage) {
      const held = heldPage;
      heldPage = null;
      held.markStarted();
      await held.gate;
    }
    const index = cursorIndex(options.page?.cursor);
    return {
      requestId: "request-page",
      entries: visible[index] ?? [],
      pageInfo: pageInfoOverride
        ? pageInfoOverride(index, visible.length)
        : pageInfo(index, visible.length),
    };
  };

  const client = {
    agents: { list },
  } as unknown as DirectoryClient;

  return {
    client,
    listCalls,
    setPages(next: AgentDirectoryPage[][]) {
      pages = next;
    },
    failNext() {
      failure = "next";
    },
    failAlways() {
      failure = "always";
    },
    failSubscription(value = true) {
      subscribeFailure = value;
    },
    holdNextPage() {
      let markStarted!: () => void;
      let releaseGate!: () => void;
      const started = new Promise<void>((resolve) => {
        markStarted = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        releaseGate = resolve;
      });
      heldPage = { started, markStarted, gate, release: releaseGate };
      return {
        started,
        release: () => {
          heldPage = null;
          releaseGate();
        },
      };
    },
    setPageInfo(
      override: (index: number, count: number) => ReturnType<typeof pageInfo>,
    ) {
      pageInfoOverride = override;
    },
    emitUpdate(update: PaseoAgentUpdate) {
      observer?.update({ type: "agent_update", payload: update });
    },
    emitUpdateOn(index: number, update: PaseoAgentUpdate) {
      observers[index]?.update({ type: "agent_update", payload: update });
    },
    emitReconnectSnapshot() {
      const first = visiblePages({ filter: { includeArchived: false } })[0] ?? [];
      observer?.snapshot({
        requestId: "request-reconnect",
        subscriptionId: "sub-1",
        entries: first,
        pageInfo: pageInfo(0, 1),
      });
    },
    emitError(error: unknown) {
      observer?.error?.(error);
    },
    get released() {
      return released;
    },
  };
}

function waitForState(
  read: () => DirectoryState,
  predicate: (state: DirectoryState) => boolean,
  timeoutMs = 2_000,
): Promise<DirectoryState> {
  return vi.waitFor(
    () => {
      const state = read();
      if (!predicate(state)) {
        throw new Error("state did not settle");
      }
      return state;
    },
    { timeout: timeoutMs },
  );
}

describe("collectDirectory", () => {
  it("reads every page instead of stopping at the visible page", async () => {
    const pages = [
      Array.from({ length: 200 }, (_, index) => entry(`page-1-${index}`)),
      Array.from({ length: 200 }, (_, index) => entry(`page-2-${index}`)),
      Array.from({ length: 105 }, (_, index) => entry(`page-3-${index}`)),
    ];
    const fake = fakeDirectory(pages);

    const collected = await collectDirectory(fake.client, false);

    expect(collected.pages).toBe(3);
    expect(collected.entries).toHaveLength(505);
    expect(new Set(collected.entries.map((item) => item.id)).size).toBe(505);
    expect(fake.listCalls.map((call) => call.page?.cursor)).toEqual([
      undefined,
      "cursor-1",
      "cursor-2",
    ]);
    expect(fake.listCalls.every((call) => call.page?.limit === 200)).toBe(true);
    expect(fake.listCalls.every((call) => call.filter?.includeArchived === false)).toBe(true);
  });

  it("rejects a repeated cursor instead of returning a partial inventory", async () => {
    const fake = fakeDirectory([Array.from({ length: 200 }, (_, index) => entry(`a-${index}`))]);
    fake.setPageInfo(() => ({ hasMore: true, nextCursor: "cursor-1", prevCursor: null }));

    await expect(collectDirectory(fake.client, false)).rejects.toThrow(/repeated page cursor/);
  });

  it("rejects a has-more page without a cursor", async () => {
    const fake = fakeDirectory([Array.from({ length: 200 }, (_, index) => entry(`a-${index}`))]);
    fake.setPageInfo(() => ({ hasMore: true, nextCursor: null, prevCursor: null }));

    await expect(collectDirectory(fake.client, false)).rejects.toThrow(/no next cursor/);
  });
});

describe("SessionDirectory", () => {
  it("searches recognized epic and workstream label values", () => {
    const directoryEntry = toDirectoryEntry(
      entry("labeled", {
        title: "Planner session",
        labels: {
          epic: "ASMA-8539",
          workstream: "kontor",
          "paseo.parent-agent-id": "internal-only",
        },
      }),
    );

    expect(directoryEntry.searchText).toContain("asma-8539");
    expect(directoryEntry.searchText).toContain("kontor");
    expect(directoryEntry.searchText).not.toContain("internal-only");
    expect(matchesQuery(directoryEntry, "ASMA-8539")).toBe(true);
    expect(matchesQuery(directoryEntry, "kontor")).toBe(true);
  });

  it("tracks native status and attention separately and keeps duplicate titles distinct", async () => {
    const fake = fakeDirectory([
      [
        entry("agent-a", { title: "Same title", status: "running" }),
        entry("agent-b", { title: "Same title", status: "idle" }),
      ],
    ]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();

    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(true);
    expect(latest.entries).toHaveLength(2);
    expect(new Set(latest.entries.map((item) => item.id))).toEqual(
      new Set(["agent-a", "agent-b"]),
    );

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a", { title: "Same title", status: "running" }).agent,
        status: "idle",
        requiresAttention: true,
        attentionReason: "permission",
      },
    } as PaseoAgentUpdate);

    const updated = latest.entries.find((item) => item.id === "agent-a") as DirectoryEntry;
    expect(updated.status).toBe("idle");
    expect(updated.requiresAttention).toBe(true);
    expect(updated.attentionReason).toBe("permission");

    fake.emitUpdate({ kind: "remove", agentId: "agent-b" } as PaseoAgentUpdate);
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);

    await directory.dispose();
    expect(fake.released).toBe(true);
  });

  it("marks the directory stale on disconnect and reconciles on reconnect", async () => {
    const fake = fakeDirectory([[entry("agent-a"), entry("agent-b")]]);
    const states: DirectoryState[] = [];
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
        states.push(state);
      },
    });

    await directory.start();
    expect(latest.entries).toHaveLength(2);

    directory.markHostOffline();
    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.entries).toHaveLength(2);

    fake.setPages([[entry("agent-a")]]);
    await directory.reconnect();

    expect(latest.status).toBe("ready");
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);
    expect(latest.live).toBe(true);

    await directory.dispose();
  });

  it("reconciles the full inventory when the existing subscription reconnects", async () => {
    const fake = fakeDirectory([[entry("agent-a"), entry("agent-b")]]);
    const states: DirectoryState[] = [];
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
        states.push(state);
      },
    });

    await directory.start();
    expect(latest.entries).toHaveLength(2);

    fake.setPages([[entry("agent-a")]]);
    fake.emitReconnectSnapshot();

    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries.length === 1,
    );
    expect(settled.entries.map((item) => item.id)).toEqual(["agent-a"]);
    expect(settled.live).toBe(true);
    expect(
      states.some((state) => state.status === "stale" && state.live === false),
    ).toBe(true);
    expect(
      fake.listCalls.filter((call) => call.subscribe === undefined).length,
    ).toBeGreaterThanOrEqual(2);

    await directory.dispose();
  });

  it("keeps the previous rows but marks them stale when a refresh fails", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    fake.failNext();
    await directory.refresh();

    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.error).toMatch(/daemon unavailable/);
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);

    await directory.dispose();
  });

  it("reports an error instead of an empty live list when the first read fails", async () => {
    const fake = fakeDirectory([]);
    fake.failAlways();
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();

    expect(latest.status).toBe("error");
    expect(latest.live).toBe(false);
    expect(latest.error).toMatch(/daemon unavailable/);

    await directory.dispose();
  });

  it("fences a queued pre-reconnect update so the reconciled snapshot wins", async () => {
    const oldRunning = entry("agent-a", {
      status: "running",
      updatedAt: "2026-10-18T10:00:00.000Z",
    });
    const fake = fakeDirectory([[oldRunning]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const held = fake.holdNextPage();
    const oldRefresh = directory.refresh();
    await held.started;

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...oldRunning.agent,
        status: "error",
        updatedAt: "2026-10-18T10:01:00.000Z",
      },
    } as PaseoAgentUpdate);

    directory.markHostOffline();
    fake.setPages([
      [
        entry("agent-a", {
          status: "idle",
          updatedAt: "2026-10-18T10:02:00.000Z",
        }),
      ],
    ]);
    const reconnect = directory.reconnect();
    held.release();
    await Promise.all([oldRefresh, reconnect]);

    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(true);
    expect(latest.entries).toHaveLength(1);
    expect(latest.entries[0]?.status).toBe("idle");
    expect(latest.entries[0]?.updatedAt).toBe("2026-10-18T10:02:00.000Z");

    await directory.dispose();
  });

  it("drops buffered updates when the existing subscription reconnects", async () => {
    const fake = fakeDirectory([
      [
        entry("agent-a", {
          status: "running",
          updatedAt: "2026-10-18T10:00:00.000Z",
        }),
      ],
    ]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const held = fake.holdNextPage();
    const oldRefresh = directory.refresh();
    await held.started;

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a").agent,
        status: "error",
        updatedAt: "2026-10-18T10:01:00.000Z",
      },
    } as PaseoAgentUpdate);
    fake.setPages([
      [
        entry("agent-a", {
          status: "idle",
          updatedAt: "2026-10-18T10:02:00.000Z",
        }),
      ],
    ]);
    fake.emitReconnectSnapshot();
    held.release();
    await oldRefresh;

    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries[0]?.status === "idle",
    );
    expect(settled.live).toBe(true);
    expect(settled.entries[0]?.updatedAt).toBe("2026-10-18T10:02:00.000Z");

    await directory.dispose();
  });

  it("ignores a late update from the previous subscription generation", async () => {
    const authoritative = entry("agent-a", {
      status: "idle",
      updatedAt: "2026-10-18T10:02:00.000Z",
    });
    const fake = fakeDirectory([[authoritative]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    await directory.reconnect();
    expect(latest.entries[0]?.status).toBe("idle");

    fake.emitUpdateOn(0, {
      kind: "upsert",
      agent: {
        ...authoritative.agent,
        status: "error",
        updatedAt: "2026-10-18T10:01:00.000Z",
      },
    } as PaseoAgentUpdate);

    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(true);
    expect(latest.entries[0]?.status).toBe("idle");
    expect(latest.entries[0]?.updatedAt).toBe("2026-10-18T10:02:00.000Z");

    await directory.dispose();
  });

  it("keeps a complete read but reports live updates unavailable when subscribing fails", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    fake.failSubscription();
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();

    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(false);
    expect(latest.entries).toHaveLength(1);
    expect(latest.staleReason).toMatch(/Live updates are unavailable/);

    fake.failSubscription(false);
    await directory.refresh();

    expect(latest.live).toBe(true);
    expect(latest.staleReason).toBeNull();

    await directory.dispose();
  });

  it("marks live updates stale when the subscription reports an error", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    expect(latest.live).toBe(true);

    fake.emitError(new Error("socket lost"));

    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.entries).toHaveLength(1);

    await directory.dispose();
  });

  it("reloads the directory when the archive scope changes", async () => {
    const archivedAt = "2026-10-18T11:00:00.000Z";
    const fake = fakeDirectory([
      [entry("agent-a"), entry("agent-b", { archivedAt })],
    ]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);

    await directory.setIncludeArchived(true);
    expect(latest.includeArchived).toBe(true);
    expect(latest.entries.map((item) => item.id).sort()).toEqual(["agent-a", "agent-b"]);

    await directory.dispose();
  });

  it("does not let an archived update stay in the default scope", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a").agent,
        archivedAt: "2026-10-18T11:00:00.000Z",
      },
    } as PaseoAgentUpdate);

    expect(latest.entries).toHaveLength(0);

    await directory.dispose();
  });
});
