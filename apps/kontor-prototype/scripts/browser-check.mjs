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
async function open(width, theme, scenario = "integrated-review") {
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
  await page.goto(`${origin}/?embed=1&scenario=${scenario}&theme=${theme}&pointer=${width < 500 ? "coarse" : "fine"}`);
  await page.getByTestId("prototype-bar").waitFor();
  await page.getByRole("tab", { name: "Sessions", exact: true }).waitFor();
  await page.waitForTimeout(500);
  assert.match(await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content"), /connect-src 'none'/);
  return { context, page, sourceCalls };
}
try {
  for (const scenario of scenarios) {
    const { context, page } = await open(390, "dark", scenario);
    await page.waitForFunction(() => !document.body.innerText.includes("Reading native sessions"));
    if (/offline|stale/.test(scenario)) await page.waitForTimeout(4200);
    assert.match(await page.locator("body").innerText(), /fictional fixture data/);
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
    await page.getByTestId("kontor-switch-search").fill("ASMA-99103");
    await page.locator('[data-testid^="kontor-switch-row-"]').first().click();
    await page.getByTestId("host-agent-shim").waitFor();
    await page.getByRole("button", { name: "Back (host)", exact: true }).click();
    await page.getByRole("button", { name: "Start or switch session", exact: true }).click();
    assert(await page.locator('[data-testid^="kontor-switch-row-"]').count() > 0, "Recent retains the opened fictional session");
    await page.getByRole("button", { name: "New LSA session", exact: true }).click();
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
    assert.match(await row.innerText(), /\bsol\b/);
    assert.match(await row.innerText(), /4\.1K/);
    assert.match(await row.innerText(), /1\.2M/);
    assert(!/fictional-route/.test(await row.innerText()), "Route account belongs in Details, not the row label");
    await row.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(values.output, `${width}-model-route-tokens.png`) });
    results.push({ width, modelRoute: "model element only", tokens: "4.1K / 1.2M" });
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
    await page.getByTestId("sessions-selection-toggle").click();
    await page.getByTestId("sessions-select-agt_recovery_parent").click();
    await page.getByTestId("sessions-select-agt_recovery_child").click();
    await page.getByRole("button", { name: "Review archive of 2 selected sessions", exact: true }).click();
    const confirm = page.getByTestId("bulk-archive-dialog");
    await confirm.waitFor();
    assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_parent").archivedAt), null, "Review has no effects");
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
  const { context, page } = await open(1440, "dark", "session-recovery");
  const archive = id => page.getByRole("button", { name: new RegExp(`^Archive session:.*\\(${id}\\)$`) });
  await archive("agt_recovery_child").click();
  await page.waitForFunction(() => window.__kontorP1Fixtures.read("agt_recovery_child").archivedAt !== null);
  assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_child").status), "closed");
  await archive("agt_recovery_archive_failure").click();
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
