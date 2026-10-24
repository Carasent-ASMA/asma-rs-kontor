import { useEffect, useState } from "react";
import { getPaseoClient } from "@getpaseo/plugin/client";
import type { PaseoApi, PaseoProject, PaseoWorkspace } from "@getpaseo/client";

export async function collectWorkspaces(client: Pick<PaseoApi, "workspaces">, signal: AbortSignal): Promise<PaseoWorkspace[]> {
  const rows = new Map<string, PaseoWorkspace>();
  const cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1000; page += 1) {
    if (signal.aborted) throw new Error("Workspace read canceled");
    const result = await client.workspaces.list({ page: { limit: 200, ...(cursor ? { cursor } : {}) } });
    for (const row of result.entries) rows.set(row.id, row);
    if (!result.pageInfo.hasMore) return [...rows.values()];
    const next = result.pageInfo.nextCursor;
    if (!next || cursors.has(next)) throw new Error("Workspace inventory has an invalid page cursor");
    cursors.add(next); cursor = next;
  }
  throw new Error("Workspace inventory exceeded the page limit");
}
const REFRESH_MS = 30_000;
type WorkspaceDirectory = {
  entries: PaseoWorkspace[]; error: string | null; readAt: string | null;
  projects: PaseoProject[]; projectError: string | null; projectsReadAt: string | null;
};
const EMPTY: WorkspaceDirectory = { entries: [], error: null, readAt: null, projects: [], projectError: null, projectsReadAt: null };
/** The last state per host for this plugin runtime; `fresh` is when both lists were last read in full. */
const retained = new Map<string, { state: WorkspaceDirectory; fresh: number | null }>();
export function forgetWorkspaceDirectories(): void { retained.clear(); }

/** Snapshot coverage is labelled independently from the live session subscription. */
export function useWorkspaceDirectory(hostId: string, online: boolean) {
  const [state, setState] = useState<WorkspaceDirectory>(() => retained.get(hostId)?.state ?? EMPTY);
  useEffect(() => {
    const lifetime = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    // A remount keeps the poll's cadence: a full read under 30 s old, shown with its own read time, is not read again.
    const publish = (change: (current: WorkspaceDirectory) => WorkspaceDirectory, fresh: number | null) => {
      const next = change(retained.get(hostId)?.state ?? EMPTY);
      retained.set(hostId, { state: next, fresh });
      setState(() => next);
    };
    async function refresh() {
      if (!online) {
        publish((current) => ({ ...current, error: "Workspace inventory stale: host offline", projectError: "Project inventory stale: host offline" }), null);
        return;
      }
      try {
        const client = getPaseoClient(hostId);
        // A desk may have no workspace at all. Read the native project registry independently.
        const [workspaces, projects] = await Promise.allSettled([
          collectWorkspaces(client, lifetime.signal), client.projects.list(),
        ]);
        if (lifetime.signal.aborted) return;
        const readAt = new Date().toISOString();
        publish((current) => ({
          ...current,
          ...(workspaces.status === "fulfilled"
            ? { entries: workspaces.value, error: null, readAt }
            : { error: `Workspace inventory unavailable: ${String(workspaces.reason)}` }),
          ...(projects.status === "fulfilled"
            ? { projects: projects.value.projects, projectError: null, projectsReadAt: readAt }
            : { projectError: `Project inventory unavailable: ${String(projects.reason)}` }),
        }), workspaces.status === "fulfilled" && projects.status === "fulfilled" ? Date.now() : null);
      } catch (error) {
        if (!lifetime.signal.aborted) publish((current) => ({ ...current, error: `Workspace inventory unavailable: ${String(error)}`, projectError: `Project inventory unavailable: ${String(error)}` }), null);
      } finally { if (!lifetime.signal.aborted) timer = setTimeout(() => { void refresh(); }, REFRESH_MS); }
    }
    const cached = retained.get(hostId);
    setState(() => cached?.state ?? EMPTY);
    const wait = online && cached?.fresh != null ? cached.fresh + REFRESH_MS - Date.now() : 0;
    if (wait > 0) timer = setTimeout(() => { void refresh(); }, wait);
    else void refresh();
    return () => { lifetime.abort(); if (timer) clearTimeout(timer); };
  }, [hostId, online]);
  return state;
}
