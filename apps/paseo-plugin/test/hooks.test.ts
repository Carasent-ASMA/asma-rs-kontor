import type {
  PluginBeforeRequests,
  PluginHookContext,
  PluginServerContext,
} from "@getpaseo/plugin/server";
import { describe, expect, it } from "vitest";
import type { KontorPluginConfig } from "../server/config";
import {
  addKontorAgentConfig,
  contribute,
  guardWorkspaceCreate,
  KONTOR_MCP_SERVER_NAME,
  KONTOR_SYSTEM_PROMPT_LINE,
  SEAT_ENV,
  seatEnvironment,
  UNBOUND,
  UNRESOLVED,
} from "../server/hooks";

type AgentCreateRequest = PluginBeforeRequests["agent.create"];
type SessionOpenRequest = PluginBeforeRequests["agent.session_open"];
type WorkspaceCreateRequest = PluginBeforeRequests["workspace.create"];
type Context = Pick<PluginHookContext, "paseo" | "signal">;
type WorkspaceRequestWithCaller = WorkspaceCreateRequest & {
  callerAgentId?: string;
  agent?: { labels?: Record<string, string>; callerAgentId?: string };
};

const CANONICAL = "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor";
const PROJECT = "prj_7409d2c6aff8e21c";

function config(overrides: Partial<KontorPluginConfig> = {}): KontorPluginConfig {
  return {
    stateRoot: "/tmp/kontor-state",
    mcpCommand: "kontor-mcp",
    mcpEnabled: true,
    serveProfile: null,
    placementRoots: [],
    paseoWorktreesRoot: "/Users/test/.paseo/worktrees",
    labelReadTimeoutMs: 20,
    ...overrides,
  };
}

type LabelResult = Record<string, string> | "error" | "never";
type WorkspaceSnapshot = {
  projectId: string | null;
  workspaceDirectory: string | null;
};
type WorkspaceResult = WorkspaceSnapshot | "error" | "never" | null;

function context(
  labels: LabelResult,
  workspaces: Record<string, WorkspaceResult> = {},
): Context {
  const paseo = {
    agents: {
      ref: () => ({
        current: () => null,
        refresh: () => {
          if (labels === "error") {
            return Promise.reject(new Error("daemon unavailable"));
          }
          if (labels === "never") {
            return new Promise(() => {});
          }
          return Promise.resolve({ agent: { labels } });
        },
      }),
    },
    workspaces: {
      ref: (workspaceId: string) => ({
        current: () => null,
        refresh: () => {
          const workspace = workspaces[workspaceId];
          if (workspace === "error") {
            return Promise.reject(new Error("daemon unavailable"));
          }
          if (workspace === "never") {
            return new Promise(() => {});
          }
          return Promise.resolve(workspace ?? null);
        },
      }),
    },
  };
  return {
    paseo,
    signal: new AbortController().signal,
  } as unknown as Context;
}

function seatLabels(overrides: Record<string, string> = {}): Record<string, string> {
  return {
    "kontor.seat_binding_id": "seat-1",
    "kontor.workspace_id": "wks_seat",
    "kontor.worktree": CANONICAL,
    ...overrides,
  };
}

function seatContext(
  labels: LabelResult = seatLabels(),
  workspaces: Record<string, WorkspaceResult> = {
    wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL },
  },
): Context {
  return context(labels, workspaces);
}

function paseoDirectLabels(overrides: Record<string, string> = {}): Record<string, string> {
  return {
    orchestration: "paseo-direct",
    "asma.slot": "SWE",
    ...overrides,
  };
}

function createRequest(overrides: Partial<AgentCreateRequest> = {}): AgentCreateRequest {
  return {
    config: {
      provider: "claude",
      cwd: "/Users/test/carasent/asma-modules",
      mcpServers: {
        existing: { type: "stdio", command: "existing-mcp" },
      },
      systemPrompt: "Base instruction.",
    },
    ...overrides,
  };
}

function openRequest(overrides: Partial<SessionOpenRequest> = {}): SessionOpenRequest {
  return {
    agentId: "agent-1",
    workspaceId: "wks_0000000000000001",
    provider: "claude",
    cwd: "/Users/test/carasent/asma-modules",
    reason: "resume",
    purpose: "interactive",
    env: { EXISTING: "kept" },
    ...overrides,
  };
}

function workspaceRequest(
  overrides: Partial<WorkspaceRequestWithCaller> = {},
): WorkspaceRequestWithCaller {
  return {
    callerAgentId: "agent-1",
    source: {
      kind: "directory",
      path: CANONICAL,
      projectId: PROJECT,
    },
    ...overrides,
  };
}

describe("agent.create hook", () => {
  it("adds the Kontor MCP server at observer authority", () => {
    const request = addKontorAgentConfig(createRequest(), config());
    const server = request.config.mcpServers?.[KONTOR_MCP_SERVER_NAME];

    expect(server).toEqual({
      type: "stdio",
      command: "kontor-mcp",
      args: [
        "--state-root",
        "/tmp/kontor-state",
        "--credential-tier",
        "observer",
      ],
    });
    expect(request.config.mcpServers?.existing).toEqual({
      type: "stdio",
      command: "existing-mcp",
    });
    expect(JSON.stringify(request)).not.toContain("KONTOR_AUTH");
    expect(JSON.stringify(request)).not.toContain("credentials.json");
  });

  it("passes an optional serve profile and the configured command", () => {
    const request = addKontorAgentConfig(
      createRequest(),
      config({ serveProfile: "leadership", mcpCommand: "/opt/kontor-mcp" }),
    );
    expect(request.config.mcpServers?.[KONTOR_MCP_SERVER_NAME]).toMatchObject({
      command: "/opt/kontor-mcp",
      args: [
        "--state-root",
        "/tmp/kontor-state",
        "--credential-tier",
        "observer",
        "--serve-profile",
        "leadership",
      ],
    });
  });

  it("leaves a Kontor-composed seat untouched", () => {
    const request = addKontorAgentConfig(
      createRequest({
        config: {
          provider: "claude",
          cwd: "/tmp",
          mcpServers: {
            kontor: { type: "stdio", command: "seat-kontor-mcp" },
          },
        },
      }),
      config(),
    );
    expect(request.config.mcpServers?.kontor).toEqual({
      type: "stdio",
      command: "seat-kontor-mcp",
    });
    expect(request.config.mcpServers?.[KONTOR_MCP_SERVER_NAME]).toBeUndefined();
  });

  it("adds the context line once", () => {
    const once = addKontorAgentConfig(createRequest(), config());
    const twice = addKontorAgentConfig(once, config());
    expect(once.config.systemPrompt).toBe(`Base instruction.\n${KONTOR_SYSTEM_PROMPT_LINE}`);
    expect(twice.config.systemPrompt).toBe(once.config.systemPrompt);
  });

  it("can omit the MCP server without dropping the context line", () => {
    const request = addKontorAgentConfig(createRequest(), config({ mcpEnabled: false }));
    expect(request.config.mcpServers?.[KONTOR_MCP_SERVER_NAME]).toBeUndefined();
    expect(request.config.systemPrompt).toContain(KONTOR_SYSTEM_PROMPT_LINE);
  });
});

describe("agent.session_open hook", () => {
  it("sets the seat for a labelled seat", async () => {
    const env = await seatEnvironment(
      openRequest(),
      context({
        "kontor.seat_binding_id": "01a00000-0000-7000-8000-000000000001",
      }),
      config(),
    );
    expect(env).toEqual({
      [SEAT_ENV]: "01a00000-0000-7000-8000-000000000001",
    });
  });

  it("reads the delivery role-slot id", async () => {
    const env = await seatEnvironment(
      openRequest(),
      context({ "kontor.role_slot_id": "slot-1" }),
      config(),
    );
    expect(env).toEqual({ [SEAT_ENV]: "slot-1" });
  });

  it("marks an unlabelled session unbound", async () => {
    expect(await seatEnvironment(openRequest(), context({}), config())).toEqual({
      [SEAT_ENV]: UNBOUND,
    });
  });

  it("marks a Paseo-direct seat by its runtime label", async () => {
    expect(
      await seatEnvironment(
        openRequest(),
        context({ orchestration: "paseo-direct", "asma.slot": "SWE" }),
        config(),
      ),
    ).toEqual({ [SEAT_ENV]: UNRESOLVED });
  });

  it("fails open when the labels cannot be read", async () => {
    expect(await seatEnvironment(openRequest(), context("error"), config())).toEqual({
      [SEAT_ENV]: UNRESOLVED,
    });
  });

  it("fails open when the label read exceeds the budget", async () => {
    expect(await seatEnvironment(openRequest(), context("never"), config())).toEqual({
      [SEAT_ENV]: UNRESOLVED,
    });
  });

  it("replaces an old seat", async () => {
    const request = openRequest({
      env: { EXISTING: "kept", [SEAT_ENV]: "old-seat" },
    });
    const env = await seatEnvironment(
      request,
      context({ "kontor.seat_binding_id": "new-seat" }),
      config(),
    );
    expect({ ...request.env, ...env }).toEqual({
      EXISTING: "kept",
      [SEAT_ENV]: "new-seat",
    });
  });
});

describe("workspace.create hook", () => {
  it("refuses a labelled seat creating a Paseo worktree", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        seatContext(),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("refuses a labelled seat creating a directory without an explicit project", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: CANONICAL,
          },
        }),
        seatContext(),
        config(),
      ),
    ).rejects.toThrow("explicit projectId");
  });

  it("refuses a labelled seat creating a workspace in Paseo's worktree root", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: "/Users/test/.paseo/worktrees/0vl4ss0m/asma-7681-authoring-surfaces",
            projectId: "prj_1",
          },
        }),
        seatContext(),
        config(),
      ),
    ).rejects.toThrow("Paseo's own worktree root");
  });

  it("refuses a labelled seat bound to one epic requesting another epic's placement", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: "/Users/test/carasent/asma-modules/.worktrees/asma-other/epic-b",
            projectId: "prj_epic_b",
          },
        }),
        seatContext(),
        config(),
      ),
    ).rejects.toThrow("project outside the seat's binding");
  });

  it("refuses when the seat's placement binding cannot be resolved", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest(),
        seatContext(seatLabels(), {}),
        config(),
      ),
    ).rejects.toThrow("placement binding could not be resolved");
  });

  it("refuses a request whose initial agent carries Kontor labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: undefined,
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: seatLabels(),
          },
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        context({}, { wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL } }),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("refuses a labelled initial agent when the caller is unlabelled", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: "agent-2",
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: seatLabels(),
          },
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        context({}, { wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL } }),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("refuses a labelled initial agent when the caller label read fails", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: "agent-2",
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: seatLabels(),
          },
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        context("error", { wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL } }),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("refuses a labelled caller carried on the initial agent", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: undefined,
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: {},
            callerAgentId: "agent-1",
          },
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        seatContext(),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("allows a labelled seat creating a canonical Kontor placement", async () => {
    await expect(
      guardWorkspaceCreate(workspaceRequest(), seatContext(), config()),
    ).resolves.toBeUndefined();
  });

  it("allows a Paseo-direct-only caller creating an explicit directory placement", async () => {
    await expect(
      guardWorkspaceCreate(workspaceRequest(), context(paseoDirectLabels()), config()),
    ).resolves.toBeUndefined();
  });

  it("allows a Paseo-direct-only initial agent creating an explicit directory placement", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: undefined,
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: paseoDirectLabels(),
          },
        }),
        context({}),
        config(),
      ),
    ).resolves.toBeUndefined();
  });

  it("refuses a Paseo-direct-only seat creating a directory without an explicit project", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: CANONICAL,
          },
        }),
        context(paseoDirectLabels()),
        config(),
      ),
    ).rejects.toThrow("explicit projectId");
  });

  it("refuses a Paseo-direct-only seat creating a Paseo worktree", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: PROJECT,
          },
        }),
        context(paseoDirectLabels()),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("refuses a Paseo-direct-only seat using a relative directory path", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: ".worktrees/asma-8507",
            projectId: PROJECT,
          },
        }),
        context(paseoDirectLabels()),
        config(),
      ),
    ).rejects.toThrow("absolute");
  });

  it("refuses a Paseo-direct-only seat creating a workspace in Paseo's worktree root", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: "/Users/test/.paseo/worktrees/0vl4ss0m/asma-8507",
            projectId: PROJECT,
          },
        }),
        context(paseoDirectLabels()),
        config(),
      ),
    ).rejects.toThrow("Paseo's own worktree root");
  });

  it("refuses a Paseo-direct-only seat outside the configured placement roots", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: "/Users/test/elsewhere/asma-8507",
            projectId: PROJECT,
          },
        }),
        context(paseoDirectLabels()),
        config({ placementRoots: ["/Users/test/carasent/asma-modules"] }),
      ),
    ).rejects.toThrow("placement roots");
  });

  it("still refuses a Kontor seat with an unresolved binding", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest(),
        seatContext(seatLabels(), {}),
        config(),
      ),
    ).rejects.toThrow("placement binding could not be resolved");
  });

  it("enforces the binding for a seat with both Kontor and Paseo-direct labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest(),
        seatContext(seatLabels({ orchestration: "paseo-direct" }), {}),
        config(),
      ),
    ).rejects.toThrow("placement binding could not be resolved");
  });

  it("enforces the bound project for a seat with both Kontor and Paseo-direct labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "directory",
            path: CANONICAL,
            projectId: "prj_epic_b",
          },
        }),
        seatContext(seatLabels({ orchestration: "paseo-direct" })),
        config(),
      ),
    ).rejects.toThrow("project outside the seat's binding");
  });

  it("leaves an interactive request with no agent caller alone", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({ callerAgentId: undefined }),
        seatContext(),
        config(),
      ),
    ).resolves.toBeUndefined();
  });

  it("leaves an unlabelled agent alone", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: { kind: "worktree", cwd: "/tmp", projectId: "prj_1" },
        }),
        context({}),
        config(),
      ),
    ).resolves.toBeUndefined();
  });

  it("fails open when the caller labels cannot be read and the request has no seat labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: { kind: "worktree", cwd: "/tmp", projectId: "prj_1" },
        }),
        context("error"),
        config(),
      ),
    ).resolves.toBeUndefined();
  });
});

describe("contribute", () => {
  it("registers the three decided hooks", () => {
    const registered: string[] = [];
    const settings: string[] = [];
    const server = {
      before: (name: string) => {
        registered.push(name);
        return () => {};
      },
      registerSettings: (definition: { readonly id: string }) => {
        settings.push(definition.id);
        return {
          read: async () => ({ status: "ready", revision: "r1", values: {} }),
          subscribe: () => () => {},
        };
      },
    } as unknown as PluginServerContext;

    const cleanup = contribute(server, config());

    expect(settings).toEqual([]);
    expect(registered).toEqual(["agent.create", "agent.session_open", "workspace.create"]);
    expect(cleanup()).toBeUndefined();
  });

  it("keeps the floor hooks without the observer MCP", async () => {
    const handlers = new Map<string, (payload: unknown) => unknown>();
    const server = {
      before: (name: string, handler: (payload: unknown) => unknown) => {
        handlers.set(name, handler);
        return () => {};
      },
      registerSettings: () => ({
        read: async () => ({ status: "ready", revision: "r1", values: {} }),
        subscribe: () => () => {},
      }),
    } as unknown as PluginServerContext;

    contribute(server, config({ mcpEnabled: false }));

    const createHandler = handlers.get("agent.create");
    expect(createHandler).toBeDefined();
    const request = createRequest();
    const result = (await createHandler?.({
      request,
    })) as AgentCreateRequest;
    expect(result.config.mcpServers).toEqual(request.config.mcpServers);
    expect(result.config.systemPrompt).toContain(KONTOR_SYSTEM_PROMPT_LINE);
  });
});
