// ASMA-8851 desktop reproduction and measurement of D1-D5 over the kontor-prototype preview, which
// renders the real plugin (index.client.tsx) in the prototype host shell. Supply PREVIEW_ORIGIN (a
// served `vite build` of apps/kontor-prototype from the tree under test), PLAYWRIGHT_MODULE and
// EVIDENCE_DIR; RUN_LABEL names the run and its screenshots (<Jira attachment>-<label>.png, for the
// report's 66841..66846). Every request other than a GET to the origin is aborted.
// ASMA8851_ASSERT=1 fails the run unless every item meets its acceptance; without it the run only
// records (a base tree). Desktop only, 1280x800 at DPR 1, fine pointer. "200%" is the R38 CSS text
// doubling; no native-device, screen-reader or Dynamic Type claim.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR, label = process.env.RUN_LABEL || 'run';
assert.ok(origin && output, 'Supply the preview origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const HOVER_DELAY_MS = 650;
// D1 moves the displayed-state paragraph from group help to the Legend; read it from the client source.
const DISPLAYED_STATE_HELP = fs.readFileSync(path.join(__dirname, '../client/organization-row.tsx'), 'utf8').match(/DISPLAYED_STATE_HELP = "([^"]+)"/)?.[1];
assert.ok(DISPLAYED_STATE_HELP, 'DISPLAYED_STATE_HELP extracted from the client source');
const report = { label, viewport: '1280x800 dpr1 pointer=fine theme=dark', items: {} };
let browser;

const shot = (page, name) => page.screenshot({ path: path.join(output, `${name}-${label}.png`) });
const rect = (node) => { const box = node?.getBoundingClientRect(); return box ? { x: Math.round(box.x * 10) / 10, y: Math.round(box.y * 10) / 10, w: Math.round(box.width * 10) / 10, h: Math.round(box.height * 10) / 10 } : null; };

async function open(context, scenario, ready) {
  const page = await context.newPage();
  await page.goto(`${origin}/?embed=1&pointer=fine&theme=dark&scenario=${scenario}`);
  await page.getByTestId(ready).first().waitFor();
  await page.waitForTimeout(500);
  return page;
}

// Same doubling as r38-selection-bar.browser.cjs, applied once to every text node.
const scale = (page) => page.evaluate(() => {
  for (const text of document.querySelectorAll('[dir="auto"]:not([data-scaled])')) {
    const style = getComputedStyle(text);
    text.style.fontSize = `${parseFloat(style.fontSize) * 2}px`;
    if (style.lineHeight !== 'normal') text.style.lineHeight = `${parseFloat(style.lineHeight) * 2}px`;
    text.setAttribute('data-scaled', '');
  }
});

// The drawn ink's vertical extent inside a clip, from the rendered pixels: a font arrow's line box
// is not where its glyph is drawn, so D4 compares ink, not boxes.
async function ink(page, clip) {
  const png = await page.screenshot({ clip });
  return page.evaluate(async ({ data, top }) => {
    const image = new Image(); image.src = `data:image/png;base64,${data}`; await image.decode();
    const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height;
    const context = canvas.getContext('2d'); context.drawImage(image, 0, 0);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
    const at = (x, y) => pixels.subarray((y * canvas.width + x) * 4, (y * canvas.width + x) * 4 + 3);
    const background = at(0, 0);
    let first = Infinity, last = -Infinity;
    for (let y = 0; y < canvas.height; y += 1) for (let x = 0; x < canvas.width; x += 1) {
      const pixel = at(x, y);
      if (Math.abs(pixel[0] - background[0]) + Math.abs(pixel[1] - background[1]) + Math.abs(pixel[2] - background[2]) > 60) { first = Math.min(first, y); last = Math.max(last, y); }
    }
    return first === Infinity ? null : { top: top + first, bottom: top + last + 1, centre: top + (first + last + 1) / 2 };
  }, { data: png.toString('base64'), top: clip.y });
}

// D1: the tree's group rows, a fast sweep over them, then a dwell on one row with neighbours.
async function d1(context) {
  const page = await open(context, 'baseline', 'sessions-filter-all');
  await page.getByTestId('sessions-filter-all').click();
  await page.getByTestId('organization-state-counts').first().waitFor();
  await page.waitForTimeout(400);
  await page.evaluate(() => {
    window.helpLog = [];
    let open = false;
    new MutationObserver(() => {
      const panel = document.querySelector('[data-testid="control-help"]');
      if (!!panel !== open) { open = !!panel; window.helpLog.push({ t: performance.now(), open, text: panel?.textContent.slice(0, 80) ?? null }); }
    }).observe(document.body, { childList: true, subtree: true });
  });
  const rows = await page.evaluate(() => {
    const list = document.querySelector('[data-testid="sessions-list"]').getBoundingClientRect();
    const seen = new Set();
    return [...document.querySelectorAll('[data-testid="organization-state-counts"]')].map((counts) => counts.closest('[aria-expanded]'))
      .filter((row) => row && !seen.has(row) && seen.add(row)).map((row) => row.getBoundingClientRect())
      .filter((box) => box.top >= list.top && box.bottom <= list.bottom).sort((a, b) => a.top - b.top)
      .map((box) => ({ x: box.x, y: box.y, w: box.width, h: box.height }));
  });
  assert.ok(rows.length >= 4, `at least four visible group rows (${rows.length})`);
  const park = await page.getByTestId('sessions-heading').boundingBox();
  const parkAt = { x: park.x + 4, y: park.y + park.height / 2 };
  const centre = (row) => ({ x: row.x + row.w / 2, y: row.y + row.h / 2 });
  await page.mouse.move(parkAt.x, parkAt.y);
  await page.waitForTimeout(300);
  // Sweep: 100 ms on each row, top to bottom, then straight off the list.
  const sweepStart = await page.evaluate(() => performance.now());
  for (const row of rows) { const point = centre(row); await page.mouse.move(point.x, point.y, { steps: 3 }); await page.waitForTimeout(100); }
  await page.mouse.move(parkAt.x, parkAt.y, { steps: 3 });
  await page.waitForTimeout(HOVER_DELAY_MS + 350);
  const sweep = await page.evaluate((start) => window.helpLog.filter((entry) => entry.t >= start), sweepStart);
  const parked = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest('[data-testid]')?.dataset.testid ?? null, parkAt);
  // Dwell: the third row, which has a row above and below it.
  const index = 2, point = centre(rows[index]);
  const dwellStart = await page.evaluate(() => performance.now());
  await page.mouse.move(point.x, point.y);
  await page.waitForTimeout(HOVER_DELAY_MS + 550);
  const dwell = await page.evaluate(({ start, point, neighbours }) => {
    const panels = [...document.querySelectorAll('[data-testid="control-help"]')];
    const panel = panels[0], layer = document.querySelector('[data-testid="control-help-layer"]');
    const box = panel?.getBoundingClientRect();
    const opened = window.helpLog.find((entry) => entry.t >= start && entry.open);
    const text = panel ? [...panel.querySelectorAll('[dir="auto"]')].pop() : null;
    const lineHeight = text ? parseFloat(getComputedStyle(text).lineHeight) : NaN;
    const overlaps = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    const frame = box ? { x: box.x, y: box.y, w: box.width, h: box.height } : null;
    const hit = box ? document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2) : null;
    return {
      panels: panels.length, delayMs: opened ? Math.round(opened.t - start) : null, text: panel?.textContent ?? null, frame,
      lines: text ? Math.round(text.getBoundingClientRect().height / lineHeight) : null,
      layerPointerEvents: layer ? getComputedStyle(layer).pointerEvents : null, panelPointerEvents: panel ? getComputedStyle(panel).pointerEvents : null,
      hitTestPassesThrough: hit ? !hit.closest('[data-testid="control-help"]') : null,
      underPointer: frame ? point.x >= frame.x && point.x <= frame.x + frame.w && point.y >= frame.y && point.y <= frame.y + frame.h : null,
      overlapsRowsAboveSelfBelow: frame ? neighbours.map((row) => overlaps(frame, row)) : null,
    };
  }, { start: dwellStart, point, neighbours: rows.slice(index - 1, index + 2) });
  await shot(page, '66844');
  // The paragraph group help leaves out is in the Legend (under Filters & view), with the denominator text.
  await page.mouse.move(parkAt.x, parkAt.y);
  await page.getByTestId('sessions-advanced-toggle').click();
  await page.getByRole('button', { name: 'Legend', exact: true }).click();
  const legendText = await page.getByTestId('sessions-legend').innerText();
  const legend = { displayedStateHelp: legendText.includes(DISPLAYED_STATE_HELP), denominator: legendText.includes('The denominator counts'), helpLeavesDenominator: !dwell.text?.includes('denominator') };
  await page.close();
  const result = {
    rows: rows.length, rowFrames: rows.slice(index - 1, index + 2), sweep: { perRowMs: 100, openings: sweep.filter((entry) => entry.open).length, events: sweep, parkedOn: parked }, dwell, legend,
  };
  result.pass = result.sweep.openings === 0 && dwell.panels === 1 && dwell.delayMs >= HOVER_DELAY_MS && dwell.lines <= 2
    && dwell.overlapsRowsAboveSelfBelow?.every((overlap) => !overlap) && dwell.underPointer === false && dwell.hitTestPassesThrough === true
    && legend.displayedStateHelp && legend.denominator && legend.helpLeavesDenominator;
  return result;
}

// D2/D3: the composer pill's Start or switch popover in the prototype's agent shim (the Sideswipe popover).
async function popover(context) {
  const page = await open(context, 'unified-switcher-synthetic', 'kontor-switch-state-all');
  await page.getByTestId('kontor-switch-state-all').click();
  await page.getByTestId('kontor-switch-list').getByTestId('session-row-body').first().click();
  await page.getByTestId('host-pill-return-to-kontor').click();
  await page.getByTestId('host-popover').getByTestId('kontor-switch-states').waitFor();
  await page.waitForTimeout(500);
  return page;
}
const popoverState = (page) => page.evaluate(() => {
  const box = (node) => { const b = node?.getBoundingClientRect(); return b ? { x: Math.round(b.x), y: Math.round(b.y), w: Math.round(b.width), h: Math.round(b.height) } : null; };
  const pop = document.querySelector('[data-testid="host-popover"]');
  const filter = pop?.querySelector('[data-testid="kontor-switch-more"]');
  const tiles = [...(pop?.querySelectorAll('[data-testid^="kontor-switch-state-"]') ?? [])];
  const focus = document.activeElement;
  return {
    popover: box(pop), strip: box(pop?.querySelector('[data-testid="kontor-switch-states"]')),
    tiles: tiles.map((tile) => tile.dataset.testid.slice('kontor-switch-state-'.length)),
    pressed: tiles.filter((tile) => tile.getAttribute('aria-pressed') === 'true').map((tile) => tile.dataset.testid.slice('kontor-switch-state-'.length)),
    filter: filter ? { text: filter.textContent, label: filter.getAttribute('aria-label'), pressed: filter.getAttribute('aria-pressed'), expanded: filter.getAttribute('aria-expanded'), border: getComputedStyle(filter).borderTopColor } : null,
    gridOpen: !!pop?.querySelector('[data-testid^="kontor-switch-more-"]'),
    focus: focus?.dataset?.testid ?? focus?.tagName ?? null,
    moreText: [...(pop?.querySelectorAll('[dir="auto"]') ?? [])].some((text) => /^More\b/.test(text.textContent.trim())),
  };
});
// The popover is shrink-to-fit (280..420): its width also follows the chosen state's list, so each pick is
// compared with the compact popover (search unfocused, as after any pick) and, for a state whose list is wider,
// with the same state picked from the compact strip without opening the grid. Hiding the list for two frames
// (restored at once) shows the width the strip alone holds.
const listHidden = (page) => page.evaluate(async () => {
  const pop = document.querySelector('[data-testid="host-popover"]'), list = pop?.querySelector('[data-testid="kontor-switch-list"]');
  if (!list) return null;
  list.style.display = 'none';
  await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const width = Math.round(pop.getBoundingClientRect().width);
  list.style.display = '';
  return width;
});
async function pick(context, testID, viaGrid, name) {
  const page = await popover(context);
  if (viaGrid) { await page.getByTestId('host-popover').getByTestId('kontor-switch-more').click(); await page.waitForTimeout(500); }
  await page.getByTestId('host-popover').getByTestId(testID).click(); await page.waitForTimeout(800);
  const state = await popoverState(page);
  if (name) await shot(page, name);
  state.popoverPxWithListHidden = await listHidden(page);
  await page.close();
  return state;
}
async function d2d3(context) {
  const cases = {};
  const page = await popover(context);
  cases.compact = await popoverState(page); await shot(page, '66841');
  await page.evaluate(() => document.activeElement?.blur()); await page.waitForTimeout(300);
  cases['compact, search unfocused'] = await popoverState(page);
  await page.getByTestId('host-popover').getByTestId('kontor-switch-more').click(); await page.waitForTimeout(500);
  cases.grid = await popoverState(page); await shot(page, '66842');
  await page.close();
  cases['in-set: Last visited from the grid'] = await pick(context, 'kontor-switch-more-recent', true, '66843');
  cases['out-of-set: Favorites from the grid'] = await pick(context, 'kontor-switch-more-favorites', true, '66843-out-of-set-favorites');
  cases['in-set: Needs input from the grid'] = await pick(context, 'kontor-switch-more-needs_input', true, '66843-in-set-needs-input');
  cases['control: Needs input from the compact strip, no grid'] = await pick(context, 'kontor-switch-state-needs_input', false);
  const compact = cases['compact, search unfocused'];
  const d2 = { compactFilter: cases.compact.filter, gridFilter: cases.grid.filter, anyMoreLabel: Object.values(cases).some((state) => state.moreText) };
  d2.pass = cases.compact.filter?.text.startsWith('Filter') && !d2.anyMoreLabel;
  const px = (state) => ({ popover: state.popover.w, strip: state.strip.w, tiles: state.tiles.length, popoverWithListHidden: state.popoverPxWithListHidden ?? null });
  const d3 = { compactPx: px(cases.compact), compactUnfocusedPx: px(compact), gridPx: px(cases.grid), picks: {} };
  const control = cases['control: Needs input from the compact strip, no grid'];
  for (const [name, state, picked, reference] of [
    ['in-set: Last visited from the grid', cases['in-set: Last visited from the grid'], 'recent', compact],
    ['out-of-set: Favorites from the grid', cases['out-of-set: Favorites from the grid'], 'favorites', compact],
    ['in-set: Needs input from the grid', cases['in-set: Needs input from the grid'], 'needs_input', control],
  ]) {
    d3.picks[name] = { ...px(state), reference: px(reference), lastTile: state.tiles.at(-1), gridOpen: state.gridOpen, focus: state.focus, pressed: state.pressed, filter: state.filter };
    d3.picks[name].pass = !state.gridOpen && state.popover.w === reference.popover.w && state.strip.w === reference.strip.w && state.tiles.length === reference.tiles.length && state.tiles.slice(0, -1).join() === reference.tiles.slice(0, -1).join()
      && state.focus === `kontor-switch-state-${picked}` && state.pressed.join() === picked && state.filter?.pressed !== 'true' && state.filter?.expanded !== 'true';
  }
  d3.picks['out-of-set: Favorites from the grid'].pass &&= d3.picks['out-of-set: Favorites from the grid'].lastTile === 'favorites';
  d3.control = px(control);
  d3.pass = d3.gridPx.strip > d3.compactPx.strip && Object.values(d3.picks).every((entry) => entry.pass);
  return { D2: d2, D3: d3 };
}

// D4: the top-nav More and the Sessions Filters chevrons against their labels, at 100% and 200% text.
async function chevrons(page) {
  const found = await page.evaluate(() => [['More', '[aria-label^="More Kontor views"]'], ['Filters', '[data-testid="sessions-advanced-toggle"]']].map(([name, selector]) => {
    const button = document.querySelector(selector), box = (node) => { const b = node.getBoundingClientRect(); return { x: b.x, y: b.y, w: b.width, h: b.height }; };
    const text = [...button.querySelectorAll('[dir="auto"]')].find((node) => node.textContent.trim().startsWith(name));
    return { name, button: box(button), label: box(text), chevron: box(button.lastElementChild), chevronIsSvg: !!button.lastElementChild.querySelector('svg') };
  }));
  const out = {};
  for (const control of found) {
    const band = { y: Math.floor(control.button.y) + 2, height: Math.floor(control.button.h) - 4 };
    const chevronInk = await ink(page, { x: Math.floor(control.chevron.x) - 1, width: Math.ceil(control.chevron.w) + 2, ...band });
    const labelInk = await ink(page, { x: Math.floor(control.label.x), width: Math.ceil(control.label.w), ...band });
    const labelCentre = control.label.y + control.label.h / 2;
    out[control.name] = { chevronIsSvg: control.chevronIsSvg, labelBoxCentre: Math.round(labelCentre * 10) / 10, labelInk, chevronInk, deltaPx: chevronInk ? Math.round((chevronInk.centre - labelCentre) * 10) / 10 : null };
  }
  return out;
}
async function d4d5(context) {
  const page = await open(context, 'baseline', 'sessions-advanced-toggle');
  const d4 = { text100: await chevrons(page) };
  const choose = async () => {
    await page.getByText('Choose plan', { exact: true }).first().waitFor();
    return page.evaluate(() => {
      const text = [...document.querySelectorAll('[dir="auto"]')].find((node) => node.textContent.trim() === 'Choose plan');
      const button = text.closest('[role="button"]'), style = getComputedStyle(button), textStyle = getComputedStyle(text), box = button.getBoundingClientRect();
      // react-native-web writes these styles inline, so the style attributes name the variant.
      return { buttonStyle: button.getAttribute('style'), labelStyle: text.getAttribute('style'),
        visual: { border: `${style.borderTopWidth} ${style.borderTopStyle} ${style.borderTopColor}`, radius: style.borderTopLeftRadius, background: style.backgroundColor, padding: `${style.paddingTop} ${style.paddingRight}`, height: Math.round(box.height), font: `${textStyle.fontSize} ${textStyle.fontWeight} ${textStyle.color}` } };
    });
  };
  await page.getByTestId('kontor-views-group').getByText('Plan', { exact: true }).click(); await page.waitForTimeout(500);
  const plan = await choose(); await shot(page, '66845');
  await page.getByTestId('kontor-views-group').getByText('Board', { exact: true }).click(); await page.waitForTimeout(500);
  const board = await choose(); await shot(page, '66846');
  await page.getByTestId('kontor-views-group').getByText('Sessions', { exact: true }).click(); await page.waitForTimeout(500);
  await scale(page); await page.waitForTimeout(400);
  d4.text200 = await chevrons(page); await shot(page, '66845-d4-text200');
  await page.close();
  d4.pass = Object.values(d4).every((size) => Object.values(size).every((control) => control.deltaPx !== null && Math.abs(control.deltaPx) <= 1));
  const d5 = { plan, board, sameComponentAndVariant: plan.buttonStyle === board.buttonStyle && plan.labelStyle === board.labelStyle && JSON.stringify(plan.visual) === JSON.stringify(board.visual) };
  d5.pass = d5.sameComponentAndVariant && parseFloat(plan.visual.border) > 0 && parseFloat(board.visual.border) > 0;
  return { D4: d4, D5: d5 };
}

(async () => {
  browser = await chromium.launch({ headless: true });
  report.browser = browser.version();
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 });
  await context.route('**/*', (route) => route.request().url().startsWith(origin + '/') && route.request().method() === 'GET' ? route.continue() : route.abort());
  try {
    report.items.D1 = await d1(context);
    Object.assign(report.items, await d2d3(context));
    Object.assign(report.items, await d4d5(context));
  } finally { await browser.close(); }
  fs.writeFileSync(path.join(output, `report-${label}.json`), JSON.stringify(report, null, 2) + '\n');
  for (const [item, result] of Object.entries(report.items)) console.log(`${item} ${result.pass ? 'PASS' : 'FAIL'}`);
  if (process.env.ASMA8851_ASSERT === '1') for (const [item, result] of Object.entries(report.items)) assert.ok(result.pass, `${item} meets its acceptance`);
})().catch((error) => { console.error(error); process.exitCode = 1; }).finally(() => browser?.close());
