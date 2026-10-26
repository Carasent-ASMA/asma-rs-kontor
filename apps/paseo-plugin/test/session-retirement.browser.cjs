// Read-only synthetic fixture of SessionsScreen retirement evidence through react-native-web.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR. The fixture must render the Sessions
// screen with read-only retirement projections (or the real KontorScreen whose only lifecycle call
// is sessions.lifecycle-review) and at least one ordinary row, one archived conflict and one missing
// evidence row. It must expose the normal search, Filters and Archived history controls used below,
// and it must not perform or answer preview/execute/reconcile/archive/send/resume actions.
// No native-device or screen-reader acceptance is claimed.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply an isolated synthetic fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
let browser;

// A row keeps retirement to one line at most, for a conflict or a needed review; its disposition and evidence are in Details.
const ordinaryRow = page => page.locator('[data-testid^="sessions-row-"]:has([data-testid="session-row-body"])').first();
const notices = page => page.locator('[data-testid^="sessions-compact-warning-"]');
const detailsButton = page => page.getByTestId('sessions-row-details').first();
const dialog = page => page.getByTestId('sessions-details');
const conflicts = page => page.locator('[data-testid^="sessions-retirement-conflict-"]');

async function lifecycleCalls(page) {
  return page.evaluate(() => (window.rpcCalls ?? []).map(([name]) => name));
}
function assertReadOnly(calls) {
  assert.ok(calls.every(name => !/lifecycle-(preview|execute|reconcile|archive)|send|resume/.test(name)),
    `read-only projection issued no lifecycle action: ${calls.filter(name => /lifecycle-(preview|execute|reconcile|archive)|send|resume/.test(name)).join(', ')}`);
}

async function openDetails(page, trigger) {
  await trigger.focus();
  await page.keyboard.press('Enter');
  await retirementDetails(page);
}
async function retirementDetails(page) {
  await dialog(page).waitFor();
  const text = await page.getByTestId('sessions-details-lines').innerText();
  assert.ok(text.includes('Retirement evidence'), 'details name the retirement evidence section');
  assert.ok(text.includes('Missing evidence'), 'details name missing evidence explicitly');
  assert.ok(text.includes('Evidence sources'), 'details keep evidence sources');
  assert.ok(text.includes('Retirement') && /\b(Retain|Ready|Unknown)\b/.test(text), 'details name the disposition');
}

// The full-screen regression covers archived-empty and project/epic scopes. This browser
// check keeps the same contract on the desktop surface: a filtered Archived membership must
// not hide the known archived conflict.
async function checkArchivedConflictRetention(page) {
  const search = page.getByTestId('sessions-search');
  await search.fill('nothing-matches-archived-conflict');
  const archived = page.getByTestId('sessions-advanced-archived');
  if (!(await archived.isVisible().catch(() => false))) {
    const toggle = page.getByTestId('sessions-advanced-toggle');
    if (await toggle.isVisible().catch(() => false)) await toggle.click();
  }
  assert.ok(await archived.isVisible().catch(() => false), 'fixture exposes Archived membership');
  await archived.click();
  await conflicts(page).first().waitFor();
  assert.ok(await conflicts(page).count(), 'empty Archived search keeps the known conflict reachable');
  assert.ok(/archived-with-/.test(await conflicts(page).first().innerText()), 'retained conflict names its exact kind');
  await archived.click();
  await search.fill('');
}

async function check(width, height, theme, touch) {
  const name = `${width}x${height}-${theme}`;
  const context = await browser.newContext({ viewport: { width, height }, ...(touch ? { hasTouch: true, isMobile: true } : {}) });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?${touch ? 'layout=compact&' : ''}theme=${theme}&retirement=1&rows=6`);
    const row = ordinaryRow(page);
    await row.waitFor();
    assert.equal(await page.locator('[data-testid^="session-retirement-summary-"]').count(), 0, 'no row draws a multi-line retirement summary');
    assert.equal(await notices(page).count(), 0, 'routine retirement adds no row line');
    const spoken = await row.getByTestId('session-row-body').getAttribute('aria-label');
    assert.ok(spoken.includes('Native status'), 'row keeps native status');
    if (!touch) assert.match(spoken, /Retirement (Retain|Ready|Unknown)/, 'a wide row still speaks its disposition');
    assert.ok(await conflicts(page).count(), 'ordinary membership exposes an archived conflict without reviving it');
    const conflictText = await conflicts(page).first().innerText();
    assert.ok(conflictText.includes('Archived conflict'), 'the conflict is labeled separately');
    assert.ok(/archived-with-/.test(conflictText), 'the conflict names its exact kind');

    const trigger = detailsButton(page);
    const box = await trigger.boundingBox();
    assert.ok(box && box.height >= (touch ? 44 : 24), 'details trigger meets the host target size');
    await openDetails(page, trigger);
    await page.getByRole('button', { name: /^Close details:/ }).click();
    await dialog(page).waitFor({ state: 'detached' });
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('data-testid')), 'sessions-row-details', 'closing details returns focus to its trigger');

    // The row's own disposition and evidence: its actions menu, then Details.
    const actions = row.locator('[data-testid^="session-actions-"]').first();
    await (touch ? actions.tap() : actions.click());
    await (touch ? page.getByTestId('session-action-details').tap() : page.getByTestId('session-action-details').click());
    await retirementDetails(page);
    await page.getByRole('button', { name: /^Close details:/ }).click();
    await dialog(page).waitFor({ state: 'detached' });

    if (!touch) await checkArchivedConflictRetention(page);
    if (touch) {
      await trigger.tap();
      await dialog(page).waitFor();
      await page.getByRole('button', { name: /^Close details:/ }).tap();
      await dialog(page).waitFor({ state: 'detached' });
    }
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'no horizontal page overflow');
    const calls = await lifecycleCalls(page);
    assertReadOnly(calls);
    assert.deepEqual(errors, [], 'no page errors');
    await page.screenshot({ path: path.join(output, `${name}-retirement.png`) });
  } finally {
    await context.close();
  }
}

(async () => {
  browser = await chromium.launch({ headless: true });
  report.browser = browser.version();
  try {
    for (const [width, height, theme, touch] of [[1280, 900, 'light', false], [390, 844, 'dark', true]]) {
      try {
        await check(width, height, theme, touch);
        report.cases.push({ width, height, theme, touch, status: 'PASS' });
        console.log(`${width}x${height}-${theme}: PASS`);
      } catch (error) {
        report.cases.push({ width, height, theme, touch, status: 'FAIL', error: error.stack });
        console.error(`${width}x${height}-${theme}: FAIL`, error);
        process.exitCode = 1;
      }
    }
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'session-retirement-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify(report.cases.map(({ width, height, theme, touch, status, error }) => ({ width, height, theme, touch, status, error })), null, 2));
})().catch(error => { console.error(error); process.exitCode = 1; });
