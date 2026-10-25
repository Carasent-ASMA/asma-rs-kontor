import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readWorkCatalog, type WorkCatalogRead } from "../shared/work-catalog";

/**
 * The shared catalog read, keyed by host and refreshed on the plugin's existing
 * 30 s cadence. A failed RPC is unavailable, never an empty catalog; the
 * server-side answer already carries a versioned last-known snapshot fallback.
 */
export function useWorkCatalog(hostId: string, hostStatus: PluginHostSummary["status"]): WorkCatalogRead | null {
  const online = hostStatus === "online";
  const read = useRpc(readWorkCatalog);
  const query = useQuery({
    queryKey: ["kontor-work-catalog", hostId],
    enabled: online,
    retry: false,
    staleTime: 0,
    refetchInterval: online ? 30_000 : false,
    queryFn: () => read({}),
  });
  if (!online) return null;
  if (query.isError) {
    return {
      mode: "unavailable",
      checkedAt: new Date().toISOString(),
      sourceAsOf: null,
      view: null,
      error: query.error instanceof Error ? query.error.message : "the shared catalog read failed",
      snapshot: null,
    };
  }
  return query.data ?? null;
}
