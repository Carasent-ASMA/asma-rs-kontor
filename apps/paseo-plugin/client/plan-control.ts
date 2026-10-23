import type { PluginButtonRegistration, PluginClientContext } from "@getpaseo/plugin/client";
import { readPlanAssociations } from "../shared/read-sources";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { bindHostDirectory } from "./use-session-directory";
import { linkedPlans } from "./navigation-state";

type PlanAssociations = {
  readonly status: string;
  readonly associations: Readonly<Record<string, readonly string[]>>;
  readonly sources: readonly { readonly id: string }[];
};

/** Host-local explicit associations only; an agent title is never consulted. */
export function contributePlanControl(client: PluginClientContext, screenId: string): () => void {
  if (typeof client.rpc !== "function") return () => {};
  const pills = new Map<string, { workspaceId: string; registration: PluginButtonRegistration }>();
  const lifetime = new AbortController();
  let timer: ReturnType<typeof setTimeout> | null = null;
  let associations: PlanAssociations | null = null;
  let entries: readonly DirectoryEntry[] = [];
  let complete = false;
  // Hermes evaluates plugin bundles with function-scoped let/const, so a press handler made in the entry loop would
  // open the last entry's plan. Each entry's IDs reach its handler only as this call's arguments.
  const openPlan = (agentId: string, ids: readonly string[]) => () => { client.openScreen({ screenId, params: { tab: "Plan", agentId, ...(ids.length === 1 ? { sourceId: ids[0]! } : {}) } }); };
  const removeAll = () => { for (const pill of pills.values()) pill.registration.remove(); pills.clear(); };
  const apply = () => {
    if (lifetime.signal.aborted) return;
    if (!associations || associations.status !== "ready") { removeAll(); return; }
    const catalog = new Set(associations.sources.map((source) => source.id));
    const seen = new Set<string>();
    for (const entry of entries) {
      const ids = linkedPlans(associations.associations, entry.id, catalog);
      if (!ids.length || !entry.workspaceId || entry.status === "closed") continue;
      seen.add(entry.id);
      // Updating a descriptor avoids duplicate registrations on settings refresh.
      const contribution = { id: "open-linked-plan", workspaceId: entry.workspaceId, agentId: entry.id,
        button: { title: "Open plan", icon: "BookOpen", label: "Open plan", behavior: { kind: "action" as const, onPress: openPlan(entry.id, ids) } } };
      const current = pills.get(entry.id);
      if (current?.workspaceId === entry.workspaceId) current.registration.update(contribution.button);
      else { current?.registration.remove(); pills.set(entry.id, { workspaceId: entry.workspaceId, registration: client.addComposerPill(contribution) }); }
    }
    // The shared directory supplies sessions; it is walked once per host. Only
    // its complete inventory can retire a pill that is no longer associated.
    if (complete) for (const [id, pill] of pills) if (!seen.has(id)) { pill.registration.remove(); pills.delete(id); }
  };
  const releaseDirectory = bindHostDirectory((state: DirectoryState) => { entries = state.entries; complete = state.status === "ready" && state.inventoryComplete === true; apply(); });
  async function reconcile() {
    try {
      associations = await client.rpc(readPlanAssociations, {});
      if (lifetime.signal.aborted) return;
      apply();
    } catch (error) {
      if (!lifetime.signal.aborted) { removeAll(); console.error("Kontor plan associations unavailable", error); }
    } finally { if (!lifetime.signal.aborted) timer = setTimeout(() => { void reconcile(); }, 15_000); }
  }
  void reconcile();
  return () => { lifetime.abort(); if (timer) clearTimeout(timer); releaseDirectory(); removeAll(); };
}
