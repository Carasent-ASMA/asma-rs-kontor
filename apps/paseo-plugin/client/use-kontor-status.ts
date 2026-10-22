import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readKontorStatus, type KontorStatus } from "../shared/kontor-status";

/**
 * The host's kontor.status, keyed by host. Offline, not yet read or a failed read is null, which reads as
 * unknown: never "not enabled", and never a change to the native host status.
 */
export function useKontorStatus(hostId: string, hostStatus: PluginHostSummary["status"]): KontorStatus | null {
  const online = hostStatus === "online";
  const read = useRpc(readKontorStatus);
  const query = useQuery({
    queryKey: ["kontor-status", hostId],
    enabled: online,
    retry: false,
    refetchInterval: online ? 30_000 : false,
    queryFn: () => read({}),
  });
  return online && !query.isError ? query.data ?? null : null;
}
