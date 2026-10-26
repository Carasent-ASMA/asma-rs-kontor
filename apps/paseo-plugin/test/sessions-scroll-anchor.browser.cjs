// ASMA-8644: live updates keep the reader's place in the Sessions list. A reader at the top stays at the top, one at
// the end stays at the end, and in between the exact first visible session keeps its viewport-relative offset
// while present; only a session explicitly excluded by the fixture update permits fallback. While a finger is on the list, or within the screen's declared wait after the
// reader's last scroll, the screen writes no offset. Synthetic SessionsScreen on react-native-web over the
// 84-session and the 436-session (long-list) fixtures; no native-device claim.
// Needs the kontor-prototype dev server (`vite`, not `preview`): live updates go through its fixture runtime module.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR; ONLY narrows the run to matching case names, and
// DRIFT=record records places without asserting them, to compare another build. Each update targets the session at
// the top of the viewport, the one platform scroll anchoring followed back up the list; some also move a session from
// below the viewport above it, and on Running one leaves and another joins. Host offline/online cycles re-read the
// directory, and touch or wheel gestures carry updates while they scroll.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output, 'Supply an isolated synthetic fixture origin and evidence directory');
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
const record = process.env.DRIFT === 'record';
// Counts the page's own scroll writes, each with its time, offset and whether a finger was on the page; a reader's
// touch or wheel scroll writes none.
const countWrites = () => {
  window.__writes = 0;
  window.__written = [];
  window.__touching = false;
  for (const type of ['touchstart', 'touchend', 'touchcancel']) document.addEventListener(type, event => { window.__touching = event.touches.length > 0; }, true);
  const wrote = top => { window.__writes += 1; window.__written.push([performance.now(), top, window.__touching]); };
  for (const name of ['scroll', 'scrollTo', 'scrollBy']) {
    const original = Element.prototype[name];
    Element.prototype[name] = function (...args) { wrote(name === 'scrollBy' ? null : typeof args[0] === 'object' ? args[0].top : args[1]); return original.apply(this, args); };
  }
  const descriptor = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
  Object.defineProperty(Element.prototype, 'scrollTop', { configurable: true, get: descriptor.get, set(value) { wrote(value); descriptor.set.call(this, value); } });
};
const writes = page => page.evaluate(() => window.__writes);

const list = page => page.getByTestId('sessions-list');
const metrics = page => page.evaluate(() => {
  const node = document.querySelector('[data-testid="sessions-list"]');
  const box = node.getBoundingClientRect();
  const cell = row => { let element = row; while (element.parentElement && element.parentElement !== node.firstElementChild) element = element.parentElement; return element; };
  const rows = [...node.querySelectorAll('[data-testid^="sessions-row-agt_"]')].map(row => ({ id: row.dataset.testid.slice('sessions-row-'.length), rect: row.getBoundingClientRect(), height: cell(row).getBoundingClientRect().height }));
  const visible = rows.filter(row => row.rect.bottom > box.top + 1 && row.rect.top < box.bottom - 1).sort((a, b) => a.rect.top - b.rect.top);
  const last = rows.sort((a, b) => a.rect.top - b.rect.top).at(-1);
  return { offset: Math.round(node.scrollTop), max: node.scrollHeight - node.clientHeight, viewport: node.clientHeight, first: visible[0]?.id ?? null,
    // A row taller than the viewport shows only as much as fits: in view is its end.
    last: last ? { id: last.id, visible: last.rect.bottom <= box.bottom + 1 && (last.rect.top >= box.top - 1 || last.rect.height > box.height && last.rect.bottom > box.top + 1) } : null,
    inView: visible.map(row => row.id), at: Object.fromEntries(rows.map(row => [row.id, Math.round(row.rect.top - box.top)])),
    height: Object.fromEntries(rows.map(row => [row.id, Math.round(row.height)])) };
});
// The smallest scroll range seen in any frame: a list that shrinks for a moment clamps the offset for good.
const watch = page => page.evaluate(() => {
  const node = document.querySelector('[data-testid="sessions-list"]');
  const seen = window.__scrollWatch = { min: Infinity, stop: false };
  const tick = () => { seen.min = Math.min(seen.min, node.scrollHeight - node.clientHeight); if (!seen.stop) requestAnimationFrame(tick); };
  tick();
});
const watched = page => page.evaluate(() => { window.__scrollWatch.stop = true; return window.__scrollWatch.min; });
// One simulated native change through the fixture host's stream; a running session re-sorts to the top of its bucket.
const update = (page, id, patch = { status: 'running' }) => page.evaluate(async ([id, patch]) => {
  const runtime = await import('/src/host/runtime.ts');
  runtime.simulateUpdate(id, patch, 'ASMA-8644 scroll proof');
}, [id, patch]);
const idleSessions = page => page.evaluate(async () => (await import('/src/host/runtime.ts')).fixtureSessions()
  .filter(session => session.agent.status === 'idle' && !session.agent.archivedAt).map(session => session.agent.id));
const refreshCycle = page => page.evaluate(async () => {
  const runtime = await import('/src/host/runtime.ts');
  runtime.setHostStatus('offline');
  await new Promise(resolve => setTimeout(resolve, 250));
  runtime.setHostStatus('online');
  await new Promise(resolve => setTimeout(resolve, 400));
});
const settle = page => page.waitForTimeout(260);
/** Rounding of row positions; a held row keeps its distance from the viewport top to within this. */
const ROW_ANCHOR_TOLERANCE = 4;
/** A reader this close to an end of the list is at that end. */
const EDGE = 1;
/** The screen's declared wait after the reader's last scroll before it holds a place again. */
const SCROLL_SETTLE_MS = 150;
async function wheel(page, delta) {
  const box = await list(page).boundingBox();
  // Just inside the list's left edge, clear of every row's icons: a pointer resting on one opens its help over the
  // list, and the help takes the wheel. It leaves the list after the wheel too, or a group's count help opens under it.
  await page.mouse.move(box.x + 2, box.y + box.height / 2);
  await page.mouse.wheel(0, delta);
  await page.mouse.move(0, 0);
  await settle(page);
}
const QUICK_STATES = ['recent', 'activity', 'running', 'needs_input', 'error', 'all', 'idle', 'attention', 'favorites'];
async function chooseState(page, compact, status) {
  const quick = page.getByTestId(compact ? `sessions-quick-${status}` : `sessions-filter-${status}`);
  // A narrow state toolbar keeps some of the nine quick states behind More; Current and the other states are chosen
  // on the Filters page.
  if (await quick.count()) await quick.click();
  else if (compact && QUICK_STATES.includes(status)) { await page.getByTestId('sessions-mobile-quick-chooser-trigger').click(); await page.getByTestId(`sessions-choose-${status}`).click(); }
  else await onFilters(page, compact, () => page.getByTestId(`sessions-filter-${status}`).click());
  await settle(page);
}
/**
 * Show in tree, from the session's actions menu. Opening the menu can first scroll its button into view, so the place
 * the peek leaves is read with the menu open.
 */
async function showInTree(page, id) {
  await page.getByTestId(`session-actions-${id}`).click();
  const place = await metrics(page);
  await page.getByTestId('session-action-show-in-tree').click();
  await page.getByTestId('sessions-peek').waitFor();
  return place;
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
 * Scrolls until the first session in view starts `gap` points below the list's top edge, with the session above it ending
 * at that edge, so the reader sees that session first. Returns its ID and the reading there.
 */
async function edgeAt(page, gap) {
  let state = await metrics(page);
  const id = Object.keys(state.at).filter(key => state.at[key] > 20).sort((a, b) => state.at[a] - state.at[b])[0];
  for (let tries = 0; tries < 4 && Math.abs(state.at[id] - gap) > 0.5; tries += 1) {
    await wheel(page, state.at[id] - gap);
    state = await metrics(page);
  }
  return { id, state };
}

/**
 * Whether the reader kept their place from one reading to the next through these changes: at the top still at the top,
 * at the end still at the end, and in between the exact first visible ID at the same viewport offset;
 * only an ID explicitly leaving the filter permits a surviving visible neighbor. A list that got shorter under the reader, even for
 * one frame, may clamp the offset; that is recorded.
 */
function judge(before, after, missing) {
  const place = before.offset <= EDGE ? 'top' : before.offset >= before.max - EDGE ? 'end' : 'row';
  const held = before.first && !missing.has(before.first) ? before.first : before.inView.find(id => !missing.has(id)) ?? null;
  const failure = place === 'top' ? after.offset > EDGE && `a reader at the top was moved to ${after.offset}`
    : place === 'end' ? after.offset < after.max - EDGE && `a reader at the end was left at ${after.offset} of ${after.max}`
    : held ? (!(held in after.at) || Math.abs(after.at[held] - before.at[held]) > ROW_ANCHOR_TOLERANCE)
      && `exact ${held} left its viewport offset (${before.at[held]} to ${after.at[held]} px)`
    : (after.offset < 0 || after.offset > after.max + EDGE) && 'missing-anchor fallback left the scroll range';
  return { place, held, failure };
}

/**
 * One round of streamed changes, then the reader's place. A round that moves every session in view is judged when
 * those changes land, where the offset holds, and again after the rest of it from the sessions the offset then shows.
 */
async function cycle(page, trace, phase, { endId = null, refresh = false, below = false, changes = null, status = 'all' } = {}) {
  const before = await metrics(page);
  const target = before.first && before.first !== endId ? before.first : null;
  const lower = Object.keys(before.at).find(id => before.at[id] > before.viewport && id !== endId) ?? null;
  const round = changes?.(target, lower) ?? [target, below && lower].filter(Boolean).map(id => [id, { status: 'running' }]);
  // Missing means explicitly excluded by this fixture update, never merely absent from the rendered window.
  const missing = new Set(round.filter(([, patch]) => patch.archivedAt
    || status === 'running' && patch.status && patch.status !== 'running'
    || status === 'current' && patch.status === 'closed').map(([id]) => id));
  const written = await writes(page);
  for (const [id, patch] of round) await update(page, id, patch);
  if (refresh) await refreshCycle(page);
  await settle(page);
  const after = await metrics(page);
  const { place, held, failure } = judge(before, after, missing);
  trace.push({ phase, place, changes: round, refresh, missing: [...missing], before: before.offset, after: after.offset, max: after.max,
    first: { before: before.first && [before.first, before.at[before.first]], after: after.first && [after.first, after.at[after.first]] },
    held: held && { id: held, before: before.at[held], after: after.at[held], first: held === before.first },
    writes: await writes(page) - written, ...(failure ? { failure } : {}) });
  if (!record) assert.ok(!failure, `${phase}: ${failure}`);
  return after;
}

/**
 * Streamed changes while the reader scrolls by touch (and its fling) or wheel. The screen writes no offset while a
 * finger is on the list or within its declared wait after the reader's last scroll, the list never runs against the
 * gesture, and once the gesture has run out the first session in view stays put. A scroll event at an offset the
 * screen just wrote is the screen's, not the reader's.
 */
async function gesture(page, trace, phase, compact, cdp, distance) {
  const before = await metrics(page);
  const box = await list(page).boundingBox();
  const began = await page.evaluate(() => {
    const node = document.querySelector('[data-testid="sessions-list"]');
    const frames = window.__frames = [], scrolls = window.__scrolls = [];
    const tick = () => { frames.push([performance.now(), Math.round(node.scrollTop), window.__writes]); if (!frames.stop) requestAnimationFrame(tick); };
    node.addEventListener('scroll', () => { if (!frames.stop) scrolls.push([performance.now(), node.scrollTop]); });
    tick();
    return performance.now();
  });
  const [x, y] = [Math.round(box.x + 2), Math.round(box.y + box.height / 2)];
  // Touch drags and flings. A wheel streams events, as a trackpad does: the headless browser's own synthesized wheel
  // gesture moves the list without the scroll events a page sees, so no screen could tell the reader is scrolling.
  const done = compact
    ? cdp.send('Input.synthesizeScrollGesture', { x, y, yDistance: -distance, speed: 700, gestureSourceType: 'touch', preventFling: false })
    : (async () => {
      for (let sent = 0; sent < Math.abs(distance); sent += 20) {
        cdp.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y, deltaX: 0, deltaY: 20 * Math.sign(distance) });
        await new Promise(resolve => setTimeout(resolve, 16));
      }
    })();
  await page.waitForFunction(start => Math.abs(document.querySelector('[data-testid="sessions-list"]').scrollTop - start) > 40, before.offset);
  const lower = Object.keys(before.at).find(id => before.at[id] > before.viewport) ?? null;
  for (const id of [before.first, lower].filter(Boolean)) await update(page, id);
  await done;
  await page.mouse.move(0, 0);
  // The fling runs out.
  let landed = await metrics(page);
  for (let wait = 0; wait < 20; wait += 1) {
    await page.waitForTimeout(150);
    const now = await metrics(page);
    if (now.offset === landed.offset) break;
    landed = now;
  }
  const { frames, scrolls, written } = await page.evaluate(() => { window.__frames.stop = true; return { frames: window.__frames, scrolls: window.__scrolls, written: window.__written }; });
  const mine = written.filter(([time]) => time >= began);
  const reader = scrolls.filter(([time, top]) => !mine.some(([at, value]) => at <= time && value !== null && Math.abs(value - top) <= 1));
  const against = mine.filter(([time, , touching]) => touching || reader.some(([at]) => at < time && time - at < SCROLL_SETTLE_MS))
    .map(([time, value, touching]) => ({ value, touching, sinceScroll: Math.round(time - (reader.filter(([at]) => at < time).at(-1)?.[0] ?? began)) }));
  const moves = frames.flatMap((frame, index) => index > 0 && frame[1] !== frames[index - 1][1] && frame[2] === frames[index - 1][2] ? [[frame[0], frame[1] - frames[index - 1][1]]] : []);
  const reversed = moves.find(([, by]) => by * Math.sign(distance) < -1) ?? null;
  await page.waitForTimeout(400);
  const later = await metrics(page);
  trace.push({ phase, gesture: compact ? 'touch' : 'wheel', distance, before: before.offset, landed: landed.offset, later: later.offset, moves: moves.length,
    scrolls: reader.length, writes: mine.map(([time, value, touching]) => [Math.round(time - began), value, touching]), against, reversed,
    first: { landed: [landed.first, landed.at[landed.first]], later: [later.first, later.at[later.first]] } });
  if (record) return later;
  assert.deepEqual(against, [], `${phase}: the screen scrolled against the reader's ${distance} px gesture`);
  assert.equal(reversed, null, `${phase}: the list ran against the ${distance} px gesture (${reversed})`);
  assert.ok(Math.sign(landed.offset - before.offset) === Math.sign(distance), `${phase}: the ${distance} px gesture went from ${before.offset} to ${landed.offset}`);
  assert.ok(later.first === landed.first && Math.abs(later.at[later.first] - landed.at[landed.first]) <= ROW_ANCHOR_TOLERANCE,
    `${phase}: the list moved after the gesture (${landed.first} at ${landed.at[landed.first]}, then ${later.first} at ${later.at[later.first]})`);
  return later;
}

/**
 * Deliberate navigation keeps its own placement: a peek shows its session, Back from it returns to the
 * results offset, and a layout change opens Grouped at its remembered tree offset (0 here). Back returns
 * focus to the session's actions button, which scrolls that button into view: within the row, toward it. A state
 * change from the Filters page scrolls nothing itself; where it leaves the list is recorded, not asserted.
 */
async function deliberate(page, compact, name) {
  await wheel(page, 1400);
  const id = (await metrics(page)).first;
  const middle = await showInTree(page, id);
  assert.ok(middle.offset > 600, `${name}: control starts mid-list`);
  await page.waitForTimeout(500);
  assert.ok((await metrics(page)).inView.includes(id), `${name}: the peek shows ${id}`);
  await page.getByRole('button', { name: /^Back to \d+ results?$/ }).click();
  await page.waitForTimeout(500);
  const back = await metrics(page);
  assert.ok(back.inView.includes(id) && Math.abs(back.at[id] - middle.at[id]) <= ROW_ANCHOR_TOLERANCE, `${name}: Back restores ${middle.offset}, got ${back.offset}`);
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
  const { width, height, scenario = 'clear-restore', status, layout, select = false, query = '', controls = false, short = false } = input;
  const compact = width < 600;
  const name = `${width}x${height}${scenario === 'long-list' ? '-long' : ''}-${status}-${layout}${select ? '-select' : ''}${query ? '-search' : ''}`;
  if (process.env.ONLY && !new RegExp(process.env.ONLY).test(name)) return;
  const context = await browser.newContext({ viewport: { width, height }, isMobile: compact, hasTouch: compact });
  await context.addInitScript(countWrites);
  if (status === 'recent') await context.addInitScript(() => localStorage.setItem('kontor.recent@2 ["srv_prototype"]', JSON.stringify({
    clearedAt: null, visits: Array.from({ length: 412 }, (_, index) => ({ id: `agt_long_${index}`, at: Date.now() - index * 60000 })),
  })));
  const page = await context.newPage(), errors = [], trace = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto(`${origin}/?embed=1&scenario=${scenario}&theme=dark&pointer=${compact ? 'coarse' : 'fine'}`);
    await list(page).waitFor();
    await chooseState(page, compact, status);
    if (status !== 'recent') await page.getByTestId(`sessions-layout-${layout}`).click();
    if (layout === 'grouped') await onFilters(page, compact, () => page.getByTestId('sessions-expand-all').click());
    if (query) await onFilters(page, compact, () => page.getByTestId('sessions-search').fill(query));
    // Select sessions is the actions menu beside the layout control.
    if (select) { await page.getByTestId('session-actions-toolbar').click(); await page.getByTestId('session-action-select').click(); }
    await settle(page);
    let state = await metrics(page);
    if (!short) assert.ok(state.max > 3 * state.viewport, `${name}: the list must be several screens long (${state.max} over ${state.viewport})`);
    // Rounds take turns. Every third leaves the first session in view alone: one from below it re-sorts above it,
    // or on Running an idle session joins there. The others re-sort the first in view, and on Running it leaves.
    const joining = status === 'running' ? await idleSessions(page) : [];
    let turn = 0;
    const changes = (target, lower) => {
      turn += 1;
      if (status !== 'running') return lower ? [[lower, { status: 'running' }]] : [];
      if (turn % 3 === 1 && joining.length) return [[joining.shift(), { status: 'running' }]];
      return target ? [[target, { status: turn % 3 === 0 ? 'idle' : 'running' }]] : [];
    };
    for (let index = 0; index < 3; index += 1) state = await cycle(page, trace, 'top', { refresh: index === 2, changes, status });
    const step = Math.round(state.viewport * 2);
    if (controls) {
      // Jump across unmeasured rows before the traversal; the old walked-list Back proof missed this.
      const box = await list(page).boundingBox();
      await wheel(page, Math.min(state.max / 2, box.height * 12));
      const original = await metrics(page);
      await cycle(page, trace, 'present-anchor-reordered-and-resized', { status,
        changes: target => target ? [[target, { status: 'running', title: 'SWE • ASMA-99101 batch exact anchor with a longer variable height title for the next layout' }]] : [] });
      const kept = await metrics(page);
      assert.ok(kept.inView.includes(original.first), `${name}: reordered exact ID remains visible`);
      const id = (await metrics(page)).first;
      const beforeBack = await showInTree(page, id);
      await page.waitForTimeout(300);
      await page.getByRole('button', { name: /^Back to \d+ results?$/ }).click();
      await page.waitForTimeout(500);
      const back = await metrics(page);
      assert.ok(id in back.at && Math.abs(back.at[id] - beforeBack.at[id]) <= ROW_ANCHOR_TOLERANCE,
        `${name}: Back over unmeasured rows restores exact ${id} at ${beforeBack.at[id]}, got ${back.at[id]}`);
      trace.push({ phase: 'unmeasured-back', id, before: beforeBack.at[id], after: back.at[id] });
      // The session above ends at the top edge, so the reader sees the next one first; that one re-sorts away from here and
      // the list must carry it, not the one above. Anything drawn under a row but outside it would hold the one above.
      const edge = await edgeAt(page, 2);
      assert.ok(edge.state.first === edge.id && edge.state.at[edge.id] === 2, `${name}: ${edge.id} starts 2 px below the top edge (${edge.state.at[edge.id]}, first ${edge.state.first})`);
      state = await cycle(page, trace, 'edge-anchor-reordered', { status, changes: target => target ? [[target, { status: 'running', title: 'SWE • ASMA-99101 batch edge anchor re-sorted from below the session above' }]] : [] });
      assert.ok(Math.abs(state.offset - edge.state.offset) > 40, `${name}: the edge re-sort moved ${edge.id} in the list (${edge.state.offset} to ${state.offset})`);
    }
    let steps = 0;
    while (state.offset < state.max - 1) {
      assert.ok(++steps < 2000, `${name}: did not reach the end`);
      await wheel(page, step);
      state = steps % 2 ? await cycle(page, trace, 'down', { refresh: steps % 7 === 0, below: steps % 5 === 0, changes, status }) : await metrics(page);
    }
    // The known end: the last row in view, in full if it fits. After three more update cycles and a re-read the offset has
    // not moved, and the same row is still the end; re-estimated row heights may leave it a short scroll below.
    const end = (await metrics(page)).last;
    assert.ok(end?.visible, `${name}: the last row is in view at the end`);
    for (let index = 0; index < 3; index += 1) state = await cycle(page, trace, 'end', { endId: end.id, refresh: index === 2, changes, status });
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
      assert.ok(++steps < 2000, `${name}: did not return to the top`);
      const previous = state.offset;
      await wheel(page, -step);
      state = await metrics(page);
      // A wheel sent while a re-read replaces the rows can be dropped; one retry, recorded.
      if (state.offset === previous) { trace.push({ phase: 'up', retry: previous }); await wheel(page, -step); state = await metrics(page); }
      assert.ok(state.offset < previous, `${name}: scrolling up did not move up (${previous} to ${state.offset})`);
      if (steps % 2) state = await cycle(page, trace, 'up', { endId: end.id, refresh: steps % 7 === 0, below: steps % 5 === 0, changes, status });
    }
    await page.screenshot({ path: path.join(output, `${name}-top.png`) });
    if (!short) {
      // The reader's own gestures, down and back up from the middle, while sessions re-sort under them.
      const cdp = await context.newCDPSession(page);
      await wheel(page, Math.round(state.max / 2));
      for (const distance of [1500, -1500]) await gesture(page, trace, 'gesture', compact, cdp, distance);
    }
    const placement = controls ? await deliberate(page, compact, name) : null;
    assert.deepEqual(errors, []);
    assert.ok((await page.evaluate(() => window.rpcCalls ?? [])).every(([call]) => !/lifecycle-(preview|execute|resume|archive)/.test(call)));
    const rounds = trace.filter(item => item.place);
    const held = rounds.filter(item => item.place === 'row' && item.held);
    const summary = { rounds: rounds.length, top: rounds.filter(item => item.place === 'top').length, end: rounds.filter(item => item.place === 'end').length,
      heldFirst: held.filter(item => item.held.first).length, passedOn: held.filter(item => !item.held.first).length, clamps: rounds.filter(item => item.clamped).length, allMoved: rounds.filter(item => item.allMoved).length,
      maxHeldError: Math.max(0, ...held.map(item => Math.abs(item.held.after - item.held.before))), gestures: trace.filter(item => item.gesture).length,
      refreshes: rounds.filter(item => item.refresh).length, retries: trace.filter(item => item.retry !== undefined).length, failures: rounds.filter(item => item.failure).length };
    report.cases.push({ name, status: record ? 'RECORDED' : 'PASS', endRow: end.id, ...summary, endMaxBefore: endMax, endMaxAfter, placement, trace });
    console.log(`${name}: ${record ? 'RECORDED' : 'PASS'} ${JSON.stringify(summary)}`);
  } catch (error) {
    report.cases.push({ name, status: 'FAIL', error: error.stack, errors, trace });
    await page.screenshot({ path: path.join(output, `${name}-failure.png`) });
    console.log(`${name}: FAIL ${error.message.split('\n')[0]}`);
  } finally { await context.close(); }
}

(async () => {
  const browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const [width, height] of [[320, 640], [390, 844]]) {
      await check(browser, { width, height, scenario: 'long-list', status: 'all', layout: 'flat', controls: true });
      await check(browser, { width, height, scenario: 'long-list', status: 'current', layout: 'flat' });
      await check(browser, { width, height, scenario: 'long-list', status: 'all', layout: 'grouped' });
      await check(browser, { width, height, scenario: 'long-list', status: 'recent', layout: 'flat' });
      await check(browser, { width, height, status: 'running', layout: 'flat', short: true });
    }
    await check(browser, { width: 320, height: 640, status: 'all', layout: 'flat', select: true });
    await check(browser, { width: 390, height: 844, status: 'all', layout: 'flat', query: 'batch' });
    await check(browser, { width: 1280, height: 900, scenario: 'long-list', status: 'all', layout: 'flat', controls: true });
  } finally { await browser.close(); fs.writeFileSync(path.join(output, 'sessions-scroll-anchor-report.json'), JSON.stringify(report, null, 2) + '\n'); }
  const failed = report.cases.filter(item => item.status === 'FAIL');
  console.log(`${report.cases.length - failed.length}/${report.cases.length} PASS`);
  if (failed.length) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; });
