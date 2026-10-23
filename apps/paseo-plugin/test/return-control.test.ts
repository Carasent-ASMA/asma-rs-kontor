import type {
  PaseoAgentListOptions,
  PaseoAgentListResult,
  PaseoAgentUpdate,
} from "@getpaseo/client";
import type {
  PluginButtonRegistration,
  PluginClientContext,
} from "@getpaseo/plugin/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { getPaseoClient } = vi.hoisted(() => ({ getPaseoClient: vi.fn() }));

vi.mock("@getpaseo/plugin/client", () => ({ getPaseoClient }));

import { contributePlanControl } from "../client/plan-control";
import {
  contributeReturnControl,
  RETURN_CONTROL_ICON,
  RETURN_CONTROL_ID,
  RETURN_CONTROL_LABEL,
  RETURN_CONTROL_TITLE,
} from "../client/return-control";
import type {
  AgentDirectoryPage,
  DirectoryState,
} from "../client/session-directory";
import {
  bindHostDirectory,
  disposeRetainedDirectories,
  startHostDirectory,
} from "../client/use-session-directory";

type Observer = {
  snapshot: () => void;
  update: (message: { type: "agent_update"; payload: PaseoAgentUpdate }) => void;
  error?: (error: unknown) => void;
};

type HostPageOverrides = {
  readonly workspaceId?: string | null;
  readonly status?: "initializing" | "idle" | "running" | "error" | "closed";
  readonly archivedAt?: string | null;
  readonly title?: string;
  readonly updatedAt?: string;
};

function agentPage(
  id: string,
  overrides: HostPageOverrides = {},
): AgentDirectoryPage {
  return {
    agent: {
      id,
      title: overrides.title ?? `Session ${id}`,
      status: overrides.status ?? "idle",
      provider: "fixture",
      cwd: "/fixture",
      workspaceId:
        overrides.workspaceId === undefined ? `wks-${id}` : overrides.workspaceId,
      model: null,
      labels: {},
      createdAt: "2026-10-22T09:00:00.000Z",
      updatedAt: overrides.updatedAt ?? "2026-10-22T10:00:00.000Z",
      archivedAt: overrides.archivedAt ?? null,
      pendingPermissions: [],
      requiresAttention: false,
      attentionReason: null,
      attentionTimestamp: null,
    },
    project: null,
  } as unknown as AgentDirectoryPage;
}

const hosts = new Map<string, ReturnType<typeof fakeHost>>();

function fakeHost(hostId: string, initialRows: readonly AgentDirectoryPage[]) {
  let rows = initialRows;
  let gate: Promise<void> | null = null;
  let unsubscribed = 0;
  let releases = 0;
  const calls: PaseoAgentListOptions[] = [];
  const observers: Observer[] = [];
  const list = async (options: PaseoAgentListOptions = {}) => {
    calls.push(options);
    if (!options.subscribe && gate) await gate;
    const visible = rows.filter(
      (row) =>
        options.filter?.includeArchived === true ||
        (row.agent.archivedAt ?? null) === null,
    );
    if (options.subscribe) {
      const limit = options.page?.limit ?? 200;
      const entries = visible.slice(0, limit);
      const snapshot = {
        requestId: "fixture",
        subscriptionId: "sub-1",
        entries,
        pageInfo: {
          hasMore: visible.length > entries.length,
          nextCursor: visible.length > entries.length ? String(entries.length) : null,
          prevCursor: null,
        },
      } as PaseoAgentListResult;
      const subscription = {
        subscriptionId: "sub-1",
        ready: Promise.resolve(snapshot),
        subscribe: (observer: Observer) => {
          observers.push(observer);
          observer.snapshot();
          return () => {
            unsubscribed += 1;
          };
        },
        release: async () => {
          releases += 1;
        },
      };
      return { ...snapshot, subscription };
    }
    const start = Number(options.page?.cursor ?? 0);
    const end = start + (options.page?.limit ?? 200);
    return {
      requestId: "fixture",
      entries: visible.slice(start, end),
      pageInfo: {
        hasMore: end < visible.length,
        nextCursor: end < visible.length ? String(end) : null,
        prevCursor: null,
      },
    };
  };
  const host = {
    hostId,
    client: { agents: { list } },
    calls,
    observers,
    get subscribes() {
      return calls.filter((call) => call.subscribe);
    },
    get walks() {
      return calls.filter((call) => !call.subscribe);
    },
    get unsubscribed() {
      return unsubscribed;
    },
    get releases() {
      return releases;
    },
    setRows(next: readonly AgentDirectoryPage[]) {
      rows = next;
    },
    holdWalk() {
      let release!: () => void;
      gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      return () => {
        gate = null;
        release();
      };
    },
    emitUpdate(update: PaseoAgentUpdate) {
      observers[0]?.update({ type: "agent_update", payload: update });
    },
  };
  hosts.set(hostId, host);
  return host;
}

type PillContribution = {
  id: string;
  workspaceId: string;
  agentId: string;
  button: {
    title: string;
    icon: string;
    label?: string;
    behavior: { kind: "action"; onPress(): void };
  };
};

type Pill = {
  readonly contribution: PillContribution;
  readonly registration: PluginButtonRegistration & {
    readonly remove: ReturnType<typeof vi.fn>;
    readonly update: ReturnType<typeof vi.fn>;
  };
  active: boolean;
};

function fakeControls(
  beforeAdd?: (contribution: PillContribution) => void,
) {
  const pills: Pill[] = [];
  const addComposerPill = vi.fn((contribution: PillContribution) => {
    beforeAdd?.(contribution);
    let pill!: Pill;
    const registration = {
      update: vi.fn(),
      remove: vi.fn(() => {
        pill.active = false;
      }),
    };
    pill = { contribution, registration, active: true };
    pills.push(pill);
    return registration;
  });
  const openScreen = vi.fn();
  const rpc = vi.fn(async () => ({
    status: "ready",
    associations: {} as Record<string, string[]>,
    sources: [] as { id: string }[],
    error: null,
  }));
  const client = {
    paseo: {},
    rpc,
    addComposerPill,
    openScreen,
  } as unknown as PluginClientContext;
  return {
    client,
    pills,
    addComposerPill,
    openScreen,
    rpc,
    activePills: () => pills.filter((pill) => pill.active),
    pillFor(agentId: string, id?: string): Pill | undefined {
      return [...pills]
        .reverse()
        .find(
          (pill) =>
            pill.active &&
            pill.contribution.agentId === agentId &&
            (id === undefined || pill.contribution.id === id),
        );
    },
  };
}

async function settle(): Promise<void> {
  for (let index = 0; index < 50; index += 1) {
    await Promise.resolve();
  }
}

function installHosts(): void {
  getPaseoClient.mockImplementation((hostId: string) => {
    const host = hosts.get(hostId);
    if (!host) {
      throw new Error(`No fixture host for ${hostId}`);
    }
    return host.client;
  });
}

describe("Kontor composer controls share one host directory", () => {
  beforeEach(async () => {
    await disposeRetainedDirectories();
    hosts.clear();
    getPaseoClient.mockReset();
    installHosts();
  });

  afterEach(async () => {
    await disposeRetainedDirectories();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("registers pills from the 25-row bootstrap and removes only after a complete walk", async () => {
    const rows = Array.from({ length: 60 }, (_, index) =>
      agentPage(`s-${index}`, { workspaceId: `wks-${index}` }),
    );
    const host = fakeHost("host-a", rows);
    const releaseWalk = host.holdWalk();
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();

    expect(host.subscribes).toHaveLength(1);
    expect(host.walks).toHaveLength(1);
    expect(controls.activePills()).toHaveLength(25);
    expect(controls.pillFor("s-0")?.contribution).toMatchObject({
      id: RETURN_CONTROL_ID,
      workspaceId: "wks-0",
      agentId: "s-0",
      button: {
        title: RETURN_CONTROL_TITLE,
        icon: RETURN_CONTROL_ICON,
        label: RETURN_CONTROL_LABEL,
        behavior: { kind: "action" },
      },
    });

    host.setRows(rows.slice(1));
    expect(controls.pillFor("s-0")).toBeDefined();

    releaseWalk();
    await settle();

    expect(controls.activePills()).toHaveLength(59);
    expect(controls.pillFor("s-0")).toBeUndefined();
    expect(host.walks).toHaveLength(1);
    expect(host.subscribes).toHaveLength(1);

    controls.pillFor("s-1")?.contribution.button.behavior.onPress();
    expect(controls.openScreen).toHaveBeenCalledWith({ screenId: "sessions" });

    stop();
  });

  it("follows native updates and removes closed, archived and removed sessions", async () => {
    const rows = [agentPage("agent-a", { workspaceId: "wks-a" })];
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    expect(controls.pillFor("agent-a")).toBeDefined();

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        archivedAt: "2026-10-22T11:00:00.000Z",
      },
    } as PaseoAgentUpdate);
    expect(controls.pillFor("agent-a")).toBeUndefined();

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        id: "agent-b",
        workspaceId: "wks-b",
      },
    } as PaseoAgentUpdate);
    expect(controls.pillFor("agent-b")).toBeDefined();

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        id: "agent-b",
        workspaceId: "wks-b",
        status: "closed",
      },
    } as PaseoAgentUpdate);
    expect(controls.pillFor("agent-b")).toBeUndefined();

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        id: "agent-c",
        workspaceId: "wks-c",
      },
    } as PaseoAgentUpdate);
    expect(controls.pillFor("agent-c")).toBeDefined();
    host.emitUpdate({ kind: "remove", agentId: "agent-c" } as PaseoAgentUpdate);
    expect(controls.pillFor("agent-c")).toBeUndefined();

    stop();
  });

  it("lets the plan control read sessions from the same directory without another walk", async () => {
    vi.useFakeTimers();
    const rows = [
      agentPage("fixture-a", { workspaceId: "wks-a" }),
      agentPage("fixture-b", { workspaceId: "wks-b" }),
      agentPage("fixture-c", { workspaceId: "wks-c" }),
    ];
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();
    controls.rpc.mockResolvedValue({
      status: "ready",
      associations: { "fixture-a": ["plan-a"], "fixture-b": ["plan-b", "plan-c"] },
      sources: [{ id: "plan-a" }, { id: "plan-b" }, { id: "plan-c" }],
      error: null,
    });

    const stopPlan = contributePlanControl(controls.client, "sessions");
    const stopReturn = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();

    expect(host.subscribes).toHaveLength(1);
    expect(host.walks).toHaveLength(1);
    expect(controls.pillFor("fixture-a", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-b", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-c", "open-linked-plan")).toBeUndefined();
    expect(controls.pillFor("fixture-a", RETURN_CONTROL_ID)).toBeDefined();
    expect(controls.rpc).toHaveBeenCalledTimes(1);

    controls.pillFor("fixture-a", "open-linked-plan")?.contribution.button.behavior.onPress();
    expect(controls.openScreen).toHaveBeenCalledWith({
      screenId: "sessions",
      params: { tab: "Plan", agentId: "fixture-a", sourceId: "plan-a" },
    });

    await vi.advanceTimersByTimeAsync(15_000);
    expect(controls.rpc).toHaveBeenCalledTimes(2);
    expect(host.walks).toHaveLength(1);
    expect(host.subscribes).toHaveLength(1);

    stopPlan();
    stopReturn();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps the remaining plan pills and both controls when one registration throws", async () => {
    const rows = ["fixture-a", "fixture-b", "fixture-c"].map((id) =>
      agentPage(id, { workspaceId: `wks-${id}` }),
    );
    const host = fakeHost("host-a", rows);
    let failFixtureB = true;
    const controls = fakeControls((contribution) => {
      if (
        failFixtureB &&
        contribution.id === "open-linked-plan" &&
        contribution.agentId === "fixture-b"
      ) {
        throw new Error("registration boom");
      }
    });
    controls.rpc.mockResolvedValue({
      status: "ready",
      associations: {
        "fixture-a": ["plan-a"],
        "fixture-b": ["plan-b"],
        "fixture-c": ["plan-c"],
        "fixture-d": ["plan-d"],
      },
      sources: ["plan-a", "plan-b", "plan-c", "plan-d"].map((id) => ({ id })),
      error: null,
    });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const stopPlan = contributePlanControl(controls.client, "sessions");
    await settle();
    const stopReturn = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();

    expect(controls.pillFor("fixture-a", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-b", "open-linked-plan")).toBeUndefined();
    expect(controls.pillFor("fixture-c", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-a", RETURN_CONTROL_ID)).toBeDefined();
    expect(controls.pillFor("fixture-c", RETURN_CONTROL_ID)).toBeDefined();
    expect(errorSpy).toHaveBeenCalledWith(
      "Kontor plan control could not reconcile a composer pill",
      expect.any(Error),
    );

    failFixtureB = false;
    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        id: "fixture-d",
        workspaceId: "wks-fixture-d",
      },
    } as PaseoAgentUpdate);
    await settle();

    expect(controls.pillFor("fixture-b", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-d", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("fixture-d", RETURN_CONTROL_ID)).toBeDefined();

    stopPlan();
    stopReturn();
  });

  it("keeps later listeners and pending bindings when one listener throws", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const first = vi.fn(() => {
      throw new Error("listener boom");
    });
    const second = vi.fn();
    const releaseFirst = bindHostDirectory(first);
    const releaseSecond = bindHostDirectory(second);
    const rows = [agentPage("agent-a", { workspaceId: "wks-a" })];
    const host = fakeHost("host-a", rows);

    expect(() => startHostDirectory("host-a")).not.toThrow();
    await settle();

    expect(first).toHaveBeenCalled();
    expect(second).toHaveBeenCalled();
    const beforeUpdate = second.mock.calls.length;
    host.emitUpdate({
      kind: "upsert",
      agent: { ...rows[0]!.agent, title: "Renamed" },
    } as PaseoAgentUpdate);
    await settle();
    expect(second.mock.calls.length).toBeGreaterThan(beforeUpdate);
    expect(errorSpy).toHaveBeenCalledTimes(1);
    expect(errorSpy).toHaveBeenCalledWith(
      "Kontor host directory listener failed for host-a",
      expect.any(Error),
    );

    const third = vi.fn();
    const releaseThird = bindHostDirectory(third);
    expect(third).toHaveBeenCalled();

    fakeHost("host-b", [agentPage("b-only", { workspaceId: "wks-b" })]);
    startHostDirectory("host-b");
    await settle();

    const fourth = vi.fn<(state: DirectoryState) => void>();
    const releaseFourth = bindHostDirectory(fourth);
    expect(fourth).toHaveBeenCalled();
    expect(
      fourth.mock.calls[0]![0].entries.map((entry) => entry.id),
    ).not.toContain("b-only");

    releaseFirst();
    releaseSecond();
    releaseThird();
    releaseFourth();
  });

  it("binds the controls to the first HostScreen host and never to a later host", async () => {
    const a = fakeHost("host-a", [agentPage("a-only", { workspaceId: "wks-a" })]);
    const b = fakeHost("host-b", [agentPage("b-only", { workspaceId: "wks-b" })]);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    expect(controls.pillFor("a-only")).toBeDefined();

    startHostDirectory("host-b");
    await settle();

    expect(b.subscribes).toHaveLength(1);
    expect(b.walks).toHaveLength(1);
    expect(controls.pillFor("b-only")).toBeUndefined();
    expect(controls.pillFor("a-only")).toBeDefined();
    expect(a.subscribes).toHaveLength(1);

    stop();
  });

  it("registers one pill for each of 283 sessions and clears every registration once", async () => {
    const rows = Array.from({ length: 283 }, (_, index) =>
      agentPage(`bulk-${index}`, { workspaceId: `wks-${index}` }),
    );
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();

    expect(controls.activePills()).toHaveLength(283);
    expect(host.subscribes).toHaveLength(1);
    expect(host.walks).toHaveLength(2);

    stop();
    expect(controls.activePills()).toHaveLength(0);
  });

  it("releases the subscription and the retained directory exactly once on cleanup", async () => {
    const host = fakeHost("host-a", [agentPage("agent-a", { workspaceId: "wks-a" })]);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    const pill = controls.pillFor("agent-a");
    expect(pill).toBeDefined();

    stop();
    expect(pill?.active).toBe(false);

    await disposeRetainedDirectories();
    expect(host.unsubscribed).toBe(1);
    expect(host.releases).toBe(1);

    await disposeRetainedDirectories();
    expect(host.unsubscribed).toBe(1);
    expect(host.releases).toBe(1);
  });
});
