// Offline 679-session fixture: use PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR.
// The fixture renders real client modules through react-native-web; ?scenario=contextual has one idle and one running session.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN;
const output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply the isolated fixture origin and an evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], tooltipChecks: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
let browser;
const counts = [679, 93, 48, 538];
const labels = ['All sessions', 'PR Review', 'Adam desk', 'Outside desks'];
const state = page => page.evaluate(() => window.readSessionView());
const chip = (page, index) => page.getByTestId(`sessions-shortcut-${index}`);
const snap = (page, name) => page.screenshot({ path: path.join(output, `${name}.png`) });
async function visibleTooltip(page, expectedText) {
  const tooltip = page.getByRole('tooltip');
  await tooltip.waitFor();
  await page.waitForFunction(text => {
    const node = document.querySelector('[role="tooltip"]');
    return node && getComputedStyle(node).opacity === '1' && node.textContent.includes(text);
  }, expectedText);
  const opacity = await tooltip.evaluate(node => getComputedStyle(node).opacity);
  assert.equal(opacity, '1', 'intended tooltip is fully opaque');
  assert.ok((await tooltip.innerText()).includes(expectedText));
  report.tooltipChecks.push({ width: page.viewportSize().width, theme: new URL(page.url()).searchParams.get('theme'), expectedText, opacity });
}
async function scopeControls(page, touch) {
  if (touch) await page.getByRole('button', { name: 'Filters and view', exact: true }).tap();
  await chip(page, 0).waitFor();
  for (let i = 0; i < 4; i++) {
    const button = chip(page, i);
    assert.equal(await button.getAttribute('aria-label'), `${labels[i]}, ${counts[i]} sessions`);
    const box = await button.boundingBox();
    assert.ok(box.width >= 44 && box.height >= 44, '44pt scope targets');
    assert.ok(box.x >= 0 && box.x + box.width <= page.viewportSize().width, 'scope fits viewport');
    assert.ok(await button.locator('svg').count(), 'SDK glyph rendered');
    assert.ok((await button.innerText()).includes(String(counts[i])));
  }
}
async function activate(button, touch) { if (touch) await button.tap(); else await button.click(); }
async function checkContextual(page, touch, theme, name, filter) {
  await page.goto(`${origin}/?theme=${theme}&scenario=contextual`);
  await activate(page.getByTestId(touch ? 'sessions-mobile-filters-trigger' : 'sessions-advanced-toggle'), touch);
  if (filter === 'search') await page.getByTestId('sessions-search').fill('Unique quiet');
  else await activate(page.getByTestId('sessions-filter-idle'), touch);
  const kontor = () => page.getByRole('button', { name: /^Workstream Kontor,/ });
  if (!touch) assert.ok((await kontor().innerText()).includes('Idle 1/1'));
  await activate(page.getByRole('button', { name: 'Legend', exact: true }), touch);
  assert.ok((await page.getByTestId('sessions-legend').innerText()).includes('matching the current search and filters'));
  await activate(page.getByTestId('sessions-layout-flat'), touch);
  if (touch) await page.getByRole('button', { name: 'Show sessions with the current filters', exact: true }).tap();
  assert.equal(await page.getByTestId('sessions-result-context-quiet').count(), 1);
  assert.equal(await page.getByTestId('sessions-result-context-busy').count(), 0);
  const before = await state(page);
  await activate(page.getByRole('button', { name: 'Show Unique quiet session in the tree', exact: true }), touch);
  assert.ok((await kontor().innerText()).includes('Running 1/2'));
  assert.ok((await kontor().getAttribute('aria-label')).includes('Idle: 1 of 2 sessions'));
  assert.equal(await page.getByTestId('sessions-row-context-busy').count(), 1, 'nonmatching context remains');
  if (touch) await page.getByTestId('sessions-mobile-filters-trigger').tap();
  const legend = page.getByTestId('sessions-legend');
  assert.ok((await legend.innerText()).includes('retained surrounding context'));
  assert.ok((await legend.innerText()).includes("outside the results' search, task or explicit state filter"));
  if (touch) {
    await legend.scrollIntoViewIfNeeded();
    await snap(page, `${name}-${filter}-context-legend`);
    await page.getByRole('button', { name: 'Show sessions with the current filters', exact: true }).tap();
  } else {
    await page.keyboard.press('Tab');
    await kontor().scrollIntoViewIfNeeded();
    await kontor().focus();
    await visibleTooltip(page, 'Running: 1 of 2 sessions. Idle: 1 of 2 sessions. The denominator counts every descendant session in the retained surrounding context');
    await snap(page, `${name}-${filter}-context-help`);
    await page.keyboard.press('Escape');
  }
  await activate(page.getByRole('button', { name: /^Epic ASMA-99100,/ }), touch);
  assert.ok((await kontor().innerText()).includes('Running 1/2'), 'collapsed context keeps its denominator');
  await activate(page.getByRole('button', { name: 'Back to 1 result', exact: true }), touch);
  assert.equal(await page.getByTestId('sessions-result-context-quiet').count(), 1);
  assert.equal(await page.getByTestId('sessions-result-context-busy').count(), 0);
  const after = await state(page);
  assert.equal(after.query, before.query);
  assert.equal(after.status, before.status);
  assert.equal(after.peek, null);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  return { filter, status: 'PASS', matchingResults: 1, contextualDescendants: 2, backResults: 1 };
}
async function check(width, theme) {
  const touch = width < 600;
  const context = await browser.newContext({ viewport: { width, height: width === 1440 ? 1000 : 844 }, hasTouch: touch, isMobile: touch });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const name = `${width}-${theme}`;
  try {
    await page.goto(`${origin}/?theme=${theme}`);
    const kontor = () => page.getByRole('button', { name: /^Workstream Kontor,/ });
    await kontor().waitFor();
    assert.ok((await kontor().innerText()).includes('Running 2/538'));
    assert.ok((await kontor().getAttribute('aria-label')).includes('Running: 2 of 538 sessions'));
    const qsw = page.getByRole('button', { name: /^Native project DESK • QSW,/ });
    assert.ok((await qsw.innerText()).includes('No sessions'));
    assert.ok(await qsw.locator('svg.lucide-zap').count());
    const unknown = page.getByRole('button', { name: /^Native project DESK • FUTURE,/ });
    assert.ok(await unknown.locator('svg.lucide-armchair').count());
    for (const row of [kontor(), qsw, unknown]) assert.ok((await row.boundingBox()).height >= 44);
    await snap(page, `${name}-tree`);
    // Reveal deeper descendants and verify the badge never escapes a narrow viewport.
    await activate(page.getByRole('button', { name: /^Epic ASMA-99100,/ }), touch);
    const badges = await page.getByTestId('organization-state-counts').evaluateAll(nodes => nodes.map(node => {
      const box = node.getBoundingClientRect(); return { text: node.textContent, left: box.left, right: box.right };
    }));
    assert.ok(badges.every(box => box.left >= 0 && box.right <= width), 'nested state ratios fit');
    await scopeControls(page, touch);
    await snap(page, `${name}-scopes`);
    await activate(chip(page, 1), touch);
    assert.equal((await state(page)).shortcut, 'PR Review');
    assert.equal(await chip(page, 1).getAttribute('aria-pressed'), 'true');
    await page.getByTestId('sessions-search').fill('Synthetic session 0');
    assert.equal(await chip(page, 1).getAttribute('aria-label'), 'PR Review, 93 sessions');
    await activate(chip(page, 2), touch);
    assert.equal((await state(page)).query, 'Synthetic session 0', 'scope keeps search');
    assert.equal((await state(page)).shortcut, 'Adam');
    await page.getByTestId('sessions-search').fill('');
    await activate(chip(page, 3), touch);
    assert.equal((await state(page)).shortcut, 'Other work', 'legacy saved ID retained');
    await activate(chip(page, 0), touch);
    if (touch) {
      assert.equal(await page.getByRole('tooltip').count(), 0, 'touch does not depend on hover');
      await page.getByRole('button', { name: 'Legend', exact: true }).tap();
      assert.ok((await page.getByTestId('sessions-legend').innerText()).includes('Scope filters'));
      assert.ok((await page.getByTestId('sessions-legend').innerText()).includes('QSW · Quick Session Workspace'));
      await page.getByRole('button', { name: 'Show sessions with the current filters', exact: true }).tap();
    } else {
      await chip(page, 1).hover();
      await visibleTooltip(page, 'Counts are before search and other filters');
      await snap(page, `${name}-hover-help`);
      await page.keyboard.press('Escape');
      await page.keyboard.press('Tab');
      await chip(page, 2).focus();
      await visibleTooltip(page, 'Adam desk');
      await snap(page, `${name}-keyboard-focus`);
      await page.keyboard.press('Enter');
      assert.equal((await state(page)).shortcut, 'Adam');
      await chip(page, 0).click();
      await page.keyboard.press('Tab');
      await kontor().focus();
      await visibleTooltip(page, 'Native lifecycle and attention signals can overlap');
      await snap(page, `${name}-group-help`);
      await page.keyboard.press('Escape');
    }
    // A search changes group denominators but leaves the scope counts intact.
    if (touch) await scopeControls(page, true);
    await page.getByTestId('sessions-search').fill('Synthetic session 141');
    if (touch) await page.getByRole('button', { name: 'Show sessions with the current filters', exact: true }).tap();
    await kontor().waitFor();
    assert.ok((await kontor().innerText()).includes('Running 1/1'));
    assert.ok((await kontor().getAttribute('aria-label')).includes('Running: 1 of 1 session'));
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
    assert.equal(overflow, false, 'no horizontal document overflow');
    const contextualPeeks = [];
    for (const filter of ['search', 'state']) {
      const contextualPage = await context.newPage();
      contextualPage.on('pageerror', error => errors.push(error.message));
      try { contextualPeeks.push(await checkContextual(contextualPage, touch, theme, name, filter)); }
      catch (error) { await snap(contextualPage, `${name}-${filter}-failure`); throw error; }
      finally { await contextualPage.close(); }
    }
    assert.equal(errors.length, 0, errors.join('\n'));
    report.cases.push({ name, status: 'PASS', counts, nestedBadges: badges, contextualPeeks, noOverflow: true, pageErrors: errors });
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.message, pageErrors: errors });
    await snap(page, `${name}-failure`);
  } finally { await context.close(); }
}
(async () => {
  browser = await chromium.launch({ headless: true, executablePath: process.env.CHROME_EXECUTABLE || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' });
  report.browser = browser.version();
  try { for (const width of [320, 390, 1440]) for (const theme of ['light', 'dark']) await check(width, theme); }
  finally { await browser.close(); fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  console.log(JSON.stringify(report.cases.map(({ name, status, error }) => ({ name, status, error })), null, 2));
  if (report.cases.some(entry => entry.status === 'FAIL')) process.exitCode = 1;
  else assert.equal(report.tooltipChecks.length, 10, 'all ten intended tooltips have strict visibility proof');
})().catch(error => { console.error(error); process.exitCode = 1; });
