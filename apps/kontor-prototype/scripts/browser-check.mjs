// A fresh browser/profile, fixture preview only. Evidence belongs outside the repository.
import assert from "node:assert/strict";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { parseArgs } from "node:util";
const { values } = parseArgs({ options: Object.fromEntries(["playwright", "executable", "origin", "output"].map(key => [key, { type: "string" }])) });
for (const key of ["playwright", "executable", "origin", "output"]) assert(values[key], `Explicit --${key} required`);
const origin = new URL(values.origin).origin;
assert.equal(new URL(origin).hostname, "127.0.0.1", "Use the isolated loopback preview");
const scenarios = [...(await readFile(new URL("../src/scenarios.ts", import.meta.url), "utf8")).matchAll(/\bid: "([^"]+)"/g)].map(match => match[1]);
const { chromium } = createRequire(import.meta.url)(values.playwright);
const browser = await chromium.launch({ executablePath: values.executable, headless: true, args: ["--disable-background-networking"] });
const errors = [], denied = [], results = [], requests = [];
await mkdir(values.output, { recursive: true });
async function open(width, theme, scenario = "integrated-review", extra = "") {
  const context = await browser.newContext({ viewport: { width, height: width < 500 ? 844 : 900 }, serviceWorkers: "block" });
  await context.route("**/*", route => {
    const request = route.request();
    if (request.method() === "GET" && new URL(request.url()).origin === origin) { requests.push(request.url()); return route.continue(); }
    denied.push(request.url()); return route.abort();
  });
  await context.routeWebSocket("**/*", socket => { denied.push(socket.url()); socket.close(); });
  const page = await context.newPage(); page.setDefaultTimeout(8000);
  const sourceCalls = [];
  page.on("pageerror", error => errors.push(`${scenario}: ${error.message}`));
  page.on("console", message => { if (message.type() === "error" || message.text().includes("paseo.unsupported")) errors.push(`${scenario}: ${message.text()}`); });
  page.on("console", message => { if (/\[kontor-prototype\] sources\./.test(message.text())) sourceCalls.push(message.text()); });
  await page.goto(`${origin}/?embed=1&scenario=${scenario}&theme=${theme}&pointer=${width < 500 ? "coarse" : "fine"}${extra}`);
  await page.getByTestId("prototype-bar").waitFor();
  await page.getByRole("tab", { name: "Sessions", exact: true }).waitFor();
  await page.waitForTimeout(500);
  assert.match(await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content"), /connect-src 'none'/);
  return { context, page, sourceCalls };
}
// Captured scenarios hold operator-local titles: assertions on them say what failed, never echo the text.
const ROW_PARTS = new Set(["state", "body", "opened", "origin", "model", "tokens", "note", "choice"]);
const switchRowIds = page => page.getByTestId("kontor-switch-list").evaluate(list => [...list.querySelectorAll('[data-testid^="session-row-"]')].map(node => node.dataset.testid.slice(12)))
  .then(ids => ids.filter(id => !ROW_PARTS.has(id)));
const rowBody = (page, id) => page.getByTestId(`session-row-${id}`).getByTestId("session-row-body");
// The switcher sheet lies over Kontor's Sessions, which lists the same session: name the switcher's list as the scope there.
async function rowAction(page, id, key, scope = page) {
  await scope.getByTestId(`session-actions-${id}`).click();
  const menu = page.getByRole("dialog", { name: "Session actions", exact: true });
  await menu.waitFor();
  const keys = await menu.evaluate(node => [...node.querySelectorAll('[data-testid^="session-action-"]')].map(item => item.dataset.testid.slice(15)).filter(item => !item.endsWith("-option")));
  await menu.getByTestId(`session-action-${key}`).click();
  await menu.waitFor({ state: "detached" });
  return keys;
}
const leftOf = async (page, left, right) => { const [a, b] = [await page.getByTestId(left).boundingBox(), await page.getByTestId(right).boundingBox()]; return a.x + a.width <= b.x && Math.abs(a.y - b.y) < 2; };
const noOverflow = page => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1);
const SWITCH_ACTIONS = ["takeover", "favorite", "summarize", "attach", "copy-id", "details", "select", "archive"];
// The shared state toolbar draws whole tiles only, so a state it has no room for is chosen from More.
async function pickState(page, state) {
  const tile = page.getByTestId(`kontor-switch-state-${state}`);
  if (await tile.count()) return tile.click();
  await page.getByTestId("kontor-switch-more").click();
  await page.getByTestId(`kontor-switch-more-${state}`).click();
}
const NINE = ["recent", "activity", "running", "needs_input", "error", "all", "idle", "attention", "favorites"];
/** A state row as drawn: its tiles, which are pressed, its last two controls, and whether any control is clipped, wrapped or scrolled. */
async function stateRow(page, id, prefix, label, pinned) {
  const row = await page.getByTestId(id).evaluate((node, prefix) => {
    const box = node.getBoundingClientRect(), buttons = [...node.querySelectorAll('[role="button"]')], rects = buttons.map(button => button.getBoundingClientRect());
    const tiles = buttons.filter(button => button.dataset.testid?.startsWith(prefix));
    return { width: box.width, tiles: tiles.map(tile => tile.dataset.testid.slice(prefix.length)), labels: tiles.map(tile => tile.getAttribute("aria-label")),
      pressed: tiles.filter(tile => tile.getAttribute("aria-pressed") === "true").map(tile => tile.dataset.testid.slice(prefix.length)), last: buttons.slice(-2).map(button => button.dataset.testid),
      whole: rects.every(rect => rect.left >= box.left - 0.5 && rect.right <= box.right + 0.5 && rect.width >= 43.5), oneLine: rects.every(rect => Math.abs(rect.top - rects[0].top) < 1),
      scrolls: [node, ...node.querySelectorAll("div")].some(element => element.scrollWidth > element.clientWidth + 1 && element.contains(buttons[0])) };
  }, prefix);
  // Tiles of 44 with gaps of 4, less the two pinned controls, from one to nine.
  const slots = Math.max(1, Math.min(9, Math.floor((row.width + 4) / 48) - 2));
  assert.equal(row.tiles.length, slots, `${label}: ${row.tiles.length} whole tiles in ${row.width}, expected ${slots}`);
  assert(row.whole && row.oneLine && !row.scrolls, `${label}: a control is clipped, wrapped or scrolled`);
  assert.deepEqual(row.last, pinned, `${label}: More and the trailing control are pinned last`);
  assert.equal(new Set(row.tiles).size, row.tiles.length, `${label}: no state twice`);
  return row;
}
const SWITCH_PINNED = ["kontor-switch-more", "kontor-switch-kontor"], SESSIONS_PINNED = ["sessions-mobile-quick-chooser-trigger", "sessions-mobile-filters-trigger"];
/** Opens a live fixture session with a workspace from the switcher, so the host shim shows its composer pills. */
async function composerPill(page) {
  const id = await page.evaluate(ids => ids.find(id => { const seat = window.__kontorNavigatorFixture.seat(id); return seat && !seat.archivedAt && seat.workspaceId; }), await switchRowIds(page));
  assert(id, "a live session with a workspace");
  await page.getByTestId("kontor-switch-list").getByTestId(`session-row-${id}`).getByTestId("session-row-body").click();
  await page.getByTestId("host-agent-shim").waitFor();
  const pill = page.locator('[data-testid^="host-pill-"]').first();
  await pill.waitFor();
  return pill;
}
/** Sessions' search as shown: the field, or on a phone the Search tile that names the current search. */
async function sessionsQuery(page) {
  const field = page.getByTestId("sessions-search");
  if (await field.count()) return field.inputValue();
  return (await page.getByTestId("sessions-mobile-search-trigger").getAttribute("aria-label")).match(/^Search sessions(?:, current search: (.*))?$/)?.[1] ?? "";
}
const inside = (inner, outer) => inner.x >= outer.x - 1 && inner.y >= outer.y - 1 && inner.x + inner.width <= outer.x + outer.width + 1 && inner.y + inner.height <= outer.y + outer.height + 1;
// The captured Codex route's six levels, which keep one row on a 320-point screen.
const CODEX_LEVELS = ["low", "medium", "high", "xhigh", "max", "ultra"];
async function codexLevelsOneRow(page, label) {
  const row = page.getByTestId("kontor-model-levels");
  await row.scrollIntoViewIfNeeded();
  const [frame, viewport] = [await row.boundingBox(), page.viewportSize()];
  const chips = await page.getByRole("radiogroup", { name: "Effort", exact: true }).getByRole("radio").evaluateAll(nodes => nodes.map(node => {
    const box = node.getBoundingClientRect(), text = node.querySelector('[dir="auto"]'), hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return { label: node.getAttribute("aria-label"), x: box.left, right: box.right, y: Math.round(box.top), height: box.height, hit: !!hit && node.contains(hit),
      clipped: text.scrollWidth > text.clientWidth + 1, disabled: node.getAttribute("aria-disabled") === "true" };
  }));
  assert.deepEqual(chips.map(chip => chip.label), CODEX_LEVELS, `${label}: the six Codex levels`);
  assert.equal(new Set(chips.map(chip => chip.y)).size, 1, `${label}: the six levels share one row: ${JSON.stringify(chips.map(chip => [chip.label, chip.y]))}`);
  for (const chip of chips) {
    assert(chip.x >= frame.x - 0.5 && chip.right <= frame.x + frame.width + 0.5 && chip.right <= viewport.width && chip.height >= (viewport.width < 600 ? 44 : 30) && chip.hit && !chip.clipped && !chip.disabled,
      `${label}: ${chip.label} is whole, inside its row and usable: ${JSON.stringify(chip)}`);
  }
  return { row: Math.round(frame.width), used: Math.round(chips.at(-1).right - chips[0].x), y: chips[0].y };
}
// R4b: a captured seat reports no saved settings, so Auto states the defaults its successor would run at, and that review waits for its operator.
const UNREPORTED = /^• This seat's settings were not reported, so the new session would run at .+'s defaults: .+\.$/m;
const BUSY = "Finish or check the unfinished takeover first.";
const TAKEOVER_LINES = ["Continuing · Previous session archived", "No available model · Original kept", "Previous session archived · No new session yet · Recovery pending", "Not confirmed · Check before retrying", "Not started · Original kept"];
try {
  for (const scenario of scenarios) {
    const { context, page } = await open(390, "dark", scenario);
    await page.waitForFunction(() => !document.body.innerText.includes("Reading native sessions"));
    if (/offline|stale/.test(scenario)) await page.waitForTimeout(4200);
    assert.match(await page.locator("body").innerText(), /fictional fixture data|captured local data, frozen; effects simulated/);
    assert(await page.locator("body").innerText().then(text => text.length > 200), scenario);
    results.push({ scenario, width: 390, theme: "dark" });
    console.log("PASS", scenario);
    await context.close();
  }
  for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
    const { context, page } = await open(width, theme);
    assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), `${width}: horizontal overflow`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}.png`) });
    if (width < 500) await page.getByTestId("sessions-mobile-filters-trigger").click();
    else await page.getByTestId("sessions-advanced-toggle").click();
    if (width < 500) {
      assert.match(await page.getByTestId("sessions-mobile-filters-count").innerText(), /\d+ matching sessions/);
      await page.screenshot({ path: join(values.output, `${width}-${theme}-filters.png`) });
    }
    await page.getByTestId("sessions-working-as").click();
    await page.getByRole("menuitem", { name: /Alex \(fictional\)/ }).click();
    assert.match(await page.getByTestId("sessions-working-as").innerText(), /Alex/);
    await page.getByTestId("sessions-sort").click();
    await page.getByRole("menuitem", { name: "Title", exact: true }).click();
    assert.match(await page.getByTestId("sessions-sort").innerText(), /Title/);
    if (width < 500) await page.getByRole("button", { name: "Show sessions with the current filters", exact: true }).click();
    await page.getByRole("button", { name: "Start or switch session", exact: true }).click();
    await pickState(page, "activity");
    await page.getByTestId("kontor-switch-search").fill("ASMA-99103");
    await page.getByTestId("kontor-switch-list").getByTestId("session-row-body").first().click();
    await page.getByTestId("host-agent-shim").waitFor();
    await page.getByRole("button", { name: "Back (host)", exact: true }).click();
    await page.getByRole("button", { name: "Start or switch session", exact: true }).click();
    assert((await switchRowIds(page)).length > 0, "Recent retains the opened fictional session");
    await page.getByTestId("kontor-switch-new-lead").click();
    await page.getByTestId("kontor-new-lsa-topic").fill("Fictional review");
    await page.getByTestId("kontor-new-lsa-create").click();
    await page.getByTestId("kontor-new-lsa-verified").waitFor();
    assert.match(await page.getByTestId("kontor-new-lsa-verified").innerText(), /SIMULATED/);
    results.push({ width, theme, profileAndSort: true, startSwitchAndRecent: true, syntheticCreation: true, noOverflow: true });
    await context.close();
  }
  for (const width of [320, 390, 1440]) {
    const { context, page } = await open(width, "light", "model-usage");
    const row = page.getByTestId("sessions-row-agt_model_report_8");
    await row.waitFor();
    // The dense row names the routed model by its alias; a last turn's usage is never shown as the session's totals.
    assert.equal(await row.getByTestId("session-row-model").innerText().then(text => text.split("\n")[0]), "S6.1");
    assert.equal(await row.getByTestId("session-row-tokens").innerText(), "↓ —   ↑ —");
    assert(!/fictional-route|4\.1K|1\.2M/.test(await row.innerText()), "Route account and last-turn usage belong in Details, not the row");
    await row.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(values.output, `${width}-model-route-tokens.png`) });
    results.push({ width, modelRoute: "alias S6.1 only", tokens: "session totals unknown" });
    await context.close();
  }
  {
    const { context, page } = await open(1440, "light", "board-tasks");
    for (const name of ["Overview", "Plan", "Board"]) {
      await page.getByRole("tab", { name, exact: true }).click();
      await page.waitForTimeout(100);
    }
    for (const name of ["Graph", "Delivery", "Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"]) {
      const tab = page.getByRole("tab", { name, exact: true });
      if (await tab.count()) await tab.click();
      else {
        await page.getByRole("button", { name: /^More Kontor views/ }).click();
        await page.getByRole("menuitem", { name, exact: true }).click();
      }
      await page.waitForTimeout(100);
    }
    await page.getByRole("button", { name: "Test shortcut", exact: true }).waitFor();
    results.push({ navigation: "all 12 plugin destinations" });
    await context.close();
  }
  {
    const { context, page } = await open(390, "dark", "session-recovery");
    await rowAction(page, "agt_recovery_parent", "select");
    await page.getByTestId("sessions-row-agt_recovery_child").getByTestId("session-row-body").click();
    assert.equal(await page.getByTestId("selection-count").innerText(), `2 / ${(await page.getByTestId("selection-count").innerText()).split(" / ")[1]}`);
    await page.getByRole("button", { name: "Review archive of 2 selected sessions", exact: true }).click();
    const confirm = page.getByTestId("bulk-archive-dialog");
    await confirm.getByText("Archive 2 sessions", { exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_parent").archivedAt), null, "Review has no effects");
    assert(await leftOf(page, "bulk-archive-cancel", "bulk-archive-confirm"), "D4: Cancel LEFT, Archive N RIGHT");
    await page.screenshot({ path: join(values.output, "390-selected-child-parent-review.png") });
    await confirm.getByRole("button", { name: "Archive 2 sessions", exact: true }).click();
    await page.waitForFunction(() => ["agt_recovery_parent", "agt_recovery_child"].every(id => window.__kontorP1Fixtures.read(id).archivedAt));
    assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_child").persistence.sessionId), "fictional-history-child");
    results.push({ bulk: "selected child+parent synthetic confirmation, history retained" });
    await context.close();
  }
  {
    const { context, page, sourceCalls } = await open(1440, "light");
    assert(sourceCalls.some(line => line.includes("sources.summaries")), "Sessions requests metadata summaries");
    assert(!sourceCalls.some(line => line.includes("sources.read ")), "Sessions must not request full sources");
    await page.getByRole("tab", { name: "Plan", exact: true }).click();
    await page.getByRole("button", { name: "Choose plan", exact: true }).click();
    await page.getByRole("menuitem", { name: /^Plan:.*ASMA-99101/ }).click();
    await page.waitForFunction(() => document.body.innerText.includes("ASMA-99101"));
    await page.waitForTimeout(300);
    assert(sourceCalls.some(line => line.includes("sources.read plan-99101")), "Selected Plan demands its exact source");
    assert(!sourceCalls.some(line => /sources.read (plan-99102|jira-capture)/.test(line)), "Unselected sources remain metadata only");
    results.push({ sourceDemand: "metadata-only Sessions, exact selected Plan", sourceCalls });
    await context.close();
  }
  for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
    // Board D1/D4 on the captured switcher: the menu's Select sessions, frozen IDs, the header count and the sticky bar.
    const { context, page } = await open(width, theme, "unified-switcher");
    const ids = await switchRowIds(page);
    assert(ids.length >= 3, `${width}-${theme}: captured Last visited lists at least three sessions`);
    assert.deepEqual(await rowAction(page, ids[0], "select", page.getByTestId("kontor-switch-list")), SWITCH_ACTIONS, "C1 menu order");
    const count = page.getByTestId("selection-count");
    const total = Number((await count.innerText()).match(/^1 \/ (\d+) selected$/)?.[1]);
    assert(total >= ids.length - 1, `${width}-${theme}: header counts the current matches`);
    await rowBody(page, ids[1]).click();
    assert.equal(await count.innerText(), `2 / ${total} selected`);
    assert.deepEqual(await Promise.all([0, 1, 2].map(index => rowBody(page, ids[index]).getAttribute("aria-checked"))), ["true", "true", "false"]);
    assert.equal(await page.getByTestId("selection-all").getAttribute("role"), "checkbox");
    assert.deepEqual(await Promise.all(["selection-takeover", "selection-takeover-auto", "selection-archive"].map(id => page.getByTestId(id).innerText())), ["Take over 2", "Auto", "Archive 2"]);
    assert(await noOverflow(page), `${width}-${theme}: selection overflows`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-selection.png`) });
    // A search that hides the selection keeps it: N counts only shown sessions, the rest are named under the header.
    await page.getByTestId("kontor-switch-search").fill("zz-fictional-no-match");
    assert.equal(await count.innerText(), "0 / 0 selected");
    assert.equal(await page.getByTestId("selection-hidden").innerText(), "2 selected sessions not shown by the current filters; they stay selected.");
    assert.equal(await page.getByTestId("selection-archive").innerText(), "Archive 2");
    await page.getByRole("button", { name: "Clear search", exact: true }).click();
    assert.equal(await count.innerText(), `2 / ${total} selected`);
    assert.equal(await page.getByTestId("selection-hidden").count(), 0);
    // D4: the review has no effect until Archive N; Cancel LEFT, the primary RIGHT.
    await page.getByTestId("selection-archive").click();
    await page.getByRole("dialog", { name: "Archive 2 sessions", exact: true }).waitFor();
    assert.match(await page.getByTestId("bulk-archive-confirm").innerText(), /^Archive [0-2]$/);
    assert(await leftOf(page, "bulk-archive-cancel", "bulk-archive-confirm"), "D4: Cancel LEFT, Archive N RIGHT");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-archive-review.png`) });
    await page.getByTestId("bulk-archive-cancel").click();
    assert.deepEqual(await page.evaluate(ids => ids.map(id => window.__kontorNavigatorFixture.seat(id).archivedAt), ids.slice(0, 2)), [null, null], "Cancel archives nothing");
    assert.equal(await count.innerText(), `2 / ${total} selected`);
    await page.getByTestId("selection-done").click();
    await page.getByTestId("selection-header").waitFor({ state: "detached" });
    assert.equal(await page.getByTestId("selection-bar").count(), 0);
    // A double tap on the state icon also enters selection, without opening the session.
    await page.getByTestId(`session-row-${ids[2]}`).getByTestId("session-row-state").dblclick();
    assert.equal(await count.innerText(), `1 / ${total} selected`);
    assert.equal(await page.getByTestId("host-agent-shim").count(), 0, "Double tap does not open the session");
    await page.getByTestId("selection-done").click();
    // C2: Copied is said only after the clipboard took it.
    await rowAction(page, ids[0], "summarize", page.getByTestId("kontor-switch-list"));
    const banner = page.getByTestId("summary-banner");
    await page.waitForFunction(() => /Copied|Not copied/.test(document.querySelector('[data-testid="summary-banner"]')?.textContent ?? ""));
    assert.match(await banner.innerText(), /^Copied to clipboard · Saved r\d+ · Source linked$/);
    assert(await leftOf(page, "summary-copy-again", "summary-attach"), "C2: Copy again LEFT, Attach RIGHT");
    assert(await noOverflow(page), `${width}-${theme}: summary overflows`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-summary.png`) });
    await page.getByRole("dialog", { name: "Session summary", exact: true }).getByRole("button", { name: "Close", exact: true }).click();
    await context.close();
    const denied = await open(width, theme, "unified-switcher", "&clipboard=denied");
    const first = (await switchRowIds(denied.page))[0];
    await rowAction(denied.page, first, "summarize", denied.page.getByTestId("kontor-switch-list"));
    await denied.page.waitForFunction(() => /Copied|Not copied/.test(document.querySelector('[data-testid="summary-banner"]')?.textContent ?? ""));
    assert.match(await denied.page.getByTestId("summary-banner").innerText(), /^Saved r\d+ · Not copied: the clipboard refused it \(SIMULATED: the clipboard refused the write\)\. Use Copy again, or select the text below\.$/);
    await denied.page.screenshot({ path: join(values.output, `${width}-${theme}-unified-summary-denied.png`) });
    results.push({ width, theme, unifiedSwitcher: "C1 order, D1 header/bar with frozen hidden selection, D4 review without effect, double-tap select, C2 copied and denied" });
    await denied.context.close();
  }
  for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
    // Board D3 on the SIMULATED sequence: five takeover-eligible seats draw success, no model, launch fails, unknown effect and refused.
    const { context, page } = await open(width, theme, "unified-takeover-outcomes");
    const shown = await page.getByRole("button", { name: /^Show the other \d+ of \d+$/ });
    if (await shown.count()) await shown.click();
    const eligible = await page.evaluate(ids => ids.filter(id => { const seat = window.__kontorNavigatorFixture.seat(id); return seat && !seat.archivedAt && seat.workspaceId && !["running", "initializing"].includes(seat.status); }), await switchRowIds(page));
    assert(eligible.length >= 5, `${width}-${theme}: five takeover-eligible captured seats (${eligible.length})`);
    const seats = eligible.slice(0, 5);
    await rowAction(page, seats[0], "select", page.getByTestId("kontor-switch-list"));
    for (const id of seats.slice(1)) await rowBody(page, id).click();
    assert.match(await page.getByTestId("selection-count").innerText(), /^5 \/ \d+ selected$/);
    assert.equal(await page.getByTestId("selection-takeover").innerText(), "Take over 5");
    await page.getByTestId("selection-takeover").click();
    const summary = page.getByTestId("takeover-results-summary");
    const settled = () => page.waitForFunction(() => !/in progress/.test(document.querySelector('[data-testid="takeover-results-summary"]')?.textContent ?? "in progress"), null, { timeout: 15000 });
    // Each held review lists the unreported-settings limit and is applied only by its own Confirm, one at a time.
    let held = 0;
    const confirmHeld = async () => {
      await settled();
      for (const id of await page.locator('[data-testid^="takeover-confirm-"]').evaluateAll(nodes => nodes.map(node => node.dataset.testid.slice(17)))) {
        assert.match(await page.getByTestId(`takeover-limits-${id}`).innerText(), UNREPORTED, `${width}-${theme}: R4b: a held review states the unreported settings`);
        if (!held++) await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-settings-unreported.png`) });
        await page.getByTestId(`takeover-confirm-${id}`).click();
        await page.getByTestId(`takeover-confirm-${id}`).waitFor({ state: "detached" });
        await settled();
      }
    };
    await confirmHeld();
    assert(held > 0, `${width}-${theme}: R4b: at least one captured seat's review was held for its unreported settings`);
    assert.equal(await summary.innerText(), "1 continued · 1 recovery pending · 1 not confirmed · 2 not started");
    for (const [index, id] of seats.entries()) assert((await page.getByTestId(`takeover-result-${id}`).innerText()).includes(TAKEOVER_LINES[index]), `${width}-${theme}: seat ${index + 1} reads ${TAKEOVER_LINES[index]}`);
    // R6: the seat with no available model answers the same again, so Retry remaining leaves it out.
    assert.equal(await page.getByTestId("takeover-results-retry").innerText(), "Retry remaining 3");
    assert(await leftOf(page, "takeover-results-close", "takeover-results-retry"), "D3: Close LEFT, Retry remaining RIGHT");
    assert.equal(await page.getByTestId("selection-header").count(), 0, "A started takeover ends the selection");
    assert(await noOverflow(page), `${width}-${theme}: takeover results overflow`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-takeover-results.png`) });
    const takeovers = () => page.evaluate(() => window.__kontorNavigatorFixture.takeover());
    if (theme === "light") {
      // A fresh review finishes the partial takeover, Check settles the unknown effect and the refused seat succeeds; no model stays kept.
      await page.getByTestId("takeover-results-retry").click();
      await page.waitForFunction(() => document.querySelector('[data-testid="takeover-results-summary"]')?.textContent !== "1 continued · 1 recovery pending · 1 not confirmed · 2 not started");
      await confirmHeld();
      assert.equal(await summary.innerText(), "4 continued · 1 not started");
      assert.equal(await page.getByTestId("takeover-results-retry").getAttribute("aria-label"), "Retry remaining 0. Nothing is left to retry.");
      await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-takeover-retried.png`) });
      await page.getByTestId("takeover-results-close").click();
      await page.getByTestId("takeover-results").waitFor({ state: "detached" });
      results.push({ width, theme, takeoverOutcomes: `D3 summary, per-seat lines, Retry remaining 3 settles 3; no model is not retried; ${held} held reviews stated unreported settings and were confirmed one by one` });
    } else {
      // R2: Close dismisses the sheet, not the takeover. The archived seat and the unconfirmed one come back from the
      // switcher and from Sessions, and each is settled by its own action: Check by its earlier review, Finish once.
      const [done, , recovery, unknown] = seats, before = await takeovers();
      const unknownReview = before.reviews.find(review => review.agentId === unknown).reviewId;
      await page.getByTestId("takeover-results-close").click();
      await page.getByTestId("takeover-results").waitFor({ state: "detached" });
      const switcher = page.getByTestId("kontor-switch-sheet");
      const notice = scope => scope.getByTestId("takeover-unfinished");
      assert.equal(await notice(switcher).innerText(), "Takeover unfinished: 1 recovery pending · 1 not confirmed · Open results");
      await page.waitForTimeout(400); // the sheet fades in
      await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-takeover-unfinished.png`) });
      // R2b: while those seats are unfinished, another seat's Take over and the selection bar's are refused where they
      // start, say why, and change nothing: no review, outcome or effect, and the unfinished seats keep their reviews.
      const switchList = switcher.getByTestId("kontor-switch-list"), other = seats[4];
      await switchList.getByTestId(`session-actions-${other}`).click();
      const otherMenu = page.getByRole("dialog", { name: "Session actions", exact: true });
      await otherMenu.waitFor();
      const single = otherMenu.getByTestId("session-action-takeover");
      assert.deepEqual([await single.getAttribute("aria-label"), await single.getAttribute("aria-disabled")], [`Take over. ${BUSY}`, "true"], "R2b: one seat's Take over is refused with its reason");
      await single.click({ force: true });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-single-refused.png`) });
      await otherMenu.getByRole("button", { name: "Close", exact: true }).click();
      await otherMenu.waitFor({ state: "detached" });
      await rowAction(page, other, "select", switchList);
      await switchList.getByTestId(`session-row-${seats[1]}`).getByTestId("session-row-body").click();
      assert.equal(await page.getByTestId("selection-takeover").getAttribute("aria-disabled"), "true", "R2b: the bulk Take over is refused");
      assert((await page.getByTestId("selection-bar").innerText()).includes(BUSY), "R2b: the selection bar says why");
      await page.getByTestId("selection-takeover").click({ force: true });
      await page.waitForTimeout(300);
      await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-bulk-refused.png`) });
      await page.getByTestId("selection-done").click();
      assert.deepEqual(await takeovers(), before, "R2b: no new review, outcome or effect after either attempt");
      assert.equal(await notice(switcher).innerText(), "Takeover unfinished: 1 recovery pending · 1 not confirmed · Open results", "R2b: the unfinished seats stay");
      await switcher.getByRole("button", { name: "Close start or switch", exact: true }).click();
      await switcher.waitFor({ state: "detached" });
      assert.equal(await notice(page).innerText(), "Takeover unfinished: 1 recovery pending · 1 not confirmed · Open results", "Sessions offers it too");
      await page.screenshot({ path: join(values.output, `${width}-${theme}-sessions-takeover-unfinished.png`) });
      await page.getByRole("button", { name: "Start or switch session", exact: true }).click();
      await notice(page.getByTestId("kontor-switch-sheet")).click();
      await page.getByTestId("takeover-results").waitFor();
      assert.equal(await summary.innerText(), "1 recovery pending · 1 not confirmed");
      assert.equal(await page.getByTestId("takeover-results-retry").innerText(), "Retry remaining 2");
      assert.deepEqual(await Promise.all([`takeover-finish-${recovery}`, `takeover-check-${unknown}`].map(id => page.getByTestId(id).isVisible())), [true, true]);
      await page.waitForTimeout(400); // the sheet fades in
      await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-takeover-reopened.png`) });
      await page.getByTestId(`takeover-check-${unknown}`).click();
      await page.waitForFunction(() => document.querySelector('[data-testid="takeover-results-summary"]')?.textContent === "1 continued · 1 recovery pending");
      await page.getByTestId(`takeover-finish-${recovery}`).click();
      await page.getByTestId(`takeover-finish-${recovery}`).waitFor({ state: "detached" });
      await confirmHeld();
      await page.waitForFunction(() => document.querySelector('[data-testid="takeover-results-summary"]')?.textContent === "2 continued");
      const after = await takeovers(), reviewsOf = id => after.reviews.filter(review => review.agentId === id).length;
      // Check read the unconfirmed seat by its earlier review, with no new review; the continued seat was never reviewed again.
      assert.deepEqual([reviewsOf(unknown), after.outcomes[unknownReview].state, reviewsOf(done)], [1, "succeeded", 1], "R2: Check uses the earlier review; continued seats are not redone");
      const successors = Object.values(after.outcomes).filter(outcome => outcome.successor?.agentId).map(outcome => [outcome.agentId, outcome.successor.agentId]);
      for (const id of [done, recovery, unknown]) assert.equal(new Set(successors.filter(([from]) => from === id).map(([, to]) => to)).size, 1, "R2: one successor per seat");
      await page.getByTestId("takeover-results-close").click();
      await page.getByTestId("takeover-results").waitFor({ state: "detached" });
      assert.equal(await notice(page).count(), 0, "Nothing unfinished remains");
      results.push({ width, theme, takeoverUnfinished: "Close keeps recovery and unconfirmed seats; another seat's and the bulk Take over are refused with the reason and change nothing; switcher and Sessions reopen them; Check by the earlier review, Finish once; one successor each" });
    }
    await context.close();
  }
  for (const [width, theme] of [[320, "light"], [390, "dark"], [1440, "light"]]) {
    // R5/R6 baseline and R1 on the captured switcher: what Take over does is written where it starts; a seat whose chain has
    // no route on its own provider is refused before any effect.
    const { context, page } = await open(width, theme, "unified-switcher");
    const list = page.getByTestId("kontor-switch-list");
    await pickState(page, "all");
    const more = page.getByRole("button", { name: /^Show the other \d+ of \d+$/ });
    if (await more.count()) await more.click();
    const ids = await switchRowIds(page);
    const seatOf = id => page.evaluate(id => window.__kontorNavigatorFixture.seat(id), id);
    const idle = [];
    for (const id of ids) { const seat = await seatOf(id); if (seat && !seat.archivedAt && seat.workspaceId && seat.status === "idle") idle.push([id, seat]); }
    // The captured seat on a second Claude account is found by what it is, not by its account's name.
    const [account, accountSeat] = idle.find(([, seat]) => seat.provider.startsWith("claude-") && seat.currentModeId === "bypassPermissions") ?? [];
    const claude = idle.find(([, seat]) => seat.provider === "claude")?.[0];
    assert(account && claude, `${width}-${theme}: the captured cohort has an idle bypassPermissions seat on a second Claude account and an idle claude seat`);
    await list.getByTestId(`session-actions-${claude}`).click();
    const menu = page.getByRole("dialog", { name: "Session actions", exact: true });
    const said = await menu.innerText();
    assert(said.includes("Saves a compact handoff, archives this session, then starts a new one that continues the task.") && said.includes("SIMULATED: no provider, account or Paseo session is contacted"), "R5: the menu states what Take over does and the host's limit");
    assert(!/Same permissions|Same personality|Same place/.test(said), "R5: no blanket same-permissions claim");
    await page.waitForTimeout(400); // the sheet fades in
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-disclosure.png`) });
    // R1, manual: a Codex model for a Claude seat cannot keep its permissions, so the picker refuses it.
    await menu.getByTestId("session-action-takeover-option").click();
    await page.getByTestId("kontor-model-picker").waitFor();
    await page.getByTestId("kontor-model-vendor-anthropic").click();
    await page.locator('[data-testid^="kontor-model-row-claude/"]').first().click();
    assert.match(await page.getByTestId("kontor-model-permissions-mode").innerText(), /^Bypass( permissions)? ⚠ · Claude/, "R1: the seat's own mode on its own provider, read-only");
    await page.getByTestId("kontor-model-problem").waitFor({ state: "detached" });
    assert.notEqual(await page.getByTestId("kontor-model-use").getAttribute("aria-disabled"), "true", "R1: its own provider keeps it");
    await page.getByTestId("kontor-model-vendor-openai").click();
    await page.locator('[data-testid^="kontor-model-row-codex/"]').first().click();
    await page.getByTestId("kontor-model-problem").waitFor();
    assert.match(await page.getByTestId("kontor-model-problem").innerText(), /cannot keep this seat's permissions \(bypassPermissions on claude\): no permission mapping between providers is qualified\.$/);
    assert.equal(await page.getByTestId("kontor-model-use").getAttribute("aria-disabled"), "true");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-picker-refused.png`) });
    await page.getByRole("button", { name: "Cancel model choice", exact: true }).click();
    await page.getByTestId("kontor-model-picker").waitFor({ state: "detached" });
    // R1, Auto: the second Claude account's provider lists no model, so the seat's chain is only the Codex default, which cannot keep bypassPermissions.
    await rowAction(page, account, "takeover", list);
    await page.waitForFunction(() => !/in progress/.test(document.querySelector('[data-testid="takeover-results-summary"]')?.textContent ?? "in progress"), null, { timeout: 15000 });
    const result = page.getByTestId(`takeover-result-${account}`);
    assert((await result.innerText()).includes("Not started · Original kept"), "R1: refused, original kept");
    assert((await result.getAttribute("aria-label")).includes(`Codex cannot keep this seat's permissions (bypassPermissions on ${accountSeat.provider})`), "R1: the reason names the permissions");
    const fixture = await page.evaluate(() => window.__kontorNavigatorFixture.takeover());
    assert.deepEqual([(await seatOf(account)).archivedAt ?? null, Object.values(fixture.outcomes).filter(outcome => outcome.agentId === account).length], [null, 0], "R1: no archive, no apply");
    assert.equal(await page.getByTestId("takeover-results-retry").getAttribute("aria-label"), "Retry remaining 0. Nothing is left to retry.", "R6: a refusal is not retried");
    // R6b: the shown text, not only the spoken label, carries the inherited mode, the refusal and why Retry skips it, with no line clipped.
    const shown = await result.innerText();
    assert(shown.includes(`(bypassPermissions on ${accountSeat.provider}): no permission mapping between providers is qualified.`) && shown.includes("Retry remaining skips it: asking again gets the same answer."), "R6b: the whole reason is shown");
    assert(await result.evaluate(node => [...node.querySelectorAll("div[dir]")].every(text => text.scrollHeight <= text.clientHeight + 1)), "R6b: no reason line is clipped");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-permissions-refused.png`) });
    await page.getByTestId("takeover-results-close").click();
    results.push({ width, theme, takeoverPermissions: "menu discloses effects and limit; picker refuses Codex for a Claude seat; Auto refuses a second Claude account's seat→Codex with no archive or apply; the whole reason is shown unclipped" });
    await context.close();
  }
  for (const [width, theme] of [[320, "light"], [390, "dark"], [1440, "dark"]]) {
    // R5: a ready review stating a limit nobody was shown waits as Review needed with nothing done; confirming that exact review continues.
    const { context, page } = await open(width, theme, "unified-switcher", "&takeoverSim=limited");
    const list = page.getByTestId("kontor-switch-list");
    const [id] = await page.evaluate(ids => ids.filter(id => { const seat = window.__kontorNavigatorFixture.seat(id); return seat && !seat.archivedAt && seat.workspaceId && seat.status === "idle" && seat.provider === "codex"; }), await switchRowIds(page));
    assert(id, `${width}-${theme}: an idle Codex seat in Last visited`);
    await rowAction(page, id, "takeover", list);
    const result = page.getByTestId(`takeover-result-${id}`);
    await page.getByTestId(`takeover-confirm-${id}`).waitFor();
    assert((await result.innerText()).includes("Review needed · Original kept"));
    const limits = await page.getByTestId(`takeover-limits-${id}`).innerText();
    assert.match(limits, UNREPORTED, "R4b: the captured Codex seat's unreported settings are stated");
    assert.deepEqual(limits.split("\n").filter(line => !UNREPORTED.test(line)), ["• SIMULATED: this seat's supervisor is not told about the new session; tell it yourself."]);
    const held = await page.evaluate(id => window.__kontorNavigatorFixture.takeover().reviews.filter(review => review.agentId === id), id);
    assert.deepEqual([held.length, await page.evaluate(id => window.__kontorNavigatorFixture.seat(id).archivedAt ?? null, id), Object.keys(await page.evaluate(() => window.__kontorNavigatorFixture.takeover().outcomes)).length], [1, null, 0], "R5: held with nothing applied");
    assert(await noOverflow(page), `${width}-${theme}: review needed overflows`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-review-needed.png`) });
    await page.getByTestId(`takeover-confirm-${id}`).click();
    await page.waitForFunction(() => document.querySelector('[data-testid="takeover-results-summary"]')?.textContent === "1 continued");
    const after = await page.evaluate(() => window.__kontorNavigatorFixture.takeover());
    assert.deepEqual([after.reviews.filter(review => review.agentId === id).length, after.outcomes[held[0].reviewId]?.state], [1, "succeeded"], "R5: the exact held review was confirmed");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-review-confirmed.png`) });
    await page.getByTestId("takeover-results-close").click();
    results.push({ width, theme, takeoverReviewNeeded: "undisclosed limit held with zero effects; Confirm applies that exact review" });
    await context.close();
  }
  for (const [width, theme] of [[390, "dark"], [1440, "light"]]) {
    // R3 and R4: the picker keeps what the next model lists; a Fast+Plan lead taken over with Auto stores and reads back Fast, Plan and its effort.
    const { context, page } = await open(width, theme, "unified-switcher");
    await page.getByTestId("kontor-switch-new-lead").click();
    const checked = async (role, name) => await page.getByRole(role, { name, exact: true }).getAttribute("aria-checked") === "true";
    const picked = async name => await page.getByRole("radiogroup", { name, exact: true }).getByRole("radio", { checked: true }).getAttribute("aria-label");
    const shown = async () => [await picked("Effort"), await picked("Speed"), await checked("switch", "Plan"), await page.getByTestId("kontor-model-permissions").getAttribute("aria-label")];
    // R11: Reset on the model already checked restores the Codex default's effort, settings and permissions; re-tapping it keeps what was set.
    assert.equal(await page.getByTestId("kontor-new-lead-model-tag").innerText(), "Codex default");
    await page.getByTestId("kontor-new-lead-model").click();
    await page.getByTestId("kontor-model-picker").waitFor();
    const codexRow = await page.locator('[data-testid^="kontor-model-row-"][aria-checked="true"]').getAttribute("data-testid");
    await page.getByRole("radiogroup", { name: "Speed", exact: true }).waitFor();
    const codexDefault = await shown();
    await page.getByRole("radiogroup", { name: "Effort", exact: true }).getByRole("radio", { checked: false }).first().click();
    await page.getByRole("radio", { name: "Fast", exact: true }).click();
    await page.getByRole("switch", { name: "Plan", exact: true }).click();
    await page.getByTestId("kontor-model-permissions").click();
    await page.getByRole("radio", { name: "Full Access ⚠", exact: true }).click();
    const set = await shown();
    assert(set.every((value, index) => value !== codexDefault[index]), `R11: every setting moved off the Codex default: ${JSON.stringify([codexDefault, set])}`);
    await page.getByTestId(codexRow).click();
    await page.waitForTimeout(300);
    assert.deepEqual(await shown(), set, "R11: re-tapping the checked model keeps its settings");
    await page.getByTestId("kontor-model-reset").click();
    await page.waitForTimeout(300);
    assert.deepEqual(await shown(), codexDefault, "R11: Reset on the same model restores the Codex default's settings");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-picker-reset-same-model.png`) });
    await page.getByTestId("kontor-model-use").click();
    await page.getByTestId("kontor-model-picker").waitFor({ state: "detached" });
    assert.equal(await page.getByTestId("kontor-new-lead-model-tag").innerText(), "Codex default", "R11: Use applies the Codex default");
    await page.getByTestId("kontor-new-lead-model").click();
    await page.getByTestId("kontor-model-picker").waitFor();
    await page.getByTestId("kontor-model-row-codex/gpt-6-astra").click();
    await page.getByRole("radiogroup", { name: "Effort" }).getByRole("radio", { name: /^high$/i }).click();
    await page.getByRole("radio", { name: "Fast", exact: true }).click();
    if (!await checked("switch", "Plan")) await page.getByRole("switch", { name: "Plan", exact: true }).click();
    await page.getByTestId("kontor-model-row-codex/gpt-6.1-sol").click();
    await page.waitForTimeout(300);
    assert.deepEqual([await page.getByRole("radiogroup", { name: "Effort" }).getByRole("radio", { name: /^high$/i }).getAttribute("aria-checked"), await checked("radio", "Fast"), await checked("switch", "Plan")], ["true", true, true], "R3: GPT-6.1 Sol keeps High, Fast and Plan");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-picker-kept-settings.png`) });
    await page.getByTestId("kontor-model-vendor-anthropic").click();
    await page.locator('[data-testid^="kontor-model-row-claude/"]').first().click();
    await page.getByTestId("kontor-model-changed").waitFor();
    assert.match(await page.getByTestId("kontor-model-changed").innerText(), /does not offer Speed Fast, Plan on, so they are not applied\./, "R3: what the new model lacks is said");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-picker-dropped-settings.png`) });
    await page.getByTestId("kontor-model-vendor-openai").click();
    await page.getByTestId("kontor-model-row-codex/gpt-6-astra").click();
    await page.waitForTimeout(300);
    if (!await checked("radio", "Fast")) await page.getByRole("radio", { name: "Fast", exact: true }).click();
    if (!await checked("switch", "Plan")) await page.getByRole("switch", { name: "Plan", exact: true }).click();
    await page.getByRole("radiogroup", { name: "Effort" }).getByRole("radio", { name: /^high$/i }).click();
    await page.getByTestId("kontor-model-use").click();
    await page.getByTestId("kontor-model-picker").waitFor({ state: "detached" });
    await page.getByTestId("kontor-new-lsa-topic").fill("Fictional Fast Plan lead");
    await page.getByTestId("kontor-new-lsa-create").click();
    await page.getByTestId("kontor-new-lsa-verified").waitFor();
    const created = (await page.getByTestId("kontor-new-lsa-verified").innerText()).match(/Session ID: (agt_fixture_create_\S+)/)?.[1];
    assert(created, "the created seat's ID is shown");
    const features = id => page.evaluate(id => { const seat = window.__kontorNavigatorFixture.seat(id); return [seat.effectiveThinkingOptionId, Object.fromEntries((seat.features ?? []).map(feature => [feature.id, feature.value]))]; }, id);
    assert.deepEqual(await features(created), ["high", { service_tier: "priority", plan_mode: true }], "the lead stores Fast, Plan and High");
    await page.getByRole("button", { name: "Done", exact: true }).click();
    await pickState(page, "all");
    await page.getByTestId("kontor-switch-search").fill("Fictional Fast Plan lead");
    await rowAction(page, created, "takeover", page.getByTestId("kontor-switch-list"));
    await page.waitForFunction(() => document.querySelector('[data-testid="takeover-results-summary"]')?.textContent === "1 continued", null, { timeout: 15000 });
    const outcome = Object.values(await page.evaluate(() => window.__kontorNavigatorFixture.takeover().outcomes)).find(entry => entry.agentId === created);
    assert.match(outcome.message, /Settings read back: Speed Fast, Plan on\.$/, "R4: the successor's stored seat reads back Fast and Plan");
    assert.deepEqual(await features(outcome.successor.agentId), ["high", { service_tier: "priority", plan_mode: true }], "R4: the successor stores Fast, Plan and High");
    await page.screenshot({ path: join(values.output, `${width}-${theme}-takeover-kept-settings.png`) });
    await page.getByTestId("takeover-results-close").click();
    results.push({ width, theme, resetSameModel: `re-tap keeps ${JSON.stringify(set)}; Reset restores and Use applies ${JSON.stringify(codexDefault)}`,
      savedSettings: "picker keeps High/Fast/Plan on GPT-6.1 Sol and says what Claude drops; Fast+Plan lead → Auto → successor reads back Fast, Plan, High" });
    await context.close();
  }
  for (const [width, theme] of [[390, "dark"], [1440, "light"]]) {
    // R9: one pending open per list. A state tap on A, then B's menu by pointer or by keyboard inside the double-tap
    // window, never opens A; a lone state tap still opens after the window. Real timers, measured.
    const { context, page } = await open(width, theme, "unified-switcher");
    const ids = await switchRowIds(page);
    const list = page.getByTestId("kontor-switch-list"), state = id => list.getByTestId(`session-row-${id}`).getByTestId("session-row-state");
    const menu = page.getByRole("dialog", { name: "Session actions", exact: true });
    const gaps = [];
    for (const keyboard of [false, true]) {
      const start = Date.now();
      await state(ids[0]).click();
      if (keyboard) { await list.getByTestId(`session-actions-${ids[1]}`).focus(); await page.keyboard.press("Enter"); }
      else await list.getByTestId(`session-actions-${ids[1]}`).click();
      gaps.push(Date.now() - start);
      await menu.waitFor();
      await page.waitForTimeout(600);
      assert.equal(await page.getByTestId("host-agent-shim").count(), 0, `R9: A does not open after B's menu (${keyboard ? "keyboard" : "pointer"})`);
      await menu.getByRole("button", { name: "Close", exact: true }).click();
      await menu.waitFor({ state: "detached" });
    }
    assert(gaps.every(gap => gap < 280), `R9: B's menu came inside the 280 ms window (${gaps.join(", ")} ms)`);
    // R9b: a search that keeps A listed drops A's waiting open; the same row node stays mounted and nothing opens.
    const initial = id => page.evaluate(id => window.__kontorNavigatorFixture.seat(id).title.match(/[\p{L}\p{N}]/u)[0].toLowerCase(), id);
    const searched = async (row, field) => {
      const query = await initial(row.id);
      await row.state.evaluate(node => { node.dataset.r9b = "kept"; });
      const start = Date.now();
      await row.state.click();
      await field.fill(query);
      const gap = Date.now() - start;
      await page.waitForTimeout(600);
      assert(gap < 280, `R9b: the search came inside the 280 ms window (${gap} ms)`);
      assert.equal(await page.getByTestId("host-agent-shim").count(), 0, "R9b: A does not open after a search");
      assert.equal(await row.list.locator('[data-r9b="kept"]').count(), 1, "R9b: A's row stayed mounted");
      await field.fill("");
      return gap;
    };
    gaps.push(await searched({ id: ids[0], state: state(ids[0]), list }, page.getByTestId("kontor-switch-search")));
    if (width === 1440) {
      await page.getByRole("button", { name: "Close start or switch", exact: true }).click();
      await page.getByTestId("kontor-switch-sheet").waitFor({ state: "detached" });
      // With the switcher closed, Kontor's Sessions is the only list on the page.
      const first = page.getByTestId("session-row-state").first();
      const id = await first.evaluate(node => node.parentElement.closest('[data-testid^="sessions-row-"]').dataset.testid.slice(13));
      gaps.push(await searched({ id, state: first, list: page }, page.getByTestId("sessions-search")));
      await page.getByRole("button", { name: "Start or switch session", exact: true }).click();
      await page.getByTestId("kontor-switch-sheet").waitFor();
    }
    await state(ids[0]).click();
    await page.getByTestId("host-agent-shim").waitFor({ timeout: 2000 });
    results.push({ width, theme, delayedOpen: `A stays closed after B's menu by pointer and keyboard, and after a search that keeps its row mounted${width === 1440 ? " in the switcher and Sessions" : ""} (${gaps.join(", ")} ms after A); a lone tap opens` });
    await context.close();
  }
  {
    // R10: Attach never replaces a destination's draft. The same revision again changes nothing; another is refused.
    const { context, page } = await open(390, "dark", "unified-switcher");
    const list = page.getByTestId("kontor-switch-list");
    const [a, b, c] = await switchRowIds(page);
    const attach = async (source, destination) => {
      await rowAction(page, source, "attach", list);
      const sheet = page.getByTestId("attach-sheet");
      await sheet.waitFor();
      await sheet.getByTestId(`attach-destination-${destination}`).getByTestId("session-row-body").click();
      await sheet.getByTestId("attach-confirm").click();
      await sheet.getByTestId("attach-result").waitFor();
      const text = await sheet.getByTestId("attach-result").innerText();
      await page.screenshot({ path: join(values.output, `390-dark-attach-${source === a ? "a" : "c"}-${Date.now() % 1000}.png`) });
      await page.getByRole("dialog", { name: "Attach summary", exact: true }).getByRole("button", { name: "Close", exact: true }).first().click();
      await sheet.waitFor({ state: "detached" });
      return text;
    };
    assert(/^SIMULATED: r\d+ is in the draft of “.+” for review\./.test(await attach(a, b)), "R10: the first attach drafts");
    const drafted = await page.evaluate(() => window.__kontorNavigatorFixture.inspect().drafts);
    assert(/^SIMULATED: r\d+ is already in the draft of “.+”; nothing changed and nothing was sent\.$/.test(await attach(a, b)), "R10: the same revision again changes nothing");
    assert(/already has a draft \(r\d+ of .+\)\. Attach never replaces a draft; send or clear it there first\. Nothing was drafted\.$/.test(await attach(c, b)), "R10: another summary is refused");
    assert.deepEqual(await page.evaluate(() => window.__kontorNavigatorFixture.inspect().drafts), drafted, "R10: the draft is unchanged");
    results.push({ attach: "first drafts, the same revision again changes nothing, another is refused; the draft is unchanged" });
    await context.close();
  }
  for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
    // One shared state toolbar in Start or switch and in Sessions: whole tiles from the row's measured width, More and the trailing
    // control pinned; More reaches all nine; a chosen state stays visible through a resize; Kontor gets the state and the search.
    const { context, page } = await open(width, theme, "unified-switcher");
    const label = `${width}-${theme}`;
    const first = await stateRow(page, "kontor-switch-states", "kontor-switch-state-", `${label} switcher`, SWITCH_PINNED);
    assert.deepEqual(first.tiles, NINE.slice(0, first.tiles.length), `${label}: the agreed order`);
    await page.screenshot({ path: join(values.output, `${label}-toolbar-switcher.png`) });
    await page.getByTestId("kontor-switch-more").click();
    const panel = page.getByTestId("kontor-switch-more-panel");
    assert.deepEqual(await panel.getByRole("group", { name: "All states", exact: true }).evaluate(group => [...group.querySelectorAll('[role="button"]')].map(node => node.dataset.testid.slice(19))), NINE, `${label}: More lists all nine`);
    await page.screenshot({ path: join(values.output, `${label}-toolbar-more.png`) });
    await page.getByTestId("kontor-switch-more-favorites").click();
    await panel.waitFor({ state: "detached" });
    await page.getByTestId("kontor-switch-search").fill("a");
    const chosen = await stateRow(page, "kontor-switch-states", "kontor-switch-state-", `${label} favorites`, SWITCH_PINNED);
    assert.deepEqual([chosen.tiles.length, chosen.tiles.at(-1), chosen.pressed], [first.tiles.length, "favorites", ["favorites"]], `${label}: Favorites takes the last tile`);
    assert(chosen.labels.every(text => /, \d+ of \d+ sessions after refinements, /.test(text)), `${label}: every tile states its filtered ratio`);
    if (width < 500) {
      for (const next of [width === 320 ? 390 : 320, width]) {
        await page.setViewportSize({ width: next, height: 844 });
        await page.waitForTimeout(300);
        const resized = await stateRow(page, "kontor-switch-states", "kontor-switch-state-", `${label} at ${next}`, SWITCH_PINNED);
        assert.deepEqual([resized.pressed, resized.tiles.at(-1), await page.getByTestId("kontor-switch-search").inputValue()], [["favorites"], "favorites", "a"], `${label}: a resize keeps the state and the search`);
      }
    }
    await page.screenshot({ path: join(values.output, `${label}-toolbar-switcher-favorites.png`) });
    await page.getByTestId("kontor-switch-kontor").click();
    await page.getByTestId("kontor-switch-states").waitFor({ state: "detached" });
    assert.equal(await sessionsQuery(page), "a", `${label}: Kontor gets the search`);
    if (width < 500) {
      const sessions = await stateRow(page, "sessions-mobile-quick-row", "sessions-quick-", `${label} Sessions`, SESSIONS_PINNED);
      assert.deepEqual([sessions.tiles.length, sessions.pressed], [first.tiles.length, ["favorites"]], `${label}: Sessions draws as many tiles and gets the state`);
      await page.screenshot({ path: join(values.output, `${label}-toolbar-sessions.png`) });
    } else assert.equal(await page.getByTestId("sessions-filter-favorites").getAttribute("aria-pressed"), "true", `${label}: Kontor gets the state`);
    results.push({ width, theme, stateToolbar: `${first.tiles.length} whole tiles in ${Math.round(first.width)}; More lists nine; Favorites promoted and kept through resize; Kontor gets state and search` });
    await context.close();
  }
  for (const [width, theme] of [[320, "light"], [320, "dark"], [390, "light"], [390, "dark"], [1440, "light"]]) {
    // Effort levels are each route's own on the captured catalog: the Selected group keeps the chosen model while another vendor
    // is browsed; DeepSeek and GLM via OpenCode list four, GLM Agent six and Haiku none; a level the next model lists is kept,
    // one it lacks is said; the list never moves; Use and Create store exactly the route, model and level chosen.
    const { context, page } = await open(width, theme, "unified-switcher");
    const label = `${width}-${theme}`;
    await page.getByTestId("kontor-switch-new-lead").click();
    await page.getByTestId("kontor-new-lead-model").click();
    await page.getByTestId("kontor-model-picker").waitFor();
    const effort = () => page.getByRole("radiogroup", { name: "Effort", exact: true });
    const levels = async () => await effort().count() ? effort().getByRole("radio").evaluateAll(radios => radios.map(radio => `${radio.getAttribute("aria-label")}${radio.getAttribute("aria-checked") === "true" ? " ✓" : ""}`)) : null;
    const row = name => page.getByRole("radio", { name, exact: true });
    const changed = async () => await page.getByTestId("kontor-model-changed").count() ? page.getByTestId("kontor-model-changed").innerText() : null;
    // Nothing conditional sits between the Selected group and the list: the list moves only by the group, whose level row wraps
    // where a route's levels do not fit one row; the explanation is below the list.
    const spans = [];
    const choose = async name => {
      await row(name).click();
      const [group, list] = await Promise.all([page.getByTestId("kontor-model-chosen").boundingBox(), page.getByTestId("kontor-model-list").boundingBox()]);
      spans.push({ name: name.split(",")[0] + name.slice(name.lastIndexOf(",")), group: Math.round(group.height), gap: Math.round(list.y - group.y - group.height) });
    };
    await page.getByTestId("kontor-model-vendor-anthropic").click();
    await choose("Opus 5.5, claude-opus-5-5, via Claude");
    // The lead's starting level is kept when Opus lists it, so only the six names and one choice are fixed here.
    const opus = await levels();
    assert.deepEqual([opus.map(text => text.replace(" ✓", "")), opus.filter(text => text.endsWith(" ✓")).length], [["Low", "Medium", "High", "Extra High", "Max", "Ultra Code"], 1], `${label}: Opus 5.5 via Claude`);
    await effort().getByRole("radio", { name: "Extra High", exact: true }).click();
    const selected = await page.getByTestId("kontor-model-selected").innerText();
    assert.match(selected, /Opus 5\.5[\s\S]*via Claude[\s\S]*Selected/, `${label}: the Selected group names the model and its route`);
    await page.getByTestId("kontor-model-vendor-deepseek").click();
    assert.deepEqual([await page.getByTestId("kontor-model-selected").innerText(), await levels()], [selected, ["Low", "Medium", "High", "Extra High ✓", "Max", "Ultra Code"]], `${label}: browsing changes nothing chosen`);
    assert.deepEqual([await page.getByTestId("kontor-model-vendor-deepseek").getAttribute("aria-selected"), await page.getByTestId("kontor-model-search").getAttribute("placeholder"),
      await page.locator('[data-testid^="kontor-model-row-"][aria-checked="true"]').count()], ["true", "Search DeepSeek models…", 0], `${label}: DeepSeek is browsed, not chosen`);
    assert.match(await page.getByTestId("kontor-model-vendor-anthropic").getAttribute("aria-label"), /, has the selected model$/);
    await page.screenshot({ path: join(values.output, `${label}-picker-browsed.png`) });
    await choose("DeepSeek V4 Pro, deepseek/deepseek-v4-pro, via OpenCode");
    assert.deepEqual([await levels(), await changed()], [["Default ✓", "low", "high", "max"], "DeepSeek V4 Pro does not offer effort Extra High; it uses Default."], `${label}: DeepSeek via OpenCode`);
    await page.screenshot({ path: join(values.output, `${label}-picker-deepseek-opencode.png`) });
    await effort().getByRole("radio", { name: "high", exact: true }).click();
    await page.getByTestId("kontor-model-vendor-zai").click();
    await choose("GLM-5.3, glm-5.3, via GLM Agent");
    assert.deepEqual([await levels(), await changed()], [["Minimal", "Low", "Medium", "High ✓", "X-High", "Max"], null], `${label}: GLM Agent keeps high`);
    await effort().getByRole("radio", { name: "X-High", exact: true }).click();
    await choose("GLM-5.3, zai-coding-plan/glm-5.3, via OpenCode");
    assert.deepEqual([await levels(), await changed()], [["Default ✓", "low", "high", "max"], "GLM-5.3 does not offer effort X-High; it uses Default."], `${label}: GLM via OpenCode`);
    await page.screenshot({ path: join(values.output, `${label}-picker-glm-routes.png`) });
    await page.getByTestId("kontor-model-vendor-anthropic").click();
    await choose("Haiku 4.5, claude-haiku-4-5, via Claude");
    assert.deepEqual([await levels(), await page.getByText("No effort setting for this model.", { exact: true }).isVisible(), await changed()], [null, true, "Haiku 4.5 does not offer effort Default; it uses no effort setting."], `${label}: Haiku lists no level`);
    await page.screenshot({ path: join(values.output, `${label}-picker-no-effort.png`) });
    assert.equal(new Set(spans.map(span => span.gap)).size, 1, `${label}: only the Selected group moves the list`);
    await page.getByTestId("kontor-model-vendor-zai").click();
    await choose("GLM-5.3, zai-coding-plan/glm-5.3, via OpenCode");
    await effort().getByRole("radio", { name: "high", exact: true }).click();
    const use = await page.getByTestId("kontor-model-use").boundingBox(), viewport = page.viewportSize();
    assert(use.y + use.height <= viewport.height && use.x + use.width <= viewport.width, `${label}: Use is in reach`);
    await page.getByTestId("kontor-model-use").click();
    await page.getByTestId("kontor-model-picker").waitFor({ state: "detached" });
    await page.getByTestId("kontor-new-lsa-topic").fill("Fictional route effort lead");
    await page.getByTestId("kontor-new-lsa-create").click();
    await page.getByTestId("kontor-new-lsa-verified").waitFor();
    const created = (await page.getByTestId("kontor-new-lsa-verified").innerText()).match(/Session ID: (agt_fixture_create_\S+)/)?.[1];
    assert.deepEqual(await page.evaluate(id => { const seat = window.__kontorNavigatorFixture.seat(id); return [seat.provider, seat.model, seat.effectiveThinkingOptionId]; }, created),
      ["opencode", "zai-coding-plan/glm-5.3", "high"], `${label}: the created seat reads back the route, model and level`);
    results.push({ width, theme, spans, routeEffort: "Opus six levels kept while DeepSeek browsed; DeepSeek/GLM via OpenCode four; GLM Agent six keeps high; Haiku none; created seat reads back opencode zai-coding-plan/glm-5.3 high" });
    await context.close();
  }
  for (const [width, theme] of [[1440, "light"], [1440, "dark"], [700, "dark"], [320, "light"], [320, "dark"], [390, "light"], [390, "dark"]]) {
    // The composer pill's registered popover in host chrome after Paseo's source: anchored above the pill on a wide window with no
    // dimmer or second heading, a sheet titled by the host below 600; Escape and the backdrop close it and focus returns to the pill.
    const { context, page } = await open(width, theme, "unified-switcher");
    const label = `${width}-${theme}`, sheet = width < 600;
    const pill = await composerPill(page), title = await pill.getAttribute("aria-label");
    const surface = page.getByTestId(sheet ? "host-popover-sheet" : "host-popover");
    const place = async () => {
      const [anchor, box, viewport] = [await pill.boundingBox(), await surface.boundingBox(), page.viewportSize()];
      if (sheet) assert(Math.abs(box.width - viewport.width) < 1 && Math.abs(box.y + box.height - viewport.height) < 1 && box.height <= viewport.height * 0.8 + 1, `${label}: a bottom sheet up to 80%`);
      // The source places the measured content, inside the surface's 1-point border, 12 above the pill, so the border lands 10 above.
      else assert(Math.abs(box.y + box.height - (anchor.y - 10)) < 1.5 && Math.abs(box.x - Math.min(anchor.x, viewport.width - box.width - 8)) < 1.5 && box.width >= 280 && box.width <= 420 && box.height <= 442,
        `${label}: 12 above the pill at its start, 280 to 420 wide, at most 440 tall: ${JSON.stringify([anchor, box])}`);
      return box;
    };
    await pill.click();
    await surface.waitFor();
    await page.waitForTimeout(300);
    const box = await place();
    assert.equal(await page.getByTestId("host-popover-backdrop").evaluate(node => getComputedStyle(node).backgroundColor), sheet ? "rgba(0, 0, 0, 0.45)" : "rgba(0, 0, 0, 0)", `${label}: dimmed only as a sheet`);
    // The sheet's host title replaces the body's heading; the anchored popover has no host heading and the body keeps its own.
    assert.deepEqual([sheet ? await page.getByTestId("host-popover-sheet-title").innerText() : null, await surface.getByRole("heading", { name: "Start or switch session", exact: true }).count(),
      await surface.getByRole("button", { name: "Close", exact: true }).count()], [sheet ? title : null, sheet ? 0 : 1, 0], `${label}: one heading, no host close row`);
    const strip = await stateRow(page, "kontor-switch-states", "kontor-switch-state-", `${label} composer`, SWITCH_PINNED);
    assert(inside(await page.getByTestId("kontor-switch-states").boundingBox(), box), `${label}: the toolbar fits the surface`);
    await page.screenshot({ path: join(values.output, `${label}-composer-popover.png`) });
    await page.keyboard.press("Escape");
    await surface.waitFor({ state: "detached" });
    assert.match(await page.evaluate(() => document.activeElement?.dataset?.testid ?? ""), /^host-pill-/, `${label}: Escape returns focus to the pill`);
    await pill.click();
    await surface.waitFor();
    await page.mouse.click(page.viewportSize().width - 4, sheet ? 60 : 4);
    await surface.waitFor({ state: "detached" });
    if (!sheet) {
      // A shorter window moves the pill; the surface follows it.
      await pill.click();
      await surface.waitFor();
      await page.setViewportSize({ width, height: 700 });
      await page.waitForTimeout(300);
      await place();
      await page.setViewportSize({ width, height: 900 });
      await page.keyboard.press("Escape");
      await surface.waitFor({ state: "detached" });
    }
    // New lead, its source chooser and the model picker stay reachable inside the surface's own bounds.
    const reach = async (id, name) => {
      const target = page.getByTestId(id);
      // The picker settles its list height after its first layout, so scroll again once it has.
      await target.scrollIntoViewIfNeeded();
      await page.waitForTimeout(300);
      await target.scrollIntoViewIfNeeded();
      assert(inside(await target.boundingBox(), await surface.boundingBox()), `${label}: ${id} is reachable in the surface`);
      await page.screenshot({ path: join(values.output, `${label}-composer-${name}.png`) });
    };
    await pill.click();
    await page.getByTestId("kontor-switch-new-lead").click();
    await reach("kontor-new-lsa-create", "new-lead");
    await page.getByRole("button", { name: "Choose source session", exact: true }).click();
    await reach("kontor-lead-source-use", "source");
    await page.keyboard.press("Escape");
    await surface.waitFor({ state: "detached" });
    await pill.click();
    await page.getByTestId("kontor-switch-new-lead").click();
    await page.getByTestId("kontor-new-lead-model").click();
    await reach("kontor-model-use", "picker");
    await page.keyboard.press("Escape");
    await surface.waitFor({ state: "detached" });
    // Open in Kontor leaves the popover and opens Sessions with its state and search.
    await pill.click();
    await page.getByTestId("kontor-switch-search").fill("a");
    await page.getByTestId("kontor-switch-kontor").click();
    await surface.waitFor({ state: "detached" });
    await page.getByTestId("host-agent-shim").waitFor({ state: "detached" });
    assert.equal(await sessionsQuery(page), "a", `${label}: Kontor gets the search`);
    results.push({ width, theme, composerPopover: `${sheet ? "sheet titled by the host" : `anchored ${Math.round(box.width)}x${Math.round(box.height)} 12 above the pill`}; ${strip.tiles.length} tiles; Escape, backdrop, focus return; New lead, source, picker reachable; Open in Kontor carries the search` });
    await context.close();
  }
  for (const [width, height, theme] of [[320, 640, "light"], [320, 640, "dark"], [320, 844, "light"], [320, 844, "dark"], [390, 844, "dark"], [1440, 900, "light"]]) {
    // Codex's six levels keep one row beside the selected model in New lead from Kontor and from the composer, and in Take over;
    // Use, Cancel and Take over's permission refusal stay reachable.
    const label = `${width}x${height}-${theme}`, rows = {};
    const page = async () => { const opened = await open(width, theme, "unified-switcher"); await opened.page.setViewportSize({ width, height }); return opened; };
    const reach = async (current, id, within) => {
      const target = current.getByTestId(id);
      await target.scrollIntoViewIfNeeded();
      const box = await target.boundingBox(), frame = within ? await current.getByTestId(within).boundingBox() : { x: 0, y: 0, ...current.viewportSize() };
      assert(box && inside(box, frame), `${label}: ${id} is reachable`);
    };
    const finish = async (current, key, name, within) => {
      rows[key] = await codexLevelsOneRow(current, `${label} ${name}`);
      await current.screenshot({ path: join(values.output, `${label}-codex-levels-${key}.png`) });
      await reach(current, "kontor-model-use", within);
      await current.getByRole("button", { name: "Cancel model choice", exact: true }).click();
      await current.getByTestId("kontor-model-picker").waitFor({ state: "detached" });
    };
    {
      const { context, page: kontor } = await page();
      await kontor.getByTestId("kontor-switch-new-lead").click();
      await kontor.getByTestId("kontor-new-lead-model").click();
      await kontor.getByTestId("kontor-model-picker").waitFor();
      await kontor.waitForTimeout(300);
      await finish(kontor, "kontor", "New lead from Kontor");
      await context.close();
    }
    {
      const { context, page: composer } = await page();
      await (await composerPill(composer)).click();
      await composer.getByTestId("kontor-switch-new-lead").click();
      await composer.getByTestId("kontor-new-lead-model").click();
      await composer.getByTestId("kontor-model-picker").waitFor();
      await composer.waitForTimeout(300);
      await finish(composer, "composer", "New lead from the composer", width < 600 ? "host-popover-sheet" : "host-popover");
      await context.close();
    }
    {
      const { context, page: takeover } = await page();
      await pickState(takeover, "all");
      const more = takeover.getByRole("button", { name: /^Show the other \d+ of \d+$/ });
      if (await more.count()) await more.click();
      const claude = await takeover.evaluate(ids => ids.find(id => { const seat = window.__kontorNavigatorFixture.seat(id); return seat && !seat.archivedAt && seat.workspaceId && seat.status === "idle" && seat.provider === "claude"; }), await switchRowIds(takeover));
      assert(claude, `${label}: an idle claude seat`);
      await takeover.getByTestId("kontor-switch-list").getByTestId(`session-actions-${claude}`).click();
      await takeover.getByRole("dialog", { name: "Session actions", exact: true }).getByTestId("session-action-takeover-option").click();
      await takeover.getByTestId("kontor-model-picker").waitFor();
      await takeover.getByTestId("kontor-model-vendor-openai").click();
      await takeover.getByTestId("kontor-model-row-codex/gpt-6-astra").click();
      await takeover.getByTestId("kontor-model-problem").waitFor();
      await takeover.waitForTimeout(300);
      const problem = await takeover.getByTestId("kontor-model-problem").boundingBox();
      rows.takeoverProblem = { bottom: Math.round(problem.y + problem.height), inView: problem.y + problem.height <= height };
      await reach(takeover, "kontor-model-problem");
      await finish(takeover, "takeover", "Take over");
      await context.close();
    }
    results.push({ width, height, theme, codexLevels: rows });
  }
  for (const width of [320, 390, 1440]) for (const theme of ["light", "dark"]) {
    // SYNTHETIC whole-session totals on three sessions, one with output not reported; every other row stays unknown.
    const { context, page } = await open(width, theme, "unified-synthetic-totals");
    await pickState(page, "all");
    const tokens = id => page.getByTestId(`session-row-${id}`).getByTestId("session-row-tokens");
    for (const [query, expected] of [["ASMA-99103", { agt_swe_99103: "↓ 1.2m   ↑ 84.6k", agt_qa_99103: "↓ 31.2k   ↑ —" }], ["Model report", { agt_model_report_0: "↓ 124.8k   ↑ 9.6k", agt_model_report_1: "↓ —   ↑ —" }]]) {
      await page.getByTestId("kontor-switch-search").fill(query);
      const more = page.getByRole("button", { name: /^Show the other \d+ of \d+$/ });
      if (await more.count()) await more.click();
      for (const [id, text] of Object.entries(expected)) await page.waitForFunction(([id, text]) => document.querySelector(`[data-testid="session-row-${id}"] [data-testid="session-row-tokens"]`)?.textContent === text, [id, text]);
      assert.deepEqual(Object.fromEntries(await Promise.all(Object.keys(expected).map(async id => [id, await tokens(id).innerText()]))), expected);
    }
    assert(await noOverflow(page), `${width}-${theme}: totals overflow`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-synthetic-totals.png`) });
    // C1 Details: the switcher row's read-only sheet states the totals and where they came from.
    await rowAction(page, "agt_model_report_0", "details", page.getByTestId("kontor-switch-list"));
    const details = page.getByTestId("kontor-switch-details");
    await details.waitFor();
    assert.match(await details.innerText(), /agt_model_report_0[\s\S]*SYNTHETIC totals for review; not read from any host/);
    assert(await noOverflow(page), `${width}-${theme}: details overflow`);
    await page.screenshot({ path: join(values.output, `${width}-${theme}-unified-details.png`) });
    await page.getByTestId("kontor-switch-details-close").click();
    await details.waitFor({ state: "detached" });
    results.push({ width, theme, syntheticTotals: "1.2m/84.6k, 31.2k/—, 124.8k/9.6k; others unknown; Details names the SYNTHETIC source" });
    await context.close();
  }
  const { context, page } = await open(1440, "dark", "session-recovery");
  await rowAction(page, "agt_recovery_child", "archive");
  await page.waitForFunction(() => window.__kontorP1Fixtures.read("agt_recovery_child").archivedAt !== null);
  assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_child").status), "closed");
  await rowAction(page, "agt_recovery_archive_failure", "archive");
  await page.getByRole("button", { name: "Review archive", exact: true }).click();
  const item = page.getByTestId("archive-recovery-item-agt_recovery_archive_failure");
  await item.waitFor();
  assert.match(await item.innerText(), /Runtime stop not confirmed/);
  assert.equal(await item.getByRole("button", { name: /^Open session:/ }).count(), 0);
  await item.getByRole("button", { name: /^Refresh archive status:/ }).click();
  await page.getByTestId("archive-recovery-ack-agt_recovery_archive_failure").waitFor();
  await page.screenshot({ path: join(values.output, "archive-recovery-ack.png") });
  await item.getByRole("button", { name: /^Hide archive notice:/ }).click();
  await item.waitFor({ state: "detached" });
  results.push({ lifecycle: "simulated running archive, retained unconfirmed fence, exact-owner Refresh and per-item Hide; no archived Open" });
  await context.close();
  assert.deepEqual(errors, [], "Browser errors or unsupported fixture APIs");
  assert.deepEqual(denied, [], "Unexpected network requests");
  console.log(JSON.stringify({ passed: results.length, scenarios: scenarios.length, errors, denied, requests: requests.length, results }, null, 2));
} catch (error) {
  for (const context of browser.contexts()) for (const [index, page] of context.pages().entries()) {
    await writeFile(join(values.output, `failure-${index}.txt`), `${page.url()}\n${await page.locator("body").innerText()}`);
    await page.screenshot({ path: join(values.output, `failure-${index}.png`) });
  }
  throw error;
} finally {
  await writeFile(join(values.output, "browser-results.json"), JSON.stringify({ results, errors, denied, requests }, null, 2));
  await browser.close();
}
