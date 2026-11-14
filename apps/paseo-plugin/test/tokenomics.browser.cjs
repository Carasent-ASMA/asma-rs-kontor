// Uses Chrome already started through `asma debug chrome`; closes only this script's tabs.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
assert.ok(process.env.EVIDENCE_DIR, "Supply an external evidence directory");
const output = path.resolve(process.env.EVIDENCE_DIR), root = process.env.PROTOTYPE_URL || "http://127.0.0.1:58225/", chrome = process.env.DEBUG_CHROME_URL || "http://127.0.0.1:9222";
const pages = ["overview", "progress", "activity", "roles", "pivot", "usage", "data", "cr-overview", "cr-fix", "cr-submitters", "cr-repos", "cr-prs", "dv-overview", "dv-people", "dv-repos", "dv-jira", "dv-bugs", "dv-prs"];
const pause = (ms = 100) => new Promise((resolve) => setTimeout(resolve, ms));
async function tab(width, height, theme) {
  const info = await (await fetch(`${chrome}/json/new?about:blank`, { method: "PUT" })).json(), socket = new WebSocket(info.webSocketDebuggerUrl), pending = new Map(), errors = [];
  let id = 0;
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  socket.onmessage = ({ data }) => { const x = JSON.parse(data); if (x.id) { const p = pending.get(x.id); pending.delete(x.id); x.error ? p?.reject(Error(JSON.stringify(x.error))) : p?.resolve(x.result); } else if (x.method === "Runtime.exceptionThrown") errors.push(x.params.exceptionDetails.text); };
  const call = (method, params = {}) => new Promise((resolve, reject) => { const key = ++id; pending.set(key, { resolve, reject }); socket.send(JSON.stringify({ id: key, method, params })); });
  const evaluate = async (expression) => { const r = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true, userGesture: true }); if (r.exceptionDetails) throw Error(JSON.stringify(r.exceptionDetails)); return r.result.value; };
  const wait = async (expression) => { for (let i = 0; i < 120; i++) { if (await evaluate(`Boolean(${expression})`).catch(() => false)) return; await pause(250); } throw Error(`Timed out: ${expression}`); };
  await call("Page.enable"); await call("Runtime.enable"); await call("Network.enable"); await call("Network.setCacheDisabled", { cacheDisabled: true });
  await call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }); await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
  const ready = async () => { await pause(); await wait("document.querySelector('[data-testid=tokenomics-page] [aria-label=\"Refresh report\"]') && !document.body.innerText.includes('Updating selection…')"); };
  const click = (prefix) => evaluate(`(()=>{const e=[...document.querySelectorAll('[role=button],button')].find(e=>e.getAttribute('aria-label')?.startsWith(${JSON.stringify(prefix)}));if(!e)throw Error('Missing '+${JSON.stringify(prefix)});e.click()})()`);
  return { call, evaluate, wait, ready, click, errors,
    async navigate(scenario) { const n = await call("Page.navigate", { url: `${root}?embed=1&scenario=${scenario}&theme=${theme}` }); for (let i = 0; i < 120; i++) { const f = await call("Page.getFrameTree"); if ((!n.loaderId || f.frameTree.frame.loaderId === n.loaderId) && await evaluate("document.readyState==='complete'").catch(() => false)) return; await pause(); } throw Error("Navigation did not settle"); },
    async page(index) { if (await evaluate("Boolean(document.querySelector('[data-testid=report-mobile-bar]'))")) { await click("Pages:"); await wait("document.querySelectorAll('[role=dialog] [role=tab]').length===18"); await evaluate(`[...document.querySelectorAll('[role=dialog] [role=tab]')][${index}].click()`); } else await evaluate(`[...document.querySelectorAll('[data-testid=tokenomics-page] [role=tab]')][${index}].click()`); await ready(); },
    async key(key) { await call("Input.dispatchKeyEvent", { type: "keyDown", key, code: key }); await call("Input.dispatchKeyEvent", { type: "keyUp", key, code: key }); await pause(); },
    async select(label, text) { await click(`${label}:`); await wait("document.querySelector('[data-testid=tokenomics-choice-popup] [role=radio]')"); await evaluate(`[...document.querySelectorAll('[data-testid=tokenomics-choice-popup] [role=radio]')].find(e=>e.textContent.includes(${JSON.stringify(text)})).click()`); await ready(); },
    async shot(name) { const r = await call("Page.captureScreenshot", { format: "png", captureBeyondViewport: false }); await fs.writeFile(path.join(output, name), Buffer.from(r.data, "base64")); },
    async close() { await call("Page.close"); socket.close(); },
  };
}
(async () => {
  await fs.mkdir(output, { recursive: true }); const evidence = [];
  for (const [width, height] of [[390, 844], [1440, 900]]) for (const theme of ["dark", "light"]) {
    const t = await tab(width, height, theme);
    try {
      await t.navigate("tokenomics"); await t.ready();
      assert.equal(await t.evaluate("document.querySelector('[data-testid=\"report-kpi-Tokens processed\"]').children[1].textContent"), "1.4K");
      for (let index = 0; index < pages.length; index++) {
        await t.page(index);
        assert.ok(await t.evaluate("document.querySelectorAll('[data-testid^=report-section-]').length>0"), pages[index]);
        assert.ok(await t.evaluate("document.documentElement.scrollWidth<=document.documentElement.clientWidth+1"), `${pages[index]} overflow`);
        if (pages[index] === "pivot") {
          if (width === 390) await t.click("Workstream ›");
          await t.click("Expand all"); await t.ready();
          assert.ok(await t.evaluate("document.querySelector('[data-testid=tokenomics-page]').innerText.includes('EXAMPLE-102')"));
          await t.click("Collapse"); await t.ready();
        }
        if (pages[index] === "activity") assert.ok(await t.evaluate("document.querySelector('[aria-label^=\"Range:\"]')?.textContent.includes('Full lifetime')||document.querySelector('[aria-label^=\"Range:\"]')?.textContent.includes('Last 30 days')"));
        evidence.push({ id: pages[index], width, height, theme });
      }
      if (width === 390) { await t.select("Period", "Custom dates"); assert.ok(await t.evaluate("document.querySelector('[aria-label=\"from date YYYY-MM-DD\"]')&&document.querySelector('[aria-label=\"to date YYYY-MM-DD\"]')")); await t.select("Period", "All time"); }
      await t.page(0);
      if (width === 390) {
        assert.ok(await t.evaluate("[...document.querySelectorAll('[data-testid^=report-kpi-]')].slice(0,6).every(e=>e.getBoundingClientRect().bottom<=innerHeight-60)"), "first viewport KPIs");
        await t.click("View and filters:"); await t.wait("document.querySelector('[aria-label^=\"User:\"]')");
        const before = await t.evaluate("document.querySelector('[aria-label^=\"Revision:\"]').getBoundingClientRect().toJSON()");
        await t.click("User:"); await t.wait("document.querySelector('[data-testid=tokenomics-choice-popup]')");
        assert.deepEqual(await t.evaluate("document.querySelector('[aria-label^=\"Revision:\"]').getBoundingClientRect().toJSON()"), before);
        assert.equal(await t.evaluate("document.querySelector('[aria-label=\"Find User\"]')"), null);
        await t.key("Escape"); assert.ok(await t.evaluate("Boolean(document.querySelector('[role=dialog]'))"), "Escape closes only the dropdown");
        await t.select("User", "Person A"); await t.key("Escape");
        assert.ok(await t.evaluate("document.querySelector('[data-testid=report-mobile-context]').innerText.includes('All users')"), "dismiss cancels staged filters");
        await t.click("View and filters:"); await t.select("User", "Person A"); await t.click("Apply filters"); await t.ready();
        assert.ok(await t.evaluate("document.querySelector('[data-testid=report-mobile-context]').innerText.includes('Person A')"));
        await t.page(4); assert.ok(await t.evaluate("document.querySelector('[data-testid=report-mobile-context]').innerText.includes('Person A')"), "navigation retains filters");
        await t.page(0); await t.click("Report actions and data information"); await t.click("Export CSV");
      } else {
        await t.select("Period", "Custom dates"); await t.ready();
        await t.evaluate("(()=>{const e=document.querySelector('[aria-label=\"from date YYYY-MM-DD\"]'),set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;set.call(e,'2026-11-02');e.dispatchEvent(new Event('input',{bubbles:true}));e.focus();e.blur()})()"); await t.ready();
        await t.select("Revision", "2026-11-01 13:00"); await t.select("Revision", "2026-11-03 13:00"); await t.select("Period", "All time"); await t.select("vs", "none");
      }
      await t.click("Copy complete CSV"); await t.wait("document.body.innerText.includes('all matching rows copied')");
      await t.shot(`tokenomics-${width}-${theme}.png`); assert.deepEqual(t.errors, []);
    } finally { await t.close(); }
  }
  for (const width of [390, 1440]) {
    const t = await tab(width, 844, "light");
    try {
      await t.navigate("tokenomics-scroll"); await t.ready(); await t.page(17);
      if (width === 390) { await t.click("All 73 rows"); await t.wait("document.querySelector('[data-testid=report-full-rows-delivery-prs]')"); }
      const selector = width === 390 ? "[data-testid=report-full-rows-delivery-prs]" : "[data-testid=report-rows-delivery-prs]";
      await t.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollTop=100000`); await pause(500);
      await t.evaluate(`document.querySelector(${JSON.stringify(selector)}).scrollTop=100000`);
      await t.wait(`document.querySelector(${JSON.stringify(selector)}).innerText.includes('demo-repo#73')`);
      if (width === 390) { await t.key("Escape"); await t.click("Report actions and data information"); await t.click("Export CSV"); }
      await t.click("Copy complete CSV"); await t.wait("document.body.innerText.includes('all matching rows copied')");
      // Browser clipboard write is explicit; no clipboard read is performed. Complete payload is checked by tokenomics-rows.test.ts.
      evidence.push({ continuousRows: 73, browserClipboardWriteResolved: true, fullCsvProjectionCheckedSeparately: true, width });
    } finally { await t.close(); }
  }
  const offline = await tab(390, 844, "dark");
  try { await offline.navigate("tokenomics-offline"); await offline.wait("document.body.innerText.includes('Host offline')"); for (const label of ["Copy complete CSV", "Refresh report"]) assert.equal(await offline.evaluate(`[...document.querySelectorAll('[aria-label]')].find(e=>e.getAttribute('aria-label')===${JSON.stringify(label)}).getAttribute('aria-disabled')`), "true"); await offline.shot("tokenomics-offline.png"); } finally { await offline.close(); }
  await fs.writeFile(path.join(output, "native-pages.json"), JSON.stringify({ prototype: true, physicalDevice: false, pages: evidence, checks: ["all 18 destinations", "first viewport KPIs", "floating selector and nested Escape", "staged apply/cancel", "filter persistence", "continuous bounded rows", "complete CSV", "pivot drill-down", "custom dates", "revision/comparison", "offline refusal"] }, null, 2));
  console.log(`PASS ${evidence.length} page/continuous-row records and adaptive interactions; prototype only.`);
})().catch((error) => { console.error(error); process.exitCode = 1; });
