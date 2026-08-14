/**
 * What a seat is allowed to be, and how that is checked.
 *
 * A team template declares slots; a slot declares its *capabilities* — the four
 * rungs of its model chain, the context class it runs under, the working set it
 * says it needs, and the authority it holds. This module is the whole rule set
 * for those declarations, as pure functions over a catalog. No React, no fetch,
 * no clock: everything here is a statement about data that was handed in.
 *
 * # Why nothing here knows a provider by name
 *
 * The rules read like they are about specific providers — "a Claude rung may not
 * be followed by another Claude rung", "a Cursor seat may not issue a verdict" —
 * and implementing them that way would have been shorter. It would also have
 * been wrong the first time a deployment ran a provider nobody wrote this for.
 *
 * So the *reasons* are data on the catalog rather than names in the code:
 * `ProviderEntry.pooledUsage` says every model on a provider draws from one
 * quota, which is what makes a fallback within it worthless; `ModelEntry.
 * degradedLane` says a route is cheap enough to work on but not to judge with;
 * `ModelEntry.efforts` says which effort levels the route actually exposes, so a
 * route that exposes none forces `unset` without any code recognizing it.
 *
 * # Why every verifiable cell carries its own provenance
 *
 * A research gate can only be as good as the fixture's ability to record its
 * verdict. The first pass hardened the one field that had a provenance marker
 * (`ModelPriceTier.source`) and left every field that did not — so windows,
 * effort ladders, charging bases and need bands were unflagged capability
 * claims, three of which drive a *blocking* rule. A ceiling nobody verified was
 * refusing publishes with the authority of a catalog read.
 *
 * Every such cell now carries a `Provenance`: what state it is in, which gate
 * record promoted it, what was read, and when. Promotion without a review
 * reference is itself a validation failure — see `provenanceIssues`.
 *
 * @see /Users/igor/kon-mvp-20-scratch/evidence/kontor-teams/COMMITTEE-RECORD.md §5
 * @see _docs/ai-orchestration/analysis/2026-08-14-11-35-analysis-kontor-teams-capability-ui.md
 * @see _docs/ai-orchestration/architecture/2026-08-05-01-07-architecture-agent-fleet-roles-model-policy.md
 * @see _docs/ai-orchestration/architecture/2026-08-13-09-43-architecture-seat-context-window-compaction-policy.md
 */

/* -------------------------------------------------------------- provenance */

/** How well established one value is. */
export type ProvenanceState =
  /** Read from the runtime this session; re-checkable by repeating the call. */
  | 'live'
  /** Established from a cited source and reviewed by a gate. */
  | 'researched'
  /** A placeholder nobody has verified. Never presented as fact. */
  | 'fixture/needs-verification'

/**
 * Where one value came from.
 *
 * Attached to every cell a gate can rule on — price tiers, context ceilings,
 * effort ladders, charging bases and need bands. The point is that a verdict
 * lives *next to the value*, so the next reader can tell "checked and true" from
 * "never checked" without leaving the file.
 */
export interface Provenance {
  /** What state this value is in. */
  readonly state: ProvenanceState
  /** The gate record this cell was promoted under. Null while unpromoted. */
  readonly reviewRef: string | null
  /** Where the value was read: a provider page URL, or the runtime call. */
  readonly citation: string | null
  /** When it was observed. A price with no date is a price with no meaning. */
  readonly observedAt: string | null
}

/** One value and the provenance of that value. */
export interface Sourced<T> {
  /** The value itself. */
  readonly value: T
  /** How it got here. */
  readonly provenance: Provenance
}

/** The gate record that authorised this fixture's promotions. */
export const GATE_RECORD = 'KON-MVP-25-GATE-2026-08-14-02'

/** The provenance of a value nobody has verified. */
export const UNVERIFIED: Provenance = {
  state: 'fixture/needs-verification',
  reviewRef: null,
  citation: null,
  observedAt: null,
}

/** A value nobody has verified. */
export function unverified<T>(value: T): Sourced<T> {
  return { value, provenance: UNVERIFIED }
}

/** A value read from the runtime, under a gate record. */
export function liveValue<T>(value: T, citation: string, observedAt: string): Sourced<T> {
  return { value, provenance: { state: 'live', reviewRef: GATE_RECORD, citation, observedAt } }
}

/** Whether a provenance claims more than "nobody checked". */
export function isPromoted(provenance: Provenance): boolean {
  return provenance.state !== 'fixture/needs-verification'
}

/**
 * What a provenance record has to satisfy to claim what it claims.
 *
 * A promotion without a review reference is the failure this record exists to
 * prevent: it asserts a value was established without saying by whom, so the
 * claim cannot be re-checked or withdrawn. The same goes for a promotion with no
 * citation (nothing to re-read) and no observation date — a rate captured inside
 * a promotional window and stored undated is not a weaker fact, it is a false
 * one.
 */
export function provenanceIssues(subject: string, provenance: Provenance): readonly Issue[] {
  if (!isPromoted(provenance)) {
    return []
  }
  const issues: Issue[] = []
  // Blank is absent. A served `/v1/catalog` will fill these from somewhere, and
  // an empty string is what a missing column looks like after it has been
  // through JSON — treating it as a reference would let a promotion satisfy the
  // check by carrying nothing at all.
  const stated = (field: string | null): boolean => field !== null && field.trim() !== ''
  if (!stated(provenance.reviewRef)) {
    issues.push({
      severity: 'blocking',
      code: 'promotion_without_review_ref',
      message: `${subject} claims state "${provenance.state}" with no review reference. A promotion nobody signed cannot be re-checked or withdrawn.`,
    })
  }
  if (!stated(provenance.citation)) {
    issues.push({
      severity: 'blocking',
      code: 'promotion_without_citation',
      message: `${subject} claims state "${provenance.state}" without saying what was read.`,
    })
  }
  if (!stated(provenance.observedAt)) {
    issues.push({
      severity: 'blocking',
      code: 'promotion_without_date',
      message: `${subject} claims state "${provenance.state}" with no observation date.`,
    })
  }
  return issues
}

/**
 * Check every promoted cell in a catalog: no cell may claim to be established
 * without carrying the record that established it.
 *
 * `TeamsView` invokes this before any catalog-backed control or draft renders.
 * A promoted cell missing its review reference, citation or observation date
 * therefore refuses the complete `/v1/catalog` payload at the trust boundary;
 * the operator sees the catalog issues and cannot edit or publish against it.
 * Need-band provenance is then enforced per draft by `reviewSeat` and
 * `validateTeam`.
 *
 * @see /Users/igor/kon-mvp-20-scratch/evidence/kontor-teams/RE-GATE-RECORD.md §4.1 (F1)
 */
export function validateCatalog(catalog: ModelCatalog): readonly Issue[] {
  const issues: Issue[] = []
  for (const provider of catalog.providers) {
    issues.push(...provenanceIssues(`provider "${provider.id}" basis`, provider.basis.provenance))
  }
  for (const model of catalog.models) {
    const ref = `${model.provider}/${model.id}`
    issues.push(...provenanceIssues(`${ref} contextWindow`, model.contextWindow.provenance))
    issues.push(...provenanceIssues(`${ref} efforts`, model.efforts.provenance))
    for (const tier of model.pricing) {
      issues.push(...provenanceIssues(`${ref} price tier @${tier.window}`, tier.provenance))
    }
  }
  return issues
}

/* ------------------------------------------------------------------ effort */

/**
 * The effort levels the schema admits.
 *
 * **This is the raw runtime ladder, not a normalisation over one.** The gate
 * required that choice to be made explicitly rather than inherited, and this is
 * it: every id here is a `thinkingOptions` id the runtime actually returns, and
 * a route's exposed set is a subset of this list. The alternative — normalising
 * provider ladders onto a common scale — was rejected because the map would have
 * to assert that one provider's `ultra` and another's `ultracode` are the same
 * rung, which nothing establishes.
 *
 * The consequence of the earlier closed list was a real capability *denied*:
 * `ultra` is exposed by the fleet's primary architect routes and `ultracode` by
 * both Claude routes, and an editor restricted to "only effort values the route
 * exposes" would have refused a legitimate pin on either.
 *
 * Declaration order is not a ranking. `ultra` and `ultracode` are different
 * providers' top rungs and nothing here claims they are comparable.
 */
export const EFFORT_LEVELS = [
  'off',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
  'ultra',
  'ultracode',
] as const

/** One effort level. */
export type EffortLevel = (typeof EFFORT_LEVELS)[number]

/**
 * What a rung pins as its effort.
 *
 * `unset` is a declaration, not an absence: it says this route exposes no effort
 * lever, so pinning one would be a silent no-op.
 */
export type RungEffort = EffortLevel | 'unset'

/* ----------------------------------------------------------------- context */

/**
 * The five context classes, and the token threshold each one triggers at.
 *
 * The set is closed and the numbers are not this console's to change: they come
 * from the seat context policy, and moving one needs that document's evidence
 * bar rather than a plausible-sounding argument in a component. `native` names
 * no number at all — it is the runtime's own default, which is why it is an
 * explicit escape hatch rather than a bigger class.
 */
export const CONTEXT_CLASSES = [
  { id: 'lean', target: 128_000 },
  { id: 'standard', target: 256_000 },
  { id: 'deep', target: 512_000 },
  { id: 'extended', target: 720_000 },
  { id: 'native', target: null },
] as const

/** One context class. */
export type ContextClass = (typeof CONTEXT_CLASSES)[number]['id']

/** The classes that name a number, smallest first. */
const RANKED_CLASSES = CONTEXT_CLASSES.filter((entry) => entry.target !== null)

/**
 * The classes a seat may not fall into by default.
 *
 * `extended` and `native` are explicit-only: a work profile, a role slot or an
 * authorized run override has to ask for them. A slot declaring one *is* that
 * explicitness, so the editor notes it rather than refusing it.
 */
const EXPLICIT_ONLY: readonly ContextClass[] = ['extended', 'native']

/** How hard a class is enforced when the model cannot deliver it. */
export type ContextEnforcement =
  /** Clamp to what the model can do, visibly, and carry on. */
  | 'best_effort'
  /** Refuse rather than run a seat that is not getting what it asked for. */
  | 'required'

/** A seat's context declaration. */
export interface ContextPolicy {
  /** Which class, from the closed set. */
  readonly class: ContextClass
  /** What happens when the model cannot honour it. */
  readonly enforcement: ContextEnforcement
}

/** The trigger target a class declares, or `null` for the runtime's own default. */
export function classTarget(cls: ContextClass): number | null {
  return CONTEXT_CLASSES.find((entry) => entry.id === cls)?.target ?? null
}

/* ----------------------------------------------------------------- catalog */

/**
 * One price step on a model's curve.
 *
 * `window` is the prompt size the step applies *from*, and the rate then applies
 * to the **whole request**, not only to the tokens above the boundary. That is
 * the shape the providers in this fleet publish; a provider that charged
 * marginally would need a different field, because computing it this way would
 * be a silent ~2x error in the one column that becomes a budget.
 */
export interface ModelPriceTier {
  /** The prompt size this rate applies from, in tokens. */
  readonly window: number
  /** Dollars per million input tokens, for the whole request. */
  readonly inputPerMtok: number
  /** Dollars per million output tokens. */
  readonly outputPerMtok: number
  /** Where these rates came from. */
  readonly provenance: Provenance
}

/** One model, as the catalog reports it. */
export interface ModelEntry {
  /** The route id, exactly as the runtime spells it. */
  readonly id: string
  /** What to show a reader. */
  readonly label: string
  /** The provider this route belongs to. */
  readonly provider: string
  /** Whether the provider serves this route as its default. */
  readonly isDefault: boolean
  /**
   * The physical ceiling, or `null` when nothing can be proven about it.
   *
   * `null` is not zero and not "small": it means nothing is established, which
   * is a different answer from any number. A live read that returns no window is
   * a *positive* finding and is recorded as `live` with a `null` value.
   */
  readonly contextWindow: Sourced<number | null>
  /**
   * The effort levels this route actually exposes.
   *
   * Empty means the lever does not exist there. Pinning effort on such a route
   * is a silent no-op, so the schema requires `unset` instead.
   */
  readonly efforts: Sourced<readonly EffortLevel[]>
  /** The price curve, cheapest step first. */
  readonly pricing: readonly ModelPriceTier[]
  /**
   * Whether a seat on this route is running degraded.
   *
   * A degraded route may do work and report everything it found. It may not
   * issue a verdict — that is the counterweight to never making a ticket wait
   * for a model.
   */
  readonly degradedLane: boolean
}

/**
 * What a seat on this provider actually spends.
 *
 * Not the same question as where a price came from, which is what a
 * `Provenance` answers. A rate can be perfectly well sourced from the provider's
 * own page and still be the wrong number to show an operator, because on a plan
 * the marginal cost of a wider context inside the window is zero dollars — it is
 * a share of something already bought. Collapsing the two turns a true list
 * price into a false budget line.
 *
 * @see _docs/ai-orchestration/analysis/2026-08-14-11-35-analysis-kontor-teams-capability-ui.md:77
 */
export type ChargingBasis =
  /** Tokens deduct from a balance. Dollars are the real unit. */
  | 'metered'
  /** A plan window. Inside it the marginal cost is nothing; past it, a refusal. */
  | 'plan_allowance'
  /** Included in a plan at token rates, until the plan's own ceiling. */
  | 'included_usage'
  /** Requests are the scarce thing; tokens are free and capped. */
  | 'request_quota'

/** One provider, as the catalog reports it. */
export interface ProviderEntry {
  /** The provider id, exactly as the runtime spells it. */
  readonly id: string
  /** What to show a reader. */
  readonly label: string
  /** What widening a seat's context here actually consumes. */
  readonly basis: Sourced<ChargingBasis>
  /**
   * The enabled runtime provider this one is reached *through*, when it is not
   * itself one.
   *
   * A vendor is not a provider. Some vendors in this catalog are dispatched
   * through another runtime provider entirely, which means a charging basis
   * declared against the vendor is an assertion about an entity the runtime does
   * not have — on the axis that decides whether a dollar prints. Recording the
   * routing keeps that visible instead of implied.
   */
  readonly reachedVia: string | null
  /**
   * Whether every model here draws from one quota.
   *
   * When it does, a fallback from one of its models to another protects nothing:
   * the rung below is blocked by the same exhaustion that blocked the rung
   * above.
   */
  readonly pooledUsage: boolean
}

/** Everything the editor is allowed to offer. */
export interface ModelCatalog {
  /** The providers. */
  readonly providers: readonly ProviderEntry[]
  /** The models, across every provider. */
  readonly models: readonly ModelEntry[]
}

/**
 * One route, by the pair that actually identifies it.
 *
 * Model ids are unique only *within* a provider. This runtime serves
 * `claude-opus-5`, `claude-fable-5`, `gpt-5.6-sol`, `gpt-5.6-terra`,
 * `gpt-5.6-luna` and `gpt-5.4-mini` under Cursor *as well as* under Claude and
 * Codex — the same id, a different contract, a different charging basis and a
 * different verdict authority. A lookup keyed on the id alone returns whichever
 * happens to be first in the array, which means a valid rung is reported as a
 * provider mismatch, a degraded lane is granted verdict authority it does not
 * have, and a price is attached to a row that cannot say which contract it
 * describes.
 */
export function modelById(
  catalog: ModelCatalog,
  provider: string,
  id: string,
): ModelEntry | undefined {
  return catalog.models.find((model) => model.provider === provider && model.id === id)
}

/** The route one rung names, or `undefined` when the catalog serves no such pair. */
export function modelForRung(catalog: ModelCatalog, rung: ModelRung): ModelEntry | undefined {
  return modelById(catalog, rung.provider, rung.model)
}

/** One provider by id. */
export function providerById(catalog: ModelCatalog, id: string): ProviderEntry | undefined {
  return catalog.providers.find((provider) => provider.id === id)
}

/** The models one provider serves, in catalog order. */
export function modelsOf(catalog: ModelCatalog, provider: string): readonly ModelEntry[] {
  return catalog.models.filter((model) => model.provider === provider)
}

/* -------------------------------------------------------------- seat shape */

/** One rung of a model chain. */
export interface ModelRung {
  /** The provider. */
  readonly provider: string
  /** The route on it. */
  readonly model: string
  /** The effort pinned on this rung. */
  readonly effort: RungEffort
}

/** How large a working set a seat says its work accumulates. */
export interface SeatNeedBand {
  /** The tokens it actually needs. */
  readonly minTokens: number
  /** Why, in the operator's own words. */
  readonly rationale: string | null
  /**
   * Where the number came from.
   *
   * Every band in the seeded fixture is unverified, and the gate was explicit
   * about why: the numbers were back-formed from the class table they are used
   * to justify, so the recommendation they feed cannot disagree with the seed it
   * was read off. A band table that cannot disagree with the class table is not
   * evidence, and until one is derived from telemetry this field says so.
   */
  readonly provenance: Provenance
}

/** Everything one slot declares. */
export interface SeatCapabilities {
  /** One to four rungs, rung 1 first. */
  readonly chain: readonly ModelRung[]
  /** The context class and how hard it is enforced. */
  readonly context: ContextPolicy
  /** The working set this seat says it needs. */
  readonly need: SeatNeedBand
  /** Optional task/run override; when larger, it is the effective need. */
  readonly taskMinimum?: SeatNeedBand
  /** Latest receipt id from the realm projection, when one exists. */
  readonly latestReceipt?: string
  /** Opaque skill keys. */
  readonly skills: readonly string[]
  /** Opaque gate keys this seat may evaluate. */
  readonly mayEvaluate: readonly string[]
  /** Opaque gate keys this seat may waive. */
  readonly mayWaive: readonly string[]
}

/** One slot in a team template. */
export interface TeamSlot {
  /** The slot key. Opaque — nothing here interprets it. */
  readonly id: string
  /** What the seat in it may be. */
  readonly capabilities: SeatCapabilities
}

/** One unpublished team template. */
export interface TeamDraft {
  /** The draft key. Opaque. */
  readonly id: string
  /** What an operator calls it. */
  readonly name: string
  /** Its slots. */
  readonly slots: readonly TeamSlot[]
  /** Realm-resolved preview at the projection cursor, absent while edited locally. */
  readonly resolvedPolicy?: readonly ResolvedPolicyProjection[]
}

/** One server-resolved context policy row shared by API, CLI, MCP and console. */
export interface ResolvedPolicyProjection {
  readonly slot: string
  readonly class: ContextClass
  readonly source: 'role_slot' | 'run_override'
  readonly effective_threshold: number | null
  readonly enforcement: ContextEnforcement
  readonly capability: 'supported' | 'clamped' | 'unsupported'
  readonly latest_receipt: string | null
}

/** One immutable published snapshot of a logical team template. */
export interface TeamRevision extends TeamDraft {
  /** Monotonic revision within the logical template id. */
  readonly version: number
}

/** Publish the next immutable revision without mutating either input. */
export function publishTeamRevision(
  revisions: readonly TeamRevision[],
  draft: TeamDraft,
): TeamRevision {
  const previous = revisions
    .filter((revision) => revision.id === draft.id)
    .reduce((highest, revision) => Math.max(highest, revision.version), 0)
  return structuredClone({ ...draft, version: previous + 1 })
}

/* ------------------------------------------------------------------ issues */

/** Whether an issue stops a publish or only has to be seen. */
export type IssueSeverity =
  /** The declaration may not be published as it stands. */
  | 'blocking'
  /** The declaration is publishable and something about it must still be said. */
  | 'notice'

/** One thing wrong, or worth saying, about a declaration. */
export interface Issue {
  /** Whether it blocks. */
  readonly severity: IssueSeverity
  /** A stable key for the rule. Rendered as itself. */
  readonly code: string
  /** What the rule found, in a sentence. */
  readonly message: string
  /** Which rung it is about, 1-based, when it is about one. */
  readonly rung?: number
  /** The slot this issue belongs to, when it is slot-scoped. */
  readonly slot?: string
}

/** Whether any of a set of issues blocks. */
export function blocks(issues: readonly Issue[]): boolean {
  return issues.some((issue) => issue.severity === 'blocking')
}

/* ------------------------------------------------------------------- chain */

/** The most rungs a chain may declare. */
export const MAX_RUNGS = 4

/**
 * Check one model chain against the catalog.
 *
 * The rules, in the order they are reported:
 *
 * - a chain declares between one and `MAX_RUNGS` rungs;
 * - every rung names a `(provider, route)` pair the catalog serves;
 * - a rung's effort is one the route exposes, and is `unset` exactly when the
 *   route exposes none;
 * - rung 2 crosses providers — a fallback onto the provider that just ran out
 *   is not a fallback;
 * - a provider repeated on adjacent rungs is flagged, and *blocked* when that
 *   provider pools its quota, because then the second rung is unreachable
 *   whenever the first one is.
 */
export function validateChain(
  chain: readonly ModelRung[],
  catalog: ModelCatalog,
): readonly Issue[] {
  const issues: Issue[] = []

  if (chain.length === 0) {
    issues.push({
      severity: 'blocking',
      code: 'chain_empty',
      message: 'A seat names at least one rung; a chain with none names no model at all.',
    })
  }
  if (chain.length > MAX_RUNGS) {
    issues.push({
      severity: 'blocking',
      code: 'chain_too_long',
      message: `A chain is at most ${MAX_RUNGS} rungs; this one declares ${chain.length}.`,
    })
  }

  chain.forEach((rung, index) => {
    const rungNumber = index + 1
    if (!providerById(catalog, rung.provider)) {
      issues.push({
        severity: 'blocking',
        code: 'unknown_provider',
        rung: rungNumber,
        message: `The catalog serves no provider "${rung.provider}".`,
      })
    }
    // Keyed on the pair: the same id under another provider is another route
    // with another contract, and resolving to it would judge the wrong thing.
    const model = modelForRung(catalog, rung)
    if (!model) {
      issues.push({
        severity: 'blocking',
        code: 'unknown_model',
        rung: rungNumber,
        message: `The catalog serves no route "${rung.provider}/${rung.model}".`,
      })
      return
    }
    const exposed = model.efforts.value
    if (rung.effort === 'unset') {
      if (exposed.length > 0) {
        issues.push({
          severity: 'blocking',
          code: 'effort_unset',
          rung: rungNumber,
          message: `"${model.id}" exposes ${exposed.join(', ')}; an unpinned rung falls through to whatever the route defaults to.`,
        })
      }
    } else if (!exposed.includes(rung.effort)) {
      issues.push({
        severity: 'blocking',
        code: 'effort_not_exposed',
        rung: rungNumber,
        message:
          exposed.length === 0
            ? `"${model.id}" exposes no effort lever, so effort is a silent no-op there and this rung has to be unset.`
            : `"${model.id}" exposes ${exposed.join(', ')}, not "${rung.effort}".`,
      })
    }
  })

  for (let index = 0; index + 1 < chain.length; index += 1) {
    const here = chain[index]
    const next = chain[index + 1]
    if (!here || !next || here.provider !== next.provider) {
      continue
    }
    const rungNumber = index + 2
    if (index === 0) {
      issues.push({
        severity: 'blocking',
        code: 'rung_2_same_provider',
        rung: 2,
        message:
          'Rung 2 has to cross providers: falling back onto the provider that just failed is not a fallback.',
      })
    } else {
      issues.push({
        severity: 'notice',
        code: 'provider_repeat',
        rung: rungNumber,
        message: `Rung ${rungNumber} stays on "${here.provider}", which rung ${rungNumber - 1} already used.`,
      })
    }
    if (providerById(catalog, here.provider)?.pooledUsage) {
      issues.push({
        severity: 'blocking',
        code: 'pooled_provider_repeat',
        rung: rungNumber,
        message: `"${here.provider}" draws every route from one quota, so rung ${rungNumber} is blocked by exactly what blocks rung ${rungNumber - 1}.`,
      })
    }
  }

  return issues
}

/**
 * Whether a seat with this chain may issue a verdict.
 *
 * Derived, never set by hand. A seat whose *own rung 1* is a degraded lane is
 * already running below the bar a verdict needs, and no amount of configuration
 * elsewhere changes that: it does its work, reports what it found, and the pass
 * flag stays for something else to set.
 *
 * Resolved on the `(provider, route)` pair, because the same route id under a
 * degraded provider is a degraded seat — reading the id alone would grant a
 * Cursor seat the verdict authority of the Claude route with the same name.
 */
export function canVerdict(chain: readonly ModelRung[], catalog: ModelCatalog): boolean {
  const lead = chain[0]
  if (!lead) {
    return false
  }
  const model = modelForRung(catalog, lead)
  return model ? !model.degradedLane : false
}

/* ------------------------------------------------------- context economics */

/** What a class turns into once the model is taken into account. */
export interface ContextResolution {
  /** The class's own trigger target, or `null` for `native`. */
  readonly requested: number | null
  /** What it actually triggers at, or `null` when nothing can be proven. */
  readonly effective: number | null
  /** Whether the model's ceiling cut the request down. */
  readonly clamped: boolean
  /** What the model can do with the request. */
  readonly capability:
    /** The model covers the request. */
    | 'supported'
    /** The model's ceiling is lower, and that ceiling is what runs. */
    | 'clamped'
    /** Nothing is established about the ceiling, so nothing can be proven. */
    | 'unsupported'
}

/**
 * Resolve one class against one model's ceiling.
 *
 * `effective = min(class target, model window)` for every class that names a
 * number. `native` names none — it *is* the model's default — so it resolves to
 * the ceiling itself and is never clamped.
 */
export function resolveContext(
  context: ContextPolicy,
  modelWindow: number | null,
): ContextResolution {
  const target = classTarget(context.class)
  if (target === null) {
    return { requested: null, effective: modelWindow, clamped: false, capability: 'supported' }
  }
  if (modelWindow === null) {
    return { requested: target, effective: null, clamped: false, capability: 'unsupported' }
  }
  if (target <= modelWindow) {
    return { requested: target, effective: target, clamped: false, capability: 'supported' }
  }
  return { requested: target, effective: modelWindow, clamped: true, capability: 'clamped' }
}

/**
 * What a context declaration has to say for itself.
 *
 * `required` refuses rather than run a seat that is not getting what it asked
 * for. `best_effort` runs and says so — the one thing neither may do is stay
 * quiet about the difference between what was asked for and what will happen.
 */
export function contextIssues(
  context: ContextPolicy,
  modelWindow: number | null,
): readonly Issue[] {
  const issues: Issue[] = []
  const resolution = resolveContext(context, modelWindow)

  if (resolution.capability === 'clamped') {
    issues.push(
      context.enforcement === 'required'
        ? {
            severity: 'blocking',
            code: 'context_clamp_refused',
            message: `"${context.class}" triggers at ${resolution.requested} tokens and this model holds ${resolution.effective}. Enforcement is required, so the seat refuses rather than running short.`,
          }
        : {
            severity: 'notice',
            code: 'context_clamped',
            message: `"${context.class}" triggers at ${resolution.requested} tokens and this model holds ${resolution.effective}, so ${resolution.effective} is what runs.`,
          },
    )
  }
  if (resolution.capability === 'unsupported') {
    issues.push(
      context.enforcement === 'required'
        ? {
            severity: 'blocking',
            code: 'context_unenforceable',
            message:
              'Nothing is established about this model’s context ceiling, so a required class cannot be shown to hold.',
          }
        : {
            severity: 'notice',
            code: 'context_not_enforced',
            message:
              'Nothing is established about this model’s context ceiling. Best effort continues and records that nothing was enforced — it never records success.',
          },
    )
  }
  if (EXPLICIT_ONLY.includes(context.class)) {
    issues.push({
      severity: 'notice',
      code: 'context_explicit_only',
      message: `"${context.class}" is never inferred; this slot is the explicit declaration that asks for it.`,
    })
  }

  return issues
}

/** One class, resolved and costed against one model. */
export interface ClassCost {
  /** The class. */
  readonly class: ContextClass
  /** What it resolves to on this model. */
  readonly resolution: ContextResolution
  /** The price step that applies, or `null` when the catalog prices none. */
  readonly tier: ModelPriceTier | null
  /**
   * Dollars to fill the effective threshold with input, or `null`.
   *
   * One request, not one seat: a seat re-sends its accumulated context every
   * turn, so real spend scales with turns. The column that renders this says so.
   */
  readonly inputUsd: number | null
}

/** One class costed against a model, placed against the other four. */
export interface RankedClassCost extends ClassCost {
  /**
   * This class's input cost as a multiple of the cheapest priced class, or
   * `null` when the comparison would carry no information.
   *
   * On a single-tier curve `inputUsd` is exactly proportional to the threshold,
   * so this ratio is arithmetically identical to the ratio of the class targets
   * — it restates the closed class set and says nothing about price, while
   * looking like economics. It is therefore withheld unless the curve actually
   * steps.
   */
  readonly relative: number | null
}

/**
 * The price step that applies at a given prompt size.
 *
 * The highest step at or below the size, which is the step a provider means when
 * it publishes one rate up to some window and another above it.
 */
export function priceTierFor(model: ModelEntry, tokens: number | null): ModelPriceTier | null {
  if (tokens === null) {
    return null
  }
  let chosen: ModelPriceTier | null = null
  for (const tier of model.pricing) {
    if (tier.window <= tokens && (chosen === null || tier.window > chosen.window)) {
      chosen = tier
    }
  }
  return chosen
}

/**
 * What one class costs on one model.
 *
 * Input only: the dollars needed to fill the effective threshold, once.
 *
 * # Why there is no output term
 *
 * The obvious formula — input at the threshold *plus* a typical response — is
 * what the design first proposed, and it is wrong. A context class is an
 * auto-compaction target: it bounds what accumulates before the seat compacts.
 * It says nothing whatever about how long the model's replies are. Output volume
 * belongs to the role, the task and the number of turns; pricing it here fused a
 * class-owned quantity with a role-owned one and required inventing a "typical
 * output" nobody could source.
 *
 * So the term is gone rather than sourced. `ModelPriceTier.outputPerMtok` stays
 * on the catalog because providers publish it and it is true — it is simply not
 * an input to comparing one context class against another.
 *
 * @see _docs/ai-orchestration/analysis/2026-08-14-11-35-analysis-kontor-teams-capability-ui.md:85
 */
export function classCost(cls: ContextClass, model: ModelEntry): ClassCost {
  const resolution = resolveContext(
    { class: cls, enforcement: 'best_effort' },
    model.contextWindow.value,
  )
  const tier = priceTierFor(model, resolution.effective)
  const effective = resolution.effective
  const inputUsd =
    tier === null || effective === null ? null : (tier.inputPerMtok * effective) / 1_000_000
  return { class: cls, resolution, tier, inputUsd }
}

/**
 * Every class, resolved and costed against one model, each placed against the
 * cheapest of them.
 *
 * The ratio is computed here rather than on a single class because it is a
 * property of the set: one class alone has nothing to be a multiple of. It is
 * suppressed entirely on a single-tier curve — see `RankedClassCost.relative`.
 */
export function classCosts(model: ModelEntry): readonly RankedClassCost[] {
  const rows = CONTEXT_CLASSES.map((entry) => classCost(entry.id, model))
  const stepped = model.pricing.length > 1
  const priced = rows
    .map((row) => row.inputUsd)
    .filter((usd): usd is number => usd !== null && usd > 0)
  const cheapest = priced.length === 0 ? null : Math.min(...priced)
  return rows.map((row) => ({
    ...row,
    relative:
      !stepped || cheapest === null || row.inputUsd === null ? null : row.inputUsd / cheapest,
  }))
}

/**
 * The smallest class that actually covers a seat's need band.
 *
 * # Why this asks about coverage and not about price
 *
 * It used to rank the covering classes by cost and return the cheapest. That
 * reads well until you ask what "cheapest" means across these providers: on a
 * metered one it is dollars off a balance, on a plan it is a share of an
 * allowance whose marginal cost is nothing, on a free route it is a request out
 * of a daily cap. Ranking four different scarce resources on one axis produces a
 * number that compares nothing.
 *
 * # Why it has to know the enforcement mode
 *
 * A clamped class still *covers* a band — its effective threshold reaches it —
 * but under `required` enforcement a clamp is a blocking refusal. Recommending
 * one there put the editor in the position of printing a recommendation directly
 * above a blocking issue refusing the very class it recommended. Under
 * `required` only a class the model covers outright is a candidate.
 *
 * `null` means no class reaches the band on this model under this enforcement.
 * That is an answer, and the caller reports it — it is never quietly turned into
 * `native`.
 */
export function recommendClass(
  need: SeatNeedBand,
  model: ModelEntry,
  enforcement: ContextEnforcement = 'best_effort',
): ContextClass | null {
  if (!Number.isFinite(need.minTokens) || need.minTokens < 0) {
    return null
  }
  const covering = RANKED_CLASSES.find((entry) => {
    const resolution = resolveContext(
      { class: entry.id, enforcement },
      model.contextWindow.value,
    )
    if (resolution.effective === null || resolution.effective < need.minTokens) {
      return false
    }
    return enforcement === 'required' ? resolution.capability === 'supported' : true
  })
  return covering?.id ?? null
}

/* -------------------------------------------------------------- seat & team */

/** What one rung of the chain resolves to. */
export interface RungReview {
  /** Which rung, 1-based. */
  readonly rung: number
  /** The route it names, when the catalog serves the pair. */
  readonly model: ModelEntry | null
  /** The declared class against that route. */
  readonly resolution: ContextResolution | null
  /** The smallest class covering the band on that route. */
  readonly recommended: ContextClass | null
}

/** Everything the editor needs to say about one slot. */
export interface SeatReview {
  /** Everything wrong with it, or worth saying about it. */
  readonly issues: readonly Issue[]
  /** Whether it may issue a verdict. Derived from rung 1. */
  readonly canVerdict: boolean
  /** The route rung 1 names, when the catalog serves it. */
  readonly leadModel: ModelEntry | null
  /** What a wider context on that route actually consumes. */
  readonly basis: ChargingBasis | null
  /** Where that charging basis came from. */
  readonly basisProvenance: Provenance | null
  /** The declared class against that route, when there is one. */
  readonly resolution: ContextResolution | null
  /** Every class resolved and costed against that route. */
  readonly costs: readonly RankedClassCost[]
  /** The smallest class covering the need band, when one does. */
  readonly recommended: ContextClass | null
  /** Which precedence source supplied the effective need. */
  readonly needSource: 'role_slot' | 'run_override'
  /** Every rung resolved, not only the first. */
  readonly rungs: readonly RungReview[]
}

/**
 * Review one slot's capabilities.
 *
 * Chain rules, context rules and the economics of the need band, composed.
 *
 * # Why every rung is resolved, not only the first
 *
 * A fallback chain exists precisely so the lower rungs get used. Resolving the
 * context contract against rung 1 alone meant a seat could declare a need its
 * own third fallback provably cannot meet, and the editor would print "Nothing
 * to report" — the chain is a promise about what happens when rung 1 is gone, so
 * a rung that cannot honour the declaration is a fact about the seat.
 *
 * The lower rungs report as notices rather than blocking: the seat is publishable
 * and may never descend that far, but it may not be silent about it.
 *
 * The need rules that are not in the chain or context rules:
 *
 * - a band that is not a non-negative number is not a band;
 * - a band no class reaches on rung 1 blocks — a need nothing covers is a
 *   problem to solve, not a reason to promote the seat to `native`;
 * - a band that cannot be *checked*, because nothing is established about the
 *   ceiling, is said out loud rather than passed;
 * - a declared class whose effective threshold sits under the band is a notice:
 *   the seat is configured below what it says it needs.
 */
export function reviewSeat(seat: SeatCapabilities, catalog: ModelCatalog): SeatReview {
  const issues: Issue[] = [...validateChain(seat.chain, catalog)]
  const need = seat.taskMinimum && seat.taskMinimum.minTokens > seat.need.minTokens
    ? seat.taskMinimum
    : seat.need
  const needSource = need === seat.need ? 'role_slot' : 'run_override'

  // The need band is the fifth provenanced cell, and the only one that does not
  // live in the catalog — so `validateCatalog` cannot reach it and this is the
  // path that must. Without this a band promoted to `researched` with no review
  // reference passed every check the editor runs, which is precisely the claim
  // the provenance record exists to make impossible.
  issues.push(...provenanceIssues('The need band', seat.need.provenance))
  if (seat.taskMinimum) {
    issues.push(...provenanceIssues('The task minimum band', seat.taskMinimum.provenance))
  }

  const rungs: RungReview[] = seat.chain.map((rung, index) => {
    const model = modelForRung(catalog, rung) ?? null
    return {
      rung: index + 1,
      model,
      resolution: model ? resolveContext(seat.context, model.contextWindow.value) : null,
      recommended: model
        ? recommendClass(need, model, seat.context.enforcement)
        : null,
    }
  })

  const leadModel = rungs[0]?.model ?? null
  const resolution = rungs[0]?.resolution ?? null
  const recommended = rungs[0]?.recommended ?? null

  if (leadModel) {
    issues.push(...contextIssues(seat.context, leadModel.contextWindow.value))
  }

  const bandIsNumber = Number.isFinite(need.minTokens) && need.minTokens >= 0

  if (!bandIsNumber) {
    issues.push({
      severity: 'blocking',
      code: 'need_invalid',
      message: 'A need band is a count of tokens, so it cannot be negative or unstated.',
    })
  } else if (leadModel) {
    if (leadModel.contextWindow.value === null) {
      issues.push({
        severity: 'notice',
        code: 'need_unresolvable',
        message: `Nothing is established about the context ceiling of "${leadModel.id}", so no class can be shown to cover this band.`,
      })
    } else if (recommended === null) {
      issues.push({
        severity: 'blocking',
        code: 'need_uncovered',
        message: `No class reaches ${need.minTokens} tokens on "${leadModel.id}" — its ceiling is ${leadModel.contextWindow.value}. That is a need to resolve, not a reason to promote the seat to native.`,
      })
    } else if (resolution && resolution.effective !== null && resolution.effective < need.minTokens) {
      issues.push({
        severity: 'notice',
        code: 'need_unmet_by_class',
        message: `"${seat.context.class}" runs at ${resolution.effective} tokens on this model, under the ${need.minTokens} this seat says it needs. "${recommended}" is the smallest covering class.`,
      })
    }
  }

  // Rungs 2..n: a fallback that cannot honour the declared need is a fact about
  // the seat, and was previously invisible.
  if (bandIsNumber) {
    for (const entry of rungs.slice(1)) {
      if (!entry.model || entry.model.contextWindow.value === null) {
        continue
      }
      if (entry.recommended === null) {
        issues.push({
          severity: 'notice',
          code: 'need_uncovered_on_fallback',
          rung: entry.rung,
          message: `No class reaches ${need.minTokens} tokens on "${entry.model.id}" — its ceiling is ${entry.model.contextWindow.value}. Rung ${entry.rung} cannot honour what this seat says it needs.`,
        })
      }
    }
  }

  return {
    issues,
    canVerdict: canVerdict(seat.chain, catalog),
    leadModel,
    basis: leadModel
      ? (providerById(catalog, leadModel.provider)?.basis.value ?? null)
      : null,
    basisProvenance: leadModel
      ? (providerById(catalog, leadModel.provider)?.basis.provenance ?? null)
      : null,
    resolution,
    costs: leadModel ? classCosts(leadModel) : [],
    recommended,
    needSource,
    rungs,
  }
}

/**
 * Check one team draft.
 *
 * Template-level rules: a name, at least one slot, slot keys that are distinct,
 * and the provenance of every need band the draft declares. Handoffs are
 * deliberately not modelled here — the prototype edits seats, and a handoff DAG
 * with no publish path behind it would be a shape nothing checks.
 *
 * The band check is here as well as in `reviewSeat` because they answer
 * different questions. This is the aggregate guard for the one provenanced cell
 * that lives on the *draft*, and it names the slot so a template with many seats
 * says *which* band is claiming more than it can show; `reviewSeat` reports the
 * same defect unqualified, in the slot the operator is already looking at.
 *
 * The four cells that live on the *catalog* are enforced by `validateCatalog`
 * immediately after `GET /v1/catalog` and before `TeamsView` renders any editor.
 * The band remains the draft-owned provenance check at publish time.
 */
export function validateTeam(draft: TeamDraft): readonly Issue[] {
  const issues: Issue[] = []

  if (draft.name.trim() === '') {
    issues.push({
      severity: 'blocking',
      code: 'team_unnamed',
      message: 'A template revision is referred to by name, so it needs one.',
    })
  }
  if (draft.slots.length === 0) {
    issues.push({
      severity: 'blocking',
      code: 'team_empty',
      message: 'A team with no slots declares nobody to do the work.',
    })
  }

  for (const slot of draft.slots) {
    issues.push(...provenanceIssues(
      `The need band on slot "${slot.id}"`,
      slot.capabilities.need.provenance,
    ).map((issue) => ({ ...issue, slot: slot.id })))
  }

  const seen = new Set<string>()
  const reported = new Set<string>()
  for (const slot of draft.slots) {
    if (seen.has(slot.id) && !reported.has(slot.id)) {
      reported.add(slot.id)
      issues.push({
        severity: 'blocking',
        code: 'duplicate_slot_id',
        message: `Two slots are called "${slot.id}", so a handoff naming it would not say which one it meant.`,
      })
    }
    seen.add(slot.id)
  }

  return issues
}

/* ---------------------------------------------------------------- fixtures */

/** The live call that established the two promoted ceilings. */
const CLAUDE_WINDOW_CITATION =
  'mcp__paseo__list_models(provider="claude") → contextWindowMaxTokens: 1000000; corroborated by FLEET:273'

/** The day the promoted cells were read. */
const OBSERVED = '2026-08-14'

/** A ceiling a live read returned nothing for — a positive finding, not an absence. */
function verifiedNoWindow(call: string): Sourced<number | null> {
  return liveValue(null, `${call} → no context window field returned`, OBSERVED)
}

/**
 * A value read from the runtime this session and deliberately **not** promoted.
 *
 * The state is `fixture/needs-verification`, so nothing downstream treats it as
 * established — but the citation records exactly which route was read, so the
 * next reader can see that the conservatism is a decision rather than an
 * oversight, and can re-run the same call to check it.
 *
 * This exists because of the opencode-fronted vendors. Both of them resolve
 * 1:1 onto a route id (`{provider}/{id}`), and both of their fixture values
 * match that route's live payload exactly — so on evidence, each could be
 * promoted. Neither is, because one of them cannot be (its row is contested on
 * other grounds), and a principle applied to one vendor and not the other on
 * identical evidence is not yet a principle. Symmetry is kept in the
 * conservative direction, which is the direction the gate's own row-ambiguity
 * rule binds.
 *
 * @see /Users/igor/kon-mvp-20-scratch/evidence/kontor-teams/RE-GATE-RECORD.md §4.2 (F2)
 */
function observedNotPromoted<T>(value: T, citation: string): Sourced<T> {
  return {
    value,
    provenance: {
      state: 'fixture/needs-verification',
      reviewRef: null,
      citation,
      observedAt: OBSERVED,
    },
  }
}

/** An effort ladder read from the runtime this session. */
function liveEfforts(values: readonly EffortLevel[], call: string): Sourced<readonly EffortLevel[]> {
  return liveValue(values, `${call} → thinkingOptions`, OBSERVED)
}

/**
 * The catalog injected by tests and offered only by the explicit offline preview.
 *
 * Route ids and effort ladders are read from this runtime; ceilings and prices
 * are almost entirely unverified and say so per cell. The two exceptions are the
 * Claude ceilings, which the research gate promoted to `live` against a call
 * anyone can repeat.
 *
 * Nothing here is a guess dressed as a fact: where a live read returned no
 * window, the cell records `null` at state `live` — "we asked and there is
 * nothing to have" is a different answer from "nobody looked", and only the
 * provenance can tell them apart.
 */
export const FIXTURE_CATALOG: ModelCatalog = {
  providers: [
    {
      id: 'cursor',
      label: 'Cursor',
      // Included in the plan at token rates, so tokens do cost — but against an
      // allowance rather than a balance.
      basis: unverified<ChargingBasis>('included_usage'),
      reachedVia: null,
      pooledUsage: false,
    },
    {
      id: 'deepseek',
      label: 'DeepSeek',
      // A vendor, not a provider in this runtime: `list_providers` enables
      // claude, codex, opencode and cursor, and this vendor's routes are served
      // through opencode. So the basis is an assertion about an entity the
      // runtime does not have, on the axis that decides whether a dollar
      // prints — flagged accordingly and never promoted.
      basis: unverified<ChargingBasis>('metered'),
      reachedVia: 'opencode',
      pooledUsage: false,
    },
    {
      id: 'codex',
      label: 'Codex',
      basis: unverified<ChargingBasis>('plan_allowance'),
      reachedVia: null,
      pooledUsage: false,
    },
    {
      id: 'claude',
      label: 'Claude',
      // Every Claude route draws from one plan window, which is what makes a
      // Claude rung under a Claude rung worth nothing.
      basis: unverified<ChargingBasis>('plan_allowance'),
      reachedVia: null,
      pooledUsage: true,
    },
    {
      id: 'openrouter',
      label: 'OpenRouter',
      // Same as DeepSeek: reached through opencode, not itself enabled.
      basis: unverified<ChargingBasis>('request_quota'),
      reachedVia: 'opencode',
      pooledUsage: false,
    },
  ],
  models: [
    {
      // The runtime spells the Auto row `auto-smart` and marks it default. The
      // fleet policy's `cursor/default` is chain shorthand for "the provider's
      // Auto row", not a route id — and `ModelEntry.id` is contracted as the id
      // exactly as the runtime spells it, so the default-ness is its own flag.
      id: 'auto-smart',
      label: 'Auto',
      provider: 'cursor',
      isDefault: true,
      contextWindow: verifiedNoWindow('mcp__paseo__list_models(provider="cursor")'),
      efforts: liveEfforts([], 'mcp__paseo__list_models(provider="cursor") → thinkingOptions: null'),
      pricing: [],
      degradedLane: true,
    },
    {
      id: 'deepseek-v4-flash',
      label: 'DeepSeek V4 Flash',
      provider: 'deepseek',
      isDefault: false,
      // Was 256_000, which no source supports; the design doc claimed a
      // correction to 1M that never landed. Neither number is established: the
      // runtime serves this vendor through opencode, which returns no context
      // window on ANY of its 361 routes, so the live second source the rule
      // names cannot be obtained by that mechanism at all.
      contextWindow: observedNotPromoted<number | null>(
        null,
        'mcp__paseo__list_models(provider="opencode") → route "deepseek/deepseek-v4-flash" returned no contextWindowMaxTokens (0 of 361 routes report one)',
      ),
      // This row describes the direct route, and the live payload confirms the
      // ladder exactly. It is still not promoted: `openrouter/deepseek/
      // deepseek-v4-flash` exposes high/xhigh instead, and three further
      // `deepseek-v4-flash` routes exist, so the price cell on this row cannot
      // say which contract it describes — and a row whose price is contested
      // does not get to promote its other cells piecemeal.
      efforts: observedNotPromoted<readonly EffortLevel[]>(
        ['low', 'high', 'max'],
        'mcp__paseo__list_models(provider="opencode") → route "deepseek/deepseek-v4-flash" thinkingOptions [low, high, max]',
      ),
      // The cache-miss input rate. NOT promotable, and not only because it is
      // uncited here: the published curve has a cache-hit rate two orders of
      // magnitude lower that a single `inputPerMtok` cannot represent, and this
      // one row collapses at least three live routes with different effort
      // ladders. A price cell that cannot say which route it prices cannot be
      // promoted whatever the number is.
      pricing: [
        {
          window: 0,
          inputPerMtok: 0.14,
          outputPerMtok: 0.28,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: false,
    },
    {
      id: 'gpt-5.6-sol',
      label: 'GPT-5.6 Sol',
      provider: 'codex',
      isDefault: false,
      // Was 400_000, which nothing supports: the runtime returns no window for
      // this provider and our own design doc asserts a different pair of
      // numbers again. Three inconsistent values across our own artifacts is
      // not a fact, and this one drove the prototype's flagship clamp demo.
      contextWindow: verifiedNoWindow('mcp__paseo__list_models(provider="codex")'),
      efforts: liveEfforts(
        ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
        'mcp__paseo__list_models(provider="codex")',
      ),
      pricing: [
        {
          window: 0,
          inputPerMtok: 1.25,
          outputPerMtok: 10,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: false,
    },
    {
      id: 'gpt-5.6-terra',
      label: 'GPT-5.6 Terra',
      provider: 'codex',
      isDefault: false,
      contextWindow: verifiedNoWindow('mcp__paseo__list_models(provider="codex")'),
      efforts: liveEfforts(
        ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'],
        'mcp__paseo__list_models(provider="codex")',
      ),
      pricing: [
        {
          window: 0,
          inputPerMtok: 0.6,
          outputPerMtok: 4,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: false,
    },
    {
      id: 'gpt-5.6-luna',
      label: 'GPT-5.6 Luna',
      provider: 'codex',
      isDefault: false,
      contextWindow: verifiedNoWindow('mcp__paseo__list_models(provider="codex")'),
      // Luna exposes no `ultra`, unlike its siblings — read, not assumed.
      efforts: liveEfforts(
        ['low', 'medium', 'high', 'xhigh', 'max'],
        'mcp__paseo__list_models(provider="codex")',
      ),
      pricing: [],
      degradedLane: false,
    },
    {
      // Named by the fleet policy at Builder-chore rung 4, and confirmed live.
      id: 'gpt-5.4-mini',
      label: 'GPT-5.4 Mini',
      provider: 'codex',
      isDefault: false,
      contextWindow: verifiedNoWindow('mcp__paseo__list_models(provider="codex")'),
      efforts: liveEfforts(
        ['low', 'medium', 'high', 'xhigh'],
        'mcp__paseo__list_models(provider="codex")',
      ),
      pricing: [],
      degradedLane: false,
    },
    {
      id: 'claude-opus-5',
      label: 'Claude Opus 5',
      provider: 'claude',
      isDefault: true,
      // The only numeric ceilings this gate promoted. Re-checkable by repeating
      // the call in the citation.
      contextWindow: liveValue<number | null>(1_000_000, CLAUDE_WINDOW_CITATION, OBSERVED),
      efforts: liveEfforts(
        ['off', 'low', 'medium', 'high', 'xhigh', 'max', 'ultracode'],
        'mcp__paseo__list_models(provider="claude")',
      ),
      pricing: [
        {
          window: 0,
          inputPerMtok: 5,
          outputPerMtok: 25,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: false,
    },
    {
      id: 'claude-fable-5',
      label: 'Claude Fable 5',
      provider: 'claude',
      isDefault: false,
      contextWindow: liveValue<number | null>(1_000_000, CLAUDE_WINDOW_CITATION, OBSERVED),
      // No `off` on this route, unlike its sibling.
      efforts: liveEfforts(
        ['low', 'medium', 'high', 'xhigh', 'max', 'ultracode'],
        'mcp__paseo__list_models(provider="claude")',
      ),
      pricing: [
        {
          window: 0,
          inputPerMtok: 10,
          outputPerMtok: 50,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: false,
    },
    {
      id: 'nvidia/nemotron-3-ultra-550b-a55b:free',
      label: 'Nemotron 3 Ultra (free route)',
      provider: 'openrouter',
      isDefault: false,
      // DEMOTED from `live` at the re-gate's F2. The evidence here is identical
      // to DeepSeek's above — same opencode payload, same 0-of-361 windows, and
      // a fixture row that resolves 1:1 onto a route id the same way
      // (`{provider}/{id}` → "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
      // whose live ladder [medium, high] matches this row exactly).
      //
      // On that evidence this row could be promoted. It is not, because the row
      // it is symmetric with cannot be, and one vendor holding `live` while an
      // identically-evidenced vendor holds `fixture` makes `live` mean two
      // things: "the runtime returned this" in one cell and "the runtime
      // returned this, from a row that may describe several routes" in the
      // other. A state that means two things is not re-checkable, which is the
      // one property promotion was supposed to buy.
      //
      // Sibling routes this row must not be confused with, both live:
      // "opencode/nemotron-3-ultra-free" (thinkingOptions: null — a different
      // ladder) and "openrouter/nvidia/nemotron-3-ultra-550b-a55b" (no :free).
      contextWindow: observedNotPromoted<number | null>(
        null,
        'mcp__paseo__list_models(provider="opencode") → route "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free" returned no contextWindowMaxTokens (0 of 361 routes report one)',
      ),
      efforts: observedNotPromoted<readonly EffortLevel[]>(
        ['medium', 'high'],
        'mcp__paseo__list_models(provider="opencode") → route "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free" thinkingOptions [medium, high]',
      ),
      // Zero is the published token price and is not the operative fact: the
      // scarce resource on this route is requests per day.
      pricing: [
        {
          window: 0,
          inputPerMtok: 0,
          outputPerMtok: 0,
          provenance: UNVERIFIED,
        },
      ],
      degradedLane: true,
    },
  ],
}

/**
 * How the ambiguous telemetry label is resolved.
 *
 * `lead` in the local AgentsRoom statistics denotes the Lead Architect role,
 * therefore it maps to this editor's `architect` slot. Manual Test Lead is a
 * QA/release-verdict role and maps to `qa`; it is not the unqualified `lead`.
 */
export const TELEMETRY_ROLE_RESOLUTION = {
  lead: 'architect',
  manualTestLead: 'qa',
} as const

const NEED_BAND_TELEMETRY =
  '/Users/igor/.agentsroom/stats/proj-1785577413079-edhf21.json; median non-zero per-agent/day tokens moved per prompt (input + cache creation + output), observed 2026-08-02..2026-08-14'

/** A telemetry-derived band that remains unpromoted until a review signs it. */
function observedBand(minTokens: number, rationale: string, role: string): SeatNeedBand {
  return {
    minTokens,
    rationale,
    provenance: {
      state: 'fixture/needs-verification',
      reviewRef: null,
      citation: `${NEED_BAND_TELEMETRY}; role=${role}`,
      observedAt: OBSERVED,
    },
  }
}

/**
 * The drafts the prototype opens with.
 *
 * Every chain below is copied rung-for-rung from the fleet policy, and the
 * copying is the point: an earlier pass replaced a route that did not exist with
 * a plausible neighbour instead of re-reading the document the chains came from,
 * and put it on a chain the policy never assigns it to. Where a seed deviates
 * from the policy now it is because the policy itself does — the standard
 * builder really does place two Codex rungs together, and the editor flags it.
 *
 * Need bands are derived from observed AgentsRoom token telemetry. They remain
 * explicitly unpromoted until a later review signs the measurements; unlike the
 * old circular placeholders, these values can and do disagree with the seeded
 * context classes.
 */
export const SEED_TEAMS: readonly TeamDraft[] = [
  {
    id: 'draft-plan-build-verify',
    name: 'ASMA plan-build-verify (prototype fixture)',
    slots: [
      {
        id: 'architect',
        capabilities: {
          chain: [
            { provider: 'codex', model: 'gpt-5.6-sol', effort: 'xhigh' },
            { provider: 'claude', model: 'claude-opus-5', effort: 'xhigh' },
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'max' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'high',
            },
          ],
          context: { class: 'deep', enforcement: 'best_effort' },
          need: observedBand(76_000, 'Median observed Software Architect token movement.', 'Software Architect'),
          skills: ['create-implementation-plan', 'create-architectural-decision-record'],
          mayEvaluate: ['plan_agreed'],
          mayWaive: [],
        },
      },
      {
        // Builder — standard. The policy's own rung order, including the two
        // adjacent Codex rungs the editor flags as a notice.
        id: 'implementer',
        capabilities: {
          chain: [
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'max' },
            { provider: 'codex', model: 'gpt-5.6-luna', effort: 'xhigh' },
            { provider: 'codex', model: 'gpt-5.6-terra', effort: 'high' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'high',
            },
          ],
          context: { class: 'standard', enforcement: 'best_effort' },
          need: observedBand(36_000, 'Median observed Full-Stack Developer token movement.', 'Full-Stack Developer'),
          skills: ['tdd', 'lint-fix'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
      {
        // Tester.
        id: 'qa',
        capabilities: {
          chain: [
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'max' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'high',
            },
            { provider: 'codex', model: 'gpt-5.6-luna', effort: 'high' },
            { provider: 'cursor', model: 'auto-smart', effort: 'unset' },
          ],
          context: { class: 'standard', enforcement: 'best_effort' },
          need: observedBand(44_000, 'Median observed QA Engineer token movement.', 'QA Engineer'),
          skills: ['run-mutation-tests'],
          mayEvaluate: ['qa_passed'],
          mayWaive: [],
        },
      },
      {
        // Inspector.
        id: 'audit',
        capabilities: {
          chain: [
            { provider: 'claude', model: 'claude-opus-5', effort: 'xhigh' },
            { provider: 'codex', model: 'gpt-5.6-sol', effort: 'xhigh' },
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'max' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'high',
            },
          ],
          context: { class: 'standard', enforcement: 'required' },
          need: observedBand(44_000, 'Inspector uses the observed QA/review lane.', 'QA Engineer'),
          skills: ['code-review', 'security-check'],
          mayEvaluate: ['audit_passed'],
          mayWaive: ['qa_passed'],
        },
      },
    ],
  },
  {
    id: 'draft-chore-lane',
    name: 'ASMA chore lane (prototype fixture)',
    slots: [
      {
        id: 'orchestrator',
        capabilities: {
          chain: [
            { provider: 'cursor', model: 'auto-smart', effort: 'unset' },
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'high' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'medium',
            },
            // The policy pins luna here, and only here among the chore lane.
            { provider: 'codex', model: 'gpt-5.6-luna', effort: 'medium' },
          ],
          context: { class: 'lean', enforcement: 'best_effort' },
          need: observedBand(398_000, 'Median observed Project Manager orchestration token movement.', 'Project Manager'),
          skills: ['epic-orchestration'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
      {
        id: 'builder-chore',
        capabilities: {
          chain: [
            { provider: 'cursor', model: 'auto-smart', effort: 'unset' },
            { provider: 'deepseek', model: 'deepseek-v4-flash', effort: 'high' },
            {
              provider: 'openrouter',
              model: 'nvidia/nemotron-3-ultra-550b-a55b:free',
              effort: 'medium',
            },
            // The policy pins 5.4-mini at this rung, not luna. A chore rung
            // exists for cost; filling it with a neighbouring route whose
            // economics are unread was the defect, not the fix.
            { provider: 'codex', model: 'gpt-5.4-mini', effort: 'medium' },
          ],
          context: { class: 'lean', enforcement: 'best_effort' },
          need: observedBand(92_000, 'Median observed Git Expert chore token movement.', 'Git Expert'),
          skills: ['lint-fix'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
    ],
  },
]
