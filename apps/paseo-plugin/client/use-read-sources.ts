import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readTeamsSource, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { readSource, readSourceSummaries, sourceReadPending, type SourceRead } from "../shared/read-sources";

/** One exact full read kept for the summary identity and revision it was read at. */
export type FullSourceRead = { hostId: string; path: string; kind: SourceRead["source"]["kind"]; revision: string | null; read: SourceRead };
type QueueItem = { key: string; hostId: string; path: string; kind: SourceRead["source"]["kind"]; id: string; revision: string | null };

/** The request identity, including path and kind, so a same-byte replacement is a new demand. */
function requestKey(item: Omit<QueueItem, "key">): string {
  return [item.hostId, item.id, item.kind, item.path, item.revision ?? ""].join("\u0000");
}
/**
 * A full read may stand in for this summary only when its own returned identity and the
 * cached request identity both match. A request label is never proof of the returned bytes.
 */
function currentFullRead(summary: SourceRead, loaded: FullSourceRead | undefined, hostId: string): boolean {
  if (!loaded || loaded.hostId !== hostId || loaded.path !== summary.source.path || loaded.kind !== summary.source.kind
    || loaded.revision !== summary.revision) return false;
  const read = loaded.read;
  if (read.source.id !== summary.source.id || read.source.path !== summary.source.path || read.source.kind !== summary.source.kind) return false;
  return read.status === "unavailable" || (read.revision !== null && read.revision === summary.revision);
}

/** Summaries overlaid by full reads of the same host, source identity and revision; Teams adapter IDs stay on their snapshots. */
export function overlaySourceReads(summaries: Readonly<Record<string, SourceRead>>, full: Readonly<Record<string, FullSourceRead>>,
  hostId: string, adapterIds: ReadonlySet<string>): Record<string, SourceRead> {
  const reads: Record<string, SourceRead> = {};
  for (const id of Object.keys(summaries)) {
    if (adapterIds.has(id)) continue;
    const summary = summaries[id]!;
    const loaded = full[id];
    reads[id] = currentFullRead(summary, loaded, hostId) ? loaded!.read : summary;
  }
  return reads;
}

/** Demanded summaries that still need their exact Markdown, graph or records read at the current identity. */
export function pendingDemandIds(demanded: readonly string[], summaries: Readonly<Record<string, SourceRead>>,
  full: Readonly<Record<string, FullSourceRead>>, hostId: string, adapterIds: ReadonlySet<string>): string[] {
  const ids: string[] = [];
  for (const id of demanded) {
    if (adapterIds.has(id)) continue;
    const summary = summaries[id];
    if (!summary || !sourceReadPending(summary)) continue;
    if (currentFullRead(summary, full[id], hostId)) continue;
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
  const attempted = useRef(new Map<string, SourceRead>());
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
        const summary = state.current.data?.reads[item.id];
        // A completion for an identity the summaries have left is stale and must not replace
        // the current revision/path. A later summaries refresh will raise the new demand.
        if (!summary || summary.source.id !== item.id || summary.source.path !== item.path
          || summary.source.kind !== item.kind || summary.revision !== item.revision) {
          attempted.current.delete(item.key);
          return;
        }
        // The returned identity, not this request's label, decides whether the read is usable.
        if (result.source.id !== item.id || result.source.path !== item.path || result.source.kind !== item.kind
          || (result.status !== "unavailable" && (result.revision === null || result.revision !== item.revision))) return;
        attempted.current.delete(item.key);
        setFull((previous) => ({ ...previous, [item.id]: { hostId: item.hostId, path: item.path, kind: item.kind, revision: item.revision, read: result } }));
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
      const request = { hostId: current.hostId, path: summary.source.path, kind: summary.source.kind, id, revision };
      const key = requestKey(request);
      // A failed or rejected attempt waits for a new summaries object, never the same request again.
      if (inFlight.current.has(key) || attempted.current.get(key) === summary) continue;
      attempted.current.set(key, summary);
      inFlight.current.add(key);
      queue.current.push({ ...request, key });
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
