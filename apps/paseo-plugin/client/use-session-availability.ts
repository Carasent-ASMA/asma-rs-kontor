import { useEffect, useMemo, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { AVAILABILITY_TTL_MS, freshAvailability, probeSessionAvailability, type LifecycleRow, type SessionAvailability } from "../shared/session-lifecycle";
import { hasSessionAttention, type DirectoryEntry } from "./session-directory";

/** Evidence is bound to the requested native ID and the displayed snapshot, never a provider handle. */
export function bindAvailability(entry: DirectoryEntry, value?: SessionAvailability): SessionAvailability {
  if (!value || value.agentId !== entry.id) return { agentId: entry.id, state: "unknown", reason: "No valid exact-ID read result. Absence is not confirmed.", checkedAt: new Date().toISOString(), revision: entry.updatedAt };
  return { ...value, revision: entry.updatedAt };
}
export function withSessionAvailability(entries: readonly DirectoryEntry[], evidence: Readonly<Record<string, SessionAvailability>>, now = Date.now()): readonly DirectoryEntry[] {
  return entries.map((entry) => {
    const availability = freshAvailability({ ...entry, availability: evidence[entry.id] }, now);
    return { ...entry, availability, ...(availability?.state === "readable" && availability.archivedAt ? { archivedAt: availability.archivedAt } : {}) };
  });
}

export function useSessionAvailability(hostId: string, entries: readonly DirectoryEntry[], enabled: boolean) {
  const probe = useRpc(probeSessionAvailability);
  const [evidence, setEvidence] = useState<{ hostId: string; values: Record<string, SessionAvailability> }>({ hostId, values: {} });
  const [checking, setChecking] = useState(false), [revision, setRevision] = useState(0);
  const entriesRef = useRef(entries); entriesRef.current = entries;
  const probeRef = useRef(probe); probeRef.current = probe;
  const generation = useRef(0), owner = useRef(hostId); owner.current = hostId;
  const signature = entries.filter(hasSessionAttention).map((entry) => `${entry.id}:${entry.updatedAt}`).join("|");
  const refresh = () => { generation.current++; setEvidence({ hostId, values: {} }); setRevision((value) => value + 1); };
  const reconcile = (rows: readonly LifecycleRow[]) => {
    generation.current++;
    const values: Record<string, SessionAvailability> = {};
    for (const row of rows) {
      const entry = entriesRef.current.find((entry) => entry.id === row.id);
      if (entry && row.availability) values[row.id] = bindAvailability(entry, row.availability);
    }
    setEvidence((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...values } }));
    setRevision((value) => value + 1);
  };
  useEffect(() => {
    const times = Object.values(evidence.values).map((value) => Date.parse(value.checkedAt) + AVAILABILITY_TTL_MS).filter(Number.isFinite);
    if (!times.length) return;
    // One expiry, not a polling loop or a bulk provider launch.
    const timer = setTimeout(() => setEvidence({ hostId, values: {} }), Math.max(0, Math.min(...times) - Date.now()));
    return () => clearTimeout(timer);
  }, [hostId, evidence]);
  useEffect(() => {
    if (!enabled) { setChecking(false); return; }
    let canceled = false, cursor = 0; const epoch = ++generation.current;
    const pending = entriesRef.current.filter(hasSessionAttention);
    setChecking(true);
    async function worker() {
      while (!canceled && cursor < pending.length && epoch === generation.current) {
        const batch = pending.slice(cursor, cursor += 25);
        let result: Record<string, SessionAvailability>;
        try { result = await probeRef.current({ agentIds: batch.map((entry) => entry.id) }); }
        catch { result = {}; }
        if (canceled || owner.current !== hostId || epoch !== generation.current) return;
        const normalized = Object.fromEntries(batch.map((entry) => [entry.id, bindAvailability(entry, result[entry.id])]));
        setEvidence((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...normalized } }));
      }
    }
    void Promise.all([worker(), worker(), worker()]).finally(() => { if (!canceled && epoch === generation.current) setChecking(false); });
    return () => { canceled = true; };
  }, [hostId, signature, enabled, revision]);
  const resolved = useMemo(() => withSessionAvailability(entries, evidence.hostId === hostId ? evidence.values : {}), [entries, evidence, hostId, revision]);
  return { entries: resolved, checking, refresh, reconcile };
}
