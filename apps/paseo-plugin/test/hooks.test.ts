import type {
  PluginBeforeRequests,
  PluginHookContext,
  PluginServerContext,
} from "@getpaseo/plugin/server";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { KontorPluginConfig } from "../server/config";
import {
  addKontorAgentConfig,
  contribute,
  contributePlacement,
  detectPlacement,
  guardWorkspaceCreate,
  PLACEMENT_DETECTED_EVENT,
  PLACEMENT_MODE_EVENT,
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
    cliCommand: "kontor",
    paseoCommand: "paseo",
    paseoHome: null,
    paseoHost: null,
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
  /** Overrides the id the daemon reads back; defaults to the requested id. */
  id?: string;
  projectId: string | null;
  workspaceDirectory: string | null;
  projectDisplayName?: string;
  projectCustomName?: string | null;
  projectKind?: string;
  projectRootPath?: string;
  archivingAt?: string | null;
};
type WorkspaceResult = WorkspaceSnapshot | "error" | "never" | null;

/** The caller's own workspace by default: a plain directory workspace in the seat's epic project. */
const CALLER_WORKSPACE: WorkspaceSnapshot = {
  projectId: PROJECT,
  workspaceDirectory: CANONICAL,
  projectDisplayName: "ESW • ASMA-8507",
};

/**
 * A daemon whose agents read back with their own id, labels and workspace. A
 * caller runs in `kontor.workspace_id` when its labels name one, else in
 * `callerWorkspace` (`wks_caller` by default).
 */
function context(
  labels: LabelResult,
  workspaces: Record<string, WorkspaceResult> = {},
  options: { callerWorkspace?: string; readsAs?: string; projects?: { projectId: string; projectDisplayName: string; projectCustomName?: string | null }[] | "error" } = {},
): Context {
  const all: Record<string, WorkspaceResult> = { wks_caller: CALLER_WORKSPACE, ...workspaces };
  const paseo = {
    agents: {
      ref: (agentId: string) => ({
        current: () => null,
        refresh: () => {
          if (labels === "error") {
            return Promise.reject(new Error("daemon unavailable"));
          }
          if (labels === "never") {
            return new Promise(() => {});
          }
          const workspaceId = options.callerWorkspace ?? labels["kontor.workspace_id"] ?? "wks_caller";
          return Promise.resolve({ agent: { id: options.readsAs ?? agentId, labels, workspaceId } });
        },
      }),
    },
    workspaces: {
      ref: (workspaceId: string) => ({
        current: () => null,
        refresh: () => {
          const workspace = all[workspaceId];
          if (workspace === "error") {
            return Promise.reject(new Error("daemon unavailable"));
          }
          if (workspace === "never") {
            return new Promise(() => {});
          }
          return Promise.resolve(workspace ? { id: workspaceId, ...workspace } : null);
        },
      }),
    },
    projects: {
      list: () =>
        options.projects === "error"
          ? Promise.reject(new Error("daemon unavailable"))
          : Promise.resolve({ projects: options.projects ?? [] }),
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
        seatContext(seatLabels(), { wks_seat: { projectId: PROJECT, workspaceDirectory: "/Users/test/elsewhere", projectDisplayName: "ESW • ASMA-8507" } }),
        config(),
      ),
    ).rejects.toThrow("placement binding could not be resolved");
  });

  describe("a Kontor binding holds only with a native checkout readback that matches its worktree label (AUD-B-01)", () => {
    const readsBack = (workspaceDirectory?: string | null): Record<string, WorkspaceResult> => ({
      wks_seat: {
        projectId: PROJECT,
        projectDisplayName: "ESW • ASMA-8507",
        ...(workspaceDirectory === undefined ? {} : { workspaceDirectory }),
      } as WorkspaceSnapshot,
    });
    const routes = [
      ["a bound caller", (workspaces: Record<string, WorkspaceResult>) => guardWorkspaceCreate(workspaceRequest(), seatContext(seatLabels(), workspaces), config())],
      [
        "a bound initial agent with no caller",
        (workspaces: Record<string, WorkspaceResult>) =>
          guardWorkspaceCreate(
            workspaceRequest({ callerAgentId: undefined, agent: { config: { provider: "claude", cwd: CANONICAL }, labels: seatLabels() } }),
            context({}, workspaces),
            config(),
          ),
      ],
    ] as const;
    for (const [route, create] of routes) {
      it(`${route}: a workspace that reads back with no checkout directory resolves no binding`, async () => {
        for (const missing of [undefined, null, "", "  "]) {
          await expect(create(readsBack(missing))).rejects.toThrow("placement binding could not be resolved");
        }
      });

      it(`${route}: a matching checkout is allowed and a different one refused`, async () => {
        await expect(create(readsBack(CANONICAL))).resolves.toBeUndefined();
        await expect(create(readsBack(`${CANONICAL}/`))).resolves.toBeUndefined();
        await expect(create(readsBack("/Users/test/elsewhere"))).rejects.toThrow("placement binding could not be resolved");
      });
    }
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

  describe("a guarded workflow creation needs qualified evidence of its target checkout (B-5, AUD-B-R9-01)", () => {
    const SIBLING = "/Users/test/carasent/asma-modules/.worktrees/asma-8508-review/asma-rs-kontor";
    const NO_EVIDENCE = "no qualified evidence names the target checkout";
    const roots = () => config({ placementRoots: ["/Users/test/carasent/asma-modules"] });
    /** The guard as the host runs it: the creation (one synthetic dispatch) follows only when the guard returns. */
    async function run(request: WorkspaceRequestWithCaller, ctx: Context, cfg = roots()) {
      const raw = JSON.stringify(request);
      let dispatched = 0;
      let error = "";
      try {
        await guardWorkspaceCreate(request, ctx, cfg);
        dispatched++;
      } catch (cause) {
        error = String(cause);
      }
      return { dispatched, error, unchanged: JSON.stringify(request) === raw };
    }

    // AUD's four witnesses (b5-witnesses): each is in the right project, and none carries evidence naming its target.
    it.each<[string, Record<string, string>, WorkspaceSnapshot, boolean]>([
      ["a Paseo-direct caller asking for a sibling review checkout", paseoDirectLabels(), CALLER_WORKSPACE, false],
      ["an unbound caller whose initial agent claims workflow work", {}, { ...CALLER_WORKSPACE, projectDisplayName: "generic repository" }, true],
      ["a Paseo-direct caller whose workspace reads back with no checkout", paseoDirectLabels(), { ...CALLER_WORKSPACE, workspaceDirectory: null }, false],
      ["a Paseo-direct caller whose workspace reads back with a blank checkout", paseoDirectLabels(), { ...CALLER_WORKSPACE, workspaceDirectory: "" }, false],
    ])("refuses %s before the creation, the request and the caller's workspace unchanged", async (_name, labels, workspace, initial) => {
      const snapshot = JSON.stringify(workspace);
      const request = workspaceRequest({
        source: { kind: "directory", path: SIBLING, projectId: PROJECT },
        ...(initial ? { agent: { config: { provider: "codex", cwd: SIBLING }, labels: paseoDirectLabels() } } : {}),
      } as never);
      const result = await run(request, context(labels, { wks_caller: workspace }));
      expect(result.dispatched).toBe(0);
      expect(result.unchanged).toBe(true);
      expect(JSON.stringify(workspace)).toBe(snapshot);
      expect(result.error).toContain(NO_EVIDENCE);
    });

    it("refuses an unbound workflow caller's own checkout too: the caller's directory is not evidence of the new workspace's target", async () => {
      for (const labels of [paseoDirectLabels(), {}]) {
        const result = await run(workspaceRequest(), context(labels));
        expect(result.dispatched).toBe(0);
        expect(result.error).toContain(NO_EVIDENCE);
      }
    });

    it("refuses when a bound caller's target evidence is missing or unreadable, never falling back to its project", async () => {
      // The seat runs in the workspace its binding names, so a failed read refuses at the caller; a readback without
      // its checkout refuses at the binding.
      const cases: [WorkspaceResult, string][] = [
        [{ projectId: PROJECT, workspaceDirectory: null }, "placement binding could not be resolved"],
        [{ projectId: PROJECT, workspaceDirectory: "" }, "placement binding could not be resolved"],
        ["error", "could not be read"],
        ["never", "could not be read"],
      ];
      for (const [workspace, reason] of cases) {
        const result = await run(workspaceRequest(), seatContext(seatLabels(), { wks_seat: workspace }));
        expect(result.dispatched).toBe(0);
        expect(result.error).toContain(reason);
      }
    });

    it("keeps a qualified binding's exact checkout allowed and refuses its sibling (AUD B5-qualified-control)", async () => {
      expect(await run(workspaceRequest(), seatContext(), config())).toMatchObject({ dispatched: 1, error: "", unchanged: true });
      const sibling = await run(workspaceRequest({ source: { kind: "directory", path: SIBLING, projectId: PROJECT } }), seatContext(), config());
      expect(sibling.dispatched).toBe(0);
      expect(sibling.error).toContain("directory outside the seat's canonical worktree");
      // The initial-agent route: a caller-less request whose initial agent carries a binding reaches exactly its checkout.
      const bound = { config: { provider: "claude", cwd: "/tmp" }, labels: seatLabels() };
      const seat = context({}, { wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL } });
      expect(await run(workspaceRequest({ callerAgentId: undefined, agent: bound } as never), seat, config())).toMatchObject({ dispatched: 1, error: "" });
    });

    it("keeps a genuine operator creation allowed: no caller and no workflow claim (AUD B5-operator-control)", async () => {
      const request = { source: { kind: "directory", path: SIBLING, projectId: PROJECT } } as never;
      expect(await run(request, context("error"), config())).toMatchObject({ dispatched: 1, error: "", unchanged: true });
    });
  });

  it("refuses a Paseo-direct-only initial agent with no caller (missing caller)", async () => {
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
    ).rejects.toThrow("must name its calling agent");
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
    ).rejects.toThrow("worktree creation is outside it");
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
        seatContext(seatLabels(), { wks_seat: { projectId: PROJECT, workspaceDirectory: "/Users/test/elsewhere", projectDisplayName: "ESW • ASMA-8507" } }),
        config(),
      ),
    ).rejects.toThrow("placement binding could not be resolved");
  });

  it("enforces the binding for a seat with both Kontor and Paseo-direct labels", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest(),
        seatContext(seatLabels({ orchestration: "paseo-direct" }), { wks_seat: { projectId: PROJECT, workspaceDirectory: "/Users/test/elsewhere", projectDisplayName: "ESW • ASMA-8507" } }),
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

  it("leaves an unlabelled agent in a project that is neither a desk nor an epic unguarded (unclassified)", async () => {
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({
          source: { kind: "worktree", cwd: "/tmp", projectId: "prj_1" },
        }),
        context({}, { wks_caller: { projectId: "prj_repo", workspaceDirectory: "/tmp/repo", projectDisplayName: "asma-modules" } }),
        config(),
      ),
    ).resolves.toBeUndefined();
    // A project that only carries a key in its name is neither a desk nor an epic project.
    for (const projectDisplayName of ["TICKETS • ASMA-8507", "asma-rs-kontor • ASMA-8507"]) {
      await expect(
        guardWorkspaceCreate(
          workspaceRequest({ source: { kind: "directory", path: "/tmp/elsewhere", projectId: "prj_1" } }),
          context({}, { wks_caller: { projectId: "prj_lookalike", workspaceDirectory: "/tmp/repo", projectDisplayName } }),
          config(),
        ),
      ).resolves.toBeUndefined();
    }
  });

  it("refuses when the caller cannot be read back, whatever its labels", async () => {
    for (const ctx of [
      context("error"),
      context("never"),
      context({}, { wks_caller: null }),
      context({}, { wks_caller: "error" }),
      context({}, { wks_caller: { ...CALLER_WORKSPACE, archivingAt: "2026-11-25T00:00:00Z" } }),
      context({}, {}, { callerWorkspace: "" }),
    ]) {
      await expect(
        guardWorkspaceCreate(workspaceRequest({ source: { kind: "worktree", cwd: "/tmp", projectId: "prj_1" } }), ctx, config()),
      ).rejects.toThrow("calling agent's own workspace and project could not be read");
    }
  });
});

describe("workspace.create hook: caller identity and scope (ASMA-8580)", () => {
  const OTHER = { kind: "directory", path: "/Users/test/carasent/asma-modules/.worktrees/asma-other/x", projectId: "prj_other" } as const;
  const DESK = { projectId: "prj_desk", workspaceDirectory: "/Users/test/.local/share/paseo/desk-projects/ADAM", projectDisplayName: "DESK • ADAM" };
  const ESW = [
    { projectId: "prj_esw_8507", projectDisplayName: "ESW • ASMA-8507" },
    { projectId: "prj_esw_9000", projectDisplayName: "ESW • ASMA-9000" },
  ];

  it("(a) holds a caller with its labels removed to its own epic project", async () => {
    await expect(guardWorkspaceCreate(workspaceRequest({ source: OTHER }), context({}), config())).rejects.toThrow(
      "project outside the seat's binding",
    );
    // In its own project it still names no evidence of its target checkout, so it is refused there too (B-5).
    await expect(guardWorkspaceCreate(workspaceRequest(), context({}), config())).rejects.toThrow("no qualified evidence names the target checkout");
  });

  it("(a) holds an unlabelled desk lead to its desk project", async () => {
    const ctx = context({}, { wks_caller: DESK }, { projects: ESW });
    await expect(guardWorkspaceCreate(workspaceRequest({ source: { ...OTHER, projectId: "prj_esw_8507" } }), ctx, config())).rejects.toThrow(
      "project outside",
    );
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { kind: "directory", path: DESK.workspaceDirectory, projectId: "prj_desk" } }), ctx, config()),
    ).rejects.toThrow("no qualified evidence names the target checkout");
  });

  it("(b) refuses a workflow caller that names no target project or uses a worktree source", async () => {
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { kind: "directory", path: CANONICAL } }), context({}), config()),
    ).rejects.toThrow("explicit projectId");
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { kind: "worktree", cwd: CANONICAL } }), context({}), config()),
    ).rejects.toThrow("worktree creation is outside it");
    await expect(guardWorkspaceCreate(workspaceRequest({ source: undefined }), context({}), config())).rejects.toThrow(
      "no workspace source",
    );
  });

  it("(c) keeps genuine operator creation working, and cannot tell a caller-less automated request from it (LSA-B-02 residual)", async () => {
    // An operator in the app sends no caller and no initial agent; an SDK script with no caller sends the same,
    // or an initial agent with no workflow claim. Both are unclassified (provenance unknown) and left unguarded.
    const free: Record<string, string>[] = [{}, { "kontor.summary.source": "a", "kontor.summary.job": "j", workflow: "free" }];
    for (const agent of [undefined, ...free.map((labels) => ({ config: { provider: "claude", cwd: "/tmp" }, labels }))]) {
      await expect(
        guardWorkspaceCreate(workspaceRequest({ callerAgentId: undefined, agent, source: { kind: "worktree", cwd: "/tmp" } }), context("error"), config()),
      ).resolves.toBeUndefined();
    }
  });

  it("(d) gives a mislabelled caller no authority from its labels", async () => {
    // A Kontor binding naming another seat's workspace is refused before it is read, even when that binding would resolve.
    const bindingElsewhere = context(
      seatLabels({ "kontor.worktree": OTHER.path }),
      { wks_seat: { projectId: "prj_other", workspaceDirectory: OTHER.path } },
      { callerWorkspace: "wks_caller" },
    );
    await expect(guardWorkspaceCreate(workspaceRequest({ source: OTHER }), bindingElsewhere, config())).rejects.toThrow(
      "names a workspace the caller does not run in",
    );
    // An epic label on an epic-project caller selects nothing.
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { ...OTHER, projectId: "prj_esw_9000" } }), context(paseoDirectLabels({ epic: "ASMA-9000" }), {}, { projects: ESW }), config()),
    ).rejects.toThrow("project outside");
    // A desk-like project label on an operator agent does not make it a desk lead.
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({ source: { ...OTHER, projectId: "prj_esw_9000" } }),
        context({ orchestration: "paseo", epic: "ASMA-9000", project: "DESK • ADAM" }, { wks_caller: { projectId: "prj_repo", workspaceDirectory: "/tmp/repo", projectDisplayName: "asma-modules" } }, { projects: ESW }),
        config(),
      ),
    ).rejects.toThrow("project outside");
  });

  it("never widens a desk caller's scope from its labels: an epic label, its key or a project name is not an assignment (LSA-B-01)", async () => {
    // The same authoritative desk caller and binding, whatever epic label it carries, gains no epic project.
    const desk = (labels: Record<string, string>) => context(labels, { wks_caller: DESK }, { projects: ESW });
    const into = (projectId: string) => workspaceRequest({ source: { ...OTHER, projectId } });
    const variants: Record<string, string>[] = [
      { role: "TPM", epic: "ASMA-8507" },
      { role: "TPM", epic: "ASMA-9000" },
      { role: "LSA", epic: "ASMA-8507", task: "ASMA-8507", orchestration: "paseo-direct" },
      // A wrong-role caller in the desk that copies a lead's labels.
      { role: "SWE", epic: "ASMA-8507", orchestration: "paseo-direct", workflow: "kontor" },
      {},
    ];
    for (const labels of variants) {
      for (const projectId of ["prj_esw_8507", "prj_esw_9000"]) {
        await expect(guardWorkspaceCreate(into(projectId), desk(labels), config())).rejects.toThrow("project outside");
      }
      await expect(
        guardWorkspaceCreate(workspaceRequest({ source: { kind: "directory", path: DESK.workspaceDirectory, projectId: "prj_desk" } }), desk(labels), config()),
      ).rejects.toThrow("no qualified evidence names the target checkout");
    }
  });

  it("leaves a known workflow seat in a misplaced legacy project unguarded once its labels are removed (LSA-B-02 residual)", async () => {
    // A review seat in a project rooted at its own review worktree: neither a desk nor an epic project.
    const legacy = { projectId: "prj_legacy", workspaceDirectory: "/Users/test/.cache/asma-8507-review-a/asma-rs-kontor", projectDisplayName: "asma-rs-kontor" };
    const elsewhere = workspaceRequest({ source: { ...OTHER, projectId: "prj_esw_9000" } });
    const sameLegacyProject = workspaceRequest({ source: { kind: "directory", path: legacy.workspaceDirectory, projectId: "prj_legacy" } });
    // While its labels remain it is held to its own (misplaced) project, and refused there too without evidence of its
    // target (B-5); the guard cannot move it to its intended epic project.
    await expect(guardWorkspaceCreate(elsewhere, context(paseoDirectLabels({ epic: "ASMA-8507" }), { wks_caller: legacy }), config())).rejects.toThrow("project outside");
    await expect(guardWorkspaceCreate(sameLegacyProject, context(paseoDirectLabels({ epic: "ASMA-8507" }), { wks_caller: legacy }), config())).rejects.toThrow("no qualified evidence names the target checkout");
    // With its labels removed nothing on the host distinguishes it from an unmarked agent: unsupported, not prevented.
    await expect(guardWorkspaceCreate(elsewhere, context({}, { wks_caller: legacy }), config())).resolves.toBeUndefined();
    await expect(
      guardWorkspaceCreate(elsewhere, context({ epic: "", task: "", orchestration: "", workflow: "" }, { wks_caller: legacy }), config()),
    ).resolves.toBeUndefined();
  });

  it("reads a renamed desk by its custom name", async () => {
    const renamedDesk = { ...DESK, projectDisplayName: "asma-modules", projectCustomName: "DESK • ADAM" };
    await expect(guardWorkspaceCreate(workspaceRequest({ source: OTHER }), context({}, { wks_caller: renamedDesk }), config())).rejects.toThrow(
      "project outside",
    );
  });

  it("refuses a caller readback that names another agent, another workspace or an archiving one", async () => {
    // A caller that reads back reaches the placement check (refused there only for want of target evidence); each of
    // these is refused at the readback instead.
    await expect(guardWorkspaceCreate(workspaceRequest(), context({}), config())).rejects.toThrow("no qualified evidence names the target checkout");
    await expect(guardWorkspaceCreate(workspaceRequest(), context({}, {}, { readsAs: "agent-other" }), config())).rejects.toThrow(
      "could not be read",
    );
    await expect(
      guardWorkspaceCreate(workspaceRequest(), context({}, { wks_caller: { ...CALLER_WORKSPACE, id: "wks_other" } }), config()),
    ).rejects.toThrow("could not be read");
    await expect(
      guardWorkspaceCreate(workspaceRequest(), context({}, { wks_caller: { ...CALLER_WORKSPACE, archivingAt: "2026-11-25T00:00:00Z" } }), config()),
    ).rejects.toThrow("could not be read");
  });

  it("holds a Kontor-bound caller and a bound initial agent to the canonical directory, not only the project", async () => {
    const sibling = { kind: "directory", path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/other", projectId: PROJECT } as const;
    await expect(guardWorkspaceCreate(workspaceRequest({ source: sibling }), seatContext(), config())).rejects.toThrow(
      "directory outside the seat's canonical worktree",
    );
    const boundAgent = { config: { provider: "claude", cwd: "/tmp" }, labels: seatLabels() };
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({ callerAgentId: undefined, agent: boundAgent, source: sibling } as never),
        context({}, { wks_seat: { projectId: PROJECT, workspaceDirectory: CANONICAL } }),
        config(),
      ),
    ).rejects.toThrow("directory outside the seat's canonical worktree");
  });

  it("enforces every current and retained workflow marker on an operator-placed caller", async () => {
    const operatorPlaced = { wks_caller: { projectId: "prj_repo", workspaceDirectory: "/tmp/repo", projectDisplayName: "asma-modules" } };
    const marked: Record<string, string>[] = [
      { orchestration: "paseo" },
      { orchestration: "kontor" },
      { orchestration: "paseo-direct" },
      { orchestration: "kontor-governed" },
      { workflow: "kontor" },
      { "kontor.agent-run": "run-1", "kontor.workspace_id": "wks_caller", "kontor.worktree": "/tmp/repo" },
    ];
    for (const labels of marked) {
      await expect(guardWorkspaceCreate(workspaceRequest({ source: OTHER }), context(labels, operatorPlaced), config())).rejects.toThrow(
        "Kontor workspace guard refused",
      );
    }
    const unmarked: Record<string, string>[] = [{ workflow: "free" }, { "kontor.summary.source": "a" }, { orchestration: "none" }];
    for (const labels of unmarked) {
      await expect(guardWorkspaceCreate(workspaceRequest({ source: OTHER }), context(labels, operatorPlaced), config())).resolves.toBeUndefined();
    }
  });

  it("scopes a caller by its initial agent's workflow claim", async () => {
    const operatorPlaced = { wks_caller: { projectId: "prj_repo", workspaceDirectory: "/tmp/repo", projectDisplayName: "asma-modules" } };
    await expect(
      guardWorkspaceCreate(
        workspaceRequest({ source: OTHER, agent: { config: { provider: "claude", cwd: "/tmp" }, labels: { orchestration: "paseo", workflow: "kontor" } } }),
        context({}, operatorPlaced),
        config(),
      ),
    ).rejects.toThrow("project outside");
  });

  it("takes the scope of a borrowed caller id: the caller is client-reported, so another live agent's id gets that agent's scope (residual)", async () => {
    // Agent A runs in ESW • ASMA-8507 and agent B in ESW • ASMA-9000, each bound to its own checkout. The 0.17.4 CLI sends
    // whatever PASEO_AGENT_ID holds, and the daemon only checks that the agent exists; the hook can read only the agent
    // the id names.
    const B_CHECKOUT = "/Users/test/epics/ASMA-9000/task-b";
    const seats: Record<string, Record<string, string>> = {
      "agent-a": seatLabels({ "kontor.workspace_id": "wks_a", "kontor.worktree": "/Users/test/epics/ASMA-8507/task-a" }),
      "agent-b": seatLabels({ "kontor.workspace_id": "wks_b", "kontor.worktree": B_CHECKOUT }),
      "agent-c": paseoDirectLabels(),
    };
    const base = context({}, {
      wks_a: { projectId: "prj_esw_8507", workspaceDirectory: "/Users/test/epics/ASMA-8507/task-a", projectDisplayName: "ESW • ASMA-8507" },
      wks_b: { projectId: "prj_esw_9000", workspaceDirectory: B_CHECKOUT, projectDisplayName: "ESW • ASMA-9000" },
      wks_c: { projectId: "prj_esw_9000", workspaceDirectory: "/Users/test/epics/ASMA-9000/task-c", projectDisplayName: "ESW • ASMA-9000" },
    });
    const runsIn: Record<string, string> = { "agent-a": "wks_a", "agent-b": "wks_b", "agent-c": "wks_c" };
    const daemon = {
      ...base,
      paseo: {
        ...(base.paseo as object),
        agents: { ref: (agentId: string) => ({ current: () => null, refresh: () => Promise.resolve({ agent: { id: agentId, labels: seats[agentId], workspaceId: runsIn[agentId] } }) }) },
      },
    } as unknown as Context;
    const intoB = { kind: "directory", path: B_CHECKOUT, projectId: "prj_esw_9000" } as const;
    await expect(guardWorkspaceCreate(workspaceRequest({ callerAgentId: "agent-a", source: intoB }), daemon, config())).rejects.toThrow("project outside");
    await expect(guardWorkspaceCreate(workspaceRequest({ callerAgentId: "agent-b", source: intoB }), daemon, config())).resolves.toBeUndefined();
    // An unbound agent's id carries no target evidence, so borrowing it admits nothing (B-5).
    await expect(guardWorkspaceCreate(workspaceRequest({ callerAgentId: "agent-c", source: intoB }), daemon, config())).rejects.toThrow(
      "no qualified evidence names the target checkout",
    );
  });

  it("refuses a valid project id outside the caller's project (the 0.16.2 payload, top-level callerAgentId)", async () => {
    const payload = {
      callerAgentId: "agent-tpm",
      source: { kind: "directory", path: OTHER.path, projectId: "prj_other" },
      title: "SWE",
      background: true,
    } as unknown as WorkspaceRequestWithCaller;
    await expect(guardWorkspaceCreate(payload, context(paseoDirectLabels()), config())).rejects.toThrow("project outside");
  });

  it("refuses a workflow caller's Paseo worktree-root directory and a worktree route", async () => {
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { kind: "directory", path: "/Users/test/.paseo/worktrees/x/asma-1", projectId: PROJECT } }), context({}), config()),
    ).rejects.toThrow("Paseo's own worktree root");
    await expect(
      guardWorkspaceCreate(workspaceRequest({ source: { kind: "worktree", cwd: CANONICAL, projectId: PROJECT } } as never), context({}), config()),
    ).rejects.toThrow("worktree creation is outside it");
  });
});

describe("workspace.create hook: Paseo 0.17.4 CLI requests carry no initial agent", () => {
  // `paseo workspace create` sends its source, title, background and the resolved caller (cli workspace/create.js:115-120).
  // `paseo run --new-workspace` sends only its source and the caller (run.js:405-409); its labels go to the agent it
  // creates afterwards (run.js:510-523), and the local source it builds has no projectId (run.js:61-75,
  // workspace/create.js:12-27).
  const runSource = { kind: "directory", path: "/Users/test/carasent/asma-modules/.worktrees/asma-other/x" } as const;

  it("leaves `run --new-workspace` with no caller unguarded, whatever labels the run carries", async () => {
    await expect(guardWorkspaceCreate({ source: runSource } as never, context("error"), config())).resolves.toBeUndefined();
  });

  it("refuses a workflow caller's `run --new-workspace` for its missing projectId, not for the run's labels", async () => {
    await expect(
      guardWorkspaceCreate({ source: runSource, callerAgentId: "agent-1" } as never, context(paseoDirectLabels()), config()),
    ).rejects.toThrow("explicit projectId");
    // An unlabelled caller outside any desk or epic project is not scoped: the run's labels never reach the guard.
    await expect(
      guardWorkspaceCreate(
        { source: runSource, callerAgentId: "agent-1" } as never,
        context({}, { wks_caller: { projectId: "prj_repo", workspaceDirectory: "/tmp/repo", projectDisplayName: "asma-modules" } }),
        config(),
      ),
    ).resolves.toBeUndefined();
  });
});

describe("contribute", () => {
  it("registers the two floor hooks; placement is registered on its own", () => {
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
    expect(registered).toEqual(["agent.create", "agent.session_open"]);
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

describe("placement switch and detector (ASMA-8580)", () => {
  let dir: string;
  let file: string;
  let lines: { info: unknown[]; errors: unknown[] };
  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "kontor-placement-"));
    file = join(dir, "placement.json");
    lines = { info: [], errors: [] };
    vi.spyOn(console, "log").mockImplementation((line) => void lines.info.push(JSON.parse(String(line))));
    vi.spyOn(console, "error").mockImplementation((line) => void lines.errors.push(JSON.parse(String(line))));
  });
  afterEach(() => {
    vi.restoreAllMocks();
    rmSync(dir, { recursive: true, force: true });
  });

  /** A workflow caller (its own epic project) naming another project: refused only when enforced. */
  const misplaced = () => ({ request: workspaceRequest({ source: { kind: "directory", path: "/Users/test/elsewhere", projectId: "prj_other" } }) });
  function register(floorEnabled: boolean) {
    const before = new Map<string, (input: unknown, ctx: Context) => Promise<unknown>>();
    const on = new Map<string, (event: unknown, ctx: Context) => Promise<void>>();
    const stopped: string[] = [];
    const server = {
      before: (name: string, handler: never) => (before.set(name, handler), () => stopped.push(`before:${name}`)),
      on: (name: string, handler: never) => (on.set(name, handler), () => stopped.push(`on:${name}`)),
    } as unknown as PluginServerContext;
    const stop = contributePlacement(server, floorEnabled, config(), file);
    return { before, on, stop, stopped };
  }
  const mode = (placement: string, reason: string) => ({ event: PLACEMENT_MODE_EVENT, placement, reason, file });

  it("follows the floor when placement.json is absent: with the floor off the guard leaves every creation untouched", async () => {
    const off = register(false);
    await expect(off.before.get("workspace.create")!(misplaced(), context({}))).resolves.toEqual(misplaced().request);
    const on = register(true);
    await expect(on.before.get("workspace.create")!(misplaced(), context({}))).rejects.toThrow("project outside");
    expect(lines.info).toEqual([mode("off", "not-configured"), mode("enforced", "not-configured")]);
  });

  it("enforces on an explicit on with the floor off, and an explicit off wins over the floor", async () => {
    writeFileSync(file, '{"placement":"on"}');
    await expect(register(false).before.get("workspace.create")!(misplaced(), context({}))).rejects.toThrow("project outside");
    writeFileSync(file, '{"placement":"off"}');
    await expect(register(true).before.get("workspace.create")!(misplaced(), context({}))).resolves.toBeDefined();
  });

  it("refuses a `..<name>` directory under Paseo's worktree root on the enforced route, and passes it when off (AUD-R3-04)", async () => {
    const dotted = (path: string) => ({ request: workspaceRequest({ source: { kind: "directory", path, projectId: PROJECT } }) });
    writeFileSync(file, '{"placement":"on"}');
    const guard = register(false).before.get("workspace.create")!;
    for (const path of ["/Users/test/.paseo/worktrees/..evil", "/Users/test/.paseo/worktrees/..evil/repo"]) {
      await expect(guard(dotted(path), context({}))).rejects.toThrow("Paseo's own worktree root");
    }
    // A `..<name>` sibling of the root is outside it: it passes the root check and is refused only for want of target
    // evidence (B-5).
    await expect(guard(dotted("/Users/test/.paseo/..worktrees-sibling"), context({}))).rejects.toThrow("no qualified evidence names the target checkout");
    writeFileSync(file, '{"placement":"off"}');
    await expect(guard(dotted("/Users/test/.paseo/worktrees/..evil"), context({}))).resolves.toEqual(dotted("/Users/test/.paseo/worktrees/..evil").request);
  });

  it("refuses a sibling checkout through the registered hook with placement on and the floor off: the request and switch file unchanged (AUD B5-switch-proof)", async () => {
    writeFileSync(file, '{"placement":"on"}');
    const switchBytes = readFileSync(file);
    const guard = register(false).before.get("workspace.create")!;
    const input = { request: workspaceRequest({ source: { kind: "directory", path: "/Users/test/carasent/asma-modules/.worktrees/asma-8508-review/asma-rs-kontor", projectId: PROJECT } }) };
    const raw = JSON.stringify(input);
    let dispatched = 0;
    const error = await guard(input, context(paseoDirectLabels())).then(() => void dispatched++, (cause: unknown) => String(cause));
    expect(dispatched).toBe(0);
    expect(JSON.stringify(input)).toBe(raw);
    expect(readFileSync(file).equals(switchBytes)).toBe(true);
    expect(error).toContain("no qualified evidence names the target checkout");
    expect(lines.info).toEqual([mode("enforced", "explicit-on")]);
  });

  it("enforces a present file that does not conform", async () => {
    writeFileSync(file, '{"placement":"of"}');
    await expect(register(false).before.get("workspace.create")!(misplaced(), context({}))).rejects.toThrow("project outside");
    expect(lines.info[0]).toMatchObject({ placement: "enforced", reason: "invalid", error: expect.stringContaining("placement") });
  });

  it("reads the switch on every creation and logs the mode line only when it changes; it never writes the file", async () => {
    const { before } = register(false);
    const guard = before.get("workspace.create")!;
    await expect(guard(misplaced(), context({}))).resolves.toBeDefined();
    writeFileSync(file, '{"placement":"on"}');
    const written = statSync(file);
    await expect(guard(misplaced(), context({}))).rejects.toThrow("project outside");
    await expect(guard(misplaced(), context({}))).rejects.toThrow("project outside");
    writeFileSync(file, '{"placement":"off"}');
    await expect(guard(misplaced(), context({}))).resolves.toBeDefined();
    rmSync(file);
    await expect(guard(misplaced(), context({}))).resolves.toBeDefined();
    expect(lines.info).toEqual([mode("off", "not-configured"), mode("enforced", "explicit-on"), mode("off", "explicit-off"), mode("off", "not-configured")]);
    expect(written.mtimeMs).toBeGreaterThan(0);
    expect(readdirSync(dir)).toEqual([]);
  });

  it("stops both registrations", () => {
    const { stop, stopped } = register(false);
    stop();
    expect(stopped).toEqual(["before:workspace.create", "on:workspace.created"]);
  });

  it("detects a git project rooted at a linked worktree or a Paseo worktree-root directory, and nothing else", async () => {
    const tree = (name: string, git: string | "dir") => {
      const root = join(dir, name);
      mkdirSync(root, { recursive: true });
      if (git === "dir") mkdirSync(join(root, ".git"));
      else writeFileSync(join(root, ".git"), git);
      return root;
    };
    const linked = tree("linked", "gitdir: /repo/.git/modules/kontor/worktrees/asma-1\n");
    const main = tree("main", "dir");
    const submodule = tree("submodule", "gitdir: ../../.git/modules/kontor\n");
    const ws = (projectKind: string, projectRootPath: string, workspaceDirectory = projectRootPath) =>
      context({}, { wks_new: { projectId: "prj_new", projectKind, projectRootPath, workspaceDirectory } });
    await expect(detectPlacement("wks_new", ws("git", linked), config())).resolves.toEqual({
      event: PLACEMENT_DETECTED_EVENT, kind: "project-at-linked-worktree", workspaceId: "wks_new", projectId: "prj_new", projectRootPath: linked, workspaceDirectory: linked,
    });
    await expect(detectPlacement("wks_new", ws("git", main), config())).resolves.toBeNull();
    await expect(detectPlacement("wks_new", ws("git", submodule), config())).resolves.toBeNull();
    await expect(detectPlacement("wks_new", ws("non_git", linked), config())).resolves.toBeNull();
    await expect(detectPlacement("wks_new", ws("git", main, "/Users/test/.paseo/worktrees/x/asma-1"), config())).resolves.toMatchObject({ kind: "paseo-worktree-root" });
    await expect(detectPlacement("wks_new", ws("git", main, "/Users/test/.paseo/worktrees/..x"), config())).resolves.toMatchObject({ kind: "paseo-worktree-root" });
    await expect(detectPlacement("wks_new", context({}, { wks_new: "error" }), config())).resolves.toBeNull();
  });

  it("logs a detection only when enforced and reads nothing when off", async () => {
    const linked = join(dir, "linked");
    mkdirSync(linked);
    writeFileSync(join(linked, ".git"), "gitdir: /repo/.git/worktrees/asma-1");
    const reads: string[] = [];
    const ctx = context({}, { wks_new: { projectId: "prj_new", projectKind: "git", projectRootPath: linked, workspaceDirectory: linked } });
    const watched = { ...ctx, paseo: { ...ctx.paseo, workspaces: { ref: (id: string) => (reads.push(id), ctx.paseo.workspaces.ref(id)) } } } as unknown as Context;
    const event = { workspace: { id: "wks_new", projectId: "prj_new", cwd: linked, name: null, archivedAt: null } };
    await register(false).on.get("workspace.created")!(event, watched);
    expect(reads).toEqual([]);
    expect(lines.errors).toEqual([]);
    await register(true).on.get("workspace.created")!(event, watched);
    expect(reads).toEqual(["wks_new"]);
    expect(lines.errors).toEqual([expect.objectContaining({ event: PLACEMENT_DETECTED_EVENT, kind: "project-at-linked-worktree" })]);
  });
});
