import { useEffect, useRef, useState } from "react";
import { unresolvedArchive, type LifecycleRow, type SessionAvailability } from "../shared/session-lifecycle";
import { notifyArchive } from "./archive-feedback";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { bindAvailability, withLocalDispositions } from "./use-session-availability";
import { useSessionLifecycle } from "./use-session-lifecycle";

/** The same exact-revision settled membership used by Sessions, without mounting its read/probe hook. */
export function archiveDispositions(entries: readonly DirectoryEntry[], rows: readonly LifecycleRow[]): Record<string, SessionAvailability> {
  const values: Record<string, SessionAvailability> = {};
  for (const row of rows) {
    const entry = entries.find((candidate) => candidate.id === row.id);
    if (entry && row.availability && (row.availability.state === "readable" && row.availability.archivedAt || row.outcome === "absent" && row.availability.state === "missing" && row.availability.localRemoval)) {
      values[row.id] = bindAvailability(entry, row.availability);
    }
  }
  return values;
}

/** One existing archive controller for the body. Mounting this adapter issues no reads or effects. */
export function useStartSwitchArchive(hostId: string, directory: DirectoryState, online: boolean) {
  const entries = useRef(directory.entries); entries.current = directory.entries;
  const [settled, setSettled] = useState<{ hostId: string; values: Record<string, SessionAvailability> }>({ hostId, values: {} });
  const [held, setHeld] = useState<{ hostId: string; revisions: Record<string, string> }>({ hostId, revisions: {} });
  const [notice, setNotice] = useState<{ hostId: string; message: string; error: boolean; review: boolean } | null>(null);
  const [pending, setPending] = useState<{ hostId: string; id: string } | null>(null);
  const active = useRef<typeof pending>(null);
  useEffect(() => () => { active.current = null; }, [hostId]);
  const lifecycle = useSessionLifecycle(hostId, (rows) => {
    const values = archiveDispositions(entries.current, rows);
    setSettled((current) => ({ hostId, values: { ...(current.hostId === hostId ? current.values : {}), ...values } }));
  });
  const archive = async (entry: DirectoryEntry) => {
    if (!online || active.current || lifecycle.busy || lifecycle.running || entry.archivedAt !== null || held.hostId === hostId && held.revisions[entry.id] === entry.updatedAt) return;
    const ticket = { hostId, id: entry.id }; active.current = ticket; setPending(ticket); setNotice(null);
    const result = await lifecycle.prepare("archive", "filtered", [entry.id]);
    if (active.current !== ticket) return;
    active.current = null; setPending(null);
    const review = typeof result === "string" || result !== null && unresolvedArchive(result);
    if (review) setHeld((current) => ({ hostId, revisions: { ...(current.hostId === hostId ? current.revisions : {}), [entry.id]: entry.updatedAt } }));
    // Reuse the existing structured outcome wording; a missing reply never becomes success.
    notifyArchive({
      show: (message) => setNotice({ hostId, message, error: false, review: false }),
      error: (message) => setNotice({ hostId, message, error: true, review }),
    }, result);
  };
  return {
    directory: settled.hostId === hostId && Object.keys(settled.values).length ? { ...directory, entries: withLocalDispositions(directory.entries, settled.values) } : directory,
    pendingId: pending?.hostId === hostId && active.current === pending ? pending.id : null,
    disabled: !online || lifecycle.busy || lifecycle.running || active.current !== null,
    held: (entry: DirectoryEntry) => held.hostId === hostId && held.revisions[entry.id] === entry.updatedAt,
    notice: notice?.hostId === hostId ? notice : null,
    archive,
  };
}
