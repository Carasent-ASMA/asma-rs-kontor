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
/** Snapshot coverage is labelled independently from the live session subscription. */
export function useWorkspaceDirectory(hostId: string, online: boolean) {
  const [state, setState] = useState<{
    entries: PaseoWorkspace[]; error: string | null; readAt: string | null;
    projects: PaseoProject[]; projectError: string | null; projectsReadAt: string | null;
  }>({ entries: [], error: null, readAt: null, projects: [], projectError: null, projectsReadAt: null });
  useEffect(() => {
    const lifetime = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    async function refresh() {
      if (!online) {
        setState((current) => ({ ...current, error: "Workspace inventory stale: host offline", projectError: "Project inventory stale: host offline" }));
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
        setState((current) => ({
          ...current,
          ...(workspaces.status === "fulfilled"
            ? { entries: workspaces.value, error: null, readAt }
            : { error: `Workspace inventory unavailable: ${String(workspaces.reason)}` }),
          ...(projects.status === "fulfilled"
            ? { projects: projects.value.projects, projectError: null, projectsReadAt: readAt }
            : { projectError: `Project inventory unavailable: ${String(projects.reason)}` }),
        }));
      } catch (error) {
        if (!lifetime.signal.aborted) setState((current) => ({ ...current, error: `Workspace inventory unavailable: ${String(error)}`, projectError: `Project inventory unavailable: ${String(error)}` }));
      } finally { if (!lifetime.signal.aborted) timer = setTimeout(() => { void refresh(); }, 30_000); }
    }
    void refresh();
    return () => { lifetime.abort(); if (timer) clearTimeout(timer); };
  }, [hostId, online]);
  return state;
}
