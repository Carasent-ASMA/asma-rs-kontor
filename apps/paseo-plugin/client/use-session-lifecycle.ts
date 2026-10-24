import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { ARCHIVE_BATCH_LIMIT, archiveSummary, DEFAULT_RECOVERY_TARGET, deriveSessionProjection, executeLifecycleItem, freshAvailability, nativeArchiveMembership, reconcileLifecycleItem, lifecycleCapabilities, previewLifecycle, readLifecycleReview, unresolvedArchive, type ArchiveIntentObservation, type LifecycleReview, type LifecycleRow, type ProjectionConflict, type ProjectionSource, type RecoveryTarget, type ScopeClaim, type SessionProjection, type UnresolvedRef } from "../shared/session-lifecycle";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";

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

type ProjectionReceipts = {
  readonly hostId: string;
  readonly state: "idle" | "reading" | "ready" | "partial" | "unknown";
  readonly reviews: Readonly<Record<string, LifecycleReview>>;
  readonly unknownIntent: ReadonlySet<string>;
  readonly error: string | null;
  readonly readAt: string | null;
};

const emptyReceipts = (hostId: string): ProjectionReceipts => ({ hostId, state: "idle", reviews: {}, unknownIntent: new Set(), error: null, readAt: null });
const failureText = (failure: unknown) => failure instanceof Error ? failure.message : String(failure);

export type SessionProjectionHookInput = {
  readonly entries: readonly DirectoryEntry[];
  readonly directory: Pick<DirectoryState, "status" | "live" | "lastUpdatedAt" | "inventoryComplete" | "includeArchived">;
  readonly sources?: readonly ReadSourceSummary[];
  readonly reads?: Readonly<Record<string, SourceRead>>;
  /** Exact plan associations by native session ID; an association is a claim, never a scope grant. */
  readonly associations?: Readonly<Record<string, readonly string[]>>;
  readonly enabled?: boolean;
};

/**
 * Read-only retention projection over the existing native directory, configured source reads and the existing
 * archive receipt read. It never mounts the action hook and never calls preview, execute, reconcile, resume,
 * archive, send, create or a provider timeline read. Receipt reads are deduplicated by exact review ID; one
 * failed read keeps every successful receipt and marks only its affected sessions' coverage unknown.
 */
export function useSessionRetirementProjection(hostId: string, input: SessionProjectionHookInput): {
  readonly projections: ReadonlyMap<string, SessionProjection>;
  readonly receipts: { readonly state: ProjectionReceipts["state"]; readonly complete: boolean; readonly error: string | null; readonly readAt: string | null };
  readonly reload: () => void;
} {
  const read = useRpc(readLifecycleReview);
  const [receipts, setReceipts] = useState<ProjectionReceipts>(() => emptyReceipts(hostId));
  const receiptsRef = useRef(receipts);
  receiptsRef.current = receipts;
  const live = useRef(false), owner = useRef(hostId), token = useRef(0);
  owner.current = hostId;
  const load = useCallback(async () => {
    const host = hostId, current = ++token.current;
    const stale = () => !live.current || owner.current !== host || token.current !== current;
    if (stale()) return;
    setReceipts((value) => value.hostId === host ? { ...value, state: "reading", error: null } : value);
    try {
      const latest = await read({});
      if (stale()) return;
      if (latest && !Array.isArray(latest.unresolved)) {
        setReceipts((value) => value.hostId === host ? { ...value, state: "unknown", error: "The host did not report its current archive fences.", readAt: new Date().toISOString() } : value);
        return;
      }
      const refs = latest?.unresolved ?? [];
      const prior = receiptsRef.current.hostId === host ? receiptsRef.current : emptyReceipts(host);
      const reviews: Record<string, LifecycleReview> = { ...prior.reviews };
      const unknown = new Set<string>();
      const pending = new Map<string, Promise<LifecycleReview | null>>();
      let partial = false;
      for (const ref of refs) {
        if (stale()) return;
        let promise = pending.get(ref.reviewId);
        if (!promise) {
          promise = read({ reviewId: ref.reviewId });
          pending.set(ref.reviewId, promise);
        }
        try {
          const review = await promise;
          if (stale()) return;
          if (review?.id === ref.reviewId && review.action === "archive") {
            reviews[review.id] = review;
            const row = review.rows.find((entry) => entry.id === ref.agentId);
            if (!row || !unresolvedArchive(row)) unknown.add(ref.agentId);
          } else {
            partial = true;
            unknown.add(ref.agentId);
          }
        } catch {
          if (stale()) return;
          partial = true;
          unknown.add(ref.agentId);
        }
      }
      const listed = new Set(refs.map((ref) => ref.reviewId));
      for (const reviewId of Object.keys(reviews)) if (!listed.has(reviewId)) delete reviews[reviewId];
      setReceipts((value) => value.hostId === host ? { hostId: host, state: partial ? "partial" : "ready", reviews, unknownIntent: unknown, error: null, readAt: new Date().toISOString() } : value);
    } catch (failure) {
      if (!stale()) setReceipts((value) => value.hostId === host ? { ...value, state: "unknown", error: failureText(failure) } : value);
    }
  }, [hostId, read]);
  useEffect(() => {
    live.current = true;
    owner.current = hostId;
    token.current += 1;
    setReceipts((value) => value.hostId === hostId ? value : emptyReceipts(hostId));
    if (input.enabled === false) return;
    void load();
    return () => { live.current = false; token.current += 1; };
  }, [hostId, input.enabled]);
  const projections = useMemo(() => {
    const hosted = receipts.hostId === hostId;
    const unknownAll = !hosted || receipts.state === "idle" || receipts.state === "reading" || receipts.state === "unknown";
    const inventoryObservedAt = input.directory.lastUpdatedAt === null ? null : new Date(input.directory.lastUpdatedAt).toISOString();
    const inventory: "complete" | "partial" | "unknown" = !input.directory.includeArchived ? "unknown" : input.directory.status === "ready" && input.directory.inventoryComplete === true ? "complete" : "partial";
    const coverage = { archiveInventory: inventory, observedAt: inventoryObservedAt,
      detail: `${input.directory.status}${input.directory.live ? ", live" : ", no live updates"}${input.directory.includeArchived ? ", archive scope requested" : ", archive scope not requested"}` };
    const summaryOf = (sourceId: string): ReadSourceSummary | null => input.sources?.find((source) => source.id === sourceId) ?? input.reads?.[sourceId]?.source ?? null;
    const sourceOf = (summary: ReadSourceSummary): ProjectionSource => {
      const readResult = input.reads?.[summary.id];
      return { id: `source:${summary.id}`, kind: summary.kind === "plan" ? "plan" : summary.kind === "jira" ? "jira" : "source",
        detail: `${summary.title} · ${readResult?.status ?? "not read"}`, observedAt: readResult?.readAt ?? null,
        revision: readResult?.revision ?? summary.expectedSha256 ?? null, fresh: readResult?.status === "ready" ? "current" : "unknown" };
    };
    const intentOf = (id: string): ArchiveIntentObservation => {
      if (hosted) for (const review of Object.values(receipts.reviews)) {
        const row = review.rows.find((entry) => entry.id === id);
        if (row && unresolvedArchive(row)) return { state: "unsettled", reviewId: review.id, sessionId: id, observedAt: review.createdAt, detail: `${row.outcome}: ${row.message ?? row.reason}` };
      }
      return unknownAll || hosted && receipts.unknownIntent.has(id)
        ? { state: "unknown", reviewId: null, sessionId: id, observedAt: null, detail: receipts.error ?? "Archive receipt coverage is unknown." }
        : { state: "none", reviewId: null, sessionId: id, observedAt: null, detail: "No unresolved archive receipt is recorded." };
    };
    const projections = new Map<string, SessionProjection>();
    for (const entry of input.entries) {
      const observedAt = inventoryObservedAt ?? entry.updatedAt, archive = nativeArchiveMembership(entry), availability = freshAvailability(entry);
      const nativeSource: ProjectionSource = { id: `native:${entry.id}`, kind: "native", detail: `native ${entry.status}; ${archive.archived ? "archived" : "not archived"}`, observedAt, revision: entry.updatedAt, fresh: "current" };
      const workClaims: ScopeClaim[] = [], roleClaims: ScopeClaim[] = [], evidenceConflicts: ProjectionConflict[] = [];
      const claim = (kind: ScopeClaim["kind"], value: string, binding: ScopeClaim["binding"], source: ProjectionSource): ScopeClaim => ({ kind, value, binding, source });
      if (entry.projectKey) workClaims.push(claim("project", entry.projectKey, "placement", { id: `placement:project:${entry.id}`, kind: "project", detail: `native project ${entry.projectKey}`, observedAt, revision: entry.updatedAt, fresh: "current" }));
      if (entry.workspaceId) workClaims.push(claim("workspace", entry.workspaceId, "placement", { id: `placement:workspace:${entry.id}`, kind: "workspace", detail: `native workspace ${entry.workspaceId}`, observedAt, revision: entry.updatedAt, fresh: "current" }));
      const label = (key: string, kind: ScopeClaim["kind"], target: ScopeClaim[]) => {
        const value = entry.labels[key];
        if (typeof value !== "string" || !value.trim()) return;
        target.push(claim(kind, value.trim(), "claimed", { id: `label:${key}:${entry.id}`, kind: "label", detail: `native label ${key}`, observedAt, revision: entry.updatedAt, fresh: "current" }));
      };
      for (const key of ["epic", "jira.epic", "origin.epic"]) label(key, "epic", workClaims);
      for (const key of ["ticket", "jira.issue"]) label(key, "ticket", workClaims);
      label("desk", "desk", workClaims);
      label("workstream", "workstream", workClaims);
      for (const key of ["role", "kontor.role"]) label(key, "role", roleClaims);
      const [titleRole = "", ...titleRest] = entry.title.split("•").map((part) => part.trim());
      if (titleRest.length > 0 && titleRole.length > 0) roleClaims.push(claim("role", titleRole, "claimed", { id: `title:${entry.id}`, kind: "title", detail: "native title prefix", observedAt, revision: entry.updatedAt, fresh: "current" }));
      const capturedEpic = entry.jira?.epics.get(entry.id), capturedAt = entry.jira?.observed[0] ?? null;
      if (capturedEpic === null) evidenceConflicts.push({ kind: "stale-or-mismatched-evidence", detail: "Historical Jira captures disagree about this session's epic; neither is a binding.", sources: [{ id: `jira:capture:${entry.id}`, kind: "jira", detail: "captured epics disagree", observedAt: capturedAt, revision: null, fresh: "unknown" }] });
      else if (capturedEpic) workClaims.push(claim("epic", capturedEpic, "claimed", { id: `jira:capture:${entry.id}`, kind: "jira", detail: "historical Jira capture", observedAt: capturedAt, revision: null, fresh: capturedAt ? "current" : "unknown" }));
      for (const sourceId of input.associations?.[entry.id] ?? []) {
        const summary = summaryOf(sourceId);
        if (summary?.epic) workClaims.push(claim("epic", summary.epic, "claimed", sourceOf(summary)));
      }
      if (entry.availability && (entry.availability.agentId !== entry.id || entry.availability.revision !== entry.updatedAt)) {
        evidenceConflicts.push({ kind: "stale-or-mismatched-evidence", detail: "An exact-ID read belongs to another native identity or revision.", sources: [{ id: `availability:${entry.id}`, kind: "native", detail: entry.availability.reason, observedAt: entry.availability.checkedAt, revision: entry.availability.revision, fresh: "stale" }] });
      }
      projections.set(entry.id, deriveSessionProjection({
        hostId, workClaims, roleClaims, evidenceConflicts, coverage, receiptObservedAt: hosted ? receipts.readAt : null, intent: intentOf(entry.id),
        session: { id: entry.id, status: entry.status, archived: archive.archived, archivedAt: entry.archivedAt ?? availability?.archivedAt ?? null,
          activeTurn: entry.activeTurn === true || entry.activeTurnStartedAt !== null && entry.activeTurnStartedAt !== undefined,
          pendingInteraction: entry.pendingPermissionCount > 0 || entry.attentionReason === "permission", title: entry.title,
          workspaceId: entry.workspaceId, projectKey: entry.projectKey, provider: entry.provider, model: entry.model, updatedAt: entry.updatedAt,
          observedAt },
        evidence: [nativeSource, ...(entry.jira ? [{ id: `jira:${entry.id}`, kind: "jira" as const, detail: "historical Jira capture", observedAt: entry.jira.observed[0] ?? null, revision: null, fresh: "unknown" as const }] : [])],
      }));
    }
    return projections;
  }, [hostId, input.entries, input.directory, input.sources, input.reads, input.associations, receipts]);
  const current = receipts.hostId === hostId ? receipts : emptyReceipts(hostId);
  return { projections, receipts: { state: current.state, complete: current.state === "ready", error: current.error, readAt: current.readAt }, reload: () => { void load(); } };
}
