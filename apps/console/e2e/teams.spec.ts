import { expect, test, type Page } from '@playwright/test'
import { FIXTURE_CATALOG, SEED_TEAMS } from '../src/state/teams'

const REALM = '01920000-0000-7000-8000-000000000001'

async function attach(page: Page): Promise<void> {
  await page.route('http://127.0.0.1:7777/v1/**', async (route) => {
    const path = new URL(route.request().url()).pathname
    const bodies: Record<string, unknown> = {
      '/v1/realm': {
        realm_id: REALM, schema_version: 20,
        created_at: '2026-08-14T00:00:00Z', display_label: 'KON-25 evidence realm',
      },
      '/v1/health': {
        realm_id: REALM, live: true, schema_version: 20,
        reconciliation: 'open', scheduling_open: true, runtimes: ['codex'],
      },
      '/v1/catalog': {
        realm_id: REALM, snapshot_cursor: 21,
        providers: FIXTURE_CATALOG.providers, models: FIXTURE_CATALOG.models,
      },
      '/v1/teams': {
        realm_id: REALM, snapshot_cursor: 21,
        drafts: SEED_TEAMS, revisions: [],
      },
    }
    await route.fulfill({ json: bodies[path] ?? { realm_id: REALM } })
  })
  await page.goto('/')
  await page.getByLabel('Realm endpoint').fill('http://127.0.0.1:7777')
  await page.getByLabel('Realm bearer').fill('operator-secret')
  await page.getByRole('button', { name: 'Connect' }).click()
  await page.getByRole('button', { name: 'Teams' }).click()
  await expect(page.getByRole('note')).toContainText('Live realm catalog')
  await page.getByRole('button', { name: /plan-build-verify/ }).click()
  await expect(page.getByLabel('Team name')).toBeVisible()
}

for (const [name, viewport] of [
  ['desktop', { width: 1440, height: 1000 }],
  ['phone', { width: 390, height: 844 }],
] as const) {
  test(`Teams live editor ${name}`, async ({ page }) => {
    await page.setViewportSize(viewport)
    await attach(page)
    await expect(page.locator('[data-slot="architect"]')).toBeVisible()
    await page.screenshot({
      path: `../../evidence/ASMA-7854-PLAYWRIGHT-${name.toUpperCase()}.png`,
      fullPage: true,
    })
  })
}
