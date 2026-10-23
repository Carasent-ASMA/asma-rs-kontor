import { useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { readLifecycleReview, reconcileLifecycleItem, type LifecycleRow, type UnresolvedRef } from "../shared/session-lifecycle";

const text = (failure: unknown) => failure instanceof Error ? failure.message : String(failure);

/**
 * One status line per X (NN/g visibility of system status). The host toast owns politeness (WCAG 2.1 SC 4.1.3).
 * Membership is the success; an unconfirmed runtime is reported, never called closed.
 */
export function notifyArchive(toast: { show(message: string): void; error(message: string): void }, result: LifecycleRow | string | null) {
  if (result === null) return;
  if (typeof result === "string") toast.error(`Archive did not report a result: ${result}`);
  else if (result.outcome === "archived" && result.runtime !== "unconfirmed") toast.show(`Archived ${result.title}.`);
  else if (result.outcome === "already_archived" || result.outcome === "absent") toast.show(result.message ?? result.reason);
  else toast.error(result.message ?? result.reason);
}

/** Known refs stay visible beside a failed lookup; unknown never reads as none. */
export function recoveryLine(unresolved: readonly UnresolvedRef[], error: string | null): string | null {
  const count = unresolved.length, line = `${count} archive${count === 1 ? "" : "s"} not confirmed`;
  if (!count) return error ? `Archive checks could not be read: ${error}` : null;
  return error ? `${line}. ${error}` : line;
}

/**
 * Archives whose fence is still held, from the durable fences rather than latest.json, so they survive later
 * reviews and a remount. Reads between actions only; a listed ref is a hint, and Check is an exact-ID read per owner
 * review that never sends archive().
 */
export function useArchiveRecovery(hostId: string, lifecycle: { review: unknown; busy: boolean; running: boolean }, onChanged: (rows: LifecycleRow[]) => void) {
  const read = useRpc(readLifecycleReview), reconcile = useRpc(reconcileLifecycleItem);
  const [unresolved, setUnresolved] = useState<readonly UnresolvedRef[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  // The host whose list a read reported since mount or host switch; until then an empty list is unknown, not none.
  const [listed, setListed] = useState<string | null>(null);
  // A host switch starts a new run; a result from an earlier run, even for the same host ID, changes nothing.
  const owner = useRef(hostId), run = useRef(0), loads = useRef(0), active = useRef(false);
  owner.current = hostId;
  const load = async () => {
    const host = hostId, current = ++loads.current;
    try {
      const result = await read({});
      if (owner.current !== host || current !== loads.current) return;
      // null: no review and no fence on this host. A review without the field (older server) is unknown: keep what is known.
      if (!result) { setUnresolved([]); setError(null); setListed(host); }
      else if (result.unresolved) { setUnresolved(result.unresolved); setError(null); setListed(host); }
      else setError("The host did not report its current list.");
    } catch (failure) { if (owner.current === host && current === loads.current) setError(text(failure)); }
  };
  // An unmount ends the run too: a Check loop never reconciles, loads or calls onChanged for an unmounted view.
  useEffect(() => { run.current++; active.current = false; setUnresolved([]); setError(null); setChecking(false); setListed(null); return () => { run.current++; }; }, [hostId]);
  useEffect(() => { if (!lifecycle.busy && !lifecycle.running) void load(); }, [hostId, lifecycle.review, lifecycle.busy, lifecycle.running]);
  const check = async () => {
    if (active.current) return;
    active.current = true; setChecking(true);
    const token = ++run.current, refs = unresolved;
    let failed: string | null = null;
    try {
      for (const ref of refs) {
        if (run.current !== token) break;
        const result = await reconcile({ reviewId: ref.reviewId, agentId: ref.agentId });
        if (run.current !== token) break;
        onChanged([result]);
      }
    } catch (failure) { failed = text(failure); }
    finally {
      if (run.current === token) { await load(); if (run.current === token) { active.current = false; setChecking(false); if (failed) setError(failed); } }
    }
  };
  return { unresolved, error, checking, listed, check, reload: load, line: recoveryLine(unresolved, error) };
}
