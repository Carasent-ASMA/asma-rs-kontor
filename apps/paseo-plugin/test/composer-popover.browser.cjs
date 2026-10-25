// Synthetic React Native Web fixture only. No daemon, account, native session or device effects.
// The real Start or switch popover body runs inside a synthetic host popover (desktop) or bottom sheet (compact);
// a stand-in Kontor screen runs the real openHandOff on its `target` param, as the Kontor screen does.
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
const { chromium, expect } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-composer-popover-browser-'));
fs.mkdirSync(output, { recursive: true });
const rootOf = (resolver, name) => path.dirname(resolver.resolve(name + '/package.json'));
const react = rootOf(renderer, 'react'), dom = rootOf(renderer, 'react-dom'), native = rootOf(renderer, 'react-native-web');
assert.equal(renderer('react/package.json').version, '19.1.0');
assert.equal(renderer('react-dom/package.json').version, '19.1.0');
assert.equal(renderer('react-native-web/package.json').version, '0.21.4');
const client = file => JSON.stringify(path.join(plugin, 'client', file));
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'native.ts'), 'export * from "react-native-web";\n');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), `
export const useRpc = contract => input => window.fixture.rpc(contract.name, input);
export const getPaseoClient = hostId => window.fixture.client(hostId);
export const useHosts = () => [{ serverId: 'fixture-a', label: 'Fixture host', status: 'online' }];
`);
fs.writeFileSync(path.join(fixture, 'sdk-native.tsx'), `import React from 'react';
export {TextInput,ScrollView} from 'react-native';
export const useToast = () => ({ error: message => { window.fixture.toasts.push(message); }, show: () => {} });
export const Icon = ({size=16,color}) => <span aria-hidden="true" style={{width:size,height:size,color}}>‹</span>;
`);
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React,{StrictMode,useEffect,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client'; import {Pressable,ScrollView,Text,View} from 'react-native';
import {useToast} from '@getpaseo/plugin/client/react-native';
import {StartSwitchPopover} from ${client('start-switch-sheet.tsx')};
import {openHandOff} from ${client('session-actions.ts')};
import {configureRecentStorage,readRecent,recordVisit} from ${client('recent-sessions.ts')};
import {useSessionDirectory} from ${client('use-session-directory.ts')};
document.body.style.margin='0';
const query=new URLSearchParams(location.search), dark=query.get('theme')==='dark', compact=innerWidth<700;
const colors={surface0:dark?'#15171c':'#fff',surface1:dark?'#1f222a':'#f5f6f8',surface2:dark?'#2e333e':'#e6ebf2',foreground:dark?'#ecedf0':'#20242c',foregroundMuted:dark?'#a8adb7':'#5e6570',border:dark?'#414754':'#c7cdd7',accent:dark?'#9cbfff':'#245bba',accentForeground:dark?'#15171c':'#fff',statusSuccess:'#168047',statusWarning:'#a76b0a',statusDanger:'#b02222'};
const HOST='fixture-a', TITLE='Start or switch session';
const values=new Map(); configureRecentStorage({read:key=>values.get(key),write:(key,value)=>values.set(key,value)});
const page=(id,title,archivedAt=null)=>({agent:{id,title,status:'idle',provider:'fixture',cwd:'/fixture',workspaceId:'wks-adam',model:null,labels:{},createdAt:'2026-10-22T09:00:00.000Z',updatedAt:'2026-10-22T10:00:00.000Z',archivedAt,pendingPermissions:[],requiresAttention:false,attentionReason:null,attentionTimestamp:null},project:null});
// Two sessions share a title: only the exact ID may leave a row. 25 searchable batches exceed the 20 shown matches.
const rows=[page('dup-1','Route review'),page('dup-2','Route review'),page('deploy-7','Deploy checklist'),...Array.from({length:25},(_,i)=>page('batch-'+String(i+1).padStart(2,'0'),'Batch '+String(i+1).padStart(2,'0'))),page('batch-archived','Batch archived','2026-10-22T11:00:00.000Z')];
const list=async(options={})=>{const visible=rows.filter(row=>options.filter?.includeArchived===true||row.agent.archivedAt===null);
 if(options.subscribe){const limit=options.page?.limit??200,entries=visible.slice(0,limit);const snapshot={requestId:'fixture',subscriptionId:'sub-1',entries,pageInfo:{hasMore:visible.length>entries.length,nextCursor:visible.length>entries.length?String(entries.length):null,prevCursor:null}};
  return {...snapshot,subscription:{subscriptionId:'sub-1',ready:Promise.resolve(snapshot),subscribe:observer=>{observer.snapshot();return()=>{};},release:async()=>{}}};}
 const start=Number(options.page?.cursor??0),end=start+(options.page?.limit??200);
 return {requestId:'fixture',entries:visible.slice(start,end),pageInfo:{hasMore:end<visible.length,nextCursor:end<visible.length?String(end):null,prevCursor:null}};};
const now=Date.now(); recordVisit(HOST,'dup-1',now-120000); recordVisit(HOST,'dup-2',now-60000);
const catalog={enabled:true,disabledReason:null,pending:null,notes:[],catalogProjectId:'project-adam',issues:[],jiraProblem:null,
 desks:[{projectId:'project-adam',project:'DESK • ADAM',desk:'ADAM',kind:'adam',cwd:'/fixture/project-adam',lead:{ADHOC:'wks-adhoc',LSA:'wks-lsa',TPM:'wks-tpm'}}],profile:null,
 providers:[{id:'active',label:'Active provider',status:'ready',available:true,note:null,persona:'supported',models:[{id:'first',label:'Active model',efforts:[],defaultEffort:null}],defaultModel:'first',modes:[],defaultMode:null}]};
window.fixture={calls:[],screens:[],opens:[],searches:[],toasts:[],results:[],closes:0,holdArrival:false,holdClose:false,failOpenScreen:false,
 client:hostId=>{if(hostId!==HOST)throw new Error('Unexpected host '+hostId);return {agents:{list}};},
 recent:()=>readRecent(HOST).record.visits.map(visit=>visit.id),
 rpc:async(name,input)=>{window.fixture.calls.push({name,input});
  if(name==='sessions.create-options'){await new Promise(resolve=>setTimeout(resolve,10));return catalog;}
  if(name==='sessions.create'){await new Promise(resolve=>setTimeout(resolve,120));return {requestId:input.requestId,state:'verified',title:'LSA • ADHOC • '+input.topic,agentId:'fixture-created-exact',workspaceId:'wks-adhoc',message:'Created and verified.',differences:[],persona:'supported',checkedAt:new Date().toISOString()};}
  throw new Error('Unexpected RPC in isolated fixture: '+name);
 }};
const navigation={openAgent:input=>{window.fixture.opens.push(input);},openWorkspace:()=>{throw new Error('Unexpected workspace navigation');}};
/** Stand-in for the Kontor screen's hand-off effect: the same call, keyed on the same param. */
function KontorStandIn({params}){const directory=useSessionDirectory({hostId:HOST,hostStatus:'online'});const toast=useToast();
 useEffect(()=>{const result=openHandOff({hostId:HOST,key:params.target,entries:directory.state.entries,navigation,onSearch:text=>window.fixture.searches.push(text)});
  window.fixture.results.push(result);if(result&&!result.ok&&result.reason!=='stale')toast.error(result.message);},[params.target]);
 return <Text testID="fixture-screen" style={{color:colors.foreground,margin:16}}>Kontor {params.target?'with a request':'without a request'}</Text>;}
function App(){const pill=useRef(null);const [open,setOpen]=useState(false),[route,setRoute]=useState(null),[mount,setMount]=useState(0);
 const dismiss=()=>{setOpen(false);setTimeout(()=>pill.current?.focus?.(),0);};
 window.fixture.show=index=>setRoute(window.fixture.screens[index]);window.fixture.remount=()=>setMount(value=>value+1);window.fixture.release=dismiss;
 // Synthetic host: Escape closes the popover at the window's bubble phase and focus returns to the pill.
 useEffect(()=>{if(!open)return;const key=event=>{if(event.key==='Escape')dismiss();};window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);},[open]);
 const openScreen=input=>{if(window.fixture.failOpenScreen)throw new Error('Fixture could not open Kontor.');window.fixture.screens.push(input);if(!window.fixture.holdArrival)setRoute(input);};
 const close=()=>{window.fixture.closes++;if(!window.fixture.holdClose)dismiss();};
 const content=<StartSwitchPopover host={{id:HOST,label:'Fixture host'}} theme={{colors}} layout={{compact,platform:'web'}} context="agent" workspaceId="wks-adam" agentId="dup-1" close={close} openScreen={openScreen} screenId="kontor"/>;
 const frame={borderWidth:1,borderColor:colors.border,backgroundColor:colors.surface1,overflow:'hidden'};
 return <View style={{height:innerHeight,width:innerWidth,backgroundColor:colors.surface0}}>
  {route?<StrictMode key={mount}><KontorStandIn params={route.params??{}}/></StrictMode>:<Text style={{color:colors.foreground,margin:16}}>Composer</Text>}
  <Pressable ref={pill} testID="host-pill" accessibilityRole="button" accessibilityLabel="Kontor" onPress={()=>setOpen(!open)} style={{position:'absolute',left:16,bottom:16,minHeight:44,minWidth:88,justifyContent:'center',paddingHorizontal:12,borderWidth:1,borderColor:colors.border,borderRadius:22}}><Text style={{color:colors.foreground}}>Kontor</Text></Pressable>
  {open&&!compact?<View testID="host-popover" role="dialog" aria-label={TITLE} style={{...frame,position:'absolute',left:16,bottom:68,minWidth:280,maxWidth:Math.min(420,innerWidth-32),maxHeight:440,borderRadius:10}}>
   <ScrollView testID="host-scroller" style={{flexGrow:0}} contentContainerStyle={{padding:8}}>{content}</ScrollView></View>:null}
  {open&&compact?<>
   <Pressable testID="host-backdrop" accessibilityLabel="Dismiss" onPress={dismiss} style={{position:'absolute',inset:0,backgroundColor:'rgba(0,0,0,0.35)'}}/>
   <View testID="host-popover" role="dialog" aria-label={TITLE} style={{...frame,position:'absolute',left:0,right:0,bottom:0,maxHeight:Math.round(innerHeight*0.85),borderTopLeftRadius:14,borderTopRightRadius:14}}>
    <Text role="heading" style={{color:colors.foreground,fontSize:15,fontWeight:'600',padding:12,paddingBottom:4}}>{TITLE}</Text>
    <ScrollView testID="host-scroller" style={{flexGrow:0}} contentContainerStyle={{padding:8,paddingBottom:24}}>{content}</ScrollView></View></>:null}
 </View>;}
createRoot(document.getElementById('root')).render(<App/>);
`);
const report = { cases: [], nativeDevice: 'unverified', hostChrome: 'synthetic popover and bottom sheet', screen: 'stand-in running the real openHandOff' };
let server, browser;
async function pageFor(width, theme = 'light') {
  const context = await browser.newContext({ viewport: { width, height: 844 }, hasTouch: width < 700, isMobile: width < 700 });
  const page = await context.newPage();
  page.setDefaultTimeout(5000);
  await page.goto(server.resolvedUrls.local[0] + '?theme=' + theme);
  await openPopover(page);
  return { page, context };
}
async function openPopover(page) {
  await page.getByTestId('host-pill').click();
  await page.getByTestId('kontor-switch-row-dup-1').waitFor();
}
const state = page => page.evaluate(() => ({ screens: window.fixture.screens, opens: window.fixture.opens, searches: window.fixture.searches, toasts: window.fixture.toasts, recent: window.fixture.recent(), closes: window.fixture.closes, results: window.fixture.results }));
/** A hand-off URL carries one opaque key and nothing that names the session. */
function assertKeyOnly(screen, forbidden) {
  assert.equal(screen.screenId, 'kontor');
  assert.deepEqual(Object.keys(screen.params), ['target']);
  for (const text of forbidden) assert.ok(!screen.params.target.includes(text), `key leaks ${text}`);
}
const within = (box, frame) => box.x >= frame.x - 0.5 && box.x + box.width <= frame.x + frame.width + 0.5;
async function run(name, test) {
  try { const extra = await test(); report.cases.push({ name, status: 'PASS', ...(extra ?? {}) }); }
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
    for (const width of [320, 390, 1440]) for (const theme of ['light', 'dark']) await run(`${width}-${theme}-layout`, async () => {
      const { page, context } = await pageFor(width, theme);
      try {
        const compact = width < 700;
        const frame = await page.getByTestId('host-popover').boundingBox();
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        const scroller = await page.getByTestId('host-scroller').evaluate(node => ({ scrollWidth: node.scrollWidth, clientWidth: node.clientWidth, scrollHeight: node.scrollHeight, clientHeight: node.clientHeight }));
        assert.ok(scroller.scrollWidth <= scroller.clientWidth, 'no horizontal scroll inside the popover');
        if (compact) assert.equal(Math.round(frame.width), width);
        else { assert.ok(frame.width >= 280 - 0.5 && frame.width <= 420 + 0.5); assert.ok(frame.height <= 440 + 0.5); }
        // The host owns the title and dismissal: the body adds neither.
        assert.equal(await page.getByText('Start or switch session', { exact: true }).count(), compact ? 1 : 0);
        assert.equal(await page.getByRole('heading', { name: 'Start or switch', exact: true }).count(), 0);
        assert.equal(await page.getByRole('button', { name: /^Close/ }).count(), 0);
        const controls = [page.getByRole('button', { name: 'Open Kontor', exact: true }), page.getByRole('button', { name: 'New session', exact: true }), page.getByRole('textbox', { name: 'Jump to any session', exact: true }),
          page.getByTestId('kontor-switch-row-dup-1'), page.getByTestId('kontor-switch-row-dup-2'), page.getByRole('button', { name: 'Clear recent on this device', exact: true })];
        await expect(controls[0]).toBeInViewport();
        await expect(controls[1]).toBeVisible();
        const sizes = [];
        for (const control of controls) {
          await control.scrollIntoViewIfNeeded();
          const box = await control.boundingBox();
          assert.ok(within(box, frame), 'control inside the popover');
          sizes.push(Math.round(box.height));
          if (compact) assert.ok(box.height >= 44, 'compact touch target');
        }
        // Equal titles stay two rows, each naming its own ID.
        assert.deepEqual(await page.getByRole('menuitem', { name: /^Route review,/ }).evaluateAll(nodes => nodes.map(node => node.dataset.testid)), ['kontor-switch-row-dup-2', 'kontor-switch-row-dup-1']);
        await page.getByTestId('host-scroller').evaluate(node => node.scrollTo(0, 0));
        await page.screenshot({ path: path.join(output, `${width}-${theme}-popover.png`) });
        return { frame, scroller, controlHeights: sizes };
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-row-hands-off-exact-id-once`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.evaluate(() => { window.fixture.holdArrival = true; });
        await page.getByTestId('kontor-switch-row-dup-1').click();
        await page.getByTestId('host-popover').waitFor({ state: 'detached' });
        let now = await state(page);
        assert.equal(now.screens.length, 1); assertKeyOnly(now.screens[0], ['dup', 'Route']);
        // Recent changes only once the screen has opened the session.
        assert.deepEqual(now.opens, []); assert.deepEqual(now.recent, ['dup-2', 'dup-1']);
        await page.evaluate(() => window.fixture.show(0));
        await page.getByText('Kontor with a request', { exact: true }).waitFor();
        now = await state(page);
        assert.deepEqual(now.opens, [{ agentId: 'dup-1', serverId: 'fixture-a' }]);
        assert.deepEqual(now.recent, ['dup-1', 'dup-2']);
        // StrictMode ran the effect twice: the second read found the key used.
        assert.deepEqual(now.results.map(result => result?.ok ? 'ok' : result?.reason), ['ok', 'stale']);
        await page.evaluate(() => window.fixture.remount());
        await page.evaluate(() => window.fixture.show(0));
        await page.waitForFunction(() => window.fixture.results.length >= 4);
        now = await state(page);
        assert.equal(now.opens.length, 1); assert.deepEqual(now.toasts, []);
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-open-kontor-without-request`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('button', { name: 'Open Kontor', exact: true }).click();
        await page.getByText('Kontor without a request', { exact: true }).waitFor();
        const now = await state(page);
        assert.deepEqual(now.screens, [{ screenId: 'kontor' }]);
        assert.equal(await page.getByTestId('host-popover').count(), 0);
        assert.deepEqual(now.opens, []); assert.deepEqual(now.searches, []); assert.deepEqual(now.toasts, []);
        assert.deepEqual(now.results, [null, null]);
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-search-show-all`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('textbox', { name: 'Jump to any session', exact: true }).fill('batch');
        const showAll = page.getByRole('button', { name: 'Show all 25 in Sessions', exact: true });
        await showAll.waitFor();
        assert.equal(await page.getByRole('menu', { name: 'Matching sessions', exact: true }).getByRole('menuitem').count(), 20);
        assert.equal(await page.getByTestId('kontor-switch-row-batch-archived').count(), 0);
        await page.screenshot({ path: path.join(output, `${width}-search.png`) });
        await showAll.click();
        await page.getByText('Kontor with a request', { exact: true }).waitFor();
        const now = await state(page);
        assert.equal(now.screens.length, 1); assertKeyOnly(now.screens[0], ['batch']);
        assert.deepEqual(now.searches, ['batch']); assert.deepEqual(now.opens, []); assert.deepEqual(now.toasts, []);
        assert.deepEqual(now.recent, ['dup-2', 'dup-1']);
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-new-lsa-inside-popover`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('button', { name: 'New session', exact: true }).click();
        await page.getByRole('heading', { name: 'New session', exact: true }).waitFor();
        await page.getByRole('textbox', { name: 'Topic', exact: true }).fill('Popover review');
        const create = page.getByTestId('kontor-new-lsa-create');
        await expect(create).toBeEnabled();
        await create.scrollIntoViewIfNeeded();
        const frame = await page.getByTestId('host-popover').boundingBox(), box = await create.boundingBox();
        assert.ok(within(box, frame) && box.y >= frame.y - 0.5 && box.y + box.height <= frame.y + frame.height + 0.5, 'Create reachable inside the popover');
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
        const scrollers = await page.evaluate(() => ['host-scroller', 'kontor-new-lsa'].map(id => { const node = document.querySelector(`[data-testid="${id}"]`); return { id, scrollHeight: node.scrollHeight, clientHeight: node.clientHeight }; }));
        await page.screenshot({ path: path.join(output, `${width}-new-lsa-form.png`) });
        await create.click();
        await page.getByTestId('kontor-new-lsa-verified').waitFor();
        await page.getByText('Session ID: fixture-created-exact', { exact: true }).waitFor();
        // A slow host close shows the hand-off status and offers no second Open.
        await page.evaluate(() => { window.fixture.holdArrival = true; window.fixture.holdClose = true; });
        await page.getByRole('button', { name: 'Open session', exact: true }).click();
        await expect(page.getByTestId('kontor-new-lsa-open-status')).toHaveText('Opening in Kontor…');
        assert.equal(await page.getByRole('button', { name: 'Open session', exact: true }).count(), 0);
        await page.screenshot({ path: path.join(output, `${width}-new-lsa-handed.png`) });
        let now = await state(page);
        assert.equal(now.closes, 1); assert.equal(now.screens.length, 1); assertKeyOnly(now.screens[0], ['fixture-created-exact', 'Popover']);
        assert.deepEqual(now.opens, []); assert.deepEqual(now.recent, ['dup-2', 'dup-1']);
        await page.evaluate(() => { window.fixture.release(); window.fixture.show(0); });
        await page.getByText('Kontor with a request', { exact: true }).waitFor();
        now = await state(page);
        assert.deepEqual(now.opens, [{ agentId: 'fixture-created-exact', serverId: 'fixture-a' }]);
        assert.deepEqual(now.recent, ['fixture-created-exact', 'dup-2', 'dup-1']);
        assert.equal(await page.evaluate(() => window.fixture.calls.filter(call => call.name === 'sessions.create').length), 1);
        assert.deepEqual(now.toasts, []);
        return { scrollers };
      } finally { await context.close(); }
    });
    // Observation only: the synthetic host's Escape is not Paseo's. Records where help draws and what each Escape closes.
    for (const width of [320, 390, 1440]) await run(`${width}-help-and-escape-observed`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('button', { name: 'New session', exact: true }).click();
        const back = page.getByRole('button', { name: 'Back to recent sessions', exact: true });
        const help = page.getByRole('tooltip'), popover = page.getByTestId('host-popover');
        const observe = async name => {
          const shown = await help.waitFor({ timeout: 2000 }).then(() => true, () => false);
          if (!shown) return { shown };
          const frame = await popover.boundingBox(), box = await help.boundingBox();
          await page.screenshot({ path: path.join(output, `${width}-help-${name}.png`) });
          return { shown, text: await help.innerText(), insidePopover: within(box, frame) && box.y >= frame.y - 0.5 && box.y + box.height <= frame.y + frame.height + 0.5,
            insideViewport: box.x >= 0 && box.y >= 0 && box.x + box.width <= width + 0.5 && box.y + box.height <= 844 + 0.5,
            unobscured: await page.evaluate(({ x, y }) => !!document.elementFromPoint(x, y)?.closest('[role="tooltip"]'), { x: box.x + box.width / 2, y: box.y + box.height / 2 }) };
        };
        const observation = {};
        if (width >= 700) {
          await back.hover();
          observation.hover = await observe('hover');
          await page.mouse.move(0, 0);
          await help.waitFor({ state: 'detached', timeout: 2000 }).catch(() => {});
        } else {
          // A touch long hold, which shows the label instead of activating on release.
          const box = await back.boundingBox(), point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
          const cdp = await context.newCDPSession(page);
          await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [point] });
          await page.waitForTimeout(700);
          observation.longHold = await observe('long-hold');
          await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
          observation.longHold.stayedInNewLsa = await page.getByRole('heading', { name: 'New session', exact: true }).count() === 1;
          await page.getByRole('textbox', { name: 'Topic', exact: true }).focus();
        }
        // Keyboard: leave the Back button and return to it.
        await back.focus(); await page.keyboard.press('Tab'); await page.keyboard.press('Shift+Tab');
        observation.keyboard = { focusOnBack: await back.evaluate(node => node === document.activeElement), ...await observe('keyboard') };
        await page.keyboard.press('Escape');
        observation.firstEscape = { helpOpen: await help.count() > 0, popoverOpen: await popover.count() > 0 };
        if (observation.firstEscape.popoverOpen) {
          await page.keyboard.press('Escape');
          observation.secondEscape = { helpOpen: await help.count() > 0, popoverOpen: await popover.count() > 0 };
        }
        await page.waitForTimeout(50);
        observation.focusOnPillAfterClose = await page.evaluate(() => document.activeElement?.dataset?.testid === 'host-pill');
        const now = await state(page);
        assert.deepEqual(now.screens, []); assert.deepEqual(now.opens, []); assert.deepEqual(now.toasts, []);
        return { observation };
      } finally { await context.close(); }
    });
    await run('1440-desk-inference-from-workspace', async () => {
      const { page, context } = await pageFor(1440);
      try {
        const input = await page.evaluate(() => window.fixture.calls.find(call => call.name === 'sessions.create-options')?.input);
        assert.deepEqual(input, { workspaceId: 'wks-adam' });
        await page.getByRole('button', { name: 'New session', exact: true }).click();
        const desk = page.getByRole('radio', { name: 'DESK • ADAM', exact: true });
        await desk.waitFor();
        assert.equal(await desk.getAttribute('aria-checked'), 'true');
      } finally { await context.close(); }
    });
    await run('390-stale-screen-leaves-newer-request', async () => {
      const { page, context } = await pageFor(390);
      try {
        await page.evaluate(() => { window.fixture.holdArrival = true; });
        await page.getByTestId('kontor-switch-row-dup-1').click();
        await openPopover(page);
        await page.getByTestId('kontor-switch-row-dup-2').click();
        await page.evaluate(() => window.fixture.show(0));
        await page.waitForFunction(() => window.fixture.results.length >= 2);
        let now = await state(page);
        assert.deepEqual(now.opens, []); assert.deepEqual(now.toasts, []);
        await page.evaluate(() => window.fixture.show(1));
        await page.waitForFunction(() => window.fixture.opens.length === 1);
        now = await state(page);
        assert.deepEqual(now.opens, [{ agentId: 'dup-2', serverId: 'fixture-a' }]);
        assert.deepEqual(now.toasts, []);
      } finally { await context.close(); }
    });
    await run('390-failed-screen-open-keeps-popover', async () => {
      const { page, context } = await pageFor(390);
      try {
        await page.evaluate(() => { window.fixture.failOpenScreen = true; });
        await page.getByTestId('kontor-switch-row-dup-1').click();
        let now = await state(page);
        assert.deepEqual(now.toasts, ['Fixture could not open Kontor.']);
        assert.equal(now.closes, 0); assert.equal(await page.getByTestId('host-popover').count(), 1);
        assert.deepEqual(now.screens, []); assert.deepEqual(now.opens, []);
        await page.evaluate(() => { window.fixture.failOpenScreen = false; });
        await page.getByTestId('kontor-switch-row-dup-1').click();
        await page.waitForFunction(() => window.fixture.opens.length === 1);
        now = await state(page);
        assert.deepEqual(now.opens, [{ agentId: 'dup-1', serverId: 'fixture-a' }]);
        assert.equal(now.toasts.length, 1);
      } finally { await context.close(); }
    });
  } finally { await browser.close(); await server.close(); }
  fs.writeFileSync(path.join(output, 'browser-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
  if (report.cases.some(test => test.status === 'FAIL')) process.exitCode = 1;
})().catch(error => { console.error(error); process.exitCode = 1; }).finally(async () => {
  await Promise.allSettled([browser?.close(), server?.close()]);
  fs.rmSync(fixture, { recursive: true, force: true });
});
