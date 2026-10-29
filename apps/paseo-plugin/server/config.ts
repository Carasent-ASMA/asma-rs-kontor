import { homedir } from "node:os";
import { join, resolve } from "node:path";

export const DEFAULT_MCP_COMMAND = "kontor-mcp";
export const DEFAULT_CLI_COMMAND = "kontor";
export const DEFAULT_CREDENTIAL_TIER = "observer";
export const DEFAULT_LABEL_READ_TIMEOUT_MS = 2_000;

export interface KontorPluginConfig {
  /** The realm state root handed to `kontor-mcp`; the plugin never reads its files. */
  readonly stateRoot: string;
  /** The `kontor-mcp` command launched for a session. */
  readonly mcpCommand: string;
  /** The `kontor` command a confirmed native takeover runs for its one local fleet selection read. */
  readonly cliCommand: string;
  /** The installed public Paseo CLI used only by the bounded stop and schedule-read adapter. */
  readonly paseoCommand: string;
  /** Optional daemon home passed to the public CLI; never read from a review or UI caller. */
  readonly paseoHome: string | null;
  /** Optional daemon host passed to the public CLI; never read from a review or UI caller. */
  readonly paseoHost: string | null;
  /** Whether `agent.create` adds the Kontor MCP server. */
  readonly mcpEnabled: boolean;
  /** Optional `--serve-profile` narrowing, passed through unchanged. */
  readonly serveProfile: string | null;
  /** Optional extra containment: every placement path must sit under one of these roots. */
  readonly placementRoots: readonly string[];
  /** Paseo's own worktree root, which is never Kontor placement. */
  readonly paseoWorktreesRoot: string;
  /** How long hooks may wait for local labels before failing open. */
  readonly labelReadTimeoutMs: number;
}

function homeOf(env: NodeJS.ProcessEnv): string {
  const configured = env.HOME?.trim();
  return configured ? configured : homedir();
}

function parseRoots(value: string | undefined, home: string): readonly string[] {
  if (!value) {
    return [];
  }
  const roots = value
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
    .map((entry) => (entry.startsWith("~/") ? join(home, entry.slice(2)) : entry))
    .map((entry) => resolve(entry));
  return [...new Set(roots)];
}

function positiveInteger(value: string | undefined, fallback: number): number {
  if (!value) {
    return fallback;
  }
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

/**
 * Read the plugin's read-only configuration.
 *
 * The credential tier is deliberately not configurable and is always
 * `observer`: the plugin floor adds a read surface for seat context, never a
 * `kontord` write credential.
 */
export function readConfig(env: NodeJS.ProcessEnv = process.env): KontorPluginConfig {
  const home = homeOf(env);
  const paseoHome = env.PASEO_HOME?.trim();
  const stateRoot =
    env.KONTOR_PLUGIN_STATE_ROOT?.trim() ||
    join(home, ".local", "state", "kontor", "asma");
  const serveProfile = env.KONTOR_PLUGIN_SERVE_PROFILE?.trim() || null;
  return {
    stateRoot,
    mcpCommand: env.KONTOR_PLUGIN_MCP_COMMAND?.trim() || DEFAULT_MCP_COMMAND,
    cliCommand: env.KONTOR_PLUGIN_CLI_COMMAND?.trim() || DEFAULT_CLI_COMMAND,
    paseoCommand: env.KONTOR_PLUGIN_PASEO_COMMAND?.trim() || "paseo",
    paseoHome: env.KONTOR_PLUGIN_PASEO_HOME?.trim() || paseoHome || null,
    paseoHost: env.KONTOR_PLUGIN_PASEO_HOST?.trim() || null,
    mcpEnabled: env.KONTOR_PLUGIN_MCP !== "off",
    serveProfile,
    placementRoots: parseRoots(env.KONTOR_PLUGIN_PLACEMENT_ROOTS, home),
    paseoWorktreesRoot: join(paseoHome || join(home, ".paseo"), "worktrees"),
    labelReadTimeoutMs: positiveInteger(
      env.KONTOR_PLUGIN_LABEL_TIMEOUT_MS,
      DEFAULT_LABEL_READ_TIMEOUT_MS,
    ),
  };
}
