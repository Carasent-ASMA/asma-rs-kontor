import type {
  PaseoAgentListResult,
  PaseoAgentUpdate,
} from "@getpaseo/client";
import type {
  PluginButtonRegistration,
  PluginClientContext,
} from "@getpaseo/plugin/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { collectDirectory } = vi.hoisted(() => ({ collectDirectory: vi.fn() }));

vi.mock("../client/session-directory", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../client/session-directory")>();
  return { ...actual, collectDirectory };
});

import {
  contributeReturnControl,
  RETURN_CONTROL_ICON,
  RETURN_CONTROL_ID,
  RETURN_CONTROL_LABEL,
  RETURN_CONTROL_TITLE,
} from "../client/return-control";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
} from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";

type ReadyAgent = {
  readonly id: string;
  readonly workspaceId: string | null;
  readonly status?: string;
  readonly archivedAt?: string | null;
};

type Observer = {
  snapshot: () => void;
  update: (message: { type: "agent_update"; payload: PaseoAgentUpdate }) => void;
  error?: () => void;
};

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

function agentSnapshot(
  id: string,
  workspaceId: string | null,
  status = "idle",
  archivedAt: string | null = null,
): ReadyAgent {
  return { id, workspaceId, status, archivedAt };
}

function readyAgent(
  id: string,
  workspaceId: string | null,
  status = "idle",
  archivedAt: string | null = null,
): PaseoAgentListResult["entries"][number] {
  return {
    agent: agentSnapshot(id, workspaceId, status, archivedAt),
  } as unknown as PaseoAgentListResult["entries"][number];
}

function fakeClient(
  readyEntries: readonly PaseoAgentListResult["entries"][number][] = [],
  bufferedUpdates: readonly PaseoAgentUpdate[] = [],
  options: { failFirstObservation?: boolean } = {},
) {
  const pills: Pill[] = [];
  const observers: Observer[] = [];
  const release = vi.fn(async () => undefined);
  const unsubscribe = vi.fn();
  let signal: AbortSignal | undefined;
  let observationAttempts = 0;
  const subscription = {
    subscribe(observer: Observer) {
      observers.push(observer);
      observer.snapshot();
      for (const payload of bufferedUpdates) {
        observer.update({ type: "agent_update", payload });
      }
      return unsubscribe;
    },
    release,
  };
  const list = vi.fn(async (listOptions?: { subscribe?: unknown; signal?: AbortSignal }) => {
    if (!listOptions?.subscribe) {
      throw new Error("the return-control test only walks through the mocked collectDirectory");
    }
    signal = listOptions.signal;
    observationAttempts += 1;
    if (options.failFirstObservation && observationAttempts === 1) {
      throw new Error("observation bootstrap unavailable");
    }
    return {
      requestId: "request-subscribe",
      subscriptionId: "sub-1",
      entries: readyEntries,
      pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
      subscription,
    };
  });
  const addComposerPill = vi.fn((contribution: PillContribution) => {
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
  const client = {
    paseo: { agents: { list } },
    addComposerPill,
    openScreen,
  } as unknown as PluginClientContext;
  return {
    client,
    list,
    addComposerPill,
    openScreen,
    pills,
    observers,
    release,
    unsubscribe,
    get signal() {
      return signal;
    },
    get observationAttempts() {
      return observationAttempts;
    },
    pillFor(agentId: string): Pill | undefined {
      return [...pills]
        .reverse()
        .find((pill) => pill.active && pill.contribution.agentId === agentId);
    },
  };
}

function updateAgent(agent: ReadyAgent): PaseoAgentUpdate {
  return {
    kind: "upsert",
    agent,
  } as PaseoAgentUpdate;
}

describe("Kontor return control", () => {
  beforeEach(() => {
    clearSessionViews();
    vi.resetAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("registers a Kontor pill for each live agent and opens the screen on press", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);
    collectDirectory.mockResolvedValueOnce({
      entries: [
        sessionEntry("agent-a", { workspaceId: "wks-a" }),
        sessionEntry("agent-b", { workspaceId: "wks-b", status: "running" }),
      ],
      pages: 1,
    });

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills).toHaveLength(2));

    expect(fake.list).toHaveBeenCalledWith(
      expect.objectContaining({ subscribe: {}, signal: expect.any(AbortSignal) }),
    );
    expect(fake.addComposerPill).toHaveBeenCalledTimes(2);
    expect(fake.pillFor("agent-a")?.contribution).toMatchObject({
      id: RETURN_CONTROL_ID,
      workspaceId: "wks-a",
      agentId: "agent-a",
      button: {
        title: RETURN_CONTROL_TITLE,
        icon: RETURN_CONTROL_ICON,
        label: RETURN_CONTROL_LABEL,
        behavior: { kind: "action" },
      },
    });

    patchSessionView("host-a", {
      query: "ASMA-8540",
      status: "running",
      attentionOnly: true,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });
    fake.pillFor("agent-a")?.contribution.button.behavior.onPress();
    expect(fake.openScreen).toHaveBeenCalledWith({ screenId: "sessions" });
    expect(readSessionView("host-a")).toMatchObject({
      query: "ASMA-8540",
      status: "running",
      attentionOnly: true,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });

    stop();
  });

  it("reconciles snapshots and update lifecycle without duplicate registrations", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);
    collectDirectory
      .mockResolvedValueOnce({
        entries: [
          sessionEntry("agent-a", { workspaceId: "wks-a" }),
          sessionEntry("agent-b", { workspaceId: "wks-b" }),
        ],
        pages: 1,
      })
      .mockResolvedValueOnce({
        entries: [
          sessionEntry("agent-a", { workspaceId: "wks-a" }),
          sessionEntry("agent-c", { workspaceId: "wks-c" }),
        ],
        pages: 1,
      });

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills).toHaveLength(2));

    const pillA = fake.pillFor("agent-a");
    const pillB = fake.pillFor("agent-b");
    expect(pillA).toBeDefined();
    expect(pillB).toBeDefined();

    fake.observers[0]!.snapshot();
    await vi.waitFor(() => expect(pillB!.registration.remove).toHaveBeenCalledTimes(1));
    expect(pillA!.registration.remove).not.toHaveBeenCalled();
    expect(fake.pillFor("agent-c")).toBeDefined();

    fake.observers[0]!.update({
      type: "agent_update",
      payload: { kind: "remove", agentId: "agent-a" },
    });
    expect(pillA!.registration.remove).toHaveBeenCalledTimes(1);

    const pillC = fake.pillFor("agent-c")!;
    fake.observers[0]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-c", "wks-c2")),
    });
    expect(pillC.registration.remove).toHaveBeenCalledTimes(1);
    const pillC2 = fake.pillFor("agent-c");
    expect(pillC2?.contribution.workspaceId).toBe("wks-c2");

    fake.observers[0]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-c", "wks-c2", "idle", "2026-10-18T12:00:00.000Z")),
    });
    expect(pillC2!.registration.remove).toHaveBeenCalledTimes(1);

    fake.observers[0]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-d", "wks-d")),
    });
    const pillD = fake.pillFor("agent-d");
    expect(pillD).toBeDefined();

    fake.observers[0]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-d", "wks-d", "closed")),
    });
    expect(pillD!.registration.remove).toHaveBeenCalledTimes(1);

    stop();
  });

  it("recovers observation after a failed bootstrap and follows later agents", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")], [], {
      failFirstObservation: true,
    });
    collectDirectory.mockResolvedValueOnce({
      entries: [sessionEntry("agent-a", { workspaceId: "wks-a" })],
      pages: 1,
    });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const stop = contributeReturnControl(fake.client, "sessions", { retryDelayMs: 100 });
    await vi.waitFor(() => expect(fake.pillFor("agent-a")).toBeDefined(), { interval: 5 });

    collectDirectory.mockResolvedValue({
      entries: [
        sessionEntry("agent-a", { workspaceId: "wks-a" }),
        sessionEntry("agent-b", { workspaceId: "wks-b" }),
      ],
      pages: 1,
    });
    await vi.waitFor(() => expect(fake.observers).toHaveLength(1), { interval: 5 });

    expect(fake.observationAttempts).toBe(2);
    expect(errorSpy).toHaveBeenCalledWith(
      "Kontor return control could not observe agents",
      expect.any(Error),
    );
    await vi.waitFor(() => expect(fake.pillFor("agent-b")).toBeDefined());
    expect(fake.observers).toHaveLength(1);

    fake.observers[0]!.update({
      type: "agent_update",
      payload: { kind: "remove", agentId: "agent-b" },
    });
    await vi.waitFor(() => expect(fake.pillFor("agent-b")).toBeUndefined());

    expect(fake.observers).toHaveLength(1);
    stop();
  });

  it("re-establishes observation after an owned subscription error", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);
    collectDirectory.mockResolvedValue({
      entries: [sessionEntry("agent-a", { workspaceId: "wks-a" })],
      pages: 1,
    });

    const stop = contributeReturnControl(fake.client, "sessions", { retryDelayMs: 100 });
    await vi.waitFor(() => expect(fake.observers).toHaveLength(1), { interval: 5 });

    fake.observers[0]!.error?.();
    await vi.waitFor(() => expect(fake.observers).toHaveLength(2), { interval: 5 });
    expect(fake.release).toHaveBeenCalledTimes(1);

    fake.observers[1]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-b", "wks-b")),
    });
    await vi.waitFor(() => expect(fake.pillFor("agent-b")).toBeDefined());

    stop();
  });

  it("retries a failed directory walk while the subscription is healthy", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);
    collectDirectory
      .mockRejectedValueOnce(new Error("transient walk failure"))
      .mockResolvedValueOnce({
        entries: [
          sessionEntry("agent-a", { workspaceId: "wks-a" }),
          sessionEntry("agent-b", { workspaceId: "wks-b" }),
        ],
        pages: 1,
      });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const stop = contributeReturnControl(fake.client, "sessions", { retryDelayMs: 20 });
    await vi.waitFor(() => expect(fake.observers).toHaveLength(1), { interval: 5 });
    await vi.waitFor(() => expect(fake.pillFor("agent-b")).toBeDefined(), { interval: 5 });

    expect(collectDirectory).toHaveBeenCalledTimes(2);
    expect(errorSpy).toHaveBeenCalledWith(
      "Kontor return control could not walk the agent directory",
      expect.any(Error),
    );

    stop();
  });

  it("does not retry observation after cleanup", async () => {
    const fake = fakeClient([], [], { failFirstObservation: true });
    collectDirectory.mockResolvedValue({ entries: [], pages: 1 });
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const stop = contributeReturnControl(fake.client, "sessions", { retryDelayMs: 100 });
    await vi.waitFor(() => expect(errorSpy).toHaveBeenCalled(), { interval: 5 });
    stop();

    await new Promise((resolve) => setTimeout(resolve, 150));

    expect(fake.observationAttempts).toBe(1);
    expect(fake.observers).toHaveLength(0);
    expect(fake.signal?.aborted).toBe(true);
  });

  it("cleans every registration and ignores late work after cleanup", async () => {
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);
    collectDirectory.mockResolvedValue({
      entries: [sessionEntry("agent-a", { workspaceId: "wks-a" })],
      pages: 1,
    });

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills).toHaveLength(1));
    const pillA = fake.pillFor("agent-a")!;

    stop();

    expect(pillA.registration.remove).toHaveBeenCalledTimes(1);
    expect(fake.unsubscribe).toHaveBeenCalledTimes(1);
    expect(fake.release).toHaveBeenCalledTimes(1);
    expect(fake.signal?.aborted).toBe(true);

    fake.observers[0]!.snapshot();
    fake.observers[0]!.update({
      type: "agent_update",
      payload: updateAgent(agentSnapshot("agent-b", "wks-b")),
    });
    await Promise.resolve();
    expect(fake.addComposerPill).toHaveBeenCalledTimes(1);
  });

  it("does not publish a late bootstrap walk after cleanup", async () => {
    let resolveWalk!: (value: { entries: ReturnType<typeof sessionEntry>[]; pages: number }) => void;
    collectDirectory.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveWalk = resolve;
      }),
    );
    const fake = fakeClient([readyAgent("agent-a", "wks-a")]);

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills).toHaveLength(1));
    stop();

    resolveWalk({
      entries: [sessionEntry("agent-b", { workspaceId: "wks-b" })],
      pages: 1,
    });
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(fake.addComposerPill).toHaveBeenCalledTimes(1);
    expect(fake.pills).toHaveLength(1);
  });

  it("lets a buffered remove win over the ready bootstrap page", async () => {
    const fake = fakeClient(
      [readyAgent("agent-a", "wks-a")],
      [{ kind: "remove", agentId: "agent-a" }],
    );
    collectDirectory.mockResolvedValueOnce({
      entries: [sessionEntry("agent-b", { workspaceId: "wks-b" })],
      pages: 1,
    });

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills.some((pill) => pill.contribution.agentId === "agent-b")).toBe(true));

    const removed = fake.pills.find((pill) => pill.contribution.agentId === "agent-a");
    expect(removed?.registration.remove).toHaveBeenCalledTimes(1);
    expect(fake.pillFor("agent-a")).toBeUndefined();

    stop();
  });

  it("skips ad-hoc sessions without a workspace, closed sessions and archived sessions", async () => {
    const fake = fakeClient();
    collectDirectory.mockResolvedValueOnce({
      entries: [
        sessionEntry("no-workspace", { workspaceId: null }),
        sessionEntry("closed", { status: "closed" }),
        sessionEntry("archived", {
          workspaceId: "wks-archived",
          archivedAt: "2026-10-18T12:00:00.000Z",
        }),
        sessionEntry("live", { workspaceId: "wks-live" }),
      ],
      pages: 1,
    });

    const stop = contributeReturnControl(fake.client, "sessions");
    await vi.waitFor(() => expect(fake.pills).toHaveLength(1));

    expect(fake.pillFor("no-workspace")).toBeUndefined();
    expect(fake.pillFor("closed")).toBeUndefined();
    expect(fake.pillFor("archived")).toBeUndefined();
    expect(fake.pillFor("live")?.contribution.workspaceId).toBe("wks-live");

    stop();
  });
});
