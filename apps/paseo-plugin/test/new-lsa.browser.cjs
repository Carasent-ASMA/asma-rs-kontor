// Synthetic React Native Web fixture only. No daemon, account, native session or device effects.
// RENDERER_PACKAGE_JSON supplies pinned React 19.1 / React DOM 19.1 / react-native-web 0.21.4.
// EVIDENCE_DIR keeps screenshots and measurements outside the repository.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const plugin = path.resolve(__dirname, '..');
const sdk = createRequire(path.join(plugin, 'package.json'));
const consoleDeps = createRequire(path.resolve(plugin, '../console/package.json'));
assert.ok(process.env.RENDERER_PACKAGE_JSON && process.env.EVIDENCE_DIR, 'Supply RENDERER_PACKAGE_JSON and an external EVIDENCE_DIR');
const renderer = createRequire(path.resolve(process.env.RENDERER_PACKAGE_JSON));
const output = path.resolve(process.env.EVIDENCE_DIR);
assert.ok(!output.startsWith(plugin + path.sep), 'Evidence stays outside the plugin tree');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-new-lsa-browser-'));
fs.mkdirSync(output, { recursive: true });
const rootOf = (resolver, name) => path.dirname(resolver.resolve(name + '/package.json'));
const react = rootOf(renderer, 'react'), dom = rootOf(renderer, 'react-dom'), native = rootOf(renderer, 'react-native-web');
assert.equal(renderer('react/package.json').version, '19.1.0');
assert.equal(renderer('react-dom/package.json').version, '19.1.0');
assert.equal(renderer('react-native-web/package.json').version, '0.21.4');
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'native.ts'), 'export * from "react-native-web";\n');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), `import {createContext,useContext} from 'react';
export const Host = createContext('fixture-a');
export const useRpc = contract => { const host = useContext(Host); return input => window.fixture.rpc(contract.name,input,host); };
export const getPaseoClient = () => { throw new Error('No live host in this fixture'); };
`);
fs.writeFileSync(path.join(fixture, 'sdk-native.tsx'), `import React from 'react';
export {TextInput,ScrollView} from 'react-native';
export const Icon = ({size=16,color}) => <span aria-hidden="true" style={{width:size,height:size,color}}>‹</span>;
`);
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React,{useRef,useState} from 'react';
import {createRoot} from 'react-dom/client'; import {Pressable,Text,View} from 'react-native';
import {Host} from './sdk-client';
import {StartSwitchSheet} from ${JSON.stringify(path.join(plugin, 'client/start-switch-sheet.tsx'))};
import {openCreatedSession} from ${JSON.stringify(path.join(plugin, 'client/session-actions.ts'))};
import {configureRecentStorage,readRecent} from ${JSON.stringify(path.join(plugin, 'client/recent-sessions.ts'))};
document.body.style.margin='0';
const query=new URLSearchParams(location.search), scenario=query.get('scenario')||'ready';
const dark=query.get('theme')==='dark';
const colors={surface0:dark?'#15171c':'#fff',surface1:dark?'#1f222a':'#f5f6f8',surface2:dark?'#2e333e':'#e6ebf2',foreground:dark?'#ecedf0':'#20242c',foregroundMuted:dark?'#a8adb7':'#5e6570',border:dark?'#414754':'#c7cdd7',accent:dark?'#9cbfff':'#245bba',accentForeground:dark?'#15171c':'#fff',statusSuccess:'#168047',statusWarning:'#a76b0a',statusDanger:'#b02222'};
const values=new Map(); configureRecentStorage({read:key=>values.get(key),write:(key,value)=>values.set(key,value)});
const model=(id,label)=>({id,label,efforts:[],defaultEffort:null});
const provider=(id,label,status,available)=>({id,label,status,available,note:available?null:status,persona:'supported',models:available?[model('first','Active model'),model('second','Second model')]:[],defaultModel:'first',modes:[],defaultMode:null});
let stale=false;
const catalog=workspaceId=>({enabled:true,disabledReason:null,pending:null,notes:[],catalogWorkspaceId:workspaceId||'wks-adam',desks:[{workspaceId:'wks-adam',projectId:'project-adam',desk:'ADAM',project:'DESK • ADAM'}],profile:null,
 providers:scenario==='empty'?[]:scenario==='loading'?[provider('loading','Loading provider','loading',false)]:[ {...provider('active','Active provider','ready',true),models:stale?[model('first','Active model')]:[model('first','Active model'),model('second','Second model')]}, provider('disabled','Disabled provider','disabled',false),provider('error','Error provider','error',false)]});
window.fixture={calls:[],opens:[],timings:[],failOpen:scenario==='open-failed', invalidateModel:()=>{stale=true;},recent:host=>readRecent(host).record,
 rpc:async(name,input,host)=>{const started=performance.now();window.fixture.calls.push({name,input,host});
  if(name==='sessions.create-options'){await new Promise(resolve=>setTimeout(resolve,10));window.fixture.timings.push({phase:'catalog',ms:performance.now()-started});return catalog(input.workspaceId);}
  if(name==='sessions.create'){await new Promise(resolve=>setTimeout(resolve,180));window.fixture.timings.push({phase:'create-fixture',ms:performance.now()-started});return {requestId:input.requestId,state:scenario==='unknown'?'unknown':'verified',title:'LSA • ADHOC • '+input.topic,agentId:scenario==='unknown'?null:'fixture-created-exact',workspaceId:input.workspaceId,message:scenario==='unknown'?'Creation may still complete.':'Created and verified.',differences:[],persona:'supported',checkedAt:new Date().toISOString()};}
  if(name==='sessions.create-check')return {requestId:input.requestId,state:'unknown',title:'LSA • ADHOC • Route review',agentId:null,workspaceId:'wks-adam',message:'Creation may still complete.',differences:[],persona:'supported',checkedAt:new Date().toISOString()};
  throw new Error('Unexpected RPC in isolated fixture: '+name);
 }};
function App(){const root=useRef(null),anchor=useRef(null);const [host,setHost]=useState('fixture-a'),[open,setOpen]=useState(true),[opened,setOpened]=useState('');
 window.fixture.switchHost=()=>setHost('fixture-b');window.fixture.close=()=>setOpen(false);
 const onOpenCreated=outcome=>{const started=performance.now();const result=openCreatedSession({outcome,serverId:host,navigation:{openAgent:input=>{if(window.fixture.failOpen)throw new Error('Fixture navigation failed');window.fixture.opens.push(input);setOpened(input.agentId);},openWorkspace:()=>{throw new Error('Unexpected workspace navigation');}}});window.fixture.timings.push({phase:'navigation',ms:performance.now()-started,ok:result.ok});if(result.ok)setOpen(false);return result;};
 return <Host.Provider value={host}><View ref={root} style={{height:innerHeight,width:innerWidth,backgroundColor:colors.surface0}}>
 <Pressable ref={anchor} accessibilityRole="button" accessibilityLabel="Start or switch" onPress={()=>setOpen(true)} style={{margin:16,minHeight:44}}><Text style={{color:colors.foreground}}>Start or switch</Text></Pressable>
 <Text testID="fixture-opened" style={{color:colors.foreground}}>{opened}</Text>
 {open?<StartSwitchSheet key={host} anchor={anchor} root={root} rootSize={{width:innerWidth,height:innerHeight}} compact={innerWidth<700} colors={colors} hostId={host} hostStatus="online" directory={{entries:[],status:'ready',live:true,lastUpdatedAt:Date.now(),error:null,staleReason:null,pages:1,includeArchived:false}} canOpen onClose={()=>setOpen(false)} onOpen={()=>{throw new Error('No directory entry in this fixture');}} onOpenCreated={onOpenCreated} onShowAll={()=>{throw new Error('Unexpected directory refresh');}}/>:null}
 </View></Host.Provider>;
}createRoot(document.getElementById('root')).render(<App/>);
`);
const report = { cases: [], nativeDevice: 'unverified', timingsAreSynthetic: true };
let server, browser;
async function pageFor(width, scenario = 'ready', theme = 'light') {
  const context = await browser.newContext({ viewport: { width, height: 844 }, hasTouch: width < 700, isMobile: width < 700 });
  const page = await context.newPage();
  page.setDefaultTimeout(5000);
  await page.goto(server.resolvedUrls.local[0] + '?scenario=' + scenario + '&theme=' + theme);
  await page.getByRole('button', { name: 'New LSA session', exact: true }).click();
  return { page, context };
}
async function run(name, test) {
  try { await test(); report.cases.push({ name, status: 'PASS' }); }
  catch (error) { report.cases.push({ name, status: 'FAIL', error: String(error.stack) }); }
}
(async () => {
  const vite = await import(consoleDeps.resolve('vite'));
  server = await vite.createServer({ root: fixture, configFile: false, define: { __DEV__: 'false', 'process.env.NODE_ENV': '"development"' },
    server: { host: '127.0.0.1', port: 0, fs: { allow: [fixture, plugin, native.split(path.sep + '.pnpm' + path.sep)[0]] } },
    resolve: { alias: [
      { find: /^react$/, replacement: react }, { find: /^react\/(.*)$/, replacement: react + '/$1' },
      { find: /^react-dom$/, replacement: dom }, { find: /^react-dom\/(.*)$/, replacement: dom + '/$1' },
      { find: /^react-native$/, replacement: path.join(fixture, 'native.ts') }, { find: /^react-native-web$/, replacement: path.join(native, 'dist/index.js') },
      { find: /^@getpaseo\/plugin$/, replacement: sdk.resolve('@getpaseo/plugin') },
      { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(fixture, 'sdk-client.ts') },
      { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(fixture, 'sdk-native.tsx') },
    ] },
  });
  await server.listen(); browser = await chromium.launch({ headless: true }); report.browser = browser.version();
  try {
    for (const width of [320, 390, 1440]) for (const theme of ['light', 'dark']) await run(`${width}-${theme}-active-exact-open`, async () => {
      const { page, context } = await pageFor(width, 'ready', theme);
      try {
        if (width < 700) await page.getByRole('button', { name: /^Provider, model and mode\./ }).click();
        await page.getByRole('radio', { name: 'Active provider', exact: true }).waitFor();
        assert.equal(await page.getByRole('radio', { name: /Disabled provider|Error provider/ }).count(), 0);
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        const radios = await page.getByRole('radio').all();
        for (const radio of radios) { const box = await radio.boundingBox(); assert.ok(box && box.x >= 0 && box.x + box.width <= width + 1); if (width < 700) assert.ok(box.height >= 44); }
        await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Route review');
        await page.screenshot({ path: path.join(output, `${width}-${theme}-choices.png`) });
        await page.getByTestId('kontor-new-lsa-create').click();
        await page.getByTestId('fixture-opened').filter({ hasText: 'fixture-created-exact' }).waitFor();
        const state = await page.evaluate(() => ({ opens: window.fixture.opens, calls: window.fixture.calls, recent: window.fixture.recent('fixture-a'), timings: window.fixture.timings }));
        assert.deepEqual(state.opens, [{ agentId: 'fixture-created-exact', serverId: 'fixture-a' }]);
        assert.deepEqual(state.recent.visits.map(visit => visit.id), ['fixture-created-exact']);
        assert.equal(state.calls.filter(call => call.name === 'sessions.create').length, 1);
        assert.equal(state.calls.filter(call => call.name === 'sessions.create-options').length, 1);
        report.cases.push({ name: `${width}-${theme}-timings`, status: 'PASS', timings: state.timings });
      } finally { await context.close(); }
    });
    for (const scenario of ['empty', 'loading']) await run(scenario, async () => {
      const { page, context } = await pageFor(390, scenario);
      try { await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Route review'); assert.equal(await page.getByTestId('kontor-new-lsa-create').isDisabled(), true); await page.getByRole('button', { name: 'Refresh choices', exact: true }).waitFor(); if (scenario === 'loading') await page.getByText('Some providers are still loading.').waitFor(); } finally { await context.close(); }
    });
    await run('invalidated-selection', async () => {
      const { page, context } = await pageFor(390);
      try { await page.getByRole('button', { name: /^Provider, model and mode\./ }).click(); await page.getByRole('radio', { name: 'Second model', exact: true }).click(); await page.evaluate(() => window.fixture.invalidateModel()); await page.getByRole('button', { name: 'Refresh choices', exact: true }).click(); await page.getByText(/Model second is no longer active/).waitFor(); assert.equal(await page.getByRole('radio', { name: 'Active model', exact: true }).getAttribute('aria-checked'), 'false'); assert.equal(await page.getByTestId('kontor-new-lsa-create').isDisabled(), true); } finally { await context.close(); }
    });
    await run('open-failed-retry-only', async () => {
      const { page, context } = await pageFor(390, 'open-failed');
      try { await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Route review'); await page.getByTestId('kontor-new-lsa-create').click(); await page.getByTestId('kontor-new-lsa-open-status').filter({ hasText: 'Opening failed' }).waitFor(); await page.getByText('Session ID: fixture-created-exact', { exact: true }).waitFor(); assert.deepEqual(await page.evaluate(() => window.fixture.recent('fixture-a').visits), []); await page.evaluate(() => { window.fixture.failOpen = false; }); await page.getByRole('button', { name: 'Open session', exact: true }).click(); await page.getByTestId('fixture-opened').filter({ hasText: 'fixture-created-exact' }).waitFor(); assert.equal(await page.evaluate(() => window.fixture.calls.filter(call => call.name === 'sessions.create').length), 1); } finally { await context.close(); }
    });
    for (const cancel of ['close', 'host-switch']) await run(`late-${cancel}`, async () => {
      const { page, context } = await pageFor(390);
      try { await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Route review'); await page.getByTestId('kontor-new-lsa-create').click(); if (cancel === 'close') await page.getByRole('button', { name: 'Close start or switch', exact: true }).click(); else await page.evaluate(() => window.fixture.switchHost()); await page.waitForTimeout(300); assert.deepEqual(await page.evaluate(() => window.fixture.opens), []); assert.deepEqual(await page.evaluate(() => window.fixture.recent('fixture-a').visits), []); } finally { await context.close(); }
    });
  } finally { await browser.close(); await server.close(); }
  fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
  if (report.cases.some(test => test.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  await Promise.allSettled([browser?.close(), server?.close()]);
  fs.rmSync(fixture, { recursive: true, force: true });
});
