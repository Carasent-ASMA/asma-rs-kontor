import { useCallback, useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { DEFAULT_RECOVERY_TARGET, executeLifecycleItem, lifecycleCapabilities, previewLifecycle, readLifecycleReview, type LifecycleReview, type RecoveryTarget } from "../shared/session-lifecycle";

/** One user-reviewed batch, one request at a time. Closing/cancel stops before the next row. */
export function useSessionLifecycle(hostId: string, onChanged: () => void) {
  const preview = useRpc(previewLifecycle), execute = useRpc(executeLifecycleItem), read = useRpc(readLifecycleReview), discover = useRpc(lifecycleCapabilities);
  const [review, setReview] = useState<LifecycleReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [capabilities, setCapabilities] = useState<Awaited<ReturnType<typeof discover>> | null>(null);
  const [target, setTarget] = useState<RecoveryTarget>(DEFAULT_RECOVERY_TARGET);
  const [selected, setSelected] = useState<readonly string[]>([]);
  const active = useRef(false), cancel = useRef(false), owner = useRef(hostId);
  owner.current = hostId;
  useEffect(() => {
    setReview(null); setSelected([]); setCapabilities(null); setError(null);
    return () => { cancel.current = true; };
  }, [hostId]);
  const fail = (failure: unknown) => setError(failure instanceof Error ? failure.message : String(failure));
  const prepare = async (action: LifecycleReview["action"], scope: LifecycleReview["scope"], ids: string[]) => {
    if (active.current) return;
    active.current = true; setBusy(true); setError(null);
    const host = hostId;
    try {
      const result = await preview({ action, scope, agentIds: ids, target });
      if (owner.current !== host) return;
      setReview(result); setSelected(result.rows.filter((row) => row.eligible).map((row) => row.id));
    } catch (failure) { if (owner.current === host) fail(failure); }
    finally { active.current = false; setBusy(false); }
  };
  const restore = async () => {
    if (active.current) return;
    active.current = true; setError(null); setBusy(true); const host = hostId;
    try { const result = await read({}); if (owner.current === host) { setReview(result); setSelected([]); } }
    catch (failure) { if (owner.current === host) fail(failure); } finally { active.current = false; setBusy(false); }
  };
  const loadCapabilities = async () => { const host = hostId; try { const result = await discover({}); if (owner.current === host) setCapabilities(result); } catch (failure) { if (owner.current === host) fail(failure); } };
  const run = async () => {
    if (!review || active.current) return;
    active.current = true; cancel.current = false; setRunning(true); setError(null);
    const batch = review, host = hostId;
    try {
      for (const id of selected) {
        if (cancel.current || owner.current !== host) break;
        const row = batch.rows.find((entry) => entry.id === id);
        if (!row?.eligible || row.outcome !== "pending") continue;
        // A transport failure has an unknown outcome: stop, then read the durable receipt.
        const result = await execute({ reviewId: batch.id, agentId: id });
        if (owner.current !== host) break;
        setReview((current) => current?.id === batch.id ? { ...current, rows: current.rows.map((entry) => entry.id === id ? result : entry) } : current);
        onChanged();
      }
    } catch (failure) { fail(failure); }
    finally { active.current = false; setRunning(false); }
  };
  const stop = useCallback(() => { cancel.current = true; }, []);
  return { review, error, busy, running, target, setTarget, capabilities, loadCapabilities, selected, setSelected, prepare, restore, run, stop,
    clear: () => { cancel.current = true; setReview(null); setSelected([]); } };
}
