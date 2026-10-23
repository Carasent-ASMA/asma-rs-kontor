// Read-only cross-owner regression using the same synthetic fixture as counters-toolbar.browser.cjs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply the isolated fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
async function visibleTip(page, title = '') {
  await page.waitForFunction(title => {
    const panel = document.querySelector('[role="tooltip"]');
    return panel && getComputedStyle(panel).opacity === '1' && panel.textContent.startsWith(title);
  }, title);
  return page.getByRole('tooltip');
}
async function check(browser, theme, source, next, title, mode = 'focus') {
  const name = `${source}-to-${next}-${mode}-${theme}`;
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?rows=12&theme=${theme}`);
    await page.getByTestId('sessions-layout-flat').click();
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (source === 'sessions-notices-button' && await hide.isVisible()) await hide.click();
    await page.mouse.move(5, 5);
    // Hiding notices returns focus to its replacement button. Use keyboard modality
    // before inspecting that focused control, as the standard hover/focus matrix does.
    if (source === 'sessions-notices-button') await page.keyboard.press('a');
    await page.getByTestId(source).first().hover();
    let tip = await visibleTip(page), first = await tip.boundingBox();
    const pointer = { x: first.x + 3, y: first.y + first.height / 2 };
    await page.mouse.move(pointer.x, pointer.y);
    await page.waitForTimeout(1000);
    assert.ok(await tip.isVisible(), 'source help persists while the pointer reads its panel');
    await page.keyboard.press('a');
    const target = page.getByTestId(next);
    if (mode === 'hover') await target.hover(); else await target.focus();
    tip = await visibleTip(page, title);
    const second = await tip.boundingBox();
    assert.ok(!(pointer.x >= second.x && pointer.x <= second.x + second.width && pointer.y >= second.y && pointer.y <= second.y + second.height), 'pointer is outside the replacement panel');
    await page.evaluate(() => {
      const sink = document.createElement('button'); sink.id = 'fixture-focus-sink'; sink.textContent = 'Fixture focus sink';
      sink.style.cssText = 'position:fixed;bottom:2px;right:2px'; document.body.appendChild(sink); sink.focus();
    });
    if (mode === 'hover') await page.mouse.move(5, 5);
    await page.waitForTimeout(1000);
    assert.equal(await tip.count(), 0, 'replacement help closes when both its focus and hover are gone');
    // The replacement panel can acquire its own hover; source blur must then preserve it.
    if (mode === 'hover') await target.hover(); else await target.focus();
    tip = await visibleTip(page, title);
    const own = await tip.boundingBox();
    await page.mouse.move(own.x + 3, own.y + own.height / 2);
    await page.evaluate(() => document.getElementById('fixture-focus-sink').focus());
    await page.waitForTimeout(1000);
    assert.ok(await tip.isVisible(), 'replacement help persists while actually hovered after source blur');
    await page.mouse.move(5, 5);
    await page.waitForTimeout(1000);
    assert.equal(await tip.count(), 0, 'replacement help closes after its own pointer leave');
    assert.deepEqual(errors, []);
    assert.ok((await page.evaluate(() => window.rpcCalls)).every(([name]) => !/lifecycle-(preview|execute|resume|archive)/.test(name)));
    await page.screenshot({ path: path.join(output, `${name}.png`) });
    report.cases.push({ name, status: 'PASS', first, pointer, second, lifecycleEffects: 0 });
    console.log(`${name}: PASS`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
  } finally { await context.close(); }
}
(async () => {
  const browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const theme of ['light', 'dark']) {
      await check(browser, theme, 'session-usage', 'sessions-selection-toggle', 'Select sessions.');
      await check(browser, theme, 'sessions-notices-button', 'sessions-layout-flat', 'Flat.');
      await check(browser, theme, 'session-usage', 'sessions-layout-grouped', 'Grouped.');
      await check(browser, theme, 'session-usage', 'sessions-selection-toggle', 'Select sessions.', 'hover');
    }
  } finally { await browser.close(); fs.writeFileSync(path.join(output, 'ownership-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  console.log(report.cases.filter(item => item.status === 'FAIL'));
  if (report.cases.some(item => item.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
