import { BUILTIN_PROVIDER_IDS } from "@getpaseo/protocol/provider-manifest";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import type { PaseoAgent, PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readUsageTotals, type UsageTotals } from "../shared/session-navigator";
import { createCodexUsageReader, type CodexUsageRead } from "./codex-usage";
import { createStateRoot } from "./session-create";

/** Use the thread identity, never the conversation-tree root, a title, cwd or a model. */
export function codexThreadIdentity(agent: Pick<PaseoAgent, "persistence" | "provider">): string | null {
  const handle = agent.persistence;
  if (handle?.provider !== "codex" || !agent.provider) return null;
  // Aliases retain their public effective route and require canonical Codex persistence.
  if (agent.provider !== "codex" && BUILTIN_PROVIDER_IDS.includes(agent.provider)) return null;
  const native = handle.nativeHandle, thread = handle.metadata?.threadId;
  if (native && typeof thread === "string" && native !== thread) return null;
  const value = native || thread;
  return typeof value === "string" ? value : null;
}
const unavailable = (source: string): UsageTotals => ({ input: null, output: null, scope: "provider-cumulative", source });

export function createVendorUsageService(reader: { read(threadId: string, budget: { bytes: number }): Promise<CodexUsageRead> }) {
  return async (paseo: Pick<PaseoApi, "agents">, agentIds: readonly string[]) => {
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
          const threadId = codexThreadIdentity(found.agent);
          if (!threadId) { result[id] = unavailable("Vendor-native usage is unqualified or its exact thread identity is unavailable. Historical/complete totals unavailable."); continue; }
          const value = await reader.read(threadId, budget);
          // A native restart/provider switch during the file read must not attach an old thread to this agent.
          const current = await paseo.agents.ref(id).refresh();
          result[id] = current?.agent.id === id && current.agent.provider === found.agent.provider && codexThreadIdentity(current.agent) === threadId ? value
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
