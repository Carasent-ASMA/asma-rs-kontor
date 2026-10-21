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
 * @see the KON-MVP-20 committee record, COMMITTEE-RECORD.md §5 (kept outside this repository)
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
 * @see the KON-MVP-20 re-gate record, RE-GATE-RECORD.md §4.1 (F1) (kept outside this repository)
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

/**
 * One standard role, selected by the revision it is read from and its code.
 *
 * The standard title is deliberately absent. It belongs to the catalog, the
 * realm resolves it, and a console that could send one would be a second source
 * for a fact it does not own. The realm refuses a payload carrying it.
 */
export interface RoleSelection {
  /** The exact catalog revision the code is read from. */
  readonly catalog_revision: { readonly id: string; readonly version: number }
  /** The stable role code, such as `LSA`. */
  readonly role_code: string
  /** A presentation-only label, when the seat is shown as something narrower. */
  readonly custom_display_name?: string
}

/** One slot in a team template. */
export interface TeamSlot {
  /** The slot key. Opaque — nothing here interprets it. */
  readonly id: string
  /** The standard role this seat fills. */
  readonly role: RoleSelection
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

/** The day the offline fixture was authored. */
const OBSERVED = '2026-08-14'

/**
 * The catalog offered by the explicit offline preview.
 *
 * This is deliberately obviously fake: the provider ids, route ids and prices
 * are invented, and every cell says it is unverified. Production reads the
 * live route catalog through `GET /v1/catalog`; this fixture exists only so the
 * editor has something to render when an operator asks for the offline preview
 * after a failed live read. It must never be mistaken for the deployment's
 * catalogue.
 */
export const FIXTURE_CATALOG: ModelCatalog = {
  providers: [
    {
      id: 'alpha',
      label: 'Alpha (offline fixture)',
      basis: unverified<ChargingBasis>('metered'),
      reachedVia: null,
      pooledUsage: false,
    },
    {
      id: 'beta',
      label: 'Beta (offline fixture)',
      basis: unverified<ChargingBasis>('plan_allowance'),
      reachedVia: null,
      pooledUsage: true,
    },
    {
      id: 'gamma',
      label: 'Gamma (offline fixture)',
      basis: unverified<ChargingBasis>('included_usage'),
      reachedVia: 'beta',
      pooledUsage: true,
    },
  ],
  models: [
    {
      id: 'alpha-one',
      label: 'Alpha One (offline fixture)',
      provider: 'alpha',
      isDefault: true,
      contextWindow: unverified<number | null>(300_000),
      efforts: unverified<readonly EffortLevel[]>(['low', 'high']),
      pricing: [{ window: 0, inputPerMtok: 1, outputPerMtok: 2, provenance: UNVERIFIED }],
      degradedLane: false,
    },
    {
      id: 'alpha-two',
      label: 'Alpha Two (offline fixture)',
      provider: 'alpha',
      isDefault: false,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>(['off', 'low', 'high']),
      pricing: [{ window: 0, inputPerMtok: 2, outputPerMtok: 4, provenance: UNVERIFIED }],
      degradedLane: false,
    },
    {
      id: 'alpha-fast',
      label: 'Alpha Fast (offline fixture)',
      provider: 'alpha',
      isDefault: false,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>(['low']),
      pricing: [{ window: 0, inputPerMtok: 1, outputPerMtok: 1, provenance: UNVERIFIED }],
      degradedLane: true,
    },
    {
      id: 'alpha-static',
      label: 'Alpha Static (offline fixture)',
      provider: 'alpha',
      isDefault: false,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>([]),
      pricing: [{ window: 0, inputPerMtok: 1, outputPerMtok: 2, provenance: UNVERIFIED }],
      degradedLane: true,
    },
    {
      id: 'beta-one',
      label: 'Beta One (offline fixture)',
      provider: 'beta',
      isDefault: true,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>(['low', 'medium', 'high']),
      pricing: [{ window: 0, inputPerMtok: 3, outputPerMtok: 6, provenance: UNVERIFIED }],
      degradedLane: false,
    },
    {
      id: 'beta-two',
      label: 'Beta Two (offline fixture)',
      provider: 'beta',
      isDefault: false,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>(['low', 'medium']),
      pricing: [{ window: 0, inputPerMtok: 2, outputPerMtok: 3, provenance: UNVERIFIED }],
      degradedLane: true,
    },
    {
      id: 'gamma-one',
      label: 'Gamma One (offline fixture)',
      provider: 'gamma',
      isDefault: true,
      contextWindow: unverified<number | null>(null),
      efforts: unverified<readonly EffortLevel[]>(['medium', 'high']),
      pricing: [{ window: 0, inputPerMtok: 0, outputPerMtok: 0, provenance: UNVERIFIED }],
      degradedLane: true,
    },
    {
      id: 'gamma-two',
      label: 'Gamma Two (offline fixture)',
      provider: 'gamma',
      isDefault: false,
      contextWindow: unverified<number | null>(500_000),
      efforts: unverified<readonly EffortLevel[]>(['high', 'max']),
      pricing: [{ window: 0, inputPerMtok: 5, outputPerMtok: 10, provenance: UNVERIFIED }],
      degradedLane: false,
    },
  ],
}

/**
 * How the ambiguous telemetry label is resolved.
 *
 * `lead` in the local statistics denotes the lead architect role, therefore it
 * maps to this editor's `architect` slot. Manual Test Lead is a QA/release
 * verdict role and maps to `qa`; it is not the unqualified `lead`.
 */
export const TELEMETRY_ROLE_RESOLUTION = {
  lead: 'architect',
  manualTestLead: 'qa',
} as const

const NEED_BAND_TELEMETRY =
  'Synthetic offline fixture; median non-zero per-agent/day tokens moved per prompt (input + cache creation + output)'

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
 * The drafts the explicit offline preview opens with.
 *
 * Every name, role code and route below is synthetic. The chains exercise the
 * editor's rules — a pooled provider repeated, a degraded verdict lane, an
 * effort the route does not expose — without naming a real deployment's
 * catalog. A publishable draft stays publishable against the fixture.
 */
const FIXTURE_ROLE_CATALOG = { id: 'offline-fixture-roles', version: 1 } as const

/** One role selection against the offline fixture catalog revision. */
const selects = (code: string): RoleSelection => ({
  catalog_revision: FIXTURE_ROLE_CATALOG,
  role_code: code,
})

export const SEED_TEAMS: readonly TeamDraft[] = [
  {
    id: 'draft-example-plan-build-verify',
    name: 'Example plan-build-verify (offline fixture)',
    slots: [
      {
        id: 'architect',
        role: selects('ARCH'),
        capabilities: {
          chain: [
            { provider: 'alpha', model: 'alpha-one', effort: 'high' },
            { provider: 'beta', model: 'beta-one', effort: 'high' },
            { provider: 'gamma', model: 'gamma-one', effort: 'medium' },
            { provider: 'alpha', model: 'alpha-fast', effort: 'low' },
          ],
          context: { class: 'deep', enforcement: 'best_effort' },
          need: observedBand(76_000, 'Synthetic architect token movement.', 'Architect'),
          skills: ['create-implementation-plan', 'create-architectural-decision-record'],
          mayEvaluate: ['plan_agreed'],
          mayWaive: [],
        },
      },
      {
        id: 'implementer',
        role: selects('SWE'),
        capabilities: {
          chain: [
            { provider: 'alpha', model: 'alpha-one', effort: 'high' },
            { provider: 'beta', model: 'beta-one', effort: 'medium' },
            { provider: 'gamma', model: 'gamma-one', effort: 'medium' },
            { provider: 'beta', model: 'beta-two', effort: 'low' },
          ],
          context: { class: 'standard', enforcement: 'best_effort' },
          need: observedBand(36_000, 'Synthetic builder token movement.', 'Builder'),
          skills: ['tdd', 'lint-fix'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
      {
        id: 'qa',
        role: selects('QA'),
        capabilities: {
          chain: [
            { provider: 'alpha', model: 'alpha-two', effort: 'low' },
            { provider: 'beta', model: 'beta-two', effort: 'medium' },
            { provider: 'gamma', model: 'gamma-one', effort: 'high' },
          ],
          context: { class: 'standard', enforcement: 'best_effort' },
          need: observedBand(44_000, 'Synthetic QA token movement.', 'QA'),
          skills: ['run-mutation-tests'],
          mayEvaluate: ['qa_passed'],
          mayWaive: [],
        },
      },
      {
        id: 'audit',
        role: selects('AUD'),
        capabilities: {
          chain: [
            { provider: 'beta', model: 'beta-one', effort: 'high' },
            { provider: 'alpha', model: 'alpha-one', effort: 'high' },
            { provider: 'gamma', model: 'gamma-one', effort: 'high' },
          ],
          context: { class: 'standard', enforcement: 'best_effort' },
          need: observedBand(44_000, 'Synthetic inspector token movement.', 'Inspector'),
          skills: ['code-review', 'security-check'],
          mayEvaluate: ['audit_passed'],
          mayWaive: ['qa_passed'],
        },
      },
    ],
  },
  {
    id: 'draft-example-chore-lane',
    name: 'Example chore lane (offline fixture)',
    slots: [
      {
        id: 'orchestrator',
        role: selects('PM'),
        capabilities: {
          chain: [
            { provider: 'gamma', model: 'gamma-one', effort: 'medium' },
            { provider: 'alpha', model: 'alpha-fast', effort: 'low' },
            { provider: 'beta', model: 'beta-one', effort: 'medium' },
          ],
          context: { class: 'lean', enforcement: 'best_effort' },
          need: observedBand(92_000, 'Synthetic coordinator token movement.', 'Coordinator'),
          skills: ['epic-orchestration'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
      {
        id: 'builder-chore',
        role: selects('SWE'),
        capabilities: {
          chain: [
            { provider: 'alpha', model: 'alpha-two', effort: 'low' },
            { provider: 'gamma', model: 'gamma-one', effort: 'medium' },
            { provider: 'beta', model: 'beta-two', effort: 'low' },
          ],
          context: { class: 'lean', enforcement: 'best_effort' },
          need: observedBand(92_000, 'Synthetic chore builder token movement.', 'Builder'),
          skills: ['lint-fix'],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
    ],
  },
]
