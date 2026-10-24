// ASMA-8644: live updates never move the Sessions list offset. Synthetic SessionsScreen on react-native-web, no native-device claim.
// Needs the kontor-prototype dev server (`vite`, not `preview`): live updates go through its fixture runtime module.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR. Each update targets the row at the top of the viewport,
// the row the browser's scroll anchoring followed back up the list; host offline/online cycles re-read the directory.
// Each cycle also records the drift: how far the other rows that were in view moved on screen.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply an isolated synthetic fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };

const list = page => page.getByTestId('sessions-list');
const metrics = page => page.evaluate(() => {
  const node = document.querySelector('[data-testid="sessions-list"]');
  const box = node.getBoundingClientRect();
  const rows = [...node.querySelectorAll('[data-testid^="sessions-row-agt_"]')].map(row => ({ id: row.dataset.testid.slice('sessions-row-'.length), rect: row.getBoundingClientRect() }));
  const visible = rows.filter(row => row.rect.bottom > box.top + 1 && row.rect.top < box.bottom - 1).sort((a, b) => a.rect.top - b.rect.top);
  const last = rows.sort((a, b) => a.rect.top - b.rect.top).at(-1);
  return { offset: Math.round(node.scrollTop), max: node.scrollHeight - node.clientHeight, viewport: node.clientHeight, first: visible[0]?.id ?? null,
    last: last ? { id: last.id, visible: last.rect.bottom <= box.bottom + 1 && last.rect.top >= box.top - 1 } : null,
    inView: visible.map(row => row.id), at: Object.fromEntries(rows.map(row => [row.id, Math.round(row.rect.top - box.top)])) };
});
// The smallest scroll range seen in any frame: a list that shrinks for a moment clamps the offset for good.
const watch = page => page.evaluate(() => {
  const node = document.querySelector('[data-testid="sessions-list"]');
  const seen = window.__scrollWatch = { min: Infinity, stop: false };
  const tick = () => { seen.min = Math.min(seen.min, node.scrollHeight - node.clientHeight); if (!seen.stop) requestAnimationFrame(tick); };
  tick();
});
const watched = page => page.evaluate(() => { window.__scrollWatch.stop = true; return window.__scrollWatch.min; });
// One simulated native change through the fixture host's stream; the moved row jumps to the top of its state bucket.
const update = (page, id) => page.evaluate(async id => {
  const runtime = await import('/src/host/runtime.ts');
  runtime.simulateUpdate(id, { status: 'running' }, 'ASMA-8644 scroll proof');
}, id);
const refreshCycle = page => page.evaluate(async () => {
  const runtime = await import('/src/host/runtime.ts');
  runtime.setHostStatus('offline');
  await new Promise(resolve => setTimeout(resolve, 250));
  runtime.setHostStatus('online');
  await new Promise(resolve => setTimeout(resolve, 400));
});
const settle = page => page.waitForTimeout(260);
/** Back re-places the results by estimated row heights before they are measured again. */
const BACK_TOLERANCE = 24;
async function wheel(page, delta) {
  const box = await list(page).boundingBox();
  // The left edge: a pointer resting on a row's info button opens its help over the list.
  await page.mouse.move(box.x + 12, box.y + box.height / 2);
  await page.mouse.wheel(0, delta);
  await settle(page);
}
async function chooseState(page, compact, status) {
  const quick = page.getByTestId(compact ? `sessions-quick-${status}` : `sessions-filter-${status}`);
  // A narrow quick toolbar keeps some states behind its chooser.
  if (await quick.count()) await quick.click();
  else { await page.getByTestId('sessions-mobile-quick-chooser-trigger').click(); await page.getByTestId(`sessions-choose-${status}`).click(); }
  await settle(page);
}
async function onFilters(page, compact, act) {
  if (compact) {
    await page.getByTestId('sessions-mobile-filters-trigger').click();
    await act();
    await page.getByRole('button', { name: 'Back to sessions, keep filter choices', exact: true }).click();
  } else {
    if (!await page.getByTestId('sessions-expand-all').count()) await page.getByTestId('sessions-advanced-toggle').click();
    await act();
  }
  await settle(page);
}

/**
 * An update moves rows, never the offset; only a list that got shorter, even for one frame, may clamp it.
 * Drift is recorded, not asserted: re-estimated heights of rows outside the rendered window can move the
 * rows in view while the offset holds.
 */
async function cycle(page, trace, phase, endId, refresh) {
  const before = await metrics(page);
  const target = before.first && before.first !== endId ? before.first : null;
  await watch(page);
  if (target) await update(page, target);
  if (refresh) await refreshCycle(page);
  await settle(page);
  const shortest = await watched(page);
  const after = await metrics(page);
  const clamped = after.offset < before.offset && after.offset >= Math.min(shortest, after.max) - 1;
  const moved = after.offset !== before.offset && !clamped;
  const shifts = before.inView.filter(id => id !== target && id in after.at).map(id => after.at[id] - before.at[id]).sort((a, b) => a - b);
  const drift = shifts.length ? shifts[Math.floor(shifts.length / 2)] : null;
  trace.push({ phase, target, refresh, before: before.offset, after: after.offset, max: after.max, shortest, clamped, moved, drift });
  assert.ok(!moved, `${phase}: update moved the list from ${before.offset} to ${after.offset} (max ${after.max})`);
  return after;
}

/**
 * Deliberate navigation keeps its own placement: Back from a peek returns to the results offset and a
 * layout change opens Grouped at its remembered tree offset (0 here). A quick filter change scrolls
 * nothing itself; where it leaves the list is recorded, not asserted.
 */
async function deliberate(page, compact, name) {
  await wheel(page, 1400);
  const middle = await metrics(page);
  assert.ok(middle.offset > 600, `${name}: control starts mid-list`);
  const title = await page.getByTestId(`sessions-row-${middle.first}`).getByTestId('sessions-row-title').innerText();
  await page.getByRole('button', { name: `Show ${title} in the tree`, exact: true }).click();
  await page.getByTestId('sessions-peek').waitFor();
  await page.getByRole('button', { name: /^Back to \d+ results?$/ }).click();
  await page.waitForTimeout(500);
  const back = await metrics(page);
  assert.ok(Math.abs(back.offset - middle.offset) <= BACK_TOLERANCE, `${name}: Back restores ${middle.offset}, got ${back.offset}`);
  let filter = null;
  if (compact) {
    await chooseState(page, compact, 'current');
    const after = await metrics(page);
    filter = { before: back.offset, after: after.offset, firstBefore: back.first, firstAfter: after.first };
  }
  await page.getByTestId('sessions-layout-grouped').click();
  await settle(page);
  const grouped = await metrics(page);
  assert.equal(grouped.offset, 0, `${name}: Grouped opens at its remembered tree offset`);
  return { middle: middle.offset, back: back.offset, filter, grouped: grouped.offset };
}

async function check(browser, input) {
  const { width, height, status, layout, select = false, query = '', controls = false } = input;
  const compact = width < 600;
  const name = `${width}x${height}-${status}-${layout}${select ? '-select' : ''}${query ? '-search' : ''}`;
  const context = await browser.newContext({ viewport: { width, height }, isMobile: compact, hasTouch: compact });
  const page = await context.newPage(), errors = [], trace = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?embed=1&scenario=clear-restore&theme=dark&pointer=${compact ? 'coarse' : 'fine'}`);
    await list(page).waitFor();
    await chooseState(page, compact, status);
    await page.getByTestId(`sessions-layout-${layout}`).click();
    if (layout === 'grouped') await onFilters(page, compact, () => page.getByTestId('sessions-expand-all').click());
    if (query) await onFilters(page, compact, () => page.getByTestId('sessions-search').fill(query));
    if (select) await page.getByTestId('sessions-selection-toggle').click();
    await settle(page);
    let state = await metrics(page);
    assert.ok(state.max > 3 * state.viewport, `${name}: the list must be several screens long (${state.max} over ${state.viewport})`);
    const step = Math.round(state.viewport * 0.6);
    let steps = 0;
    while (state.offset < state.max - 1) {
      assert.ok(++steps < 200, `${name}: did not reach the end`);
      await wheel(page, step);
      state = steps % 2 ? await cycle(page, trace, 'down', null, steps % 7 === 0) : await metrics(page);
    }
    // The known end: the last row, fully in view. After three more update cycles and a re-read the offset has
    // not moved, and the same row is still the end; re-estimated row heights may leave it a short scroll below.
    const end = (await metrics(page)).last;
    assert.ok(end?.visible, `${name}: the last row is in view at the end`);
    for (let index = 0; index < 3; index += 1) state = await cycle(page, trace, 'end', end.id, index === 2);
    const endMax = state.max;
    for (let more = 0; state.offset < state.max - 1; more += 1) {
      assert.ok(more < 5, `${name}: the end moved away after updates`);
      await wheel(page, step);
      state = await metrics(page);
    }
    assert.ok(state.last?.id === end.id && state.last.visible, `${name}: the same last row ends the list after updates`);
    const endMaxAfter = state.max;
    await page.screenshot({ path: path.join(output, `${name}-end.png`) });
    steps = 0;
    while (state.offset > 0) {
      assert.ok(++steps < 200, `${name}: did not return to the top`);
      const previous = state.offset;
      await wheel(page, -step);
      state = await metrics(page);
      // A wheel sent while a re-read replaces the rows can be dropped; one retry, recorded.
      if (state.offset === previous) { trace.push({ phase: 'up', retry: previous }); await wheel(page, -step); state = await metrics(page); }
      assert.ok(state.offset < previous, `${name}: scrolling up did not move up (${previous} to ${state.offset})`);
      if (steps % 2) state = await cycle(page, trace, 'up', end.id, steps % 7 === 0);
    }
    await page.screenshot({ path: path.join(output, `${name}-top.png`) });
    const placement = controls ? await deliberate(page, compact, name) : null;
    assert.deepEqual(errors, []);
    assert.ok((await page.evaluate(() => window.rpcCalls ?? [])).every(([call]) => !/lifecycle-(preview|execute|resume|archive)/.test(call)));
    report.cases.push({ name, status: 'PASS', endRow: end.id, updates: trace.filter(item => item.target).length, refreshes: trace.filter(item => item.refresh).length,
      clamps: trace.filter(item => item.clamped).length, retries: trace.filter(item => item.retry !== undefined).length,
      maxDrift: Math.max(0, ...trace.map(item => Math.abs(item.drift ?? 0))), endMaxBefore: endMax, endMaxAfter, placement, trace });
    console.log(`${name}: PASS, max drift ${report.cases.at(-1).maxDrift} px`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors, trace });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
    console.log(`${name}: FAIL ${error.message.split('\n')[0]}`);
  } finally { await context.close(); }
}

(async () => {
  const browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const [width, height] of [[320, 640], [390, 844], [1280, 900]]) {
      await check(browser, { width, height, status: 'all', layout: 'flat', controls: true });
      await check(browser, { width, height, status: 'current', layout: 'flat' });
      await check(browser, { width, height, status: 'all', layout: 'grouped' });
      await check(browser, { width, height, status: 'all', layout: 'flat', select: true });
      await check(browser, { width, height, status: 'all', layout: 'flat', query: 'batch' });
    }
  } finally { await browser.close(); fs.writeFileSync(path.join(output, 'sessions-scroll-anchor-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  const failed = report.cases.filter(item => item.status !== 'PASS');
  console.log(`${report.cases.length - failed.length}/${report.cases.length} PASS`);
  if (failed.length) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
