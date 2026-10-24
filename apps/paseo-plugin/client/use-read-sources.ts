import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useRpc, type PluginHostSummary } from "@getpaseo/plugin/client";
import { readTeamsSource, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { listReadSources, readSource, readSourceSummaries, sourceReadPending, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";

/** One exact full read kept for the summary identity and revision it was read at. */
export type FullSourceRead = { hostId: string; path: string; kind: SourceRead["source"]["kind"]; revision: string | null; read: SourceRead };
type QueueItem = { key: string; hostId: string; path: string; kind: SourceRead["source"]["kind"]; id: string; revision: string | null; sourceIdentity: string };

/** The catalog identity a summaries transaction must still match before it is published. */
function catalogIdentity(value: { sources: readonly ReadSourceSummary[]; diagnostics: readonly string[]; registry?: unknown }): string {
  return JSON.stringify([value.sources, value.diagnostics, value.registry]);
}
/** The exact source identity a Teams snapshot must share with the catalog it is published under. */
function sourceIdentity(source: ReadSourceSummary): string {
  return JSON.stringify([source.id, source.kind, source.path, source.title, source.workstream, source.epic, source.projectId,
    source.revisionKind, source.logicalPlanId, source.expectedSha256]);
}
/** The request identity, including path, kind and catalog policy, so a same-byte replacement or repin is a new demand. */
function requestKey(item: Omit<QueueItem, "key">): string {
  return [item.hostId, item.id, item.sourceIdentity, item.revision ?? ""].join("\u0000");
}
/**
 * A full read may stand in for this summary only when its own returned identity and the
 * cached request identity both match. A request label is never proof of the returned bytes.
 */
function currentFullRead(summary: SourceRead, loaded: FullSourceRead | undefined, hostId: string): boolean {
  // The current summary's status and catalog policy always win over an older exact read.
  if (summary.status !== "ready" || !loaded || loaded.hostId !== hostId || loaded.path !== summary.source.path
    || loaded.kind !== summary.source.kind || loaded.revision !== summary.revision) return false;
  const read = loaded.read;
  if (read.source.id !== summary.source.id || read.source.path !== summary.source.path
    || read.source.kind !== summary.source.kind || sourceIdentity(read.source) !== sourceIdentity(summary.source)) return false;
  // A completed same-policy exact failure stays visible; a successful read must prove its revision.
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
    const loaded = full[id];
    // An unavailable exact read is shown, but the next summaries refresh retries it.
    if (loaded && currentFullRead(summary, loaded, hostId) && loaded.read.status !== "unavailable") continue;
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
  const list = useRpc(listReadSources);
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
          try {
            const snapshot = await readTeams({ sourceId: source.id });
            // A snapshot read through a replacement catalog must not be published under this one.
            if (sourceIdentity(snapshot.source) !== sourceIdentity(source)) {
              teamsErrors[source.id] = "Teams source identity changed during read; refresh sources.";
              continue;
            }
            teamsSnapshots[source.id] = snapshot;
          }
          catch (failure) { teamsErrors[source.id] = failure instanceof Error ? failure.message : String(failure); }
        }
      }
      await Promise.all(Array.from({ length: Math.min(3, bound.length) }, () => worker()));
      if (signal.aborted) throw new Error("Configured source refresh canceled");
      // The final catalog re-read proves the summaries and every Teams snapshot share one
      // catalog transaction; a changed catalog fails this refresh instead of publishing A + B.
      const current = await list({});
      if (catalogIdentity(current) !== catalogIdentity(catalog)) throw new Error("Source catalog changed during refresh; refresh again");
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
        // A completion for an identity or policy the summaries have left is stale and must not
        // replace the current result. A later summaries refresh raises the new demand.
        if (!summary || summary.status !== "ready" || summary.source.id !== item.id || summary.source.path !== item.path
          || summary.source.kind !== item.kind || summary.revision !== item.revision
          || sourceIdentity(summary.source) !== item.sourceIdentity) {
          attempted.current.delete(item.key);
          return;
        }
        // The returned identity, not this request's label, decides whether the read is usable.
        if (result.source.id !== item.id || result.source.path !== item.path || result.source.kind !== item.kind
          || sourceIdentity(result.source) !== item.sourceIdentity
          || (result.status !== "unavailable" && (result.revision === null || result.revision !== item.revision))) return;
        // An unavailable exact read remains retryable after the next summaries refresh; a
        // successful one satisfies its identity and may be re-read if that identity returns.
        if (result.status !== "unavailable") attempted.current.delete(item.key);
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
      const request = { hostId: current.hostId, path: summary.source.path, kind: summary.source.kind, id, revision,
        sourceIdentity: sourceIdentity(summary.source) };
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
