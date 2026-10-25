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
// The popover body is drawn by the host; this suite checks only what the pill hands it.
vi.mock("../client/start-switch-sheet", () => ({ StartSwitchPopover: function StartSwitchPopover() { return null; } }));

import type { ReactElement } from "react";
import { contributePlanControl } from "../client/plan-control";
import { handOffOpen, takeHandOff } from "../client/session-actions";
import { StartSwitchPopover } from "../client/start-switch-sheet";
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
import {
  configureRecentStorage,
  readRecent,
  recordVisit,
} from "../client/recent-sessions";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
} from "../client/session-view-store";

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
      observers.at(-1)?.update({ type: "agent_update", payload: update });
    },
    emitError(error: unknown) {
      observers.at(-1)?.error?.(error);
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
    behavior:
      | { kind: "action"; onPress(): void }
      | { kind: "popover"; Content(props: object): ReactElement<{ screenId: string; openScreen(input: object): void }> };
  };
};

function press(pill: Pill | undefined): void {
  const behavior = pill?.contribution.button.behavior;
  if (behavior?.kind !== "action") throw new Error("not an action pill");
  behavior.onPress();
}

function popover(pill: Pill | undefined) {
  const behavior = pill?.contribution.button.behavior;
  if (behavior?.kind !== "popover") throw new Error("not a popover pill");
  return behavior.Content;
}

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
    clearSessionViews();
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
        behavior: { kind: "popover" },
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

    patchSessionView("host-a", {
      query: "ASMA-8540",
      status: "running",
      shortcut: "Adam",
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });
    configureRecentStorage(null);
    recordVisit("host-a", "s-1", Date.now() - 60_000);
    const beforeReturn = readRecent("host-a");
    const beforeView = readSessionView("host-a");

    // Every pill shares one body, bound to this installation's openScreen and the Kontor screen.
    const Content = popover(controls.pillFor("s-1"));
    expect(popover(controls.pillFor("s-2"))).toBe(Content);
    const body = Content({ host: { id: "host-a", label: "Host A" }, close: vi.fn() });
    expect(body.type).toBe(StartSwitchPopover);
    expect(body.props).toMatchObject({ screenId: "sessions", host: { id: "host-a" } });
    body.props.openScreen({ screenId: "sessions" });
    expect(controls.openScreen).toHaveBeenCalledWith({ screenId: "sessions" });
    expect(readRecent("host-a")).toEqual(beforeReturn);
    expect(readSessionView("host-a")).toEqual(beforeView);

    // A disposed plugin leaves no request for a later Kontor screen to open.
    const key = handOffOpen("host-a", { kind: "session", agentId: "s-1" });
    stop();
    expect(takeHandOff("host-a", key)).toBeNull();
  });

  it("follows native updates and removes closed, archived and removed sessions", async () => {
    const rows = [agentPage("agent-a", { workspaceId: "wks-a" })];
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    const initialPill = controls.pillFor("agent-a");
    expect(initialPill).toBeDefined();

    host.emitUpdate({
      kind: "upsert",
      agent: { ...rows[0]!.agent, workspaceId: "wks-a2" },
    } as PaseoAgentUpdate);
    expect(initialPill?.registration.remove).toHaveBeenCalledTimes(1);
    expect(controls.pillFor("agent-a")?.contribution.workspaceId).toBe("wks-a2");

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[0]!.agent,
        workspaceId: "wks-a2",
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

    press(controls.pillFor("fixture-a", "open-linked-plan"));
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

  it("excludes archived sessions from plan pills even when history is in scope", async () => {
    const rows = [
      agentPage("archived", {
        workspaceId: "wks-archived",
        archivedAt: "2026-10-22T11:00:00.000Z",
      }),
      agentPage("live", { workspaceId: "wks-live" }),
    ];
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();
    controls.rpc.mockResolvedValue({
      status: "ready",
      associations: {
        archived: ["plan-archived"],
        live: ["plan-live"],
      },
      sources: [{ id: "plan-archived" }, { id: "plan-live" }],
      error: null,
    });

    const stopPlan = contributePlanControl(controls.client, "sessions");
    await settle();
    const stopReturn = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a", "online", true);
    await settle();

    expect(controls.pillFor("archived", "open-linked-plan")).toBeUndefined();
    expect(controls.pillFor("live", "open-linked-plan")).toBeDefined();
    expect(controls.pillFor("archived", RETURN_CONTROL_ID)).toBeUndefined();
    expect(controls.pillFor("live", RETURN_CONTROL_ID)).toBeDefined();

    host.emitUpdate({
      kind: "upsert",
      agent: {
        ...rows[1]!.agent,
        archivedAt: "2026-10-22T12:00:00.000Z",
      },
    } as PaseoAgentUpdate);
    await settle();

    expect(controls.pillFor("live", "open-linked-plan")).toBeUndefined();
    expect(controls.pillFor("live", RETURN_CONTROL_ID)).toBeUndefined();

    stopPlan();
    stopReturn();
  });

  it("drops a buffered workspace update when the subscription fails during the held walk", async () => {
    const rows = [agentPage("agent-a", { workspaceId: "wks-a" })];
    const host = fakeHost("host-a", rows);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    expect(controls.pillFor("agent-a")?.contribution.workspaceId).toBe("wks-a");

    startHostDirectory("host-a", "offline");
    const releaseWalk = host.holdWalk();
    startHostDirectory("host-a", "online");
    await settle();
    expect(host.observers).toHaveLength(2);

    host.emitUpdate({
      kind: "upsert",
      agent: { ...rows[0]!.agent, workspaceId: "wks-b" },
    } as PaseoAgentUpdate);
    host.emitError(new Error("stream closed"));
    releaseWalk();
    await settle();

    expect(controls.pillFor("agent-a")?.contribution.workspaceId).toBe("wks-a");
    stop();
  });

  it("lets a buffered remove win over the bootstrap page and the held walk", async () => {
    const rows = [
      agentPage("keep", { workspaceId: "wks-keep" }),
      agentPage("remove", { workspaceId: "wks-remove" }),
    ];
    const host = fakeHost("host-a", rows);
    const releaseWalk = host.holdWalk();
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    expect(controls.pillFor("remove")).toBeDefined();

    host.emitUpdate({ kind: "remove", agentId: "remove" } as PaseoAgentUpdate);
    releaseWalk();
    await settle();

    expect(controls.pillFor("remove")).toBeUndefined();
    expect(controls.pillFor("keep")).toBeDefined();
    stop();
  });

  it("registers pills only for sessions with a usable workspace in a mixed inventory", async () => {
    const rows = [
      agentPage("no-workspace", { workspaceId: null }),
      agentPage("empty-workspace", { workspaceId: "" }),
      agentPage("live", { workspaceId: "wks-live" }),
      agentPage("closed", { workspaceId: "wks-closed", status: "closed" }),
      agentPage("archived", {
        workspaceId: "wks-archived",
        archivedAt: "2026-10-22T11:00:00.000Z",
      }),
    ];
    fakeHost("host-a", rows);
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();

    expect(controls.activePills()).toHaveLength(1);
    expect(controls.pillFor("live")).toBeDefined();
    expect(controls.pillFor("no-workspace")).toBeUndefined();
    expect(controls.pillFor("empty-workspace")).toBeUndefined();
    expect(controls.pillFor("closed")).toBeUndefined();
    expect(controls.pillFor("archived")).toBeUndefined();
    stop();
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
    const observer = host.observers.at(-1)!;
    expect(pill).toBeDefined();

    stop();
    expect(pill?.active).toBe(false);

    await disposeRetainedDirectories();
    expect(host.unsubscribed).toBe(1);
    expect(host.releases).toBe(1);

    const adds = controls.addComposerPill.mock.calls.length;
    observer.snapshot();
    observer.update({
      type: "agent_update",
      payload: {
        kind: "upsert",
        agent: agentPage("late", { workspaceId: "wks-late" }).agent,
      } as PaseoAgentUpdate,
    });
    observer.error?.(new Error("late error"));
    await settle();
    expect(controls.addComposerPill).toHaveBeenCalledTimes(adds);
    expect(controls.activePills()).toHaveLength(0);

    await disposeRetainedDirectories();
    expect(host.unsubscribed).toBe(1);
    expect(host.releases).toBe(1);
  });

  it("leaves no consumable hand-off after cleanup even when removing a pill throws", async () => {
    fakeHost("host-a", [agentPage("native-a", { workspaceId: "wks-a" })]);
    const controls = fakeControls();
    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    const pill = controls.pillFor("native-a");
    expect(pill).toBeDefined();
    pill!.registration.remove.mockImplementation(() => {
      throw new Error("remove boom");
    });

    const key = handOffOpen("host-a", { kind: "session", agentId: "native-a" });
    expect(stop).toThrow("remove boom");
    expect(takeHandOff("host-a", key)).toBeNull();
  });

  it("publishes nothing from a late bootstrap walk after cleanup", async () => {
    const rows = [
      agentPage("agent-a", { workspaceId: "wks-a" }),
      agentPage("agent-b", { workspaceId: "wks-b" }),
    ];
    const host = fakeHost("host-a", rows);
    const releaseWalk = host.holdWalk();
    const controls = fakeControls();

    const stop = contributeReturnControl(controls.client, "sessions");
    startHostDirectory("host-a");
    await settle();
    expect(controls.activePills()).toHaveLength(2);

    const adds = controls.addComposerPill.mock.calls.length;
    stop();
    await disposeRetainedDirectories();
    releaseWalk();
    await settle();

    expect(controls.addComposerPill).toHaveBeenCalledTimes(adds);
    expect(controls.activePills()).toHaveLength(0);
  });
});
