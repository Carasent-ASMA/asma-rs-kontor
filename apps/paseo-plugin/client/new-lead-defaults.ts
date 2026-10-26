import type { CreateProvider, FeatureValues } from "../shared/session-create";
import type { NewLeadDefault } from "../shared/organization";

/** The provider whose own catalog default a new lead starts with. */
export const CODEX = "codex";

/** A model for one lead: what the picker returns and what Make default saves. */
export type LeadModel = { readonly provider: string; readonly model: string; readonly effort: string | null; readonly featureValues?: FeatureValues };

/** Codex's configured default: the model its catalog flags, at that model's default effort. Null when Codex is not available or flags none. */
export function codexDefault(providers: readonly CreateProvider[]): LeadModel | null {
  const codex = providers.find((entry) => entry.id === CODEX && entry.available);
  const model = codex?.models.find((entry) => entry.id === codex.defaultModel);
  return model ? { provider: CODEX, model: model.id, effort: model.defaultEffort } : null;
}

/** A saved choice counts only while this host offers its provider, model and effort. */
function offered(providers: readonly CreateProvider[], choice: LeadModel): boolean {
  const model = providers.find((entry) => entry.id === choice.provider && entry.available)?.models.find((entry) => entry.id === choice.model);
  return !!model && (model.efforts.length ? model.efforts.some((effort) => effort.id === choice.effort) : choice.effort === null);
}

/**
 * The model a new lead starts with when the operator chose none: the saved new-lead default while this host offers it,
 * else Codex's own default. A role profile or the first listed model never stands in, and a saved default that is gone is named.
 */
export function leadDefault(providers: readonly CreateProvider[], saved: NewLeadDefault | null | undefined): { choice: LeadModel | null; saved: boolean; note: string | null } {
  if (saved && offered(providers, saved)) return { choice: saved, saved: true, note: null };
  const codex = codexDefault(providers);
  const note = saved ? `The saved new-lead default ${saved.model} is not offered here now, so ${codex ? "this lead starts on the Codex default" : "choose a model"}.` : null;
  return { choice: codex, saved: false, note };
}
