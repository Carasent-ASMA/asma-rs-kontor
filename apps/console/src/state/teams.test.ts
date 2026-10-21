/**
 * The seat rules, one assertion per rule.
 *
 * The mutants this file exists to kill: a chain checker that lets a fallback
 * land back on the provider that just ran out, an effort select that pins a
 * lever the route does not have, a context resolver that reports a class as
 * delivered when the model clamped it, an enforcement mode that clamps quietly,
 * a recommendation that promotes a seat to `native` because nothing else fit,
 * a catalog lookup that returns another provider's route of the same name, and a
 * fixture cell that claims to be established without saying who established it.
 *
 * Nothing here asserts against a provider by name where the rule is not about
 * that provider: the pooled-quota, degraded-lane and id-collision cases are
 * driven from purpose-built catalogs, so a rule that started recognizing
 * "claude" or "cursor" would still fail them.
 */
import { describe, expect, it } from 'vitest'
import {
  CONTEXT_CLASSES,
  EFFORT_LEVELS,
  FIXTURE_CATALOG,
  GATE_RECORD,
  MAX_RUNGS,
  SEED_TEAMS,
  TELEMETRY_ROLE_RESOLUTION,
  UNVERIFIED,
  blocks,
  canVerdict,
  classCost,
  classCosts,
  classTarget,
  contextIssues,
  liveValue,
  modelById,
  priceTierFor,
  provenanceIssues,
  publishTeamRevision,
  recommendClass,
  resolveContext,
  reviewSeat,
  unverified,
  validateCatalog,
  validateChain,
  validateTeam,
  type ChargingBasis,
  type ContextClass,
  type EffortLevel,
  type Issue,
  type ModelCatalog,
  type ModelEntry,
  type ModelRung,
  type SeatCapabilities,
  type SeatNeedBand,
  type TeamDraft,
} from './teams'

/** The codes a rule set reported, in order. */
function codes(issues: readonly Issue[]): string[] {
  return issues.map((issue) => issue.code)
}

/** The codes it reported at one severity. */
function codesAt(issues: readonly Issue[], severity: Issue['severity']): string[] {
  return codes(issues.filter((issue) => issue.severity === severity))
}

/** One rung, spelled out. */
function rung(provider: string, model: string, effort: ModelRung['effort']): ModelRung {
  return { provider, model, effort }
}

/** An unverified need band. */
function band(minTokens: number): SeatNeedBand {
  return { minTokens, rationale: null, provenance: UNVERIFIED }
}

/** One model, with only the fields a given test cares about spelled out. */
function model(
  overrides: Partial<Omit<ModelEntry, 'contextWindow' | 'efforts'>> &
    Pick<ModelEntry, 'id' | 'provider'> & {
      window?: number | null
      efforts?: readonly EffortLevel[]
    },
): ModelEntry {
  const { window = 256_000, efforts = ['high', 'max'], ...rest } = overrides
  return {
    label: overrides.id,
    isDefault: false,
    degradedLane: false,
    pricing: [{ window: 0, inputPerMtok: 1, outputPerMtok: 2, provenance: UNVERIFIED }],
    ...rest,
    contextWindow: unverified<number | null>(window),
    efforts: unverified<readonly EffortLevel[]>(efforts),
  }
}

/** One provider, with a basis nobody verified. */
function provider(
  id: string,
  basis: ChargingBasis,
  pooledUsage = false,
): ModelCatalog['providers'][number] {
  return { id, label: id, basis: unverified(basis), reachedVia: null, pooledUsage }
}

/**
 * A catalog with two providers that differ only in how their quota works.
 *
 * `pooled` draws every route from one bucket; `separate` does not. Nothing else
 * about them differs, so any rule that treats them differently is reading the
 * catalog rather than the names.
 */
const QUOTA_CATALOG: ModelCatalog = {
  providers: [provider('pooled', 'metered', true), provider('separate', 'metered')],
  models: [
    model({ id: 'pooled-one', provider: 'pooled' }),
    model({ id: 'pooled-two', provider: 'pooled' }),
    model({ id: 'separate-one', provider: 'separate' }),
    model({ id: 'separate-two', provider: 'separate' }),
  ],
}

/** A catalog whose two routes differ only in whether they are a degraded lane. */
const LANE_CATALOG: ModelCatalog = {
  providers: [provider('p-full', 'metered'), provider('p-degraded', 'metered')],
  models: [
    model({ id: 'full-route', provider: 'p-full' }),
    model({ id: 'degraded-route', provider: 'p-degraded', degradedLane: true }),
  ],
}

/**
 * Two providers serving the *same route id* under different contracts.
 *
 * This is the live shape, not a hypothetical: this runtime serves
 * `claude-opus-5` under both `claude` and `cursor`, at different charging bases
 * and with different verdict authority. A lookup keyed on the id alone answers
 * with whichever is first in the array.
 */
const COLLISION_CATALOG: ModelCatalog = {
  providers: [provider('claude', 'plan_allowance', true), provider('cursor', 'included_usage')],
  models: [
    model({ id: 'claude-opus-5', provider: 'claude', window: 1_000_000 }),
    // Same id, other contract: a resold route, degraded and billed elsewhere.
    model({
      id: 'claude-opus-5',
      provider: 'cursor',
      window: null,
      efforts: [],
      degradedLane: true,
      pricing: [],
    }),
  ],
}

/** A capability set built around one chain. */
/** Any well-formed role selection; these suites are about capabilities, not roles. */
const anyRole = { catalog_revision: { id: 'standard-roles', version: 1 }, role_code: 'SWE' }

function seat(overrides: Partial<SeatCapabilities> = {}): SeatCapabilities {
  return {
    chain: [rung('alpha', 'alpha-one', 'high'), rung('beta', 'beta-one', 'high')],
    context: { class: 'standard', enforcement: 'best_effort' },
    need: band(200_000),
    skills: [],
    mayEvaluate: [],
    mayWaive: [],
    ...overrides,
  }
}

describe('provenance', () => {
  it('lets an unverified cell say nothing and demands everything of a promoted one', () => {
    expect(provenanceIssues('a cell', UNVERIFIED)).toEqual([])

    const promoted = liveValue(1, 'a call', '2026-08-14').provenance
    expect(provenanceIssues('a cell', promoted)).toEqual([])
  })

  it('rejects a promotion with no review reference', () => {
    // The exact failure the record exists to prevent: a value asserted as
    // established with nothing saying who established it, so the claim cannot be
    // re-checked or withdrawn.
    const unsigned = { state: 'live', reviewRef: null, citation: 'a call', observedAt: '2026-08-14' } as const
    const issues = provenanceIssues('a cell', unsigned)
    expect(codesAt(issues, 'blocking')).toEqual(['promotion_without_review_ref'])
  })

  it('rejects a promotion with nothing to re-read and no date', () => {
    const undated = { state: 'researched', reviewRef: GATE_RECORD, citation: null, observedAt: null } as const
    expect(codesAt(provenanceIssues('a cell', undated), 'blocking')).toEqual([
      'promotion_without_citation',
      'promotion_without_date',
    ])
  })

  it('treats blank metadata as absent, not as a reference', () => {
    // An API-fed catalog fills these from somewhere, and an empty string is what
    // a missing column looks like after a round trip through JSON.
    const blank = { state: 'live', reviewRef: '  ', citation: '', observedAt: ' ' } as const
    expect(codesAt(provenanceIssues('a cell', blank), 'blocking')).toEqual([
      'promotion_without_review_ref',
      'promotion_without_citation',
      'promotion_without_date',
    ])
  })

  it('checks every promoted cell in a catalog, and passes the shipped fixture', () => {
    expect(validateCatalog(FIXTURE_CATALOG)).toEqual([])

    const rigged: ModelCatalog = {
      ...FIXTURE_CATALOG,
      models: FIXTURE_CATALOG.models.map((entry) =>
        entry.id === 'alpha-one'
          ? { ...entry, contextWindow: { value: 1_000_000, provenance: { ...UNVERIFIED, state: 'live' } } }
          : entry,
      ),
    }
    expect(codesAt(validateCatalog(rigged), 'blocking')).toContain('promotion_without_review_ref')
  })
})

describe('need-band provenance is enforced, not merely stored', () => {
  /** A band claiming to be established, with nothing behind the claim. */
  const unsigned: SeatNeedBand = {
    minTokens: 200_000,
    rationale: 'measured, allegedly',
    provenance: { state: 'researched', reviewRef: null, citation: null, observedAt: null },
  }

  /** The same band, properly signed. */
  const signed: SeatNeedBand = {
    ...unsigned,
    provenance: {
      state: 'researched',
      reviewRef: 'SOME-GATE-RECORD',
      citation: 'transcript telemetry, sample of 40 runs',
      observedAt: '2026-08-14',
    },
  }

  /** A draft whose one slot carries the given band. */
  function draftWith(need: SeatNeedBand): TeamDraft {
    return {
      id: 'd-band',
      name: 'a draft',
      slots: [{ id: 'architect', role: anyRole, capabilities: seat({ need }) }],
    }
  }

  it('fails seat validation when a promoted band carries no review reference', () => {
    // The band is the one provenanced cell that does not live in the catalog, so
    // `validateCatalog` cannot reach it. Before this it passed every check the
    // editor ran while claiming to be researched.
    const review = reviewSeat(seat({ need: unsigned }), FIXTURE_CATALOG)
    expect(codesAt(review.issues, 'blocking')).toEqual(
      expect.arrayContaining([
        'promotion_without_review_ref',
        'promotion_without_citation',
        'promotion_without_date',
      ]),
    )
    expect(blocks(review.issues)).toBe(true)
  })

  it('fails team validation too, naming the slot the band belongs to', () => {
    const issues = validateTeam(draftWith(unsigned))
    expect(codesAt(issues, 'blocking')).toEqual([
      'promotion_without_review_ref',
      'promotion_without_citation',
      'promotion_without_date',
    ])
    expect(issues[0]?.message).toMatch(/slot "architect"/)
  })

  it('passes both paths when the promoted band is properly provenanced', () => {
    expect(validateTeam(draftWith(signed))).toEqual([])
    const review = reviewSeat(seat({ need: signed }), FIXTURE_CATALOG)
    expect(codes(review.issues)).not.toContain('promotion_without_review_ref')
    expect(codes(review.issues)).not.toContain('promotion_without_citation')
    expect(codes(review.issues)).not.toContain('promotion_without_date')
  })

  it('says nothing about the unverified bands the fixture actually ships', () => {
    for (const draft of SEED_TEAMS) {
      expect(validateTeam(draft)).toEqual([])
    }
  })
})

describe('telemetry-derived need bands', () => {
  it('resolves lead explicitly and can disagree with seeded context classes', () => {
    expect(TELEMETRY_ROLE_RESOLUTION).toEqual({ lead: 'architect', manualTestLead: 'qa' })
    const architect = SEED_TEAMS[0]?.slots.find((slot) => slot.id === 'architect')
    expect(architect?.capabilities.need.provenance.citation).toContain('role=Architect')
    expect(architect?.capabilities.need.provenance.state).toBe('fixture/needs-verification')
    expect(architect?.capabilities.context.class).toBe('deep')
    expect(recommendClass(architect!.capabilities.need, FIXTURE_CATALOG.models[7]!, 'best_effort')).toBe('lean')
  })
})

describe('team revisions', () => {
  it('publishes the next immutable revision', () => {
    const draft = SEED_TEAMS[0]!
    const first = publishTeamRevision([], draft)
    const second = publishTeamRevision([first], { ...draft, name: 'changed' })
    expect([first.version, second.version]).toEqual([1, 2])
    expect(first.name).toBe(draft.name)
    expect(second.name).toBe('changed')
    expect(first).not.toBe(draft)
  })
})

describe('task minimum precedence', () => {
  it('uses a larger task-declared minimum as the run override', () => {
    const reviewed = reviewSeat(seat({
      need: band(100_000),
      taskMinimum: band(300_000),
    }), FIXTURE_CATALOG)
    expect(reviewed.needSource).toBe('run_override')
    expect(reviewed.recommended).toBe('deep')
  })
})

describe('catalog lookup keyed on (provider, id)', () => {
  it('returns the route the pair names, not the first of that name', () => {
    expect(modelById(COLLISION_CATALOG, 'claude', 'claude-opus-5')?.degradedLane).toBe(false)
    expect(modelById(COLLISION_CATALOG, 'cursor', 'claude-opus-5')?.degradedLane).toBe(true)
    expect(modelById(COLLISION_CATALOG, 'nobody', 'claude-opus-5')).toBeUndefined()
  })

  it('does not report a valid rung on the second provider as a mismatch', () => {
    // Keyed on the id alone this rung resolved to the Claude row and was
    // reported as a blocking provider mismatch, on a configuration the fleet
    // policy actually contemplates.
    const issues = validateChain([rung('cursor', 'claude-opus-5', 'unset')], COLLISION_CATALOG)
    expect(codes(issues)).toEqual([])
  })

  it('does not grant a degraded seat the verdict authority of its namesake', () => {
    expect(canVerdict([rung('cursor', 'claude-opus-5', 'unset')], COLLISION_CATALOG)).toBe(false)
    expect(canVerdict([rung('claude', 'claude-opus-5', 'high')], COLLISION_CATALOG)).toBe(true)
  })

  it('resolves the charging basis through the provider the rung actually names', () => {
    const onCursor = reviewSeat(
      seat({ chain: [rung('cursor', 'claude-opus-5', 'unset')], need: band(0) }),
      COLLISION_CATALOG,
    )
    expect(onCursor.basis).toBe('included_usage')

    const onClaude = reviewSeat(
      seat({ chain: [rung('claude', 'claude-opus-5', 'high')], need: band(0) }),
      COLLISION_CATALOG,
    )
    expect(onClaude.basis).toBe('plan_allowance')
  })

  it('resolves the ceiling through the right contract, so the clamp follows the pair', () => {
    const onCursor = reviewSeat(
      seat({ chain: [rung('cursor', 'claude-opus-5', 'unset')], need: band(0) }),
      COLLISION_CATALOG,
    )
    // The resold route establishes no ceiling; its namesake claims 1M.
    expect(onCursor.resolution?.capability).toBe('unsupported')
    const onClaude = reviewSeat(
      seat({ chain: [rung('claude', 'claude-opus-5', 'high')], need: band(0) }),
      COLLISION_CATALOG,
    )
    expect(onClaude.resolution?.capability).toBe('supported')
  })
})

describe('validateChain', () => {
  it('accepts a chain that crosses providers with efforts every route exposes', () => {
    const issues = validateChain(
      [
        rung('alpha', 'alpha-one', 'high'),
        rung('beta', 'beta-one', 'high'),
        rung('gamma', 'gamma-one', 'medium'),
        rung('alpha', 'alpha-fast', 'low'),
      ],
      FIXTURE_CATALOG,
    )
    expect(issues).toEqual([])
  })

  it('refuses a chain that names no rung at all', () => {
    expect(codes(validateChain([], FIXTURE_CATALOG))).toEqual(['chain_empty'])
  })

  it(`refuses more than ${MAX_RUNGS} rungs`, () => {
    const issues = validateChain(
      [
        rung('alpha', 'alpha-one', 'high'),
        rung('beta', 'beta-one', 'high'),
        rung('gamma', 'gamma-one', 'medium'),
        rung('alpha', 'alpha-two', 'low'),
        rung('beta', 'beta-two', 'low'),
      ],
      FIXTURE_CATALOG,
    )
    expect(codes(issues)).toEqual(['chain_too_long'])
  })

  it('refuses a rung 2 that stays on the provider rung 1 used', () => {
    const issues = validateChain(
      [rung('separate', 'separate-one', 'max'), rung('separate', 'separate-two', 'max')],
      QUOTA_CATALOG,
    )
    expect(codesAt(issues, 'blocking')).toEqual(['rung_2_same_provider'])
  })

  it('blocks a provider that pools its quota from sitting under itself', () => {
    const issues = validateChain(
      [
        rung('separate', 'separate-one', 'max'),
        rung('pooled', 'pooled-one', 'max'),
        rung('pooled', 'pooled-two', 'max'),
      ],
      QUOTA_CATALOG,
    )
    expect(codesAt(issues, 'notice')).toEqual(['provider_repeat'])
    expect(codesAt(issues, 'blocking')).toEqual(['pooled_provider_repeat'])
    expect(issues.find((issue) => issue.code === 'pooled_provider_repeat')?.rung).toBe(3)
  })

  it('blocks the pooled fixture provider under itself, on both rules', () => {
    // The rule is covered by QUOTA_CATALOG above; this also pins the fixture's
    // pooled-provider declaration so the offline preview exercises it.
    const issues = validateChain(
      [rung('beta', 'beta-one', 'high'), rung('beta', 'beta-two', 'low')],
      FIXTURE_CATALOG,
    )
    expect(codesAt(issues, 'blocking')).toEqual([
      'rung_2_same_provider',
      'pooled_provider_repeat',
    ])
  })

  it('flags a provider repeated further down the chain without blocking it', () => {
    const issues = validateChain(
      [
        rung('pooled', 'pooled-one', 'max'),
        rung('separate', 'separate-one', 'max'),
        rung('separate', 'separate-two', 'max'),
      ],
      QUOTA_CATALOG,
    )
    expect(codes(issues)).toEqual(['provider_repeat'])
    expect(blocks(issues)).toBe(false)
  })

  it('refuses an effort the route does not expose', () => {
    const issues = validateChain(
      [rung('alpha', 'alpha-one', 'max'), rung('beta', 'beta-one', 'high')],
      FIXTURE_CATALOG,
    )
    expect(codes(issues)).toEqual(['effort_not_exposed'])
    expect(issues[0]?.rung).toBe(1)
  })

  it('refuses any effort on a route with no effort lever', () => {
    const issues = validateChain(
      [rung('alpha', 'alpha-static', 'high'), rung('beta', 'beta-one', 'high')],
      FIXTURE_CATALOG,
    )
    expect(codes(issues)).toEqual(['effort_not_exposed'])
    expect(issues[0]?.message).toMatch(/silent no-op/)
  })

  it('accepts unset only on a route with no effort lever', () => {
    expect(
      validateChain(
        [rung('alpha', 'alpha-static', 'unset'), rung('beta', 'beta-one', 'high')],
        FIXTURE_CATALOG,
      ),
    ).toEqual([])
    const unpinned = validateChain(
      [rung('alpha', 'alpha-one', 'unset'), rung('beta', 'beta-one', 'high')],
      FIXTURE_CATALOG,
    )
    expect(codes(unpinned)).toEqual(['effort_unset'])
  })

  it('refuses a route the catalog does not serve, and judges nothing else about that rung', () => {
    const issues = validateChain(
      [
        rung('alpha', 'alpha-one', 'high'),
        rung('beta', 'does-not-exist', 'low'),
      ],
      FIXTURE_CATALOG,
    )
    expect(codes(issues)).toEqual(['unknown_model'])
  })

  it('refuses a route that belongs to another provider', () => {
    const issues = validateChain(
      [rung('alpha', 'beta-one', 'high'), rung('beta', 'beta-one', 'high')],
      FIXTURE_CATALOG,
    )
    // Under (provider, id) keying this is simply a pair the catalog does not
    // serve — which is the same refusal, stated without pretending to know which
    // provider the operator meant.
    expect(codes(issues)).toEqual(['unknown_model'])
  })

  it('refuses a provider the catalog does not serve', () => {
    const issues = validateChain(
      [rung('nowhere', 'alpha-one', 'high'), rung('beta', 'beta-one', 'high')],
      FIXTURE_CATALOG,
    )
    expect(codes(issues)).toEqual(['unknown_provider', 'unknown_model'])
  })
})

describe('canVerdict', () => {
  it('is false when rung 1 is a degraded lane', () => {
    expect(canVerdict([rung('p-degraded', 'degraded-route', 'max')], LANE_CATALOG)).toBe(false)
  })

  it('is true when rung 1 is a full lane, whatever sits below it', () => {
    expect(
      canVerdict(
        [rung('p-full', 'full-route', 'max'), rung('p-degraded', 'degraded-route', 'max')],
        LANE_CATALOG,
      ),
    ).toBe(true)
  })

  it('is false for the degraded routes the fixture catalog carries', () => {
    for (const [providerId, route, effort] of [
      ['alpha', 'alpha-static', 'unset'],
      ['gamma', 'gamma-one', 'medium'],
    ] as const) {
      expect(modelById(FIXTURE_CATALOG, providerId, route)).toBeDefined()
      expect(canVerdict([rung(providerId, route, effort)], FIXTURE_CATALOG)).toBe(false)
    }
  })

  it('is false when there is no rung 1, or when the catalog does not serve it', () => {
    expect(canVerdict([], FIXTURE_CATALOG)).toBe(false)
    expect(canVerdict([rung('alpha', 'does-not-exist', 'high')], FIXTURE_CATALOG)).toBe(false)
  })
})

describe('resolveContext', () => {
  it.each([
    ['lean', 128_000, false, 'supported'],
    ['standard', 256_000, false, 'supported'],
    ['deep', 400_000, true, 'clamped'],
    ['extended', 400_000, true, 'clamped'],
    ['native', 400_000, false, 'supported'],
  ] as const)(
    'resolves %s against a 400000 ceiling to %i (%s)',
    (cls, effective, clamped, capability) => {
      const resolution = resolveContext({ class: cls, enforcement: 'best_effort' }, 400_000)
      expect(resolution.effective).toBe(effective)
      expect(resolution.clamped).toBe(clamped)
      expect(resolution.capability).toBe(capability)
      expect(resolution.requested).toBe(classTarget(cls))
    },
  )

  it('leaves every class intact under a ceiling above all of them', () => {
    for (const entry of CONTEXT_CLASSES) {
      const resolution = resolveContext({ class: entry.id, enforcement: 'required' }, 1_000_000)
      expect(resolution.clamped).toBe(false)
      expect(resolution.capability).toBe('supported')
      expect(resolution.effective).toBe(entry.target ?? 1_000_000)
    }
  })

  it('proves nothing about a model whose ceiling nothing establishes', () => {
    const resolution = resolveContext({ class: 'lean', enforcement: 'best_effort' }, null)
    expect(resolution.capability).toBe('unsupported')
    expect(resolution.effective).toBeNull()
    expect(resolution.clamped).toBe(false)
  })

  it('never clamps native, because native names no number to clamp', () => {
    const resolution = resolveContext({ class: 'native', enforcement: 'required' }, 8_000)
    expect(resolution.requested).toBeNull()
    expect(resolution.effective).toBe(8_000)
    expect(resolution.clamped).toBe(false)
  })
})

describe('contextIssues', () => {
  it('refuses a clamp under required enforcement and reports it under best effort', () => {
    const required = contextIssues({ class: 'deep', enforcement: 'required' }, 400_000)
    expect(codesAt(required, 'blocking')).toEqual(['context_clamp_refused'])

    const effort = contextIssues({ class: 'deep', enforcement: 'best_effort' }, 400_000)
    expect(codes(effort)).toEqual(['context_clamped'])
    expect(blocks(effort)).toBe(false)
    expect(effort[0]?.message).toMatch(/400000/)
  })

  it('refuses an unprovable ceiling under required enforcement and records it under best effort', () => {
    expect(codes(contextIssues({ class: 'standard', enforcement: 'required' }, null))).toEqual([
      'context_unenforceable',
    ])
    const effort = contextIssues({ class: 'standard', enforcement: 'best_effort' }, null)
    expect(codes(effort)).toEqual(['context_not_enforced'])
    expect(effort[0]?.message).toMatch(/never records success/)
  })

  it('says nothing about a class the model simply covers', () => {
    expect(contextIssues({ class: 'standard', enforcement: 'required' }, 1_000_000)).toEqual([])
  })

  it('notes that the two explicit-only classes were asked for explicitly', () => {
    for (const cls of ['extended', 'native'] as const) {
      const issues = contextIssues({ class: cls, enforcement: 'best_effort' }, 1_000_000)
      expect(codes(issues)).toEqual(['context_explicit_only'])
      expect(blocks(issues)).toBe(false)
    }
  })
})

describe('pricing', () => {
  const flat = model({ id: 'flat', provider: 'p', window: 1_000_000 })

  /**
   * A two-step curve, built here rather than taken from the catalog.
   *
   * No fixture entry is stepped: the one real stepped curve in this fleet is a
   * Codex one whose boundary is unverified against a second source, so it is not
   * seeded. A synthetic curve keeps the machinery tested without any test
   * claiming a real provider charges this way.
   */
  const stepped = model({
    id: 'stepped',
    provider: 'p',
    window: 1_000_000,
    pricing: [
      { window: 0, inputPerMtok: 5, outputPerMtok: 25, provenance: UNVERIFIED },
      { window: 200_000, inputPerMtok: 10, outputPerMtok: 50, provenance: UNVERIFIED },
    ],
  })

  const unpriced = model({ id: 'unpriced', provider: 'p', window: 1_000_000, pricing: [] })

  it('takes the highest price step at or below the prompt size', () => {
    expect(priceTierFor(stepped, 128_000)?.window).toBe(0)
    expect(priceTierFor(stepped, 200_000)?.window).toBe(200_000)
    expect(priceTierFor(stepped, 256_000)?.window).toBe(200_000)
    expect(priceTierFor(stepped, 1_000_000)?.window).toBe(200_000)
  })

  it('reprices the whole request at the step it lands on, not only the excess', () => {
    expect(classCost('lean', stepped).inputUsd).toBeCloseTo((5 * 128_000) / 1_000_000, 6)
    expect(classCost('standard', stepped).inputUsd).toBeCloseTo((10 * 256_000) / 1_000_000, 6)
  })

  it('has no step to take when nothing is priced or nothing is known', () => {
    expect(priceTierFor(unpriced, 128_000)).toBeNull()
    expect(priceTierFor(stepped, null)).toBeNull()
  })

  it('costs the input needed to fill the effective threshold, and nothing else', () => {
    expect(classCost('lean', flat).inputUsd).toBeCloseTo((1 * 128_000) / 1_000_000, 6)
    expect(classCost('standard', flat).inputUsd).toBeCloseTo((1 * 256_000) / 1_000_000, 6)
  })

  it('prices no output at all, whatever the tier charges for it', () => {
    const dearOutput = model({
      id: 'dear',
      provider: 'p',
      window: 1_000_000,
      pricing: stepped.pricing.map((tier) => ({
        ...tier,
        outputPerMtok: tier.outputPerMtok * 1000,
      })),
    })
    for (const entry of CONTEXT_CLASSES) {
      expect(classCost(entry.id, dearOutput).inputUsd).toBe(classCost(entry.id, stepped).inputUsd)
    }
  })

  it('costs a clamped class at what actually runs, not at what was asked for', () => {
    const small = model({ id: 'small', provider: 'p', window: 400_000 })
    const deep = classCost('deep', small)
    expect(deep.resolution.capability).toBe('clamped')
    expect(deep.inputUsd).toBeCloseTo((1 * 400_000) / 1_000_000, 6)
  })

  it('reports no cost rather than a zero for a route with no published price', () => {
    const lean = classCost('lean', unpriced)
    expect(lean.tier).toBeNull()
    expect(lean.inputUsd).toBeNull()
  })

  it('places each class against the cheapest one only where the curve steps', () => {
    const rows = classCosts(stepped)
    expect(rows.find((row) => row.class === 'lean')?.relative).toBeCloseTo(1, 6)
    expect(rows.find((row) => row.class === 'standard')?.relative).toBeCloseTo(
      (10 * 256_000) / (5 * 128_000),
      6,
    )
  })

  it('withholds the ratio on a flat curve, where it only restates the class targets', () => {
    // On one tier the cost is exactly proportional to the threshold, so the
    // ratio is the ratio of `CONTEXT_CLASSES` — arithmetic dressed as economics.
    expect(classCosts(flat).every((row) => row.relative === null)).toBe(true)
    expect(classCosts(unpriced).every((row) => row.relative === null)).toBe(true)
  })
})

describe('recommendClass', () => {
  const wide = model({ id: 'wide', provider: 'p', window: 1_000_000 })
  const narrow = model({ id: 'narrow', provider: 'p', window: 400_000 })
  const unknown = model({ id: 'unknown', provider: 'p', window: null })

  it('takes the smallest class that covers the band', () => {
    expect(recommendClass(band(100_000), wide)).toBe('lean')
    expect(recommendClass(band(240_000), wide)).toBe('standard')
    expect(recommendClass(band(300_000), wide)).toBe('deep')
  })

  it('counts a clamped class under best effort, where a clamp is only a notice', () => {
    expect(recommendClass(band(300_000), narrow, 'best_effort')).toBe('deep')
  })

  it('will not recommend a class that required enforcement would block', () => {
    // `deep` clamps 512K to 400K here. Under `best_effort` that is a visible
    // notice and the class is usable; under `required` the same clamp is a
    // blocking refusal, so recommending it would print a recommendation directly
    // above an issue refusing it.
    expect(recommendClass(band(300_000), narrow, 'required')).toBeNull()
    expect(recommendClass(band(200_000), narrow, 'required')).toBe('standard')
  })

  it('recommends nothing when the band is above the model ceiling', () => {
    expect(recommendClass(band(500_000), narrow)).toBeNull()
  })

  it('recommends nothing when nothing establishes a ceiling to compare against', () => {
    expect(recommendClass(band(1_000), unknown)).toBeNull()
  })

  it('recommends nothing for a band that is not a count of tokens', () => {
    expect(recommendClass({ ...band(0), minTokens: -1 }, wide)).toBeNull()
    expect(recommendClass({ ...band(0), minTokens: Number.NaN }, wide)).toBeNull()
  })

  it('cannot be moved by output pricing, at any output rate', () => {
    const inverted = model({
      id: 'inverted',
      provider: 'p',
      window: 1_000_000,
      pricing: [
        { window: 0, inputPerMtok: 5, outputPerMtok: 900, provenance: UNVERIFIED },
        { window: 200_000, inputPerMtok: 10, outputPerMtok: 1, provenance: UNVERIFIED },
      ],
    })
    for (const tokens of [0, 100_000, 240_000, 300_000, 600_000]) {
      expect(recommendClass(band(tokens), inverted)).toBe(recommendClass(band(tokens), wide))
    }
  })
})

describe('reviewSeat', () => {
  it('composes the context rules and the need rules', () => {
    const review = reviewSeat(
      seat({
        chain: [rung('alpha', 'alpha-two', 'low'), rung('beta', 'beta-one', 'high')],
        context: { class: 'deep', enforcement: 'required' },
      }),
      FIXTURE_CATALOG,
    )
    expect(codes(review.issues)).toEqual(
      expect.arrayContaining(['context_unenforceable']),
    )
    expect(blocks(review.issues)).toBe(true)
    expect(review.leadModel?.id).toBe('alpha-two')
    expect(review.costs).toHaveLength(CONTEXT_CLASSES.length)
  })

  it('blocks a need band no class reaches rather than reaching for native', () => {
    const review = reviewSeat(
      seat({
        chain: [rung('alpha', 'alpha-one', 'high')],
        need: band(2_000_000),
      }),
      FIXTURE_CATALOG,
    )
    expect(codesAt(review.issues, 'blocking')).toContain('need_uncovered')
    expect(review.recommended).toBeNull()
    expect(review.issues.find((issue) => issue.code === 'need_uncovered')?.message).toMatch(
      /not a reason to promote the seat to native/,
    )
  })

  it('says a band cannot be checked when nothing establishes the ceiling', () => {
    const review = reviewSeat(
      seat({
        chain: [rung('gamma', 'gamma-one', 'medium'), rung('beta', 'beta-two', 'medium')],
        context: { class: 'lean', enforcement: 'best_effort' },
      }),
      FIXTURE_CATALOG,
    )
    expect(codesAt(review.issues, 'notice')).toEqual(
      expect.arrayContaining(['context_not_enforced', 'need_unresolvable']),
    )
    expect(codes(review.issues)).not.toContain('need_uncovered')
    expect(blocks(review.issues)).toBe(false)
    expect(review.canVerdict).toBe(false)
  })

  it('notes a seat configured under the band it declares, and names what would cover it', () => {
    const review = reviewSeat(
      seat({
        chain: [rung('alpha', 'alpha-one', 'high'), rung('beta', 'beta-one', 'high')],
        context: { class: 'lean', enforcement: 'best_effort' },
        need: band(200_000),
      }),
      FIXTURE_CATALOG,
    )
    expect(codesAt(review.issues, 'notice')).toEqual(['need_unmet_by_class'])
    expect(review.recommended).toBe('standard')
    // The operator is told what covers it, not what is "cheapest" — a ranking
    // across four different scarce resources that compares nothing.
    expect(review.issues[0]?.message).toMatch(/smallest covering class/)
    expect(review.issues[0]?.message).not.toMatch(/cheapest/i)
  })

  it('refuses a band that is not a count of tokens', () => {
    const review = reviewSeat(seat({ need: { ...band(0), minTokens: -5 } }), FIXTURE_CATALOG)
    expect(codesAt(review.issues, 'blocking')).toEqual(['need_invalid'])
  })

  it('resolves nothing against a rung 1 the catalog does not serve', () => {
    const review = reviewSeat(
      seat({ chain: [rung('alpha', 'does-not-exist', 'high')] }),
      FIXTURE_CATALOG,
    )
    expect(review.leadModel).toBeNull()
    expect(review.resolution).toBeNull()
    expect(review.costs).toEqual([])
    expect(codesAt(review.issues, 'blocking')).toEqual(['unknown_model'])
  })

  it('resolves every rung, not only the first', () => {
    const review = reviewSeat(
      seat({ chain: [rung('beta', 'beta-one', 'high'), rung('alpha', 'alpha-one', 'high')] }),
      FIXTURE_CATALOG,
    )
    expect(review.rungs.map((entry) => entry.rung)).toEqual([1, 2])
    expect(review.rungs[0]?.model?.id).toBe('beta-one')
    expect(review.rungs[1]?.model?.id).toBe('alpha-one')
  })

  it('says so when a fallback rung cannot honour the band the seat declares', () => {
    // A chain exists so the lower rungs get used. Resolving only rung 1 meant a
    // seat could declare a need its own fallback provably cannot meet and the
    // editor would print "Nothing to report".
    const catalog: ModelCatalog = {
      providers: [provider('big', 'metered'), provider('small', 'metered')],
      models: [
        model({ id: 'roomy', provider: 'big', window: 1_000_000 }),
        model({ id: 'cramped', provider: 'small', window: 128_000 }),
      ],
    }
    const review = reviewSeat(
      seat({
        chain: [rung('big', 'roomy', 'high'), rung('small', 'cramped', 'high')],
        need: band(300_000),
      }),
      catalog,
    )
    const fallback = review.issues.find((issue) => issue.code === 'need_uncovered_on_fallback')
    expect(fallback?.severity).toBe('notice')
    expect(fallback?.rung).toBe(2)
    // Rung 1 covers it, so the seat is still publishable.
    expect(blocks(review.issues)).toBe(false)
  })

  it('is silent about a fallback whose ceiling nothing establishes', () => {
    // Not knowing is not the same as not covering.
    const review = reviewSeat(
      seat({
        chain: [rung('alpha', 'alpha-one', 'high'), rung('beta', 'beta-one', 'high')],
        need: band(100_000),
      }),
      FIXTURE_CATALOG,
    )
    expect(codes(review.issues)).not.toContain('need_uncovered_on_fallback')
  })
})

describe('validateTeam', () => {
  const draft: TeamDraft = {
    id: 'd-1',
    name: 'a template',
    slots: [
      { id: 's-1', role: anyRole, capabilities: seat() },
      { id: 's-2', role: anyRole, capabilities: seat() },
    ],
  }

  it('accepts a named draft with distinct slots', () => {
    expect(validateTeam(draft)).toEqual([])
  })

  it('refuses two slots with the same key, once per key', () => {
    const issues = validateTeam({
      ...draft,
      slots: [
        { id: 's-1', role: anyRole, capabilities: seat() },
        { id: 's-1', role: anyRole, capabilities: seat() },
        { id: 's-1', role: anyRole, capabilities: seat() },
      ],
    })
    expect(codes(issues)).toEqual(['duplicate_slot_id'])
  })

  it('refuses a draft with nobody in it, and one with no name', () => {
    expect(codes(validateTeam({ ...draft, slots: [] }))).toEqual(['team_empty'])
    expect(codes(validateTeam({ ...draft, name: '   ' }))).toEqual(['team_unnamed'])
  })
})

describe('the effort ladder', () => {
  it('admits every level the runtime exposes, as raw ids', () => {
    // The closed list omitted `ultra` (exposed by the fleet's primary architect
    // routes), `ultracode` (both Claude routes) and `off`, so the editor would
    // have refused a legitimate pin. The same live call that killed a fabricated
    // route also proved these real ones were being denied.
    for (const level of ['off', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra', 'ultracode']) {
      expect(EFFORT_LEVELS).toContain(level as EffortLevel)
    }
  })

  it('carries the ladder each fake route exposes', () => {
    const ladders = Object.fromEntries(
      FIXTURE_CATALOG.models.map((entry) => [`${entry.provider}/${entry.id}`, entry.efforts.value]),
    )
    expect(ladders).toEqual({
      'alpha/alpha-one': ['low', 'high'],
      'alpha/alpha-two': ['off', 'low', 'high'],
      'alpha/alpha-fast': ['low'],
      'alpha/alpha-static': [],
      'beta/beta-one': ['low', 'medium', 'high'],
      'beta/beta-two': ['low', 'medium'],
      'gamma/gamma-one': ['medium', 'high'],
      'gamma/gamma-two': ['high', 'max'],
    })
  })

  it('accepts the levels the earlier closed list would have refused', () => {
    expect(
      validateChain(
        [rung('beta', 'beta-one', 'high'), rung('gamma', 'gamma-two', 'high')],
        FIXTURE_CATALOG,
      ),
    ).toEqual([])
    expect(
      validateChain([rung('alpha', 'alpha-two', 'off')], FIXTURE_CATALOG),
    ).toEqual([])
  })
})

describe('the fixtures', () => {
  it('is obviously fake and claims no established cell', () => {
    for (const entry of FIXTURE_CATALOG.models) {
      expect(entry.contextWindow.provenance.state).toBe('fixture/needs-verification')
      expect(entry.efforts.provenance.state).toBe('fixture/needs-verification')
      expect(entry.contextWindow.provenance.reviewRef).toBeNull()
      expect(entry.efforts.provenance.reviewRef).toBeNull()
      expect(entry.label).toContain('offline fixture')
    }
    for (const provider of FIXTURE_CATALOG.providers) {
      expect(provider.label).toContain('offline fixture')
      expect(provider.basis.provenance.state).toBe('fixture/needs-verification')
    }
  })

  it('names no real provider or model route', () => {
    const routeIds = FIXTURE_CATALOG.models.map((entry) => entry.id)
    const providerIds = FIXTURE_CATALOG.providers.map((entry) => entry.id)
    for (const invented of ['claude', 'codex', 'cursor', 'deepseek', 'openrouter']) {
      expect(providerIds).not.toContain(invented)
    }
    for (const invented of ['claude-opus-5', 'gpt-5.6-sol', 'auto-smart', 'deepseek-flash']) {
      expect(routeIds).not.toContain(invented)
    }
  })

  it('records the one vendor reached through another provider', () => {
    const reached = Object.fromEntries(
      FIXTURE_CATALOG.providers.map((entry) => [entry.id, entry.reachedVia]),
    )
    expect(reached).toEqual({
      alpha: null,
      beta: null,
      gamma: 'beta',
    })
  })

  it('marks every price as unverified', () => {
    // The research gate is the point: the day a price is relabelled, it has to
    // be because a reviewed matrix said so — and this assertion makes that a
    // deliberate act rather than an edit.
    for (const entry of FIXTURE_CATALOG.models) {
      for (const tier of entry.pricing) {
        expect(tier.provenance.state).toBe('fixture/needs-verification')
        expect(tier.provenance.reviewRef).toBeNull()
      }
    }
  })

  it('marks every need band unverified, because they were read off the class table', () => {
    for (const draft of SEED_TEAMS) {
      for (const slot of draft.slots) {
        expect(slot.capabilities.need.provenance.state).toBe('fixture/needs-verification')
        expect(slot.capabilities.need.provenance.reviewRef).toBeNull()
      }
    }
  })

  it('states only the fake ceilings it declares', () => {
    const windows = Object.fromEntries(
      FIXTURE_CATALOG.models.map((entry) => [
        `${entry.provider}/${entry.id}`,
        entry.contextWindow.value,
      ]),
    )
    expect(windows).toEqual({
      'alpha/alpha-one': 300_000,
      'alpha/alpha-two': null,
      'alpha/alpha-fast': null,
      'alpha/alpha-static': null,
      'beta/beta-one': null,
      'beta/beta-two': null,
      'gamma/gamma-one': null,
      'gamma/gamma-two': 500_000,
    })
  })

  it('serves only routes the fake catalog lists', () => {
    const alpha = FIXTURE_CATALOG.models
      .filter((entry) => entry.provider === 'alpha')
      .map((entry) => entry.id)
    expect(alpha).toEqual(['alpha-one', 'alpha-two', 'alpha-fast', 'alpha-static'])
    expect(modelById(FIXTURE_CATALOG, 'alpha', 'alpha-one')?.isDefault).toBe(true)
    expect(modelById(FIXTURE_CATALOG, 'alpha', 'default')).toBeUndefined()

    for (const draft of SEED_TEAMS) {
      for (const slot of draft.slots) {
        for (const entry of slot.capabilities.chain) {
          expect(modelById(FIXTURE_CATALOG, entry.provider, entry.model)).toBeDefined()
        }
      }
    }
  })

  it('pins every seeded rung to a fixture route', () => {
    const chains = Object.fromEntries(
      SEED_TEAMS.flatMap((draft) =>
        draft.slots.map((slot) => [
          slot.id,
          slot.capabilities.chain.map((entry) => `${entry.provider}/${entry.model}@${entry.effort}`),
        ]),
      ),
    )
    expect(chains['implementer']).toEqual([
      'alpha/alpha-one@high',
      'beta/beta-one@medium',
      'gamma/gamma-one@medium',
      'beta/beta-two@low',
    ])
    expect(chains['qa']?.[2]).toBe('gamma/gamma-one@high')
    expect(chains['orchestrator']?.[2]).toBe('beta/beta-one@medium')
    expect(chains['builder-chore']?.[2]).toBe('beta/beta-two@low')
  })

  it('seeds no draft that could not be published', () => {
    for (const draft of SEED_TEAMS) {
      expect(blocks(validateTeam(draft))).toBe(false)
      for (const slot of draft.slots) {
        const review = reviewSeat(slot.capabilities, FIXTURE_CATALOG)
        expect({ slot: slot.id, blocking: codesAt(review.issues, 'blocking') }).toEqual({
          slot: slot.id,
          blocking: [],
        })
      }
    }
  })

  it('stacks no pooled provider on itself in any seeded chain', () => {
    // This replaced an assertion that the standard builder carried a
    // `provider_repeat` *notice* — the fixture's way of showing it did not hide
    // a deviation the fleet policy carries. Once `codex.pooledUsage` is true
    // that same repeat is `pooled_provider_repeat`, blocking, so the honest
    // invariant is structural: an unreachable rung is not a deviation to
    // display, it is a rung that never runs. The policy document still needs the
    // same correction — `docs/QUOTA-FALLBACK-PLAN.md`, "Open question".
    for (const draft of SEED_TEAMS) {
      for (const slot of draft.slots) {
        const chain = slot.capabilities.chain
        const pooledRepeat = chain.some(
          (rung, index) =>
            index > 0 &&
            rung.provider === chain[index - 1]?.provider &&
            FIXTURE_CATALOG.providers.find((entry) => entry.id === rung.provider)
              ?.pooledUsage === true,
        )
        expect({ slot: slot.id, pooledRepeat }).toEqual({ slot: slot.id, pooledRepeat: false })
      }
    }
  })

  it('names every class the closed set names, and no other', () => {
    expect(CONTEXT_CLASSES.map((entry) => entry.id)).toEqual([
      'lean',
      'standard',
      'deep',
      'extended',
      'native',
    ])
    expect(CONTEXT_CLASSES.map((entry) => entry.target)).toEqual([
      128_000,
      256_000,
      512_000,
      720_000,
      null,
    ])
    expect(classTarget('native' as ContextClass)).toBeNull()
  })
})
