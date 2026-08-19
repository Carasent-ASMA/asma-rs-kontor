import { expect, test, type Page } from '@playwright/test'

const REALM = '01920000-0000-7000-8000-000000000001'
const PROFILE = { id: 'independent-review', version: 1, name: 'Independent Review', definition_hash: 'profile-hash' }
const REVISION = { id: 'standard-roles', version: 1 }

async function attach(page: Page): Promise<void> {
  await page.route('http://127.0.0.1:7777/v1/**', async (route) => {
    const url = new URL(route.request().url())
    const profileCatalog = { realm_id: REALM, project_id: 'project-1', revision: 1, snapshot_cursor: 21, revisions: [PROFILE] }
    const bodies: Record<string, unknown> = {
      '/v1/realm': { realm_id: REALM, schema_version: 20, created_at: '2026-08-17T00:00:00Z', display_label: 'Operational evidence' },
      '/v1/health': { realm_id: REALM, live: true, schema_version: 20, reconciliation: 'open', scheduling_open: true, runtimes: ['paseo.agent'] },
      '/v1/projects/project-1/epics/epic-1': { realm_id: REALM, project_id: 'project-1', epic_id: 'epic-1', name: 'Operational MVP', revision: 7, scheduling_open: true, snapshot_cursor: 21, authorizations: [] },
      '/v1/projects/project-1/topology:inspect': {
        realm_id: REALM, project_id: 'project-1', snapshot_cursor: 21,
        pinned_spec: { id: 'operational-topology', version: 1, canonical_hash: 'hash' },
        nodes: [{
          topology_node_id: 'esw-1', parent_topology_node_id: 'psw-1', kind_key: 'ESW', lifecycle: 'active', placement: 'confirmed',
          desired_binding: { runtime_kind: 'paseo.project', projection_capabilities: [] },
          observed_binding: { native_id: 'prj-1', native_name: 'Epic project', runtime_kind: 'paseo.project', observed_at: '2026-08-17T00:00:00Z', cwd: '/work' },
          seats: [{ seat_binding_id: 'seat-lsa', role_slot_id: 'lsa', lifecycle: 'active', role: { catalog_revision: REVISION, role_code: 'LSA', segment: 'leadership', standard_title: 'Lead Software Architect' } }],
        }],
      },
      '/v1/projects/project-1/core-team': { realm_id: REALM, project_id: 'project-1', revision: 2, snapshot_cursor: 21, seats: [{ role: { catalog_revision: REVISION, role_code: 'LSA', segment: 'leadership', standard_title: 'Lead Software Architect' }, presence: 'required', ad_hoc_allowed: true, seat_binding_id: 'seat-lsa' }] },
      '/v1/projects/project-1/quick-roles': { realm_id: REALM, project_id: 'project-1', snapshot_cursor: 21, roles: [{ role_code: 'LSA', standard_title: 'Lead Software Architect', segment: 'leadership', lifecycle: 'active', responsibility_summary: 'Owns architecture.', capability_defaults: ['persistent'] }] },
      '/v1/projects/project-1/capacity': { realm_id: REALM, project_id: 'project-1', snapshot_cursor: 21, accounts: [], active_team_runs: 4, adaptive_streak: 2, adaptive_width: 5, mission_ceiling: 7, last_refusal: 'eighth run refused' },
      '/v1/projects/project-1/epics/epic-1/code-help': { realm_id: REALM, epic_id: 'epic-1', snapshot_cursor: 21, entries: [
        { category: 'role', code: 'LSA', full_name: 'Lead Software Architect', meaning: 'Owns architecture.', lifecycle: 'active', source: REVISION },
        { category: 'node_kind', code: 'ESW', full_name: 'Epic Session Workspace', meaning: 'Logical epic scope.', lifecycle: 'active', source: { id: 'operational-topology', version: 1 } },
      ] },
      '/v1/projects/project-1/advisor-profiles': profileCatalog,
      '/v1/projects/project-1/committee-templates': profileCatalog,
      '/v1/projects/project-1/completion-profiles': profileCatalog,
      '/v1/projects/project-1/epics/epic-1/completion': { realm_id: REALM, epic_id: 'epic-1', revision: 2, snapshot_cursor: 21, phase: { phase: 'verdict', round: 1 }, blockers: [{ blocker: 'committee_verdict', round: 1 }], profile: PROFILE, integrations: [], rounds: [], closeout: { receipts: [] }, wakes: [], needs_human: null },
    }
    await route.fulfill({ json: bodies[url.pathname] ?? { realm_id: REALM } })
  })

  await page.goto('/')
  await page.getByLabel('Realm endpoint').fill('http://127.0.0.1:7777')
  await page.getByLabel('Realm bearer').fill('operator-secret')
  await page.getByRole('button', { name: 'Connect' }).click()
  await page.getByRole('button', { name: 'Project Operations' }).click()
  await page.getByRole('textbox', { name: 'Project' }).fill('project-1')
  await page.getByRole('textbox', { name: 'Epic' }).fill('epic-1')
  await page.getByRole('button', { name: 'Read' }).click()
  await expect(page.getByRole('heading', { name: 'Project Session Topology' })).toBeVisible()
}

for (const [name, viewport] of [
  ['desktop', { width: 1440, height: 1000 }],
  ['phone', { width: 390, height: 844 }],
] as const) {
  test(`Project Operations ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport)
    await attach(page)
    await expect(page.getByText('eighth run refused')).toBeVisible()
    const code = page.getByRole('button', { name: 'ESW' })
    await expect(code).toBeVisible()
    await code.click()
    await expect(page.getByRole('tooltip').filter({ hasText: 'Epic Session Workspace' })).toBeVisible()
    await code.click()
    await page.screenshot({ path: `../../evidence/ASMA-7878-PROJECT-${name.toUpperCase()}.png`, fullPage: true })
  })
}
