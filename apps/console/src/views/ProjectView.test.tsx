import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { ProjectView } from './ProjectView'

const REVISION = { id: 'standard-roles', version: 1 }
const RECEIPT = {
  applied: 'created' as const,
  realm_id: 'realm-1',
  receipt_id: 'receipt-1',
  revision: 4,
  snapshot_cursor: 30,
}
const ROLES = [
  { role_code: 'LSA', standard_title: 'Lead Software Architect', segment: 'leadership', lifecycle: 'active', responsibility_summary: 'Owns architecture.', capability_defaults: ['persistent'] },
  { role_code: 'TPM', standard_title: 'Technical Program Manager', segment: 'leadership', lifecycle: 'active', responsibility_summary: 'Owns delivery.', capability_defaults: ['persistent'] },
  { role_code: 'ADV', standard_title: 'Advisor', segment: 'consultative', lifecycle: 'active', responsibility_summary: 'Provides advice.', capability_defaults: ['ad_hoc'] },
]
const CORE_TEAM = {
  project_id: 'project-1', realm_id: 'realm-1', revision: 3, snapshot_cursor: 20,
  seats: [{ role: { catalog_revision: REVISION, role_code: 'LSA', segment: 'leadership', standard_title: 'Lead Software Architect' }, presence: 'required', ad_hoc_allowed: true, seat_binding_id: 'seat-lsa' }],
}
const PROFILE = { id: 'independent-review', version: 1, name: 'Independent Review', definition_hash: 'profile-hash' }

function operationalClient(overrides: Record<string, unknown> = {}) {
  return {
    epic: vi.fn(async () => ({
      realm_id: 'realm-1', project_id: 'project-1', epic_id: 'epic-1', name: 'Operational MVP',
      revision: 7, scheduling_open: true, snapshot_cursor: 20, authorizations: [],
      tasks: [{
        task_id: 'task-1', title: 'OP-19', state: 'in_progress', revision: 3,
        depends_on: [], gates: [], links: [], required_artifacts: [],
        team_runs: [{
          team_run_id: 'team-1', lifecycle: 'running',
          seats: [{
            role_slot: 'implement', agent_run_id: 'run-1', runtime_kind: 'paseo',
            native_id: 'agent-1', attached: false, observed: 'unknown',
            derived: 'stale', freshness: 'unknown', last_confirmed_at: null,
          }],
        }],
      }],
    })),
    topology: vi.fn(async () => ({
      realm_id: 'realm-1', project_id: 'project-1', snapshot_cursor: 20,
      pinned_spec: { id: 'operational-topology', version: 1, canonical_hash: 'topology-hash' },
      nodes: [{
        topology_node_id: 'psw-1', kind_key: 'PSW', lifecycle: 'active', placement: 'confirmed',
        desired_binding: { runtime_kind: 'native.project', projection_capabilities: [] },
        observed_binding: { native_id: 'prj-1', native_name: 'Project', runtime_kind: 'native.project', observed_at: '2026-08-17T00:00:00Z', cwd: '/work' },
        seats: [{ seat_binding_id: 'seat-lsa', role_slot_id: 'lsa', lifecycle: 'active', role: { catalog_revision: REVISION, role_code: 'LSA', segment: 'leadership', standard_title: 'Lead Software Architect' } }],
      }],
    })),
    coreTeam: vi.fn(async () => CORE_TEAM),
    previewCoreTeam: vi.fn(async () => ({ realm_id: 'realm-1', preview_hash: 'preview-1', effects: [{ effect: 'seat' }] })),
    applyCoreTeam: vi.fn(async () => ({ core_team: { ...CORE_TEAM, revision: 4 }, receipt: RECEIPT })),
    quickRoles: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', snapshot_cursor: 20, roles: ROLES })),
    ensureQuickSession: vi.fn(async () => ({ realm_id: 'realm-1', quick_session_id: 'quick-1', topology_node_id: 'qsw-1', role: { catalog_revision: REVISION, role_code: 'ADV', segment: 'consultative', standard_title: 'Advisor' }, receipt: RECEIPT })),
    previewPromotion: vi.fn(async () => ({ realm_id: 'realm-1', quick_session_id: 'quick-1', preview_hash: 'promotion-1', effects: [] })),
    applyPromotion: vi.fn(async () => ({ quick_session_id: 'quick-1', epic_id: 'epic-2', receipt: RECEIPT })),
    projectCapacity: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', snapshot_cursor: 20, accounts: [], active_team_runs: 4, adaptive_streak: 2, adaptive_width: 5, mission_ceiling: 7, last_observation_id: 'observation-1', last_refusal: 'eighth run refused' })),
    codeHelp: vi.fn(async () => ({ realm_id: 'realm-1', epic_id: 'epic-1', snapshot_cursor: 20, entries: [
      { category: 'role', code: 'LSA', full_name: 'Lead Software Architect', meaning: 'Owns architecture.', lifecycle: 'active', source: REVISION },
      { category: 'role', code: 'TPM', full_name: 'Technical Program Manager', meaning: 'Owns delivery.', lifecycle: 'active', source: REVISION },
      { category: 'role', code: 'ADV', full_name: 'Advisor', meaning: 'Provides advice.', lifecycle: 'active', source: REVISION },
      { category: 'node_kind', code: 'PSW', full_name: 'Project Session Workspace', meaning: 'Logical project root.', lifecycle: 'active', source: { id: 'operational-topology', version: 1 } },
    ] })),
    advisorProfiles: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', revision: 1, snapshot_cursor: 20, revisions: [PROFILE] })),
    committeeTemplates: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', revision: 1, snapshot_cursor: 20, revisions: [PROFILE] })),
    completionProfiles: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', revision: 1, snapshot_cursor: 20, revisions: [PROFILE] })),
    invokeAdvisor: vi.fn(async () => ({ realm_id: 'realm-1', epic_id: 'epic-1', advisor_run_id: 'advisor-1', state: 'running', profile: PROFILE, receipt: RECEIPT })),
    invokeCommittee: vi.fn(async () => ({ realm_id: 'realm-1', epic_id: 'epic-1', committee_run_id: 'committee-1', state: 'running', findings_recorded: 0, template: PROFILE, receipt: RECEIPT })),
    completion: vi.fn(async () => ({ realm_id: 'realm-1', epic_id: 'epic-1', profile: PROFILE, integrations: [], rounds: [], closeout: { receipts: [] }, wakes: [], needs_human: null, revision: 2, snapshot_cursor: 20, phase: { phase: 'verdict', round: 1 }, blockers: [{ blocker: 'committee_verdict', round: 1 }] })),
    advanceCompletion: vi.fn(async () => ({ state: { realm_id: 'realm-1', epic_id: 'epic-1', profile: PROFILE, integrations: [], rounds: [], closeout: { receipts: [] }, wakes: [], needs_human: null, revision: 3, snapshot_cursor: 30, phase: { phase: 'done' }, blockers: [] }, receipt: RECEIPT })),
    remediateCompletion: vi.fn(async () => ({ state: { realm_id: 'realm-1', epic_id: 'epic-1', profile: PROFILE, integrations: [], rounds: [], closeout: { receipts: [] }, wakes: [], needs_human: null, revision: 3, snapshot_cursor: 30, phase: { phase: 'remediation', round: 1 }, blockers: [{ blocker: 'remediation_result', round: 1 }] }, receipt: RECEIPT })),
    ...overrides,
  }
}

async function open(client = operationalClient()) {
  render(<ProjectView client={client as never} />)
  fireEvent.change(screen.getByLabelText('Project'), { target: { value: 'project-1' } })
  fireEvent.change(screen.getByLabelText('Epic'), { target: { value: 'epic-1' } })
  fireEvent.click(screen.getByRole('button', { name: 'Read' }))
  await screen.findByRole('heading', { name: 'Project Session Topology' })
  await screen.findByText(/eighth run refused/)
  return client
}

describe('<ProjectView>', () => {
  it('renders server capacity, logical/native topology and code help without local derivation', async () => {
    await open()
    expect(screen.getByText('4')).toBeInTheDocument()
    expect(screen.getByText('non-terminal TeamRuns')).toBeInTheDocument()
    expect(screen.getByText('stale')).toBeInTheDocument()
    expect(screen.getByText('5')).toBeInTheDocument()
    expect(screen.getByText('7')).toBeInTheDocument()
    expect(screen.getByText(/separate native project/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'PSW' })).toHaveAttribute('aria-describedby')
    expect(screen.getAllByRole('tooltip').some((tooltip) => tooltip.textContent?.includes('Project Session Workspace'))).toBe(true)
    expect(screen.getAllByText(/Member count and protocol are not exposed/)).toHaveLength(2)
  })

  it('groups catalog roles and cannot apply a Core Team before preview confirmation', async () => {
    let confirm!: (value: unknown) => void
    const applyCoreTeam = vi.fn(() => new Promise((resolve) => { confirm = resolve }))
    const client = await open(operationalClient({ applyCoreTeam }))
    const section = screen.getByRole('heading', { name: 'Project Core Team' }).closest('section') as HTMLElement
    const apply = within(section).getByRole('button', { name: 'Apply confirmed preview' })
    expect(apply).toBeDisabled()
    fireEvent.change(within(section).getByLabelText('Role'), { target: { value: 'TPM' } })
    fireEvent.change(within(section).getByLabelText(/Custom seat label/), { target: { value: 'Delivery lead' } })
    fireEvent.change(within(section).getByLabelText('Epic presence'), { target: { value: 'default' } })
    fireEvent.click(within(section).getByLabelText('Quick-session eligible'))
    expect((within(section).getByRole('option', { name: /TPM/ }).parentElement as HTMLOptGroupElement).label).toBe('leadership')
    fireEvent.click(within(section).getByRole('button', { name: 'Add to preview' }))
    fireEvent.click(within(section).getByRole('button', { name: 'Preview Core Team' }))
    await waitFor(() => expect(client.previewCoreTeam).toHaveBeenCalled())
    expect(apply).toBeEnabled()
    fireEvent.click(apply)
    expect(apply).toBeDisabled()
    fireEvent.click(apply)
    await waitFor(() => expect(client.applyCoreTeam).toHaveBeenCalledTimes(1))
    expect(client.applyCoreTeam).toHaveBeenCalledWith(
      'project-1',
      expect.objectContaining({
        expected_revision: 3,
        preview_hash: 'preview-1',
        seats: expect.arrayContaining([expect.objectContaining({
          presence: 'default',
          ad_hoc_allowed: true,
          role: expect.objectContaining({ role_code: 'TPM', custom_display_name: 'Delivery lead' }),
        })]),
      }),
      expect.any(String),
    )
    confirm({ core_team: { ...CORE_TEAM, revision: 4 }, receipt: RECEIPT })
    expect(await within(section).findByText(/Confirmed receipt/)).toHaveTextContent('receipt-1')
  })

  it('replays one uncertain intent under its original idempotency key', async () => {
    // The mutant this kills: minting the key at activation. A retry of an
    // unchanged intent must reach the daemon as the same command, or the
    // uncertain first attempt becomes a second durable write.
    let attempt = 0
    const applyCoreTeam = vi.fn(async (_project: string, _request: unknown, _commandId: string) => {
      attempt += 1
      if (attempt === 1) throw new Error('the realm could not be reached')
      return { core_team: { ...CORE_TEAM, revision: 4 }, receipt: RECEIPT }
    })
    const client = await open(operationalClient({ applyCoreTeam }))
    const section = screen.getByRole('heading', { name: 'Project Core Team' }).closest('section') as HTMLElement

    fireEvent.click(within(section).getByRole('button', { name: 'Preview Core Team' }))
    await waitFor(() => expect(client.previewCoreTeam).toHaveBeenCalled())
    const apply = within(section).getByRole('button', { name: 'Apply confirmed preview' })

    fireEvent.click(apply)
    expect(await within(section).findByText('the realm could not be reached')).toBeInTheDocument()
    fireEvent.click(apply)
    await waitFor(() => expect(applyCoreTeam).toHaveBeenCalledTimes(2))

    const first = applyCoreTeam.mock.calls[0]
    const second = applyCoreTeam.mock.calls[1]
    expect(second?.[1]).toEqual(first?.[1])
    expect(second?.[2]).toBe(first?.[2])
    expect(first?.[2]).toEqual(expect.any(String))
  })

  it('mints a new idempotency key once the intent itself changes', async () => {
    const applyCoreTeam = vi.fn(async (_project: string, _request: unknown, _commandId: string) => {
      throw new Error('the realm could not be reached')
    })
    const client = await open(operationalClient({ applyCoreTeam }))
    const section = screen.getByRole('heading', { name: 'Project Core Team' }).closest('section') as HTMLElement
    const apply = within(section).getByRole('button', { name: 'Apply confirmed preview' })

    fireEvent.click(within(section).getByRole('button', { name: 'Preview Core Team' }))
    await waitFor(() => expect(client.previewCoreTeam).toHaveBeenCalledTimes(1))
    fireEvent.click(apply)
    await waitFor(() => expect(applyCoreTeam).toHaveBeenCalledTimes(1))

    fireEvent.click(within(section).getByRole('button', { name: 'Remove' }))
    fireEvent.click(within(section).getByRole('button', { name: 'Preview Core Team' }))
    await waitFor(() => expect(client.previewCoreTeam).toHaveBeenCalledTimes(2))
    fireEvent.click(apply)
    await waitFor(() => expect(applyCoreTeam).toHaveBeenCalledTimes(2))

    expect(applyCoreTeam.mock.calls[1]?.[2]).not.toBe(applyCoreTeam.mock.calls[0]?.[2])
  })

  it('keeps the Core Team roster when the role catalog read fails', async () => {
    await open(operationalClient({
      quickRoles: vi.fn(async () => { throw new Error('role catalog unavailable') }),
    }))
    const section = screen.getByRole('heading', { name: 'Project Core Team' }).closest('section') as HTMLElement
    expect(within(section).getByRole('list', { name: 'current Core Team seats' }))
      .toHaveTextContent('Lead Software Architect')
    expect(within(section).getByText(/role catalog unavailable/)).toBeInTheDocument()
    expect(within(section).getByRole('button', { name: 'Add to preview' })).toBeDisabled()
  })

  it('keeps epic completion when the profile catalog read fails', async () => {
    await open(operationalClient({
      completionProfiles: vi.fn(async () => { throw new Error('profile catalog unavailable') }),
    }))
    const section = screen.getByRole('heading', { name: 'Completion Profiles' }).closest('section') as HTMLElement
    expect(within(section).getByText('profile catalog unavailable')).toBeInTheDocument()
    expect(within(section).getByRole('button', { name: 'verdict' })).toBeInTheDocument()
    expect(within(section).getByRole('button', { name: 'Advance completion' })).toBeEnabled()
  })

  it('keeps the profile catalog when the epic completion read fails', async () => {
    await open(operationalClient({
      completion: vi.fn(async () => { throw new Error('completion state unavailable') }),
    }))
    const section = screen.getByRole('heading', { name: 'Completion Profiles' }).closest('section') as HTMLElement
    expect(within(section).getByRole('list', { name: 'Completion profiles' }))
      .toHaveTextContent('Independent Review')
    expect(within(section).getByText('completion state unavailable')).toBeInTheDocument()
  })

  it('names the calling seat from the topology projection when invoking a consultation', async () => {
    // The contract requires an exact active epic seat. The console offers the
    // ones the server projected rather than letting an id be typed.
    const client = await open()
    const form = screen.getByRole('form', { name: 'Invoke Advisor' })
    expect(within(form).getByLabelText('Calling seat')).toHaveValue('seat-lsa')
    fireEvent.change(within(form).getByLabelText('Question'), { target: { value: 'is the seam right?' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Invoke Advisor' }))
    await waitFor(() => expect(client.invokeAdvisor).toHaveBeenCalled())
    expect(client.invokeAdvisor).toHaveBeenCalledWith(
      'project-1',
      'epic-1',
      expect.objectContaining({
        caller_seat_binding_id: 'seat-lsa',
        expected_revision: 7,
        question: 'is the seam right?',
      }),
      expect.any(String),
    )
  })

  it('does not fabricate a consultation confirmation when the receipt is absent', async () => {
    const invokeAdvisor = vi.fn(async () => ({
      realm_id: 'realm-1', epic_id: 'epic-1', advisor_run_id: 'advisor-1', state: 'running', profile: PROFILE,
    }))
    const client = await open(operationalClient({ invokeAdvisor }))
    const form = screen.getByRole('form', { name: 'Invoke Advisor' })

    fireEvent.change(within(form).getByLabelText('Question'), { target: { value: 'is the seam right?' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Invoke Advisor' }))

    await waitFor(() => expect(client.invokeAdvisor).toHaveBeenCalledOnce())
    expect(screen.getByText('advisor-1')).toBeInTheDocument()
    expect(screen.queryAllByText(/Confirmed receipt/)).toHaveLength(0)
  })

  it('refuses to offer a consultation caller when the topology projected no seat', async () => {
    await open(operationalClient({
      topology: vi.fn(async () => { throw new Error('topology unavailable') }),
    }))
    const form = screen.getByRole('form', { name: 'Invoke Advisor' })
    expect(within(form).getByRole('button', { name: 'Invoke Advisor' })).toBeDisabled()
    expect(screen.getAllByText(/no seat to call from/).length).toBeGreaterThan(0)
  })

  it('sends the tagged remediation authority the operator selected', async () => {
    // The mutant this kills: a merged action carrying both variants' fields, or
    // the old free-form reason.
    const client = await open()
    const form = screen.getByRole('form', { name: 'Return completion to remediation' })
    expect(within(form).getByLabelText('Round')).toHaveValue(1)
    fireEvent.change(within(form).getByLabelText('Failed-round evidence digest'), { target: { value: 'evidence-1' } })
    fireEvent.change(within(form).getByLabelText('Proposed correction digest'), { target: { value: 'proposal-1' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Return for remediation' }))
    await waitFor(() => expect(client.remediateCompletion).toHaveBeenCalledTimes(1))
    expect(client.remediateCompletion).toHaveBeenCalledWith(
      'project-1',
      'epic-1',
      { expected_revision: 2, action: { action: 'lsa_proposal', round: 1, failed_round_evidence: 'evidence-1', proposal: 'proposal-1' } },
      expect.any(String),
    )

    fireEvent.change(within(form).getByLabelText('Acting authority'), { target: { value: 'tpm_route' } })
    expect(within(form).queryByLabelText('Proposed correction digest')).toBeNull()
    fireEvent.change(within(form).getByLabelText('Routed task-set digest'), { target: { value: 'route-1' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Return for remediation' }))
    await waitFor(() => expect(client.remediateCompletion).toHaveBeenCalledTimes(2))
    expect(client.remediateCompletion).toHaveBeenLastCalledWith(
      'project-1',
      'epic-1',
      { expected_revision: 3, action: { action: 'tpm_route', round: 1, route: 'route-1' } },
      expect.any(String),
    )
  })

  it('renders the typed completion phase and its server blockers', async () => {
    await open()
    const section = screen.getByRole('heading', { name: 'Completion Profiles' }).closest('section') as HTMLElement
    expect(within(section).getByRole('button', { name: 'verdict' })).toBeInTheDocument()
    const blockers = within(section).getByRole('list', { name: 'Completion blockers' })
    expect(within(blockers).getByRole('button', { name: 'committee_verdict' })).toBeInTheDocument()
    expect(blockers).toHaveTextContent('round')
  })

  it('keeps independent projection refusals visible while rendering the rest', async () => {
    await open(operationalClient({ committeeTemplates: vi.fn(async () => { throw new Error('committee service unavailable') }) }))
    expect(screen.getByText('committee service unavailable')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Project Core Team' })).toBeInTheDocument()
  })
})
