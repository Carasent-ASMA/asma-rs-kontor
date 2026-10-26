// Focused synthetic read-only proof using the existing prototype's real Sessions screen.
// No native host, device or assistive-technology acceptance. Evidence stays outside the repository.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const { pathToFileURL } = require('node:url');
const root = path.resolve(__dirname, '../../..');
const prototype = path.join(root, 'apps/kontor-prototype');
const renderer = createRequire(path.join(prototype, 'package.json'));
const consoleDeps = createRequire(path.join(root, 'apps/console/package.json'));
const { chromium, expect } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
assert.ok(process.env.EVIDENCE_DIR, 'Supply an external EVIDENCE_DIR');
const output = path.resolve(process.env.EVIDENCE_DIR);
assert.ok(!output.startsWith(root + path.sep), 'Evidence stays outside the source tree');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndAT: 'unverified', host: 'existing isolated fictional prototype only' };
let server, browser;

async function check(width, theme) {
  const compact = width < 768, name = `${width}-${theme}`;
  const context = await browser.newContext({ viewport: { width, height: 900 }, hasTouch: compact, isMobile: compact, reducedMotion: 'reduce' });
  const page = await context.newPage(), errors = [], events = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.text().startsWith('[kontor-prototype]')) events.push(message.text()); });
  try {
    await page.goto(`${server.resolvedUrls.local[0]}?embed=1&scenario=compact-icons&pointer=${compact ? 'coarse' : 'fine'}&theme=${theme}`);
    const row = page.getByTestId('sessions-row-agt_model_report_0');
    await expect(row).toBeVisible({ timeout: 30000 });
    await page.evaluate(async () => {
      const runtime = await import('/src/host/runtime.ts');
      const list = runtime.fakePaseo.agents.list;
      window.compactRowReads = 0;
      runtime.fakePaseo.agents.list = (...args) => { window.compactRowReads++; return list(...args); };
    });
    const result = page.getByTestId('sessions-result-agt_model_report_0'), body = row.getByTestId('session-row-body');
    const rowText = await row.innerText(), box = await row.boundingBox(), spoken = await body.getAttribute('aria-label');
    assert.ok(box && box.height >= 44, 'row retains the touch target');
    assert.ok(rowText.includes('Codex selected model') && rowText.split('\n').includes('S6.1 · XH'), 'human title and the model alias stay visible');
    assert.ok(spoken.includes('OpenAI gpt-6.1-sol, effort XH'), 'the spoken row names the full model');
    await expect(row.getByTestId('sessions-compact-content')).toHaveCount(0);
    assert.ok(!/Availability unknown|Retirement:|Not archived|Native running|Native idle/.test(rowText), 'routine diagnostics stay behind Details');
    assert.ok(box.height <= 132, `normal row remains bounded at ${width}: ${box.height}`);
    // The dense row's state icon and menu trigger are 40px wide by design; height keeps the 44px target.
    for (const button of await result.getByRole('button').all()) {
      const target = await button.boundingBox();
      if (target) assert.ok(target.width >= 40 && target.height >= 44, 'adjacent row controls keep 40x44px targets');
    }
    const documentWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    assert.ok(documentWidth <= width + 1, `no page overflow: ${documentWidth}/${width}`);
    await page.screenshot({ path: path.join(output, `${name}-rows.png`), fullPage: true });
    await page.getByTestId('session-actions-agt_model_report_0').click();
    await page.getByTestId('session-action-details').click();
    await expect(page.getByTestId('sessions-details')).toBeVisible();
    const facts = await page.getByTestId('sessions-details-lines').innerText();
    assert.ok(facts.includes('Native status and availability') && facts.includes('Not archived'), 'native membership and availability remain in Details');
    assert.ok(facts.includes('gpt-6.1-sol') && facts.includes('Retirement evidence'), 'full model and retirement evidence remain reachable');
    await page.screenshot({ path: path.join(output, `${name}-details.png`), fullPage: true });
    await page.getByRole('button', { name: /^Close details:/ }).click();
    await expect(page.getByTestId('sessions-details')).toHaveCount(0);
    const extraReads = await page.evaluate(() => window.compactRowReads);
    assert.equal(extraReads, 0, 'row rendering and Details add no inventory requests');
    assert.ok(!events.some(event => /simulated\.lifecycle|settings\.save|keyboard\.save|paseo\.unsupported/.test(event)), 'no lifecycle/settings/unsupported effects');
    await body.focus(); await page.keyboard.press('Enter');
    await expect.poll(() => events.filter(event => event.includes('navigation.openAgent agt_model_report_0')).length).toBe(1);
    assert.equal(errors.length, 0, errors.join('\n'));
    report.cases.push({ name, status: 'PASS', compact, rowHeight: box.height, rowText, extraInventoryReads: extraReads, events, errors });
  } catch (error) {
    await page.screenshot({ path: path.join(output, `${name}-failure.png`), fullPage: true }).catch(() => {});
    report.cases.push({ name, status: 'FAIL', error: String(error.stack || error), events, errors });
    throw error;
  } finally { await context.close(); }
}

(async () => {
  try {
    const { createServer } = await import(pathToFileURL(renderer.resolve('vite')).href);
    server = await createServer({ configFile: path.join(prototype, 'vite.config.ts'), server: { port: 0, strictPort: false, host: '127.0.0.1', hmr: false } });
    await server.listen(); browser = await chromium.launch({ headless: true });
    for (const width of [320, 390, 1440]) for (const theme of ['light', 'dark']) await check(width, theme);
  } catch (error) { console.error(error); process.exitCode = 1; }
  finally {
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
    await browser?.close(); await server?.close();
  }
})();
