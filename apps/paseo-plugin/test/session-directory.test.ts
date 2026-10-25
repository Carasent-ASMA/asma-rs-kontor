import type {
  PaseoAgentUpdate,
  PaseoAgentListOptions,
  PaseoAgentListResult,
} from "@getpaseo/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  collectDirectory,
  deriveNeedsInput,
  emptyDirectoryState,
  DIRECTORY_REQUEST_TIMEOUT_MS,
  SessionDirectory,
  sortDirectoryEntries,
  toDirectoryEntry,
  type AgentDirectoryPage,
  type DirectoryClient,
  type DirectoryEntry,
  type DirectoryState,
} from "../client/session-directory";
import { attentionLabel, earlierErrorLabel, matchesQuery, matchesStatus } from "../client/session-view";
import { sessionState } from "../client/session-organization";
import { usagePresentation } from "../client/session-usage";

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
  pendingPermissions?: readonly unknown[];
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
      pendingPermissions: overrides.pendingPermissions ?? [],
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
  type HeldPage = {
    readonly started: Promise<void>;
    readonly markStarted: () => void;
    readonly gate: Promise<void>;
    readonly release: () => void;
  };
  const heldPages: HeldPage[] = [];
  const heldSubscribes: HeldPage[] = [];
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
      if (heldSubscribes.length > 0) {
        const held = heldSubscribes.shift() as HeldPage;
        held.markStarted();
        await held.gate;
      }
      const limit = options.page?.limit ?? 200;
      const first = (visible[0] ?? []).slice(0, limit);
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
    if (heldPages.length > 0) {
      const held = heldPages.shift() as HeldPage;
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
    recover() {
      failure = "none";
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
      const held: HeldPage = {
        started,
        markStarted,
        gate,
        release: () => {
          const index = heldPages.indexOf(held);
          if (index >= 0) {
            heldPages.splice(index, 1);
          }
          releaseGate();
        },
      };
      heldPages.push(held);
      return { started, release: held.release };
    },
    holdNextSubscribe() {
      let markStarted!: () => void;
      let releaseGate!: () => void;
      const started = new Promise<void>((resolve) => {
        markStarted = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        releaseGate = resolve;
      });
      const held: HeldPage = {
        started,
        markStarted,
        gate,
        release: () => {
          const index = heldSubscribes.indexOf(held);
          if (index >= 0) {
            heldSubscribes.splice(index, 1);
          }
          releaseGate();
        },
      };
      heldSubscribes.push(held);
      return { started, release: held.release };
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
  it("enforces native archive membership on every page even if an upstream page includes an archive", async () => {
    const pages = [[entry("live-a"), entry("archived-a", { archivedAt: "2026-10-18T11:00:00Z" })],
      [entry("closed-readable", { status: "closed" }), entry("archived-b", { archivedAt: "2026-10-18T11:00:00Z" })]];
    const list = vi.fn(async (options: PaseoAgentListOptions) => {
      const index = cursorIndex(options.page?.cursor);
      return { requestId: "fixture", entries: pages[index], pageInfo: pageInfo(index, pages.length) };
    });
    const client = { agents: { list } } as unknown as DirectoryClient;
    expect((await collectDirectory(client, false)).entries.map((row) => row.id)).toEqual(["live-a", "closed-readable"]);
    expect((await collectDirectory(client, true)).entries.map((row) => row.id)).toEqual(["live-a", "archived-a", "closed-readable", "archived-b"]);
    expect(list).toHaveBeenCalledTimes(4);
  });
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
    expect(fake.listCalls.every((call) => call.timeout === DIRECTORY_REQUEST_TIMEOUT_MS)).toBe(true);
  });

  it("settles a page wait when a newer directory read supersedes it", async () => {
    const controller = new AbortController();
    const list = vi.fn(() => new Promise<never>(() => undefined));
    const client = { agents: { list } } as unknown as DirectoryClient;
    const read = collectDirectory(client, false, controller.signal);
    controller.abort();
    await expect(read).rejects.toThrow(/superseded/);
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

  it("rejects a malformed blank cursor instead of returning a partial inventory", async () => {
    const fake = fakeDirectory([Array.from({ length: 200 }, (_, index) => entry(`a-${index}`))]);
    fake.setPageInfo(() => ({ hasMore: true, nextCursor: "   ", prevCursor: null }));

    await expect(collectDirectory(fake.client, false)).rejects.toThrow(/no next cursor/);
  });

  it("walks more than one thousand sessions and keeps one row per exact native ID", async () => {
    const rows = Array.from({ length: 1_237 }, (_, index) => entry(`bulk-${String(index).padStart(4, "0")}`));
    const pages = Array.from({ length: 7 }, (_, page) => rows.slice(page * 200, page * 200 + 200));
    pages[1] = [entry("bulk-0000", { title: "Updated between pages" }), ...(pages[1] ?? [])];
    const fake = fakeDirectory(pages);

    const collected = await collectDirectory(fake.client, true);

    expect(collected.pages).toBe(7);
    expect(collected.entries).toHaveLength(1_237);
    expect(new Set(collected.entries.map((item) => item.id)).size).toBe(1_237);
    expect(collected.entries.find((item) => item.id === "bulk-0000")?.title).toBe("Updated between pages");
    expect(collected.entries.some((item) => item.id === "bulk-1236")).toBe(true);
    expect(fake.listCalls.every((call) => call.page?.limit === 200 && call.filter?.includeArchived === true)).toBe(true);
  });
});

describe("SessionDirectory", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("applies the bootstrap page provisionally and keeps the inventory count unknown until the walk completes", async () => {
    const rows = Array.from({ length: 60 }, (_, index) => entry(`bootstrap-${index}`));
    const fake = fakeDirectory([rows.slice(0, 40), rows.slice(40)]);
    const held = fake.holdNextPage();
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      bootstrapLimit: 25,
      onChange: (state) => {
        latest = state;
      },
    });

    const starting = directory.start();
    await held.started;

    expect(latest).toMatchObject({
      status: "loading",
      live: false,
      pages: 0,
      lastUpdatedAt: null,
      inventoryComplete: false,
    });
    expect(latest.entries).toHaveLength(25);
    expect(new Set(latest.entries.map((item) => item.id))).toEqual(
      new Set(rows.slice(0, 25).map((item) => item.agent.id)),
    );
    // The count guard in matchingCount stays "…" until a completed walk sets lastUpdatedAt.
    expect(latest.lastUpdatedAt).toBeNull();

    held.release();
    await starting;

    expect(latest).toMatchObject({
      status: "ready",
      live: true,
      pages: 2,
      inventoryComplete: true,
    });
    expect(latest.entries).toHaveLength(60);
    expect(latest.lastUpdatedAt).not.toBeNull();

    await directory.dispose();
  });

  it("keeps a failed first walk retrying with a bounded delay until it succeeds", async () => {
    vi.useFakeTimers();
    const fake = fakeDirectory([[entry("agent-a")]]);
    fake.failAlways();
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      retryDelayMs: 100,
      maxRetryDelayMs: 150,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    expect(latest).toMatchObject({ status: "error", entries: [] });

    fake.recover();
    await vi.advanceTimersByTimeAsync(150);

    expect(latest).toMatchObject({ status: "ready", live: true, inventoryComplete: true });
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);
    expect(vi.getTimerCount()).toBe(0);

    await directory.dispose();
  });

  it("clears a pending retry on dispose and does not read again", async () => {
    vi.useFakeTimers();
    const fake = fakeDirectory([[entry("agent-a")]]);
    fake.failAlways();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      retryDelayMs: 100,
      onChange: () => undefined,
    });

    await directory.start();
    const calls = fake.listCalls.length;
    await directory.dispose();

    expect(vi.getTimerCount()).toBe(0);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(fake.listCalls).toHaveLength(calls);
  });

  it("drops archived rows immediately when leaving history even while the replacement read waits or fails", async () => {
    const fake = fakeDirectory([[entry("live"), entry("archived", { archivedAt: "2026-10-18T11:00:00Z" })]]);
    const directory = new SessionDirectory({ client: () => fake.client, includeArchived: true, onChange: () => {} });
    await directory.start();
    expect(directory.getState().entries.map((row) => row.id).sort()).toEqual(["archived", "live"]);
    const held = fake.holdNextSubscribe();
    const changing = directory.setIncludeArchived(false);
    await held.started;
    try {
      expect(directory.getState().entries.map((row) => row.id)).toEqual(["live"]);
    } finally {
      fake.failNext(); held.release(); await changing;
    }
    expect(directory.getState().status).toBe("stale");
    expect(directory.getState().entries.map((row) => row.id)).toEqual(["live"]);
    await directory.dispose();
  });
  it("keeps prompt and current-turn times separate from metadata updates, and keeps a turn without a start time", () => {
    const seed = entry("times", { status: "running" });
    const prompt = "2026-10-18T08:00:00.000Z";
    const startedAt = "2026-10-18T08:01:00.000Z";
    const first = toDirectoryEntry({ ...seed, agent: { ...seed.agent, lastUserMessageAt: prompt, activeTurn: { turnId: "turn-times", startedAt } } });
    expect(first).toMatchObject({ updatedAt: seed.agent.updatedAt, lastUserMessageAt: prompt, activeTurn: true, activeTurnStartedAt: startedAt });
    const unstarted = toDirectoryEntry({ ...seed, agent: { ...seed.agent, status: "idle", activeTurn: { turnId: "turn-without-start", startedAt: null } } });
    expect(unstarted).toMatchObject({ activeTurn: true, activeTurnStartedAt: null });
    const renamed = toDirectoryEntry({ ...seed, agent: { ...seed.agent, title: "Renamed", updatedAt: "2026-10-18T12:00:00.000Z", lastUserMessageAt: prompt, activeTurn: undefined } }, first);
    expect(renamed).toMatchObject({ updatedAt: "2026-10-18T12:00:00.000Z", lastUserMessageAt: prompt, activeTurn: false, activeTurnStartedAt: null });
    expect(toDirectoryEntry(seed)).toMatchObject({ lastUserMessageAt: null, activeTurn: false });
  });

  it("retains closed unarchived history across pages, subscriptions, stale cache and archive toggles", async () => {
    const history = Array.from({ length: 13 }, (_, n) => entry(`history-${n}`, {
      status: "closed", requiresAttention: true, attentionReason: "error",
      workspaceName: n < 10 ? "Archived workspace" : "Current workspace",
    }));
    const archived = entry("archived", { status: "closed", attentionReason: "error", archivedAt: "2026-10-18T11:00:00Z" });
    const fake = fakeDirectory([
      Array.from({ length: 200 }, (_, n) => entry(`filler-${n}`)),
      [...history, archived, entry("current", { status: "error", requiresAttention: true, attentionReason: "error" })],
    ]);
    const directory = new SessionDirectory({ client: () => fake.client, includeArchived: false, onChange: () => {} });
    const matching = (status: Parameters<typeof matchesStatus>[1]) => directory.getState().entries.filter((item) => matchesStatus(item, status));
    await directory.start();
    expect(directory.getState().pages).toBe(2);
    expect(directory.getState().entries).toHaveLength(214);
    expect(matching("earlier_error")).toHaveLength(13);
    expect(matching("closed")).toHaveLength(13);
    expect(matching("error").map((item) => item.id)).toEqual(["current"]);
    expect(matching("attention").map((item) => item.id)).toEqual(["current"]);
    expect(directory.getState().entries.find((item) => item.id === "history-0")?.archivedAt).toBeNull();

    for (const status of ["running", "idle", "closed"] as const) {
      fake.emitUpdate({ kind: "upsert", ...entry("current", { status, requiresAttention: true, attentionReason: "error" }) });
      const changed = directory.getState().entries.find((item) => item.id === "current")!;
      expect(matching("error")).toEqual([]);
      expect(matching("attention")).toEqual([]);
      expect(matching("earlier_error")).toHaveLength(14);
      expect(sessionState(changed)).toBe(status);
      expect(attentionLabel(changed)).toBeNull();
      expect(earlierErrorLabel(changed)).toBe(`Earlier error · now ${status}`);
    }
    fake.emitUpdate({ kind: "upsert", ...entry("current", { status: "error", requiresAttention: false, attentionReason: "error" }) });
    expect(matching("error").map((item) => item.id)).toEqual(["current"]);

    directory.markHostOffline();
    expect(directory.getState().status).toBe("stale");
    expect(matching("earlier_error")).toHaveLength(13);
    await directory.reconnect();
    expect(directory.getState().live).toBe(true);
    expect(matching("earlier_error")).toHaveLength(13);
    await directory.setIncludeArchived(true);
    expect(directory.getState().entries).toHaveLength(215);
    expect(matching("earlier_error")).toHaveLength(14);
    expect(directory.getState().entries.find((item) => item.id === "archived")?.archivedAt).not.toBeNull();
    await directory.setIncludeArchived(false);
    expect(directory.getState().entries).toHaveLength(214);
    expect(matching("earlier_error")).toHaveLength(13);
    await directory.dispose();
  });

  it("projects subscription usage snapshots without accumulating duplicates or carrying reports over a reset", async () => {
    const fake = fakeDirectory([[entry("usage-agent")]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({ client: () => fake.client, includeArchived: false, onChange: (state) => { latest = state; } });
    await directory.start();
    const native = { ...entry("usage-agent").agent, provider: "codex", model: "gpt-6.1-sol", lastUsage: { inputTokens: 100, outputTokens: 20, cachedInputTokens: 80 } };
    const upsert = { kind: "upsert", agent: native } as const;
    fake.emitUpdate(upsert);
    fake.emitUpdate(upsert);
    expect(usagePresentation(latest.entries[0]!).label).toBe("Last report 100 ↓ · 20 ↑");
    fake.emitUpdate({ kind: "upsert", agent: { ...native, model: "other-model", lastUsage: { inputTokens: 5, outputTokens: 0 } } });
    expect(latest.entries[0]!.model).toBe("other-model");
    expect(usagePresentation(latest.entries[0]!).label).toBe("Last report 5 ↓ · 0 ↑");
    fake.emitUpdate({ kind: "upsert", agent: { ...native, provider: "other-route", lastUsage: undefined, runtimeInfo: undefined } });
    expect(usagePresentation(latest.entries[0]!).label).toBe("—");
    directory.markHostOffline("Disconnected");
    expect(latest.status).toBe("stale");
    await directory.dispose();
  });
  it("searches recognized epic and workstream label values", () => {
    const directoryEntry = toDirectoryEntry(
      entry("labeled", {
        title: "Planner session",
        labels: {
          epic: "ASMA-8539",
          workstream: "kontor",
          "kontor.role": "rev-b",
          "paseo.parent-agent-id": "internal-only",
        },
      }),
    );

    expect(directoryEntry.searchText).toContain("asma-8539");
    expect(directoryEntry.searchText).toContain("kontor");
    expect(directoryEntry.searchText).toContain("rev-b");
    expect(directoryEntry.searchText).not.toContain("internal-only");
    expect(matchesQuery(directoryEntry, "ASMA-8539")).toBe(true);
    expect(matchesQuery(directoryEntry, "kontor")).toBe(true);
  });

  it("derives native needs-input priority from pending permissions and permission attention", () => {
    const pending = toDirectoryEntry(
      entry("pending", {
        status: "running",
        requiresAttention: false,
        pendingPermissions: [{ id: "permission-1" }],
      }),
    );
    expect(pending.status).toBe("running");
    expect(pending.pendingPermissionCount).toBe(1);
    expect(pending.needsInput).toBe(true);

    const permissionReason = toDirectoryEntry(
      entry("permission-reason", {
        status: "running",
        requiresAttention: false,
        attentionReason: "permission",
      }),
    );
    expect(permissionReason.status).toBe("running");
    expect(permissionReason.pendingPermissionCount).toBe(0);
    expect(permissionReason.needsInput).toBe(true);

    const failed = toDirectoryEntry(
      entry("failed", {
        status: "error",
        requiresAttention: true,
        attentionReason: "error",
      }),
    );
    const finished = toDirectoryEntry(
      entry("finished", {
        status: "idle",
        requiresAttention: true,
        attentionReason: "finished",
      }),
    );
    const idle = toDirectoryEntry(entry("idle", { status: "idle" }));
    const closed = toDirectoryEntry(entry("closed", { status: "closed" }));
    const initializing = toDirectoryEntry(
      entry("initializing", { status: "initializing" }),
    );

    expect(failed.needsInput).toBe(false);
    expect(finished.needsInput).toBe(false);
    expect(idle.needsInput).toBe(false);
    expect(closed.needsInput).toBe(false);
    expect(initializing.needsInput).toBe(false);
    expect(deriveNeedsInput({ pendingPermissionCount: 0, attentionReason: null })).toBe(
      false,
    );
  });

  it("clears needs input from the next snapshot instead of carrying a stale badge", () => {
    const pending = toDirectoryEntry(
      entry("agent-a", {
        status: "running",
        pendingPermissions: [{ id: "permission-1" }],
      }),
    );
    const resolved = toDirectoryEntry(
      entry("agent-a", { status: "idle" }),
      pending,
    );

    expect(pending.needsInput).toBe(true);
    expect(resolved.status).toBe("idle");
    expect(resolved.pendingPermissionCount).toBe(0);
    expect(resolved.needsInput).toBe(false);
  });

  it("sorts needs input ahead of failed and other attention states", () => {
    const entries = [
      toDirectoryEntry(entry("failed", { status: "error", attentionReason: "error" })),
      toDirectoryEntry(entry("finished", { status: "idle", requiresAttention: true, attentionReason: "finished" })),
      toDirectoryEntry(entry("needs-input", { status: "running", pendingPermissions: [{ id: "p" }] })),
      toDirectoryEntry(entry("idle", { status: "idle" })),
    ];

    expect(sortDirectoryEntries(entries).map((item) => item.id)).toEqual([
      "needs-input",
      "failed",
      "finished",
      "idle",
    ]);
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

  it("clears needs input after a permission update resolves or is cancelled", async () => {
    const fake = fakeDirectory([
      [
        entry("agent-a", {
          status: "running",
          requiresAttention: false,
          pendingPermissions: [{ id: "permission-1" }],
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
    expect(latest.entries[0]?.needsInput).toBe(true);
    expect(latest.entries[0]?.status).toBe("running");

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a", { status: "idle" }).agent,
        pendingPermissions: [],
        requiresAttention: false,
        attentionReason: null,
      },
    } as PaseoAgentUpdate);

    expect(latest.entries[0]?.status).toBe("idle");
    expect(latest.entries[0]?.needsInput).toBe(false);
    expect(latest.entries[0]?.pendingPermissionCount).toBe(0);

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a", { status: "running" }).agent,
        pendingPermissions: [],
        requiresAttention: false,
        attentionReason: "permission",
      },
    } as PaseoAgentUpdate);
    expect(latest.entries[0]?.needsInput).toBe(true);

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...entry("agent-a", { status: "idle" }).agent,
        pendingPermissions: [],
        requiresAttention: false,
        attentionReason: null,
      },
    } as PaseoAgentUpdate);
    expect(latest.entries[0]?.needsInput).toBe(false);

    await directory.dispose();
  });

  it("does not carry a needs-input badge across a reconnect", async () => {
    const fake = fakeDirectory([
      [
        entry("agent-a", {
          status: "running",
          requiresAttention: false,
          pendingPermissions: [{ id: "permission-1" }],
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
    expect(latest.entries[0]?.needsInput).toBe(true);

    directory.markHostOffline();
    expect(latest.status).toBe("stale");
    expect(latest.entries[0]?.needsInput).toBe(true);

    fake.setPages([[entry("agent-a", { status: "idle" })]]);
    await directory.reconnect();

    expect(latest.status).toBe("ready");
    expect(latest.entries[0]?.status).toBe("idle");
    expect(latest.entries[0]?.needsInput).toBe(false);
    expect(latest.entries[0]?.pendingPermissionCount).toBe(0);

    await directory.dispose();
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

  it("re-walks when an overflowed refresh walk fails once", async () => {
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
    const refreshing = directory.refresh();
    for (let index = 0; index < 257; index += 1) {
      fake.emitUpdate({
        kind: "upsert",
        agent: {
          ...entry("agent-a").agent,
          title: `Session agent-a ${index}`,
          updatedAt: `2026-10-18T10:${String(index % 60).padStart(2, "0")}:00.000Z`,
        },
      } as PaseoAgentUpdate);
    }
    await refreshing;

    expect(
      fake.listCalls.filter((call) => call.subscribe === undefined).length,
    ).toBeGreaterThanOrEqual(2);
    expect(latest.status).toBe("ready");

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

  it("refuses to report an inventory when a mid-walk page fails", async () => {
    const fake = fakeDirectory([
      Array.from({ length: 200 }, (_, index) => entry(`partial-${index}`)),
      [entry("partial-tail")],
    ]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({ client: () => fake.client, includeArchived: true, onChange: (state) => { latest = state; } });
    const held = fake.holdNextPage();
    const starting = directory.start();
    await held.started;
    fake.failNext();
    held.release();
    await starting;

    expect(latest.status).toBe("error");
    expect(latest.inventoryComplete).toBe(false);
    expect(latest.lastUpdatedAt).toBeNull();
    expect(latest.entries).toHaveLength(0);
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

  it("keeps stale and not live while the post-reconnect walk is still pending", async () => {
    const oldRunning = entry("agent-a", {
      status: "running",
      updatedAt: "2026-10-18T10:00:00.000Z",
    });
    const freshIdle = entry("agent-a", {
      status: "idle",
      updatedAt: "2026-10-18T10:02:00.000Z",
    });
    const fake = fakeDirectory([[oldRunning]]);
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
    const oldWalk = fake.holdNextPage();
    const freshWalk = fake.holdNextPage();
    const refreshing = directory.refresh();
    await oldWalk.started;

    fake.emitUpdate({
      kind: "upsert",
      agent: {
        ...oldRunning.agent,
        status: "error",
        updatedAt: "2026-10-18T10:01:00.000Z",
      },
    } as PaseoAgentUpdate);
    fake.setPages([[freshIdle]]);
    const beforeReconnect = states.length;
    fake.emitReconnectSnapshot();

    oldWalk.release();
    await freshWalk.started;

    const duringFreshWalk = states.slice(beforeReconnect);
    expect(
      duringFreshWalk.some((state) => state.status === "ready" || state.live),
    ).toBe(false);
    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.entries[0]?.status).toBe("running");
    expect(latest.entries[0]?.updatedAt).toBe("2026-10-18T10:00:00.000Z");

    freshWalk.release();
    await refreshing;

    const settled = await waitForState(
      () => latest,
      (state) =>
        state.status === "ready" &&
        state.live &&
        state.entries[0]?.status === "idle" &&
        state.entries[0]?.updatedAt === "2026-10-18T10:02:00.000Z",
    );
    expect(settled.entries[0]?.status).toBe("idle");

    await directory.dispose();
  });

  it("aborts a pre-reconnect read that never settles and starts a fresh walk", async () => {
    const fake = fakeDirectory([[entry("agent-a", { title: "Old" })]]);
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
    fake.setPages([[entry("agent-a", { title: "Fresh" })]]);

    directory.markHostOffline();
    const reconnecting = directory.reconnect();
    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries[0]?.title === "Fresh",
      1_500,
    );
    await reconnecting;

    expect(settled.live).toBe(true);
    expect(settled.entries.map((item) => item.id)).toEqual(["agent-a"]);
    expect(settled.entries[0]?.title).toBe("Fresh");

    held.release();
    await oldRefresh;
    expect(latest.entries[0]?.title).toBe("Fresh");

    await directory.dispose();
  });

  it("settles a pre-reconnect subscription that never returns before reconnecting", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    const held = fake.holdNextSubscribe();
    const starting = directory.start();
    await held.started;

    directory.markHostOffline();
    await starting;

    const reconnecting = directory.reconnect();
    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries.length === 1,
      1_500,
    );
    await reconnecting;

    expect(settled.live).toBe(true);
    expect(settled.entries[0]?.id).toBe("agent-a");

    held.release();
    await directory.dispose();
  });

  it("Retry supersedes a held read while last-known rows stay stale until the fresh walk completes", async () => {
    const fake = fakeDirectory([[entry("agent-a", { title: "Old" })]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const oldHeld = fake.holdNextPage();
    const oldRefresh = directory.refresh();
    await oldHeld.started;
    fake.setPages([[entry("agent-a", { title: "Fresh" })]]);

    const retryHeld = fake.holdNextPage();
    const retrying = directory.retry();
    await retryHeld.started;

    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);
    expect(latest.entries[0]?.title).toBe("Old");
    expect(latest.staleReason).toMatch(/Reconnected/);

    retryHeld.release();
    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries[0]?.title === "Fresh",
      1_500,
    );
    await retrying;

    expect(settled.live).toBe(true);
    expect(settled.entries[0]?.title).toBe("Fresh");

    oldHeld.release();
    await oldRefresh;
    expect(latest.entries[0]?.title).toBe("Fresh");

    await directory.dispose();
  });

  it("uses one fresh generation for repeated Retry and fences both late reads", async () => {
    const fake = fakeDirectory([[entry("agent-a", { title: "Old" })]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const firstHeld = fake.holdNextPage();
    const oldRefresh = directory.refresh();
    await firstHeld.started;

    const secondHeld = fake.holdNextPage();
    const firstRetry = directory.retry();
    await secondHeld.started;

    fake.setPages([[entry("agent-a", { title: "Latest retry" })]]);
    const thirdHeld = fake.holdNextPage();
    const secondRetry = directory.retry();
    await thirdHeld.started;

    thirdHeld.release();
    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries[0]?.title === "Latest retry",
      1_500,
    );
    await Promise.all([firstRetry, secondRetry]);

    expect(settled.live).toBe(true);
    expect(settled.entries[0]?.title).toBe("Latest retry");

    firstHeld.release();
    secondHeld.release();
    await oldRefresh;
    expect(latest.entries[0]?.title).toBe("Latest retry");

    await directory.dispose();
  });

  it("keeps the latest Retry stale and does not walk while its subscription is held", async () => {
    const fake = fakeDirectory([[entry("agent-a", { title: "Last known" })]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const initialPages = fake.listCalls.filter((call) => call.subscribe === undefined).length;
    const first = fake.holdNextSubscribe();
    const second = fake.holdNextSubscribe();
    const firstRetry = directory.retry();
    await first.started;
    const secondRetry = directory.retry();
    await second.started;

    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(latest.status).toBe("stale");
    expect(latest.live).toBe(false);
    expect(latest.inventoryComplete).toBe(false);
    expect(latest.entries[0]?.title).toBe("Last known");
    expect(fake.listCalls.filter((call) => call.subscribe === undefined)).toHaveLength(initialPages);

    second.release();
    first.release();
    await Promise.all([firstRetry, secondRetry]);
    await directory.dispose();
  });

  it("runs one fresh walk after the latest Retry and keeps it live when the old subscription settles late", async () => {
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
    const initialPages = fake.listCalls.filter((call) => call.subscribe === undefined).length;
    const first = fake.holdNextSubscribe();
    const second = fake.holdNextSubscribe();
    const firstRetry = directory.retry();
    await first.started;
    const secondRetry = directory.retry();
    await second.started;
    await new Promise((resolve) => setTimeout(resolve, 20));

    second.release();
    await Promise.all([firstRetry, secondRetry]);
    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(true);
    expect(latest.inventoryComplete).toBe(true);

    first.release();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(true);
    expect(latest.inventoryComplete).toBe(true);
    expect(fake.listCalls.filter((call) => call.subscribe === undefined)).toHaveLength(initialPages + 1);
    await directory.dispose();
  });

  it("starts a fresh walk for each reconnect while an earlier read is held", async () => {
    const fake = fakeDirectory([[entry("agent-a", { title: "Old" })]]);
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    await directory.start();
    const firstHeld = fake.holdNextPage();
    const oldRefresh = directory.refresh();
    await firstHeld.started;

    fake.setPages([[entry("agent-a", { title: "First reconnect" })]]);
    const secondHeld = fake.holdNextPage();
    const firstReconnect = directory.reconnect();
    await secondHeld.started;

    fake.setPages([[entry("agent-a", { title: "Latest reconnect" })]]);
    const latestReconnect = directory.reconnect();
    const settled = await waitForState(
      () => latest,
      (state) => state.status === "ready" && state.entries[0]?.title === "Latest reconnect",
      1_500,
    );
    await Promise.all([firstReconnect, latestReconnect]);

    expect(settled.entries[0]?.title).toBe("Latest reconnect");
    firstHeld.release();
    secondHeld.release();
    await oldRefresh;
    expect(latest.entries[0]?.title).toBe("Latest reconnect");

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
    expect(latest.inventoryComplete).toBe(false);
    expect(latest.entries).toHaveLength(1);
    expect(latest.staleReason).toMatch(/may be incomplete/);

    fake.failSubscription(false);
    await directory.refresh();

    expect(latest.live).toBe(true);
    expect(latest.inventoryComplete).toBe(true);
    expect(latest.staleReason).toBeNull();

    await directory.dispose();
  });

  it("does not claim a complete inventory when a session moves behind the cursor while live is unavailable", async () => {
    const first = entry("agent-a", { updatedAt: "2026-10-18T10:02:00.000Z" });
    const moved = entry("agent-b", { updatedAt: "2026-10-18T10:00:00.000Z" });
    const fake = fakeDirectory([[first], [moved]]);
    fake.failSubscription();
    const held = fake.holdNextPage();
    let latest = emptyDirectoryState();
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: (state) => {
        latest = state;
      },
    });

    const starting = directory.start();
    await held.started;
    // Simulate the host moving agent-b ahead of the cursor after page one was
    // read: the remaining walk no longer contains it, and no subscription is
    // available to deliver the update.
    fake.setPages([[first], []]);
    held.release();
    await starting;

    expect(latest.status).toBe("ready");
    expect(latest.live).toBe(false);
    expect(latest.inventoryComplete).toBe(false);
    expect(latest.staleReason).toMatch(/may be incomplete/);
    expect(latest.entries.map((item) => item.id)).toEqual(["agent-a"]);

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

  it("shows an error, not a spinner, when the archive scope changes offline", async () => {
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
    directory.markHostOffline();
    await directory.setIncludeArchived(true);

    expect(latest.includeArchived).toBe(true);
    expect(latest.status).toBe("error");
    expect(latest.error).toMatch(/offline/);
    expect(latest.entries).toHaveLength(0);

    await directory.dispose();
  });

  it("keeps rows when the online archive-scope re-read fails", async () => {
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
    const previousPages = latest.pages;
    const previousReadAt = latest.lastUpdatedAt;
    const subscribe = fake.holdNextSubscribe();
    const changing = directory.setIncludeArchived(true);
    await subscribe.started;
    fake.failNext();
    subscribe.release();
    await changing;

    expect(latest.includeArchived).toBe(true);
    expect(latest.status).toBe("stale");
    expect(latest.entries).toHaveLength(1);
    expect(latest.error).toMatch(/daemon unavailable/);
    expect(latest.pages).toBe(previousPages);
    expect(latest.lastUpdatedAt).toBe(previousReadAt);

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

  it("serializes a second refresh while the first waits for the re-subscribe", async () => {
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
    fake.emitError(new Error("live updates stopped"));
    fake.listCalls.length = 0;
    const subscribe = fake.holdNextSubscribe();
    const first = directory.refresh();
    await subscribe.started;
    const second = directory.refresh();
    const page = fake.holdNextPage();
    subscribe.release();
    await page.started;
    const third = directory.refresh();

    expect(fake.listCalls.filter((call) => call.subscribe === undefined)).toHaveLength(1);

    page.release();
    await Promise.all([first, second, third]);
    expect(latest.entries.map((item) => item.id)).toContain("agent-a");

    await directory.dispose();
  });

  it("does not walk after dispose while the subscription is pending", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: () => undefined,
    });

    const subscribe = fake.holdNextSubscribe();
    const starting = directory.start();
    await subscribe.started;
    await directory.dispose();
    subscribe.release();
    await starting;

    expect(fake.listCalls.filter((call) => call.subscribe === undefined)).toHaveLength(0);
  });

  it("opens one subscription when refresh races a pending connect", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]);
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: () => undefined,
    });

    const subscribe = fake.holdNextSubscribe();
    const starting = directory.start();
    await subscribe.started;
    const refreshing = directory.refresh();
    subscribe.release();
    await Promise.all([starting, refreshing]);

    expect(fake.listCalls.filter((call) => call.subscribe !== undefined)).toHaveLength(1);
    await directory.dispose();
  });

  it("stops page fetches after dispose during a walk", async () => {
    const fake = fakeDirectory([[entry("agent-a")], [entry("agent-b")]]);
    const directory = new SessionDirectory({
      client: () => fake.client,
      includeArchived: false,
      onChange: () => undefined,
    });

    const page = fake.holdNextPage();
    const starting = directory.start();
    await page.started;
    await directory.dispose();
    page.release();
    await starting;

    expect(fake.listCalls.filter((call) => call.subscribe === undefined)).toHaveLength(1);
  });
});

describe("routine refresh after an Archive X", () => {
  async function started(fake: ReturnType<typeof fakeDirectory>) {
    const states: DirectoryState[] = [];
    const directory = new SessionDirectory({ client: () => fake.client, includeArchived: false, onChange: (state) => { states.push(state); } });
    await directory.start();
    return { directory, states, latest: () => states[states.length - 1]! };
  }

  it("keeps a live, complete, ready directory calm while the refresh walks", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]), s = await started(fake);
    expect(s.latest()).toMatchObject({ status: "ready", live: true, inventoryComplete: true, staleReason: null });
    const from = s.states.length, held = fake.holdNextPage(), refreshing = s.directory.refresh();
    await held.started;
    expect(s.states.slice(from).map((state) => state.status)).not.toContain("stale");
    held.release(); await refreshing;
    expect(s.states.slice(from).every((state) => state.status === "ready" && state.staleReason === null)).toBe(true);
    await s.directory.dispose();
  });

  it("still reports a failed calm refresh as stale", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]), s = await started(fake);
    fake.failNext(); await s.directory.refresh();
    expect(s.latest()).toMatchObject({ status: "stale", staleReason: "Could not refresh the full directory.", error: expect.stringMatching(/daemon unavailable/) });
    await s.directory.dispose();
  });

  it.each<[string, (fake: ReturnType<typeof fakeDirectory>, s: Awaited<ReturnType<typeof started>>) => Promise<void> | void, RegExp]>([
    ["stopped live updates", (fake) => fake.emitError(new Error("socket lost")), /Live updates stopped/],
    ["a failed refresh", async (fake, s) => { fake.failNext(); await s.directory.refresh(); }, /Could not refresh/],
    ["a partial inventory", async (fake, s) => { fake.failSubscription(); s.directory.markHostOffline(); await s.directory.reconnect(); }, /may be incomplete|Live updates are unavailable/],
  ])("keeps %s stale through a routine refresh", async (_label, arrange, reason) => {
    const fake = fakeDirectory([[entry("agent-a")]]), s = await started(fake);
    await arrange(fake, s);
    expect(s.latest().staleReason).toMatch(reason);
    const from = s.states.length, held = fake.holdNextPage(), refreshing = s.directory.refresh();
    await held.started;
    expect(s.states.slice(from).length).toBeGreaterThan(0);
    expect(s.states.slice(from).every((state) => state.status === "stale" && reason.test(state.staleReason ?? ""))).toBe(true);
    held.release(); await refreshing;
    await s.directory.dispose();
  });

  it("keeps an offline host offline instead of walking", async () => {
    const fake = fakeDirectory([[entry("agent-a")]]), s = await started(fake);
    s.directory.markHostOffline(); const calls = fake.listCalls.length;
    await s.directory.refresh();
    expect(s.latest()).toMatchObject({ status: "stale", staleReason: expect.stringMatching(/disconnected/) });
    expect(fake.listCalls).toHaveLength(calls);
    await s.directory.dispose();
  });
});
