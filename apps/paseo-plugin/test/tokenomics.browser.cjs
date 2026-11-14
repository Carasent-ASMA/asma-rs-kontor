// The existing prototype renders the plugin's React Native client. No installed plugin or physical device is touched.
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
assert.ok(process.env.PLAYWRIGHT_MODULE && process.env.EVIDENCE_DIR, "Supply existing Playwright and an external evidence directory");
const { chromium } = require(process.env.PLAYWRIGHT_MODULE);
const output = path.resolve(process.env.EVIDENCE_DIR);
fs.mkdirSync(output, { recursive: true });
const root = process.env.PROTOTYPE_URL || "http://127.0.0.1:58225/";
const pages = {
  Tokenomics: [["Overview", "Epic progress and consumption"], ["Progress between checks", "Captured totals"], ["Activity", "Activity timeline: team → people → workstreams → epics"], ["Roles & models", "Role consumption share"], ["Pivot", "Pivot"], ["All usage", "All usage by Jira epic"], ["Coverage & prices", "Data coverage"]],
  "Code Review": [["Overview", "By severity"], ["Fix rate", "By reviewer seat"], ["Submitters", "Submitters"], ["Repositories", "Repositories"], ["PRs", "Pull requests"]],
  Delivery: [["Overview", "PRs merged over time by author"], ["People", "People: output and quality"], ["Repositories", "Repositories: output and quality"], ["Jira vs GitHub", "By issue"], ["Bugs & code quality", "Bugs"], ["Pull requests", "Pull requests"]],
};
(async () => {
  const browser = await chromium.launch({ headless: true });
  const evidence = [];
  const oracle = { checked: false };
  const local = { checked: false };
  try {
    if (process.env.ORACLE_URL) {
      const page = await browser.newPage(); await page.goto(process.env.ORACLE_URL);
      await page.waitForSelector("#kpis .tile");
      const kpis = await page.locator("#kpis").innerText(), review = await page.locator("#crKpis").innerText(), delivery = await page.locator("#dvKpis").innerText();
      for (const expected of ["$6.00", "1.4K", "64.5%", "2 / 3", "340"]) assert.ok(kpis.includes(expected), `Original renderer is missing expected ${expected}`);
      assert.ok(review.includes("Fix rate100%") && review.includes("Desk cost$2.00"));
      assert.ok(delivery.includes("Code lines added+1.0K") && delivery.includes("Bugs per 1K code lines1.00"));
      const progression = await page.locator("#progOverall").innerText(); assert.ok(progression.includes("123") && progression.includes("999"));
      const ledger = await page.locator("#ledgerTable").innerText(); assert.ok(ledger.includes("Unknown epic") && ledger.includes("200"));
      Object.assign(oracle, { checked: true, processedTokens: 1400, cost: 6, hitRate: "64.5%", seats: "2/3", autoReviewTokens: 340, reviewFixRate: "100%", reviewCost: 2, codeAdded: 1000, bugsPerK: 1, capturedTokens: [123, 999], unknownTokens: 200 });
      await page.close();
    }
    for (const [width, height] of [[390, 844], [1440, 900]]) for (const theme of ["dark", "light"]) {
      const page = await browser.newPage({ viewport: { width, height } });
      const app = page.getByTestId("tokenomics-page");
      const errors = []; page.on("pageerror", (e) => errors.push(e.message));
      await page.goto(`${root}?embed=1&scenario=tokenomics&theme=${theme}&pointer=${width < 600 ? "coarse" : "fine"}`);
      await app.getByRole("heading", { name: "Epic progress and consumption", exact: true }).waitFor();
      assert.ok((await app.innerText()).includes("1,400") || (await app.innerText()).includes("1400"));
      for (const [group, views] of Object.entries(pages)) {
        await app.getByRole("button", { name: group, exact: true }).click();
        for (const [name, heading] of views) {
          await app.getByRole("tab", { name, exact: true }).click();
          await app.getByRole("heading", { name: heading, exact: true }).waitFor();
          assert.equal(await app.getByText("Report read failed.", { exact: true }).count(), 0);
          const dimensions = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
          assert.ok(dimensions.scroll <= width + 1, `Horizontal page overflow ${group}/${name} at ${width}`);
          evidence.push({ width, height, theme, group, page: name, rendered: heading });
          if (name === "Pivot") {
            await app.getByRole("button", { name: "Expand all groups", exact: true }).click();
            await app.getByText("EXAMPLE-102", { exact: true }).first().waitFor();
            await app.getByRole("button", { name: "Collapse all groups", exact: true }).click();
          }
        }
      }
      await app.getByRole("button", { name: "Tokenomics", exact: true }).click();
      await app.getByRole("tab", { name: "Overview", exact: true }).click();
      await app.getByRole("heading", { name: "Epic progress and consumption", exact: true }).waitFor();
      await app.getByRole("button", { name: "Copy full CSV", exact: true }).click();
      await app.getByText(/all matching rows copied as CSV/).waitFor();
      await app.evaluate((element) => { element.scrollTop = 0; });
      await page.screenshot({ path: path.join(output, `tokenomics-overview-${width}-${theme}.png`) });
      await app.getByRole("button", { name: "Period: All time", exact: true }).click();
      await app.getByRole("button", { name: "Custom dates", exact: true }).click();
      const from = app.getByRole("textbox", { name: "from date YYYY-MM-DD", exact: true }), to = app.getByRole("textbox", { name: "to date YYYY-MM-DD", exact: true });
      await from.fill("2026-11-02"); await from.blur(); await to.fill("2026-11-02"); await to.blur();
      await app.getByRole("heading", { name: "Epic progress and consumption", exact: true }).waitFor();
      await app.getByRole("button", { name: "At revision: Latest check", exact: true }).click();
      await app.getByRole("button", { name: "2026-11-01 13:00 · daily", exact: true }).click();
      await app.getByRole("button", { name: "At revision: 2026-11-01 13:00 · daily", exact: true }).click();
      await app.getByRole("button", { name: "Latest check", exact: true }).click();
      await app.getByRole("button", { name: "Compare revision: No comparison", exact: true }).click();
      await app.getByRole("button", { name: "2026-11-01 13:00 · daily", exact: true }).click();
      await app.getByRole("heading", { name: "Epic progress and consumption", exact: true }).waitFor();
      await page.screenshot({ path: path.join(output, `tokenomics-${width}-${theme}.png`) });
      assert.deepEqual(errors, [], `Client errors at ${width}/${theme}`);
      await page.close();
    }
    const offline = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await offline.goto(`${root}?embed=1&scenario=tokenomics-offline&theme=dark&pointer=coarse`);
    const offlineApp = offline.getByTestId("tokenomics-page");
    await offlineApp.getByRole("heading", { name: "Source: stale", exact: true }).waitFor();
    assert.equal(await offlineApp.getByRole("button", { name: "Copy full CSV", exact: true }).getAttribute("aria-disabled"), "true");
    assert.equal(await offlineApp.getByRole("button", { name: "Refresh report", exact: true }).getAttribute("aria-disabled"), "true");
    await offlineApp.getByText(/Host offline/).waitFor();
    await offline.screenshot({ path: path.join(output, "tokenomics-offline.png") }); await offline.close();
    if (process.env.LOCAL_REPORT_URL) {
      const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
      const response = page.waitForResponse((r) => new URL(r.url()).pathname === "/__kontor_tokenomics");
      await page.goto(process.env.LOCAL_REPORT_URL); const received = await response;
      assert.equal(received.status(), 200); const view = await received.json();
      const app = page.getByTestId("tokenomics-page"); await app.getByRole("heading", { name: "Epic progress and consumption", exact: true }).waitFor();
      await page.getByText(/local Tokenomics report; simulated Paseo host/).waitFor();
      for (const tile of view.tiles) {
        const card = app.getByRole("heading", { name: tile.label, exact: true }).locator("..");
        assert.ok((await card.innerText()).includes(String(tile.value ?? "—")), "Local native tile differs from its bounded adapter snapshot");
      }
      assert.ok((await page.locator('meta[http-equiv="Content-Security-Policy"]').getAttribute("content")).includes("connect-src 'self'"));
      Object.assign(local, { checked: true, projectionBytes: (await received.body()).length, contentRevision: view.revision, matchedTiles: view.tiles.length, privateValuesRecorded: false });
      await page.close();
    }
    fs.writeFileSync(path.join(output, "native-pages.json"), JSON.stringify({ prototype: true, physicalDevice: false, originalRendererOracle: oracle, localReport: local, pages: evidence }, null, 2));
    console.log(`PASS ${evidence.length} native page renders: 18 pages × compact/desktop × dark/light; pivot drill-down, complete CSV, custom dates/revisions and offline source refusal.`);
  } finally { await browser.close(); }
})().catch((error) => { console.error(error); process.exitCode = 1; });
