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
    epic: vi.fn(async () => ({ realm_id: 'realm-1', project_id: 'project-1', epic_id: 'epic-1', name: 'Operational MVP', revision: 7, scheduling_open: true, snapshot_cursor: 20, authorizations: [] })),
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
    completion: vi.fn(async () => ({ realm_id: 'realm-1', epic_id: 'epic-1', revision: 2, snapshot_cursor: 20, phase: 'review', outstanding: ['audit'], profile: PROFILE })),
    advanceCompletion: vi.fn(async () => ({ state: { realm_id: 'realm-1', epic_id: 'epic-1', revision: 3, snapshot_cursor: 30, phase: 'complete', outstanding: [], profile: PROFILE }, receipt: RECEIPT })),
    remediateCompletion: vi.fn(async () => ({ state: { realm_id: 'realm-1', epic_id: 'epic-1', revision: 3, snapshot_cursor: 30, phase: 'remediation', outstanding: ['fix'], profile: PROFILE }, receipt: RECEIPT })),
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

  it('keeps independent projection refusals visible while rendering the rest', async () => {
    await open(operationalClient({ committeeTemplates: vi.fn(async () => { throw new Error('committee service unavailable') }) }))
    expect(screen.getByText('committee service unavailable')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Project Core Team' })).toBeInTheDocument()
  })
})
