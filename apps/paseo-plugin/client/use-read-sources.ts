import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readTeamsSource, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { listReadSources, readSource, type SourceRead } from "../shared/read-sources";

export function useReadSources(hostId: string, hostStatus: PluginHostSummary["status"]) {
  const online = hostStatus === "online";
  const list = useRpc(listReadSources);
  const read = useRpc(readSource);
  const readTeams = useRpc(readTeamsSource);
  const query = useQuery({
    queryKey: ["kontor-read-sources", hostId],
    enabled: online,
    retry: false,
    staleTime: 0,
    refetchInterval: online ? 30_000 : false,
    queryFn: async ({ signal }) => {
      const catalog = await list({});
      const bindings = teamsSourceBindings(catalog.sources);
      const teamsSnapshots: Record<string, TeamsSourceSnapshot> = {};
      const teamsErrors: Record<string, string> = {};
      const results: Array<readonly [string, SourceRead]> = [];
      let cursor = 0;
      async function worker() {
        while (!signal.aborted) {
          const source = catalog.sources[cursor++];
          if (!source) return;
          if (bindings.formats[source.id]) {
            try { teamsSnapshots[source.id] = await readTeams({ sourceId: source.id }); }
            catch (failure) { teamsErrors[source.id] = failure instanceof Error ? failure.message : String(failure); }
            continue;
          }
          try { results.push([source.id, await read({ sourceId: source.id })]); }
          catch (failure) { results.push([source.id, { source, status: "unavailable", revision: null, readAt: new Date().toISOString(),
            modifiedAt: null, content: null, graph: null, records: null, diagnostics: [failure instanceof Error ? failure.message : String(failure)] }]); }
        }
        throw new Error("Configured source read canceled");
      }
      await Promise.all(Array.from({ length: Math.min(3, catalog.sources.length) }, () => worker()));
      // A host catalog edit must not combine old bindings with reads of replacement paths.
      const current = await list({});
      if (JSON.stringify(current) !== JSON.stringify(catalog)) throw new Error("Source catalog changed during refresh; refresh again");
      return { ...catalog, reads: Object.fromEntries(results), teamsSnapshots, teamsErrors, teamsBindingDiagnostics: bindings.diagnostics };
    },
  });
  const hostState = hostStatus === "connecting" ? "reconnecting" : hostStatus;
  const error = !online ? `Paseo host ${hostState}. Configured sources unavailable; retained reads are stale.`
    : query.isError ? `Configured source service unavailable: ${query.error.message}. Retained reads are stale.`
      : query.data?.diagnostics.join(" ") || null;
  return { teamsSnapshots: query.data?.teamsSnapshots ?? {}, teamsErrors: query.data?.teamsErrors ?? {}, teamsBindingDiagnostics: query.data?.teamsBindingDiagnostics ?? [], sources: query.data?.sources ?? [], reads: query.data?.reads ?? {}, error,
    loading: query.isFetching, available: online && query.data !== undefined && !query.isError,
    refresh: async () => { await query.refetch(); } };
}
