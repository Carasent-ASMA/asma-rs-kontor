// Read-only synthetic fixture of SessionsScreen through react-native-web.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR. No native-device claim.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply an isolated synthetic fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
let browser;
const primary = page => page.getByTestId('sessions-mobile-count');
async function back(page) {
  await page.getByRole('button', { name: 'Back to sessions, keep filter choices', exact: true }).click();
  await page.waitForFunction(() => document.activeElement?.getAttribute('data-testid') === 'sessions-mobile-filters-trigger');
}
async function noOverflow(page, width) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  for (const id of ['sessions-selection-toggle', 'sessions-layout-grouped', 'sessions-layout-flat']) {
    const target = page.getByTestId(id);
    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    assert.ok(box && box.width >= 44 && box.height >= 44, `${id}: 44pt target`);
    assert.ok(box.x >= 0 && box.x + box.width <= width + 1, `${id}: inside viewport`);
    assert.ok(await target.locator('svg').count(), `${id}: familiar icon`);
    assert.equal(await target.evaluate(node => node.scrollWidth > node.clientWidth), false, `${id}: content fits`);
  }
}
async function check(width, height, theme, largeText = false, source = 'live') {
  const name = `${width}x${height}-${theme}${largeText ? '-text200' : ''}-${source}`;
  const context = await browser.newContext({ viewport: { width, height }, hasTouch: true, isMobile: true });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?layout=compact&theme=${theme}&source=${source}&rows=12`);
    await page.getByTestId('sessions-layout-flat').waitFor();
    const initial = await primary(page).innerText();
    assert.match(initial, /^15 matching sessions/);
    if (source === 'stale') assert.match(initial, /stale/);
    await page.getByTestId('sessions-layout-flat').click();
    assert.equal(await page.getByTestId('sessions-layout-flat').getAttribute('aria-checked'), 'true', 'layout checked state');
    assert.equal(await page.getByTestId('sessions-layout-grouped').getAttribute('aria-checked'), 'false', 'unchecked choice');
    assert.equal(await primary(page).innerText(), initial, 'layout preserves the matching count');
    if (source === 'live') {
      await page.getByTestId('sessions-selection-toggle').click();
      assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'true', 'selection entered');
      assert.ok((await page.getByTestId('sessions-selection-toggle').innerText()).includes('Cancel'));
      await page.getByTestId('sessions-selection-toggle').click();
      assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'false', 'selection cancelled');
    }
    await page.getByTestId('sessions-mobile-filters-trigger').click();
    assert.equal(await page.getByTestId('sessions-mobile-filters-count').innerText(), initial);
    await page.getByTestId('sessions-search').fill('Verify results');
    assert.match(await page.getByTestId('sessions-mobile-filters-count').innerText(), /^1 matching session/);
    await back(page);
    assert.match(await primary(page).innerText(), /^1 matching session/);
    await page.getByTestId('sessions-mobile-clear-filters').click();
    assert.equal(await primary(page).innerText(), initial);
    await page.getByTestId('sessions-mobile-filters-trigger').click();
    await page.getByTestId('sessions-reset-view').click();
    await back(page);
    assert.equal(await page.getByTestId('sessions-layout-grouped').getAttribute('aria-checked'), 'true', 'layout checked state');
    assert.equal(await primary(page).innerText(), initial);
    await page.getByTestId('sessions-layout-grouped').focus();
    await page.keyboard.press('ArrowRight');
    assert.equal(await page.getByTestId('sessions-layout-flat').getAttribute('aria-checked'), 'true', 'layout checked state');
    await page.keyboard.press('Home');
    assert.equal(await page.getByTestId('sessions-layout-grouped').getAttribute('aria-checked'), 'true', 'layout checked state');
    await page.keyboard.press('Escape');
    if (largeText) {
      // Text scaling stress check; iOS Dynamic Type and VoiceOver remain separate acceptance.
      await page.evaluate(() => {
        for (const text of document.querySelectorAll('[dir="auto"]')) {
          const style = getComputedStyle(text);
          text.style.fontSize = `${parseFloat(style.fontSize) * 2}px`;
          if (style.lineHeight !== 'normal') text.style.lineHeight = `${parseFloat(style.lineHeight) * 2}px`;
        }
      });
    }
    await noOverflow(page, width);
    if (!largeText) {
      const boxes = await Promise.all(['sessions-selection-toggle', 'sessions-layout-grouped', 'sessions-layout-flat'].map(id => page.getByTestId(id).boundingBox()));
      assert.ok(boxes.every(box => Math.abs(box.y - boxes[0].y) <= 1), 'three controls share one compact row');
    }
    const badges = await page.getByTestId('organization-state-counts').allTextContents();
    assert.ok(badges.length && badges.every(text => /\d+ sessions?/.test(text) && !/\d+\/\d+/.test(text)), 'group totals are labelled once, without ratios');
    const calls = await page.evaluate(() => window.rpcCalls);
    assert.ok(calls.every(([name]) => !/lifecycle-(preview|execute|resume|archive)/.test(name)), 'view/selection changes dispatch no lifecycle effects');
    assert.deepEqual(errors, []);
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    report.cases.push({ name, status: 'PASS', initial, badges, lifecycleEffects: 0 });
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
  } finally { await context.close(); }
}
(async () => {
  browser = await chromium.launch({ headless: true });
  report.browser = browser.version();
  try {
    for (const width of [320, 390]) for (const height of [320, 844]) for (const theme of ['light', 'dark']) await check(width, height, theme);
    for (const width of [320, 390]) for (const theme of ['light', 'dark']) await check(width, 844, theme, true);
    await check(390, 844, 'dark', false, 'stale');
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify(report.cases.map(({ name, status, error }) => ({ name, status, error })), null, 2));
  if (report.cases.some(result => result.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
