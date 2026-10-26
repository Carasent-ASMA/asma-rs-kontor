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
    // Sessions lands on Last visited; Current sits under More, on the Filters page or in the advanced states. Once chosen
    // on a phone the Filters trigger shows it and its count; the nine-state strip gains no button.
    if (compact) await page.getByTestId('sessions-mobile-filters-trigger').click();
    else if (!(await page.getByTestId('sessions-filter-current').isVisible())) await page.getByTestId('sessions-advanced-toggle').click();
    await page.getByTestId('sessions-filter-current').click();
    if (compact) await back(page);
    const selected = () => page.getByTestId(compact ? 'sessions-mobile-filters-trigger' : 'sessions-filter-current');
    if (compact) assert.equal(await page.getByTestId('sessions-quick-current').count(), 0);
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
    // Filtered over total, split by a rule rather than a slash, on the chosen state's control on both layouts.
    if (source === 'live') assert.deepEqual(await selected().getByTestId('session-state-ratio-count').evaluate(node => ({
      texts: [...node.children].map(child => child.textContent), rule: getComputedStyle(node.firstElementChild).borderBottomStyle,
    })), { texts: ['120', '240'], rule: 'solid' });
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
    // Whole tiles in the agreed order, as many as the row holds beside More and Filters; the others are in More, never clipped or swiped.
    if (compact) {
      const target = fontScale === 1 ? 44 : 72;
      const row = await page.getByTestId('sessions-mobile-quick-row').evaluate(node => { const rect = node.getBoundingClientRect(); return { x: rect.x, right: rect.right, width: rect.width, scrolls: node.scrollWidth > node.clientWidth }; });
      const slots = Math.max(0, Math.min(9, Math.floor((row.width + 4) / (target + 4)) - 2));
      assert.deepEqual(bounds.map(box => box.id.slice('sessions-quick-'.length)), ['recent', 'activity', 'running', 'needs_input', 'error', 'all', 'idle', 'attention', 'favorites'].slice(0, slots));
      const pinned = await Promise.all(['sessions-mobile-quick-chooser-trigger', 'sessions-mobile-filters-trigger'].map(id => page.getByTestId(id).boundingBox()));
      assert.equal(new Set([...bounds, ...pinned].map(box => box.y)).size, 1, 'one row with More and Filters');
      assert.ok(row.x >= 0 && row.right <= width && !row.scrolls && pinned[1].x + pinned[1].width <= row.right + 0.5, 'the row stays whole on screen');
    }
    for (const box of bounds) {
      assert.equal(box.width, fontScale === 1 ? 44 : 72, 'ratios preserve the existing fixed target width');
      assert.equal(box.height, box.width, 'ratios add no taller row');
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
        // More explains the nine-state strip alone, Last visited first; the advanced states stay on the Filters page.
        await page.getByTestId('sessions-mobile-quick-chooser-trigger').click();
        const choices = await page.locator('[data-testid^="sessions-choose-"]').evaluateAll(nodes => nodes.map(node => node.dataset.testid.slice('sessions-choose-'.length)));
        assert.deepEqual(choices, ['recent', 'activity', 'running', 'needs_input', 'error', 'all', 'idle', 'attention', 'favorites']);
        await page.keyboard.press('Escape');
        await page.getByTestId('sessions-mobile-quick-chooser').waitFor({ state: 'detached' });
        await page.getByTestId('sessions-mobile-filters-trigger').click();
        const all = page.getByTestId('sessions-filter-all');
        assert.match(await all.getAttribute('aria-label'), /269 of 537 sessions/);
        await all.click();
        await back(page);
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
// Attention's Check IDs narrows the compact row; at 320 points with doubled targets More and Filters leave no room for a whole
// state tile, so the row shows none rather than overlapping More, and More still names and reaches the chosen state.
async function attention(browser, width, theme, shown) {
  const name = `${width}x844-${theme}-attention-font2`;
  const context = await browser.newContext({ viewport: { width, height: 844 }, isMobile: true, hasTouch: true });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?layout=compact&scenario=ratios&source=live&theme=${theme}&fontScale=2`);
    await page.getByTestId('sessions-layout-flat').waitFor();
    await page.getByTestId('sessions-mobile-quick-chooser-trigger').click();
    await page.getByTestId('sessions-choose-attention').click();
    await page.getByTestId('sessions-mobile-quick-chooser').waitFor({ state: 'detached' });
    // The row settles once it is measured beside Check IDs; the assertions below judge the settled row, not this wait.
    await page.waitForFunction(count => document.querySelectorAll('[data-testid^="sessions-quick-"]').length === count, shown.length, { timeout: 5000 }).catch(() => {});
    const box = await page.getByTestId('sessions-mobile-quick-row').evaluate(node => {
      const edges = element => { const rect = element.getBoundingClientRect(); return { id: element.dataset.testid, x: rect.x, right: rect.right, y: rect.y, width: rect.width, height: rect.height }; };
      return { row: edges(node), controls: [...node.querySelectorAll('[role="button"]')].map(edges) };
    });
    const check = await page.getByRole('button', { name: 'Check availability', exact: true }).boundingBox();
    for (const control of box.controls) {
      assert.deepEqual([control.width, control.height, control.y], [72, 72, box.controls[0].y], `${control.id} keeps its scaled target on the row`);
      assert.ok(control.x >= box.row.x - 0.5 && control.right <= box.row.right + 0.5, `${control.id} stays inside the row`);
    }
    box.controls.slice(1).forEach((control, index) => assert.ok(control.x >= box.controls[index].right - 0.5, `${control.id} overlaps ${box.controls[index].id} by ${box.controls[index].right - control.x}`));
    assert.deepEqual(box.controls.map(control => control.id), [...shown.map(status => `sessions-quick-${status}`), 'sessions-mobile-quick-chooser-trigger', 'sessions-mobile-filters-trigger']);
    assert.ok(check.x >= box.row.right && check.x + check.width <= width, 'Check IDs sits beside the row, on screen');
    assert.match(await page.getByTestId('sessions-mobile-quick-chooser-trigger').getAttribute('aria-label'), /, current: Attention$/);
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    await page.getByTestId('sessions-mobile-quick-chooser-trigger').click();
    assert.equal(await page.getByTestId('sessions-choose-attention').getAttribute('aria-pressed'), 'true');
    await page.keyboard.press('Escape');
    await page.getByTestId('sessions-mobile-quick-chooser').waitFor({ state: 'detached' });
    assert.deepEqual(errors, []);
    report.cases.push({ name, status: 'PASS', box, check });
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
    for (const theme of ['light', 'dark']) await attention(browser, 320, theme, []);
    await attention(browser, 390, 'dark', ['attention']);
  } finally { await browser.close(); fs.writeFileSync(path.join(output, 'state-counts-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  console.log(report.cases.filter(item => item.status !== 'PASS'));
  if (report.cases.some(item => item.status !== 'PASS')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
