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
// R38 V2: the inline line is exactly "Take over: " + the fixed effects sentence. Read the constant
// from the client source so the harness cannot drift from it (session-takeover.tsx:18).
const TAKEOVER_EFFECTS = fs.readFileSync(path.join(__dirname, '../client/session-takeover.tsx'), 'utf8')
  .match(/TAKEOVER_EFFECTS = "([^"]+)"/)?.[1];
assert.ok(TAKEOVER_EFFECTS && TAKEOVER_EFFECTS.length > 80, 'TAKEOVER_EFFECTS extracted from the client source');
const INLINE_LINE = `Take over: ${TAKEOVER_EFFECTS}`;
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
    const buttonsRowTop = buttons.length ? Math.min(...buttons.map(node => node.top)) : null;
    const safeArea = document.querySelector('#root > div');
    const disclosure = document.querySelector('[data-testid="selection-takeover-disclosure"]');
    const measureBox = document.querySelector('[data-testid="selection-label-measure"]');
    const measure = measureBox?.getBoundingClientRect() ?? null;
    const reasonLines = barNode ? [...barNode.querySelectorAll('[dir="auto"]')].filter(node => !node.closest('[role="button"]') && node.dataset.testid !== 'selection-takeover-disclosure' && !node.closest('[data-testid="selection-label-measure"]') && !node.querySelector('[role="button"]')).map(node => ({ text: node.textContent.slice(0, 40), top: node.getBoundingClientRect().top, bottom: node.getBoundingClientRect().bottom, height: Math.round(node.getBoundingClientRect().height * 10) / 10 })) : [];
    // R38 F2, honest metric: the last VISIBLE text line (disclosure, reason, or the Limits label) —
    // clamped to the bar's own clip — not the bar's box, which can carry the 44pt target's slack.
    const textNodes = barNode ? [...barNode.querySelectorAll('[dir="auto"]')].filter(node => !node.closest('[data-testid="selection-label-measure"]')) : [];
    const lastTextBottom = bar ? textNodes.reduce((bottom, node) => {
      const rect = node.getBoundingClientRect();
      if (rect.top >= bar.bottom - 0.5) return bottom;                    // below the fold: not visible
      return Math.max(bottom, Math.min(rect.bottom, bar.bottom));         // visible, clamped to the bar's clip
    }, 0) : null;
    // R38 NOTE-A: the glyph (Range content) rect of that same last visible text line, reported NEXT TO
    // the line-box number — the box carries half-leading, the Range rect tracks the laid-out content.
    const lastGlyphBottom = bar ? textNodes.reduce((bottom, node) => {
      const rect = node.getBoundingClientRect();
      if (rect.top >= bar.bottom - 0.5) return bottom;
      const range = document.createRange(); range.selectNodeContents(node);
      const glyph = range.getBoundingClientRect();
      return Math.max(bottom, Math.min(glyph.bottom, bar.bottom));
    }, 0) : null;
    const scroller = barNode?.querySelector('div');
    const barChildren = scroller ? [...scroller.children].filter(node => node.dataset.testid !== 'selection-label-measure')
      .map(node => ({ part: node.dataset.testid ?? node.querySelector('[data-testid^="selection-"]')?.getAttribute('data-testid') ?? 'reason-line', height: Math.round(node.getBoundingClientRect().height) })) : [];
    const stacked = buttons.length === 3 ? buttons[2].top > buttons[1].bottom + 1 : null;
    // F3: on a short root the bar's text may yield (scroll away) — when the effects row is clipped by
    // the bar's own scroll box, the last-text breath is reported but not bounded.
    const effectsClipped = bar && disclosure ? disclosure.getBoundingClientRect().bottom > bar.bottom + 1 : false;
    return {
      viewport: { width: innerWidth, height: innerHeight },
      list: list ? { top: list.top, height: list.height } : null,
      listTop: list && screen ? list.top - screen.top : null,
      stacked,
      effectsClipped,
      barChildren,
      buttonRects: buttons.map(node => ({ top: Math.round(node.top), bottom: Math.round(node.bottom), left: Math.round(node.left), right: Math.round(node.right) })),
      bar: bar ? { top: bar.top, bottom: bar.bottom, height: bar.height } : null,
      nav: nav ? { top: nav.top, bottom: nav.bottom, height: nav.height, paddingBottom: getComputedStyle(navNode).paddingBottom } : null,
      navLabelsBottom: navLabels,
      screen: screen ? { top: screen.top, bottom: screen.bottom, height: screen.height, paddingBottom: screenStyle?.paddingBottom } : null,
      gaps: {
        // R38-D2-r2 order: text first, buttons row LAST — the D3 no-dead-band contract now measures the
        // gap from the last text line ABOVE the row to the row's top (same ≤ 6 contract, inverted pair).
        buttonsRowGapFromText: bar && buttonsRowTop !== null ? buttonsRowTop - Math.max(...reasonLines.map(line => line.bottom), disclosure ? disclosure.getBoundingClientRect().bottom : 0) : null,
        barLastLineToNavTop: bar && nav ? nav.top - bar.bottom : null,
        barLastTextToNavTop: bar && nav && lastTextBottom !== null ? nav.top - lastTextBottom : null,
        barLastGlyphToNavTop: bar && nav && lastGlyphBottom !== null ? nav.top - lastGlyphBottom : null,
        underNav: nav && navLabels ? nav.bottom - navLabels : null,
        navToViewportBottom: nav ? innerHeight - nav.bottom : null,
        listToBar: bar && list ? bar.top - list.bottom : null,
        screenBottomToNavTop: screen && nav ? nav.top - screen.bottom : null,
      },
      reasons: reasonLines,
      disclosure: disclosure ? { chars: (disclosure.textContent ?? '').length, text: disclosure.textContent ?? '', inlineHeight: disclosure.getBoundingClientRect().height, clipped: disclosure.scrollHeight > disclosure.clientHeight + 1 } : null,
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

// Reach a control in the bar the way an operator does — the bar's own scroll, just far enough to
// reveal the control — and press it with a real pointer press at freshly read coordinates. The
// reach+read+dispatch is retried with an outcome check: background refetches can reflow text
// between a box read and the dispatch (the press must land on the control, verified, or retry).
async function tapBarControl(page, testId) {
  for (let attempt = 0; attempt < 3; attempt++) {
    const outcome = await page.evaluate((id) => {
      const barNode = document.querySelector('[data-testid="selection-bar"]');
      const scroller = barNode?.querySelector('div');
      const control = document.querySelector(`[data-testid="${id}"]`);
      if (!barNode || !control) return { state: 'missing' };
      const s = scroller.getBoundingClientRect(), c = control.getBoundingClientRect();
      if (c.width === 0 || c.height === 0) return { state: 'missing' };
      if (c.top < s.top + 1) scroller.scrollTop += c.top - s.top - 2;
      else if (c.bottom > s.bottom - 1) scroller.scrollTop += c.bottom - s.bottom + 2;
      const r = control.getBoundingClientRect();
      const x = r.x + r.width / 2, y = r.y + r.height / 2;
      const hit = document.elementFromPoint(x, y);
      if (!hit || (!control.contains(hit) && hit !== control)) return { state: 'covered', hit: `${hit?.tagName}.${hit?.closest('[data-testid]')?.getAttribute('data-testid') ?? ''}` };
      return { state: 'ready', x, y };
    }, testId);
    if (outcome.state === 'ready') {
      await page.mouse.click(outcome.x, outcome.y);
      await page.waitForTimeout(180);
      // The caller decides what one press means (R9 AUD-F3): for a closing press the panel's
      // ABSENCE is the success state, so the open-panel check only guards the opening presses.
      if (!expectOpen || await page.getByTestId('control-help').count() > 0) return true;
    }
    await page.waitForTimeout(300);
  }
  return false;
}

// R38 F4: text200 must cover the BAR too — scale() runs before this pass scales only the chrome and
// list rows that exist then; the bar mounts later, so every measurement re-applies the doubling (the
// :not([data-scaled]) marker keeps it idempotent) before the geometry is read.
let scaleText = false;

async function measureSelection(page, name, expectCount) {
  if (scaleText) { await scale(page); await page.waitForTimeout(400); }
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
  scaleText = largeText;
  const name = `${width}x${height}-${theme}${largeText ? '-text200' : ''}${extra.inflat ? '-inset34' : ''}${extra.state ? '-mywork' : ''}`;
  const context = await browser.newContext({ viewport: { width, height }, ...(touch ? { hasTouch: true, isMobile: true } : {}) });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const gated = (label, condition, detail) => {
    report.cases.push({ name: `${name}-gate-${label}`, status: condition ? 'PASS' : 'FAIL', detail });
    console.log(`${name}-gate-${label}: ${condition ? 'PASS' : 'FAIL'} ${detail ?? ''}`);
  };
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
    // R38 V2: the inline line is EXACTLY "Take over: " + the effects sentence — the host limits ride
    // only the Limits popover (whose held text stays the full ~1,700-char production disclosure).
    assert.ok(one.geometry.disclosure, `${name}: the takeover disclosure line renders while Take over can start`);
    assert.equal(one.geometry.disclosure.text, INLINE_LINE, `${name}: the inline line is the effects sentence only (V2)`);
    // R38-D2-r3, AT REST with no harness scroll: when the bar's content does not fit, the bar starts
    // scrolled to its END, so the buttons row shows at rest — Archive whole and tappable, Take over
    // and Auto visible and hit-testable at their visible centre.
    if (extra.short && one.geometry.buttonRects?.length === 3) {
      const atRest = await page.evaluate(() => {
        const barNode = document.querySelector('[data-testid="selection-bar"]');
        const scroller = barNode?.querySelector('div');
        const bar = barNode.getBoundingClientRect();
        const inside = (node) => { const r = node.getBoundingClientRect(); return r.top >= bar.top - 1 && r.bottom <= bar.bottom + 1 && r.left >= bar.left - 1 && r.right <= bar.right + 1; };
        const labelWhole = (node) => { const label = node.querySelector('[dir="auto"]'); const range = document.createRange(); if (label) range.selectNodeContents(label); return !label || range.getBoundingClientRect().width <= label.getBoundingClientRect().width + 1; };
        const hitAtVisibleCentre = (node) => {
          const r = node.getBoundingClientRect();
          const top = Math.max(r.top, bar.top), bottom = Math.min(r.bottom, bar.bottom);
          if (bottom - top < 20) return false;
          const hit = document.elementFromPoint(r.x + r.width / 2, (top + bottom) / 2);
          return hit === node || node.contains(hit);
        };
        const byId = (id) => document.querySelector(`[data-testid="${id}"]`);
        const archive = byId('selection-archive'), takeover = byId('selection-takeover'), auto = byId('selection-takeover-auto');
        return {
          scrollTop: Math.round(scroller?.scrollTop ?? -1),
          archiveWholeAndTappable: !!archive && inside(archive) && labelWhole(archive) && (() => { const r = archive.getBoundingClientRect(); const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2); return hit === archive || archive.contains(hit); })(),
          takeoverHit: !!takeover && hitAtVisibleCentre(takeover),
          autoHit: !!auto && hitAtVisibleCentre(auto),
        };
      });
      gated('buttons-at-rest', atRest.archiveWholeAndTappable && atRest.takeoverHit && atRest.autoHit, `rest scrollTop ${atRest.scrollTop} (no harness scroll): archive whole+tappable ${atRest.archiveWholeAndTappable}, takeover hit ${atRest.takeoverHit}, auto hit ${atRest.autoHit}`);
    }

    // The 2/5/all progression and the row-tap check need visible rows; on a short root (R38 F3) the
    // floor yields to the bar's buttons and the list intentionally has no room, so they are skipped —
    // the F3 proof at these roots is 0/1 with the buttons row fully visible (gates below).
    if (!extra.short) {
      for (const id of ['agt_tpm_99101']) await rowBody(page, id).click();
      const two = await measureSelection(page, `${name}-2-selected`, '2');
      for (const id of ['agt_qa_99103', 'agt_aud_99103', 'agt_reva_99104']) await rowBody(page, id).click();
      const five = await measureSelection(page, `${name}-5-selected`, '5');
      await page.getByTestId('selection-all').click();
      const all = await measureSelection(page, `${name}-all-selected`, 'all');
      void two; void five; void all;

      // Rows stay tappable while selecting: a visible row toggles at every count (D2).
      for (const [stage] of [['one'], ['two'], ['five'], ['all']]) {
        const visible = await rowBody(page, 'agt_qa_99104').isVisible();
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
    }

    // D1/D2/D3 gate assertions — these encode the fix's targets and are expected RED at base.
    // (gated is defined at the top of this try block.)
    // D2: while selecting on compact, the list area is at least max(40% of the Sessions root, 3×96),
    // CAPPED so the bar's buttons stay visible on a short root (R38 F3): the floor yields first.
    if (touch) {
      const screenRoot = await page.evaluate(() => {
        const bar = document.querySelector('[data-testid="selection-bar"]');
        return bar?.parentElement?.getBoundingClientRect().height ?? 0;
      });
      const listHeight = one.geometry.list?.height ?? 0;
      const uncapped = Math.max(0.4 * screenRoot, 288);
      const floor = Math.min(uncapped, Math.max(0, screenRoot - (one.geometry.listTop ?? 0) - 4 - 112));
      gated('list-floor-1', listHeight >= floor - 0.5, `list ${Math.round(listHeight)}px vs floor ${Math.round(floor)}px (root ${Math.round(screenRoot)}px, uncapped ${Math.round(uncapped)}, listTop ${Math.round(one.geometry.listTop ?? 0)})`);
    }
    // D1: the effects line is bounded to two lines (lineHeight 22; doubled at text200 — it must clip, not push).
    if (one.geometry.disclosure) {
      const twoLineBound = largeText ? 2 * 44 + 2 : 2 * 22 + 2;
      gated('effects-bounded-2-lines', one.geometry.disclosure.inlineHeight <= twoLineBound, `effects line ${Math.round(one.geometry.disclosure.inlineHeight)}px (unbounded at base renders ~25 lines)`);
      // R38 V2: the visible two-line text is what a screen reader reads — no host limit rides inline.
      gated('inline-line-text', one.geometry.disclosure.text === INLINE_LINE, `${one.geometry.disclosure.chars} chars, exactly "Take over: " + the effects sentence (limits only in the Limits popover)`);
      // R38 D2-r2: NOTHING on the Sessions screen draws under the nav — the bar's bottom stays at
      // least 4 pt above the screen root's bottom at every text size (the root is the bar's parent).
      const screenBottom = one.geometry.screen?.bottom ?? null;
      if (touch && screenBottom !== null && one.geometry.bar) {
        gated('bar-bottom-breath', one.geometry.bar.bottom <= screenBottom - 4 + 0.5, `bar bottom ${Math.round(one.geometry.bar.bottom)} vs root bottom ${Math.round(screenBottom)} (breath ${Math.round(screenBottom - one.geometry.bar.bottom)}pt)`);
      }
    }
    // D3: buttons row to next line <= 6; bar's last VISIBLE text line to nav top <= 8 (F2 honest
    // metric — the box number is kept for the record); under nav = inset + <= 6.
    if (touch && one.geometry.gaps.buttonsRowGapFromText !== null && Number.isFinite(one.geometry.gaps.buttonsRowGapFromText)) {
      gated('buttons-row-gap', one.geometry.gaps.buttonsRowGapFromText <= 6, `${Math.round(one.geometry.gaps.buttonsRowGapFromText)}px (last text line above the row -> row top)`);
    }
    if (touch && one.geometry.gaps.barLastTextToNavTop !== null) {
      // The bar is content-sized (no minHeight, R38-D2-r2) and fills its box to the bottom with real
      // content — text above, the buttons row last — so the honest box breath IS the no-dead-band
      // contract; the last-text and glyph numbers ride the geometry for the F2 tables.
      const clipped = one.geometry.effectsClipped;
      const value = one.geometry.gaps.barLastLineToNavTop;
      gated('bar-to-nav-gap', value <= 8, `box -> nav ${Math.round(value)}px (content-filled; last text -> nav ${Math.round(one.geometry.gaps.barLastTextToNavTop)}px, glyph -> nav ${one.geometry.gaps.barLastGlyphToNavTop !== null ? Math.round(one.geometry.gaps.barLastGlyphToNavTop) : 'n/a'}${clipped ? '; effects row yields at this root' : ''})`);
    }
    // R38-D2-r2: nothing draws under the nav. Where everything fits, the buttons row is fully visible
    // above the nav; where the root is too short (short roots at text200), the bar takes what is left
    // and ITS OWN ScrollView scrolls the buttons — proved by scrolling to the end: Archive whole and
    // tappable (elementFromPoint at its centre is the Archive control). No label is cut, no control
    // is covered, at both text sizes.
    if (touch && one.geometry.buttonRects?.length === 3 && one.geometry.nav) {
      const viewportWidth = one.geometry.viewport.width;
      const inBar = one.geometry.buttonRects.every(rect => rect.top >= (one.geometry.bar?.top ?? 0) - 2 && rect.bottom <= one.geometry.nav.top + 1 && rect.left >= -1 && rect.right <= viewportWidth + 1);
      if (inBar) {
        gated('buttons-visible', true, 'takeover/auto/archive fully inside the bar, above the nav (no scroll needed)');
      } else {
        // The bar yields: scroll its scroller to the end and prove Archive is whole and tappable there.
        await page.evaluate(() => {
          const scroller = document.querySelector('[data-testid="selection-bar"]')?.querySelector('div');
          if (scroller && scroller.scrollHeight > scroller.clientHeight) scroller.scrollTop = scroller.scrollHeight;
        });
        await page.waitForTimeout(150);
        const scrolled = await page.evaluate(() => {
          const barNode = document.querySelector('[data-testid="selection-bar"]');
          const archive = document.querySelector('[data-testid="selection-archive"]');
          if (!barNode || !archive) return { whole: false, tappable: false };
          const barBox = barNode.getBoundingClientRect(), box = archive.getBoundingClientRect();
          const whole = box.top >= barBox.top - 1 && box.bottom <= barBox.bottom + 1 && box.left >= barBox.left - 1 && box.right <= barBox.right + 1;
          const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
          const tappable = hit === archive || archive.contains(hit);
          // And the label is not cut: the Range content rect stays inside the label's box.
          const label = archive.querySelector('[dir="auto"]');
          const range = document.createRange(); if (label) range.selectNodeContents(label);
          const wholeLabel = !label || range.getBoundingClientRect().width <= label.getBoundingClientRect().width + 1;
          return { whole, tappable, wholeLabel, scrollTop: Math.round(barNode.querySelector('div')?.scrollTop ?? 0) };
        });
        gated('buttons-visible', scrolled.whole && scrolled.tappable && scrolled.wholeLabel, `buttons yield to the root: bar's own scroll reaches Archive whole (${scrolled.whole}) and tappable (${scrolled.tappable}, label whole ${scrolled.wholeLabel}) at scroll ${scrolled.scrollTop}`);
      }
      const labelsWhole = await page.evaluate(() => [...document.querySelectorAll('[data-testid="selection-bar"] [role="button"] [dir="auto"]')].every((node) => {
        // CSS ellipsis is a paint effect: the DOM text keeps its full laid-out width, so the Range
        // content box exposes truncation that scrollWidth hides.
        const range = document.createRange(); range.selectNodeContents(node);
        return range.getBoundingClientRect().width <= node.getBoundingClientRect().width + 1;
      }));
      gated('labels-whole', labelsWhole, 'no button label ellipsized');
    }
    if (touch && one.geometry.gaps.underNav !== null) {
      gated('under-nav-single-inset', one.geometry.gaps.underNav <= 6, `${Math.round(one.geometry.gaps.underNav)}px (browser inset is 0; nav paddingBottom only)`);
    }

    // R38-D2-r3, never fights the user: scroll the bar UP with a real wheel gesture (so RNW's own
    // onScroll events fire and release the pin), then a content-neutral re-render — a fixture refetch
    // tick (the same synthetic data, so the bar's content size and height are unchanged) — and the
    // offset must hold, asserted >= 300 ms later, past RNW's 100 ms scroll-end emission. Preconditions
    // are asserted and printed; where they cannot be met the case is NOT RUN with the reason, never
    // passed vacuously (the bar must actually scroll, and the resting offset must sit >= 20 px below
    // the end). Runs at the short roots; at 430×330 100% there is only 8 px of scroll room, at
    // 393×420 100% the bar cannot scroll at all.
    if (touch && extra.short) {
      const pre = await page.evaluate(() => {
        const scroller = document.querySelector('[data-testid="selection-bar"]')?.querySelector('div');
        if (!scroller) return null;
        return { sh: scroller.scrollHeight, ch: scroller.clientHeight, box: document.querySelector('[data-testid="selection-bar"]')?.getBoundingClientRect().toJSON?.() ?? null };
      });
      if (!pre || pre.sh <= pre.ch) {
        report.cases.push({ name: `${name}-gate-scroll-holds-against-rerender`, status: 'NOT RUN', detail: `the bar cannot scroll at this root (scrollHeight ${pre?.sh ?? '?'} <= clientHeight ${pre?.ch ?? '?'}); no qualifying re-render exists` });
        console.log(`${name}-gate-scroll-holds-against-rerender: NOT RUN (bar cannot scroll)`);
      } else {
        const barBox = await page.locator('[data-testid="selection-bar"]').boundingBox();
        const vh = page.viewportSize().height;
        await page.mouse.move(barBox.x + barBox.width / 2, Math.max(10, Math.min(barBox.y + barBox.height / 2, vh - 10)));
        await page.mouse.wheel(0, -240);
        await page.waitForTimeout(400); // settle, past RNW's 100 ms scroll-end emission
        const after = await page.evaluate(() => {
          const scroller = document.querySelector('[data-testid="selection-bar"] div');
          return { top: scroller.scrollTop, sh: scroller.scrollHeight, ch: scroller.clientHeight };
        });
        const end = after.sh - after.ch, belowEnd = end - after.top;
        if (belowEnd < 20) {
          report.cases.push({ name: `${name}-gate-scroll-holds-against-rerender`, status: 'NOT RUN', detail: `the bar scrolls but only by ${end}px (rest offset ${after.top}, ${belowEnd}px below the end < 20); no qualifying hold can be shown` });
          console.log(`${name}-gate-scroll-holds-against-rerender: NOT RUN (only ${end}px of scroll room)`);
        } else {
          // The content-neutral re-render: the fixture's refetch tick is a real re-render that does
          // NOT reach the pin's trigger (RNW derives onContentSizeChange from the content container's
          // layout reporting, which fires only on observe and on a real size change — the refetch
          // alone fires nothing). The gate then calls the scroller's own onContentSizeChange prop
          // directly, through the fiber, with UNCHANGED dimensions — a stand-in for the next real
          // size or height change. That seam keeps the release under test real (a real wheel gesture
          // → RNW onScroll → pinnedToEnd false), while the at-rest gates (rest positions 8/115/0/91)
          // prove the platform's real triggers do call repinToEnd. Nothing about the bar's content or
          // height changes here; a correct pin must not move, one that ignores the flag (M-pin-always)
          // jumps back to the end.
          await page.evaluate(() => window.__r38Refetch());
          await page.waitForTimeout(200);
          const fired = await page.evaluate(() => {
            const scroller = document.querySelector('[data-testid="selection-bar"] div');
            const key = Object.keys(scroller).find((k) => k.startsWith('__reactFiber'));
            let cursor = scroller[key];
            while (cursor) {
              const props = cursor.memoizedProps;
              if (props && typeof props.onContentSizeChange === 'function') {
                props.onContentSizeChange(scroller.scrollWidth, scroller.scrollHeight);
                return true;
              }
              cursor = cursor.return;
            }
            return false;
          });
          await page.waitForTimeout(400);
          const held = await page.evaluate(() => document.querySelector('[data-testid="selection-bar"] div')?.scrollTop ?? -1);
          gated('scroll-holds-against-rerender', fired && Math.abs(held - after.top) <= 1, `precondition scrollHeight ${pre.sh} > clientHeight ${pre.ch}; real wheel up to ${after.top} (${belowEnd}px below the end, >= 20); refetch tick (a real re-render that does not reach the pin's trigger) + the scroller's own onContentSizeChange fired directly with unchanged dimensions (standing in for the next real size or height change; fired ${fired}) + 400 ms later the offset is ${held}`);
        }
      }
    }

    // D1 + R9 AUD-F1/F3: Limits is a disclosure toggle whose expanded state rides the DOM
    // (aria-expanded — RNW 0.21.4 does not map accessibilityState) and makes ONE toggle decision per
    // press: tap opens, a SECOND press closes, a third opens; an outside press and Escape dismiss;
    // keyboard Enter and Space toggle from either side without double-firing; a remount starts
    // collapsed. Real presses go through the bar's own scroll (tapBarControl) — locator.click()
    // would re-centre the control with a second scroll, and that scroll dismisses the just-opened
    // help (the provider closes on scroll by design).
    const limits = page.getByTestId('selection-limits');
    if (!(await limits.isVisible())) {
      gated('limits-control', false, 'no Limits control (base renders the limits inline, unbounded)');
    } else {
      const aria = () => page.evaluate(() => document.querySelector('[data-testid="selection-limits"]')?.getAttribute('aria-expanded') ?? null);
      const panelCount = () => page.getByTestId('control-help').count();
      const reveal = async () => { await page.evaluate(() => { const scroller = document.querySelector('[data-testid="selection-bar"]')?.querySelector('div'); if (scroller && scroller.scrollHeight > scroller.clientHeight) scroller.scrollTop = scroller.scrollHeight; }); await page.waitForTimeout(100); };
      gated('limits-aria-rest', (await aria()) === 'false' && (await panelCount()) === 0, `at rest aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await tapBarControl(page, 'selection-limits');
      await page.waitForTimeout(150);
      gated('limits-aria-open', (await aria()) === 'true' && (await panelCount()) === 1, `tap: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await tapBarControl(page, 'selection-limits', false);
      await page.waitForTimeout(150);
      gated('limits-second-press-closes', (await aria()) === 'false' && (await panelCount()) === 0, `second press: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await tapBarControl(page, 'selection-limits');
      await page.waitForTimeout(150);
      gated('limits-third-press-opens', (await aria()) === 'true' && (await panelCount()) === 1, `third press: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      // An outside press on a neutral spot (the selecting header's count text) dismisses.
      const count = await page.getByTestId('selection-count').boundingBox().catch(() => null);
      const spot = count ?? { x: 12, y: 120, width: 40, height: 24 };
      await page.touchscreen.tap(spot.x + spot.width / 2, spot.y + spot.height / 2);
      await page.waitForTimeout(150);
      gated('limits-outside-press-closes', (await aria()) === 'false' && (await panelCount()) === 0, `outside press: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await reveal();
      await tapBarControl(page, 'selection-limits');
      await page.waitForTimeout(150);
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      gated('limits-escape-closes', (await aria()) === 'false' && (await panelCount()) === 0, `Escape: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      // Keyboard Enter and Space toggle from either side with no double-fire: a keyboard activation
      // has no press start (no pointerdown, no onPressIn), so the decision must come from the live
      // state exactly once.
      await reveal();
      await limits.focus();
      await page.keyboard.press('Enter');
      await page.waitForTimeout(150);
      gated('limits-keyboard-opens', (await aria()) === 'true' && (await panelCount()) === 1, `Enter from closed: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(150);
      gated('limits-keyboard-closes', (await aria()) === 'false' && (await panelCount()) === 0, `Enter from open: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await page.keyboard.press(' ');
      await page.waitForTimeout(150);
      gated('limits-space-opens', (await aria()) === 'true' && (await panelCount()) === 1, `Space from closed: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      await page.keyboard.press(' ');
      await page.waitForTimeout(150);
      gated('limits-space-closes', (await aria()) === 'false' && (await panelCount()) === 0, `Space from open: aria-expanded ${await aria()}, panels ${await panelCount()}`);
      // A remount starts collapsed.
      await page.reload();
      await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
      await enterSelection(page, 'agt_lsa_99101');
      await page.waitForTimeout(250);
      await reveal();
      gated('limits-aria-remount', (await aria()) === 'false' && (await panelCount()) === 0, `after remount aria-expanded ${await aria()}, panels ${await panelCount()}`);
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

// R38 V1/D2-r1: wide and iPad are measured and GATED like every other pass — no hard-coded PASS.
// D1 applies at every width: the bar keeps its content (effects line + Limits, and a refused
// action's reason line) fully inside itself, up to the 60% maximum; only the D3 spacing is
// unchanged on wide (root paddingBottom 16, nav untouched).
async function widePass(width, height) {
  const name = `${width}x${height}-light`;
  scaleText = false;
  const context = await browser.newContext({ viewport: { width, height } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?scenario=baseline&theme=light&layout=wide&archiveBusy=1`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    const gated = (label, condition, detail) => {
      report.cases.push({ name: `${name}-gate-${label}`, status: condition ? 'PASS' : 'FAIL', detail });
      console.log(`${name}-gate-${label}: ${condition ? 'PASS' : 'FAIL'} ${detail ?? ''}`);
    };
    await measureSelection(page, `${name}-0-selected`, '0');
    await enterSelection(page, 'agt_lsa_99101');
    const one = await measureSelection(page, `${name}-1-selected`, '1');
    assert.ok(one.geometry.bar, `${name}: the bar renders on wide`);
    assert.equal(one.geometry.disclosure.text, INLINE_LINE, `${name}: the inline line is the effects sentence only (V2 on wide)`);
    // V1: the bar keeps its content — effects line AND the Limits control fully inside the bar box,
    // not hidden inside its scroller (base shrank the bar to its minHeight and clipped both).
    const inside = await page.evaluate(() => {
      const bar = document.querySelector('[data-testid="selection-bar"]')?.getBoundingClientRect();
      const line = document.querySelector('[data-testid="selection-takeover-disclosure"]')?.getBoundingClientRect();
      const limits = document.querySelector('[data-testid="selection-limits"]')?.getBoundingClientRect();
      const inBox = (box) => bar && box && box.top >= bar.top - 1 && box.bottom <= bar.bottom + 1 && box.left >= bar.left - 1 && box.right <= bar.right + 1;
      return { bar, line: line ? [Math.round(line.top), Math.round(line.bottom)] : null, lineInside: inBox(line), limitsInside: inBox(limits) };
    });
    gated('wide-effects-limits-inside', !one.geometry.effectsClipped && inside.lineInside && inside.limitsInside,
      `effectsClipped ${one.geometry.effectsClipped}; line ${JSON.stringify(inside.line)} inside bar ${inside.lineInside}; Limits inside ${inside.limitsInside}`);
    // The deterministic refusal: archiveBusy holds the archive preview, so the bar's Archive control
    // refuses for a real reason and the reason line must be VISIBLE inside the bar (D1 at every width).
    await page.getByRole('button', { name: 'Review archive of 1 selected session', exact: true }).click();
    await page.waitForTimeout(600);
    const refusal = await page.evaluate(() => {
      const barNode = document.querySelector('[data-testid="selection-bar"]');
      const bar = barNode?.getBoundingClientRect();
      const reason = [...(barNode?.querySelectorAll('[dir="auto"]') ?? [])].find(node => node.textContent === 'Archive waits for the host and any running archive.');
      const box = reason?.getBoundingClientRect();
      const inside = bar && box && box.top >= bar.top - 1 && box.bottom <= bar.bottom + 1 && box.height > 0;
      return { present: !!reason, inside, bottom: box ? Math.round(box.bottom) : null, barBottom: bar ? Math.round(bar.bottom) : null };
    });
    gated('wide-reason-visible', refusal.present && refusal.inside, `refusal reason present ${refusal.present}, fully inside the bar ${refusal.inside} (line bottom ${refusal.bottom}, bar bottom ${refusal.barBottom})`);
    // Only the D3 spacing is unchanged on wide: the screen root keeps the wide padding (16, untouched
    // by the diff) and the nav is the same static chrome as at 0-selected (equal-base; absolute base
    // numbers are in runs/base-final).
    const baseChrome = await page.evaluate(() => {
      const bar = document.querySelector('[data-testid="selection-bar"]');
      const screenEl = bar?.parentElement, nav = document.querySelector('[role="tablist"][aria-label="Kontor views"]');
      return { paddingBottom: screenEl ? getComputedStyle(screenEl).paddingBottom : null, navBox: nav?.getBoundingClientRect().toJSON?.() ?? null, navPadding: nav ? getComputedStyle(nav).paddingBottom : null };
    });
    const navTop0 = report.cases.find(result => result.name === `${name}-0-selected`)?.geometry?.nav;
    const navEqual = baseChrome.navBox && navTop0 && Math.abs(baseChrome.navBox.top - navTop0.top) < 0.5 && Math.abs(baseChrome.navBox.height - navTop0.height) < 0.5;
    // Wide has no compact bottom nav: the base-equality contract here is the root padding (16, untouched
    // by the diff) and the nav chrome being identical to the same pass's 0-selected measurement.
    gated('wide-root-nav-base', baseChrome.paddingBottom === '16px' && navEqual, `root paddingBottom ${baseChrome.paddingBottom} (wide base), nav chrome equal to 0-selected ${!!navEqual}`);
    // Let the held preview finish so the fixture state is consistent, then close the review that opens.
    await page.evaluate(() => window.releaseArchive?.());
    await page.waitForTimeout(400);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack ?? String(error) });
    console.log(`${name}: FAIL ${error.message}`);
  } finally {
    await context.close();
  }
}

// R38 V3: with sessions selected in the switch sheet, the Limits popover must appear ABOVE the sheet,
// in BOTH of the sheet's forms (wide popover, compact sheet). Proven by a hit test on the popover
// TEXT: elementFromPoint at the panel's centre must return the panel itself (a z-order read is not
// enough — a transparent overlay would win a z-order check but fail a hit test).
// R38 V3 + F-SHEET: the switch sheet in BOTH forms. At rest (scrollTop 0, no harness scroll) the
// shared bar must show its buttons whole and hit-testable, the effects line and Limits inside the
// bar, and the bar inside the sheet; sheet rows stay visible and tappable at 1/2/5/all. Only then
// the Limits-above-sheet hit test (V3).
async function switchSheetPass(width, height, form, largeText = false, shortSheet = false, keyboardRoot = 0) {
  const compact = width < 600;
  const name = `switch-sheet-${form}-${width}x${height}${largeText ? '-text200' : ''}${keyboardRoot ? `-kbd${keyboardRoot}` : ''}`;
  scaleText = false;
  let gateRecorded = false;
  const context = await browser.newContext({ viewport: { width, height }, ...(compact ? { hasTouch: true, isMobile: true } : {}) });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    const gated = (label, condition, detail) => {
      gateRecorded = true;
      report.cases.push({ name: `${name}-gate-${label}`, status: condition ? 'PASS' : 'FAIL', detail });
      console.log(`${name}-gate-${label}: ${condition ? 'PASS' : 'FAIL'} ${detail ?? ''}`);
    };
    await page.goto(`${origin}/?scenario=baseline&theme=light&layout=${compact ? 'compact' : 'wide'}&sheet=switch${keyboardRoot ? `&inflat=${height - keyboardRoot}` : ''}`);
    await page.getByTestId('sessions-list').waitFor({ timeout: 15000 });
    const hide = page.getByRole('button', { name: 'Hide notices', exact: true });
    if (await hide.isVisible()) await hide.click();
    await page.getByTestId('kontor-switch-heading').waitFor({ timeout: 15000 });
    // The sheet lands on the Recent state; the fixture has no visits, so All lists the sessions. On
    // the narrowest roots (393) the states strip truncates and All folds into the More panel, so the
    // chip click falls back to needs_input (agt_qa_99103 is a needs_input row in the fixture).
    const allChip = page.getByTestId('kontor-switch-state-all');
    const needsInputChip = page.getByTestId('kontor-switch-state-needs_input');
    await allChip.or(needsInputChip).first().waitFor({ timeout: 15000 });
    await ((await allChip.isVisible()) ? allChip : needsInputChip).click();
    await page.waitForTimeout(300);
    // Select a session FROM the sheet: its row menu's Select sessions starts the sheet's selection,
    // which mounts the shared SelectionBar as the sheet's footer. Scoped to the sheet's own list —
    // the Sessions screen behind renders rows with the same action testids.
    await page.getByTestId('kontor-switch-list').getByTestId('session-actions-agt_qa_99103').click();
    const menu = page.getByRole('dialog', { name: 'Session actions', exact: true });
    await menu.getByTestId('session-action-select').click();
    await menu.waitFor({ state: 'detached' });
    await page.getByTestId('selection-bar').waitFor({ timeout: 15000 });
    await page.waitForTimeout(400);
    if (largeText) { await scale(page); await page.waitForTimeout(400); }
    // F-SHEET, AT REST: no harness scroll has happened. The bar's scroll sits at 0 and STILL the
    // buttons row must be whole and hit-testable, with the effects line and Limits inside the bar
    // and the bar inside the sheet.
    const atRest = await page.evaluate(() => {
      const barNode = document.querySelector('[data-testid="selection-bar"]');
      // The sheet root is the AnchoredMenu's own testid — present in both the popover and sheet forms.
      const sheetRoot = document.querySelector('[data-testid="kontor-switch-sheet"]');
      const scroller = barNode?.querySelector('div');
      const bar = barNode?.getBoundingClientRect(), sheet = sheetRoot?.getBoundingClientRect();
      const barBox = barNode?.querySelector('div')?.getBoundingClientRect();
      const whole = (node, box) => { const r = node.getBoundingClientRect(); return r.top >= box.top - 1 && r.bottom <= box.bottom + 1 && r.left >= box.left - 1 && r.right <= box.right + 1; };
      const buttons = ['selection-takeover', 'selection-takeover-auto', 'selection-archive'].map((id) => {
        const node = document.querySelector(`[data-testid="${id}"]`);
        if (!node || !bar) return { id, present: false };
        const r = node.getBoundingClientRect();
        const hitNode = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        const label = node.querySelector('[dir="auto"]');
        const range = document.createRange(); if (label) range.selectNodeContents(label);
        return { id, present: true, whole: whole(node, bar), hit: hitNode === node || node.contains(hitNode), labelWhole: !label || range.getBoundingClientRect().width <= label.getBoundingClientRect().width + 1, box: [Math.round(r.top), Math.round(r.bottom)] };
      });
      const line = document.querySelector('[data-testid="selection-takeover-disclosure"]');
      const limits = document.querySelector('[data-testid="selection-limits"]');
      const scrollTop = scroller?.scrollTop ?? -1;
      const inside = () => ({ lineInside: line && bar ? whole(line, bar) : false, limitsInside: limits && bar ? whole(limits, bar) : false });
      let insideNow = inside();
      // F-SHEET-SHORT: with a definite bound an overflowing bar rests END-PINNED (D2-r3), clipping a
      // few px of the line's top until scrolled. The line and Limits must still be fully visible
      // through the bar's own scroll: verify at the scroller's scrollTop 0, then restore the rest.
      let scrolledToTop = false;
      if (scroller && scroller.scrollHeight > scroller.clientHeight + 1) {
        const saved = scroller.scrollTop;
        scroller.scrollTop = 0; insideNow = inside(); scroller.scrollTop = saved;
        scrolledToTop = true;
      }
      return {
        scrollTop, sh: scroller?.scrollHeight ?? null, ch: scroller?.clientHeight ?? null, scrolledToTop,
        bar: bar ? [Math.round(bar.top), Math.round(bar.bottom)] : null,
        buttons, allWhole: buttons.every(b => b.present && b.whole && b.hit && b.labelWhole),
        lineInside: insideNow.lineInside, limitsInside: insideNow.limitsInside,
        barInsideSheet: bar && sheet ? bar.bottom <= sheet.bottom + 1 && bar.top >= sheet.top - 1 && bar.left >= sheet.left - 1 && bar.right <= sheet.right + 1 : false,
      };
    });
    await shot(page, `${name}-at-rest`);
    // At rest: content that fits rests at scrollTop 0; content that overflows rests end-pinned
    // (D2-r3 rest pin). Both are rest — the buttons must be whole and hit-testable either way.
    const endPinned = atRest.sh !== null && atRest.ch !== null && atRest.scrollTop >= atRest.sh - atRest.ch - 1;
    gated('sheet-buttons-at-rest', atRest.allWhole && (atRest.scrollTop <= 1 || endPinned), `scrollTop ${atRest.scrollTop}/${atRest.sh}/${atRest.ch}${atRest.scrolledToTop ? ' (rest end-pinned, no harness scroll)' : ' (rest, no harness scroll)'}; buttons ${JSON.stringify(atRest.buttons)}`);
    gated('sheet-effects-limits-inside', atRest.lineInside && atRest.limitsInside, `effects line inside ${atRest.lineInside}, Limits inside ${atRest.limitsInside}${atRest.scrolledToTop ? ' (verified via the bar\'s own scrollTop 0; rest position restored)' : ''}`);
    gated('sheet-bar-inside-sheet', atRest.barInsideSheet, `bar ${JSON.stringify(atRest.bar)} inside the sheet root`);
    // F-SHEET-SHORT: on a short compact root the menu frame has only a maxHeight (cover = root.height
    // − 2×margin), so the bar's percentage maxHeight may not resolve — and the buttons are LAST, so a
    // frame clip removes exactly them, with no scroll to reach them. At rest, no harness scroll:
    // Archive must be whole and tappable, Take over and Auto hit-tested, and the bar's bottom inside
    // the frame's visible bottom (the clip edge), not merely inside the viewport.
    const shortProbe = await page.evaluate(() => {
      const frameNode = document.querySelector('[data-testid="kontor-switch-sheet"]');
      const barNode = document.querySelector('[data-testid="selection-bar"]');
      const scroller = barNode?.querySelector('div');
      const box = (n) => { const r = n.getBoundingClientRect(); return [Math.round(r.top), Math.round(r.bottom), Math.round(r.left), Math.round(r.right)]; };
      const buttons = ['selection-takeover', 'selection-takeover-auto', 'selection-archive'].map((id) => {
        const node = document.querySelector(`[data-testid="${id}"]`);
        if (!node || !barNode) return { id, present: false };
        const r = node.getBoundingClientRect();
        const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        const label = node.querySelector('[dir="auto"]');
        const range = document.createRange(); if (label) range.selectNodeContents(label);
        return { id, present: true, box: [Math.round(r.top), Math.round(r.bottom)], tappable: (hit === node || node.contains(hit)) && r.height > 0, labelWhole: !label || range.getBoundingClientRect().width <= label.getBoundingClientRect().width + 1 };
      });
      return {
        frame: frameNode ? box(frameNode) : null,
        frameMaxHeight: frameNode ? getComputedStyle(frameNode).maxHeight : null,
        bar: barNode ? box(barNode) : null,
        scroller: scroller ? { scrollTop: Math.round(scroller.scrollTop), scrollHeight: scroller.scrollHeight, clientHeight: scroller.clientHeight } : null,
        buttons,
      };
    });
    const archiveRow = shortProbe.buttons[2], takeoverRow = shortProbe.buttons[0], autoRow = shortProbe.buttons[1];
    const barInFrame = shortProbe.bar && shortProbe.frame && shortProbe.bar[1] <= shortProbe.frame[1] + 1;
    gateRecorded = true;
    gated('sheet-short-bar-unclipped', shortProbe.frame && archiveRow.present && archiveRow.tappable && archiveRow.labelWhole && takeoverRow.tappable && autoRow.tappable && barInFrame,
      `menu frame ${JSON.stringify(shortProbe.frame)} maxHeight ${shortProbe.frameMaxHeight}; bar ${JSON.stringify(shortProbe.bar)} scroller ${JSON.stringify(shortProbe.scroller)}; buttons ${JSON.stringify(shortProbe.buttons)}; archive whole+tappable ${archiveRow.tappable && archiveRow.labelWhole}, takeover/auto hit ${takeoverRow.tappable}/${autoRow.tappable}, bar bottom <= frame clip edge ${barInFrame}`);
    // R9 AUD-F1/F3 in the sheet forms: the same DOM expanded state and the one-decision toggle where
    // the Limits control renders (the bar's own scroll reaches it on short frames too).
    const sheetLimits = page.getByTestId('selection-limits');
    if (await sheetLimits.isVisible().catch(() => false)) {
      const sheetAria = () => page.evaluate(() => document.querySelector('[data-testid="selection-limits"]')?.getAttribute('aria-expanded') ?? null);
      const sheetPanels = () => page.getByTestId('control-help').count();
      gated('sheet-limits-aria-rest', (await sheetAria()) === 'false' && (await sheetPanels()) === 0, `at rest aria-expanded ${await sheetAria()}, panels ${await sheetPanels()}`);
      await tapBarControl(page, 'selection-limits');
      await page.waitForTimeout(150);
      gated('sheet-limits-aria-open', (await sheetAria()) === 'true' && (await sheetPanels()) === 1, `tap: aria-expanded ${await sheetAria()}, panels ${await sheetPanels()}`);
      await tapBarControl(page, 'selection-limits', false);
      await page.waitForTimeout(150);
      gated('sheet-limits-second-press-closes', (await sheetAria()) === 'false' && (await sheetPanels()) === 0, `second press: aria-expanded ${await sheetAria()}, panels ${await sheetPanels()}`);
    } else {
      gated('sheet-limits-aria-rest', false, 'no Limits control in the sheet form');
    }
    // Sheet rows stay visible and tappable while the bar is up: 2, 5, then all; a visible row toggles
    // at each stage (the sheet's own list scrolls rows into view). The short-root sheet passes claim
    // only the F-SHEET-SHORT row above; their frame leaves no room for the 2/5/all stages.
    const sheetCount = () => page.getByTestId('selection-count').innerText();
    const toggleVisible = async (stage) => {
      const body = page.getByTestId('kontor-switch-list').getByTestId('session-row-agt_revb_pr812').getByTestId('session-row-body');
      await body.scrollIntoViewIfNeeded().catch(() => undefined);
      const before = await sheetCount();
      await body.click();
      await page.waitForTimeout(150);
      const after = await sheetCount();
      const toggled = before !== after;
      gated(`sheet-rows-toggle-${stage}`, toggled, `${before} -> ${after}`);
      if (toggled) await body.click();
      await page.waitForTimeout(120);
    };
    if (!shortSheet) {
      await page.getByTestId('kontor-switch-list').getByTestId('session-row-agt_tpm_99101').getByTestId('session-row-body').click();
      await page.waitForTimeout(150);
      gated('sheet-2-selected', (await sheetCount()).startsWith('2 /'), `count ${await sheetCount()}`);
      await toggleVisible('2');
      for (const id of ['agt_aud_99103', 'agt_qa_99202', 'agt_swe_99104_a']) await page.getByTestId('kontor-switch-list').getByTestId(`session-row-${id}`).getByTestId('session-row-body').click();
      await page.waitForTimeout(150);
      gated('sheet-5-selected', (await sheetCount()).startsWith('5 /'), `count ${await sheetCount()}`);
      await toggleVisible('5');
      await page.getByTestId('selection-all').click();
      await page.waitForTimeout(150);
      gated('sheet-all-selected', !(await sheetCount()).startsWith('5 /'), `count ${await sheetCount()}`);
      await toggleVisible('all');
      // Restore a 1-selected bar for the Limits pass: clear (the checked select-all), then toggle one
      // row body back on — the sheet is still in selecting mode, where rows have no menus, only bodies.
      await page.getByTestId('selection-all').click();
      await page.waitForTimeout(150);
      const restoreRow = page.getByTestId('kontor-switch-list').getByTestId('session-row-agt_qa_99103').getByTestId('session-row-body');
      await restoreRow.scrollIntoViewIfNeeded().catch(() => undefined);
      await restoreRow.click();
      await page.waitForTimeout(400);
    }
    // Reach the Limits control the way an operator does — the bar's own scroll, just far enough —
    // then press it with a real pointer press (tapBarControl; locator.click()'s re-centring scroll
    // would dismiss the just-opened help).
    await tapBarControl(page, 'selection-limits');
    await page.getByTestId('control-help').waitFor({ timeout: 15000 });
    await page.waitForTimeout(150);
    // Hit test on the popover TEXT: the panel's centre must resolve to the panel (or its content).
    const hit = await page.evaluate(() => {
      const panel = document.querySelector('[data-testid="control-help"]');
      const box = panel.getBoundingClientRect();
      const centre = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      const onTop = centre === panel || panel.contains(centre);
      const viewportFit = box.left >= -1 && box.right <= innerWidth + 1 && box.top >= -1 && box.bottom <= innerHeight + 1;
      return { onTop, viewportFit, centre: centre ? `${centre.tagName}${centre.closest('[data-testid]')?.getAttribute('data-testid') ?? ''}` : null, box: [Math.round(box.left), Math.round(box.top), Math.round(box.right), Math.round(box.bottom)] };
    });
    await shot(page, `${name}-limits-open`);
    gated('popover-on-top', hit.onTop && hit.viewportFit, `elementFromPoint at panel centre -> ${hit.centre}; panel box ${JSON.stringify(hit.box)} inside viewport ${hit.viewportFit}`);
    // R9 AUD-F3 regression: hover and focus help on ANOTHER control is unchanged by the trigger
    // exception — only the anchor that HOLDS the popover keeps its state through its own press; a
    // press or hover elsewhere still dismisses and re-shows as before. The popover form is a mouse
    // context, so hover help fires; Escape resets the input mode so focus help fires.
    if (!compact) {
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      const chip = page.getByTestId('kontor-switch-state-recent');
      await chip.hover();
      await page.waitForTimeout(200);
      const hoverPanels = await page.getByTestId('control-help').count();
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      await chip.focus();
      await page.waitForTimeout(200);
      const focusPanels = await page.getByTestId('control-help').count();
      await page.keyboard.press('Escape');
      await page.waitForTimeout(150);
      const closedPanels = await page.getByTestId('control-help').count();
      gated('limits-help-others-unchanged', hoverPanels === 1 && focusPanels === 1 && closedPanels === 0, `hover help on the Recent state chip: panels ${hoverPanels}; focus help after keyboard use: panels ${focusPanels}; Escape closes: panels ${closedPanels}`);
    }
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack ?? String(error) });
    console.log(`${name}: FAIL ${error.message}`);
    // F-SHEET-SHORT: if the sheet never got far enough to record its gate, the row is NOT RUN with
    // the reason — never passed, and never silently missing.
    if (!gateRecorded) {
      report.cases.push({ name: `${name}-gate-sheet-short-bar-unclipped`, status: 'NOT RUN', detail: `the sheet did not open or complete at this root: ${error.message.split('\n')[0]}` });
      console.log(`${name}-gate-sheet-short-bar-unclipped: NOT RUN (sheet did not open or complete)`);
    }
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
    // R38 F3/D2-r2: short roots — the list floor and the bar both yield to the root; the buttons stay
    // reachable through the bar's own scroll (0/1 only).
    for (const [width, height] of [[430, 330], [393, 420]]) {
      await pass(width, height, 'light', false, { short: true });
      await pass(width, height, 'light', true, { short: true });
    }
    // R38 V1/D2-r1: wide and iPad — the bar keeps its content, gated, at 1280x800 and 820x1180.
    await widePass(1280, 800);
    await widePass(820, 1180);
    // R38 V3 + F-SHEET: the Limits popover above the switch sheet, in BOTH forms, hit-tested on the
    // panel text — and the at-rest button gates in both forms, the compact sheet also at text200.
    await switchSheetPass(1280, 800, 'popover');
    await switchSheetPass(430, 932, 'sheet');
    await switchSheetPass(430, 932, 'sheet', true);
    // F-SHEET-SHORT: the compact sheet on short roots — the menu frame's percentage question.
    await switchSheetPass(430, 330, 'sheet', false, true);
    await switchSheetPass(430, 330, 'sheet', true, true);
    await switchSheetPass(393, 420, 'sheet', false, true);
    await switchSheetPass(393, 420, 'sheet', true, true);
    // F-SHEET-SHORT / R6-F1: keyboard-shortened ROOTS — the window stays tall, only the screen root
    // is shortened by the simulated inset (bottom padding standing in for the keyboard). The bound
    // must derive from the measured root, so these pass only when it does.
    await switchSheetPass(430, 932, 'sheet', false, true, 330);
    await switchSheetPass(430, 932, 'sheet', true, true, 330);
    await switchSheetPass(393, 852, 'sheet', false, true, 420);
    await switchSheetPass(393, 852, 'sheet', true, true, 420);
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  }
  const failed = report.cases.filter(result => result.status === 'FAIL');
  console.log(`\ncases: ${report.cases.length}, FAIL: ${failed.length}`);
  for (const result of failed) console.log(`FAIL ${result.name}: ${result.error ?? result.detail ?? ''}`);
  if (failed.length) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
