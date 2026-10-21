// Disposable fixture pages only. Attach to an existing Chromium; never launch/stop it.
// node scripts/lifecycle-accessibility-check.mjs --playwright <installed module> --cdp <endpoint> --origin <fixture preview> --output <evidence directory>
import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { join } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: Object.fromEntries(["playwright", "cdp", "origin", "output"].map((key) => [key, { type: "string" }])) });
for (const key of ["playwright", "cdp", "origin", "output"]) assert(values[key], `Explicit --${key} required`);
const origin = new URL(values.origin).origin;
assert(["127.0.0.1", "localhost"].includes(new URL(origin).hostname), "Only the local disposable fixture preview is allowed");
const { chromium } = createRequire(import.meta.url)(values.playwright);
const results = [], pages = [], errors = [], denied = [], requests = [];
const record = (name, actual, expected) => { assert.deepEqual(actual, expected, name); results.push({ name, pass: true }); };

async function run() {
  await mkdir(values.output, { recursive: true });
  const html = await (await fetch(origin)).text();
  assert(html.includes("Kontor plugin · review prototype"), "The supplied port is not the disposable fixture preview");
  const browser = await chromium.connectOverCDP(values.cdp);
  try {
    for (const [width, height, touch] of [[1440, 900, false], [390, 844, true]]) {
      const page = await browser.contexts()[0].newPage(); pages.push(page);
      page.setDefaultTimeout(8_000);
      await page.setViewportSize({ width, height });
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: touch, maxTouchPoints: 1 });
      await cdp.send("Accessibility.enable");
      page.on("pageerror", (error) => errors.push(String(error)));
      await page.route("**/*", (route) => {
        const request = route.request();
        if (request.method() === "GET" && new URL(request.url()).origin === origin) { requests.push(request.url()); return route.continue(); }
        denied.push(request.url()); return route.abort();
      });
      await page.routeWebSocket("**/*", (socket) => { denied.push(socket.url()); socket.close(); });
      await page.goto(`${origin}/?embed=1&scenario=session-recovery&pointer=${touch ? "coarse" : "fine"}&theme=${touch ? "light" : "dark"}`);
      const recovery = page.getByRole("button", { name: "Recover sessions", exact: true });
      await recovery.waitFor();
      await page.getByTestId("sessions-result-agt_recovery_unknown").getByText("Availability unknown", { exact: false }).waitFor();
      await page.getByTestId("sessions-result-agt_recovery_missing").waitFor({ state: "detached" });
      const archive = page.getByRole("button", { name: /^Archive session:/ });
      const labels = await archive.evaluateAll((nodes) => nodes.map((node) => node.getAttribute("aria-label")));
      record(`${width}: archive controls present`, labels.length, 12);
      record(`${width}: archive targets unique`, new Set(labels).size, 12);
      record(`${width}: every archive names its exact target`, labels.every((label) => /\(agt_recovery_[a-z_]+\)$/.test(label)), true);
      if (touch) { const box = await archive.first().boundingBox(); record(`${width}: archive touch target`, box.width >= 44 && box.height >= 44, true); }
      await recovery.click();
      const dialog = page.getByTestId("session-lifecycle-dialog");
      await dialog.getByRole("button", { name: "Review sessions", exact: true }).click();
      const unknown = dialog.getByRole("checkbox", { name: /\(agt_recovery_unknown\)/ });
      await unknown.waitFor();
      const defaults = await dialog.getByRole("checkbox").evaluateAll((nodes) => nodes.map((node) => ({ checked: node.getAttribute("aria-checked"), visual: node.textContent.startsWith("☑") })));
      record(`${width}: defaults expose checked state`, defaults.every((state) => state.checked === String(state.visual)), true);
      record(`${width}: eligible defaults checked`, defaults.filter((state) => state.checked === "true").length, 3);
      async function checked(expected, action) {
        await page.waitForFunction(({ expected }) => [...document.querySelectorAll('[role="checkbox"]')].find((node) => node.getAttribute("aria-label").includes("(agt_recovery_unknown)"))?.getAttribute("aria-checked") === String(expected), { expected });
        const ax = (await cdp.send("Accessibility.getFullAXTree")).nodes.find((node) => node.role?.value === "checkbox" && node.name?.value.includes("(agt_recovery_unknown)"));
        record(`${width}: ${action} DOM state`, await unknown.getAttribute("aria-checked"), String(expected));
        record(`${width}: ${action} AX state`, String(ax?.properties?.find((property) => property.name === "checked")?.value?.value), String(expected));
      }
      await checked(false, "initial unknown");
      await unknown.focus(); await page.keyboard.press("Space"); await checked(true, "Space select");
      await page.keyboard.press("Space"); await checked(false, "Space clear");
      if (touch) {
        await unknown.scrollIntoViewIfNeeded(); const box = await unknown.boundingBox();
        await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x + box.width / 2, y: box.y + box.height / 2 }] });
        await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
      } else await unknown.click();
      await checked(true, touch ? "touch select" : "pointer select");
      await page.keyboard.press("Enter"); await checked(false, "Enter single activation");
      await page.keyboard.down("Space"); await checked(true, "Space held initial activation");
      await page.keyboard.down("Space"); await page.keyboard.up("Space"); await checked(true, "Space repeat ignored");
      await page.screenshot({ path: join(values.output, `checkbox-${width}.png`) });
      await writeFile(join(values.output, `checkbox-${width}.ax.txt`), await dialog.ariaSnapshot());
      record(`${width}: checkbox interaction dispatched no lifecycle effect`, await dialog.getByText(/^(submitted|archived|uncertain):/).count(), 0);
      await page.close();
    }
    record("no page errors", errors, []); record("no non-fixture requests", denied, []);
  } finally {
    for (const page of pages) if (!page.isClosed()) await page.close();
    await writeFile(join(values.output, "browser-regression.json"), JSON.stringify({ results, errors, denied, requests, effects: "Fixture selection only; no native lifecycle or inventory operations" }, null, 2) + "\n");
  }
  console.log(JSON.stringify({ passed: results.length, errors: errors.length, denied: denied.length }));
}
// Exit this test client to disconnect its CDP socket; never close the shared browser.
run().then(() => process.exit(0), (error) => { console.error(error); process.exit(1); });
