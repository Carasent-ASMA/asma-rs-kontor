import { useCallback, useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { DEFAULT_RECOVERY_TARGET, executeLifecycleItem, reconcileLifecycleItem, lifecycleCapabilities, previewLifecycle, readLifecycleReview, type LifecycleReview, type LifecycleRow, type RecoveryTarget } from "../shared/session-lifecycle";

/** One user-reviewed batch, one request at a time. Closing/cancel stops before the next row. */
export function useSessionLifecycle(hostId: string, onChanged: (rows: LifecycleReview["rows"]) => void) {
  const preview = useRpc(previewLifecycle), execute = useRpc(executeLifecycleItem), read = useRpc(readLifecycleReview), discover = useRpc(lifecycleCapabilities);
  const check = useRpc(reconcileLifecycleItem);
  const [review, setReview] = useState<LifecycleReview | null>(null);
  const [action, setAction] = useState<LifecycleReview["action"]>("resume");
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
  // Hermes evaluates plugin bundles with function-scoped let/const, so an updater made in the execute loop would
  // write a later row's ID with an earlier result. Rows reach the updater only as this call's arguments.
  const putRow = (reviewId: string, id: string, result: LifecycleRow) => setReview((current) => current?.id === reviewId ? { ...current, rows: current.rows.map((entry) => entry.id === id ? result : entry) } : current);
  const prepare = async (action: LifecycleReview["action"], scope: LifecycleReview["scope"], ids: string[]) => {
    if (active.current) return;
    active.current = true; cancel.current = false; setBusy(true); setError(null); setAction(action);
    const host = hostId;
    try {
      const result = await preview({ action, scope, agentIds: ids, target });
      if (owner.current !== host) return;
      onChanged(result.rows); setReview(result); setSelected(result.rows.filter((row) => row.eligible).map((row) => row.id));
      // X is the operator's explicit, single-target Archive action. Resume still
      // requires batch review. Child/identity/availability checks remain server-side.
      if (action === "archive" && result.rows.length === 1 && result.rows[0]?.eligible && !cancel.current) {
        setRunning(true);
        const row = await execute({ reviewId: result.id, agentId: result.rows[0].id });
        if (owner.current !== host) return;
        setReview({ ...result, rows: [row] }); onChanged([row]);
      }
    } catch (failure) { if (owner.current === host) fail(failure); }
    finally { active.current = false; if (owner.current === host) { setBusy(false); setRunning(false); } }
  };
  const restore = async () => {
    if (active.current) return;
    active.current = true; setError(null); setBusy(true); const host = hostId;
    try { const result = await read({}); if (owner.current === host) { if (result) onChanged(result.rows); setReview(result); setSelected([]); setAction(result?.action ?? "resume"); } }
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
        if (!row || row.outcome !== "pending") continue;
        // A transport failure has an unknown outcome: stop, then read the durable receipt.
        const result = await execute({ reviewId: batch.id, agentId: id });
        if (owner.current !== host) break;
        putRow(batch.id, id, result);
        onChanged([result]);
      }
    } catch (failure) { fail(failure); }
    finally { active.current = false; setRunning(false); }
  };
  const stop = useCallback(() => { cancel.current = true; }, []);
  const reconcile = async (id: string) => {
    if (!review || active.current) return;
    const row = review.rows.find((entry) => entry.id === id); if (!row) return;
    active.current = true; setBusy(true); setError(null); const host = hostId, batch = review;
    try {
      const result = await check({ reviewId: row.reconcileReviewId ?? batch.id, agentId: id });
      if (owner.current !== host) return;
      putRow(batch.id, id, result);
      onChanged([result]);
    } catch (failure) { if (owner.current === host) fail(failure); }
    finally { active.current = false; if (owner.current === host) setBusy(false); }
  };
  return { review, action, error, busy, running, target, setTarget, capabilities, loadCapabilities, selected, setSelected, prepare, restore, run, stop, reconcile,
    clear: () => { if (active.current) return; cancel.current = true; setReview(null); setSelected([]); setAction("resume"); } };
}
