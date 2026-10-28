// Synthetic host + native-list updates only. No live daemon, native device or lifecycle effects.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const plugin = path.resolve(__dirname, '..');
const deps = createRequire(path.resolve(plugin, '../console/package.json'));
assert.ok(process.env.RENDERER_PACKAGE_JSON && process.env.EVIDENCE_DIR, 'Supply pinned renderer and external evidence directory');
const renderer = createRequire(path.resolve(process.env.RENDERER_PACKAGE_JSON));
const output = path.resolve(process.env.EVIDENCE_DIR);
assert.ok(!output.startsWith(plugin + path.sep), 'Keep evidence outside the plugin');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || deps.resolve('@playwright/test'));
for (const [name, version] of [['react', '19.2.3'], ['react-dom', '19.2.3'], ['react-native-web', '0.21.2']]) assert.equal(renderer(name + '/package.json').version, version);
const rootOf = name => path.dirname(renderer.resolve(name + '/package.json'));
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-live-pill-'));
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'sdk.ts'), 'export const getPaseoClient = id => window.fixture.client(id);\n');
fs.writeFileSync(path.join(fixture, 'icons.tsx'), `import React from 'react';
export const Icon = ({name,color,size}) => <span aria-hidden="true" data-icon={name} style={{color,width:size,height:size,display:'inline-flex',alignItems:'center',justifyContent:'center'}}>{({Building2:'▦',Play:'▶',Pause:'Ⅱ',CircleAlert:'!',MessageCircleQuestion:'?',Bell:'♧',Loader:'◌',Square:'□'})[name]}</span>;`);
fs.writeFileSync(path.join(fixture, 'popover.tsx'), `import React from 'react';import {Pressable,Text} from 'react-native';
export function StartSwitchPopover({openScreen,screenId,close}) { return <Pressable accessibilityRole="button" accessibilityLabel="Open Kontor" onPress={()=>{openScreen({screenId});close();}}><Text>Open Kontor</Text></Pressable>; }`);
fs.writeFileSync(path.join(fixture, 'main.tsx'), `
import React,{useEffect,useState} from 'react';import {createRoot} from 'react-dom/client';import {Pressable,Text,TextInput,View} from 'react-native';
import {contributeReturnControl} from ${JSON.stringify(path.join(plugin, 'client/return-control.ts'))};
import {startHostDirectory,disposeRetainedDirectories} from ${JSON.stringify(path.join(plugin, 'client/use-session-directory.ts'))};
document.body.style.margin='0';document.body.style.backgroundColor='#181B1A';
let colors={surface0:'#181B1A',surface1:'#1E2120',surface2:'#272A29',border:'#252B2A',foreground:'#fafafa',foregroundMuted:'#A1A5A4',accent:'#20744A',accentForeground:'#fff',statusSuccess:'#6cb17b',statusWarning:'#c09664',statusDanger:'#d8847b'};
const hostProps={theme:{colors},host:{id:'host-a',label:'Synthetic host'},layout:{compact:innerWidth<700,platform:'web'},size:16,color:colors.foreground};
const native=(id,status='idle')=>({id,title:'Synthetic '+id,status,workspaceId:'workspace-'+id,provider:'fixture',cwd:'/fixture',model:null,labels:{},createdAt:'2026-10-25T10:00:00Z',updatedAt:'2026-10-25T10:00:00Z',archivedAt:null,pendingPermissions:[],requiresAttention:false,attentionReason:null,attentionTimestamp:null});
let rows=[native('a'),native('b','running')], observer, notify=()=>{};const pills=new Map();
window.fixture={reads:0,subscriptions:0,adds:0,updates:0,opens:[],closes:0,pushes:0,paints:[],longTasks:[],client:id=>{if(id!=='host-a')throw Error('Wrong host');return {agents:{list:async(options={})=>{
 window.fixture.reads++;const result={requestId:'fixture',entries:rows.map(agent=>({agent,project:null})),pageInfo:{hasMore:false,nextCursor:null,prevCursor:null}};
 if(!options.subscribe)return result;window.fixture.subscriptions++;return {...result,subscriptionId:'sub',subscription:{subscriptionId:'sub',ready:Promise.resolve(result),subscribe:o=>{observer=o;o.snapshot();return()=>{};},release:async()=>{}}};
 }}};},emit:(id,patch)=>{const index=rows.findIndex(row=>row.id===id);rows[index]={...native(id),...patch};window.fixture.pushes++;performance.mark('push');observer.update({type:'agent_update',payload:{kind:'upsert',agent:rows[index]}});},
 offline:()=>startHostDirectory('host-a','offline'),online:()=>startHostDirectory('host-a','online'),rename:()=>window.fixture.emit('a',{title:'New title'}),
 dispose:()=>{stop();disposeRetainedDirectories();}};
if(typeof PerformanceObserver!=='undefined'&&PerformanceObserver.supportedEntryTypes.includes('longtask'))new PerformanceObserver(list=>window.fixture.longTasks.push(...list.getEntries().map(({duration,startTime})=>({duration,startTime})))).observe({entryTypes:['longtask']});
const client={addComposerPill:contribution=>{window.fixture.adds++;const item={...contribution};pills.set(item.agentId,item);notify();return {update:patch=>{Object.assign(item.button,patch);window.fixture.updates++;notify();},remove:()=>{pills.delete(item.agentId);notify();}};},openScreen:input=>window.fixture.opens.push(input)};
const stop=contributeReturnControl(client,'sessions');
function App(){const [revision,setRevision]=useState(0),[hidden,setHidden]=useState(false),[open,setOpen]=useState(null),[draft,setDraft]=useState('Draft stays');notify=()=>setRevision(n=>n+1);window.fixture.hide=setHidden;window.fixture.light=()=>{colors={...colors,surface0:'#ffffff',surface1:'#f5f6f8',surface2:'#e6ebf2',foreground:'#20242c',foregroundMuted:'#5e6570',statusSuccess:'#3e704a'};hostProps.theme={colors};notify();};useEffect(()=>{startHostDirectory('host-a');return()=>{};},[]);
 useEffect(()=>{requestAnimationFrame(()=>requestAnimationFrame(()=>{const mark=performance.getEntriesByName('push').at(-1);if(mark)window.fixture.paints.push({revision,eventToPaintOpportunityMs:performance.now()-mark.startTime});}));},[revision]);
 return <View style={{padding:12,gap:12,backgroundColor:colors.surface0,minHeight:300}}><Text style={{color:colors.foreground}}>Composer</Text><TextInput accessibilityLabel="Chat draft" value={draft} onChangeText={setDraft} style={{color:colors.foreground,borderWidth:1,borderColor:colors.border,minHeight:44}} />
 <View style={{gap:12,display:hidden?'none':'flex'}}>{[...pills.values()].map(item=>{const Glyph=item.button.icon;return <View key={item.agentId} style={{alignItems:'flex-start',gap:4}}><Text style={{color:colors.foregroundMuted}}>Session {item.agentId}</Text><Pressable key={item.agentId} testID={'pill-'+item.agentId} accessibilityRole="button" accessibilityLabel={item.button.title} onPress={()=>setOpen(item)} style={{flexDirection:'row',alignItems:'center',gap:6,minHeight:44,paddingHorizontal:12,borderRadius:16,backgroundColor:colors.surface2}}><Glyph {...hostProps} context="agent" agentId={item.agentId} workspaceId={item.workspaceId}/><Text style={{color:colors.foreground}}>{item.button.label}</Text></Pressable></View>;})}</View>
 {open&&<View testID="popover"><open.button.behavior.Content {...hostProps} context="agent" agentId={open.agentId} workspaceId={open.workspaceId} close={()=>{window.fixture.closes++;setOpen(null);}}/></View>}</View>;
}createRoot(document.getElementById('root')).render(<App/>);
`);
async function waitForATitle(page, label) {
  await page.waitForFunction(label => document.querySelector('[data-testid="pill-a"]')?.getAttribute('aria-label') === label, label);
}
let server, browser;
const report = { scope: 'Synthetic Chromium / React Native Web host wrapper. Actual contribution, canonical directory, presentation helper; icon/public SDK/native list and unchanged popover are fixture adapters. No native/physical/live proof.', cases: [] };
(async () => {
  try {
    const vite = await import(deps.resolve('vite'));
    server = await vite.createServer({ root: fixture, configFile: false,
      define: { global: 'globalThis', __DEV__: 'false', 'process.env.NODE_ENV': '"development"' }, server: { host: '127.0.0.1', port: 0, fs: { allow: [fixture,plugin,path.resolve(plugin,'../..'),path.resolve(path.dirname(process.env.RENDERER_PACKAGE_JSON),'../..')] } },
      resolve: { alias: [
        { find: /^react$/, replacement: rootOf('react') }, { find: /^react\/(.*)$/, replacement: rootOf('react')+'/$1' },
        { find: /^react-dom$/, replacement: rootOf('react-dom') }, { find: /^react-dom\/(.*)$/, replacement: rootOf('react-dom')+'/$1' },
        { find: /^react-native$/, replacement: path.join(rootOf('react-native-web'),'dist/index.js') },
        { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(fixture,'sdk.ts') },
        { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(fixture,'icons.tsx') },
        { find: './start-switch-sheet', replacement: path.join(fixture,'popover.tsx') },
      ] },
    });
    await server.listen();browser=await chromium.launch({headless:true});report.browser=browser.version();
    for(const width of [320,390,1440]){
      const context=await browser.newContext({viewport:{width,height:844},reducedMotion:'reduce'});const page=await context.newPage();const errors=[];page.on('pageerror',error=>errors.push(String(error)));
      try{
        await page.goto(server.resolvedUrls.local[0]);const a=page.getByTestId('pill-a'),b=page.getByTestId('pill-b');
        await page.getByRole('button',{name:'Synthetic a · Idle',exact:true}).waitFor();
        const baseline=await page.evaluate(()=>({reads:window.fixture.reads,adds:window.fixture.adds,subscriptions:window.fixture.subscriptions}));
        await page.getByRole('textbox',{name:'Chat draft'}).fill('Preserve this draft');
        // Running uses Kontor's existing blue appearance ink even when the host's CTA accent is green.
        const transitions=[['running','Running','rgb(156, 191, 255)',{}],['running','Needs input','rgb(192, 150, 100)',{pendingPermissions:[{id:'permission-a',provider:'codex',name:'Read',kind:'tool'}]}],['error','Error','rgb(216, 132, 123)',{}],['idle','Attention','rgb(108, 177, 123)',{requiresAttention:true,attentionReason:'finished'}],['idle','Idle','rgb(161, 165, 164)',{}],['initializing','Starting','rgb(161, 165, 164)',{}],['closed','Closed','rgb(161, 165, 164)',{}]];
        const measurements=[];
        for(const [status,label,ink,extra]of transitions){await page.evaluate(({status,extra})=>window.fixture.emit('a',{status,...extra}),{status,extra});await waitForATitle(page,'Synthetic a · '+label);assert.deepEqual(await a.locator('[data-icon]').evaluateAll(nodes=>nodes.map(node=>[node.dataset.icon,getComputedStyle(node).color])),[['Building2',ink]]);assert.equal(await b.getAttribute('aria-label'),'Synthetic b · Running');
          await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));measurements.push({label,ms:await page.evaluate(()=>window.fixture.paints.at(-1)?.eventToPaintOpportunityMs)});
        }
        await page.evaluate(()=>window.fixture.emit('a',{status:'running'}));await waitForATitle(page,'Synthetic a · Running');
        const beforeOffline=await a.locator('[data-icon]').evaluate(node=>getComputedStyle(node).color);
        await page.evaluate(()=>window.fixture.offline());await waitForATitle(page,'Synthetic a · Last known: Running');
        assert.equal(await a.locator('[data-icon]').evaluate(node=>getComputedStyle(node).color),'rgb(161, 165, 164)');assert.notEqual(beforeOffline,'rgb(161, 165, 164)');
        const offline=await page.evaluate(()=>({reads:window.fixture.reads,adds:window.fixture.adds,subscriptions:window.fixture.subscriptions}));assert.deepEqual(offline,baseline);
        await page.evaluate(()=>window.fixture.online());await waitForATitle(page,'Synthetic a · Running');
        const reconnected=await page.evaluate(()=>({reads:window.fixture.reads,adds:window.fixture.adds,subscriptions:window.fixture.subscriptions}));assert.equal(reconnected.adds,2);assert.equal(reconnected.subscriptions,2);
        await a.click();await page.getByRole('button',{name:'Open Kontor',exact:true}).click();assert.deepEqual(await page.evaluate(()=>window.fixture.opens),[{screenId:'sessions'}]);assert.equal(await page.evaluate(()=>window.fixture.closes),1);
        assert.equal(await page.getByRole('textbox',{name:'Chat draft'}).inputValue(),'Preserve this draft');assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
        for(const pill of [a,b])assert.ok((await pill.boundingBox()).height>=44);
        if(width===390){
          await page.emulateMedia({reducedMotion:'no-preference'});
          const pulse=a.getByTestId('composer-running-pulse'),tone=a.getByTestId('composer-running-tone');
          await tone.waitFor();
          const opacity=await tone.evaluate(node=>getComputedStyle(node).opacity);
          await page.waitForFunction(()=>Number(document.querySelector('[data-testid="pill-a"] [data-testid="composer-running-tone"]')?.style.opacity)>0.95);
          const peak=await pulse.evaluate(node=>({transform:getComputedStyle(node).transform,opacity:getComputedStyle(node.querySelector('[data-testid="composer-running-tone"]')).opacity}));
          assert.ok(Number(peak.opacity)>0.95);
          await page.waitForFunction(()=>Number(document.querySelector('[data-testid="pill-a"] [data-testid="composer-running-tone"]')?.style.opacity)<0.05);
          const rest=await pulse.evaluate(node=>getComputedStyle(node).transform);
          await page.screenshot({path:path.join(output,'390-dark-motion.png')});
          await page.evaluate(()=>window.fixture.light());
          assert.deepEqual(await a.locator('[data-icon]').evaluateAll(nodes=>nodes.map(node=>getComputedStyle(node).color)),['rgb(36, 91, 186)','rgb(62, 112, 74)']);
          await page.screenshot({path:path.join(output,'390-light-motion.png')});
          await page.emulateMedia({reducedMotion:'reduce'});await tone.waitFor({state:'detached'});
          assert.equal(await pulse.evaluate(node=>getComputedStyle(node).transform),'matrix(1, 0, 0, 1, 0, 0)');
          await page.emulateMedia({reducedMotion:'no-preference'});await tone.waitFor();
          await page.evaluate(()=>window.fixture.emit('a',{status:'idle'}));await waitForATitle(page,'Synthetic a · Idle');await pulse.waitFor({state:'detached'});
          await page.evaluate(()=>window.fixture.emit('a',{status:'running'}));await waitForATitle(page,'Synthetic a · Running');await tone.waitFor();
          await page.evaluate(()=>window.fixture.hide(true));
          await tone.waitFor({state:'detached',timeout:5000});
          await page.evaluate(()=>window.fixture.hide(false));await tone.waitFor();
          report.motion={opacityBefore:opacity,peak,rest,liveReducedMotion:'PASS',themeChange:'PASS',stateChange:'PASS',retainedDisplayNone:'PASS',nativeDriver:'UNVERIFIED: Chromium uses RN Web'};
        }
        await page.screenshot({path:path.join(output,width+'-composer-live-pill.png')});assert.deepEqual(errors,[]);
        report.cases.push({width,status:'PASS',baseline,reconnected,measurements,longTasks:await page.evaluate(()=>window.fixture.longTasks),reducedMotion:width===390?'live motion comparison':'reduce; static',effectRPCs:0});
      }catch(error){report.cases.push({width,status:'FAIL',error:String(error.stack),errors});}
      finally{await context.close();}
    }
  }finally{if(browser)await browser.close();if(server)await server.close();fs.rmSync(fixture,{recursive:true,force:true});report.pass=report.cases.length===3&&report.cases.every(x=>x.status==='PASS');fs.writeFileSync(path.join(output,'result.json'),JSON.stringify(report,null,2)+'\n');}
  assert.ok(report.pass,JSON.stringify(report));console.log(JSON.stringify(report));
})().catch(error=>{console.error(error);process.exitCode=1;});
