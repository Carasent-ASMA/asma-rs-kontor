import type {
  PaseoAgentListOptions,
  PaseoAgentListResult,
} from "@getpaseo/client";
import type { PluginClientContext } from "@getpaseo/plugin/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { hosts } = vi.hoisted(() => ({
  hosts: new Map<string, { client: { agents: { list: unknown } } }>(),
}));

vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  KeyboardAvoidingView: () => null,
  Linking: {},
  Platform: { OS: "web" },
  Pressable: () => null,
  SafeAreaView: () => null,
  ScrollView: () => null,
  Settings: {},
  Text: () => null,
  View: () => null,
}));

vi.mock("@getpaseo/plugin/client", () => ({
  useHosts: () => [],
  getPaseoClient: (hostId: string) => {
    const host = hosts.get(hostId);
    if (!host) {
      throw new Error(`No fixture host for ${hostId}`);
    }
    return host.client;
  },
}));

vi.mock("@getpaseo/plugin/client/ui", () => ({
  SidebarRow: () => null,
}));

vi.mock("@getpaseo/plugin/client/react-native", () => ({
  FlatList: () => null,
  TextInput: () => null,
  useToast: () => ({ show: () => undefined, error: () => undefined }),
}));

import { disposeRetainedDirectories, startHostDirectory } from "../client/use-session-directory";
import contribute from "../index.client";

type AgentPage = {
  readonly agent: {
    readonly id: string;
    readonly title: string;
    readonly status: string;
    readonly provider: string;
    readonly cwd: string;
    readonly workspaceId: string;
    readonly model: null;
    readonly labels: Record<string, string>;
    readonly createdAt: string;
    readonly updatedAt: string;
    readonly archivedAt: null;
    readonly pendingPermissions: readonly unknown[];
    readonly requiresAttention: boolean;
    readonly attentionReason: null;
    readonly attentionTimestamp: null;
  };
  readonly project: null;
};

function agentPage(id: string, workspaceId: string): AgentPage {
  return {
    agent: {
      id,
      title: id,
      status: "idle",
      provider: "fixture",
      cwd: "/fixture",
      workspaceId,
      model: null,
      labels: {},
      createdAt: "2026-10-22T09:00:00.000Z",
      updatedAt: "2026-10-22T10:00:00.000Z",
      archivedAt: null,
      pendingPermissions: [],
      requiresAttention: false,
      attentionReason: null,
      attentionTimestamp: null,
    },
    project: null,
  } as unknown as AgentPage;
}

function fakeHost(hostId: string, rows: readonly AgentPage[]) {
  let unsubscribed = 0;
  let releases = 0;
  const list = async (options: PaseoAgentListOptions = {}) => {
    if (options.subscribe) {
      const limit = options.page?.limit ?? 200;
      const snapshot = {
        requestId: "fixture",
        subscriptionId: "sub-1",
        entries: rows.slice(0, limit),
        pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
      } as unknown as PaseoAgentListResult;
      return {
        ...snapshot,
        subscription: {
          subscriptionId: "sub-1",
          ready: Promise.resolve(snapshot),
          subscribe: (observer: { snapshot: () => void }) => {
            observer.snapshot();
            return () => {
              unsubscribed += 1;
            };
          },
          release: async () => {
            releases += 1;
          },
        },
      };
    }
    return {
      requestId: "fixture",
      entries: rows,
      pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
    };
  };
  const host = {
    client: { agents: { list } },
    get unsubscribed() {
      return unsubscribed;
    },
    get releases() {
      return releases;
    },
  };
  hosts.set(hostId, host);
  return host;
}

describe("Kontor client entry cleanup", () => {
  beforeEach(async () => {
    await disposeRetainedDirectories();
    hosts.clear();
  });

  afterEach(async () => {
    await disposeRetainedDirectories();
    vi.restoreAllMocks();
  });

  it("still releases the directory and runs later cleanups when one control teardown throws", async () => {
    const host = fakeHost("host-a", [agentPage("agent-a", "wks-a")]);
    const cleanups: string[] = [];
    const registrations: Array<{ remove: ReturnType<typeof vi.fn> }> = [];
    const client = {
      addScreen: () => () => cleanups.push("screen"),
      addSidebarHeaderItem: () => () => cleanups.push("sidebar"),
      addCommandCenterItem: () => () => cleanups.push("command"),
      addComposerPill: () => {
        const registration = {
          update: vi.fn(),
          remove: vi.fn(() => {
            throw new Error("remove boom");
          }),
        };
        registrations.push(registration);
        return registration;
      },
      openScreen: vi.fn(),
    } as unknown as PluginClientContext;
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const cleanup = contribute(client);
    startHostDirectory("host-a");
    await vi.waitFor(() => expect(registrations).toHaveLength(1));

    cleanup();

    expect(host.unsubscribed).toBe(1);
    expect(host.releases).toBe(1);
    expect(cleanups.sort()).toEqual(["command", "command", "command", "screen", "sidebar"]);
    expect(errorSpy).toHaveBeenCalledWith(
      "Kontor return control cleanup failed",
      expect.any(Error),
    );
  });
});
