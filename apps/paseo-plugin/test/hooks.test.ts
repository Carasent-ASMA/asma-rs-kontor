import type {
  PluginBeforeRequests,
  PluginHookContext,
  PluginServerContext,
} from "@getpaseo/plugin/server";
import { describe, expect, it } from "vitest";
import type { KontorPluginConfig } from "../server/config";
import {
  addKontorAgentConfig,
  BINDING_GENERATION_ENV,
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
  agent?: { labels?: Record<string, string> };
};

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

function context(labels: LabelResult): Context {
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
  };
  return {
    paseo,
    signal: new AbortController().signal,
  } as unknown as Context;
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
      path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
      projectId: "prj_7409d2c6aff8e21c",
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

  it("preserves a Kontor server already composed for the seat", () => {
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
  it("sets the seat and generation for a labelled seat", async () => {
    const env = await seatEnvironment(
      openRequest(),
      context({
        "kontor.seat": "01a00000-0000-7000-8000-000000000001",
        "kontor.binding_generation": "4",
      }),
      config(),
    );
    expect(env).toEqual({
      [SEAT_ENV]: "01a00000-0000-7000-8000-000000000001",
      [BINDING_GENERATION_ENV]: "4",
    });
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
});

describe("workspace.create hook", () => {
  it("refuses a labelled seat creating a Paseo worktree", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: "prj_7409d2c6aff8e21c",
          },
        }),
        context({ "kontor.seat": "seat-1" }),
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
            path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
          },
        }),
        context({ "kontor.agent-run": "run-1" }),
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
        context({ "kontor.seat": "seat-1" }),
        config(),
      ),
    ).rejects.toThrow("Paseo's own worktree root");
  });

  it("refuses a request whose initial agent carries Kontor labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          callerAgentId: undefined,
          agent: {
            config: { provider: "claude", cwd: "/Users/test/carasent/asma-modules" },
            labels: { "kontor.seat": "seat-1" },
          },
          source: {
            kind: "worktree",
            cwd: "/Users/test/carasent/asma-modules",
            projectId: "prj_7409d2c6aff8e21c",
          },
        }),
        context({}),
        config(),
      ),
    ).rejects.toThrow("Kontor workspace guard refused");
  });

  it("allows a labelled seat creating a canonical Kontor placement", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest(),
        context({ "kontor.seat": "seat-1" }),
        config(),
      ),
    ).resolves.toBeUndefined();
  });

  it("leaves an interactive request with no agent caller alone", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({ callerAgentId: undefined }),
        context({ "kontor.seat": "seat-1" }),
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

  it("fails open when the caller labels cannot be read", async () => {
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
    const server = {
      before: (name: string) => {
        registered.push(name);
        return () => {};
      },
    } as unknown as PluginServerContext;

    const cleanup = contribute(server, config());

    expect(registered).toEqual(["agent.create", "agent.session_open", "workspace.create"]);
    expect(cleanup()).toBeUndefined();
  });
});
