import { useQuery } from "@tanstack/react-query";
import { useRpc } from "@getpaseo/plugin/client";
import { listReadSources, readSource, type SourceRead } from "../shared/read-sources";

export function useReadSources(hostId: string, online: boolean) {
  const list = useRpc(listReadSources);
  const read = useRpc(readSource);
  const query = useQuery({
    queryKey: ["kontor-read-sources", hostId],
    enabled: online,
    retry: false,
    staleTime: 0,
    refetchInterval: online ? 30_000 : false,
    queryFn: async ({ signal }) => {
      const catalog = await list({});
      const results: Array<readonly [string, SourceRead]> = [];
      let cursor = 0;
      async function worker() {
        while (!signal.aborted) {
          const source = catalog.sources[cursor++];
          if (!source) return;
          try { results.push([source.id, await read({ sourceId: source.id })]); }
          catch (failure) { results.push([source.id, { source, status: "unavailable", revision: null, readAt: new Date().toISOString(),
            modifiedAt: null, content: null, graph: null, records: null, diagnostics: [failure instanceof Error ? failure.message : String(failure)] }]); }
        }
        throw new Error("Configured source read canceled");
      }
      await Promise.all(Array.from({ length: Math.min(3, catalog.sources.length) }, () => worker()));
      return { ...catalog, reads: Object.fromEntries(results) };
    },
  });
  const error = !online ? "Paseo host offline. Configured sources unavailable; retained reads are stale."
    : query.isError ? `Configured source service unavailable: ${query.error.message}. Retained reads are stale.`
      : query.data?.diagnostics.join(" ") || null;
  return { sources: query.data?.sources ?? [], reads: query.data?.reads ?? {}, error,
    loading: query.isFetching, available: online && query.data !== undefined && !query.isError,
    refresh: async () => { await query.refetch(); } };
}
