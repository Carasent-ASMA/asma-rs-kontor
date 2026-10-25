// Synthetic shared New session picker proof. No live host, creation or lifecycle effects.
// Supply RENDERER_PACKAGE_JSON, PLAYWRIGHT_MODULE and external EVIDENCE_DIR.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const plugin = path.resolve(__dirname, '..');
const sdk = createRequire(path.join(plugin, 'package.json'));
const consoleDeps = createRequire(path.resolve(plugin, '../console/package.json'));
const rendererManifest = process.env.RENDERER_PACKAGE_JSON || process.argv[2];
const evidenceDirectory = process.env.EVIDENCE_DIR || process.argv[3];
assert.ok(rendererManifest && evidenceDirectory);
const renderer = createRequire(path.resolve(rendererManifest));
const output = path.resolve(evidenceDirectory);
assert.ok(!output.startsWith(plugin + path.sep));
const { chromium, expect } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-picker-browser-'));
fs.mkdirSync(output, { recursive: true });
const rootOf = (resolver, name) => path.dirname(resolver.resolve(name + '/package.json'));
const react = rootOf(renderer, 'react'), dom = rootOf(renderer, 'react-dom'), native = rootOf(renderer, 'react-native-web');
assert.equal(renderer('react/package.json').version, '19.1.0');
assert.equal(renderer('react-native-web/package.json').version, '0.21.4');
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'native.ts'), 'export * from "react-native-web";\n');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), `import {createContext,useContext} from 'react';
export const Host=createContext('fixture-a');
export const useRpc=contract=>{const host=useContext(Host);return input=>window.fixture.rpc(contract.name,input,host);};
export const getPaseoClient=host=>window.fixture.client(host);
export const useHosts=()=>[{serverId:'fixture-a',status:'online'}];
`);
fs.writeFileSync(path.join(fixture, 'sdk-native.tsx'), `import React from 'react';
export {TextInput,ScrollView} from 'react-native';
export const useToast=()=>({error:message=>{throw new Error(message);},show:()=>{}});
export const Icon=({name,size=16,color})=><span aria-hidden="true" data-icon={name} style={{display:'inline-block',width:size,height:size,color}}>◇</span>;
`);
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React,{useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';import {Pressable,Text,View} from 'react-native';
import {Host} from './sdk-client';
import {StartSwitchSheet,StartSwitchPopover} from ${JSON.stringify(path.join(plugin, 'client/start-switch-sheet.tsx'))};
import {configureRecentStorage} from ${JSON.stringify(path.join(plugin, 'client/recent-sessions.ts'))};
document.body.style.margin='0';
const mode=new URLSearchParams(location.search).get('mode');
const dark=new URLSearchParams(location.search).has('dark');
const colors={surface0:dark?'#15171c':'#fff',surface1:dark?'#1f222a':'#f5f6f8',surface2:dark?'#2e333e':'#e6ebf2',foreground:dark?'#ecedf0':'#20242c',foregroundMuted:dark?'#a8adb7':'#5e6570',border:dark?'#414754':'#c7cdd7',accent:dark?'#9cbfff':'#245bba',accentForeground:dark?'#15171c':'#fff',statusSuccess:'#168047',statusWarning:'#a76b0a',statusDanger:'#b02222'};
const saved=new Map();configureRecentStorage({read:key=>saved.get(key),write:(key,value)=>saved.set(key,value)});
const sid=i=>'00000000-0000-4000-8000-'+String(i).padStart(12,'0');
const sessions=Array.from({length:70},(_,i)=>({agent:{id:sid(i+1),title:'Human context '+(i+1)+' with a long readable session name',status:i%3===0?'error':'closed',archivedAt:i>=50?'2026-10-20':null,createdAt:new Date(1000000*(i+1)).toISOString(),updatedAt:new Date(1000000*(70-i)).toISOString(),provider:'fixture',cwd:'/fixture',labels:{}},project:null}));
const issues=Array.from({length:90},(_,i)=>({key:'ASMA-'+(900-i),summary:(i===89?'Outside initial window':'Long readable issue title '+i)+' for the shared picker and keyboard layout',epic:i%2===0,type:i%2===0?'Epic':i%3===0?'Bug':'Task',status:'In Development',statusCategory:'In Progress',open:true,parent:null}));
const desks=[{projectId:'adam',project:'DESK • ADAM',desk:'ADAM',kind:'adam',cwd:'/fixture/adam',lead:{ADHOC:'adhoc',LSA:'lsa',TPM:'tpm'}},{projectId:'tickets',project:'DESK • TICKETS',desk:'TICKETS',kind:'tickets',cwd:'/fixture/tickets',lead:{ADHOC:'adhoc-t',LSA:'lsa-t',TPM:'tpm-t'}}];
window.fixture={calls:[],mode:'normal',sourceTitle:null,client:host=>({agents:{list:async input=>{window.fixture.calls.push({name:'agents.list',host,input});const offset=input.page.cursor?35:0;return {entries:sessions.slice(offset,offset+35),pageInfo:{hasMore:offset===0,nextCursor:offset===0?'page2':null}};},ref:id=>({refresh:async()=>{window.fixture.calls.push({name:'agent.refresh',id,host});if(window.fixture.mode==='transport')throw new Error('Fixture transport timeout');if(window.fixture.mode==='absent')return null;const entry=sessions.find(row=>row.agent.id===id);return entry?{...entry,agent:{...entry.agent,title:window.fixture.sourceTitle||entry.agent.title}}:null;},timeline:{refetch:async()=>{if(window.fixture.mode==='context-error')throw new Error('Fixture timeline unavailable');return {entries:window.fixture.mode==='no-message'?[]:[{item:{type:'assistant_message',text:'Last assistant message fixture'}}]};}}})}}),
rpc:async(name,input,host)=>{window.fixture.calls.push({name,input,host});if(name==='sessions.create-options'){const query=(input.issueKey||'').toLowerCase();if(query==='slow')await new Promise(resolve=>{window.fixture.releaseSlow=resolve;});if(query==='transport')throw new Error('Fixture Jira transport');const matches=issues.filter(issue=>!query||issue.summary.toLowerCase().includes(query)||issue.key.toLowerCase()===query);return {enabled:true,disabledReason:null,pending:null,notes:[],catalogProjectId:input.projectId||'adam',issues:input.start==='from-jira'?matches.slice(0,100):[],jiraProblem:null,desks,profile:null,providers:[{id:'fixture',label:'Fixture provider',available:true,note:null,persona:'supported',status:'ready',models:[{id:'fixture',label:'Fixture model',efforts:[],defaultEffort:null}],defaultModel:'fixture',modes:[],defaultMode:null}]};}
if(name==='sessions.create')return {requestId:input.requestId,state:'verified',title:'Fixture create',agentId:'fixture-created',workspaceId:'lsa',message:'Fixture only',differences:[],persona:'supported',checkedAt:new Date().toISOString()};throw new Error('Unexpected RPC '+name);}};
function App(){const root=useRef(null),anchor=useRef(null);const [open,setOpen]=useState(true);return <Host.Provider value="fixture-a"><View ref={root} style={{height:innerHeight,width:innerWidth,backgroundColor:colors.surface0}}>
<Pressable ref={anchor} accessibilityRole="button" accessibilityLabel="Start or switch" style={{margin:16,minHeight:44}}><Text style={{color:colors.foreground}}>Start or switch</Text></Pressable>
{open?(mode==='composer'?<View style={{width:'100%',maxWidth:520,alignSelf:'center',maxHeight:innerHeight-40}}><StartSwitchPopover host={{id:'fixture-a'}} theme={{colors}} layout={{compact:innerWidth<700}} workspaceId="adhoc" close={()=>setOpen(false)} openScreen={()=>{}} screenId="kontor"/></View>:<StartSwitchSheet anchor={anchor} root={root} rootSize={{width:innerWidth,height:innerHeight}} compact={innerWidth<700} colors={colors} hostId="fixture-a" hostStatus="online" directory={{entries:[],status:'ready',live:true,lastUpdatedAt:Date.now(),error:null,staleReason:null,pages:1,includeArchived:false}} canOpen onClose={()=>setOpen(false)} onOpen={()=>{}} onOpenCreated={()=>({ok:true})} onShowAll={()=>{}}/>):null}
</View></Host.Provider>;}createRoot(document.getElementById('root')).render(<App/>);
`);
const report = { cases: [], synthetic: true, nativePhoneAndAT: 'unverified', backendNewestEligible50: 'UNRESOLVED: fixture does not qualify the installed backend', actualJiraIcons: 'UNRESOLVED: contract has type names only' };
let server, browser;
const run = async (name, test) => { try { await test(); report.cases.push({ name, status: 'PASS' }); } catch (error) { report.cases.push({ name, status: 'FAIL', error: String(error.stack) }); } };
async function start(width, mode = 'screen', height = 844) {
  const context = await browser.newContext({ viewport: { width, height }, hasTouch: width < 700, isMobile: width < 700 });
  const page = await context.newPage(); page.setDefaultTimeout(5000);
  await page.goto(server.resolvedUrls.local[0] + '?mode=' + mode + (width === 390 ? '&dark' : ''));
  await page.getByRole('button', { name: 'New session', exact: true }).click();
  return { page, context };
}
async function bounds(page, width, height, name) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'no horizontal overflow');
  for (const label of ['Clear choose a jira issue search', 'All', 'Epics', 'Tickets', 'Back from choose a jira issue']) {
    const node = page.getByRole('button', { name: label, exact: true }); const box = await node.boundingBox();
    assert.ok(box && box.x >= 0 && box.x + box.width <= width + 1 && box.y >= 0 && box.y + box.height <= height + 1, label + ' reachable');
    assert.ok(box.height >= 44, label + ' 44px');
  }
  await page.screenshot({ path: path.join(output, name + '.png') });
}
(async () => {
  const vite = await import(consoleDeps.resolve('vite'));
  server = await vite.createServer({ root: fixture, configFile: false, define: { __DEV__: 'false', 'process.env.NODE_ENV': '"development"' },
    server: { host: '127.0.0.1', port: 0, fs: { allow: [fixture, plugin, native.split(path.sep + '.pnpm' + path.sep)[0]] } },
    resolve: { alias: [ { find: /^react$/, replacement: react }, { find: /^react\/(.*)$/, replacement: react + '/$1' }, { find: /^react-dom$/, replacement: dom }, { find: /^react-dom\/(.*)$/, replacement: dom + '/$1' }, { find: /^react-native$/, replacement: path.join(fixture, 'native.ts') }, { find: /^react-native-web$/, replacement: path.join(native, 'dist/index.js') }, { find: /^@getpaseo\/plugin$/, replacement: sdk.resolve('@getpaseo/plugin') }, { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(fixture, 'sdk-client.ts') }, { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(fixture, 'sdk-native.tsx') } ] },
  });
  await server.listen(); browser = await chromium.launch({ headless: true });
  for (const width of [320, 390, 1440]) for (const mode of ['screen', 'composer']) await run(width + '-' + mode + '-picker-layout-and-selection', async () => {
    const { page, context } = await start(width, mode);
    try {
      await page.getByRole('radio', { name: /^From Jira ticket/ }).click();
      const search = page.getByRole('combobox', { name: 'Choose a Jira issue' });
      await expect(page.getByRole('option')).toHaveCount(50);
      await bounds(page, width, 844, width + '-' + mode + '-jira');
      await page.getByRole('button', { name: 'Epics', exact: true }).click(); await expect(page.getByRole('option')).toHaveCount(45);
      await page.getByRole('button', { name: 'Tickets', exact: true }).click(); await expect(page.getByRole('option')).toHaveCount(45);
      await page.getByRole('button', { name: 'All', exact: true }).click();
      await search.fill('Outside initial window'); await expect(page.getByRole('option')).toHaveCount(1);
      await page.getByRole('option').click(); await expect(page.getByRole('textbox', { name: /^Title topic after/ })).toBeVisible();
      await page.getByRole('textbox', { name: /^Title topic after/ }).fill('Keep this edited topic');
      await page.getByRole('button', { name: 'Change Jira issue' }).click(); await search.fill('missing query'); await expect(page.getByText('No open ASMA issue matches.')).toBeVisible();
      await page.getByRole('button', { name: 'Back from choose a jira issue' }).click(); await expect(page.getByRole('textbox', { name: /^Title topic after/ })).toHaveValue('Keep this edited topic');
      await expect(page.getByRole('button', { name: 'Change Jira issue' })).toBeFocused();
      await page.getByRole('button', { name: 'Change Jira issue' }).click(); await search.fill(''); await expect(page.getByRole('option')).toHaveCount(50);
      await page.setViewportSize({ width, height: 480 }); await search.focus(); await bounds(page, width, 480, width + '-' + mode + '-keyboard-constrained');
      await page.getByRole('button', { name: 'Back from choose a jira issue' }).click();
      await page.getByRole('button', { name: 'Choose source session' }).click();
      await expect(page.getByRole('option')).toHaveCount(10);
      await expect(page.getByRole('option').first()).toHaveAccessibleName(/Human context 50/);
      const source = page.getByRole('combobox', { name: 'Choose a source session' });
      await source.fill('Human context 1 with'); await expect(page.getByRole('option')).toHaveCount(1);
      await source.fill(''); await page.getByRole('button', { name: 'Include archived' }).click();
      await expect(page.getByRole('option').first()).toHaveAccessibleName(/Human context 70.*Archived/);
      await page.getByRole('button', { name: 'Include archived' }).click();
      await source.fill('00000000-0000-4000-8000-000000000070'); await expect(page.getByRole('option')).toHaveAccessibleName(/Human context 70.*Archived/);
      await page.getByRole('option').click();
      await expect(page.getByTestId('kontor-source-selected')).toContainText('Human context 70');
      await page.getByRole('button', { name: 'Clear source' }).click(); await expect(page.getByTestId('kontor-source-selected')).toHaveCount(0);
      assert.equal(await page.evaluate(() => window.fixture.calls.some(call => call.name === 'sessions.create')), false);
    } finally { await context.close(); }
  });
  await run('stale-Jira-error-retry-and-source-read-states', async () => {
    const { page, context } = await start(390);
    try {
      await page.getByRole('radio', { name: /^From Jira ticket/ }).click(); const jira = page.getByRole('combobox', { name: 'Choose a Jira issue' });
      await jira.fill('slow'); await page.waitForFunction(() => !!window.fixture.releaseSlow); await jira.fill('Outside initial window');
      await expect(page.getByRole('option')).toHaveCount(1); await page.evaluate(() => window.fixture.releaseSlow()); await expect(page.getByRole('option')).toHaveCount(1);
      await jira.fill('transport'); await expect(page.getByText(/Fixture Jira transport/)).toBeVisible();
      await jira.fill(''); await expect(page.getByRole('option')).toHaveCount(50); await page.getByRole('button', { name: 'Back from choose a jira issue' }).click();
      await page.getByRole('button', { name: 'Choose source session' }).click(); const source = page.getByRole('combobox', { name: 'Choose a source session' });
      await page.evaluate(() => { window.fixture.mode = 'absent'; }); await source.fill('00000000-0000-4000-8000-000000000070'); await expect(page.getByText('Session not found.', { exact: true })).toBeVisible();
      await page.evaluate(() => { window.fixture.mode = 'transport'; }); await source.fill('00000000-0000-4000-8000-000000000069'); await expect(page.getByText(/Session lookup failed: Fixture transport timeout/)).toBeVisible();
      await page.evaluate(() => { window.fixture.mode = 'context-error'; }); await page.getByRole('button', { name: 'Retry', exact: true }).click(); await expect(page.getByRole('option')).toHaveAccessibleName(/Human context 69.*Context could not be read/);
      await page.screenshot({ path: path.join(output, 'source-context-unavailable.png') });
    } finally { await context.close(); }
  });
  for (const width of [320, 390]) for (const mode of ['screen', 'composer']) await run(width + '-' + mode + '-combobox-keyboard', async () => {
    const { page, context } = await start(width, mode);
    try {
      await page.getByRole('radio', { name: /^From Jira ticket/ }).click();
      const search = page.getByRole('combobox', { name: 'Choose a Jira issue' }); await search.fill('Outside initial window');
      const option = page.locator('#kontor-jira-picker-option-ASMA-811');
      await expect(page.getByRole('option')).toHaveCount(1);
      await search.press('ArrowDown');
      await expect(search).toHaveAttribute('aria-activedescendant', 'kontor-jira-picker-option-ASMA-811');
      await expect(option).toHaveAttribute('aria-selected', 'true');
      await search.press('Home');
      await expect(search).toHaveAttribute('aria-activedescendant', 'kontor-jira-picker-option-ASMA-811');
      await search.press('End');
      await expect(search).toHaveAttribute('aria-activedescendant', 'kontor-jira-picker-option-ASMA-811');
      await search.press('ArrowUp');
      await expect(search).toHaveAttribute('aria-activedescendant', 'kontor-jira-picker-option-ASMA-811');
      await search.press('Enter');
      await page.getByRole('button', { name: 'Change Jira issue' }).click();
      await search.fill('Outside initial window');
      await expect(page.getByRole('option')).toHaveCount(1);
      await search.press('ArrowDown');
      await search.press('Tab');
      if (mode === 'screen') await expect(page.getByRole('dialog')).toContainText('Choose a Jira issue');
      else await expect(page.getByRole('button', { name: 'Clear choose a jira issue search', exact: true })).toBeFocused();
      await search.press('Escape');
      await expect(page.getByRole('button', { name: 'Change Jira issue' })).toBeFocused();
    } finally { await context.close(); }
  });
  await run('source-create-revalidation-title-error-clear-and-optional-fresh-start', async () => {
    const { page, context } = await start(390);
    try {
      await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Keep typed topic');
      await page.getByRole('button', { name: 'Choose source session' }).click(); await page.getByRole('option').first().click();
      await page.evaluate(() => { window.fixture.sourceTitle = 'Renamed human source'; });
      await page.getByTestId('kontor-new-lsa-create').click(); await expect(page.getByText(/source session.s title changed/)).toBeVisible();
      await expect(page.getByTestId('kontor-source-selected')).toHaveText('Renamed human source');
      await expect(page.getByRole('textbox', { name: 'Topic', exact: true })).toHaveValue('Keep typed topic');
      assert.equal(await page.evaluate(() => window.fixture.calls.some(call => call.name === 'sessions.create')), false);
      await page.getByRole('button', { name: 'Change source' }).click(); await page.getByRole('option').first().click();
      await page.evaluate(() => { window.fixture.mode = 'transport'; });
      await page.getByTestId('kontor-new-lsa-create').click(); await expect(page.getByText(/Source lookup failed: Fixture transport timeout/)).toBeVisible();
      assert.equal(await page.evaluate(() => window.fixture.calls.some(call => call.name === 'sessions.create')), false);
      await page.getByRole('button', { name: 'Clear source' }).click();
      await page.getByTestId('kontor-new-lsa-create').click(); await page.getByTestId('kontor-new-lsa-verified').waitFor();
      const calls = await page.evaluate(() => window.fixture.calls.filter(call => call.name === 'sessions.create'));
      assert.equal(calls.length, 1); assert.equal(calls[0].input.sourceAgentId, null);
    } finally { await context.close(); }
  });
  await run('source-create-exact-selected-ID', async () => {
    const { page, context } = await start(390);
    try {
      await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Exact source context');
      await page.getByRole('button', { name: 'Choose source session' }).click();
      await page.getByRole('combobox', { name: 'Choose a source session' }).fill('00000000-0000-4000-8000-000000000070');
      await page.getByRole('option').click(); await page.getByTestId('kontor-new-lsa-create').click(); await page.getByTestId('kontor-new-lsa-verified').waitFor();
      const calls = await page.evaluate(() => window.fixture.calls.filter(call => call.name === 'sessions.create'));
      assert.equal(calls.length, 1); assert.equal(calls[0].input.sourceAgentId, '00000000-0000-4000-8000-000000000070');
    } finally { await context.close(); }
  });
})().catch(error => { report.error = String(error.stack); }).finally(async () => {
  if (browser) await browser.close(); if (server) await server.close(); fs.rmSync(fixture, { recursive: true, force: true });
  fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2)); if (report.error || report.cases.some(row => row.status === 'FAIL')) process.exitCode = 1;
});
