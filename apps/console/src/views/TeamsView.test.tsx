/**
 * The Teams section: constrained controls, and no quiet repairs.
 *
 * The mutants this file exists to kill: a model select that keeps offering
 * another provider's routes, an effort select that stays live on a route with no
 * effort lever, a clamp that renders the same under `best_effort` and
 * `required`, a verdict badge that is set rather than derived, a price shown
 * without saying it is unverified, and an editor that silently promotes a seat
 * to a class that covers its need band instead of reporting that nothing does.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { TeamsView } from './TeamsView'
import { setViewport } from '../test/viewport'
import { UNVERIFIED, unverified } from '../state/teams'
import type {
  ChargingBasis,
  EffortLevel,
  ModelCatalog,
  TeamDraft,
} from '../state/teams'

/**
 * A catalog whose one price step says where it came from.
 *
 * The output rate is deliberately absurd: if it ever reaches a rendered figure,
 * the arithmetic says so loudly rather than plausibly.
 */
/** Any well-formed role selection; this suite is about model routing, not roles. */
const anyRole = { catalog_revision: { id: 'standard-roles', version: 1 }, role_code: 'SWE' }

const SOURCED_CATALOG: ModelCatalog = {
  providers: [
    {
      id: 'p',
      label: 'P',
      basis: unverified<ChargingBasis>('metered'),
      reachedVia: null,
      pooledUsage: false,
    },
  ],
  models: [
    {
      id: 'm',
      label: 'M',
      provider: 'p',
      isDefault: false,
      contextWindow: unverified<number | null>(1_000_000),
      efforts: unverified<readonly EffortLevel[]>(['high']),
      pricing: [
        {
          window: 0,
          inputPerMtok: 4,
          outputPerMtok: 999,
          // A step that says where it came from, so the dollar is printable.
          provenance: {
            state: 'researched',
            reviewRef: 'TEST-RECORD',
            citation: 'a provider page',
            observedAt: '2026-08-14',
          },
        },
        // A second step, so the ratio column is not suppressed as flat.
        {
          window: 512_000,
          inputPerMtok: 8,
          outputPerMtok: 999,
          provenance: {
            state: 'researched',
            reviewRef: 'TEST-RECORD',
            citation: 'a provider page',
            observedAt: '2026-08-14',
          },
        },
      ],
      degradedLane: false,
    },
  ],
}

/** One draft on that catalog. */
const SOURCED_SEED: readonly TeamDraft[] = [
  {
    id: 'd-sourced',
    name: 'a sourced draft',
    slots: [
      {
        id: 'seat',
        role: anyRole,
        capabilities: {
          chain: [{ provider: 'p', model: 'm', effort: 'high' }],
          context: { class: 'lean', enforcement: 'best_effort' },
          need: { minTokens: 100_000, rationale: null, provenance: UNVERIFIED },
          skills: [],
          mayEvaluate: [],
          mayWaive: [],
        },
      },
    ],
  },
]

/**
 * A metered provider with a stated ceiling and an unverified price.
 *
 * The shipped fixture deliberately no longer has such a route: the ceilings it
 * used to state were unsourced and were withdrawn, so the clamp, the class table
 * and the withheld-dollar behaviours have to be exercised against a catalog that
 * declares what it is rather than against a number nobody verified.
 */
const BENCH_CATALOG: ModelCatalog = {
  providers: [
    {
      id: 'b',
      label: 'B',
      basis: unverified<ChargingBasis>('metered'),
      reachedVia: null,
      pooledUsage: false,
    },
  ],
  models: [
    {
      id: 'bench',
      label: 'Bench',
      provider: 'b',
      isDefault: false,
      contextWindow: unverified<number | null>(400_000),
      efforts: unverified<readonly EffortLevel[]>(['high', 'max']),
      pricing: [{ window: 0, inputPerMtok: 1, outputPerMtok: 2, provenance: UNVERIFIED }],
      degradedLane: false,
    },
  ],
}

/** One draft on the bench catalog, declaring a class its ceiling cannot hold. */
function benchSeed(minTokens = 100_000): readonly TeamDraft[] {
  return [
    {
      id: 'd-bench',
      name: 'a bench draft',
      slots: [
        {
          id: 'seat',
          role: anyRole,
          capabilities: {
            chain: [{ provider: 'b', model: 'bench', effort: 'high' }],
            context: { class: 'deep', enforcement: 'best_effort' },
            need: { minTokens, rationale: null, provenance: UNVERIFIED },
            skills: [],
            mayEvaluate: [],
            mayWaive: [],
          },
        },
      ],
    },
  ]
}

/** Open the bench draft. */
function openBench(minTokens?: number): HTMLElement {
  const { container } = render(
    <TeamsView catalog={BENCH_CATALOG} seed={benchSeed(minTokens)} />,
  )
  fireEvent.click(screen.getByRole('button', { name: /a bench draft/ }))
  return container
}

/** Open one seeded draft and hand back the container it was rendered into. */
function openDraft(name: RegExp): HTMLElement {
  const { container } = render(<TeamsView />)
  fireEvent.click(screen.getByRole('button', { name }))
  return container
}

/** One slot's editor. */
function slotOf(container: HTMLElement, id: string): HTMLElement {
  return container.querySelector(`[data-slot="${id}"]`) as HTMLElement
}

/** The class-table body, so a row count is a count of rows. */
function rowsOf(table: HTMLElement): HTMLElement {
  return table.querySelector('tbody') as HTMLElement
}

/** One chain row inside a slot's editor. */
function rungOf(slot: HTMLElement, number: number): HTMLElement {
  return slot.querySelector(`[data-rung="${number}"]`) as HTMLElement
}

describe('<TeamsView>', () => {
  beforeEach(() => setViewport('desktop'))

  it('says on screen that nothing here came from the realm', () => {
    render(<TeamsView />)
    const banner = screen.getByRole('note')
    expect(banner).toHaveTextContent(/Nothing on this screen came from the realm/)
    expect(banner).toHaveTextContent(/fixture\/needs-verification/)
    expect(banner).toHaveTextContent(/immutable snapshots/i)
  })

  it('loads the live realm catalog and projection before rendering an editor', async () => {
    const client = {
      modelCatalog: vi.fn(async () => ({
        realm_id: 'realm-live', snapshot_cursor: 8,
        providers: SOURCED_CATALOG.providers, models: SOURCED_CATALOG.models,
      })),
      teams: vi.fn(async () => ({
        realm_id: 'realm-live', snapshot_cursor: 9,
        drafts: SOURCED_SEED, revisions: [],
      })),
      saveTeamDraft: vi.fn(),
      publishTeam: vi.fn(),
    }
    render(<TeamsView client={client as never} />)
    expect(screen.getByText(/Loading the realm catalog/)).toBeInTheDocument()
    const banner = await screen.findByRole('note')
    expect(banner).toHaveAttribute('data-banner', 'realm-live')
    expect(banner).toHaveTextContent('cursor 8')
    expect(screen.getByRole('button', { name: /a sourced draft/ })).toBeInTheDocument()
  })

  it('kills validateCatalog removal on the live /v1/catalog response', async () => {
    const unsigned: ModelCatalog = {
      ...SOURCED_CATALOG,
      models: SOURCED_CATALOG.models.map((model) => ({
        ...model,
        contextWindow: {
          ...model.contextWindow,
          provenance: { ...model.contextWindow.provenance, state: 'live', reviewRef: null },
        },
      })),
    }
    const client = {
      modelCatalog: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 4, ...unsigned })),
      teams: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 4, drafts: SOURCED_SEED, revisions: [] })),
      saveTeamDraft: vi.fn(), publishTeam: vi.fn(),
    }
    render(<TeamsView client={client as never} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('/v1/catalog')
    expect(screen.queryByRole('list', { name: 'team templates' })).toBeNull()
  })

  it('saves then publishes through the live realm and renders its revision cursor', async () => {
    const published = {
      realm_id: 'realm-live', snapshot_cursor: 12,
      drafts: SOURCED_SEED,
      revisions: [{ ...SOURCED_SEED[0], version: 1 }],
    }
    const client = {
      modelCatalog: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 10, ...SOURCED_CATALOG })),
      teams: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 10, drafts: SOURCED_SEED, revisions: [] })),
      saveTeamDraft: vi.fn(async () => ({ ...published, snapshot_cursor: 11, revisions: [] })),
      publishTeam: vi.fn(async () => published),
    }
    render(<TeamsView client={client as never} />)
    fireEvent.click(await screen.findByRole('button', { name: /a sourced draft/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Publish next revision' }))
    await waitFor(() => expect(client.publishTeam).toHaveBeenCalledWith('d-sourced', expect.any(String)))
    expect(client.saveTeamDraft).toHaveBeenCalledTimes(1)
    expect(await screen.findByText('a sourced draft · v1')).toBeInTheDocument()
    expect(screen.getByRole('note')).toHaveTextContent('cursor 12')
  })

  it('refuses an unsigned promoted catalog at the /v1/catalog trust boundary', () => {
    const catalog: ModelCatalog = {
      ...BENCH_CATALOG,
      models: BENCH_CATALOG.models.map((entry) => ({
        ...entry,
        contextWindow: {
          value: entry.contextWindow.value,
          provenance: { ...UNVERIFIED, state: 'live' },
        },
      })),
    }
    render(<TeamsView catalog={catalog} seed={benchSeed()} />)
    expect(screen.getByRole('alert')).toHaveTextContent('/v1/catalog')
    expect(screen.queryByRole('list', { name: 'team templates' })).toBeNull()
  })

  it('claims provenance for exactly the five cell classes it renders', () => {
    // The banner used to promise more than the screen delivered: it said every
    // cell states its provenance while only the ceiling and the price did. It
    // now names all five, and each one below is rendered.
    render(<TeamsView />)
    const banner = screen.getByRole('note')
    for (const cell of [
      /context ceiling/i,
      /effort ladder/i,
      /charging basis/i,
      /a price/i,
      /a need band/i,
    ]) {
      expect(banner).toHaveTextContent(cell)
    }
  })

  it('does not carry the malformed sentence fragment', () => {
    render(<TeamsView />)
    const banner = screen.getByRole('note')
    expect(banner).not.toHaveTextContent(/coverage, which is\./)
    expect(banner).toHaveTextContent(/recommendation rests on coverage instead/)
  })

  it('renders the provenance of the effort ladder, the basis and the need band', () => {
    // The three the banner claimed and the screen did not show.
    const container = openDraft(/plan-build-verify/)
    const architect = slotOf(container, 'architect')

    // Effort ladder, per rung — it decides which pins the editor accepts.
    expect(
      within(rungOf(architect, 1)).getByTitle('rung 1 effort ladder provenance: live'),
    ).toHaveTextContent('live')

    // Need band, per seat — it drives a blocking rule.
    expect(
      within(architect).getByTitle(
        'architect need band provenance: fixture/needs-verification',
      ),
    ).toHaveTextContent('fixture/needs-verification')

    // Charging basis, per provider — it decides whether a dollar prints at all.
    const caption = within(architect).getByRole('table').querySelector('caption') as HTMLElement
    expect(within(caption).getByTitle('charging basis: plan_allowance')).toBeInTheDocument()
    expect(
      within(caption).getByTitle('charging basis provenance: fixture/needs-verification'),
    ).toBeInTheDocument()
  })

  it('lists the seeded drafts and edits none of them until one is chosen', () => {
    render(<TeamsView />)
    const list = screen.getByRole('list', { name: 'team templates' })
    expect(within(list).getAllByRole('button')).toHaveLength(2)
    expect(screen.getByText('Select a team template.')).toBeInTheDocument()
  })

  it('opens a draft into one capability editor per slot', () => {
    const container = openDraft(/plan-build-verify/)
    const slots = container.querySelectorAll('[data-slot]')
    expect(Array.from(slots).map((slot) => slot.getAttribute('data-slot'))).toEqual([
      'architect',
      'implementer',
      'qa',
      'audit',
    ])
    const chosen = screen.getByRole('button', { name: /plan-build-verify/ })
    expect(chosen).toHaveAttribute('aria-current', 'true')
  })

  it('narrows the model select to the chosen provider and the effort select to that model', () => {
    const container = openDraft(/plan-build-verify/)
    const rung = rungOf(slotOf(container, 'architect'), 1)

    const provider = within(rung).getByLabelText('Provider')
    const modelSelect = within(rung).getByLabelText('Model')
    const effort = within(rung).getByLabelText('Effort')

    expect(provider).toHaveValue('codex')
    expect(modelSelect).toHaveValue('gpt-5.6-sol')
    expect(effort).toHaveValue('xhigh')

    fireEvent.change(provider, { target: { value: 'deepseek' } })

    // Only that provider's routes are on offer, and the effort that was pinned
    // is not one this route exposes, so it moved to one that is.
    const offered = within(within(rung).getByLabelText('Model')).getAllByRole('option')
    expect(offered.map((option) => option.textContent)).toEqual(['DeepSeek V4 Flash'])
    // The live ladder for this route is low/high/max — `xhigh` is not on it, so
    // the pin moved to a level the route actually exposes.
    expect(within(rung).getByLabelText('Effort')).toHaveValue('low')
    expect(
      within(within(rung).getByLabelText('Effort')).getAllByRole('option').map((o) => o.textContent),
    ).toEqual(['low', 'high', 'max'])
  })

  it('forces unset and disables the control on a route with no effort lever', () => {
    const container = openDraft(/plan-build-verify/)
    const rung = rungOf(slotOf(container, 'architect'), 1)

    fireEvent.change(within(rung).getByLabelText('Provider'), { target: { value: 'cursor' } })

    const effort = within(rung).getByLabelText('Effort')
    expect(effort).toBeDisabled()
    expect(effort).toHaveValue('unset')
    // Unset is the correct declaration there, so it is not an issue.
    const issues = within(slotOf(container, 'architect')).getByRole('list', {
      name: 'architect issues',
    })
    expect(issues.querySelector('[data-code="effort_unset"]')).toBeNull()
    expect(issues.querySelector('[data-code="effort_not_exposed"]')).toBeNull()
  })

  it('keeps a pinned effort the new route also exposes', () => {
    const container = openDraft(/plan-build-verify/)
    const rung = rungOf(slotOf(container, 'architect'), 1)
    fireEvent.change(within(rung).getByLabelText('Model'), { target: { value: 'gpt-5.6-terra' } })
    expect(within(rung).getByLabelText('Effort')).toHaveValue('xhigh')
  })

  it('reports a clamp as a notice under best effort and as a refusal under required', () => {
    const container = openBench()
    const seat = slotOf(container, 'seat')

    // `deep` asks for 512000; this route states 400000.
    const clamped = seat.querySelector('[data-code="context_clamped"]') as HTMLElement
    expect(clamped).toHaveAttribute('data-severity', 'notice')
    expect(clamped).toHaveTextContent(/400000/)

    fireEvent.change(within(seat).getByLabelText('Enforcement'), {
      target: { value: 'required' },
    })

    expect(seat.querySelector('[data-code="context_clamped"]')).toBeNull()
    const refused = seat.querySelector('[data-code="context_clamp_refused"]') as HTMLElement
    expect(refused).toHaveAttribute('data-severity', 'blocking')
  })

  it('shows every class against the rung 1 ceiling', () => {
    const container = openDraft(/plan-build-verify/)
    const table = within(slotOf(container, 'architect')).getByRole('table')

    const rows = Array.from(table.querySelectorAll('tbody tr'))
    expect(rows.map((row) => row.getAttribute('data-class'))).toEqual([
      'lean',
      'standard',
      'deep',
      'extended',
      'native',
    ])
    // Nothing establishes this route's ceiling, so no class can be shown to hold
    // — which is a different answer from "it fits", and the table says so.
    expect(rows.map((row) => row.getAttribute('data-capability'))).toEqual([
      'unsupported',
      'unsupported',
      'unsupported',
      'unsupported',
      'supported',
    ])
    expect(table.querySelector('tr[data-selected="true"]')?.getAttribute('data-class')).toBe('deep')
  })

  it('renders the complete resolved-policy preview', () => {
    const container = openBench()
    const preview = slotOf(container, 'seat').querySelector('[data-resolved-policy]') as HTMLElement
    expect(preview).toHaveTextContent('class deep')
    expect(preview).toHaveTextContent('source role_slot')
    expect(preview).toHaveTextContent('effective 400000')
    expect(preview).toHaveTextContent('enforcement best_effort')
    expect(preview).toHaveTextContent('capability clamped')
    expect(preview).toHaveTextContent('latest receipt none')
  })

  it('shows a stated ceiling clamping the classes above it', () => {
    const container = openBench()
    const table = within(slotOf(container, 'seat')).getByRole('table')
    expect(
      Array.from(table.querySelectorAll('tbody tr')).map((row) =>
        row.getAttribute('data-capability'),
      ),
    ).toEqual(['supported', 'supported', 'clamped', 'clamped', 'supported'])
  })

  it('withholds the dollar figure while a metered price step is unverified', () => {
    // A metered provider with a stated ceiling is the one place a dollar would be
    // the right *kind* of answer — and it is still withheld, because nobody can
    // tell a researched number from an invented one by looking at it. The ratio
    // goes with it: a multiple of an invented figure is invented too.
    const container = openBench()
    const table = within(slotOf(container, 'seat')).getByRole('table')

    expect(within(table).getAllByText('price not verified')).toHaveLength(5)
    expect(within(table).queryByText(/^\$/)).toBeNull()
    expect(within(table).queryByText(/x$/)).toBeNull()
    // The provenance itself is still stated, on every row.
    expect(within(rowsOf(table)).getAllByText('fixture/needs-verification')).toHaveLength(5)
    expect(
      Array.from(table.querySelectorAll('tbody tr')).map((row) => row.getAttribute('data-priced')),
    ).toEqual(['unverified', 'unverified', 'unverified', 'unverified', 'unverified'])
  })

  it('prints the dollar and the ratio once the step says where it came from', () => {
    render(<TeamsView catalog={SOURCED_CATALOG} seed={SOURCED_SEED} />)
    fireEvent.click(screen.getByRole('button', { name: /a sourced draft/ }))
    const table = screen.getByRole('table')

    /** One row's cells, by column. */
    const cells = (cls: string): string[] =>
      Array.from(
        (table.querySelector(`tr[data-class="${cls}"]`) as HTMLElement).querySelectorAll('td'),
      ).map((cell) => cell.textContent?.trim() ?? '')

    // lean fills 128000 at $4/Mtok = $0.51; standard fills 256000, exactly twice it.
    expect(cells('lean')[3]).toContain('$0.51')
    expect(cells('standard')[3]).toContain('$1.02')
    expect(cells('lean')[4]).toBe('1.00x')
    expect(cells('standard')[4]).toBe('2.00x')
    // The 999/Mtok output rate reached nothing: 128000 x $4/Mtok is $0.512 and
    // that is what was printed.
    expect(cells('lean')[3]).toContain('researched')
    expect(within(table).queryByText('price not verified')).toBeNull()
    expect(table.querySelector('tr[data-class="lean"]')).toHaveAttribute('data-priced', 'sourced')
  })

  it('names what a plan seat spends instead of pricing it in dollars', () => {
    // Rung 1 here is on a plan. A wider context costs no money at all — it
    // spends allowance — so a dollar column would be the wrong quantity even if
    // every rate in it had been verified.
    const container = openDraft(/plan-build-verify/)
    const table = within(slotOf(container, 'architect')).getByRole('table')

    expect(within(rowsOf(table)).getAllByText('plan_allowance')).toHaveLength(5)
    expect(within(table).queryByText(/^\$/)).toBeNull()
    expect(within(table).queryByText('price not verified')).toBeNull()
    expect(
      Array.from(table.querySelectorAll('tbody tr')).map((row) => row.getAttribute('data-priced')),
    ).toEqual(Array(5).fill('not-money'))
    expect(table.querySelector('caption')).toHaveTextContent(/does not cost money/)
  })

  it('distinguishes included usage from money, and keeps the ceiling unknown', () => {
    const container = openDraft(/chore lane/)
    const table = within(slotOf(container, 'orchestrator')).getByRole('table')
    // Cursor bills included usage at token rates: not money off a balance, and
    // not a free route either. Three different answers, three different words.
    expect(within(rowsOf(table)).getAllByText('included_usage')).toHaveLength(5)
    expect(within(table).getAllByText('not reported').length).toBeGreaterThan(0)
  })

  it('derives a verdict badge from a rung 1 that may judge', () => {
    const plan = openDraft(/plan-build-verify/)
    expect(within(slotOf(plan, 'audit')).getByText('verdict_capable')).toBeInTheDocument()
  })

  it('derives the badge from rung 1 rather than carrying a flag', () => {
    const chore = openDraft(/chore lane/)
    // Rung 1 is a degraded lane, so this seat works and reports; it does not judge.
    expect(within(slotOf(chore, 'orchestrator')).getByText('cannot_verdict')).toBeInTheDocument()
    // Nothing about the slot's own authority set changed that.
    expect(within(slotOf(chore, 'orchestrator')).getByText('epic-orchestration')).toBeInTheDocument()
  })

  it('recommends the smallest covering class, and refuses to invent one', () => {
    const container = openBench()
    const seat = slotOf(container, 'seat')
    const bandInput = within(seat).getByLabelText('Working set needed (tokens)')

    expect(seat.querySelector('[data-recommended]')).toHaveAttribute('data-recommended', 'lean')

    fireEvent.change(bandInput, { target: { value: '300000' } })
    // `deep` clamps 512000 to 400000 here, which still covers the band under
    // best effort.
    expect(seat.querySelector('[data-recommended]')).toHaveAttribute('data-recommended', 'deep')

    // Above the stated ceiling nothing covers it, and the editor says so instead
    // of promoting the seat to native.
    fireEvent.change(bandInput, { target: { value: '900000' } })
    expect(seat.querySelector('[data-recommended]')).toHaveAttribute('data-recommended', 'none')
    const uncovered = seat.querySelector('[data-code="need_uncovered"]') as HTMLElement
    expect(uncovered).toHaveAttribute('data-severity', 'blocking')
  })

  it('will not recommend a class that required enforcement would refuse', () => {
    // The editor used to print a recommendation directly above a blocking issue
    // refusing the very class it recommended.
    const container = openBench(300_000)
    const seat = slotOf(container, 'seat')
    expect(seat.querySelector('[data-recommended]')).toHaveAttribute('data-recommended', 'deep')

    fireEvent.change(within(seat).getByLabelText('Enforcement'), { target: { value: 'required' } })
    expect(seat.querySelector('[data-recommended]')).toHaveAttribute('data-recommended', 'none')
  })

  it('counts what is blocking in the list beside the draft', () => {
    const container = openDraft(/plan-build-verify/)
    const entry = screen.getByRole('button', { name: /plan-build-verify/ })
    expect(within(entry).queryByText(/blocking/)).toBeNull()

    fireEvent.change(within(slotOf(container, 'architect')).getByLabelText('Enforcement'), {
      target: { value: 'required' },
    })
    expect(within(entry).getByText('1 blocking')).toBeInTheDocument()
  })

  it('publishes monotonically without mutating an earlier revision', () => {
    const container = openDraft(/plan-build-verify/)
    fireEvent.click(screen.getByRole('button', { name: 'Publish next revision' }))
    expect(screen.getByRole('list', { name: 'published team revisions' })).toHaveTextContent('v1')
    fireEvent.change(screen.getByLabelText('Team name'), { target: { value: 'renamed team' } })
    fireEvent.click(screen.getByRole('button', { name: 'Publish next revision' }))
    const revisions = screen.getByRole('list', { name: 'published team revisions' })
    expect(revisions).toHaveTextContent('v1')
    expect(revisions).toHaveTextContent('renamed team · v2')
    expect(container).toBeTruthy()
  })

  it('deduplicates blocking issues by code and slot', () => {
    const unsigned = {
      state: 'researched',
      reviewRef: null,
      citation: null,
      observedAt: null,
    } as const
    const seed = benchSeed().map((draft) => ({
      ...draft,
      slots: draft.slots.map((slot) => ({
        ...slot,
        capabilities: {
          ...slot.capabilities,
          need: { ...slot.capabilities.need, provenance: unsigned },
        },
      })),
    }))
    render(<TeamsView catalog={BENCH_CATALOG} seed={seed} />)
    const entry = screen.getByRole('button', { name: /a bench draft/ })
    expect(within(entry).getByText('3 blocking')).toBeInTheDocument()
  })

  it('kills the (code,slot) dedup mutant on a live Teams projection', async () => {
    const unsigned = {
      state: 'researched', reviewRef: null, citation: null, observedAt: null,
    } as const
    const drafts = benchSeed().map((draft) => ({
      ...draft,
      slots: draft.slots.map((slot) => ({
        ...slot,
        capabilities: {
          ...slot.capabilities,
          need: { ...slot.capabilities.need, provenance: unsigned },
        },
      })),
    }))
    const client = {
      modelCatalog: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 6, ...BENCH_CATALOG })),
      teams: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 6, drafts, revisions: [] })),
      saveTeamDraft: vi.fn(), publishTeam: vi.fn(),
    }
    render(<TeamsView client={client as never} />)
    const entry = await screen.findByRole('button', { name: /a bench draft/ })
    expect(within(entry).getByText('3 blocking')).toBeInTheDocument()
    expect(within(entry).queryByText('6 blocking')).toBeNull()
  })

  it('kills the clamp mutant on a live catalog projection', async () => {
    const drafts = benchSeed()
    const client = {
      modelCatalog: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 7, ...BENCH_CATALOG })),
      teams: vi.fn(async () => ({ realm_id: 'realm-live', snapshot_cursor: 7, drafts, revisions: [] })),
      saveTeamDraft: vi.fn(), publishTeam: vi.fn(),
    }
    const { container } = render(<TeamsView client={client as never} />)
    fireEvent.click(await screen.findByRole('button', { name: /a bench draft/ }))
    const preview = container.querySelector('[data-resolved-policy]')
    expect(preview).toHaveTextContent('effective 400000')
    expect(preview).toHaveTextContent('capability clamped')
    fireEvent.change(screen.getByLabelText('Enforcement'), { target: { value: 'required' } })
    expect(container.querySelector('[data-code="context_clamp_refused"]')).toBeTruthy()
  })

  it('renames a draft, and refuses one with no name left', () => {
    const container = openDraft(/plan-build-verify/)
    const name = screen.getByLabelText('Team name')
    fireEvent.change(name, { target: { value: 'another name' } })
    expect(screen.getByRole('button', { name: /another name/ })).toBeInTheDocument()

    fireEvent.change(name, { target: { value: '  ' } })
    expect(container.querySelector('[data-code="team_unnamed"]')).toHaveAttribute(
      'data-severity',
      'blocking',
    )
  })

  it('adds and removes rungs within the bound the schema allows', () => {
    const container = openDraft(/plan-build-verify/)
    const architect = slotOf(container, 'architect')

    // Four rungs already, so there is nothing to add.
    expect(within(architect).queryByRole('button', { name: 'Add rung' })).toBeNull()
    expect(within(architect).getByText(/at most 4 rungs/)).toBeInTheDocument()

    fireEvent.click(within(architect).getByRole('button', { name: 'Remove rung 4' }))
    expect(architect.querySelectorAll('[data-rung]')).toHaveLength(3)
    expect(within(architect).getByRole('button', { name: 'Add rung' })).toBeInTheDocument()
  })

  it('moves through the draft list with the arrow keys', () => {
    render(<TeamsView />)
    const list = screen.getByRole('list', { name: 'team templates' })
    fireEvent.keyDown(list, { key: 'ArrowDown' })
    expect(screen.getByRole('button', { name: /chore lane/ })).toHaveAttribute(
      'aria-current',
      'true',
    )
    fireEvent.keyDown(list, { key: 'ArrowDown' })
    expect(screen.getByRole('button', { name: /plan-build-verify/ })).toHaveAttribute(
      'aria-current',
      'true',
    )
  })

  it('renders opaque authority keys as themselves', () => {
    const container = openDraft(/plan-build-verify/)
    const audit = within(slotOf(container, 'audit'))
    expect(audit.getByText('audit_passed')).toBeInTheDocument()
    expect(audit.getByText('qa_passed')).toBeInTheDocument()
    expect(audit.getByText('security-check')).toBeInTheDocument()
  })
})
