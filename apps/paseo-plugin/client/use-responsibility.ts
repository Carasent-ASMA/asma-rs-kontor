import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readResponsibility, type ResponsibilityRead } from "../shared/responsibility";

/**
 * The host's responsibility projection; keyed by host so a late read never reaches another host's view.
 * A failed read wins over a retained one: an unavailable source never qualifies a match.
 */
export function useResponsibility(hostId: string, hostStatus: PluginHostSummary["status"]): ResponsibilityRead | null {
  const online = hostStatus === "online";
  const read = useRpc(readResponsibility);
  const query = useQuery({
    queryKey: ["kontor-responsibility", hostId],
    enabled: online,
    retry: false,
    staleTime: 0,
    refetchInterval: online ? 60_000 : false,
    queryFn: () => read({}),
  });
  const error = query.isError ? query.error.message : null;
  const { data, errorUpdatedAt } = query;
  return useMemo(() => error === null ? data ?? null : { status: "unavailable", projection: null, revision: null,
    readAt: new Date(errorUpdatedAt).toISOString(), diagnostics: [`Responsibility service unavailable: ${error}`] }, [data, error, errorUpdatedAt]);
}
