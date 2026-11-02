import { readFileSync } from "node:fs";
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

/**
 * The daemon's own `daemon.listen` from `<paseoHome>/config.json` as a client `host:port`, with a wildcard bind
 * mapped to its loopback. A socket, an unreadable file or any other listen shape is null: the honest no-host gap.
 */
function listenHost(paseoHome: string): string | null {
  let listen: unknown;
  let host: string;
  try {
    // ponytail: one small synchronous read per readConfig() call while the variable is unset, across about eight
    // path-helper callers; memoize per Paseo home (or make paseoHost lazy) if a caller ever reads config on a hot path.
    listen = (JSON.parse(readFileSync(join(paseoHome, "config.json"), "utf8")) as { daemon?: { listen?: unknown } } | null)?.daemon?.listen;
  } catch {
    return null;
  }
  const match = typeof listen === "string" ? /^(\[[0-9a-fA-F:.]+\]|[a-zA-Z0-9_][a-zA-Z0-9_.-]*):(\d{1,5})$/.exec(listen.trim()) : null;
  const port = Number(match?.[2]);
  if (!match || port < 1 || port > 65535) {
    return null;
  }
  try {
    // The URL form is canonical, so every all-zero spelling becomes 0.0.0.0 or [::].
    host = new URL(`http://${match[1]}`).hostname;
  } catch {
    return null;
  }
  return `${host === "0.0.0.0" ? "127.0.0.1" : host === "[::]" ? "[::1]" : host}:${port}`;
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
  const configuredPaseoHome = env.KONTOR_PLUGIN_PASEO_HOME?.trim() || paseoHome || null;
  return {
    stateRoot,
    mcpCommand: env.KONTOR_PLUGIN_MCP_COMMAND?.trim() || DEFAULT_MCP_COMMAND,
    cliCommand: env.KONTOR_PLUGIN_CLI_COMMAND?.trim() || DEFAULT_CLI_COMMAND,
    paseoCommand: env.KONTOR_PLUGIN_PASEO_COMMAND?.trim() || "paseo",
    paseoHome: configuredPaseoHome,
    paseoHost: env.KONTOR_PLUGIN_PASEO_HOST?.trim() || listenHost(configuredPaseoHome || join(home, ".paseo")),
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
