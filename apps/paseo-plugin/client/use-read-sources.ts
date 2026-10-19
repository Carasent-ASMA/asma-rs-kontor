import { useCallback, useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { listReadSources, readSource, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";

export function useReadSources(hostId: string, online: boolean) {
  const list = useRpc(listReadSources);
  const read = useRpc(readSource);
  const [sources, setSources] = useState<ReadSourceSummary[]>([]);
  const [reads, setReads] = useState<Record<string, SourceRead>>({});
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [available, setAvailable] = useState(false);
  const generation = useRef(0);
  const refresh = useCallback(async () => {
    const version = ++generation.current;
    if (!online) { setAvailable(false); setError("Paseo host offline. Configured sources unavailable; retained reads are stale."); return; }
    setLoading(true);
    try {
      const catalog = await list({});
      if (version !== generation.current) return;
      setSources(catalog.sources);
      setAvailable(true);
      const results = await Promise.all(catalog.sources.map(async (source) => {
        try { return [source.id, await read({ sourceId: source.id })] as const; }
        catch (failure) { return [source.id, { source, status: "unavailable" as const, revision: null, readAt: new Date().toISOString(),
          modifiedAt: null, content: null, graph: null, records: null, diagnostics: [failure instanceof Error ? failure.message : String(failure)] }] as const; }
      }));
      if (version !== generation.current) return;
      setReads(Object.fromEntries(results));
      setError(catalog.diagnostics.join(" ") || null);
    } catch (failure) {
      if (version === generation.current) { setAvailable(false); setError(`Configured source service unavailable: ${failure instanceof Error ? failure.message : String(failure)}. Retained reads are stale.`); }
    } finally { if (version === generation.current) setLoading(false); }
  }, [list, read, online]);
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => { void refresh(); }, 30_000);
    return () => { generation.current += 1; clearInterval(timer); };
  }, [hostId, refresh]);
  return { sources, reads, error, loading, available, refresh };
}
