import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readTeamsSource, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { readSource, readSourceSummaries, sourceReadPending, type SourceRead } from "../shared/read-sources";

/** One exact full read kept for the summary identity and revision it was read at. */
export type FullSourceRead = { hostId: string; path: string; revision: string | null; read: SourceRead };
type QueueItem = { key: string; hostId: string; path: string; id: string; revision: string | null };

/** Summaries overlaid by full reads of the same host and revision; Teams adapter IDs stay on their snapshots. */
export function overlaySourceReads(summaries: Readonly<Record<string, SourceRead>>, full: Readonly<Record<string, FullSourceRead>>,
  hostId: string, adapterIds: ReadonlySet<string>): Record<string, SourceRead> {
  const reads: Record<string, SourceRead> = {};
  for (const id of Object.keys(summaries)) {
    if (adapterIds.has(id)) continue;
    const summary = summaries[id]!;
    const loaded = full[id];
    reads[id] = loaded && loaded.hostId === hostId && loaded.path === summary.source.path && loaded.revision === summary.revision ? loaded.read : summary;
  }
  return reads;
}

/** Demanded summaries that still need their exact Markdown, graph or records read at the current revision. */
export function pendingDemandIds(demanded: readonly string[], summaries: Readonly<Record<string, SourceRead>>,
  full: Readonly<Record<string, FullSourceRead>>, hostId: string, adapterIds: ReadonlySet<string>): string[] {
  const ids: string[] = [];
  for (const id of demanded) {
    if (adapterIds.has(id)) continue;
    const summary = summaries[id];
    if (!summary || !sourceReadPending(summary)) continue;
    const loaded = full[id];
    if (loaded && loaded.hostId === hostId && loaded.path === summary.source.path && loaded.revision === summary.revision) continue;
    ids.push(id);
  }
  return ids;
}

/**
 * The source catalog and its metadata are one summaries read on the existing 30 s cadence.
 * Pages demand exact sources; full reads keep at most three workers, are keyed by id and
 * revision, and a failed attempt is retried only after the next summaries refresh.
 */
export function useReadSources(hostId: string, hostStatus: PluginHostSummary["status"]) {
  const online = hostStatus === "online";
  const summaries = useRpc(readSourceSummaries);
  const read = useRpc(readSource);
  const readTeams = useRpc(readTeamsSource);
  const query = useQuery({
    queryKey: ["kontor-read-source-summaries", hostId],
    enabled: online,
    retry: false,
    staleTime: 0,
    refetchInterval: online ? 30_000 : false,
    queryFn: async ({ signal }) => {
      const catalog = await summaries({});
      const bindings = teamsSourceBindings(catalog.sources);
      const teamsSnapshots: Record<string, TeamsSourceSnapshot> = {};
      const teamsErrors: Record<string, string> = {};
      const bound = catalog.sources.filter((source) => bindings.formats[source.id]);
      let cursor = 0;
      async function worker() {
        while (!signal.aborted) {
          const source = bound[cursor++];
          if (!source) return;
          try { teamsSnapshots[source.id] = await readTeams({ sourceId: source.id }); }
          catch (failure) { teamsErrors[source.id] = failure instanceof Error ? failure.message : String(failure); }
        }
      }
      await Promise.all(Array.from({ length: Math.min(3, bound.length) }, () => worker()));
      return { ...catalog, teamsSnapshots, teamsErrors, teamsBindingDiagnostics: bindings.diagnostics };
    },
  });
  const [full, setFull] = useState<Readonly<Record<string, FullSourceRead>>>({});
  const [demandRevision, setDemandRevision] = useState(0);
  const demanded = useRef<readonly string[]>([]);
  const queue = useRef<QueueItem[]>([]);
  const inFlight = useRef(new Set<string>());
  const running = useRef(0);
  const attempted = useRef(new Map<string, string>());
  const mounted = useRef(true);
  const readRef = useRef(read);
  readRef.current = read;
  const adapterIds = new Set(Object.keys(teamsSourceBindings(query.data?.sources ?? []).formats));
  const adapterKey = [...adapterIds].sort().join("\u0000");
  const state = useRef({ data: query.data, hostId, online, full, adapterIds });
  state.current = { data: query.data, hostId, online, full, adapterIds };
  const demandRef = useRef<((ids: readonly string[]) => void) | null>(null);
  if (!demandRef.current) demandRef.current = (ids) => {
    const next = [...new Set(ids.filter((id) => typeof id === "string" && id.length > 0))].sort();
    const current = demanded.current;
    if (next.length === current.length && next.every((id, index) => id === current[index])) return;
    demanded.current = next;
    setDemandRevision((revision) => revision + 1);
  };
  const pumpRef = useRef<(() => void) | null>(null);
  if (!pumpRef.current) {
    // Hermes evaluates this bundle with function-scoped loop bindings, so each read's
    // callbacks receive their item as this call's argument, never from the loop.
    const start = (item: QueueItem) => {
      running.current += 1;
      void readRef.current({ sourceId: item.id }).then((result) => {
        if (!mounted.current) return;
        setFull((previous) => ({ ...previous, [item.id]: { hostId: item.hostId, path: item.path, revision: item.revision, read: result } }));
      }, () => undefined).finally(() => {
        running.current -= 1;
        inFlight.current.delete(item.key);
        pumpRef.current!();
      });
    };
    pumpRef.current = () => {
      while (running.current < 3 && queue.current.length) start(queue.current.shift()!);
    };
  }
  const scheduleRef = useRef<(() => void) | null>(null);
  if (!scheduleRef.current) scheduleRef.current = () => {
    const current = state.current;
    const data = current.data;
    if (!current.online || !data) return;
    for (const id of pendingDemandIds(demanded.current, data.reads, current.full, current.hostId, current.adapterIds)) {
      const summary = data.reads[id]!;
      const revision = summary.revision;
      const key = `${id}@${revision}`;
      // A failed attempt waits for the next summaries refresh, which carries a new read time.
      if (inFlight.current.has(key) || attempted.current.get(key) === summary.readAt) continue;
      attempted.current.set(key, summary.readAt);
      inFlight.current.add(key);
      queue.current.push({ key, hostId: current.hostId, path: summary.source.path, id, revision });
    }
    pumpRef.current!();
  };
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; queue.current = []; };
  }, []);
  useEffect(() => { scheduleRef.current!(); }, [query.data, full, online, demandRevision]);
  const reads = useMemo(() => query.data ? overlaySourceReads(query.data.reads, full, hostId,
    new Set(adapterKey ? adapterKey.split("\u0000") : [])) : {}, [query.data, full, hostId, adapterKey]);
  const hostState = hostStatus === "connecting" ? "reconnecting" : hostStatus;
  const error = !online ? `Paseo host ${hostState}. Configured sources unavailable; retained reads are stale.`
    : query.isError ? `Configured source service unavailable: ${query.error.message}. Retained reads are stale.`
      : query.data?.diagnostics.join(" ") || null;
  return { registry: query.data?.registry, teamsSnapshots: query.data?.teamsSnapshots ?? {}, teamsErrors: query.data?.teamsErrors ?? {}, teamsBindingDiagnostics: query.data?.teamsBindingDiagnostics ?? [], sources: query.data?.sources ?? [], reads, error,
    loading: query.isFetching, available: online && query.data !== undefined && !query.isError,
    demand: demandRef.current,
    refresh: async () => { await query.refetch(); } };
}
