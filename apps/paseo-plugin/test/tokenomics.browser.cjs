// Run against Chrome already started through `asma debug chrome`; only this script's tabs are closed.
const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
assert.ok(process.env.EVIDENCE_DIR, "Supply an external evidence directory");
const output = path.resolve(process.env.EVIDENCE_DIR), root = process.env.PROTOTYPE_URL || "http://127.0.0.1:58225/", chrome = process.env.DEBUG_CHROME_URL || "http://127.0.0.1:9222";
const pages = [
  ["overview", "Epic progress and consumption"], ["progress", "Progress and consumption between checks"], ["activity", "Activity timeline — who worked when, on what, and what needs a look"], ["roles", "Role consumption share"], ["pivot", "Pivot — group token consumption any way"], ["usage", "All known token usage — workstreams vs ad-hoc work"], ["data", "Data coverage"],
  ["cr-overview", "Code review — overview"], ["cr-fix", "Fix rate analytics"], ["cr-submitters", "Submitter analytics"], ["cr-repos", "Repository analytics"], ["cr-prs", "PR analytics"], ["dv-overview", "Delivery — overview"], ["dv-people", "People"], ["dv-repos", "Repositories"], ["dv-jira", "Jira vs GitHub"], ["dv-bugs", "Bugs & code quality"], ["dv-prs", "Pull requests"],
];
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function tab(width, height, theme) {
  const info = await (await fetch(`${chrome}/json/new?about:blank`, { method: "PUT" })).json(), socket = new WebSocket(info.webSocketDebuggerUrl), pending = new Map(), errors = [];
  let id = 0;
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  socket.onmessage = ({ data }) => { const x = JSON.parse(data); if (x.id) { const p = pending.get(x.id); pending.delete(x.id); x.error ? p?.reject(Error(JSON.stringify(x.error))) : p?.resolve(x.result); } else if (x.method === "Runtime.exceptionThrown") errors.push(x.params.exceptionDetails.text); };
  const call = (method, params = {}) => new Promise((resolve, reject) => { const key = ++id; pending.set(key, { resolve, reject }); socket.send(JSON.stringify({ id: key, method, params })); });
  const evaluate = async (expression) => { const r = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true, userGesture: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.text); return r.result.value; };
  const wait = async (expression) => { for (let i = 0; i < 120; i++) { if (await evaluate(`Boolean(${expression})`).catch(() => false)) return; await pause(250); } throw Error(`Timed out: ${expression}`); };
  await call("Page.enable"); await call("Runtime.enable"); await call("Network.enable"); await call("Network.setCacheDisabled", { cacheDisabled: true });
  await call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }); await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
  return { call, evaluate, wait, errors,
    navigate: (url) => call("Page.navigate", { url }),
    ready: async () => { await pause(100); await wait("document.querySelector('[data-testid=tokenomics-page] [aria-label=\"Refresh report\"]') && !document.querySelector('[data-testid=tokenomics-page]').innerText.includes('Updating selection…')"); },
    click: (label) => evaluate(`(()=>{const e=[...document.querySelectorAll('[role=button],button')].find(e=>e.getAttribute('aria-label')===${JSON.stringify(label)}||e.textContent===${JSON.stringify(label)});if(!e)throw Error('Missing button');e.click()})()`),
    async select(label, value) { await evaluate(`[...document.querySelectorAll('[role=button]')].find(e=>e.getAttribute('aria-label')?.startsWith(${JSON.stringify(label + ":")})).click()`); await wait("document.querySelector('[role=radio]')"); await evaluate(`[...document.querySelectorAll('[role=radio]')].find(e=>e.textContent.includes(${JSON.stringify(value)})).click()`); await pause(100); await wait("!document.querySelector('[role=radio]') && !document.body.innerText.includes('Updating selection…')"); },
    async screenshot(name) { await call("Input.dispatchMouseEvent", { type: "mouseMoved", x: width - 1, y: 1 }); const r = await call("Page.captureScreenshot", { format: "png", captureBeyondViewport: false }); await fs.writeFile(path.join(output, name), Buffer.from(r.data, "base64")); },
    async close() { await call("Page.close"); socket.close(); },
  };
}
(async () => {
  await fs.mkdir(output, { recursive: true }); const evidence = [];
  for (const [width, height] of [[390, 844], [1440, 900]]) for (const theme of ["dark", "light"]) {
    const t = await tab(width, height, theme);
    try {
      await t.navigate(`${root}?embed=1&scenario=tokenomics&theme=${theme}`); await t.ready();
      assert.equal(await t.evaluate("document.querySelector('[data-testid=\"report-kpi-Tokens processed\"]').children[1].textContent"), "1.4K");
      for (const [index, [id, heading]] of pages.entries()) {
        await t.evaluate(`document.querySelector('[data-testid=tokenomics-page]').querySelectorAll('[role=tab]')[${index}].click()`); await t.ready();
        assert.ok(await t.evaluate(`[...document.querySelectorAll('[data-testid=tokenomics-page] [role=heading]')].some(e=>e.textContent===${JSON.stringify(heading)})`), id);
        assert.ok(await t.evaluate("document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1"), `${id}: page overflow`);
        if (id === "progress") assert.ok(await t.evaluate("Boolean(document.querySelector('[aria-label^=\"From:\"]')) && !document.querySelector('[aria-label^=\"From:\"]').getAttribute('aria-label').includes('None')"));
        if (id === "cr-fix") assert.equal(await t.evaluate("document.querySelectorAll('[aria-label^=\"Outcomes:\"]').length"), 3);
        if (id === "pivot") { await t.click("Expand all"); await t.ready(); assert.ok(await t.evaluate("document.querySelector('[data-testid=report-table-pivot]').innerText.includes('EXAMPLE-102')")); await t.click("Collapse"); await t.ready(); }
        evidence.push({ id, width, height, theme, heading });
      }
      if (width === 1440) {
        await t.evaluate("document.querySelector('[data-testid=tokenomics-page]').querySelectorAll('[role=tab]')[2].click()"); await t.ready();
        assert.ok(await t.evaluate("Boolean(document.querySelector('[aria-label=\"Range: Full lifetime\"]'))"));
        await t.call("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: false }); await t.ready();
        assert.ok(await t.evaluate("Boolean(document.querySelector('[aria-label=\"Range: Last 30 days\"]'))"));
        await t.select("Range", "Last 14 days"); await t.call("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }); await t.ready();
        assert.ok(await t.evaluate("Boolean(document.querySelector('[aria-label=\"Range: Last 14 days\"]'))"));
      }
      await t.evaluate("document.querySelector('[data-testid=tokenomics-page]').querySelector('[role=tab]').click()"); await t.ready();
      assert.ok(await t.evaluate("[...document.querySelectorAll('[data-testid=report-progress]')].length>0 && [...document.querySelectorAll('[data-testid=report-progress]')].every(e=>e.getBoundingClientRect().height===12 && e.getBoundingClientRect().width>=160)"));
      assert.ok(await t.evaluate("Boolean(document.querySelector('[data-testid=report-workstream-change-Done]')) && document.querySelector('[data-testid=report-workstream-change-Done]').textContent!=='not captured'"));
      await t.click("Expand all"); await t.ready(); assert.ok(await t.evaluate("Boolean(document.querySelector('[data-testid=\"report-table-epics:build\"]'))"));
      assert.ok(await t.evaluate("document.querySelectorAll('[data-testid=report-epic-change]').length>0"));
      assert.ok(await t.evaluate("[...document.querySelectorAll('[data-testid=report-progress]')].length>0 && [...document.querySelectorAll('[data-testid=report-progress]')].every(e=>e.getBoundingClientRect().height===12)"));
      await t.click("Copy full CSV"); await t.wait("document.body.innerText.includes('all matching rows copied')");
      await t.select("Period", "Custom dates"); await t.ready();
      await t.evaluate("(()=>{const e=document.querySelector('[aria-label=\"from date YYYY-MM-DD\"]');const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set;set.call(e,'2026-11-02');e.dispatchEvent(new Event('input',{bubbles:true}));e.focus();e.blur()})()"); await t.ready();
      await t.select("Revision", "2026-11-01 13:00"); await t.ready(); await t.select("Revision", "2026-11-03 13:00"); await t.ready();
      await t.select("Period", "All time"); await t.ready(); await t.select("vs", "none"); await t.ready();
      await t.evaluate("(()=>{const r=document.querySelector('[data-testid=tokenomics-page]');r.scrollTop=0;for(const e of r.querySelectorAll('*'))if(e.scrollHeight>e.clientHeight+10&&['auto','scroll'].includes(getComputedStyle(e).overflowY))e.scrollTop=0;})()");
      await t.screenshot(`tokenomics-${width}-${theme}.png`); assert.deepEqual(t.errors, []);
    } finally { await t.close(); }
  }
  const offline = await tab(390, 844, "dark");
  try { await offline.navigate(`${root}?embed=1&scenario=tokenomics-offline&theme=dark`); await offline.wait("document.body.innerText.includes('Host offline')"); for (const label of ["Copy full CSV", "Refresh report"]) assert.equal(await offline.evaluate(`document.querySelector('[aria-label=${JSON.stringify(label)}]').getAttribute('aria-disabled')`), "true"); await offline.screenshot("tokenomics-offline.png"); } finally { await offline.close(); }
  await fs.writeFile(path.join(output, "native-pages.json"), JSON.stringify({ prototype: true, physicalDevice: false, pages: evidence, checks: ["responsive auto/manual timeline range", "complete CSV", "pivot drill-down", "workstream expansion", "custom dates", "revision selection", "comparison selection", "offline export/read refusal"] }, null, 2));
  console.log(`PASS ${evidence.length} native page renders and selection/export/offline interactions; prototype only.`);
})().catch((error) => { console.error(error); process.exitCode = 1; });
