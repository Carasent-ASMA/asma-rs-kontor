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

async function pulseCheck(width, theme, longList = false) {
  const name = `pulse-${width}-${theme}${longList ? '-long-list' : ''}`;
  if (process.env.ONLY && !new RegExp(process.env.ONLY).test(name)) return;
  const context = await browser.newContext({ viewport: { width, height: 900 }, reducedMotion: 'no-preference' });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    const request = window.requestAnimationFrame;
    window.pulseFrames = [];
    window.requestAnimationFrame = callback => request.call(window, timestamp => {
      const frames = window.pulseFrames, last = frames.at(-1);
      if (last?.timestamp === timestamp) last.callbacks++; else frames.push({ timestamp, callbacks: 1 });
      callback(timestamp);
    });
  });
  try {
    await page.goto(`${server.resolvedUrls.local[0]}?embed=1&scenario=${longList ? 'long-list' : 'compact-icons'}&theme=${theme}`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 30000 });
    await page.evaluate(async longList => {
      const runtime = await import('/src/host/runtime.ts');
      const chosen = longList ? runtime.fixtureSessions().filter(row => !row.agent.archivedAt).slice(0, 200).map(row => row.agent.id) : ['agt_model_report_0','agt_model_report_1'];
      window.pulseTestIds = chosen;
      for (const id of chosen) runtime.simulateUpdate(id, { status:'running', pendingPermissions:[], attentionReason:null, requiresAttention:false }, 'R14 running marker');
    }, longList);
    if (longList) {
      const running = page.getByTestId(width < 768 ? 'sessions-quick-running' : 'sessions-filter-running');
      if (await running.count()) await running.click();
      else { await page.getByTestId('sessions-mobile-quick-chooser-trigger').click(); await page.getByTestId('sessions-choose-running').click(); }
      await page.getByTestId('sessions-layout-flat').click();
    }
    const tones = page.getByTestId('composer-running-tone');
    await expect(tones.first()).toBeVisible({ timeout: 10000 });
    const glyph = page.getByTestId('composer-running-pulse').first();
    const samples = [];
    for (let index = 0; index < 5; index++) { samples.push(await glyph.evaluate(node => ({ transform:getComputedStyle(node).transform, tone:node.querySelector('[data-testid="composer-running-tone"]')?.style.opacity }))); await page.waitForTimeout(180); }
    assert.ok(new Set(samples.map(value => value.transform)).size > 1, 'running mark pulses');
    assert.ok(new Set(samples.map(value => value.tone)).size > 1, 'running blue/green layers change');
    assert.deepEqual(await glyph.locator('svg').evaluateAll(nodes => nodes.map(node => node.getAttribute('stroke'))), [theme === 'dark' ? '#9cbfff' : '#245bba', theme === 'dark' ? '#6cb17b' : '#3e704a']);
    assert.equal(await page.getByTestId('organization-state-counts').getByTestId('composer-running-pulse').count(), 0, 'aggregate badges stay static');
    if (!longList) {
      const idle = page.getByTestId('sessions-row-agt_model_report_2');
      assert.equal(await idle.getByTestId('composer-running-pulse').count(), 0, 'another session running never animates idle rows');
      await page.evaluate(async () => (await import('/src/host/runtime.ts')).simulateUpdate('agt_model_report_0', { status:'idle' }, 'R14 stop exact session'));
      await expect(page.getByTestId('sessions-row-agt_model_report_0').getByTestId('composer-running-pulse')).toHaveCount(0);
      await expect(page.getByTestId('sessions-row-agt_model_report_1').getByTestId('composer-running-tone')).toBeVisible();
    } else {
      const list = page.getByTestId('sessions-list');
      await list.evaluate(node => { node.scrollTop = node.scrollHeight / 2; });
      await page.waitForTimeout(600);
      const viewport = await list.boundingBox();
      const activeBoxes = await tones.evaluateAll(nodes => nodes.map(node => { const r=node.getBoundingClientRect(); return {top:r.top,bottom:r.bottom}; }));
      assert.ok(activeBoxes.length > 0 && activeBoxes.length <= Math.ceil(viewport.height / 44) + 2, 'active nodes bounded by viewport, not 200 running entries');
      assert.ok(activeBoxes.every(box => box.bottom > viewport.y && box.top < viewport.y + viewport.height), 'offscreen work stopped');
      await page.evaluate(() => { window.pulseFrames = []; }); await page.waitForTimeout(450);
      const frames = await page.evaluate(() => window.pulseFrames);
      const mean = frames.reduce((sum, frame) => sum + frame.callbacks, 0) / Math.max(1, frames.length);
      assert.ok(mean <= 4, `bounded shared clock: ${mean} callbacks/frame`);
      report.cases.push({ name:name+'-work', status:'PASS', runningEntries:200, activeNodes:activeBoxes.length, callbacksPerFrame:mean, scope:'RN Web instrumentation; no native performance claim' });
    }
    await page.screenshot({ path:path.join(output,name+'-running.png') });
    await page.emulateMedia({ reducedMotion:'reduce' });
    await expect(tones).toHaveCount(0);
    await page.emulateMedia({ reducedMotion:'no-preference' }); await expect(tones.first()).toBeVisible();
    await page.evaluate(async () => (await import('/src/host/runtime.ts')).setHostStatus('offline'));
    await expect(tones).toHaveCount(0);
    assert.equal(await page.getByTestId('composer-running-pulse').count(), 0, 'last-known running does not animate');
    await page.screenshot({ path:path.join(output,name+'.png') });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    assert.deepEqual(errors,[]);
    report.cases.push({ name, status:'PASS', samples, errors, scope:'real SessionsScreen, controlled fixture updates; native/phone unverified' });
  } catch (error) {
    report.cases.push({ name, status:'FAIL', error:String(error.stack), errors });
    throw error;
  } finally { await context.close(); }
}

(async () => {
  try {
    const { createServer } = await import(pathToFileURL(renderer.resolve('vite')).href);
    server = await createServer({ configFile: path.join(prototype, 'vite.config.ts'), server: { port: 0, strictPort: false, host: '127.0.0.1', hmr: false } });
    await server.listen(); browser = await chromium.launch({ headless: true });
    if (process.env.R14_ONLY === '1') {
      for (const width of [320,390,1440]) for (const theme of ['light','dark']) await pulseCheck(width,theme);
      for (const width of [390,1440]) await pulseCheck(width,'dark',true);
    } else for (const width of [320, 390, 1440]) for (const theme of ['light', 'dark']) await check(width, theme);
  } catch (error) { console.error(error); process.exitCode = 1; }
  finally {
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
    await browser?.close(); await server?.close();
  }
})();
