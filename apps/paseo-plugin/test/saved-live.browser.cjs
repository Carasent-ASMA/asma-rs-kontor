// Focused browser artifact for fix F2: a live running session under Saved entries draws the live rainbow
// like any live row (the ordinary live visit row beside it is the control), and drops to the muted ink only
// when the directory really turns last known. The app's own storage (web: localStorage) is seeded before
// load, so every read below runs through the app's real module instance — no page-side import of plugin code.
// Prototype fixture host only; no daemon, account or device effects.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { createRequire } = require('node:module');
const root = path.resolve(__dirname, '../../..');
const prototype = path.join(root, 'apps/kontor-prototype');
const req = createRequire(path.join(root, 'apps/console/package.json'));
assert.ok(process.env.EVIDENCE_DIR, 'Supply an external EVIDENCE_DIR');
const output = path.resolve(process.env.EVIDENCE_DIR);
assert.ok(!output.startsWith(root + path.sep), 'Evidence stays outside the source tree');
fs.mkdirSync(output, { recursive: true });
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || req.resolve('@playwright/test'));
const report = { scope: 'Real SessionsScreen on the prototype fixture host; web renderer, no native/device claim.', cases: [] };
let server, browser;
(async () => {
  try {
    const { createServer } = await import(req.resolve('vite'));
    server = await createServer({ configFile: path.join(prototype, 'vite.config.ts'), server: { port: 0, strictPort: false, host: '127.0.0.1', hmr: false } });
    await server.listen();
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: 'no-preference' });
    // Seed the device store the plugin really reads (deviceProfileStorage -> localStorage on web):
    // one verified visit (the live control row) and one legacy saved entry that is not a visit.
    await page.addInitScript(() => {
      const at = Date.now();
      localStorage.setItem('kontor.recent@1', JSON.stringify({ clearedAt: null, visits: [{ id: 'agt_model_report_1', at }] }));
      localStorage.setItem('kontor.recent@2 ["srv_prototype"]', JSON.stringify({ clearedAt: null, visits: [{ id: 'agt_model_report_0', at }] }));
    });
    const errors = [];
    page.on('pageerror', (error) => errors.push(String(error)));
    await page.goto(`${server.resolvedUrls.local[0]}?embed=1&scenario=compact-icons&theme=light`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 30000 });
    await page.evaluate(async () => {
      const runtime = await import('/src/host/runtime.ts');
      for (const id of ['agt_model_report_0', 'agt_model_report_1']) runtime.simulateUpdate(id, { status: 'running', pendingPermissions: [], requiresAttention: false, attentionReason: null }, 'R37 F2 saved live running');
    });
    await page.getByTestId('sessions-filter-recent').click();
    const footer = page.getByTestId('sessions-recent-saved');
    await footer.waitFor({ timeout: 10000 });
    await footer.scrollIntoViewIfNeeded();
    const account = await page.getByTestId('sessions-recent-account').innerText();
    assert.ok(account.includes('1 kept'), `the seeded visit is a verified visit: ${account}`);
    const live = await footer.evaluate((node) => ({
      text: node.innerText,
      svgs: node.querySelectorAll('svg[stroke^="url(#kontor-rainbow-"]').length,
      pulses: node.querySelectorAll('[data-testid="composer-running-pulse"]').length,
      tones: node.querySelectorAll('[data-testid="composer-running-tone"]').length,
    }));
    assert.ok(live.text.includes('Saved entries'), 'the saved section is visible');
    assert.ok(live.svgs >= 1 && live.pulses >= 1 && live.tones >= 1, `the live running saved entry draws the animated rainbow: ${JSON.stringify(live)}`);
    const control = await page.getByTestId('sessions-row-agt_model_report_0').evaluate((node) => ({
      svgs: node.querySelectorAll('svg[stroke^="url(#kontor-rainbow-"]').length,
      pulses: node.querySelectorAll('[data-testid="composer-running-pulse"]').length,
      tones: node.querySelectorAll('[data-testid="composer-running-tone"]').length,
    }));
    assert.ok(control.svgs >= 1 && control.pulses >= 1 && control.tones >= 1, `the ordinary live visit row control animates the same rainbow: ${JSON.stringify(control)}`);
    await footer.screenshot({ path: path.join(output, 'saved-running-live.png') });
    // The host really turns last known: the saved entry keeps its place but drops to the muted ink.
    await page.evaluate(async () => (await import('/src/host/runtime.ts')).setHostStatus('offline'));
    await page.waitForTimeout(600);
    const lastKnown = await footer.evaluate((node) => ({
      text: node.innerText,
      svgs: node.querySelectorAll('svg[stroke^="url(#kontor-rainbow-"]').length,
      pulses: node.querySelectorAll('[data-testid="composer-running-pulse"]').length,
      tones: node.querySelectorAll('[data-testid="composer-running-tone"]').length,
    }));
    assert.ok(lastKnown.text.includes('Saved entries'), 'the saved section survives the offline transition');
    assert.equal(lastKnown.pulses, 0, 'last known keeps the muted ink: no motion carrier');
    assert.equal(lastKnown.svgs, 0, 'last known keeps the muted ink: no rainbow mark');
    await footer.screenshot({ path: path.join(output, 'saved-running-last-known.png') });
    report.account = account;
    report.saved = live;
    report.control = control;
    report.lastKnown = lastKnown;
    report.errors = errors;
    assert.deepEqual(errors, []);
    report.cases.push({ name: 'saved-live-running', status: 'PASS' });
  } catch (error) {
    report.cases.push({ status: 'FAIL', error: String(error && error.stack || error) });
    report.pass = false;
    fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
    throw error;
  } finally {
    if (browser) await browser.close();
    if (server) await server.close();
  }
  report.pass = report.cases.length === 1 && report.cases[0].status === 'PASS';
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  assert.ok(report.pass, JSON.stringify(report));
  console.log(JSON.stringify(report));
})().catch((error) => { console.error(error); process.exitCode = 1; });
