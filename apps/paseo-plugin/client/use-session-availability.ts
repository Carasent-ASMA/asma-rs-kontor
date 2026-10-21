import { useEffect, useMemo, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { probeSessionAvailability, type SessionAvailability } from "../shared/session-lifecycle";
import { hasSessionAttention, type DirectoryEntry } from "./session-directory";

/** Availability is separate from attention, archive, provider health and task completion. */
export function withSessionAvailability(entries: readonly DirectoryEntry[], evidence: Readonly<Record<string, SessionAvailability>>): readonly DirectoryEntry[] {
  return entries.map((entry) => {
    const availability = evidence[entry.id];
    return availability?.revision === entry.updatedAt ? { ...entry, availability } : entry;
  });
}

export function useSessionAvailability(hostId: string, entries: readonly DirectoryEntry[], enabled: boolean) {
  const probe = useRpc(probeSessionAvailability);
  const [evidence, setEvidence] = useState<{ hostId: string; values: Record<string, SessionAvailability> }>({ hostId, values: {} });
  const [checking, setChecking] = useState(false), [revision, setRevision] = useState(0);
  const entriesRef = useRef(entries); entriesRef.current = entries;
  const probeRef = useRef(probe); probeRef.current = probe;
  const signature = entries.filter(hasSessionAttention).map((entry) => `${entry.id}:${entry.updatedAt}`).join("|");
  useEffect(() => {
    if (!enabled) return;
    let canceled = false, cursor = 0;
    const pending = entriesRef.current.filter(hasSessionAttention);
    setChecking(true);
    async function worker() {
      while (!canceled && cursor < pending.length) {
        const batch = pending.slice(cursor, cursor += 25);
        const checkedAt = new Date().toISOString();
        let result: Record<string, SessionAvailability>;
        try { result = await probeRef.current({ agentIds: batch.map((entry) => entry.id) }); }
        catch (failure) { result = Object.fromEntries(batch.map((entry) => [entry.id, { state: "unknown" as const, reason: `Availability probe failed: ${failure instanceof Error ? failure.message : String(failure)}`, checkedAt, revision: entry.updatedAt }])); }
        if (canceled) return;
        const normalized = Object.fromEntries(batch.map((entry) => [entry.id, { ...result[entry.id] ?? { state: "unknown" as const, reason: "No exact-ID probe result", checkedAt }, revision: entry.updatedAt }]));
        setEvidence((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...normalized } }));
      }
    }
    void Promise.all([worker(), worker(), worker()]).finally(() => { if (!canceled) setChecking(false); });
    return () => { canceled = true; };
  }, [hostId, signature, enabled, revision]);
  const resolved = useMemo(() => withSessionAvailability(entries, evidence.hostId === hostId ? evidence.values : {}), [entries, evidence, hostId]);
  return { entries: resolved, checking, refresh: () => setRevision((value) => value + 1) };
}
