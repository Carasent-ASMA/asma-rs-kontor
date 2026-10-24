// Read-only synthetic fixture of the SessionsScreen bulk archive flow through react-native-web.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR. The fixture answers lifecycle RPCs itself; its sessions are
// agt_lead (children agt_idle idle, agt_run running), agt_a, agt_b (runtime not observed), agt_fence (held fence) and
// agt_lost (runtime close unconfirmed). No native-device or screen-reader claim.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply an isolated synthetic fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
const LEAD = 'TPM • DEMO-2 • Coordinate the sessions navigator release train, its archive review and the follow-up handoff';
let browser;

// Text scaling stress check, re-applied after each render; iOS Dynamic Type remains separate acceptance.
const scale = page => page.evaluate(() => {
  for (const text of document.querySelectorAll('[dir="auto"]:not([data-scaled])')) {
    const style = getComputedStyle(text);
    text.style.fontSize = `${parseFloat(style.fontSize) * 2}px`;
    if (style.lineHeight !== 'normal') text.style.lineHeight = `${parseFloat(style.lineHeight) * 2}px`;
    text.setAttribute('data-scaled', '');
  }
});
// Every visible control in the scope is inside the viewport with a minimum target; no text is clipped sideways.
async function layout(page, scope, min) {
  const problems = await page.evaluate(([scope, min]) => {
    const root = document.querySelector(`[data-testid="${scope}"]`), out = [];
    if (!root) return [`${scope} missing`];
    if (document.documentElement.scrollWidth > innerWidth) out.push('page scrolls sideways');
    for (const node of root.querySelectorAll('[role="button"], [role="checkbox"]')) {
      const box = node.getBoundingClientRect(), name = node.getAttribute('aria-label') || node.textContent;
      if (!box.width) continue;
      if (box.width < min || box.height < min) out.push(`${name}: ${Math.round(box.width)}x${Math.round(box.height)} target`);
      if (box.left < -1 || box.right > innerWidth + 1) out.push(`${name}: outside viewport`);
    }
    for (const text of root.querySelectorAll('[dir="auto"]')) if (text.scrollWidth > text.clientWidth + 1) out.push(`${text.textContent}: clipped`);
    return out;
  }, [scope, min]);
  assert.deepEqual(problems, [], `${scope} layout`);
}
// WCAG 1.4.11: the drawn box against what is behind it, measured on the rendered page.
const contrast = (page, id) => page.evaluate(id => {
  const rgb = value => (value.match(/[\d.]+/g) || []).map(Number);
  const lum = ([r, g, b]) => [r, g, b].map(v => (v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4).reduce((sum, v, i) => sum + v * [0.2126, 0.7152, 0.0722][i], 0);
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  const behind = node => { for (let n = node; n; n = n.parentElement) { const c = rgb(getComputedStyle(n).backgroundColor); if (c.length === 3 || c[3] > 0) return c.slice(0, 3); } return [255, 255, 255]; };
  const box = document.querySelector(`[data-testid="${id}"]`), style = getComputedStyle(box), back = behind(box.parentElement);
  const fill = rgb(style.backgroundColor), filled = fill.length === 3 || fill[3] > 0;
  return { size: [box.offsetWidth, box.offsetHeight], state: filled ? 'filled' : 'outlined', ratio: Math.round(ratio(filled ? fill.slice(0, 3) : rgb(style.borderTopColor), back) * 100) / 100 };
}, id);
const calls = page => page.evaluate(() => window.rpcCalls);

async function check(width, height, theme, largeText, earlyClose = false) {
  const touch = width < 600, name = `${width}x${height}-${theme}${largeText ? '-text200' : ''}${earlyClose ? '-early-close' : ''}`;
  const context = await browser.newContext({ viewport: { width, height }, ...(touch ? { hasTouch: true, isMobile: true } : {}) });
  const page = await context.newPage(), errors = [], toasts = [], min = touch ? 44 : 24;
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.text().startsWith('toast')) toasts.push(message.text()); });
  const shot = async step => { if (largeText) await scale(page); await page.screenshot({ path: path.join(output, `${name}-${step}.png`) }); };
  const dialog = () => page.getByTestId('bulk-archive-dialog');
  const summary = () => page.getByTestId('bulk-archive-summary');
  try {
    await page.goto(`${origin}/?${touch ? 'layout=compact&' : ''}theme=${theme}&rows=4`);
    await page.getByTestId('sessions-selection-toggle').waitFor();
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    await page.getByTestId('sessions-layout-flat').click();
    await page.getByTestId('sessions-selection-toggle').click();
    const all = page.getByTestId('sessions-select-all');
    assert.equal(await all.getAttribute('aria-checked'), 'false');
    const clear = await contrast(page, 'sessions-select-agt_a-box');
    await shot('1-selecting');

    for (const id of ['agt_lead', 'agt_b', 'agt_fence', 'agt_lost']) await page.getByTestId(`sessions-select-${id}`).click();
    // Keyboard: Space toggles once and the focus outline is drawn.
    await page.getByTestId('sessions-select-agt_a').focus();
    await page.keyboard.press('Space');
    assert.equal(await page.getByTestId('sessions-select-agt_a').getAttribute('aria-checked'), 'true', 'Space selects once');
    assert.notEqual(await page.getByTestId('sessions-select-agt_a').evaluate(node => getComputedStyle(node).borderTopColor), 'rgba(0, 0, 0, 0)', 'focus outline');
    assert.equal(await all.getAttribute('aria-checked'), 'mixed', 'select-all shows some selected');
    assert.ok((await page.getByTestId('sessions-bulk-archive').innerText()).includes('5 selected'));
    const on = await contrast(page, 'sessions-select-agt_a-box');
    for (const box of [clear, on]) { assert.deepEqual(box.size, [22, 22]); assert.ok(box.ratio >= 3, `checkbox ${box.state} ${box.ratio}:1`); }
    assert.deepEqual([clear.state, on.state], ['outlined', 'filled'], 'selected and clear differ by fill and mark');
    const target = await page.getByTestId('sessions-select-agt_a').boundingBox();
    assert.ok(target.width >= 44 && target.height >= 44, 'checkbox 44pt target');
    await layout(page, 'sessions-bulk-archive', min);
    await shot('2-selected-focus');

    await page.getByRole('button', { name: 'Review archive of 5 selected sessions', exact: true }).click();
    await dialog().waitFor();
    await summary().filter({ hasText: '4 ready · 1 blocked' }).waitFor();
    assert.ok(await page.getByRole('heading', { name: 'Archive 5 sessions?', exact: true }).isVisible());
    const text = await dialog().innerText();
    assert.equal(text.split('There is no undo.').length, 2, 'one consequence line');
    assert.ok(!text.includes('agt_lead'), 'IDs wait in details');
    assert.ok(text.includes('2 active children not selected · 1 running'));
    assert.ok(text.includes('Finish running children in their own workflow, then review again.'));
    assert.ok(await page.getByRole('button', { name: 'Archive 4 sessions', exact: true }).isEnabled(), 'confirm counts ready rows only');
    if (largeText) await scale(page);
    await layout(page, 'bulk-archive-dialog', min);
    await shot('3-review');

    await page.getByRole('button', { name: `Details for ${LEAD}`, exact: true }).click();
    const details = await page.getByTestId('bulk-archive-row-agt_lead').innerText();
    assert.ok(details.includes('ID agt_lead') && details.includes('Child: SWE • DEMO-2 • Worker still running · running · agt_run'));
    await page.getByRole('button', { name: 'About archive', exact: true }).click();
    assert.ok((await dialog().innerText()).includes('may be archived by the host with its parent, even if unselected'));
    if (largeText) await scale(page);
    await layout(page, 'bulk-archive-dialog', min);
    await shot('4-details-about');

    await page.getByRole('button', { name: `Add 1 idle child of ${LEAD} and review again`, exact: true }).click();
    await summary().filter({ hasText: '5 ready · 1 blocked' }).waitFor();
    const added = await dialog().innerText();
    assert.ok(added.includes('1 active child not selected · 1 running') && !/Add \d+ idle/.test(added), 'running child is never offered');
    const previews = (await calls(page)).filter(([rpc]) => rpc === 'sessions.lifecycle-preview').map(([, input]) => input.agentIds);
    assert.deepEqual(previews.at(-1).slice().sort(), ['agt_a', 'agt_b', 'agt_fence', 'agt_idle', 'agt_lead', 'agt_lost']);
    assert.ok(!(await calls(page)).some(([rpc]) => rpc === 'sessions.lifecycle-execute-item'), 'review dispatches nothing');
    await shot('5-added-child');

    await page.evaluate(() => { window.hold = true; });
    await page.getByRole('button', { name: 'Archive 5 sessions', exact: true }).click();
    // Progress counts the five confirmed sessions, not the blocked parent's preflight request.
    await summary().filter({ hasText: 'Session 1 of 5' }).waitFor();
    assert.ok(await page.getByRole('heading', { name: 'Archiving…', exact: true }).isVisible());
    if (largeText) await scale(page);
    await layout(page, 'bulk-archive-dialog', min);
    await shot('6-running');
    if (earlyClose) {
      await page.getByRole('button', { name: 'Close', exact: true }).click();
      await dialog().waitFor({ state: 'detached' });
      await page.evaluate(() => { window.hold = false; window.release(); });
      await page.getByTestId('sessions-bulk-status').filter({ hasText: 'Stopped before the next session.' }).waitFor();
      await page.waitForTimeout(100);
      assert.deepEqual(toasts, ['toast Stopped before the next session. Archived 1 · 5 not attempted'], 'one toast after an early close');
      assert.deepEqual((await calls(page)).filter(([rpc]) => rpc === 'sessions.lifecycle-execute-item').map(([, input]) => input.agentId), ['agt_idle'], 'stop before the next session');
      await shot('7-closed-early');
    } else {
      await page.evaluate(() => { window.hold = false; window.release(); });
      await page.getByRole('heading', { name: 'Archive finished', exact: true }).waitFor();
      assert.equal(await summary().innerText(), 'Archived 3 · 1 not confirmed · 2 blocked');
      const result = await dialog().innerText();
      for (const heading of ['Blocked · 2', 'Not confirmed · 1', 'Archived · 3']) assert.ok(result.includes(heading), heading);
      assert.ok(result.includes('Archived, but stopping its runtime is not confirmed'));
      assert.ok(await page.getByRole('button', { name: 'Review 1 blocked or unattempted session again', exact: true }).isVisible());
      if (largeText) await scale(page);
      await layout(page, 'bulk-archive-dialog', min);
      await shot('7-finished');

      await page.getByRole('button', { name: 'Check outcome: SWE • DEMO-4 • Runtime close reply lost', exact: true }).click();
      await summary().filter({ hasText: 'Archived 4 · 2 blocked' }).waitFor();
      await page.getByRole('button', { name: 'Archived · 4', exact: true }).click();
      assert.ok((await page.getByTestId('bulk-archive-row-agt_b').innerText()).includes('Runtime not reported closed'));
      await shot('8-checked-archived');

      await page.keyboard.press('Escape');
      await dialog().waitFor({ state: 'detached' });
      assert.deepEqual(toasts, [], 'no toast while the dialog showed the result');
      await page.getByTestId('sessions-bulk-status').filter({ hasText: 'Archived 4 · 2 blocked' }).waitFor();
      await page.getByRole('button', { name: 'Deselect 2 blocked sessions', exact: true }).click();
      for (const id of ['agt_lead', 'agt_fence']) assert.equal(await page.getByTestId(`sessions-select-${id}`).getAttribute('aria-checked'), 'false', `${id} deselected`);
      await layout(page, 'sessions-bulk-archive', min);
      await shot('9-toolbar-results');
      await page.getByRole('button', { name: 'Dismiss archive results', exact: true }).click();
      await page.getByTestId('sessions-bulk-status').waitFor({ state: 'detached' });
      const executed = (await calls(page)).filter(([rpc]) => rpc === 'sessions.lifecycle-execute-item').map(([, input]) => input.agentId);
      assert.deepEqual(executed.slice().sort(), ['agt_a', 'agt_b', 'agt_fence', 'agt_idle', 'agt_lead', 'agt_lost'], 'each exact ID once, running child never sent');
      assert.ok(executed.indexOf('agt_idle') < executed.indexOf('agt_lead'), 'child before its parent');
      assert.equal((await calls(page)).filter(([rpc]) => rpc === 'sessions.lifecycle-reconcile-item').length, 1, 'Check reconciles and never resends');
    }
    assert.deepEqual(errors, []);
    report.cases.push({ name, status: 'PASS', checkbox: { clear, selected: on } });
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
    for (const [width, height] of [[320, 568], [390, 844], [1440, 900]]) for (const theme of ['light', 'dark']) for (const largeText of [false, true]) await check(width, height, theme, largeText);
    await check(390, 844, 'dark', false, true);
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  console.log(JSON.stringify(report.cases.map(({ name, status, error }) => ({ name, status, error })), null, 2));
  if (report.cases.some(result => result.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
