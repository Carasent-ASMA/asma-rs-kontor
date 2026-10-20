import { useEffect, useState } from "react";
import { getPaseoClient } from "@getpaseo/plugin/client";
import type { PaseoApi, PaseoWorkspace } from "@getpaseo/client";

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
  const [state, setState] = useState<{ entries: PaseoWorkspace[]; error: string | null; readAt: string | null }>({ entries: [], error: null, readAt: null });
  useEffect(() => {
    const lifetime = new AbortController();
    let timer: ReturnType<typeof setTimeout> | null = null;
    async function refresh() {
      if (!online) { setState((current) => ({ ...current, error: "Workspace inventory stale: host offline" })); return; }
      try {
        const entries = await collectWorkspaces(getPaseoClient(hostId), lifetime.signal);
        if (!lifetime.signal.aborted) setState({ entries, error: null, readAt: new Date().toISOString() });
      } catch (error) {
        if (!lifetime.signal.aborted) setState((current) => ({ ...current, error: `Workspace inventory unavailable: ${error instanceof Error ? error.message : String(error)}` }));
      } finally { if (!lifetime.signal.aborted) timer = setTimeout(() => { void refresh(); }, 30_000); }
    }
    void refresh();
    return () => { lifetime.abort(); if (timer) clearTimeout(timer); };
  }, [hostId, online]);
  return state;
}
