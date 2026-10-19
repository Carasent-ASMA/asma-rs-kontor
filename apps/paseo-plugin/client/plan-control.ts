import type { PluginButtonRegistration, PluginClientContext } from "@getpaseo/plugin/client";
import { readPlanAssociations } from "../shared/read-sources";
import { collectDirectory } from "./session-directory";
import { linkedPlans } from "./navigation-state";

/** Host-local explicit associations only; an agent title is never consulted. */
export function contributePlanControl(client: PluginClientContext, screenId: string): () => void {
  if (typeof client.rpc !== "function") return () => {};
  const pills = new Map<string, { workspaceId: string; registration: PluginButtonRegistration }>();
  const lifetime = new AbortController();
  let timer: ReturnType<typeof setTimeout> | null = null;
  const removeAll = () => { for (const pill of pills.values()) pill.registration.remove(); pills.clear(); };
  async function reconcile() {
    try {
      const associations = await client.rpc(readPlanAssociations, {});
      if (lifetime.signal.aborted) return;
      if (associations.status !== "ready") { removeAll(); return; }
      const catalog = new Set(associations.sources.map((source) => source.id));
      const directory = await collectDirectory(client.paseo, false, lifetime.signal);
      if (lifetime.signal.aborted) return;
      const seen = new Set<string>();
      for (const entry of directory.entries) {
        const ids = linkedPlans(associations.associations, entry.id, catalog);
        if (!ids.length || !entry.workspaceId || entry.status === "closed") continue;
        seen.add(entry.id);
        // Updating a descriptor avoids duplicate registrations on settings refresh.
        const contribution = { id: "open-linked-plan", workspaceId: entry.workspaceId, agentId: entry.id,
          button: { title: "Open plan", icon: "BookOpen", label: "Open plan", behavior: { kind: "action" as const,
            onPress() { client.openScreen({ screenId, params: { tab: "Plan", agentId: entry.id, ...(ids.length === 1 ? { sourceId: ids[0]! } : {}) } }); } } } };
        const current = pills.get(entry.id);
        if (current?.workspaceId === entry.workspaceId) current.registration.update(contribution.button);
        else { current?.registration.remove(); pills.set(entry.id, { workspaceId: entry.workspaceId, registration: client.addComposerPill(contribution) }); }
      }
      for (const [id, pill] of pills) if (!seen.has(id)) { pill.registration.remove(); pills.delete(id); }
    } catch (error) {
      if (!lifetime.signal.aborted) { removeAll(); console.error("Kontor plan associations unavailable", error); }
    } finally { if (!lifetime.signal.aborted) timer = setTimeout(() => { void reconcile(); }, 15_000); }
  }
  void reconcile();
  return () => { lifetime.abort(); if (timer) clearTimeout(timer); removeAll(); };
}
