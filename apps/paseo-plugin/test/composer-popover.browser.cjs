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
const iconRoot = path.join(rootOf(renderer, 'lucide-react'), 'dist/esm/icons');
const fixtureGlyphs = { Play: 'play', MessageCircleQuestion: 'message-circle-question', Ban: 'ban', CircleAlert: 'circle-alert', X: 'x' };
const glyphImports = Object.entries(fixtureGlyphs).map(([name, file]) => `import ${name} from ${JSON.stringify(path.join(iconRoot, file + '.mjs'))};`).join('\n');
const client = file => JSON.stringify(path.join(plugin, 'client', file));
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'native.ts'), 'export * from "react-native-web";\n');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), `
export const useRpc = contract => input => window.fixture.rpc(contract.name, input);
export const getPaseoClient = hostId => window.fixture.client(hostId);
import {useSyncExternalStore} from 'react';
let cache;
// No saved new-lead default is read here, so a lead names its model explicitly.
export const useSettings = () => ({ status: 'loading' });
export const useHosts = () => useSyncExternalStore(listener=>{window.fixture.hostListeners.add(listener);return()=>window.fixture.hostListeners.delete(listener)},()=>{const status=window.fixture.hostStatus;if(cache?.[0].status!==status)cache=[{serverId:'fixture-a',label:'Fixture host',status}];return cache});
`);
fs.writeFileSync(path.join(fixture, 'sdk-native.tsx'), `import React from 'react';
${glyphImports}
const icons={Play,MessageCircleQuestion,Ban,CircleAlert,X};
import {Pressable,Text,View} from 'react-native';
export {TextInput,ScrollView,FlatList} from 'react-native';
export const copyText = async () => {};
export const Modal = Object.assign(({open,title,onOpenChange,children}) => open ? <View role="dialog" aria-label={title}><Pressable accessibilityRole="button" accessibilityLabel="Close" onPress={()=>onOpenChange(false)}><Text>×</Text></Pressable>{children}</View> : null, { Content: ({children}) => <View>{children}</View> });
export const useToast = () => ({ error: message => { window.fixture.toasts.push(message); }, show: () => {} });
export const Icon = ({name,size=16,color}) => {const Glyph=icons[name];return Glyph?<Glyph aria-hidden="true" width={size} height={size} color={color}/>:<span aria-hidden="true" style={{width:size,height:size,color}}>‹</span>;};
`);
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React,{StrictMode,useEffect,useRef,useState} from 'react';
import {createRoot} from 'react-dom/client'; import {Pressable,ScrollView,Text,View} from 'react-native';
import {useToast} from '@getpaseo/plugin/client/react-native';
import {StartSwitchPopover} from ${client('start-switch-sheet.tsx')};
import {openHandOff} from ${client('session-actions.ts')};
import {configureRecentStorage,readRecent,recordVisit} from ${client('recent-sessions.ts')};
import {useSessionDirectory,startHostDirectory} from ${client('use-session-directory.ts')};
import {switchViewPatch} from ${client('session-state-shortcuts.ts')};
import {readSessionView,patchSessionView} from ${client('session-view-store.ts')};
import {filterDirectoryEntries} from ${client('session-view.ts')};
import {providerBlockedView} from ${client('provider-blocked-filter.ts')};
document.body.style.margin='0';
const query=new URLSearchParams(location.search), dark=query.get('theme')==='dark', compact=innerWidth<700;
const colors={surface0:dark?'#15171c':'#fff',surface1:dark?'#1f222a':'#f5f6f8',surface2:dark?'#2e333e':'#e6ebf2',foreground:dark?'#ecedf0':'#20242c',foregroundMuted:dark?'#a8adb7':'#5e6570',border:dark?'#414754':'#c7cdd7',accent:dark?'#9cbfff':'#245bba',accentForeground:dark?'#15171c':'#fff',statusSuccess:'#168047',statusWarning:'#a76b0a',statusDanger:'#b02222'};
const HOST='fixture-a', TITLE='Start or switch session';
const reviews=new Map();
const values=new Map(); configureRecentStorage({read:key=>values.get(key),write:(key,value)=>values.set(key,value)});
const page=(id,title,archivedAt=null)=>({agent:{id,title,status:'idle',provider:'fixture',cwd:'/fixture',workspaceId:'wks-adam',model:null,labels:{},createdAt:'2026-10-22T09:00:00.000Z',updatedAt:'2026-10-22T10:00:00.000Z',archivedAt,pendingPermissions:[],requiresAttention:false,attentionReason:null,attentionTimestamp:null},project:null});
// Two sessions share a title: only the exact ID may leave a row. 25 searchable batches exceed the 20 shown matches.
const rows=[page('dup-1','Route review'),page('dup-2','Route review'),page('deploy-7','Deploy checklist'),...Array.from({length:25},(_,i)=>page('batch-'+String(i+1).padStart(2,'0'),'Batch '+String(i+1).padStart(2,'0'))),page('batch-archived','Batch archived','2026-10-22T11:00:00.000Z')];
Object.assign(rows[0].agent,{status:'running'});Object.assign(rows[1].agent,{attentionReason:'permission',requiresAttention:true});Object.assign(rows[2].agent,{attentionReason:'finished',requiresAttention:true});Object.assign(rows[3].agent,{status:'error'});Object.assign(rows[4].agent,{status:'running',attentionReason:'permission',requiresAttention:true});
const list=async(options={})=>{window.fixture.inventoryCalls.push(options);const visible=rows.filter(row=>options.filter?.includeArchived===true||row.agent.archivedAt===null);
 if(options.subscribe){const limit=options.page?.limit??200,entries=visible.slice(0,limit);const snapshot={requestId:'fixture',subscriptionId:'sub-1',entries,pageInfo:{hasMore:visible.length>entries.length,nextCursor:visible.length>entries.length?String(entries.length):null,prevCursor:null}};
  return {...snapshot,subscription:{subscriptionId:'sub-1',ready:Promise.resolve(snapshot),subscribe:observer=>{window.fixture.observers.push(observer);observer.snapshot();return()=>{};},release:async()=>{}}};}
 const start=Number(options.page?.cursor??0),end=start+(options.page?.limit??200);
 return {requestId:'fixture',entries:visible.slice(start,end),pageInfo:{hasMore:end<visible.length,nextCursor:end<visible.length?String(end):null,prevCursor:null}};};
const now=Date.now(); recordVisit(HOST,'dup-1',now-120000); recordVisit(HOST,'dup-2',now-60000);
const catalog={enabled:true,disabledReason:null,pending:null,notes:[],catalogProjectId:'project-adam',issues:[],jiraProblem:null,
 desks:[{projectId:'project-adam',project:'DESK • ADAM',desk:'ADAM',kind:'adam',cwd:'/fixture/project-adam',lead:{ADHOC:'wks-adhoc',LSA:'wks-lsa',TPM:'wks-tpm'}}],profile:null,
 providers:[{id:'active',label:'Active provider',status:'ready',available:true,note:null,persona:'supported',models:[{id:'first',label:'Active model',efforts:[],defaultEffort:null}],defaultModel:'first',modes:[],defaultMode:null}]};
window.fixture={archiveMode:'confirmed',hostStatus:'online',hostListeners:new Set(),inventoryCalls:[],observers:[],views:[],draft:'unfinished chat draft',pendingQuestion:'pending-permission',calls:[],screens:[],opens:[],toasts:[],results:[],closes:0,holdArrival:false,holdClose:false,failOpenScreen:false,
 client:hostId=>{if(hostId!==HOST)throw new Error('Unexpected host '+hostId);return {agents:{list}};},
 recent:()=>readRecent(HOST).record.visits.map(visit=>visit.id),
 rpc:async(name,input)=>{window.fixture.calls.push({name,input});
  if(name==='sessions.create-options'){await new Promise(resolve=>setTimeout(resolve,10));return catalog;}
  if(name==='sessions.create-features')return {features:[],problem:null};
  if(name==='sessions.navigator-capabilities'){const no={supported:false,reason:'Not offered by this fixture host.'};return {usageTotals:no,summaries:no,draftAttach:no,takeover:no,takeoverBulk:no,takeoverRoute:no};}
  if(name==='sessions.create'){await new Promise(resolve=>setTimeout(resolve,120));return {requestId:input.requestId,state:'verified',title:'LSA • ADHOC • '+input.topic,agentId:'fixture-created-exact',workspaceId:'wks-adhoc',message:'Created and verified.',differences:[],persona:'supported',checkedAt:new Date().toISOString()};}
  if(name==='sessions.lifecycle-preview'){
   const item=rows.find(row=>row.agent.id===input.agentIds[0]);if(!item)throw new Error('Unknown exact ID');const a=item.agent,blocked=window.fixture.archiveMode==='blocked';
   const row={id:a.id,title:a.title,workspaceId:a.workspaceId,cwd:a.cwd,provider:a.provider,model:a.model,effort:null,mode:null,nativeStatus:a.status,fingerprint:'fixture',category:'candidate',eligible:!blocked,reason:blocked?'Managed child is protected.':'Fixture eligible.',children:[],outcome:blocked?'blocked':'pending',message:null};
   const review={id:'00000000-0000-4000-8000-'+String(reviews.size+1).padStart(12,'0'),action:'archive',scope:'filtered',createdAt:new Date().toISOString(),target:input.target,rows:[row]};reviews.set(review.id,review);return review;
  }
  if(name==='sessions.lifecycle-execute-item'){
   await new Promise(resolve=>setTimeout(resolve,120));const review=reviews.get(input.reviewId),row=review.rows.find(row=>row.id===input.agentId);if(!row||!row.eligible)throw new Error('Unexpected archive execute');
   const item=rows.find(value=>value.agent.id===input.agentId),uncertain=window.fixture.archiveMode==='uncertain';
   if(!uncertain){item.agent={...item.agent,status:'closed',archivedAt:new Date().toISOString(),updatedAt:new Date().toISOString()};for(const observer of window.fixture.observers)observer.update({type:'agent_update',payload:{kind:'upsert',agent:item.agent,project:null}});}
   return {...row,outcome:uncertain?'uncertain':'archived',runtime:uncertain?'unconfirmed':'closed',availability:{agentId:row.id,state:'readable',nativeStatus:item.agent.status,archivedAt:item.agent.archivedAt,revision:item.agent.updatedAt,checkedAt:new Date().toISOString(),reason:'Fixture exact read.'}};
  }
  throw new Error('Unexpected RPC in isolated fixture: '+name);
 }};
window.fixture.setHost=status=>{window.fixture.hostStatus=status;for(const listener of window.fixture.hostListeners)listener()};
startHostDirectory(HOST,'online');
const navigation={openAgent:input=>{window.fixture.opens.push(input);},openWorkspace:()=>{throw new Error('Unexpected workspace navigation');}};
/** Stand-in for the Kontor screen's hand-off effect: the same call, keyed on the same param. */
function KontorStandIn({params}){const directory=useSessionDirectory({hostId:HOST,hostStatus:'online'});const toast=useToast();const [view,setView]=useState(()=>readSessionView(HOST));
 // The switcher's state, search and membership arrive whole, as the Kontor screen's Sessions applies them.
 useEffect(()=>{const result=openHandOff({hostId:HOST,key:params.target,entries:directory.state.entries,navigation,onView:shown=>{const next=patchSessionView(HOST,switchViewPatch(readSessionView(HOST),shown));setView(next);window.fixture.views.push(shown)}});
  window.fixture.results.push(result);if(result&&!result.ok&&result.reason!=='stale')toast.error(result.message);},[params.target]);
 const matching=view.status==='provider_blocked'?providerBlockedView({state:directory.state,includeArchived:false,candidates:directory.state.entries,reads:new Map(),freshness:'Live',blocked:null}).sessions:filterDirectoryEntries(directory.state.entries,view);
 return <View><Text testID="fixture-status-filter" style={{color:colors.foreground}}>{view.status}</Text><Text testID="fixture-status-ids" style={{color:colors.foreground}}>{matching.map(entry=>entry.id).join(',')}</Text><Text testID="fixture-screen" style={{color:colors.foreground,margin:16}}>Kontor {params.target?'with a request':'without a request'}</Text></View>;}
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
  const context = await browser.newContext({ viewport: { width, height: 844 }, reducedMotion: width === 320 ? 'reduce' : 'no-preference', hasTouch: width < 700, isMobile: width < 700 });
  const page = await context.newPage();
  page.setDefaultTimeout(5000);
  await page.goto(server.resolvedUrls.local[0] + '?theme=' + theme, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(()=>window.fixture?.inventoryCalls.length>=2);
  await openPopover(page);
  return { page, context };
}
async function openPopover(page) {
  await page.getByTestId('host-pill').click();
  await page.getByTestId('session-row-dup-1').waitFor();
}
const state = page => page.evaluate(() => ({ screens: window.fixture.screens, opens: window.fixture.opens, views: window.fixture.views, toasts: window.fixture.toasts, recent: window.fixture.recent(), closes: window.fixture.closes, results: window.fixture.results }));
/** A hand-off URL carries one opaque key and nothing that names the session. */
function assertKeyOnly(screen, forbidden) {
  assert.equal(screen.screenId, 'kontor');
  assert.deepEqual(Object.keys(screen.params), ['target']);
  for (const text of forbidden) assert.ok(!screen.params.target.includes(text), `key leaks ${text}`);
}
const within = (box, frame) => box.x >= frame.x - 0.5 && box.x + box.width <= frame.x + frame.width + 0.5;
// A row's actions are its trailing menu (board C1); nothing runs until one is chosen.
const actionsMenu = page => page.getByRole('dialog', { name: 'Session actions', exact: true });
async function rowAction(page, id, key) {
  await page.getByTestId(`session-actions-${id}`).click();
  await actionsMenu(page).getByTestId(`session-action-${key}`).click();
  await actionsMenu(page).waitFor({ state: 'detached' });
}
const ROW_PARTS = new Set(['state', 'body', 'opened', 'origin', 'model', 'tokens', 'note', 'choice']);
const listedIds = page => page.getByTestId('kontor-switch-list').evaluate(list => [...list.querySelectorAll('[data-testid^="session-row-"]')].map(node => node.dataset.testid.slice(12)))
  .then(ids => ids.filter(id => !ROW_PARTS.has(id)));
// Every nonarchived session is listed under All, one of the nine states: a tile where the toolbar has room for it, else in More.
async function pickState(page, status) {
  const tile = page.getByTestId('kontor-switch-state-' + status);
  if (await tile.count()) return tile.click();
  await page.getByTestId('kontor-switch-more').click(); await page.getByTestId('kontor-switch-more-' + status).click();
}
const showAll = page => pickState(page, 'all');
// This host lists no Codex provider and no saved default is read, so a lead names its model explicitly.
async function chooseModel(page) {
  await page.getByTestId('kontor-new-lead-model').click();
  await page.getByRole('radio', { name: /^Active model, / }).click();
  await page.getByTestId('kontor-model-use').click();
  await page.getByTestId('kontor-model-picker').waitFor({ state: 'detached' });
}
async function run(name, test) {
  if(process.env.CASE_FILTER&&(!new RegExp(process.env.CASE_FILTER).test(name)||report.cases.some(test=>test.status==='FAIL')))return;
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
    for(const width of [320,390,1440]) await run(`archive-menu-${width}`,async()=>{
      const {page,context}=await pageFor(width,'light');const errors=[];page.on('pageerror',error=>errors.push(error.message));
      const count=async name=>page.evaluate(name=>window.fixture.calls.filter(call=>call.name===name).length,name);
      try{
        const initial=await page.evaluate(()=>({inventory:window.fixture.inventoryCalls.length,lifecycle:window.fixture.calls.filter(call=>call.name.startsWith('sessions.lifecycle')).length}));assert.equal(initial.lifecycle,0);
        const trigger=page.getByTestId('session-actions-dup-1'),open=page.getByTestId('session-row-dup-1');
        const box=await trigger.boundingBox(),openBox=await open.getByTestId('session-row-body').boundingBox();assert.ok(box.width>=40&&box.height>=44);assert.ok(openBox.x+openBox.width<=box.x+0.5,'Open and the actions menu have independent hit areas');
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);await page.screenshot({path:path.join(output,`archive-menu-${width}.png`)});
        await trigger.focus();assert.equal(await trigger.evaluate(node=>document.activeElement===node),true);await trigger.press('Enter');
        const archive=actionsMenu(page).getByTestId('session-action-archive');await archive.focus();await archive.press('Enter');
        // Asserted last, so the outcome below is still proven when no progress shows.
        const busy=await page.locator('[aria-busy="true"]').or(page.getByText(/^Archiving/)).first().waitFor({timeout:1000}).then(()=>true,()=>false);
        await expect(open).toHaveCount(0);await expect(page.getByTestId('kontor-switch-archive-result')).toHaveText('Archived Route review.');
        await expect(page.getByTestId('kontor-switch-state-running')).toHaveText('1');assert.equal(await count('sessions.lifecycle-preview'),1);assert.equal(await count('sessions.lifecycle-execute-item'),1);
        assert.deepEqual(await page.evaluate(()=>({screens:window.fixture.screens.length,opens:window.fixture.opens.length,closes:window.fixture.closes,draft:window.fixture.draft,pending:window.fixture.pendingQuestion})),{screens:0,opens:0,closes:0,draft:'unfinished chat draft',pending:'pending-permission'});
        await page.evaluate(()=>window.fixture.archiveMode='blocked');await rowAction(page,'dup-2','archive');
        await expect(page.getByTestId('kontor-switch-archive-result')).toHaveText('Managed child is protected.');await expect(page.getByTestId('session-row-dup-2')).toHaveCount(1);assert.equal(await count('sessions.lifecycle-execute-item'),1);
        await showAll(page);await page.getByRole('textbox',{name:'Find a session',exact:true}).fill('Batch');await page.evaluate(()=>window.fixture.archiveMode='uncertain');
        await rowAction(page,'batch-01','archive');
        await expect(page.getByTestId('kontor-switch-archive-result')).toContainText('archive result unknown');
        // An unresolved archive holds the row's Archive until its review settles.
        await page.getByTestId('session-actions-batch-01').click();const held=actionsMenu(page).getByTestId('session-action-archive');
        assert.equal(await held.getAttribute('aria-label'),'Archive. Archive is not available for this session right now.');await held.press('Enter');await expect(held).toHaveCount(1);
        const heldDisabled=await held.getAttribute('aria-disabled');
        await actionsMenu(page).getByRole('button',{name:'Close',exact:true}).click();await actionsMenu(page).waitFor({state:'detached'});
        await expect(page.getByTestId('session-row-batch-01')).toHaveCount(1);await expect(page.getByRole('textbox',{name:'Find a session',exact:true})).toHaveValue('Batch');
        await expect(page.getByRole('button',{name:'Open Sessions to review archive',exact:true})).toHaveCount(1);assert.equal(await count('sessions.lifecycle-execute-item'),2);
        assert.equal(await page.evaluate(()=>window.fixture.inventoryCalls.length),initial.inventory,'Archive UI must not add inventory reads');await page.screenshot({path:path.join(output,`archive-menu-${width}-uncertain.png`)});
        assert.deepEqual(errors,[]);
        assert.deepEqual([busy?null:'a running single archive shows it is in progress',heldDisabled==='true'?null:'an unavailable menu item is exposed as disabled'].filter(Boolean),[]);
        return{box,openBox,inventoryReads:initial.inventory,preview:await count('sessions.lifecycle-preview'),execute:await count('sessions.lifecycle-execute-item'),exactId:true,noOpenOrClose:true,uncertainRetained:true,errors};
      }finally{await context.close();}
    });
    for (const width of [320,390,1440]) for (const theme of ['light','dark']) await run(`statuses-${width}-${theme}`,async()=>{
      const {page,context}=await pageFor(width,theme);const errors=[];page.on('pageerror',error=>errors.push(error.message));
      try{
        await expect(page.getByTestId('kontor-switch-state-running')).toHaveText('2');
        // The strip counts this host's nonarchived sessions; the heading names the host.
        await expect(page.getByText('This host',{exact:true})).toHaveCount(1);
        const expected={running:'2',needs_input:'2',error:'1'},boxes=[];
        // A state the toolbar has no whole tile for keeps its count in More.
        for(const [status,count] of Object.entries(expected)){const control=page.getByTestId('kontor-switch-state-'+status);
          if(!await control.count()){await page.getByTestId('kontor-switch-more').click();assert.ok((await page.getByTestId('kontor-switch-more-'+status).getAttribute('aria-label')).endsWith(', '+count));await page.getByTestId('kontor-switch-more').click();continue;}
          assert.equal(await control.innerText(),count);const box=await control.boundingBox();assert.ok(box.width>=40&&box.height>=40,'state cells keep the strip\'s 40pt floor');boxes.push(box);}
        // Provider blocked stays unknown until Kontor's Sessions reads the refusals; it is under More.
        await page.getByTestId('kontor-switch-more').click();assert.equal(await page.getByTestId('kontor-switch-more-provider_blocked').getAttribute('aria-label'),'Provider blocked (observed), …');await page.getByTestId('kontor-switch-more').click();
        assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
        await page.getByRole('button',{name:'New lead',exact:true}).waitFor();await page.getByRole('button',{name:'Open in Kontor',exact:true}).waitFor();
        const before=await page.evaluate(()=>({calls:window.fixture.inventoryCalls.length,draft:window.fixture.draft,pending:window.fixture.pendingQuestion}));
        await page.screenshot({path:path.join(output,`statuses-${width}-${theme}.png`)});
        await page.evaluate(()=>window.fixture.release());await page.getByTestId('host-popover').waitFor({state:'detached'});await openPopover(page);assert.equal(await page.evaluate(()=>window.fixture.inventoryCalls.length),before.calls,'opening strip must add zero inventory requests');
        await page.evaluate(()=>window.fixture.setHost('offline'));await page.waitForFunction(()=>document.querySelector('[data-testid=kontor-switch-state-running]')?.textContent==='…');
        assert.match(await page.getByTestId('kontor-switch-freshness').innerText(),/offline/i);
        await page.evaluate(()=>window.fixture.setHost('online'));await page.waitForFunction(()=>document.querySelector('[data-testid=kontor-switch-state-running]')?.textContent==='2');
        const afterReconnect=await page.evaluate(()=>window.fixture.inventoryCalls.length);await page.evaluate(()=>window.fixture.release());await page.getByTestId('host-popover').waitFor({state:'detached'});await openPopover(page);assert.equal(await page.evaluate(()=>window.fixture.inventoryCalls.length),afterReconnect);
        // A state narrows the switcher in place; Open in Kontor hands that view to Sessions whole.
        const press=page.getByTestId('kontor-switch-state-needs_input');await press.focus();assert.equal(await press.evaluate(node=>document.activeElement===node),true);await press.press('Enter');
        await expect(press).toHaveAttribute('aria-pressed','true');assert.deepEqual((await listedIds(page)).sort(),['batch-02','dup-2']);
        await page.getByRole('button',{name:'Open in Kontor',exact:true}).click();
        await page.waitForFunction(()=>window.fixture.views.length===1);await expect(page.getByTestId('fixture-status-filter')).toHaveText('needs_input');
        assert.deepEqual((await page.getByTestId('fixture-status-ids').innerText()).split(',').sort(),['batch-02','dup-2']);
        const result=await page.evaluate(()=>({screens:window.fixture.screens.length,closes:window.fixture.closes,draft:window.fixture.draft,pending:window.fixture.pendingQuestion,opens:window.fixture.opens,views:window.fixture.views,rpcs:window.fixture.calls.map(call=>call.name)}));
        assert.equal(result.screens,1);assert.equal(result.closes,1);assert.equal(result.draft,before.draft);assert.equal(result.pending,before.pending);assert.deepEqual(result.opens,[]);
        assert.deepEqual(result.views,[{status:'needs_input',query:'',membership:'sessions'}]);
        assert.ok(result.rpcs.every(name=>name==='sessions.create-options'||name==='sessions.navigator-capabilities'),'only read-only catalog and capability reads');assert.deepEqual(errors,[]);
        return {boxes,expected,reopenDismissal:'Synthetic host close callback; no Escape-from-textbox claim',inventoryBefore:before.calls,inventoryAfterReconnect:afterReconnect,result,errors};
      }finally{await context.close();}
    });
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
        // The host owns dismissal: the body adds no Close. Its title is checked last.
        assert.equal(await page.getByRole('heading', { name: 'Start or switch', exact: true }).count(), 0);
        assert.equal(await page.getByRole('button', { name: /^Close/ }).count(), 0);
        // The search target is its 44pt field around the input; the strip's Open in Kontor shares the strip's 40pt floor.
        const controls = [page.getByRole('button', { name: 'Open in Kontor', exact: true }), page.getByRole('button', { name: 'New lead', exact: true }), page.getByRole('textbox', { name: 'Find a session', exact: true }).locator('xpath=..'),
          page.getByTestId('session-row-dup-1'), page.getByTestId('session-row-dup-2'), page.getByRole('button', { name: 'Clear recent on this device', exact: true })];
        await expect(controls[0]).toBeInViewport();
        await expect(controls[1]).toBeVisible();
        const sizes = [];
        for (const control of controls) {
          await control.scrollIntoViewIfNeeded();
          const box = await control.boundingBox();
          assert.ok(within(box, frame), 'control inside the popover');
          sizes.push(Math.round(box.height));
          if (compact) assert.ok(box.height >= (control === controls[0] ? 40 : 44), 'compact touch target');
        }
        // Equal titles stay two rows, each its own exact ID.
        assert.deepEqual(await listedIds(page), ['dup-2', 'dup-1']);
        for (const id of ['dup-2', 'dup-1']) assert.match(await page.getByTestId(`session-row-${id}`).getByTestId('session-row-body').getAttribute('aria-label'), /^Route review\. /);
        await page.getByTestId('host-scroller').evaluate(node => node.scrollTo(0, 0));
        await page.screenshot({ path: path.join(output, `${width}-${theme}-popover.png`) });
        // The body names itself once; in the compact host sheet, which carries the same title, it must not repeat it.
        assert.equal(await page.getByText('Start or switch session', { exact: true }).count(), 1, 'one Start or switch session title');
        return { frame, scroller, controlHeights: sizes };
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-row-hands-off-exact-id-once`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.evaluate(() => { window.fixture.holdArrival = true; });
        await page.getByTestId('session-row-dup-1').click();
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
    for (const width of [320, 390, 1440]) await run(`${width}-open-kontor-hands-off-view`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('button', { name: 'Open in Kontor', exact: true }).click();
        await page.getByText('Kontor with a request', { exact: true }).waitFor();
        const now = await state(page);
        assert.equal(now.screens.length, 1); assertKeyOnly(now.screens[0], ['dup', 'Route']);
        assert.deepEqual(now.views, [{ status: 'recent', query: '', membership: 'sessions' }]);
        assert.equal(await page.getByTestId('host-popover').count(), 0);
        assert.deepEqual(now.opens, []); assert.deepEqual(now.toasts, []);
        // StrictMode ran the effect twice: the second read found the key used.
        assert.deepEqual(now.results.map(result => result?.ok ? 'ok' : result?.reason), ['ok', 'stale']);
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-search-show-more-hands-off`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await showAll(page);
        await page.getByRole('textbox', { name: 'Find a session', exact: true }).fill('batch');
        await expect(page.getByTestId('kontor-switch-count')).toHaveText('20 of 25');
        assert.equal((await listedIds(page)).length, 20);
        assert.equal(await page.getByTestId('session-row-batch-archived').count(), 0);
        await page.screenshot({ path: path.join(output, `${width}-search.png`) });
        // The rest are one Show more away, never silently dropped.
        await page.getByRole('button', { name: 'Show the other 5 of 25', exact: true }).click();
        await expect(page.getByTestId('kontor-switch-count')).toHaveText('25 of 25');
        assert.equal((await listedIds(page)).length, 25);
        await page.getByRole('button', { name: 'Open in Kontor', exact: true }).click();
        await page.getByText('Kontor with a request', { exact: true }).waitFor();
        const now = await state(page);
        assert.equal(now.screens.length, 1); assertKeyOnly(now.screens[0], ['batch']);
        assert.deepEqual(now.views, [{ status: 'all', query: 'batch', membership: 'sessions' }]); assert.deepEqual(now.opens, []); assert.deepEqual(now.toasts, []);
        assert.deepEqual(now.recent, ['dup-2', 'dup-1']);
      } finally { await context.close(); }
    });
    for (const width of [320, 390, 1440]) await run(`${width}-new-lsa-inside-popover`, async () => {
      const { page, context } = await pageFor(width);
      try {
        await page.getByRole('button', { name: 'New lead', exact: true }).click();
        await page.getByRole('heading', { name: 'New lead', exact: true }).waitFor();
        await chooseModel(page);
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
        await page.getByRole('button', { name: 'New lead', exact: true }).click();
        const back = page.getByRole('button', { name: 'Back to sessions', exact: true });
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
          observation.longHold.stayedInNewLsa = await page.getByRole('heading', { name: 'New lead', exact: true }).count() === 1;
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
        await page.getByRole('button', { name: 'New lead', exact: true }).click();
        // One desk is a fact, not a choice; the workspace the popover opened in chose it.
        await expect(page.getByTestId('kontor-new-lead-desk')).toContainText('DESK • ADAM');
      } finally { await context.close(); }
    });
    await run('390-stale-screen-leaves-newer-request', async () => {
      const { page, context } = await pageFor(390);
      try {
        await page.evaluate(() => { window.fixture.holdArrival = true; });
        await page.getByTestId('session-row-dup-1').click();
        await openPopover(page);
        await page.getByTestId('session-row-dup-2').click();
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
        await page.getByTestId('session-row-dup-1').click();
        let now = await state(page);
        assert.deepEqual(now.toasts, ['Fixture could not open Kontor.']);
        assert.equal(now.closes, 0); assert.equal(await page.getByTestId('host-popover').count(), 1);
        assert.deepEqual(now.screens, []); assert.deepEqual(now.opens, []);
        await page.evaluate(() => { window.fixture.failOpenScreen = false; });
        await page.getByTestId('session-row-dup-1').click();
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
