import type {
  PluginBeforeRequests,
  PluginHookContext,
  PluginServerContext,
} from "@getpaseo/plugin/server";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { deskName, epicProjectKey } from "../shared/session-create";
import { bounded, readAgentLabels, type LabelReadOptions } from "./agent-labels";
import {
  DEFAULT_CREDENTIAL_TIER,
  readConfig,
  type KontorPluginConfig,
} from "./config";
import { placementFile, readFloor } from "./floor";
import { claimsWorkflow, hasKontorLabels, isKontorSeat, seatBinding, type AgentLabels } from "./labels";
import { evaluatePlacement, within, type PlacementScope } from "./placement";
import { KONTOR_WORKSPACE_ID_LABEL, readPlacementBinding } from "./placement-binding";

type AgentCreateRequest = PluginBeforeRequests["agent.create"];
type SessionOpenRequest = PluginBeforeRequests["agent.session_open"];
type WorkspaceCreateRequest = PluginBeforeRequests["workspace.create"];

export const KONTOR_MCP_SERVER_NAME = "kontor-observer";
export const KONTOR_COMPOSED_SERVER_NAME = "kontor";
export const KONTOR_SYSTEM_PROMPT_LINE =
  "This session may be a Kontor seat: resolve its mini-project from Kontor at start and after compaction.";

export const SEAT_ENV = "KONTOR_SEAT";
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
 * The server is composed at observer authority under its own name only when the
 * request carries no `kontor` server of its own. A Kontor-composed scoped seat
 * keeps its server at its own tier and never receives a second realm-wide
 * observer server. The plugin never reads or carries a `kontord` credential: it
 * names the state root, and the MCP process reads its own read-only credential
 * there.
 */
export function addKontorAgentConfig(
  request: AgentCreateRequest,
  config: KontorPluginConfig,
): AgentCreateRequest {
  const current = request.config;
  const mcpServers = { ...(current.mcpServers ?? {}) };
  const hasKontorServer = Object.keys(mcpServers).some(
    (name) =>
      name === KONTOR_COMPOSED_SERVER_NAME ||
      name.startsWith(`${KONTOR_COMPOSED_SERVER_NAME}-`),
  );
  if (config.mcpEnabled && !hasKontorServer) {
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
 * `KONTOR_SEAT` is written on every open, so a resumed session cannot keep the
 * previous seat's environment. Kontor writes no binding-generation label today,
 * so the plugin does not publish a generation key; when a writer carries one,
 * restore it here and in the README.
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
    return { [SEAT_ENV]: UNRESOLVED };
  }
  if (!isKontorSeat(labels)) {
    return { [SEAT_ENV]: UNBOUND };
  }
  const { seat } = seatBinding(labels);
  return { [SEAT_ENV]: seat ?? UNRESOLVED };
}

/**
 * The caller the daemon attached to this creation, when the release carries it.
 *
 * Paseo 0.16.2 carries it as a top-level `callerAgentId` on the
 * `workspace.create` request; its initial agent (`agent`, an agent-create
 * request) may carry one too, which 0.11.2 used. Both are read.
 * `firstAgentContext` holds only a prompt and attachments for name generation,
 * so it names no caller and carries no labels. The guard also reads the
 * initial agent's labels below.
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

/** A workflow seat whose `kontor.*` labels must resolve to a binding; a summary helper's labels are not one. */
function kontorSeat(labels: AgentLabels): labels is Record<string, string> {
  return claimsWorkflow(labels) && hasKontorLabels(labels);
}

interface CallerPlacement {
  readonly labels: AgentLabels;
  readonly workspaceId: string;
  readonly projectId: string;
  readonly projectName: string;
}

type HookPaseo = PluginHookContext["paseo"];

/** The calling agent as the host reads it now: its own workspace and project, bounded. Null when either is not read back. */
async function readCaller(paseo: HookPaseo, agentId: string, options: LabelReadOptions): Promise<CallerPlacement | null> {
  try {
    const found = await bounded(paseo.agents.ref(agentId).refresh(), options.timeoutMs, options.signal);
    const agent = found?.agent;
    if (!agent || agent.id !== agentId || !agent.workspaceId) return null;
    const workspace = await bounded(paseo.workspaces.ref(agent.workspaceId).refresh(), options.timeoutMs, options.signal);
    if (!workspace || workspace.id !== agent.workspaceId || workspace.archivingAt || !workspace.projectId) return null;
    return {
      labels: agent.labels ?? null,
      workspaceId: workspace.id,
      projectId: workspace.projectId,
      projectName: workspace.projectCustomName ?? workspace.projectDisplayName ?? "",
    };
  } catch {
    return null;
  }
}

/** Whether the caller's own native project is a workflow project: a desk or an epic project. */
function workflowProject(caller: CallerPlacement): boolean {
  return deskName(caller.projectName) !== null || epicProjectKey(caller.projectName) !== null;
}

/**
 * Where a workflow caller may place a workspace, from its own native placement.
 *
 * A caller with a Kontor binding is held to that binding, and the binding must
 * name the workspace the caller runs in. Otherwise the caller stays in its own
 * project. Nothing widens that: no source the hook can read proves that a desk
 * caller is assigned to an epic (an `epic` label, a key's syntax or a project's
 * name is not an assignment), so a desk caller cannot place into an epic project
 * through a guarded route.
 */
async function callerScope(paseo: HookPaseo, caller: CallerPlacement, options: LabelReadOptions): Promise<PlacementScope | string> {
  const labels = caller.labels;
  if (kontorSeat(labels)) {
    if (labels[KONTOR_WORKSPACE_ID_LABEL]?.trim() !== caller.workspaceId) {
      return "the caller's Kontor binding names a workspace the caller does not run in";
    }
    const binding = await readPlacementBinding(paseo, labels, options);
    return binding ? { projects: [binding.projectId], directory: binding.directory } : "the seat's placement binding could not be resolved";
  }
  return { projects: [caller.projectId] };
}

/**
 * Refuse a workflow creation outside its own placement.
 *
 * Scope comes from the authoritative caller the daemon attached
 * (`callerAgentId`), read back natively: a caller in a desk or epic project,
 * or one whose labels or whose initial agent's labels claim workflow work, is
 * held to its own project (see `callerScope`). For a caller in a desk or epic
 * project the scope comes from that native placement, so removing its labels
 * does not remove it. A caller anywhere else is in scope only through a
 * workflow claim, so removing its labels removes its scope (the residual
 * below). Labels alone never widen a scope. An unreadable caller is refused.
 *
 * With no caller, a Kontor binding on the initial agent is enforced and any
 * other workflow claim is refused because it names no caller to scope it.
 *
 * Unsupported, and left unguarded: a request with no caller and no workflow
 * claim, and a caller outside any desk or epic project that carries no workflow
 * label. The host gives no provenance that tells such a request from an
 * operator's, so it is unclassified, not operator work.
 */
export async function guardWorkspaceCreate(
  request: WorkspaceCreateRequest,
  context: Pick<PluginHookContext, "paseo" | "signal">,
  config: KontorPluginConfig,
): Promise<void> {
  const options = { timeoutMs: config.labelReadTimeoutMs, signal: context.signal };
  const refuse = (reason: string): never => {
    throw new Error(`Kontor workspace guard refused: ${reason}`);
  };
  const initialLabels = initialAgentLabelsOf(request);
  const callerAgentId = callerAgentIdOf(request);
  const scopes: (PlacementScope | string)[] = [];
  if (callerAgentId) {
    const caller = await readCaller(context.paseo, callerAgentId, options);
    if (!caller) return refuse("the calling agent's own workspace and project could not be read");
    if (workflowProject(caller) || claimsWorkflow(caller.labels) || claimsWorkflow(initialLabels)) {
      scopes.push(await callerScope(context.paseo, caller, options));
    }
  } else if (claimsWorkflow(initialLabels) && !kontorSeat(initialLabels)) {
    return refuse("a workflow creation must name its calling agent");
  }
  if (kontorSeat(initialLabels)) {
    const binding = await readPlacementBinding(context.paseo, initialLabels, options);
    scopes.push(binding ? { projects: [binding.projectId], directory: binding.directory } : "the seat's placement binding could not be resolved");
  }
  for (const scope of scopes) {
    if (typeof scope === "string") refuse(scope);
    const decision = evaluatePlacement(request, config, scope as PlacementScope);
    if (!decision.allowed) refuse(decision.reason);
  }
}

export const PLACEMENT_MODE_EVENT = "kontor.placement.mode";
export const PLACEMENT_DETECTED_EVENT = "kontor.placement.detected";

/**
 * Whether placement is enforced, read on every request from `placement.json`
 * (`{"placement":"on"|"off"}`). Absent follows the floor read at start, so
 * installing this build with the floor off changes nothing. A present file that
 * does not conform enforces, like the floor.
 */
export function placementEnforced(floorEnabled: boolean, file = placementFile()): { enforced: boolean; line: string } {
  const mode = readFloor(file, "placement");
  const enforced = mode.reason === "not-configured" ? floorEnabled : mode.enabled;
  return {
    enforced,
    line: JSON.stringify({ event: PLACEMENT_MODE_EVENT, placement: enforced ? "enforced" : "off", reason: mode.reason, file, ...(mode.error ? { error: mode.error } : {}) }),
  };
}

/** Whether a directory is a linked git worktree: its `.git` is a file pointing into a `worktrees` directory. */
function linkedWorktree(directory: string): boolean {
  try {
    return /^gitdir:.*\/worktrees\/[^/]+\s*$/.test(readFileSync(join(directory, ".git"), "utf8").trim());
  } catch {
    return false;
  }
}

/**
 * Report a created workspace whose native placement is a known misplacement:
 * a git project rooted at a linked worktree, or a directory under Paseo's own
 * worktree root. Detection only: it reads the workspace and writes one log
 * line; it never archives, renames or moves anything.
 */
export async function detectPlacement(
  workspaceId: string,
  context: Pick<PluginHookContext, "paseo" | "signal">,
  config: KontorPluginConfig,
): Promise<Record<string, string> | null> {
  const workspace = await bounded(context.paseo.workspaces.ref(workspaceId).refresh(), config.labelReadTimeoutMs, context.signal).catch(() => null);
  if (!workspace) return null;
  const kind =
    workspace.projectKind === "git" && workspace.projectRootPath && linkedWorktree(workspace.projectRootPath)
      ? "project-at-linked-worktree"
      : workspace.workspaceDirectory && within(resolve(workspace.workspaceDirectory), resolve(config.paseoWorktreesRoot))
        ? "paseo-worktree-root"
        : null;
  return kind
    ? { event: PLACEMENT_DETECTED_EVENT, kind, workspaceId: workspace.id, projectId: workspace.projectId, projectRootPath: workspace.projectRootPath ?? "", workspaceDirectory: workspace.workspaceDirectory ?? "" }
    : null;
}

/**
 * Placement prevention and detection, registered whatever the floor and
 * independent of the observer MCP. Each request reads `placement.json`; off
 * leaves every creation untouched. The mode line is logged when it changes.
 */
export function contributePlacement(
  server: PluginServerContext,
  floorEnabled: boolean,
  config: KontorPluginConfig = readConfig(),
  file = placementFile(),
): () => void {
  let last = "";
  const enforced = (): boolean => {
    const mode = placementEnforced(floorEnabled, file);
    if (mode.line !== last) console.log((last = mode.line));
    return mode.enforced;
  };
  enforced();
  const stopGuard = server.before("workspace.create", async ({ request }, context) => {
    if (enforced()) await guardWorkspaceCreate(request, context, config);
    return request;
  });
  const stopDetect = server.on("workspace.created", async ({ workspace }, context) => {
    if (!enforced()) return;
    const found = await detectPlacement(workspace.id, context, config);
    if (found) console.error(JSON.stringify(found));
  });
  return () => {
    stopGuard();
    stopDetect();
  };
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

  return () => {};
}
