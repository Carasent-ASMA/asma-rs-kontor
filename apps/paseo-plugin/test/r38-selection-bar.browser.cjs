// R38 reproduction and measurement of the Sessions selection bar and the bottom space, through
// react-native-web over the full KontorScreen (compact bottom nav included). Supply PREVIEW_ORIGIN,
// PLAYWRIGHT_MODULE and EVIDENCE_DIR. The fixture (an evidence-dir vite bundle of KontorScreen over
// the kontor-prototype runtime) answers every RPC in page memory; its takeover capability states the
// PRODUCTION limit list the installed plugin's server answers with:
//   - TAKEOVER_EFFECTS — apps/paseo-plugin/client/session-takeover.tsx:18,
//   - TAKEOVER_LIMITS — apps/paseo-plugin/shared/session-lifecycle.ts:96,
//   - GOVERNED_CAPABILITY_LIMIT — apps/paseo-plugin/server/session-navigator.ts (GOVERNED_CAPABILITY_LIMIT),
// joined by client/session-operations.tsx:73-74 into the disclosure the SelectionBar prints.
// No native-device, screen-reader or Dynamic Type claim: text200 is a CSS doubling, and the
// home-indicator inset is simulated by an explicit SafeAreaView padding (inflat=<px>).
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply the isolated fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], base: process.env.R38_RUN_LABEL || 'run', browser: null, nativeDeviceAndScreenReaderAcceptance: 'unverified' };
let browser;

// Text scaling stress check, re-applied before each measured state; iOS Dynamic Type remains separate acceptance.
const scale = page => page.evaluate(() => {
  for (const text of document.querySelectorAll('[dir="auto"]:not([data-scaled])')) {
    const style = getComputedStyle(text);
    text.style.fontSize = `${parseFloat(style.fontSize) * 2}px`;
    if (style.lineHeight !== 'normal') text.style.lineHeight = `${parseFloat(style.lineHeight) * 2}px`;
    text.setAttribute('data-scaled', '');
  }
});

// The measured geometry, in CSS px (pt), of the bar, the list and every D3 gap, with each gap
// attributed to the source that owns it. The list is [data-testid="sessions-list"], the bar is
// [data-testid="selection-bar"] (its PARENT is the Sessions screen root View, which owns the
// compact paddingBottom), and the compact nav is the [role="tablist"] labelled Kontor views.
async function measure(page) {
  return page.evaluate(() => {
    const box = id => document.querySelector(`[data-testid="${id}"]`)?.getBoundingClientRect() ?? null;
    const barNode = document.querySelector('[data-testid="selection-bar"]');
    const screen = barNode?.parentElement?.getBoundingClientRect() ?? null;
    const screenStyle = barNode?.parentElement ? getComputedStyle(barNode.parentElement) : null;
    const list = box('sessions-list'), bar = barNode?.getBoundingClientRect() ?? null;
    const navNode = document.querySelector('[role="tablist"][aria-label="Kontor views"]');
    const nav = navNode?.getBoundingClientRect() ?? null;
    const navLabels = navNode ? [...navNode.querySelectorAll('[role="tab"]')].reduce((bottom, tab) => Math.max(bottom, tab.getBoundingClientRect().bottom), 0) : null;
    const buttons = ['selection-takeover', 'selection-takeover-auto', 'selection-archive'].map(box).filter(Boolean);
    const buttonsRowBottom = buttons.length ? Math.max(...buttons.map(node => node.bottom)) : null;
    const safeArea = document.querySelector('#root > div');
    const disclosure = document.querySelector('[data-testid="selection-takeover-disclosure"]');
    const measureBox = document.querySelector('[data-testid="selection-label-measure"]');
    const measure = measureBox?.getBoundingClientRect() ?? null;
    const reasonLines = barNode ? [...barNode.querySelectorAll('[dir="auto"]')].filter(node => !node.closest('[role="button"]') && node.dataset.testid !== 'selection-takeover-disclosure' && !node.closest('[data-testid="selection-label-measure"]') && !node.querySelector('[role="button"]')).map(node => ({ text: node.textContent.slice(0, 40), top: node.getBoundingClientRect().top, bottom: node.getBoundingClientRect().bottom, height: Math.round(node.getBoundingClientRect().height * 10) / 10 })) : [];
    return {
      viewport: { width: innerWidth, height: innerHeight },
      list: list ? { top: list.top, height: list.height } : null,
      bar: bar ? { top: bar.top, bottom: bar.bottom, height: bar.height } : null,
      nav: nav ? { top: nav.top, bottom: nav.bottom, height: nav.height, paddingBottom: getComputedStyle(navNode).paddingBottom } : null,
      navLabelsBottom: navLabels,
      screen: screen ? { top: screen.top, bottom: screen.bottom, height: screen.height, paddingBottom: screenStyle?.paddingBottom } : null,
      gaps: {
        buttonsRowToNextLine: bar && buttonsRowBottom !== null ? Math.min(...reasonLines.map(line => line.top), disclosure ? disclosure.getBoundingClientRect().top : Infinity) - buttonsRowBottom : null,
        barLastLineToNavTop: bar && nav ? nav.top - bar.bottom : null,
        underNav: nav && navLabels ? nav.bottom - navLabels : null,
        navToViewportBottom: nav ? innerHeight - nav.bottom : null,
        listToBar: bar && list ? bar.top - list.bottom : null,
        screenBottomToNavTop: screen && nav ? nav.top - screen.bottom : null,
      },
      reasons: reasonLines,
      disclosure: disclosure ? { chars: (disclosure.textContent ?? '').length, inlineHeight: disclosure.getBoundingClientRect().height, clipped: disclosure.scrollHeight > disclosure.clientHeight + 1 } : null,
      measureBox: measure,
      safeAreaViewPaddingBottom: safeArea ? getComputedStyle(safeArea).paddingBottom : null,
    };
  });
}

async function selectionState(page) {
  return page.evaluate(() => {
    const count = document.querySelector('[data-testid="selection-count"]');
    const rows = [...document.querySelectorAll('[data-testid^="sessions-row-"]')].filter(node => {
      const box = node.getBoundingClientRect();
      return box.height > 0 && box.bottom > 0 && box.top < innerHeight;
    });
    return { count: count?.textContent ?? null, visibleRows: rows.length };
  });
}

const rowBody = (page, id) => page.getByTestId(`sessions-row-${id}`).getByTestId('session-row-body');
const shot = (page, name) => page.screenshot({ path: path.join(output, `${name}.png`) });

async function enterSelection(page, firstRowId) {
  await page.getByTestId(`session-actions-${firstRowId}`).click();
  const menu = page.getByRole('dialog', { name: 'Session actions', exact: true });
  await menu.getByTestId('session-action-select').click();
  await menu.waitFor({ state: 'detached' });
}

async function measureSelection(page, name, expectCount) {
  await page.waitForTimeout(250);
  const state = await selectionState(page);
  const geometry = await measure(page);
  await shot(page, `${name}`);
  report.cases.push({ name, expectCount, state, geometry });
  console.log(`${name}: count=${state.count} visibleRows=${state.visibleRows} list=${geometry.list ? Math.round(geometry.list.height) : null}px bar=${geometry.bar ? Math.round(geometry.bar.height) : null}px disclosure=${geometry.disclosure ? `${geometry.disclosure.chars} chars` : 'absent'} gaps=${JSON.stringify(Object.fromEntries(Object.entries(geometry.gaps).map(([key, value]) => [key, value === null || value === undefined || !Number.isFinite(value) ? null : Math.round(value * 10) / 10])))}`);
  return { state, geometry };
}

// One pass per viewport and text size: reproduce, measure at 0/1/2/5/all, then the interaction checks.
// extra: fixture query params (inflat, state) — the device-equivalent viewports use them.
async function pass(width, height, theme, largeText, extra = {}) {
  const touch = width < 600;
  const name = `${width}x${height}-${theme}${largeText ? '-text200' : ''}${extra.inflat ? '-inset34' : ''}${extra.state ? '-mywork' : ''}`;
  const context = await browser.newContext({ viewport: { width, height }, ...(touch ? { hasTouch: true, isMobile: true } : {}) });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    const query = new URLSearchParams({ scenario: 'baseline', theme, layout: touch ? 'compact' : 'wide', ...(extra.inflat ? { inflat: String(extra.inflat) } : {}), ...(extra.state ? { state: extra.state } : {}) });
    await page.goto(`${origin}/?${query}`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    if (largeText) { await scale(page); await page.waitForTimeout(150); }

    await measureSelection(page, `${name}-0-selected`, '0');

    // Select one from the row menu; the bar appears and 1-selected is the repro state.
    await enterSelection(page, 'agt_lsa_99101');
    const one = await measureSelection(page, `${name}-1-selected`, '1');
    assert.ok(one.geometry.bar, `${name}: the selection bar renders at 1`);
    // The disclosure is the production wall: TAKEOVER_EFFECTS + the host limits, 1,700 characters or more.
    assert.ok(one.geometry.disclosure, `${name}: the takeover disclosure line renders while Take over can start`);
    assert.ok(one.geometry.disclosure.chars >= 1700, `${name}: disclosure is production length (>=1700 chars), got ${one.geometry.disclosure.chars}`);

    for (const id of ['agt_tpm_99101']) await rowBody(page, id).click();
    const two = await measureSelection(page, `${name}-2-selected`, '2');
    for (const id of ['agt_qa_99103', 'agt_aud_99103', 'agt_reva_99104']) await rowBody(page, id).click();
    const five = await measureSelection(page, `${name}-5-selected`, '5');
    await page.getByTestId('selection-all').click();
    const all = await measureSelection(page, `${name}-all-selected`, 'all');

    // Rows stay tappable while selecting: a visible row toggles at every count (D2).
    for (const [stage] of [['one'], ['two'], ['five'], ['all']]) {
      const visible = stage === 'one' ? await rowBody(page, 'agt_qa_99104').isVisible() : await rowBody(page, 'agt_qa_99104').isVisible();
      if (!visible) { report.cases.push({ name: `${name}-row-tap-${stage}`, status: 'FAIL', error: 'a session row is not visible to tap while selecting' }); console.log(`${name}-row-tap-${stage}: FAIL (row not visible)`); continue; }
      const countBefore = (await selectionState(page)).count;
      await rowBody(page, 'agt_qa_99104').click();
      await page.waitForTimeout(150);
      const countAfter = (await selectionState(page)).count;
      const toggled = countBefore !== countAfter;
      report.cases.push({ name: `${name}-row-tap-${stage}`, status: toggled ? 'PASS' : 'FAIL', countBefore, countAfter });
      console.log(`${name}-row-tap-${stage}: ${toggled ? 'PASS' : 'FAIL'} (${countBefore} -> ${countAfter})`);
      if (toggled) await rowBody(page, 'agt_qa_99104').click(); // restore the count
      await page.waitForTimeout(100);
    }

    // D1/D2/D3 gate assertions — these encode the fix's targets and are expected RED at base.
    const gated = (label, condition, detail) => {
      report.cases.push({ name: `${name}-gate-${label}`, status: condition ? 'PASS' : 'FAIL', detail });
      console.log(`${name}-gate-${label}: ${condition ? 'PASS' : 'FAIL'} ${detail ?? ''}`);
    };
    // D2: while selecting on compact, the list area is at least max(40% of the Sessions root height, 3 rows).
    if (touch) {
      const screenRoot = await page.evaluate(() => {
        const bar = document.querySelector('[data-testid="selection-bar"]');
        return bar?.parentElement?.getBoundingClientRect().height ?? 0;
      });
      const listHeight = one.geometry.list?.height ?? 0;
      const threeRows = 3 * 60; // a compact row is at least ~60 px tall at default text
      gated('list-floor-1', listHeight >= Math.max(0.4 * screenRoot, threeRows), `list ${Math.round(listHeight)}px vs floor ${Math.round(Math.max(0.4 * screenRoot, threeRows))}px (root ${Math.round(screenRoot)}px)`);
    }
    // D1: the effects line is bounded to two lines (it must clip, not push).
    if (one.geometry.disclosure) {
      gated('effects-bounded-2-lines', one.geometry.disclosure.inlineHeight <= 2 * 16 + 2, `effects line ${Math.round(one.geometry.disclosure.inlineHeight)}px (unbounded at base renders ~25 lines)`);
    }
    // D3: buttons row to next line <= 6; bar last line to nav top <= 8; under nav = inset + <= 6.
    if (touch && one.geometry.gaps.buttonsRowToNextLine !== null && Number.isFinite(one.geometry.gaps.buttonsRowToNextLine)) {
      gated('buttons-row-gap', one.geometry.gaps.buttonsRowToNextLine <= 6, `${Math.round(one.geometry.gaps.buttonsRowToNextLine)}px`);
    }
    if (touch && one.geometry.gaps.barLastLineToNavTop !== null) {
      gated('bar-to-nav-gap', one.geometry.gaps.barLastLineToNavTop <= 8, `${Math.round(one.geometry.gaps.barLastLineToNavTop)}px`);
    }
    if (touch && one.geometry.gaps.underNav !== null) {
      gated('under-nav-single-inset', one.geometry.gaps.underNav <= 6, `${Math.round(one.geometry.gaps.underNav)}px (browser inset is 0; nav paddingBottom only)`);
    }

    // D1: the Limits control — collapsed by default, opens the full disclosure held, closes on Escape.
    const limits = page.getByTestId('selection-limits');
    if (!(await limits.isVisible())) {
      gated('limits-control', false, 'no Limits control (base renders the limits inline, unbounded)');
    } else {
      const expandedBefore = await limits.getAttribute('aria-expanded');
      gated('limits-collapsed-default', await page.getByTestId('control-help').count() === 0, `aria-expanded ${expandedBefore}`);
      // When the screen cannot fit the bar's content, the bar scrolls inside itself (D2); reach the
      // control the way an operator does — the bar's own scroll — before tapping.
      await page.evaluate(() => {
        const scroller = document.querySelector('[data-testid="selection-bar"]')?.querySelector('div');
        if (scroller && scroller.scrollHeight > scroller.clientHeight) scroller.scrollTop = scroller.scrollHeight;
      });
      await page.waitForTimeout(100);
      await limits.click();
      await page.waitForTimeout(150);
      const helpText = await page.getByTestId('control-help').innerText().catch(() => null);
      gated('limits-opens-tap', helpText !== null && helpText.length >= 1700, `help panel ${helpText === null ? 'absent' : `${helpText.length} chars`}`);
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      gated('limits-escape-closes', await page.getByTestId('control-help').count() === 0, 'Escape closes the held popover');
      // Keyboard on a FRESH mount: the control's expanded flag is per-mount (the provider's Escape and
      // outside-press dismissals re-sync on this control's next press), so the keyboard contract is
      // measured without the tap sequence's state. Enter activates the button (its hold() bypasses the
      // pointer-mode guard), Escape closes.
      await page.reload();
      await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
      await enterSelection(page, 'agt_lsa_99101');
      await page.waitForTimeout(250);
      const limits2 = page.getByTestId('selection-limits');
      // Same operator step as the tap path: reveal the control through the bar's own scroll when tight.
      await page.evaluate(() => {
        const scroller = document.querySelector('[data-testid="selection-bar"]')?.querySelector('div');
        if (scroller && scroller.scrollHeight > scroller.clientHeight) scroller.scrollTop = scroller.scrollHeight;
      });
      await page.waitForTimeout(100);
      await limits2.focus();
      await page.keyboard.press('Enter');
      await page.waitForTimeout(150);
      gated('limits-opens-keyboard', await page.getByTestId('control-help').count() > 0, 'Enter opens the held limits');
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      gated('limits-keyboard-closes', await page.getByTestId('control-help').count() === 0, 'Escape closes again');
    }

    if (errors.length) report.cases.push({ name: `${name}-page-errors`, status: 'FAIL', errors });
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack ?? String(error) });
    console.log(`${name}: FAIL ${error.message}`);
  } finally {
    await context.close();
  }
}

// Simulated-inset fixture: the host inset enters only through SafeAreaView paddingBottom
// (kontor-screen.tsx:354); drive it explicitly and verify the under-nav distance is inset + 6, once.
async function insetPass(width, height) {
  const name = `${width}x${height}-inset34`;
  const context = await browser.newContext({ viewport: { width, height }, hasTouch: true, isMobile: true });
  const page = await context.newPage();
  try {
    await page.goto(`${origin}/?scenario=baseline&theme=light&layout=compact&inflat=34`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    await enterSelection(page, 'agt_lsa_99101');
    await page.waitForTimeout(600); // the fixture re-applies the inset at 50 ms and 400 ms
    const one = await measureSelection(page, `${name}-1-selected`, '1');
    const underNav = one.geometry.gaps.underNav;
    const navPadding = parseFloat(one.geometry.nav?.paddingBottom ?? '0');
    const belowNav = one.geometry.gaps.navToViewportBottom;
    // The injected inset is the ONLY thing below the nav's border box: nav.bottom -> viewport == inset,
    // and the labels -> nav.bottom == the nav's own paddingBottom. Nothing else may join them.
    const insetOnly = Math.abs(belowNav - 34) <= 1.5 && Math.abs(underNav - navPadding) <= 1.5;
    report.cases.push({ name: `${name}-under-nav`, status: insetOnly ? 'PASS' : 'FAIL', inset: 34, navPaddingBottom: navPadding, navBottomToViewport: belowNav, labelsToNavBottom: underNav });
    console.log(`${name}-under-nav: ${insetOnly ? 'PASS' : 'FAIL'} inset 34 below the nav border (${belowNav}) + nav paddingBottom ${navPadding} above it (${underNav}); no second inset`);
    // No second inset: the distance from the last nav label to the viewport bottom is exactly the injected inset plus the nav's own padding.
    const labelToBottom = await page.evaluate(() => {
      const nav = document.querySelector('[role="tablist"][aria-label="Kontor views"]');
      const labels = [...nav.querySelectorAll('[role="tab"]')];
      const bottom = Math.max(...labels.map(node => node.getBoundingClientRect().bottom));
      return innerHeight - bottom;
    });
    const navPaddingSecond = parseFloat(one.geometry.nav?.paddingBottom ?? '0');
    const single = Math.abs(labelToBottom - (34 + navPaddingSecond)) <= 1.5;
    report.cases.push({ name: `${name}-labels-to-viewport`, status: single ? 'PASS' : 'FAIL', labelsBottomToViewport: labelToBottom, insetPlusNavPadding: 34 + navPaddingSecond });
    console.log(`${name}-labels-to-viewport: ${single ? 'PASS' : 'FAIL'} last label -> viewport ${labelToBottom}, expected inset 34 + navPadding ${navPaddingSecond} (no double inset)`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack ?? String(error) });
    console.log(`${name}: FAIL ${error.message}`);
  } finally {
    await context.close();
  }
}

(async () => {
  browser = await chromium.launch({ headless: true });
  report.browser = browser.version();
  try {
    for (const [width, height] of [[430, 932], [393, 852]]) {
      await pass(width, height, 'light', false);
      await pass(width, height, 'light', true);
      await insetPass(width, height);
    }
    // Device-equivalent viewports: the phone's window minus the host chrome (status bar + Paseo header,
    // 122 pt), with the simulated 34 pt home inset and Igor's My work state (the appliedSummary line).
    for (const [width, height] of [[430, 810], [393, 730]]) {
      await pass(width, height, 'light', false, { inflat: 34, state: 'mywork' });
      await pass(width, height, 'light', true, { inflat: 34, state: 'mywork' });
    }
    // Wide is measured for the unchanged-metrics comparison; no gate assertions there.
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();
    try {
      await page.goto(`${origin}/?scenario=baseline&theme=light&layout=wide`);
      await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
      const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
      if (await hide.isVisible()) await hide.click();
      await measureSelection(page, '1280x800-light-0-selected', '0');
      await enterSelection(page, 'agt_lsa_99101');
      const one = await measureSelection(page, '1280x800-light-1-selected', '1');
      report.cases.push({ name: 'wide-metrics', status: 'PASS', bar: one.geometry.bar, screen: one.geometry.screen, nav: one.geometry.nav, gaps: one.geometry.gaps });
    } finally { await context.close(); }
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  const failed = report.cases.filter(result => result.status === 'FAIL');
  console.log(`\ncases: ${report.cases.length}, FAIL: ${failed.length}`);
  for (const result of failed) console.log(`FAIL ${result.name}: ${result.error ?? result.detail ?? ''}`);
  if (failed.length) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
