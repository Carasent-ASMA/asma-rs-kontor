const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const origin = process.env.PREVIEW_ORIGIN || 'http://127.0.0.1:58154/';
const output = __dirname;
const report = { origin, source: '007ec9269c0c8b0ddbd445ae211421842623d195', app: '6423d7afbf5448265eb41891b903eaedde426140', evidenceClass: 'Chrome, react-native-web fixture; touch and viewport emulation', checks: [], errors: [], observations: [] };
const check = (name, condition) => { assert(condition, name); report.checks.push(name); };
(async () => {
  const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
  report.browser = browser.version();
  const contexts = [];
  try {
    const mobile = await browser.newContext({ viewport: { width: 320, height: 640 }, hasTouch: true, isMobile: true });
    contexts.push(mobile);
    const touch = await mobile.newPage();
    touch.on('pageerror', error => report.errors.push(error.stack || error.message));
    touch.setDefaultTimeout(8000);
    const tap = async locator => { const b = await locator.boundingBox(); assert(b, 'tap target exists'); await touch.touchscreen.tap(b.x + b.width / 2, b.y + b.height / 2); };
    await touch.goto(`${origin}?embed=1&scenario=compact-icons&pointer=coarse`);
    const info = touch.getByRole('button', { name: 'Details: Model report • Codex selected model', exact: true });
    await info.waitFor();
    const target = await info.boundingBox();
    check('320x640 coarse Details target remains at least 44pt', target.width >= 44 && target.height >= 44);
    await tap(info);
    await touch.getByTestId('sessions-details').waitFor();
    check('touch disclosure does not open the host session', await touch.getByTestId('host-agent-shim').count() === 0);
    await tap(touch.getByRole('button', { name: 'Close details: Model report • Codex selected model', exact: true }));
    await touch.getByTestId('sessions-details').waitFor({ state: 'detached' });
    // measureInWindow callbacks are asynchronous on React Native Web.
    await touch.waitForTimeout(240);
    check('Details Close restores its own Info focus', await info.evaluate(node => document.activeElement === node));
    check('focus return leaves no obstructing tooltip', await touch.getByRole('tooltip').count() === 0);
    await touch.screenshot({ path: path.join(output, 'touch320-after-close.png') });
    const next = touch.getByTestId('sessions-row-agt_model_report_1');
    await next.waitFor();
    const nextBox = await next.boundingBox();
    report.observations.push({ nextTap: nextBox });
    await tap(next);
    await touch.getByTestId('host-agent-shim').waitFor();
    check('the next real touch reaches the next session action', (await touch.getByTestId('host-agent-shim').innerText()).includes('agt_model_report_1'));
    await touch.screenshot({ path: path.join(output, 'touch320-next-action.png') });

    const desktop = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    contexts.push(desktop);
    const page = await desktop.newPage();
    page.on('pageerror', error => report.errors.push(error.stack || error.message));
    page.setDefaultTimeout(8000);
    await page.goto(`${origin}?embed=1&scenario=compact-icons&pointer=fine`);
    const desktopInfo = page.getByRole('button', { name: 'Details: Model report • Codex selected model', exact: true });
    await desktopInfo.focus();
    await page.keyboard.press('Enter');
    await page.getByTestId('sessions-details').waitFor();
    await page.keyboard.press('Escape');
    await page.getByTestId('sessions-details').waitFor({ state: 'detached' });
    await page.waitForTimeout(240);
    check('one Escape closes Details and restores the trigger', await desktopInfo.evaluate(node => document.activeElement === node));
    check('keyboard Details return also leaves help dismissed', await page.getByRole('tooltip').count() === 0);

    for (const mode of ['hover', 'focus', 'escaped']) {
      await page.goto(`${origin}?embed=1&scenario=model-usage-updates&pointer=fine`);
      const row = page.getByTestId('sessions-row-agt_model_report_0');
      await row.waitFor();
      if (mode === 'hover') await row.getByTestId('session-usage').hover();
      else await row.focus();
      const tip = page.getByRole('tooltip');
      await tip.waitFor();
      check(`${mode}: initial help explains the selected-model source`, (await tip.innerText()).includes('Selected model configuration'));
      if (mode === 'escaped') await page.keyboard.press('Escape');
      await row.getByTestId('session-usage').filter({ hasText: '24.8' }).waitFor({ timeout: 7000 });
      if (mode === 'escaped') {
        await row.getByTestId('session-usage').filter({ hasText: '—' }).waitFor({ timeout: 20000 });
        await page.waitForTimeout(240);
        check('actual model-availability text change does not reopen Escape-dismissed help', await tip.count() === 0);
        continue;
      }
      check(`${mode}: updated report preserves the explanatory counter caveat`, (await tip.innerText()).includes('not whole-session totals'));
      if (mode === 'focus') {
        check('dynamic refresh retains keyboard ownership', await row.evaluate(node => document.activeElement === node));
        check('dynamic refresh preserves aria-describedby association', await row.getAttribute('aria-describedby') === await tip.getAttribute('id'));
      }
      // The same scenario also resets counts and clears a provider report.
      await row.getByTestId('session-usage').filter({ hasText: '5 ↓ · 0 ↑' }).waitFor({ timeout: 15000 });
      check(`${mode}: report reset preserves explicit zero and explanatory help`, (await tip.innerText()).includes('zero = explicit zero'));
      await page.screenshot({ path: path.join(output, `dynamic-${mode}-reset.png`) });
      await row.getByTestId('session-usage').filter({ hasText: 'other-model' }).waitFor({ timeout: 7000 });
      check(`${mode}: model change retains unknown-attribution caveat`, (await tip.innerText()).includes('model attribution are unknown'));
      await page.waitForFunction(() => document.querySelector('[role="tooltip"]')?.textContent.includes('Model not supplied'), undefined, { timeout: 7000 });
      report.observations.push({ mode, finalHelp: await tip.innerText() });
      check(`${mode}: stationary help refreshes to missing model after provider switch`, !(await tip.innerText()).includes('Selected model configuration'));
    }
    check('no browser runtime errors', report.errors.length === 0);
    report.status = 'pass';
  } catch (error) {
    report.status = 'fail';
    report.failure = error.stack || error.message;
    throw error;
  } finally {
    fs.writeFileSync(path.join(output, 'browser-verification.json'), JSON.stringify(report, null, 2) + '\n');
    for (const context of contexts) await context.close();
    await browser.close();
  }
  console.log(JSON.stringify({ status: report.status, checks: report.checks.length, errors: report.errors.length }));
})().catch(error => { console.error(error); process.exitCode = 1; });
