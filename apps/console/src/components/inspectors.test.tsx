/**
 * The five inspectors, against the shapes their projections will have.
 *
 * These renderers are complete and unwired: the `/v1` contract serves no
 * projection for any of them yet, so no view in the running application supplies
 * one. They are exercised here, and here only, which is the difference between a
 * renderer that is ready and an application that shows made-up data.
 *
 * The mutants this file exists to kill: a team ledger that hides an unfilled
 * slot, persona evidence that lets the executor grade itself unremarked, an
 * intake row that drops its lineage, a workflow inspector that offers a free
 * status control, and a scheduling panel that renders "unrestricted" as blank
 * space.
 */
import { render, screen, within } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { TeamLedger } from './TeamLedger'
import { PersonaEvidence } from './PersonaEvidence'
import { IntakeInbox } from './IntakeInbox'
import { WorkflowInspector } from './WorkflowInspector'
import { SchedulingPanel } from './SchedulingPanel'
import type {
  IntakeItem,
  PersonaEvidenceView,
  SchedulingView,
  TeamLedgerView,
  WorkflowInspection,
} from '../views/projections'

describe('<TeamLedger>', () => {
  const ledger: TeamLedgerView = {
    teamRunId: 'tr-1',
    templateId: 'tpl-x',
    templateVersion: 4,
    slots: [
      {
        role: 'r-alpha',
        mayEvaluate: ['g-1'],
        mayWaive: [],
        skills: ['s-1', 's-2'],
        agentRunId: 'run-1',
        runState: 'confirmed',
        bindingState: 'attached',
        freshness: 'fresh',
      },
      {
        role: 'r-beta',
        mayEvaluate: [],
        mayWaive: ['g-1'],
        skills: [],
        agentRunId: null,
        runState: null,
        bindingState: null,
        freshness: null,
      },
    ],
  }

  it('renders arbitrary role, gate and skill keys as data', () => {
    render(<TeamLedger ledger={ledger} />)
    expect(screen.getByText('r-alpha')).toBeInTheDocument()
    // `g-1` is declared twice — one role evaluates it, another waives it — and
    // both declarations are rendered.
    expect(screen.getAllByText('g-1')).toHaveLength(2)
    expect(screen.getByText('s-2')).toBeInTheDocument()
  })

  it('shows a declared slot nothing fills rather than omitting it', () => {
    const { container } = render(<TeamLedger ledger={ledger} />)
    const unfilled = container.querySelector('[data-role="r-beta"]') as HTMLElement
    expect(unfilled).not.toBeNull()
    // The gap between what the template promised and what is running is the
    // reason to look at this panel at all.
    expect(within(unfilled).getAllByLabelText('not reported by the realm').length).toBeGreaterThan(0)
  })

  it('separates the gates a role may evaluate from the ones it may waive', () => {
    const { container } = render(<TeamLedger ledger={ledger} />)
    const alpha = container.querySelector('[data-role="r-alpha"]') as HTMLElement
    const evaluate = within(alpha).getByText('may evaluate').parentElement as HTMLElement
    const waive = within(alpha).getByText('may waive').parentElement as HTMLElement
    expect(within(evaluate).getByText('g-1')).toBeInTheDocument()
    expect(within(waive).getByText('none declared')).toBeInTheDocument()
  })
})

describe('<PersonaEvidence>', () => {
  const evidence: PersonaEvidenceView = {
    scenarioId: 'sc-1',
    version: 2,
    persona: 'p-1',
    identity: 'id-1',
    seeded: true,
    environment: 'env-1',
    steps: [
      { order: 2, instruction: 'second', expectedEvidence: ['a-2'], retained: false },
      { order: 1, instruction: 'first', expectedEvidence: ['a-1'], retained: true },
    ],
    prohibitedActions: ['never do this'],
    gateUnderTest: 'g-under-test',
    actorRole: 'r-actor',
    evaluatorRoles: ['r-judge'],
  }

  it('keeps the executor and the gate authority visibly apart', () => {
    render(<PersonaEvidence evidence={evidence} />)
    expect(screen.getByText('performs the scenario')).toBeInTheDocument()
    expect(screen.getByText('independent authority over the gate')).toBeInTheDocument()
    expect(screen.getByText('r-actor')).toBeInTheDocument()
    expect(screen.getByText('r-judge')).toBeInTheDocument()
  })

  it('warns when the role that ran the scenario also judges its gate', () => {
    render(
      <PersonaEvidence
        evidence={{ ...evidence, evaluatorRoles: ['r-actor', 'r-judge'] }}
      />,
    )
    expect(screen.getByText(/not independent/)).toBeInTheDocument()
  })

  it('renders steps in declared order and says which evidence was retained', () => {
    const { container } = render(<PersonaEvidence evidence={evidence} />)
    const steps = Array.from(container.querySelectorAll('.persona-steps li'))
    expect(steps.map((step) => step.getAttribute('data-retained'))).toEqual(['true', 'false'])
    expect(steps[0]?.textContent).toContain('first')
  })

  it('renders the safety constraints', () => {
    render(<PersonaEvidence evidence={evidence} />)
    expect(screen.getByText('never do this')).toBeInTheDocument()
  })
})

describe('<IntakeInbox>', () => {
  const items: readonly IntakeItem[] = [
    {
      receiptId: 'rc-1',
      source: 'src-a',
      dedupKey: 'dk-1',
      triggerId: 'tg-1',
      triggerVersion: 3,
      approvalState: 'approved',
      receivedAt: '2026-08-10T09:00:00Z',
      createdWork: [{ kind: 'task', id: 'task-1' }],
    },
    {
      receiptId: 'rc-2',
      source: 'src-b',
      dedupKey: 'dk-2',
      triggerId: null,
      triggerVersion: null,
      approvalState: 'pending',
      receivedAt: '2026-08-10T09:01:00Z',
      createdWork: [],
    },
  ]

  it('shows source, dedup key, matched trigger revision and approval state', () => {
    render(<IntakeInbox items={items} />)
    expect(screen.getByText('src-a')).toBeInTheDocument()
    expect(screen.getByText('dk-1')).toBeInTheDocument()
    expect(screen.getByText('tg-1 @3')).toBeInTheDocument()
    expect(screen.getByText('approved')).toBeInTheDocument()
  })

  it('distinguishes work that was created from work that was not', () => {
    const { container } = render(<IntakeInbox items={items} />)
    const created = container.querySelector('[data-receipt-id="rc-1"]') as HTMLElement
    const none = container.querySelector('[data-receipt-id="rc-2"]') as HTMLElement
    expect(within(created).getByText('task-1')).toBeInTheDocument()
    expect(within(none).getByText('none')).toBeInTheDocument()
    expect(within(none).getByText('no trigger matched')).toBeInTheDocument()
  })

  it('says so when nothing has arrived', () => {
    render(<IntakeInbox items={[]} />)
    expect(screen.getByText('no intake has been received')).toBeInTheDocument()
  })
})

describe('<WorkflowInspector>', () => {
  const inspection: WorkflowInspection = {
    connector: 'c-1',
    externalRef: 'EXT-7',
    workflowId: 'wf-1',
    workflowVersion: 5,
    internalFacts: [{ key: 'phase', value: 'p-1' }],
    latestObservation: {
      status: 'their-status',
      assignee: 'someone',
      observedAt: '2026-08-10T09:00:00Z',
    },
    assigneeOwnership: 'theirs',
    proposal: { kind: 'transition', to: 'their-next-status', because: 'the pinned revision allows it' },
    receipt: { receiptId: 'rc-9', state: 'acknowledged', attempts: 1 },
  }

  it('shows this realm’s facts and the external report as separate claims', () => {
    render(<WorkflowInspector inspection={inspection} />)
    expect(screen.getByText('this realm believes')).toBeInTheDocument()
    expect(screen.getByText('the external system last reported')).toBeInTheDocument()
    expect(screen.getByText('their-status')).toBeInTheDocument()
  })

  it('offers no status, assignee or comment control of any kind', () => {
    const { container } = render(<WorkflowInspector inspection={inspection} />)
    // A proposal is not a control. Anything an operator could type into would be
    // a write to a system this realm does not own.
    expect(container.querySelectorAll('input, textarea, select, button')).toHaveLength(0)
  })

  it('renders each proposal kind, with the reason it was reached', () => {
    const { rerender } = render(<WorkflowInspector inspection={inspection} />)
    expect(screen.getByText('transition to their-next-status')).toBeInTheDocument()

    rerender(
      <WorkflowInspector
        inspection={{
          ...inspection,
          proposal: { kind: 'noop', because: 'the two already agree' },
        }}
      />,
    )
    expect(screen.getByText('no change')).toBeInTheDocument()
    expect(screen.getByText('the two already agree')).toBeInTheDocument()

    rerender(
      <WorkflowInspector
        inspection={{
          ...inspection,
          proposal: { kind: 'conflict', because: 'they disagree in a way the revision cannot resolve' },
        }}
      />,
    )
    expect(screen.getByText('conflict — a human decides')).toBeInTheDocument()
  })

  it('says nothing has been observed rather than showing an empty status', () => {
    render(<WorkflowInspector inspection={{ ...inspection, latestObservation: null }} />)
    expect(screen.getByText(/not the same as nothing having happened/)).toBeInTheDocument()
  })
})

describe('<SchedulingPanel>', () => {
  it('states an unrestricted realm in words rather than as an empty calendar', () => {
    render(<SchedulingPanel scheduling={{ kind: 'unrestricted' }} />)
    expect(screen.getByText(/no calendar restriction/)).toBeInTheDocument()
  })

  const calendar: SchedulingView = {
    kind: 'calendar',
    profileId: 'cal-1',
    version: 2,
    timezone: 'Europe/Oslo',
    windows: [{ day: 'mon', from: '09:00', to: '17:00' }],
    exceptions: [{ day: '2026-12-24', reason: 'closed' }],
    draining: false,
    override: { until: '2026-08-11T00:00:00Z', approvedBy: 'someone' },
    lastDecision: {
      admitted: false,
      explanation: 'outside every window this calendar opens',
      evaluatedAt: '2026-08-10T22:00:00Z',
    },
  }

  it('shows the pinned revision, timezone, windows and exceptions', () => {
    render(<SchedulingPanel scheduling={calendar} />)
    expect(screen.getByText('cal-1')).toBeInTheDocument()
    expect(screen.getByText('Europe/Oslo')).toBeInTheDocument()
    expect(screen.getByText('mon')).toBeInTheDocument()
    expect(screen.getByText('2026-12-24')).toBeInTheDocument()
  })

  it('shows the realm’s own explanation of the last rejection, verbatim', () => {
    render(<SchedulingPanel scheduling={calendar} />)
    expect(screen.getByText('rejected')).toBeInTheDocument()
    expect(
      screen.getByText('outside every window this calendar opens'),
    ).toBeInTheDocument()
  })

  it('shows an override in force and the drain state', () => {
    const { container, rerender } = render(<SchedulingPanel scheduling={calendar} />)
    expect(container.querySelector('[data-override="active"]')).not.toBeNull()
    expect(screen.getByText('accepting')).toBeInTheDocument()

    rerender(<SchedulingPanel scheduling={{ ...calendar, draining: true, override: null }} />)
    expect(screen.getByText('draining')).toBeInTheDocument()
    expect(container.querySelector('[data-override="active"]')).toBeNull()
  })

  it('says a calendar that opens no window admits nothing', () => {
    render(<SchedulingPanel scheduling={{ ...calendar, windows: [] }} />)
    expect(screen.getByText(/opens no windows/)).toBeInTheDocument()
  })
})
