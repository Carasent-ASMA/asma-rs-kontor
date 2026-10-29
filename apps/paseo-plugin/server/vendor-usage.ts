import { homedir } from "node:os";
import { dirname, join } from "node:path";
import type { PaseoAgent, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readUsageTotals, type UsageTotals } from "../shared/session-navigator";
import { createCodexUsageReader, type CodexUsageRead } from "./codex-usage";
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

/** Positive host metadata, never absence from a pinned list, establishes an alias. */
async function verifiedCodexRoute(paseo: UsageClient, provider: string): Promise<boolean> {
  if (provider === "codex") return true;
  if (!paseo.providers?.snapshot || !paseo.config?.get) return false;
  try {
    const [snapshot, { config }] = await Promise.all([paseo.providers.snapshot(), paseo.config.get()]);
    const entries = snapshot.entries.filter((entry) => entry.provider === provider);
    // Keep only this Boolean: daemon config can contain credentials and provider homes.
    return entries.length === 1 && entries[0]!.source === "custom" && config.providers?.[provider]?.extends === "codex";
  } catch { return false; }
}
const unavailable = (source: string): UsageTotals => ({ input: null, output: null, scope: "provider-cumulative", source });

export function createVendorUsageService(reader: { read(threadId: string, budget: { bytes: number }): Promise<CodexUsageRead> }) {
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
          const codexRoute = (found.agent.persistence?.provider === "codex" || found.agent.provider === "codex") && await verifiedCodexRoute(paseo, found.agent.provider);
          const threadId = codexThreadIdentity(found.agent, codexRoute);
          if (!threadId) {
            result[id] = unavailable(codexRoute
              ? "Codex exact native thread identity could not be verified. Historical/complete totals unavailable."
              : "No qualified session-usage source or verified Codex alias for this provider route. Only Codex 0.160.0 local records are imported; account quotas and latest-turn telemetry are not session totals.");
            continue;
          }
          const value = await reader.read(threadId, budget);
          // A native restart/provider switch during the file read must not attach an old thread to this agent.
          const current = await paseo.agents.ref(id).refresh();
          const routeStillCodex = current?.agent.provider === found.agent.provider && await verifiedCodexRoute(paseo, current.agent.provider);
          result[id] = current?.agent.id === id && routeStillCodex && codexThreadIdentity(current.agent, routeStillCodex) === threadId ? value
            : unavailable("Native thread changed during the read. Historical/complete totals unavailable.");
        } catch { result[id] = unavailable("Vendor usage could not be read. Historical/complete totals unavailable."); }
      }
    }));
    return result;
  };
}

export function contributeVendorUsage(server: PluginServerContext) {
  // Only this host's explicit/default vendor namespace is eligible; never search account/profile homes.
  const providerRoot = process.env.CODEX_HOME?.trim() || join(homedir(), ".codex");
  const reader = createCodexUsageReader(providerRoot, join(dirname(createStateRoot()), "vendor-usage-cache-v1"));
  const read = createVendorUsageService(reader);
  server.handle(readUsageTotals, ({ agentIds }, { paseo }) => read(paseo, agentIds));
}
