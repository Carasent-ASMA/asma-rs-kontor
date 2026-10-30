import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import type { PaseoAgent, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readUsageTotals, type UsageTotals } from "../shared/session-navigator";
import { createCodexUsageReader, type VendorUsageRead } from "./codex-usage";
import { createClaudeUsageReader } from "./claude-usage";
import { createOpenCodeUsageReader, openCodeDatabasePath } from "./opencode-usage";
import { createStateRoot } from "./session-create";

/** Use the thread identity, never the conversation-tree root, a title, cwd or a model. */
export function codexThreadIdentity(agent: Pick<PaseoAgent, "persistence" | "provider">, verifiedCodexRoute = agent.provider === "codex"): string | null {
  const handle = agent.persistence;
  if (handle?.provider !== "codex" || !agent.provider) return null;
  if (!verifiedCodexRoute) return null;
  const native = handle.nativeHandle, thread = handle.metadata?.threadId;
  if (native && typeof thread === "string" && native !== thread) return null;
  const value = native || thread;
  return typeof value === "string" ? value : null;
}
type UsageClient = Pick<PaseoApi, "agents"> & Partial<Pick<PaseoApi, "providers" | "config">>;
export type VendorBase = "codex" | "opencode" | "claude";
export type VendorUsageRoute = { readonly base: VendorBase; readonly source: string };
export type VendorUsageReader = { read(route: VendorUsageRoute, nativeHandle: string, budget: { bytes: number }): Promise<VendorUsageRead> };
export type VerifiedRoute = VendorUsageRoute & { readonly provider: string };
type ProviderConfig = { readonly extends?: unknown; readonly env?: Record<string, unknown> };
type UsageConfig = { readonly providers?: Record<string, ProviderConfig> };

const unavailable = (source: string): UsageTotals => ({ input: null, output: null, scope: "provider-cumulative", source });
const label = (base: VendorBase) => base === "claude" ? "Claude" : base === "opencode" ? "OpenCode" : "Codex";
const configuredPath = (value: unknown) => typeof value === "string" && value.trim() && isAbsolute(value.trim()) ? value.trim() : null;
const envPath = (value: string | undefined, fallback: string) => value?.trim() ? (isAbsolute(value.trim()) ? value.trim() : fallback) : fallback;
const codexSource = (home: string) => resolve(home);
const claudeSource = (configDir: string) => resolve(join(configDir, "projects"));

function defaultSource(base: VendorBase): string {
  if (base === "codex") return codexSource(envPath(process.env.CODEX_HOME, join(homedir(), ".codex")));
  if (base === "claude") return claudeSource(envPath(process.env.CLAUDE_CONFIG_DIR, join(homedir(), ".claude")));
  return resolve(openCodeDatabasePath());
}

/** Positive host metadata, never absence from a pinned list, establishes an alias and its own source root. */
export async function verifiedVendorRoute(paseo: UsageClient, provider: string): Promise<VerifiedRoute | null> {
  if (provider === "codex" || provider === "opencode" || provider === "claude") return { base: provider, provider, source: defaultSource(provider) };
  if (!paseo.providers?.snapshot || !paseo.config?.get) return null;
  try {
    const [snapshot, { config }] = await Promise.all([paseo.providers.snapshot(), paseo.config.get()]);
    const entries = snapshot.entries.filter((entry) => entry.provider === provider);
    if (entries.length !== 1 || entries[0]!.source !== "custom") return null;
    const entry = (config as UsageConfig).providers?.[provider], base = entry?.extends;
    if (base !== "codex" && base !== "opencode" && base !== "claude") return null;
    if (base === "codex") {
      const home = entry?.env?.CODEX_HOME === undefined ? envPath(process.env.CODEX_HOME, join(homedir(), ".codex")) : configuredPath(entry.env.CODEX_HOME);
      return home ? { base, provider, source: codexSource(home) } : null;
    }
    if (base === "claude") {
      const dir = entry?.env?.CLAUDE_CONFIG_DIR === undefined ? envPath(process.env.CLAUDE_CONFIG_DIR, join(homedir(), ".claude")) : configuredPath(entry.env.CLAUDE_CONFIG_DIR);
      return dir ? { base, provider, source: claudeSource(dir) } : null;
    }
    const dataHome = entry?.env?.XDG_DATA_HOME === undefined ? process.env.XDG_DATA_HOME?.trim() || undefined : configuredPath(entry.env.XDG_DATA_HOME);
    if (entry?.env?.XDG_DATA_HOME !== undefined && !dataHome) return null;
    return { base, provider, source: resolve(openCodeDatabasePath(dataHome ? { ...process.env, XDG_DATA_HOME: dataHome } : process.env)) };
  } catch { return null; }
}

export function nativeIdentity(agent: PaseoAgent, route: VerifiedRoute): string | null {
  const handle = agent.persistence;
  if (!handle || handle.provider !== route.base) return null;
  if (route.base === "codex") return codexThreadIdentity(agent, true);
  const native = handle.nativeHandle;
  return typeof native === "string" && native ? native : null;
}

const sameRoute = (left: VerifiedRoute, right: VerifiedRoute | null) =>
  !!right && left.base === right.base && left.provider === right.provider && left.source === right.source;

export function createVendorUsageService(reader: VendorUsageReader) {
  return async (paseo: UsageClient, agentIds: readonly string[]) => {
    const result: Record<string, UsageTotals | null> = {};
    const ids = [...new Set(agentIds)], budget = { bytes: 32 * 1024 * 1024 };
    // Serialize scans so one legal record can use the aggregate budget before yielding.
    let cursor = 0;
    await Promise.all(Array.from({ length: Math.min(ids.length, 1) }, async () => {
      while (cursor < ids.length) {
        const id = ids[cursor++]!;
        try {
          const found = await paseo.agents.ref(id).refresh();
          if (!found || found.agent.id !== id) { result[id] = unavailable("Exact native session could not be verified. Historical/complete totals unavailable."); continue; }
          const route = await verifiedVendorRoute(paseo, found.agent.provider);
          if (!route) {
            result[id] = unavailable("No qualified session-usage source or verified Codex, OpenCode or Claude alias for this provider route. Only local cumulative vendor records are imported; account quotas and latest-turn telemetry are not session totals.");
            continue;
          }
          const native = nativeIdentity(found.agent, route);
          if (!native) {
            result[id] = unavailable(route.base === "codex"
              ? "Codex exact native thread identity could not be verified. Historical/complete totals unavailable."
              : `${label(route.base)} exact native session identity could not be verified. Historical/complete totals unavailable.`);
            continue;
          }
          const value = await reader.read({ base: route.base, source: route.source }, native, budget);
          // A native restart, provider switch or alias-home change during the source read must not attach old totals.
          const current = await paseo.agents.ref(id).refresh();
          const after = current?.agent.id === id ? await verifiedVendorRoute(paseo, current.agent.provider) : null;
          const afterNative = current?.agent.id === id && after ? nativeIdentity(current.agent, after) : null;
          result[id] = sameRoute(route, after) && afterNative === native ? value
            : unavailable(`${label(route.base)} native session changed during the read. Historical/complete totals unavailable.`);
        } catch { result[id] = unavailable("Vendor usage could not be read. Historical/complete totals unavailable."); }
      }
    }));
    return result;
  };
}

export function createVendorUsageReader(cacheRoot: string): VendorUsageReader {
  const codex = new Map<string, ReturnType<typeof createCodexUsageReader>>();
  const claude = new Map<string, ReturnType<typeof createClaudeUsageReader>>();
  const opencode = new Map<string, ReturnType<typeof createOpenCodeUsageReader>>();
  return {
    async read(route, nativeHandle, budget) {
      if (route.base === "codex") {
        const instance = codex.get(route.source) ?? createCodexUsageReader(route.source, cacheRoot); codex.set(route.source, instance);
        return instance.read(nativeHandle, budget);
      }
      if (route.base === "claude") {
        const instance = claude.get(route.source) ?? createClaudeUsageReader(route.source, cacheRoot); claude.set(route.source, instance);
        return instance.read(nativeHandle, budget);
      }
      const instance = opencode.get(route.source) ?? createOpenCodeUsageReader(route.source); opencode.set(route.source, instance);
      return instance.read(nativeHandle);
    },
  };
}

export function contributeVendorUsage(server: PluginServerContext) {
  const cacheRoot = join(dirname(createStateRoot()), "vendor-usage-cache-v1");
  const read = createVendorUsageService(createVendorUsageReader(cacheRoot));
  server.handle(readUsageTotals, ({ agentIds }, { paseo }) => read(paseo, agentIds));
}
