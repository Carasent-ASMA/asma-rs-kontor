// Synthetic SessionsScreen fixture, no host/lifecycle effects. See counters-toolbar.browser.cjs.
// The ratios scenario supplies 240 Current and 537 All sessions, alternating Keep/Other titles.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output);
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
const baseline = process.env.BASELINE_BOUNDS ? JSON.parse(fs.readFileSync(process.env.BASELINE_BOUNDS, 'utf8')) : null;
async function back(page) {
  await page.getByRole('button', { name: 'Back to sessions, keep filter choices', exact: true }).click();
  await page.waitForFunction(() => document.activeElement?.dataset.testid === 'sessions-mobile-filters-trigger');
}
async function check(browser, width, height, theme, source = 'live', fontScale = 1) {
  const name = `${width}x${height}-${theme}-${source}-font${fontScale}`;
  const context = await browser.newContext({ viewport: { width, height }, isMobile: width < 600, hasTouch: width < 600 });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?layout=${width < 600 ? 'compact' : 'wide'}&scenario=ratios&source=${source}&theme=${theme}&fontScale=${fontScale}`);
    await page.getByTestId('sessions-layout-flat').waitFor();
    const compact = width < 600;
    const selected = () => page.getByTestId(compact ? 'sessions-quick-current' : 'sessions-filter-current');
    const initial = await selected().getAttribute('aria-label');
    assert.match(initial, /240 sessions/);
    assert.equal(await page.getByTestId('sessions-mobile-count').count(), 0);
    assert.equal(await page.getByTestId('sessions-count').count(), 0);
    if (compact) await page.getByTestId('sessions-mobile-filters-trigger').click();
    await page.getByTestId('sessions-search').fill('Keep');
    if (compact) {
      const sticky = await page.getByTestId('sessions-mobile-filters-count').innerText();
      assert.match(sticky, /^120 matching sessions/);
      await back(page);
    }
    assert.match(await selected().getAttribute('aria-label'), /120 of 240 sessions/);
    assert.equal(await page.getByTestId('sessions-applied-filters').innerText(), 'My work • “Keep”');
    if (fontScale > 1) await page.evaluate(scale => {
      for (const text of document.querySelectorAll('[dir="auto"]')) {
        const style = getComputedStyle(text);
        text.style.fontSize = `${parseFloat(style.fontSize) * scale}px`;
        if (style.lineHeight !== 'normal') text.style.lineHeight = `${parseFloat(style.lineHeight) * scale}px`;
      }
    }, fontScale);
    const bounds = compact ? await page.locator('[data-testid^="sessions-quick-"]').evaluateAll(nodes => nodes.map(node => {
      const rect = node.getBoundingClientRect();
      const texts = [...node.querySelectorAll('[dir="auto"]')].map(text => {
        const box = text.getBoundingClientRect();
        return { text: text.textContent, x: box.x, right: box.right, height: box.height, fontSize: getComputedStyle(text).fontSize, scrollWidth: text.scrollWidth, clientWidth: text.clientWidth };
      });
      return { id: node.dataset.testid, x: rect.x, y: rect.y, width: rect.width, height: rect.height, texts };
    })) : [];
    for (const box of bounds) {
      assert.equal(box.width, fontScale === 1 ? 44 : 72, 'ratios preserve the existing fixed target width');
      assert.equal(box.height, box.width, 'ratios add no taller row');
      assert.ok(box.x >= 0 && box.x + box.width <= width, 'button stays on screen');
      for (const text of box.texts) {
        assert.ok(text.x >= box.x && text.right <= box.x + box.width, 'readable text fits inside the unchanged button');
        assert.ok(text.scrollWidth <= text.clientWidth + 1, 'count text is not clipped');
        assert.ok(parseFloat(text.fontSize) >= 10 * fontScale, 'ratio never shrinks existing count typography');
      }
      if (baseline && fontScale === 1) {
        const original = baseline.find(item => item.width === width)?.bounds.find(item => item.id === box.id);
        if (original) assert.deepEqual({ x: box.x, y: box.y, width: box.width, height: box.height }, { x: original.x, y: original.y, width: original.width, height: original.height });
      }
    }
    if (source !== 'live') {
      assert.ok(!await page.getByTestId('session-state-ratio').count(), 'unknown total never renders as a complete ratio');
      assert.match(await selected().getAttribute('aria-label'), /partial read|retained read/);
    }
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    if (fontScale === 1 && source === 'live') {
      await page.getByTestId('sessions-layout-flat').click();
      assert.match(await selected().getAttribute('aria-label'), /120 of 240 sessions/);
      if (compact) {
        await page.getByTestId('sessions-mobile-quick-chooser-trigger').click();
        const all = page.getByTestId('sessions-choose-all');
        assert.match(await all.getAttribute('aria-label'), /269 of 537 sessions/);
        await all.click();
        assert.match(await page.getByTestId('sessions-quick-all').getAttribute('aria-label'), /269 of 537 sessions/);
        await page.getByTestId('sessions-mobile-clear-filters').click();
        assert.equal(await page.getByTestId('sessions-applied-filters').count(), 0);
        assert.match(await page.getByTestId('sessions-quick-all').getAttribute('aria-label'), /537 sessions/);
      }
    }
    assert.deepEqual(errors, []);
    assert.ok((await page.evaluate(() => window.rpcCalls)).every(([name]) => !/lifecycle-(preview|execute|resume|archive)/.test(name)));
    report.cases.push({ name, status: 'PASS', bounds, lifecycleEffects: 0 });
    console.log(`${name}: PASS`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
  } finally { await context.close(); }
}
(async () => {
  const browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const width of [320, 390]) for (const height of [320, 844]) for (const theme of ['light', 'dark']) await check(browser, width, height, theme);
    for (const width of [320, 390]) for (const theme of ['light', 'dark']) await check(browser, width, 844, theme, 'live', 2);
    for (const source of ['partial', 'stale', 'loading']) await check(browser, 390, 844, 'dark', source);
    for (const theme of ['light', 'dark']) await check(browser, 1280, 900, theme);
  } finally { await browser.close(); fs.writeFileSync(path.join(output, 'state-counts-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  console.log(report.cases.filter(item => item.status !== 'PASS'));
  if (report.cases.some(item => item.status !== 'PASS')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
