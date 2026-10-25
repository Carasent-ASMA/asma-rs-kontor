import { useEffect, useMemo, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { AVAILABILITY_TTL_MS, freshAvailability, probeSessionAvailability, readSessionDispositions, type LifecycleRow, type SessionAvailability } from "../shared/session-lifecycle";
import { hasSessionAttention, type DirectoryEntry } from "./session-directory";

/** Evidence is bound to the requested native ID and the displayed snapshot, never a provider handle. */
export function bindAvailability(entry: DirectoryEntry, value?: SessionAvailability): SessionAvailability {
  if (!value || value.agentId !== entry.id) return { agentId: entry.id, state: "unknown", reason: "No valid exact-ID read result. Absence is not confirmed.", checkedAt: new Date().toISOString(), revision: entry.updatedAt };
  // An older native observation cannot be promoted to this newer requested snapshot.
  if (Date.parse(value.revision) < Date.parse(entry.updatedAt)) return value;
  return { ...value, revision: entry.updatedAt };
}
export function withSessionAvailability(entries: readonly DirectoryEntry[], evidence: Readonly<Record<string, SessionAvailability>>, now = Date.now()): readonly DirectoryEntry[] {
  return entries.map((entry) => {
    const availability = freshAvailability({ ...entry, availability: evidence[entry.id] }, now);
    return { ...entry, availability, ...(availability?.state === "readable" && availability.archivedAt ? { archivedAt: availability.archivedAt } : {}) };
  });
}
/** An operator's settled action reconciles a stale list until that exact snapshot changes.
 * Unlike availability TTL, this is local disposition, not a claim about provider history. */
export function withLocalDispositions(entries: readonly DirectoryEntry[], values: Readonly<Record<string, SessionAvailability>>): readonly DirectoryEntry[] {
  return entries.flatMap((entry) => {
    const value = values[entry.id];
    if (!value || value.agentId !== entry.id || value.revision !== entry.updatedAt) return [entry];
    if (value.localRemoval && value.state === "missing") return [];
    return [{ ...entry, ...(value.state === "readable" && value.archivedAt ? { archivedAt: value.archivedAt } : {}) }];
  });
}

export function useSessionAvailability(hostId: string, entries: readonly DirectoryEntry[], enabled: boolean) {
  const probe = useRpc(probeSessionAvailability);
  const readDispositions = useRpc(readSessionDispositions);
  const [evidence, setEvidence] = useState<{ hostId: string; values: Record<string, SessionAvailability> }>({ hostId, values: {} });
  const [settled, setSettled] = useState<{ hostId: string; values: Record<string, SessionAvailability> }>({ hostId, values: {} });
  const [checking, setChecking] = useState(false), [revision, setRevision] = useState(0);
  const entriesRef = useRef(entries); entriesRef.current = entries;
  const probeRef = useRef(probe); probeRef.current = probe;
  const dispositionsRef = useRef(readDispositions); dispositionsRef.current = readDispositions;
  const generation = useRef(0), owner = useRef(hostId); owner.current = hostId;
  const signature = entries.filter(hasSessionAttention).map((entry) => `${entry.id}:${entry.updatedAt}`).join("|");
  // Sorted: a running session's reorder is not a new set of sessions, and must not re-read every disposition.
  const idsSignature = entries.map((entry) => entry.id).sort().join("|");
  const refresh = () => { generation.current++; setEvidence({ hostId, values: {} }); setSettled({ hostId, values: {} }); setRevision((value) => value + 1); };
  const reconcile = (rows: readonly LifecycleRow[]) => {
    generation.current++;
    const values: Record<string, SessionAvailability> = {};
    const dispositions: Record<string, SessionAvailability> = {};
    for (const row of rows) {
      // This callback belongs to the render that started the request. A newer render's
      // entriesRef must never relabel its reply (including a revision-less absence).
      const entry = entries.find((entry) => entry.id === row.id);
      if (entry && row.availability) {
        values[row.id] = bindAvailability(entry, row.availability);
        if (row.outcome === "archived" || row.outcome === "already_archived" || row.outcome === "absent" && row.availability.localRemoval) dispositions[row.id] = values[row.id]!;
      }
    }
    setEvidence((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...values } }));
    setSettled((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...dispositions } }));
    setRevision((value) => value + 1);
  };
  // Hermes evaluates plugin bundles with function-scoped let/const, so an updater made inside a read loop would
  // see a later batch, or a `let` reset to undefined. Batches reach state updaters only as these calls' arguments.
  const addEvidence = (values: Record<string, SessionAvailability>) => setEvidence((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...values } }));
  const settleBatch = (batch: readonly DirectoryEntry[], result: Record<string, SessionAvailability>) => {
    addEvidence(Object.fromEntries(batch.filter(entry => result[entry.id]).map(entry => [entry.id, bindAvailability(entry, result[entry.id])])));
    setSettled((current) => {
      const values = { ...(current.hostId === hostId ? current.values : {}) };
      for (const entry of batch) {
        const value = result[entry.id];
        if (!value || value.agentId !== entry.id) continue;
        if (value.localRemoval && value.state === "missing") values[entry.id] = bindAvailability(entry, value);
        else delete values[entry.id];
      }
      return { hostId, values };
    });
  };
  useEffect(() => {
    let canceled = false;
    const snapshot = entriesRef.current;
    void (async () => {
      for (let cursor = 0; cursor < snapshot.length && !canceled; cursor += 100) {
        const batch = snapshot.slice(cursor, cursor + 100);
        let result: Record<string, SessionAvailability>;
        try { result = await dispositionsRef.current({ agentIds: batch.map((entry) => entry.id) }); }
        catch { continue; } // No local removal is inferred when receipt/native reads fail.
        if (canceled || owner.current !== hostId) return;
        settleBatch(batch, result);
      }
    })();
    return () => { canceled = true; };
  }, [hostId, idsSignature, revision]);
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
        addEvidence(Object.fromEntries(batch.map((entry) => [entry.id, bindAvailability(entry, result[entry.id])])));
      }
    }
    void Promise.all([worker(), worker(), worker()]).finally(() => { if (!canceled && epoch === generation.current) setChecking(false); });
    return () => { canceled = true; };
  }, [hostId, signature, enabled, revision]);
  const resolved = useMemo(() => withLocalDispositions(withSessionAvailability(entries, evidence.hostId === hostId ? evidence.values : {}), settled.hostId === hostId ? settled.values : {}), [entries, evidence, settled, hostId, revision]);
  return { entries: resolved, checking, refresh, reconcile };
}
