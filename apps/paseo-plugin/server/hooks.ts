import type {
  PluginBeforeRequests,
  PluginHookContext,
  PluginServerContext,
} from "@getpaseo/plugin/server";
import { readAgentLabels } from "./agent-labels";
import {
  DEFAULT_CREDENTIAL_TIER,
  readConfig,
  type KontorPluginConfig,
} from "./config";
import { isKontorSeat, seatBinding, type AgentLabels } from "./labels";
import { evaluatePlacement } from "./placement";
import { readPlacementBinding } from "./placement-binding";

type AgentCreateRequest = PluginBeforeRequests["agent.create"];
type SessionOpenRequest = PluginBeforeRequests["agent.session_open"];
type WorkspaceCreateRequest = PluginBeforeRequests["workspace.create"];

export const KONTOR_MCP_SERVER_NAME = "kontor-observer";
export const KONTOR_SYSTEM_PROMPT_LINE =
  "This session may be a Kontor seat: resolve its mini-project from Kontor at start and after compaction.";

export const SEAT_ENV = "KONTOR_SEAT";
export const BINDING_GENERATION_ENV = "KONTOR_BINDING_GENERATION";
export const UNBOUND = "unbound";
export const UNRESOLVED = "unresolved";

function appendPromptLine(systemPrompt: string | undefined, line: string): string {
  const current = systemPrompt?.trimEnd() ?? "";
  if (current.split("\n").some((entry) => entry.trim() === line)) {
    return current;
  }
  return current.length === 0 ? line : `${current}\n${line}`;
}

/**
 * Add the Kontor read surface to a new agent.
 *
 * The server is always composed at observer authority under its own name, so a
 * `kontor` server the provider home already defines is never replaced. The
 * plugin never reads or carries a `kontord` credential: it names the state
 * root, and the MCP process reads its own read-only credential there.
 */
export function addKontorAgentConfig(
  request: AgentCreateRequest,
  config: KontorPluginConfig,
): AgentCreateRequest {
  const current = request.config;
  const mcpServers = { ...(current.mcpServers ?? {}) };
  if (config.mcpEnabled && !(KONTOR_MCP_SERVER_NAME in mcpServers)) {
    mcpServers[KONTOR_MCP_SERVER_NAME] = {
      type: "stdio",
      command: config.mcpCommand,
      args: [
        "--state-root",
        config.stateRoot,
        "--credential-tier",
        DEFAULT_CREDENTIAL_TIER,
        ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : []),
      ],
    };
  }
  return {
    ...request,
    config: {
      ...current,
      mcpServers,
      systemPrompt: appendPromptLine(current.systemPrompt, KONTOR_SYSTEM_PROMPT_LINE),
    },
  };
}

/**
 * Resolve the seat identity for a session opening.
 *
 * Fails open by design: when the local labels cannot be read within the hook
 * budget, the environment carries `unresolved` and the Paseo operation
 * continues. A person opening a tab never blocks on Kontor.
 *
 * Both keys are written on every open: a resumed session can carry the previous
 * seat's environment, and an old generation must never outlive the seat
 * identity it belonged to. A generation that cannot be resolved is cleared.
 */
export async function seatEnvironment(
  request: SessionOpenRequest,
  context: Pick<PluginHookContext, "paseo" | "signal">,
  config: KontorPluginConfig,
): Promise<Record<string, string>> {
  const labels = await readAgentLabels(context.paseo, request.agentId, {
    timeoutMs: config.labelReadTimeoutMs,
    signal: context.signal,
  });
  if (labels === null) {
    return { [SEAT_ENV]: UNRESOLVED, [BINDING_GENERATION_ENV]: "" };
  }
  if (!isKontorSeat(labels)) {
    return { [SEAT_ENV]: UNBOUND, [BINDING_GENERATION_ENV]: "" };
  }
  const { seat, generation } = seatBinding(labels);
  if (seat === null) {
    return { [SEAT_ENV]: UNRESOLVED, [BINDING_GENERATION_ENV]: "" };
  }
  return {
    [SEAT_ENV]: seat,
    [BINDING_GENERATION_ENV]: generation ?? "",
  };
}

/**
 * The caller the daemon attached to this creation, when the release carries it.
 *
 * Paseo 0.11.2 carries the caller as `agent.callerAgentId` on the
 * `workspace.create` request and passes it through to the hook. A top-level
 * `callerAgentId` is read as well, so a release that moves the field needs no
 * code change. The guard also reads the initial agent's labels below.
 */
function callerAgentIdOf(request: WorkspaceCreateRequest): string | undefined {
  const direct = (request as { callerAgentId?: unknown }).callerAgentId;
  if (typeof direct === "string" && direct.length > 0) {
    return direct;
  }
  const nested = (request as { agent?: { callerAgentId?: unknown } }).agent?.callerAgentId;
  return typeof nested === "string" && nested.length > 0 ? nested : undefined;
}

function initialAgentLabelsOf(request: WorkspaceCreateRequest): Record<string, string> | null {
  const agent = (request as { agent?: { labels?: unknown } }).agent;
  const labels = agent?.labels;
  if (!labels || typeof labels !== "object" || Array.isArray(labels)) {
    return null;
  }
  return labels as Record<string, string>;
}

/**
 * Refuse a labelled Kontor seat's workspace creation outside its own placement.
 *
 * A human's request carries no labelled initial agent, an unlabelled caller is
 * interactive, and an unreadable caller lookup with no seat labels on the
 * request fails open; in those cases the request is unchanged. Both label sets
 * are checked: the request's initial agent and, when the release passes it, the
 * caller. When either marks a Kontor seat, the request must match that seat's
 * binding — the native project its own workspace belongs to and the canonical
 * worktree its labels name.
 */
export async function guardWorkspaceCreate(
  request: WorkspaceCreateRequest,
  context: Pick<PluginHookContext, "paseo" | "signal">,
  config: KontorPluginConfig,
): Promise<void> {
  const initialLabels = initialAgentLabelsOf(request);
  const callerAgentId = callerAgentIdOf(request);
  const callerLabels = callerAgentId
    ? await readAgentLabels(context.paseo, callerAgentId, {
        timeoutMs: config.labelReadTimeoutMs,
        signal: context.signal,
      })
    : null;
  const seatLabels: AgentLabels[] = [initialLabels, callerLabels].filter((labels) =>
    isKontorSeat(labels),
  );
  if (seatLabels.length === 0) {
    return;
  }
  for (const labels of seatLabels) {
    const binding = await readPlacementBinding(context.paseo, labels, {
      timeoutMs: config.labelReadTimeoutMs,
      signal: context.signal,
    });
    if (binding === null) {
      throw new Error(
        "Kontor workspace guard refused: the seat's placement binding could not be resolved",
      );
    }
    const decision = evaluatePlacement(request, config, binding);
    if (!decision.allowed) {
      throw new Error(`Kontor workspace guard refused: ${decision.reason}`);
    }
  }
}

export function contribute(
  server: PluginServerContext,
  config: KontorPluginConfig = readConfig(),
): () => void {
  server.before("agent.create", ({ request }) => addKontorAgentConfig(request, config));

  server.before("agent.session_open", async ({ request }, context) => ({
    ...request,
    env: {
      ...request.env,
      ...(await seatEnvironment(request, context, config)),
    },
  }));

  server.before("workspace.create", async ({ request }, context) => {
    await guardWorkspaceCreate(request, context, config);
    return request;
  });

  return () => {};
}
