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
const primary = page => page.getByTestId('sessions-applied-filters');
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
async function toolbarState(page) {
  return Promise.all(['sessions-selection-toggle', 'sessions-layout-grouped', 'sessions-layout-flat', 'sessions-notices-button'].map(async id => {
    const node = page.getByTestId(id);
    return await node.count() ? [await node.getAttribute('aria-pressed'), await node.getAttribute('aria-checked'), await node.getAttribute('aria-expanded')] : null;
  }));
}
async function point(page, id) {
  const target = page.getByTestId(id);
  await target.scrollIntoViewIfNeeded();
  const box = await target.boundingBox();
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}
async function hold(page, context, id, title) {
  const before = await toolbarState(page);
  const cdp = await context.newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [await point(page, id)] });
  await page.getByRole('tooltip').waitFor({ timeout: 3000 });
  await page.waitForFunction(() => getComputedStyle(document.querySelector('[role="tooltip"]')).opacity === '1');
  assert.ok((await page.getByRole('tooltip').innerText()).startsWith(`${title}.`), 'hold help has an action title and explanation');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await page.waitForTimeout(100);
  assert.deepEqual(await toolbarState(page), before, `${id}: long hold never activates on release`);
  assert.ok(await page.getByRole('tooltip').isVisible(), 'help persists after release');
  await cdp.detach();
}
async function touchHelp(page, context) {
  const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
  if (await hide.isVisible()) await hide.click();
  await hold(page, context, 'sessions-selection-toggle', 'Select sessions');
  await page.keyboard.press('Escape');
  await page.getByTestId('sessions-selection-toggle').tap();
  assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'true', 'normal tap selects once');
  await hold(page, context, 'sessions-selection-toggle', 'Cancel selection');
  await page.keyboard.press('Escape');
  await page.getByTestId('sessions-selection-toggle').tap();
  assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'false', 'normal tap cancels once');
  await hold(page, context, 'sessions-layout-flat', 'Flat');
  await page.keyboard.press('Escape');
  await page.getByTestId('sessions-layout-flat').tap();
  await hold(page, context, 'sessions-layout-grouped', 'Grouped');
  await page.keyboard.press('Escape');
  await page.getByTestId('sessions-layout-grouped').tap();
  await hold(page, context, 'sessions-notices-button', 'Notices');
  await page.keyboard.press('Escape');
  await page.getByTestId('sessions-notices-button').tap();
  await hide.waitFor();
  await hide.tap();
  // Touch movement before recognition and native pointer/touch cancellation must not change layout.
  const cdp = await context.newCDPSession(page), start = await point(page, 'sessions-layout-flat');
  const before = await toolbarState(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [start] });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: start.x, y: start.y + 70 }] });
  await page.waitForTimeout(650);
  assert.equal(await page.getByRole('tooltip').count(), 0, 'drag/scroll cancels hold recognition');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  assert.deepEqual(await toolbarState(page), before, 'drag does not activate a layout');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [await point(page, 'sessions-layout-flat')] });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
  await page.waitForTimeout(650);
  assert.equal(await page.getByRole('tooltip').count(), 0, 'pointer/touch cancellation leaves no late help');
  assert.deepEqual(await toolbarState(page), before, 'cancelled touch does not activate a layout');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [await point(page, 'sessions-layout-flat')] });
  await page.getByRole('tooltip').waitFor({ timeout: 3000 });
  const moving = await point(page, 'sessions-layout-flat');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: moving.x, y: moving.y + 70 }] });
  await page.getByRole('tooltip').waitFor({ state: 'detached' });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
  assert.deepEqual(await toolbarState(page), before, 'movement after recognition hides help without activating');
  await cdp.detach();
}
async function check(width, height, theme, largeText = false, source = 'live') {
  const name = `${width}x${height}-${theme}${largeText ? '-text200' : ''}-${source}`;
  const context = await browser.newContext({ viewport: { width, height }, hasTouch: true, isMobile: true, locale: 'nb-NO' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?layout=compact&theme=${theme}&source=${source}&rows=12`);
    await page.getByTestId('sessions-layout-flat').waitFor();
    const initial = await primary(page).innerText();
    assert.equal(initial, 'My work');
    assert.match(await page.getByTestId('sessions-quick-current').getAttribute('aria-label'), /15 sessions/);
    if (source === 'stale') assert.match(await page.getByTestId('sessions-quick-current').getAttribute('aria-label'), /retained read/);
    await page.getByTestId('sessions-layout-flat').click();
    assert.equal(await page.getByTestId('sessions-layout-flat').getAttribute('aria-checked'), 'true', 'layout checked state');
    assert.equal(await page.getByTestId('sessions-layout-grouped').getAttribute('aria-checked'), 'false', 'unchecked choice');
    assert.equal(await primary(page).innerText(), initial, 'layout preserves the matching count');
    const usage = await page.getByTestId('session-usage').allTextContents();
    assert.ok(usage.some(text => text.includes('≥300K ↓ · 1.2M ↑ · 234.6K cached')), 'NB-locale rows retain compact input/output/cache units');
    await page.screenshot({ path: path.join(output, `${name}-flat-tokens.png`) });
    if (source === 'live') {
      await page.getByTestId('sessions-selection-toggle').click();
      assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'true', 'selection entered');
      assert.ok((await page.getByTestId('sessions-selection-toggle').getAttribute('aria-label')).includes('Cancel'));
      await page.getByTestId('sessions-selection-toggle').click();
      assert.equal(await page.getByTestId('sessions-selection-toggle').getAttribute('aria-pressed'), 'false', 'selection cancelled');
    }
    await page.getByTestId('sessions-mobile-filters-trigger').click();
    assert.match(await page.getByTestId('sessions-mobile-filters-count').innerText(), /^15 matching sessions/);
    await page.getByTestId('sessions-search').fill('Verify results');
    assert.match(await page.getByTestId('sessions-mobile-filters-count').innerText(), /^1 matching session/);
    await back(page);
    assert.equal(await primary(page).innerText(), 'My work • “Verify results”');
    assert.match(await primary(page).getAttribute('aria-label'), /1 matching session/);
    await page.getByTestId('sessions-mobile-clear-filters').click();
    assert.equal(await primary(page).count(), 0, 'Clear removes the applied-filter summary');
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
    if (!largeText && source === 'live') await touchHelp(page, context);
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
    const rowText = await page.getByTestId('sessions-view-controls').innerText();
    assert.ok(!/Select|Cancel|Grouped|Flat|notice/i.test(rowText), 'action row is icon-only, with a numeric notice badge');
    const calls = await page.evaluate(() => window.rpcCalls);
    assert.ok(calls.every(([name]) => !/lifecycle-(preview|execute|resume|archive)/.test(name)), 'view/selection changes dispatch no lifecycle effects');
    assert.deepEqual(errors, []);
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    if (!largeText && source === 'live') {
      await hold(page, context, 'sessions-layout-flat', 'Flat');
      await page.evaluate(() => window.leaveSessions());
      await page.getByRole('tooltip').waitFor({ state: 'detached' });
      assert.ok((await page.locator('body').innerText()).includes('Fixture next page'), 'control unmount removes held help');
    }
    report.cases.push({ name, status: 'PASS', initial, badges, lifecycleEffects: 0 });
    console.log(`${name}: PASS`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
  } finally { await context.close(); }
}
async function desktop(theme) {
  const name = `1280x900-${theme}-mouse-keyboard`;
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'nb-NO' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?theme=${theme}&rows=12`);
    await page.getByTestId('sessions-selection-toggle').waitFor();
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    const before = await toolbarState(page);
    for (const [id, title] of [['sessions-selection-toggle', 'Select sessions'], ['sessions-layout-grouped', 'Grouped'], ['sessions-layout-flat', 'Flat'], ['sessions-notices-button', 'Notices']]) {
      const target = page.getByTestId(id);
      await target.hover();
      const tip = page.getByRole('tooltip');
      await tip.waitFor({ timeout: 3000 });
      await page.waitForFunction(() => getComputedStyle(document.querySelector('[role="tooltip"]')).opacity === '1');
      assert.ok((await tip.innerText()).startsWith(`${title}.`));
      assert.equal(await target.getAttribute('aria-describedby'), await tip.getAttribute('id'));
      await tip.hover();
      await page.waitForTimeout(200);
      assert.ok(await tip.isVisible(), `${id}: hover help stays while the pointer is over it`);
      await page.keyboard.press('Escape');
      assert.equal(await tip.count(), 0, 'Escape dismisses hover help');
      await page.keyboard.press('Tab');
      await target.focus();
      await tip.waitFor({ timeout: 3000 });
      assert.ok((await tip.innerText()).startsWith(`${title}.`), 'keyboard focus exposes titled help');
      await page.keyboard.press('Escape');
    }
    assert.deepEqual(await toolbarState(page), before, 'discovery never activates controls');
    assert.deepEqual(errors, []);
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    report.cases.push({ name, status: 'PASS', lifecycleEffects: 0 });
    console.log(`${name}: PASS`);
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
    for (const theme of ['light', 'dark']) await desktop(theme);
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify(report.cases.map(({ name, status, error }) => ({ name, status, error })), null, 2));
  if (report.cases.some(result => result.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
