// G-RENDER-1: the real-render motion gate for the R37-r2 running mark (ASMA-8539 D3-r2/D6/D7).
// Real react-native-web 0.21.4 + the host's react-native-svg MAJOR (15.15.4, read from the installed
// Paseo.app web bundle's embedded dependency manifest) rendering the real plugin client modules in
// headless Chromium (playwright drives CDP). Two passes: prefers-reduced-motion emulated ON (colours
// flow, carrier scale exactly 1) and OFF (colours flow + pulse), plus the last-known row staying muted
// grey and the hidden/off-screen mark retaining no clock. Frames, timestamps, numeric diffs, the
// generator command and result.json all land in the evidence dir. No daemon, host account, native
// session or device effects. Evidence stays outside the repository.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const { execFileSync } = require('node:child_process');
const plugin = path.resolve(__dirname, '..');
const consoleDeps = createRequire(path.join(plugin, '../console/package.json'));
assert.ok(process.env.EVIDENCE_DIR, 'Supply an external EVIDENCE_DIR');
const output = path.join(path.resolve(process.env.EVIDENCE_DIR), 'g-render-1');
fs.mkdirSync(output, { recursive: true });
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
const store = path.join(plugin, '../../node_modules/.pnpm');
const SVG_VERSION = '15.15.4';
const SVG_HOST_BUNDLE = '/Applications/Paseo.app/Contents/Resources/app-dist/_expo/static/js/web/index-4df2750168de94b9b8fdf3170859afa9.js';
const renderer = path.join(output, 'renderer');
fs.rmSync(renderer, { recursive: true, force: true });
fs.mkdirSync(path.join(renderer, 'node_modules'), { recursive: true });
// No "type" here: react-native-svg ships mixed CJS/ESM .js files and a "type":"module" parent would
// mislabel them (vite import-analysis then loses the CJS named exports).
fs.writeFileSync(path.join(renderer, 'package.json'), '{"name":"r37-r2-gate-renderer","private":true}\n');
const pinned = {
  react: 'react@19.1.0/node_modules/react',
  'react-dom': 'react-dom@19.1.0_react@19.1.0/node_modules/react-dom',
  'react-native-web': 'react-native-web@0.21.4_react-dom@19.1.0_react@19.1.0__react@19.1.0/node_modules/react-native-web',
  'lucide-react': 'lucide-react@1.50.0_@types+react@19.2.18_react@19.1.0/node_modules/lucide-react',
};
// The host's react-native-svg MAJOR, installed from the npm registry into this renderer. npm prunes
// anything already in node_modules, so it runs BEFORE the pinned symlinks below.
const install = ['install', `react-native-svg@${SVG_VERSION}`, '--no-save', '--ignore-scripts', '--no-audit', '--no-fund'];
const installOutput = execFileSync('npm', install, { cwd: renderer, stdio: 'pipe', encoding: 'utf8' });
// npm auto-installs react-native-svg's peer deps (its own react/react-dom); the host pins win, and
// the npm react-native package goes entirely: its Flow sources break the optimizer, and every
// 'react-native' import this fixture needs resolves through the aliases below.
for (const name of [...Object.keys(pinned), 'react-native']) fs.rmSync(path.join(renderer, 'node_modules', name), { recursive: true, force: true });
for (const [name, dir] of Object.entries(pinned)) fs.symlinkSync(path.join(store, dir), path.join(renderer, 'node_modules', name), 'dir');
const rootOf = name => path.join(renderer, 'node_modules', name);
const versionOf = name => require(path.join(rootOf(name), 'package.json')).version;
for (const [name, version] of [['react', '19.1.0'], ['react-dom', '19.1.0'], ['react-native-web', '0.21.4'], ['lucide-react', '1.50.0'], ['react-native-svg', SVG_VERSION]]) {
  assert.equal(require(path.join(rootOf(name), 'package.json')).version, version, `pinned/installed ${name}`);
}
// The vite dep optimizer resolves react-native-svg's native variants without this server's .web.js
// extension preference; the two native entry variants become web re-exports so every crawler path
// (and the browser pipeline, which already prefers .web.js) stays on the web implementation.
const svgModule = path.join(rootOf('react-native-svg'), 'lib/module');
fs.writeFileSync(path.join(svgModule, 'ReactNativeSVG.js'), "export * from './ReactNativeSVG.web.js';\nexport default null;\n");
fs.writeFileSync(path.join(svgModule, 'elements.js'), "export * from './elements.web.js';\nexport { default } from './elements.web.js';\n");
// The host bundle's embedded dependency manifest re-read here makes the "where read" claim reproducible.
const hostBundleText = fs.readFileSync(SVG_HOST_BUNDLE, 'utf8');
assert.ok(hostBundleText.includes(`"react-native-svg":"${SVG_VERSION}"`), `host bundle manifest pins react-native-svg ${SVG_VERSION}`);
const generatorCommand = [process.execPath, ...process.argv.slice(1)].join(' ')
  + `  (EVIDENCE_DIR=${process.env.EVIDENCE_DIR})`;
fs.writeFileSync(path.join(output, 'commands.txt'), [
  '# G-RENDER-1 generator and renderer commands — ' + new Date().toISOString(),
  'node ' + process.argv.slice(1).map(a => a.replace(os.homedir(), '~')).join(' '),
  `  EVIDENCE_DIR=${process.env.EVIDENCE_DIR}`,
  'npm ' + install.join(' ') + '   # cwd: ' + renderer.replace(os.homedir(), '~'),
  `renderer: react ${versionOf('react')}, react-dom ${versionOf('react-dom')}, react-native-web ${versionOf('react-native-web')}, lucide-react ${versionOf('lucide-react')}, react-native-svg ${versionOf('react-native-svg')} (host major from ${SVG_HOST_BUNDLE})`,
  '',
].join('\n'));

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-motion-gate-'));
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), 'export const getPaseoClient = () => { throw new Error("not used by this evidence"); };\n');
// Deep react-native imports (react-native-svg's native fabric components) get an inert stub: on web
// they are never rendered, and the real ones are Flow sources the browser bundle cannot parse.
fs.writeFileSync(path.join(fixture, 'rn-deep-stub.ts'), 'const codegenNativeComponent = () => null;\nexport default codegenNativeComponent;\n');
// resolveAssetUri (web chain) asks for the native asset registry; SvgXml paths never hit it.
fs.writeFileSync(path.join(fixture, 'rn-asset-registry-stub.ts'), 'export const getAssetByID = () => undefined;\n');
// Precise stub: session-view drags the plugin SDK/zod into the browser bundle and nothing under test
// uses its runtime values; QUICK_FILTER_ICONS (the last-known grey path) stays the real module.
fs.writeFileSync(path.join(fixture, 'session-view-stub.ts'), [
  'export type StatusFilter = string;',
  'export const QUICK_STATUS_FILTERS = ["recent","activity","current","all","running","idle","error","needs_input","attention","favorites","initializing","closed","earlier_error","provider_blocked"];',
  'export const statusFilterLabel = (status: string) => status;',
  '',
].join('\n'));
// The host Icon: inline plugin SVG through the REAL react-native-svg SvgXml at width/height = size,
// and Lucide glyphs for names — the same split the Paseo host ships.
fs.writeFileSync(path.join(fixture, 'icons.tsx'), `import React from 'react';
import Play from ${JSON.stringify(path.join(rootOf('lucide-react'), 'dist/esm/icons/play.mjs'))};
import Building2 from ${JSON.stringify(path.join(rootOf('lucide-react'), 'dist/esm/icons/building-2.mjs'))};
import { SvgXml } from 'react-native-svg';
export const Icon = ({name, icon, size = 16, color}) => {
  if (icon && typeof icon.svg === 'string') return <SvgXml xml={icon.svg} width={size} height={size} />;
  if (name === 'Play') return <Play size={size} color={color} strokeWidth={2} aria-hidden />;
  if (name === 'Building2') return <Building2 size={size} color={color} strokeWidth={2} aria-hidden />;
  return <div style={{width:size, height:size}} />;
};
`);
const dark = { surface0:'#181B1A', surface1:'#1E2120', surface2:'#272A29', border:'#252B2A', foreground:'#fafafa', foregroundMuted:'#A1A5A4', accent:'#20744A', accentForeground:'#ffffff', statusSuccess:'#6cb17b', statusWarning:'#c09664', statusDanger:'#d8847b' };
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React from 'react';
import { createRoot } from 'react-dom/client';
import { View, Text } from 'react-native';
import { ComposerPulse } from ${JSON.stringify(path.join(plugin, 'client/composer-pulse.tsx'))};
import { RAINBOW_FRAMES, rainbowFrameInterpolation, rainbowMarkSvg } from ${JSON.stringify(path.join(plugin, 'client/running-mark.tsx'))};
import { SessionPulseContext, SessionStatusIcon, runningMarkNode } from ${JSON.stringify(path.join(plugin, 'client/session-state-marker.tsx'))};
const dark = ${JSON.stringify(dark)};
const light = { surface0:'#ffffff', surface1:'#fafafa', surface2:'#f4f4f5', border:'#e4e4e7', foreground:'#1a1a1e', foregroundMuted:'#71717a', accent:'#20744A', accentForeground:'#ffffff', statusSuccess:'#3e704a', statusWarning:'#7b5d39', statusDanger:'#9d433b' };
const label = (text) => <Text style={{color: dark.foreground, fontSize: 13, fontFamily: 'monospace'}}>{text}</Text>;
// Live session row: the real row glyph path (SessionPulseContext -> RunningSessionMark -> runningMarkNode).
const liveRow = (
  <SessionPulseContext.Provider value={{live: true, visible: true}}>
    <SessionStatusIcon status="running" session colors={dark} size={16} />
  </SessionPulseContext.Provider>
);
// Last-known row: the same real path with live:false — today's muted ink, never the rainbow.
const lastKnownRow = (
  <SessionPulseContext.Provider value={{live: false, visible: true}}>
    <SessionStatusIcon status="running" session colors={dark} size={16} />
  </SessionPulseContext.Provider>
);
function App() {
  return <div style={{display:'flex', flexDirection:'column', gap:22, padding:18, background: dark.surface0}}>
    <div id="live-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 10}}>
      {label('live running row')} {liveRow}
    </div>
    <div id="composer-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 10}}>
      {label('composer mark')} <ComposerPulse colors={dark} size={20} name="Play" />
    </div>
    <div id="evidence-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 24}}>
      {label('evidence mark')} <ComposerPulse colors={dark} size={96} name="Play" />
    </div>
    <div id="lastknown-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 10}}>
      {label('last-known row')} {lastKnownRow} {runningMarkNode({live: false, visible: true}, dark, 22)}
    </div>
    <div id="hidden-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 10}}>
      {label('to-hide mark')} <ComposerPulse colors={dark} size={20} name="Play" />
    </div>
  </div>;
}
createRoot(document.getElementById('root')).render(<App />);
// F2: the shipped compositing read, exposed for the wrap-segment analysis over the real frame markup.
const opacityAt = (config, value) => {
  if (value <= config.inputRange[0]) return config.outputRange[0];
  for (let index = 1; index < config.inputRange.length; index++) {
    if (value <= config.inputRange[index]) {
      const span = config.inputRange[index] - config.inputRange[index - 1];
      const share = span === 0 ? 1 : (value - config.inputRange[index - 1]) / span;
      return config.outputRange[index - 1] + share * (config.outputRange[index] - config.outputRange[index - 1]);
    }
  }
  return config.outputRange[config.outputRange.length - 1];
};
(window).gate = {
  frames: RAINBOW_FRAMES,
  alphaAt: (frame, t) => opacityAt(rainbowFrameInterpolation(frame, RAINBOW_FRAMES), t),
  markup: (theme, frame) => rainbowMarkSvg('play', theme === 'light' ? light : dark, frame),
  surface: (theme) => (theme === 'light' ? light.surface0 : dark.surface0),
};
document.title = 'gate-ready';
`);

let server, browser;
const report = {
  gate: 'G-RENDER-1',
  scope: 'Real-render web evidence on the pinned renderer (react 19.1.0 / react-dom 19.1.0 / react-native-web 0.21.4 / react-native-svg 15.15.4) with the real plugin client modules; iOS per-mark cost is a static mount count, on-device measurement UNVERIFIED. No host, account or device effects.',
  generator: { file: 'apps/paseo-plugin/test/composer-motion-gate.browser.cjs', command: generatorCommand, evidenceDir: path.resolve(process.env.EVIDENCE_DIR) },
  renderer: {
    react: versionOf('react'), reactDom: versionOf('react-dom'), reactNativeWeb: versionOf('react-native-web'),
    lucideReact: versionOf('lucide-react'), reactNativeSvg: versionOf('react-native-svg'),
    reactNativeSvgSource: 'npm registry install into ' + renderer,
    hostReactNativeSvg: { version: SVG_VERSION, readFrom: SVG_HOST_BUNDLE, verified: hostBundleText.includes(`"react-native-svg":"${SVG_VERSION}"`) },
    fixtureNote: 'session-view is stubbed (plugin-SDK/zod-free bundle); the last-known grey path uses the real QUICK_FILTER_ICONS + runningMarkNode.',
  },
  passes: {}, cases: [],
};
// In-page PNG diff: decode both screenshots, count pixels whose channels moved > 8, RMS over painted pixels.
const diffShots = ({ a, b }) => new Promise((resolve, reject) => {
  const load = (src) => new Promise((res, rej) => { const i = new Image(); i.onload = () => res(i); i.onerror = rej; i.src = 'data:image/png;base64,' + src; });
  Promise.all([load(a), load(b)]).then(([ia, ib]) => {
    const w = Math.max(ia.width, ib.width), h = Math.max(ia.height, ib.height);
    const c = document.createElement('canvas'); c.width = w; c.height = h;
    const ctx = c.getContext('2d', { willReadFrequently: true });
    ctx.drawImage(ia, 0, 0); const da = ctx.getImageData(0, 0, w, h).data;
    ctx.clearRect(0, 0, w, h); ctx.drawImage(ib, 0, 0); const db = ctx.getImageData(0, 0, w, h).data;
    let changed = 0, sum = 0, painted = 0, chromatic = 0;
    for (let i = 0; i < da.length; i += 4) {
      if (Math.max(da[i+3], db[i+3]) > 16) painted++;
      if (Math.abs(da[i]-db[i]) > 8 || Math.abs(da[i+1]-db[i+1]) > 8 || Math.abs(da[i+2]-db[i+2]) > 8) changed++;
      if (da[i+3] > 16 || db[i+3] > 16) sum += (da[i]-db[i])**2 + (da[i+1]-db[i+1])**2 + (da[i+2]-db[i+2])**2;
      if (db[i+3] > 16) { const mx = Math.max(db[i], db[i+1], db[i+2]), mn = Math.min(db[i], db[i+1], db[i+2]); if (mx - mn > 20) chromatic++; }
    }
    resolve({ width: w, height: h, paintedPixels: painted, changedPixels: changed, rms: Math.round(Math.sqrt(sum / Math.max(1, painted)) * 1000) / 1000, chromaticPixels: chromatic });
  }).catch(reject);
});
const carrierTransform = () => {
  const el = document.querySelector('#composer-wrap [data-testid="composer-running-pulse"]');
  return getComputedStyle(el).transform;
};
const frameOpacity = (selector) => {
  const el = document.querySelector(selector);
  return el ? Number(el.style.opacity) : null;
};
const toneCount = (scope) => document.querySelectorAll((scope || '') + ' [data-testid="composer-running-tone"]').length;

async function shootSequence(page, dir, count, gapMs) {
  fs.mkdirSync(dir, { recursive: true });
  const frames = [];
  for (let index = 0; index < count; index++) {
    const file = path.join(dir, `frame-${String(index).padStart(2, '0')}.png`);
    const startedAt = Date.now();
    const shot = await page.locator('#evidence-wrap').screenshot({ path: file });
    const finishedAt = Date.now();
    const previous = frames[frames.length - 1];
    const diff = previous
      ? await page.evaluate(diffShots, { a: previous.base64, b: shot.toString('base64') })
      : null;
    frames.push({ index, file: path.relative(output, file), startedAt, finishedAt, diff, base64: shot.toString('base64') });
    if (index + 1 < count) await page.waitForTimeout(gapMs);
  }
  for (const frame of frames) delete frame.base64;
  return frames;
}

async function sampleTransforms(page, count, gapMs) {
  const samples = [];
  for (let index = 0; index < count; index++) {
    samples.push({ at: Date.now(), transform: await page.evaluate(carrierTransform) });
    if (index + 1 < count) await page.waitForTimeout(gapMs);
  }
  return samples;
}
const matrixScale = (transform) => {
  const match = /^matrix\(([^)]+)\)$/.exec(transform);
  if (!match) return null;
  const parts = match[1].split(',').map(value => Number(value.trim()));
  return { a: parts[0], d: parts[3] };
};

(async () => {
  const vite = await import(consoleDeps.resolve('vite'));
  server = await vite.createServer({ root: fixture, configFile: false,
    define: { global: 'globalThis', __DEV__: 'false', 'process.env.NODE_ENV': '"development"' },
    server: { host: '127.0.0.1', port: 0, fs: { allow: [fixture, plugin, store, renderer] } },
    resolve: {
      extensions: ['.web.js', '.mjs', '.js', '.mts', '.ts', '.jsx', '.tsx', '.json'],
      alias: [
        { find: /^react$/, replacement: rootOf('react') }, { find: /^react\/(.*)$/, replacement: rootOf('react') + '/$1' },
        { find: /^react-dom$/, replacement: rootOf('react-dom') }, { find: /^react-dom\/(.*)$/, replacement: rootOf('react-dom') + '/$1' },
        { find: /^react-native$/, replacement: path.join(rootOf('react-native-web'), 'dist/index.js') },
        { find: /^react-native\/.*/, replacement: path.join(fixture, 'rn-deep-stub.ts') },
        { find: /^@react-native\/assets-registry\/registry$/, replacement: path.join(fixture, 'rn-asset-registry-stub.ts') },
        { find: /^react-native-svg$/, replacement: path.join(rootOf('react-native-svg')) },
        { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(fixture, 'sdk-client.ts') },
        { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(fixture, 'icons.tsx') },
        { find: /.*session-view$/, replacement: path.join(fixture, 'session-view-stub.ts') },
      ],
    },
  });
  await server.listen();
  browser = await chromium.launch({ headless: true });
  const errors = [];
  const runPass = async (passName, contextOptions, tail) => {
    const context = await browser.newContext({ viewport: { width: 900, height: 780 }, ...contextOptions });
    const page = await context.newPage();
    page.on('pageerror', error => errors.push(`${passName}: ${error}`));
    const pass = { contextOptions, frames: null, transforms: null, lastKnown: null, perMark: null, hidden: null, liveToggle: null };
    try {
      await page.goto(server.resolvedUrls.local[0]);
      await page.waitForFunction(() => document.title === 'gate-ready', undefined, { timeout: 30000 });
      const chosenK = await page.evaluate(() => (window).gate.frames);
      assert.equal(chosenK, 8, 'the shipped RAINBOW_FRAMES equals the chosen K = 8');
      // The live row mounts the real tone: 8 Animated.Views and 8 real react-native-svg nodes per mark.
      await page.waitForSelector('#live-wrap [data-testid="composer-running-tone"]', { timeout: 15000 });
      const perMark = await page.evaluate(() => ({
        animatedFrameViews: document.querySelectorAll('#live-wrap [data-testid^="composer-running-frame-"]').length,
        svgNodes: document.querySelectorAll('#live-wrap [data-testid="composer-running-tone"] svg').length,
        gradientStrokes: document.querySelectorAll('#live-wrap [data-testid="composer-running-tone"] svg[stroke^="url(#kontor-rainbow-"]').length,
      }));
      assert.equal(perMark.animatedFrameViews, 8, 'eight Animated.Views per live mark (K = 8)');
      assert.equal(perMark.svgNodes, 8, 'eight mounted SVG nodes per live mark');
      assert.equal(perMark.gradientStrokes, 8, 'every frame paints the real gradient stroke');
      pass.perMark = perMark;
      // >= 4 screenshots per loop with differing colour positions, timestamps and numeric diffs.
      pass.frames = await shootSequence(page, path.join(output, 'frames', passName), 6, 500);
      pass.frameSpanMs = pass.frames[pass.frames.length - 1].startedAt - pass.frames[0].startedAt;
      assert.ok(pass.frames.length >= 4 && pass.frameSpanMs > 1500, `at least four frames across the loop with timestamps (${passName})`);
      for (const frame of pass.frames.slice(1)) assert.ok(frame.diff.changedPixels > 0 && frame.diff.rms > 0, `colour positions differ between frames ${frame.index - 1} and ${frame.index} (${passName})`);
      const first = pass.frames[0], mid = pass.frames[3];
      const edgeDiff = await page.evaluate(diffShots, { a: fs.readFileSync(path.join(output, first.file)).toString('base64'), b: fs.readFileSync(path.join(output, mid.file)).toString('base64') });
      assert.ok(edgeDiff.changedPixels > 0, `the flow moves across the sampled loop (${passName})`);
      pass.edgeDiff = edgeDiff;
      // Carrier transform: exactly scale 1 under Reduce Motion; the pulse rides the same clock otherwise.
      pass.transforms = await sampleTransforms(page, 18, 160);
      const scales = pass.transforms.map(sample => matrixScale(sample.transform));
      if (contextOptions.reducedMotion === 'reduce') {
        for (const sample of pass.transforms) assert.equal(sample.transform, 'matrix(1, 0, 0, 1, 0, 0)', `carrier scale is exactly 1 under Reduce Motion (${passName}, at ${sample.at})`);
      } else {
        const values = scales.map(scale => scale.a);
        assert.ok(Math.max(...values) > 1.005, `the pulse lifts the carrier above 1 (${passName}): max ${Math.max(...values)}`);
        assert.ok(Math.max(...values) <= 1.121, `the pulse never exceeds the 1.12 cap (${passName}): max ${Math.max(...values)}`);
        assert.ok(Math.min(...values) >= 0.999, `the pulse never shrinks below 1 (${passName}): min ${Math.min(...values)}`);
      }
      // Last-known row: muted grey — no tone, no rainbow gradient, zero chromatic pixels in its paint.
      const lastKnownTones = await page.evaluate(toneCount, '#lastknown-wrap');
      assert.equal(lastKnownTones, 0, 'the last-known row mounts no tone');
      const lastKnownRainbow = await page.evaluate(() => document.querySelector('#lastknown-wrap').innerHTML.includes('kontor-rainbow-'));
      assert.equal(lastKnownRainbow, false, 'the last-known row paints no rainbow gradient');
      const marker = await page.locator('#lastknown-wrap [data-testid="session-state-marker"]').screenshot({ path: path.join(output, 'frames', passName, 'last-known-marker.png') });
      const chroma = await page.evaluate(diffShots, { a: marker.toString('base64'), b: marker.toString('base64') });
      assert.equal(chroma.chromaticPixels, 0, `the last-known row stays muted grey (${passName}): ${chroma.chromaticPixels} chromatic pixels`);
      pass.lastKnown = { toneCount: lastKnownTones, rainbow: lastKnownRainbow, chromaticPixels: chroma.chromaticPixels, file: path.join('frames', passName, 'last-known-marker.png') };
      await page.screenshot({ path: path.join(output, `pass-${passName}-page.png`), fullPage: true });
      if (passName === 'reduce-on') {
        // F2: the wrap segment t ∈ [7/8, 1] composited from the real shipped paint, against a
        // mid-segment reference: mean chroma of the painted stroke and the dominant phase per step.
        pass.wrap = await page.evaluate(async ({ themes }) => {
          const gate = (window).gate;
          const K = gate.frames;
          const angleOf = (frame) => 20 + (360 / K) * frame;
          const size = 64;
          const loadImage = (src) => new Promise((done, fail) => { const image = new Image(); image.onload = () => done(image); image.onerror = fail; image.src = src; });
          const analyse = async (theme, tValues) => {
            const out = [];
            for (const t of tValues) {
              const canvas = document.createElement('canvas');
              canvas.width = size; canvas.height = size;
              const ctx = canvas.getContext('2d', { willReadFrequently: true });
              ctx.fillStyle = gate.surface(theme); ctx.fillRect(0, 0, size, size);
              const layers = [];
              for (let frame = 0; frame < K; frame++) {
                const alpha = gate.alphaAt(frame, t);
                if (alpha <= 0) continue;
                layers.push({ alpha, image: await loadImage('data:image/svg+xml;utf8,' + encodeURIComponent(gate.markup(theme, frame))) });
              }
              layers.forEach(({ alpha, image }) => { ctx.globalAlpha = alpha; ctx.drawImage(image, 0, 0, size, size); });
              ctx.globalAlpha = 1;
              const data = ctx.getImageData(0, 0, size, size).data;
              let chromaSum = 0, painted = 0, x = 0, y = 0, weight = 0;
              for (let index = 0; index < data.length; index += 4) {
                if (data[index + 3] <= 16) continue;
                const r = data[index], g = data[index + 1], b = data[index + 2];
                chromaSum += Math.max(r, g, b) - Math.min(r, g, b); painted++;
              }
              for (let frame = 0; frame < K; frame++) {
                const alpha = gate.alphaAt(frame, t);
                const radians = (angleOf(frame) * Math.PI) / 180;
                x += alpha * Math.cos(radians); y += alpha * Math.sin(radians); weight += alpha;
              }
              const phase = Math.atan2(y / weight, x / weight) * (180 / Math.PI);
              out.push({ t, chroma: Math.round((chromaSum / Math.max(1, painted)) * 100) / 100, phase: Math.round(((phase + 360) % 360) * 100) / 100 });
            }
            return out;
          };
          const mid = [0.3125, 0.375, 0.4375];
          const wrap = Array.from({ length: 9 }, (_, index) => 0.875 + index / 64);
          const result = {};
          for (const theme of themes) result[theme] = { mid: await analyse(theme, mid), wrap: await analyse(theme, wrap) };
          return result;
        }, { themes: ['dark', 'light'] });
        // The wrap segment must not wash out: its mean chroma stays within the mid-segment's band,
        // and the dominant phase advances forward across the wrap samples.
        for (const [theme, entry] of Object.entries(pass.wrap)) {
          const midChroma = entry.mid.reduce((sum, sample) => sum + sample.chroma, 0) / entry.mid.length;
          const wrapMin = Math.min(...entry.wrap.map(sample => sample.chroma));
          assert.ok(wrapMin >= midChroma * 0.75, `wrap chroma keeps mid-segment level on ${theme}: wrap min ${wrapMin} vs mid ${Math.round(midChroma * 100) / 100}`);
          const phases = entry.wrap.map(sample => sample.phase);
          for (let index = 1; index < phases.length; index++) {
            const step = (phases[index] - phases[index - 1] + 360) % 360;
            assert.ok(step <= 45.01, `wrap phase advances forward on ${theme} at sample ${index}: step ${Math.round(step * 100) / 100}`);
          }
        }
        fs.writeFileSync(path.join(output, 'wrap-analysis.json'), JSON.stringify(pass.wrap, null, 2) + '\n');
      }
      if (tail) await tail(page, pass);
      report.passes[passName] = pass;
      report.cases.push({ name: passName, status: 'PASS' });
    } catch (error) {
      report.passes[passName] = pass;
      report.cases.push({ name: passName, status: 'FAIL', error: String(error && error.stack || error) });
      try { fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify({ ...report, pass: false }, null, 2) + '\n'); } catch {}
      throw error;
    } finally {
      await context.close();
    }
  };
  // Hidden / off-screen marks retain no clock: the tone detaches, the shared clock resets, and a
  // re-entering mark remounts its flow fresh from frame 0 while the still-visible marks keep moving.
  const hiddenTail = async (page, pass) => {
    const hidden = {};
    hidden.detach = { at: Date.now() };
    await page.evaluate(() => { const wrap = document.getElementById('hidden-wrap'); wrap.style.position = 'fixed'; wrap.style.left = '-3000px'; wrap.style.top = '0'; });
    await page.waitForFunction(() => document.querySelectorAll('#hidden-wrap [data-testid="composer-running-tone"]').length === 0, undefined, { timeout: 5000 });
    hidden.detach.toneCountAfter = 0;
    // The visible marks keep animating: the shared clock survives for whoever still retains it.
    const stillFrames = await shootSequence(page, path.join(output, 'frames', 'hidden-live-still'), 3, 400);
    for (const frame of stillFrames.slice(1)) assert.ok(frame.diff.changedPixels > 0, 'the visible mark keeps flowing while another mark is hidden');
    hidden.visibleStillFlowing = true;
    // Hide every mark: no consumer retains the shared clock, so every tone is gone.
    hidden.allDetached = { at: Date.now() };
    await page.evaluate(() => {
      for (const id of ['live-wrap', 'composer-wrap', 'evidence-wrap']) { const wrap = document.getElementById(id); wrap.style.position = 'fixed'; wrap.style.left = '-3000px'; wrap.style.top = '0'; }
    });
    await page.waitForFunction(() => document.querySelectorAll('[data-testid="composer-running-tone"]').length === 0, undefined, { timeout: 5000 });
    hidden.allDetached.toneCountAfter = 0;
    // Re-enter: the mark remounts its flow fresh — frame 0 starts fully opaque (clock restarted at 0).
    hidden.reenter = { at: Date.now() };
    await page.evaluate(() => { const wrap = document.getElementById('live-wrap'); wrap.style.position = ''; wrap.style.left = ''; wrap.style.top = ''; });
    await page.waitForSelector('#live-wrap [data-testid="composer-running-tone"]', { timeout: 5000 });
    const freshOpacity = await page.evaluate(frameOpacity, '#live-wrap [data-testid="composer-running-frame-0"]');
    assert.ok(freshOpacity !== null && freshOpacity >= 0.99, `the re-entering mark restarts its flow from frame 0 (opacity ${freshOpacity})`);
    hidden.reenter.frame0Opacity = freshOpacity;
    // Restore the other wraps so the page stays coherent for any later reads.
    await page.evaluate(() => {
      for (const id of ['composer-wrap', 'evidence-wrap', 'hidden-wrap']) { const wrap = document.getElementById(id); wrap.style.position = ''; wrap.style.left = ''; wrap.style.top = ''; }
    });
    pass.hidden = hidden;
  };
  // Live OS toggle: Reduce Motion flips mid-run — only the pulse stops, the colour flow never does.
  const toggleTail = async (page, pass) => {
    const toggle = {};
    toggle.toReduce = { at: Date.now() };
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.waitForFunction(() => {
      const el = document.querySelector('#composer-wrap [data-testid="composer-running-pulse"]');
      return getComputedStyle(el).transform === 'matrix(1, 0, 0, 1, 0, 0)';
    }, undefined, { timeout: 5000 });
    const reduceSamples = await sampleTransforms(page, 6, 160);
    assert.ok(reduceSamples.every(sample => sample.transform === 'matrix(1, 0, 0, 1, 0, 0)'), 'the live toggle drops only the pulse');
    const reduceFrames = await shootSequence(page, path.join(output, 'frames', 'toggle-reduce-flow'), 4, 350);
    for (const frame of reduceFrames.slice(1)) assert.ok(frame.diff.changedPixels > 0, 'the colour flow continues under a live Reduce Motion toggle');
    assert.equal(await page.evaluate(toneCount, ''), 4, 'every visible mark keeps its tone under Reduce Motion');
    toggle.toReduce = { ...toggle.toReduce, scaleFrozen: true, flowContinues: true, tones: 4 };
    toggle.toMotion = { at: Date.now() };
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await page.waitForFunction(() => {
      const el = document.querySelector('#composer-wrap [data-testid="composer-running-pulse"]');
      const match = /^matrix\(([^)]+)\)$/.exec(getComputedStyle(el).transform);
      return match && Number(match[1].split(',')[0]) > 1.005;
    }, undefined, { timeout: 5000 });
    toggle.toMotion.pulseResumed = true;
    pass.liveToggle = toggle;
  };
  const reduceTail = async (page, pass) => {
    const toggle = {};
    toggle.toMotion = { at: Date.now() };
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await page.waitForFunction(() => {
      const el = document.querySelector('#composer-wrap [data-testid="composer-running-pulse"]');
      const match = /^matrix\(([^)]+)\)$/.exec(getComputedStyle(el).transform);
      return match && Number(match[1].split(',')[0]) > 1.005;
    }, undefined, { timeout: 5000 });
    toggle.pulseResumed = true;
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.waitForFunction(() => {
      const el = document.querySelector('#composer-wrap [data-testid="composer-running-pulse"]');
      return getComputedStyle(el).transform === 'matrix(1, 0, 0, 1, 0, 0)';
    }, undefined, { timeout: 5000 });
    const backFrames = await shootSequence(page, path.join(output, 'frames', 'reduce-restored-flow'), 3, 350);
    for (const frame of backFrames.slice(1)) assert.ok(frame.diff.changedPixels > 0, 'the colour flow continues when Reduce Motion returns');
    toggle.flowContinues = true;
    pass.liveToggle = toggle;
  };
  await runPass('reduce-on', { reducedMotion: 'reduce' }, reduceTail);
  await runPass('motion', { reducedMotion: 'no-preference' }, async (page, pass) => { await hiddenTail(page, pass); await toggleTail(page, pass); });
  report.perMarkCost = {
    web: { animatedFrameViews: 8, svgNodes: 8, measured: 'DOM count of the real render (live row tone)' },
    iOS: { animatedViews: 8, svgNodes: 8, measured: 'static mount count from RAINBOW_FRAMES = 8; on-device measurement UNVERIFIED' },
  };
  report.chosenK = 8;
  report.errors = errors;
  assert.deepEqual(errors, [], 'no page errors');
  report.pass = report.cases.length === 2 && report.cases.every(entry => entry.status === 'PASS');
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  assert.ok(report.pass, JSON.stringify(report.cases));
  console.log(JSON.stringify({ gate: 'G-RENDER-1', pass: true, passes: Object.keys(report.passes), chosenK: 8 }));
})().catch(async error => {
  report.pass = false;
  if (!report.cases.length) report.cases.push({ status: 'FAIL', error: String(error && error.stack || error) });
  try { fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n'); } catch {}
  console.error(error);
  process.exitCode = 1;
}).finally(async () => {
  try { if (browser) await browser.close(); } catch {}
  try { if (server) await server.close(); } catch {}
  fs.rmSync(fixture, { recursive: true, force: true });
});
