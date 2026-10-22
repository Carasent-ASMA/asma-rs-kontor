import { useCallback, useEffect, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { ARCHIVE_BATCH_LIMIT, archiveSummary, DEFAULT_RECOVERY_TARGET, executeLifecycleItem, reconcileLifecycleItem, lifecycleCapabilities, previewLifecycle, readLifecycleReview, type LifecycleReview, type LifecycleRow, type RecoveryTarget, type UnresolvedRef } from "../shared/session-lifecycle";

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
  const [batch, setBatch] = useState<{ currentId: string | null; completed: number; total: number; announcement: string; summary: string | null } | null>(null);
  // Each host view of a mount is one generation with its own one-action lock, as in useArchiveRecovery: the cleanup
  // ends it on a host switch (even A→B→A) and on unmount, the next view starts idle, and nothing an earlier generation
  // started (result, failure or completion) sets state, reports, calls onChanged or frees the current view's lock.
  const active = useRef(false), cancel = useRef(false), generation = useRef(0);
  useEffect(() => {
    active.current = false; setBusy(false); setRunning(false); setReview(null); setSelected([]); setCapabilities(null); setError(null); setBatch(null);
    return () => { cancel.current = true; generation.current++; };
  }, [hostId]);
  const done = (view: number) => { if (generation.current !== view) return; active.current = false; setBusy(false); setRunning(false); };
  const text = (failure: unknown) => failure instanceof Error ? failure.message : String(failure);
  const fail = (failure: unknown) => setError(text(failure));
  // Hermes evaluates plugin bundles with function-scoped let/const, so an updater made in the execute loop would
  // write a later row's ID with an earlier result. Rows reach the updater only as this call's arguments.
  const putRow = (reviewId: string, id: string, result: LifecycleRow) => setReview((current) => current?.id === reviewId ? { ...current, rows: current.rows.map((entry) => entry.id === id ? result : entry) } : current);
  const progress = (id: string, completed: number) => setBatch((current) => current && { ...current, currentId: id, completed });
  /** Resolves to the X's final row or failure text for one status line; null when ignored or for a review. */
  const prepare = async (action: LifecycleReview["action"], scope: LifecycleReview["scope"], ids: string[], confirmArchive = false): Promise<LifecycleRow | string | null> => {
    if (active.current) return null;
    active.current = true; cancel.current = false; setBusy(true); setError(null); setAction(action); setBatch(null);
    const view = generation.current;
    try {
      const result = await preview({ action, scope, agentIds: ids, target });
      if (generation.current !== view) return null;
      // X is the operator's explicit, single-target Archive action. Resume still
      // requires batch review. Child/identity/availability checks remain server-side.
      // The X's preview rows never reach the directory: it changes once, with the final row.
      const single = action === "archive" && result.rows.length === 1 && !confirmArchive, only = result.rows[0];
      if (!single) onChanged(result.rows);
      setReview(result); setSelected(result.rows.filter((row) => action === "archive" || row.eligible).map((row) => row.id));
      if (!single || !only) return null;
      if (!only.eligible || cancel.current) { onChanged(result.rows); return only; }
      setRunning(true);
      const row = await execute({ reviewId: result.id, agentId: only.id });
      if (generation.current !== view) return null;
      setReview({ ...result, rows: [row] }); onChanged([row]);
      return row;
    } catch (failure) { if (generation.current !== view) return null; fail(failure); return text(failure); }
    finally { done(view); }
  };
  /**
   * Reads one archive's own review (its exact owner ref), or the latest without a ref. Resolves to the review read,
   * failure text, or null when ignored, from an earlier generation, or none is recorded. An owner read that fails or
   * returns anything but that review with that session keeps no earlier, unrelated review.
   */
  const restore = async (ref?: UnresolvedRef): Promise<LifecycleReview | string | null> => {
    if (active.current) return null;
    active.current = true; setError(null); setBusy(true); const view = generation.current;
    try {
      const result = await read(ref ? { reviewId: ref.reviewId } : {});
      if (generation.current !== view) return null;
      if (ref && !(result?.id === ref.reviewId && result.rows.some((row) => row.id === ref.agentId))) throw new Error("The host did not return this archive's own review.");
      if (result) onChanged(result.rows); setReview(result); setSelected([]); setAction(result?.action ?? "resume");
      return result;
    } catch (failure) {
      if (generation.current !== view) return null;
      if (ref) { setReview(null); setSelected([]); }
      fail(failure); return text(failure);
    } finally { done(view); }
  };
  const loadCapabilities = async () => { const view = generation.current; try { const result = await discover({}); if (generation.current === view) setCapabilities(result); } catch (failure) { if (generation.current === view) fail(failure); } };
  const run = async () => {
    if (!review || active.current) return null;
    if (review.action === "archive" && (review.scope !== "filtered" || review.rows.length > ARCHIVE_BATCH_LIMIT)) { fail("Archive batch exceeds its explicit filtered scope or 50-session cap."); return null; }
    active.current = true; cancel.current = false; setRunning(true); setError(null);
    const batch = review, view = generation.current;
    const rows = batch.rows.map((row) => ({ ...row })), ids = batch.action === "archive" ? batch.rows.filter((row) => selected.includes(row.id)).map((row) => row.id) : [...selected];
    let completed = 0, inFlight: string | null = null;
    if (batch.action === "archive") setBatch({ currentId: null, completed, total: ids.length, announcement: `Archiving ${ids.length} selected sessions.`, summary: null });
    try {
      for (const id of ids) {
        if (cancel.current) break;
        const row = batch.rows.find((entry) => entry.id === id);
        if (!row || row.outcome !== "pending") continue;
        inFlight = id;
        if (batch.action === "archive") progress(id, completed);
        // A transport failure has an unknown outcome: stop, then read the durable receipt.
        const result = await execute({ reviewId: batch.id, agentId: id });
        if (generation.current !== view) break;
        putRow(batch.id, id, result);
        rows[rows.findIndex((entry) => entry.id === id)] = result; completed++; inFlight = null;
        onChanged([result]);
      }
    } catch (failure) {
      if (generation.current === view) {
        fail(failure);
        if (batch.action === "archive" && inFlight) {
          const row = rows.find((entry) => entry.id === inFlight)!;
          const uncertain: LifecycleRow = { ...row, outcome: "uncertain", message: "The request did not report a result. Read its durable receipt and check the native outcome; do not resend." };
          putRow(batch.id, row.id, uncertain); rows[rows.indexOf(row)] = uncertain; onChanged([uncertain]);
        }
      }
    } finally { done(view); }
    if (generation.current !== view || batch.action !== "archive") return null;
    const summary = `${cancel.current ? "Stopped before the next session. " : ""}${archiveSummary(rows.filter((row) => ids.includes(row.id)))}`;
    setBatch({ currentId: null, completed, total: ids.length, announcement: summary, summary });
    return summary;
  };
  const stop = useCallback(() => {
    cancel.current = true;
    if (active.current) setBatch((current) => current && !current.summary ? { ...current, announcement: "Stopping after the current session. No next session will be started." } : current);
  }, []);
  const reconcile = async (id: string) => {
    if (!review || active.current) return;
    const row = review.rows.find((entry) => entry.id === id); if (!row) return;
    active.current = true; setBusy(true); setError(null); const view = generation.current, batch = review;
    try {
      const result = await check({ reviewId: row.reconcileReviewId ?? batch.id, agentId: id });
      if (generation.current !== view) return;
      putRow(batch.id, id, result);
      onChanged([result]);
      if (batch.action === "archive") {
        const summary = archiveSummary(batch.rows.map((row) => row.id === id ? result : row));
        setBatch((current) => current && { ...current, summary, announcement: summary });
      }
    } catch (failure) { if (generation.current === view) fail(failure); }
    finally { done(view); }
  };
  return { review, action, error, busy, running, batch, target, setTarget, capabilities, loadCapabilities, selected, setSelected, prepare, restore, run, stop, reconcile,
    clear: () => { if (active.current) return; cancel.current = true; setReview(null); setSelected([]); setBatch(null); setAction("resume"); } };
}
