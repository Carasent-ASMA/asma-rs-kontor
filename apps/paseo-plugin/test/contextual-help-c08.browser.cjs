// Run against the contextual-help fixture with PREVIEW_ORIGIN and PLAYWRIGHT_MODULE.
// This browser regression intentionally uses the consumer's supplied Playwright.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

const origin = process.env.PREVIEW_ORIGIN;
assert.ok(origin, 'PREVIEW_ORIGIN must identify the exact fixture under test');
const report = { origin, results: [], physicalScreenReaderAcceptance: 'unverified' };
const helpText = 'Show model, reported usage, timing and placement.';
const rowId = 'sessions-row-agt_qa_99103';
const detailsName = 'Details: QA • ASMA-99103 verify results';
let browser;

async function check(name, options, run) {
  const context = await browser.newContext(options);
  const page = await context.newPage();
  try {
    await page.goto(`${origin}/?embed=1&scenario=contextual-help&pointer=${options.hasTouch ? 'coarse' : 'fine'}`);
    await page.getByTestId(rowId).waitFor();
    const result = await run(page);
    report.results.push({ name, status: 'PASS', ...result });
  } catch (error) {
    report.results.push({ name, status: 'FAIL', error: error.message });
  } finally {
    await context.close();
  }
}

async function hostCount(page, expected) {
  assert.equal(await page.getByTestId('prototype-log-toggle').innerText(), `Host log (${expected})`);
}

async function guideRoundTrip(page, keyboard) {
  const guide = page.getByRole('button', { name: 'Model and usage guide', exact: true });
  assert.equal(await guide.getAttribute('aria-expanded'), 'false');
  if (keyboard) { await guide.focus(); await page.keyboard.press('Enter'); }
  else await guide.tap();
  assert.equal(await guide.getAttribute('aria-expanded'), 'true');
  if (keyboard) await page.keyboard.press('Enter');
  else await guide.tap();
  assert.equal(await guide.getAttribute('aria-expanded'), 'false');
  await hostCount(page, 0);
}

async function restoredAndNextTap(page) {
  await page.waitForTimeout(1000);
  assert.equal(await page.getByRole('dialog').count(), 0);
  const opener = page.getByRole('button', { name: detailsName, exact: true });
  assert.equal(await opener.evaluate(el => document.activeElement === el), true, 'return focus to Details');
  const tooltipCount = await page.getByRole('tooltip').count();
  const row = page.getByTestId(rowId);
  const hit = await row.evaluate(el => {
    const box = el.getBoundingClientRect();
    // The baseline tooltip overlaps this lower-middle point in the first row.
    const x = box.x + box.width / 2;
    const y = box.bottom - 4;
    const target = document.elementFromPoint(x, y);
    return { x, y, rowReceivesTap: target === el || el.contains(target),
      interceptedBy: target?.getAttribute('role'), text: target?.textContent };
  });
  await hostCount(page, 0);
  await page.touchscreen.tap(hit.x, hit.y);
  await page.waitForTimeout(100);
  const hostLog = await page.getByTestId('prototype-log-toggle').innerText();
  assert.equal(tooltipCount, 0, `restored focus reopened supplemental help: ${JSON.stringify({hit, hostLog})}`);
  assert.equal(hit.rowReceivesTap, true, 'next touch must hit the intended session row');
  await hostCount(page, 1);
  assert.match(await page.getByTestId('host-agent-shim').innerText(), /ID agt_qa_99103/);
  return { focusRetained: true, tooltipCount, hit, nextSingleTapHostLog: hostLog,
    openedNativeId: 'agt_qa_99103' };
}

(async () => {
  browser = process.env.CDP_URL ? await chromium.connectOverCDP(process.env.CDP_URL, { noDefaults: true }) : await chromium.launch({ headless: true,
    executablePath: process.env.CHROME_EXECUTABLE || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' });
  report.browser = browser.version();
  try {
    for (const viewport of [{ width: 390, height: 844 }, { width: 320, height: 640 }]) {
      for (const close of ['Close', 'Escape']) {
        await check(`touch ${viewport.width}x${viewport.height}: guide expand/collapse then ${close}`, {
          viewport, hasTouch: true, isMobile: true,
        }, async page => {
          await page.getByRole('button', { name: detailsName, exact: true }).tap();
          assert.equal(await page.getByRole('dialog').count(), 1);
          await guideRoundTrip(page, false);
          if (close === 'Escape') await page.keyboard.press('Escape');
          else await page.getByRole('button', { name: /^Close details:/ }).tap();
          return restoredAndNextTap(page);
        });
      }
    }
    await check('desktop hover, keyboard focus, guide and Close preserve dismissal', {
      viewport: { width: 1440, height: 900 }, hasTouch: false, isMobile: false,
    }, async page => {
      const opener = page.getByRole('button', { name: detailsName, exact: true });
      await opener.hover();
      await page.getByRole('tooltip').waitFor();
      assert.equal(await page.getByRole('tooltip').innerText(), helpText);
      await page.keyboard.press('Escape');
      assert.equal(await page.getByRole('tooltip').count(), 0);
      await page.mouse.move(0, 0);
      await opener.focus();
      await page.getByRole('tooltip').waitFor();
      assert.equal(await page.getByRole('tooltip').innerText(), helpText);
      await page.keyboard.press('Enter');
      assert.equal(await page.getByRole('dialog').count(), 1);
      const closeControl = page.getByRole('button', { name: /^Close details:/ });
      assert.equal(await closeControl.evaluate(el => document.activeElement === el), true);
      const openDialogHelp = await page.getByRole('tooltip').allTextContents();
      assert.ok(openDialogHelp.every(text => text === 'Close details'),
        'Inside the open dialog only the focused Close control may show its own help');
      assert.equal(openDialogHelp.includes(helpText), false, 'The stale Details opener help must be dismissed');
      await guideRoundTrip(page, true);
      await page.getByRole('button', { name: /^Close details:/ }).focus();
      await page.keyboard.press('Enter');
      await page.waitForTimeout(1000);
      assert.equal(await opener.evaluate(el => document.activeElement === el), true);
      assert.equal(await page.getByRole('tooltip').count(), 0);
      await hostCount(page, 0);
      return { hoverHelp: true, keyboardFocusHelp: true, focusRetained: true,
        tooltipAfterClose: 0, accidentalActions: 0 };
    });
  } finally {
    await browser.close();
  }
  report.status = report.results.every(result => result.status === 'PASS') ? 'PASS' : 'FAIL';
  const output = `${JSON.stringify(report, null, 2)}\n`;
  if (process.env.REPORT_PATH) fs.writeFileSync(process.env.REPORT_PATH, output, { flag: 'wx' });
  process.stdout.write(output);
  process.exitCode = report.status === 'PASS' ? 0 : 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
