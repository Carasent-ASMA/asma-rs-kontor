// Synthetic SessionsScreen fixture through the web prototype, no host/lifecycle effects. See counters-toolbar.browser.cjs.
// Supply PREVIEW_ORIGIN, PLAYWRIGHT_MODULE and EVIDENCE_DIR. No native-device claim.
// Recent is seeded in the preview origin's own storage before load: kept visits in open order, one session in error,
// one closed, one absent from the host, and saved entries from an earlier version.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const origin = process.env.PREVIEW_ORIGIN, output = process.env.EVIDENCE_DIR;
assert.ok(origin && output);
fs.mkdirSync(output, { recursive: true });
const report = { cases: [], nativeDeviceAndScreenReaderAcceptance: 'unverified' };
const HOST = 'srv_prototype';
const VISITED = [
  ['agt_qa_99103', 'QA • ASMA-99103 verify results'],
  ['agt_swe_99104_a', 'SWE • ASMA-99104 chain (first)'],
  ['missing-session', null],
  ['agt_sa_99103', 'SA • ASMA-99103 scope'],
  ['agt_lsa_99101', 'LSA • ASMA-99101 sessions organisation'],
];
const LEGACY = JSON.stringify({ clearedAt: null, visits: [{ id: 'agt_reva_pr812', at: 1 }, { id: 'gone-legacy', at: 2 }] });

async function check(browser, width, height, theme, seeded) {
  const compact = width < 600, name = `${width}x${height}-${theme}-${seeded ? 'seeded' : 'empty'}`;
  const context = await browser.newContext({ viewport: { width, height }, isMobile: compact, hasTouch: compact });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const now = Date.now();
  if (seeded) await page.addInitScript(([key, record, legacy]) => {
    localStorage.setItem(key, record); localStorage.setItem('kontor.recent@1', legacy);
  }, ['kontor.recent@2 ' + JSON.stringify([HOST]), JSON.stringify({ clearedAt: null, visits: VISITED.map(([id], index) => ({ id, at: now - index * 60_000 })) }), LEGACY]);
  try {
    await page.goto(`${origin}/?embed=1&pointer=${compact ? 'coarse' : 'fine'}&scenario=baseline&theme=${theme}`);
    await page.getByTestId('sessions-layout-grouped').waitFor();
    // Lands on Recent; the saved layout stays checked but disabled, with the reason as its help.
    if (!compact) assert.equal(await page.getByTestId('sessions-filter-recent').getAttribute('aria-pressed'), 'true');
    else assert.equal(await page.getByTestId('sessions-quick-recent').getAttribute('aria-pressed'), 'true');
    assert.equal(await page.getByTestId('sessions-layout').getAttribute('aria-disabled'), 'true');
    // The group is disabled to assistive technology; a press still explains why and changes nothing.
    await page.getByTestId('sessions-layout-flat').click({ force: true });
    assert.equal(await page.getByTestId('sessions-layout-flat').getAttribute('aria-checked'), 'false');
    await page.getByRole('tooltip').or(page.getByText(/^Recent lists sessions in the order you opened them/)).first().waitFor();
    await page.keyboard.press('Escape');
    // Compact Search is the first control of the Select/layout row and no longer sits in the quick filters.
    if (compact) {
      const first = await page.getByTestId('sessions-view-controls').evaluate(row => row.firstElementChild?.getAttribute('data-testid'));
      assert.equal(first, 'sessions-mobile-search-trigger');
      assert.equal(await page.locator('[data-testid="sessions-mobile-search-trigger"]').count(), 1);
    }
    const list = page.getByTestId('sessions-list');
    if (seeded) {
      const shown = VISITED.filter(([, title]) => title);
      const tops = [];
      for (const [, title] of shown) tops.push((await list.getByText(title, { exact: true }).first().boundingBox()).y);
      assert.deepEqual(tops, [...tops].sort((a, b) => a - b), 'Recent rows keep open order');
      const account = await page.getByTestId('sessions-recent-account').innerText();
      assert.match(account, /keeps the last 500 sessions .* with no age limit: 5 kept · 4 shown · 1 not found in this host's current list\./);
      const saved = page.getByTestId('sessions-recent-saved');
      await saved.scrollIntoViewIfNeeded();
      assert.match(await saved.innerText(), /Saved entries · visit time and host unknown/);
      assert.match(await saved.innerText(), /REV A • PR 812/);
      assert.match(await page.getByTestId('sessions-recent-saved-absent').innerText(), /^1 not found in this host's current list: gone-legacy$/);
      await page.screenshot({ path: path.join(output, `${name}.png`), fullPage: false });
      // Search narrows Recent; the open order stays.
      if (compact) await page.getByTestId('sessions-mobile-search-trigger').click();
      await page.getByTestId('sessions-search').fill('99103');
      await list.getByText('QA • ASMA-99103 verify results', { exact: true }).waitFor();
      assert.equal(await list.getByText('LSA • ASMA-99101 sessions organisation', { exact: true }).count(), 0);
      assert.match(await page.getByTestId('sessions-recent-account').innerText(), /5 kept · 2 shown · 2 hidden by search, scope or filters · 1 not found/);
      await page.getByTestId('sessions-search').fill('');
      // Opening a saved entry records a real visit; its bytes from the earlier version stay.
      await page.getByTestId('sessions-recent-saved').getByTestId('kontor-switch-row-agt_reva_pr812').click();
      const stored = await page.evaluate(key => JSON.parse(localStorage.getItem(key)).visits[0].id, 'kontor.recent@2 ' + JSON.stringify([HOST]));
      assert.equal(stored, 'agt_reva_pr812');
      assert.equal(await page.evaluate(() => localStorage.getItem('kontor.recent@1')), LEGACY);
    } else {
      const empty = page.getByTestId('sessions-empty');
      assert.match(await empty.innerText(), /No recent sessions yet\. Sessions you open from Kontor on this host and device appear here/);
      await page.screenshot({ path: path.join(output, `${name}.png`), fullPage: false });
      await page.getByTestId('sessions-empty-current').click();
      if (!compact) assert.equal(await page.getByTestId('sessions-filter-current').getAttribute('aria-pressed'), 'true');
      else assert.equal(await page.getByTestId('sessions-quick-current').getAttribute('aria-pressed'), 'true');
      assert.equal(await page.getByTestId('sessions-layout').getAttribute('aria-disabled'), null);
    }
    assert.deepEqual(errors, []);
    report.cases.push({ name, pass: true });
  } finally {
    await context.close();
  }
}

(async () => {
  const browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const [width, height] of [[1280, 800], [390, 844]]) for (const seeded of [true, false]) await check(browser, width, height, seeded ? 'light' : 'dark', seeded);
  } finally {
    await browser.close();
    fs.writeFileSync(path.join(output, 'recent-visits.json'), JSON.stringify(report, null, 2));
  }
  console.log(JSON.stringify(report));
})().catch(error => { console.error(error); process.exit(1); });
