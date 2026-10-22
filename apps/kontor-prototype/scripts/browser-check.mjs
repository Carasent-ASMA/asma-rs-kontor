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
  page.on("pageerror", error => errors.push(`${scenario}: ${error.message}`));
  page.on("console", message => { if (message.type() === "error" || message.text().includes("paseo.unsupported")) errors.push(`${scenario}: ${message.text()}`); });
  await page.goto(`${origin}/?embed=1&scenario=${scenario}&theme=${theme}&pointer=${width < 500 ? "coarse" : "fine"}`);
  await page.getByTestId("prototype-bar").waitFor();
  await page.getByRole("tab", { name: "Sessions", exact: true }).waitFor();
  await page.waitForTimeout(500);
  assert.match(await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content"), /connect-src 'none'/);
  return { context, page };
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
    await page.getByTestId("sessions-working-as").click();
    await page.getByRole("menuitem", { name: /Alex \(fictional\)/ }).click();
    assert.match(await page.getByTestId("sessions-working-as").innerText(), /Alex/);
    await page.getByTestId("sessions-sort").click();
    await page.getByRole("menuitem", { name: "Title", exact: true }).click();
    assert.match(await page.getByTestId("sessions-sort").innerText(), /Title/);
    results.push({ width, theme, profileAndSort: true, noOverflow: true });
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
  const { context, page } = await open(1440, "dark", "session-recovery");
  const archive = id => page.getByRole("button", { name: new RegExp(`^Archive session:.*\\(${id}\\)$`) });
  await archive("agt_recovery_child").click();
  await page.waitForFunction(() => window.__kontorP1Fixtures.read("agt_recovery_child").archivedAt !== null);
  assert.equal(await page.evaluate(() => window.__kontorP1Fixtures.read("agt_recovery_child").status), "closed");
  await archive("agt_recovery_archive_failure").click();
  await page.getByRole("button", { name: "Archive details", exact: true }).click();
  await page.getByRole("button", { name: "Check native outcome", exact: true }).waitFor();
  await page.getByRole("button", { name: "Check native outcome", exact: true }).click();
  results.push({ lifecycle: "simulated running archive and uncertain outcome reconciliation" });
  await context.close();
  assert.deepEqual(errors, [], "Browser errors or unsupported fixture APIs");
  assert.deepEqual(denied, [], "Unexpected network requests");
  console.log(JSON.stringify({ passed: results.length, scenarios: scenarios.length, errors, denied, requests: requests.length, results }, null, 2));
} finally {
  await writeFile(join(values.output, "browser-results.json"), JSON.stringify({ results, errors, denied, requests }, null, 2));
  await browser.close();
}
