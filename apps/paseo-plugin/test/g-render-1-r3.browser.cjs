// G-RENDER-1-r3: the linear-clock gate for the R37-r3 running-mark clock (ASMA-8539 D7).
// Real react-native-web 0.21.4 + the host's react-native-svg MAJOR (15.15.4, read from the installed
// Paseo.app web bundle's embedded dependency manifest) rendering the real plugin client modules in
// headless Chromium. Reduced motion ON: at least 8 DOM samples ~300 ms apart on one running mark,
// spanning at least one wrap of the 2700 ms loop; per sample the top frame, the eight crossfade
// opacities and the gradientTransform are recorded, position = top frame + crossfade, and the
// per-sample progress in frames must stay within ±15% of linear ((interval / 2700 ms) × 8).
// Reduced motion OFF: one pass showing the pulse scale reaching ~1.12 and returning to 1.
// Frames, samples JSON, generator command and result.json all land in the evidence dir. No daemon,
// host account, native session or device effects. Evidence stays outside the repository.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const { execFileSync } = require('node:child_process');
const plugin = path.resolve(__dirname, '..');
const consoleDeps = createRequire(path.join(plugin, '../console/package.json'));
assert.ok(process.env.EVIDENCE_DIR, 'Supply an external EVIDENCE_DIR');
const output = path.join(path.resolve(process.env.EVIDENCE_DIR), 'g-render-1-r3');
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
fs.writeFileSync(path.join(renderer, 'package.json'), '{"name":"r37-r3-clock-renderer","private":true}\n');
const pinned = {
  react: 'react@19.1.0/node_modules/react',
  'react-dom': 'react-dom@19.1.0_react@19.1.0/node_modules/react-dom',
  'react-native-web': 'react-native-web@0.21.4_react-dom@19.1.0_react@19.1.0__react@19.1.0/node_modules/react-native-web',
  'lucide-react': 'lucide-react@1.50.0_@types+react@19.2.18_react@19.1.0/node_modules/lucide-react',
};
const install = ['install', `react-native-svg@${SVG_VERSION}`, '--no-save', '--ignore-scripts', '--no-audit', '--no-fund'];
execFileSync('npm', install, { cwd: renderer, stdio: 'pipe', encoding: 'utf8' });
for (const name of [...Object.keys(pinned), 'react-native']) fs.rmSync(path.join(renderer, 'node_modules', name), { recursive: true, force: true });
for (const [name, dir] of Object.entries(pinned)) fs.symlinkSync(path.join(store, dir), path.join(renderer, 'node_modules', name), 'dir');
const rootOf = name => path.join(renderer, 'node_modules', name);
const versionOf = name => require(path.join(rootOf(name), 'package.json')).version;
for (const [name, version] of [['react', '19.1.0'], ['react-dom', '19.1.0'], ['react-native-web', '0.21.4'], ['lucide-react', '1.50.0'], ['react-native-svg', SVG_VERSION]]) {
  assert.equal(require(path.join(rootOf(name), 'package.json')).version, version, `pinned/installed ${name}`);
}
const svgModule = path.join(rootOf('react-native-svg'), 'lib/module');
fs.writeFileSync(path.join(svgModule, 'ReactNativeSVG.js'), "export * from './ReactNativeSVG.web.js';\nexport default null;\n");
fs.writeFileSync(path.join(svgModule, 'elements.js'), "export * from './elements.web.js';\nexport { default } from './elements.web.js';\n");
const hostBundleText = fs.readFileSync(SVG_HOST_BUNDLE, 'utf8');
assert.ok(hostBundleText.includes(`"react-native-svg":"${SVG_VERSION}"`), `host bundle manifest pins react-native-svg ${SVG_VERSION}`);
const generatorCommand = [process.execPath, ...process.argv.slice(1)].join(' ')
  + `  (EVIDENCE_DIR=${process.env.EVIDENCE_DIR})`;
fs.writeFileSync(path.join(output, 'commands.txt'), [
  '# G-RENDER-1-r3 generator and renderer commands — ' + new Date().toISOString(),
  'node ' + process.argv.slice(1).map(a => a.replace(os.homedir(), '~')).join(' '),
  `  EVIDENCE_DIR=${process.env.EVIDENCE_DIR}`,
  'npm ' + install.join(' ') + '   # cwd: ' + renderer.replace(os.homedir(), '~'),
  `renderer: react ${versionOf('react')}, react-dom ${versionOf('react-dom')}, react-native-web ${versionOf('react-native-web')}, lucide-react ${versionOf('lucide-react')}, react-native-svg ${versionOf('react-native-svg')} (host major from ${SVG_HOST_BUNDLE})`,
  '',
].join('\n'));

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-clock-gate-'));
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), 'export const getPaseoClient = () => { throw new Error("not used by this evidence"); };\n');
fs.writeFileSync(path.join(fixture, 'rn-deep-stub.ts'), 'const codegenNativeComponent = () => null;\nexport default codegenNativeComponent;\n');
fs.writeFileSync(path.join(fixture, 'rn-asset-registry-stub.ts'), 'export const getAssetByID = () => undefined;\n');
fs.writeFileSync(path.join(fixture, 'session-view-stub.ts'), [
  'export type StatusFilter = string;',
  'export const QUICK_STATUS_FILTERS = ["recent","activity","current","all","running","idle","error","needs_input","attention","favorites","initializing","closed","earlier_error","provider_blocked"];',
  'export const statusFilterLabel = (status: string) => status;',
  '',
].join('\n'));
fs.writeFileSync(path.join(fixture, 'icons.tsx'), `import React from 'react';
import Play from ${JSON.stringify(path.join(rootOf('lucide-react'), 'dist/esm/icons/play.mjs'))};
import { SvgXml } from 'react-native-svg';
export const Icon = ({name, icon, size = 16, color}) => {
  if (icon && typeof icon.svg === 'string') return <SvgXml xml={icon.svg} width={size} height={size} />;
  if (name === 'Play') return <Play size={size} color={color} strokeWidth={2} aria-hidden />;
  return <div style={{width:size, height:size}} />;
};
`);
const dark = { surface0:'#181B1A', surface1:'#1E2120', surface2:'#272A29', border:'#252B2A', foreground:'#fafafa', foregroundMuted:'#A1A5A4', accent:'#20744A', accentForeground:'#ffffff', statusSuccess:'#6cb17b', statusWarning:'#c09664', statusDanger:'#d8847b' };
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React from 'react';
import { createRoot } from 'react-dom/client';
import { View, Text } from 'react-native';
import { ComposerPulse } from ${JSON.stringify(path.join(plugin, 'client/composer-pulse.tsx'))};
const dark = ${JSON.stringify(dark)};
const label = (text) => <Text style={{color: dark.foreground, fontSize: 13, fontFamily: 'monospace'}}>{text}</Text>;
function App() {
  return <div style={{display:'flex', flexDirection:'column', gap:22, padding:18, background: dark.surface0}}>
    <div id="evidence-wrap" style={{display:'flex', alignItems:'center', gap:10, background: dark.surface1, padding: 24}}>
      {label('clock mark')} <ComposerPulse colors={dark} size={96} name="Play" />
    </div>
  </div>;
}
createRoot(document.getElementById('root')).render(<App />);
// The sample read: per sample, every frame's crossfade opacity and the gradient transforms, plus the
// carrier scale (for the reduced-motion-OFF pass). Positions are computed out of these raw reads only.
(window).clockRead = () => {
  const wrap = document.querySelector('#evidence-wrap');
  const frames = [...wrap.querySelectorAll('[data-testid^="composer-running-frame-"]')].map((el) => ({
    id: el.getAttribute('data-testid'),
    opacity: el.style.opacity === '' ? null : Number(el.style.opacity),
  }));
  const gradients = [...wrap.querySelectorAll('linearGradient')].map((g) => ({
    gradientTransform: g.getAttribute('gradientTransform'),
  }));
  const pulse = wrap.querySelector('[data-testid="composer-running-pulse"]');
  const carrierTransform = pulse ? getComputedStyle(pulse).transform : null;
  return { frames, gradients, carrierTransform };
};
document.title = 'clock-gate-ready';
`);

let server, browser;
const report = {
  gate: 'G-RENDER-1-r3',
  scope: 'Real-render web evidence on the pinned renderer (react 19.1.0 / react-dom 19.1.0 / react-native-web 0.21.4 / react-native-svg 15.15.4) with the real plugin client modules. Per-sample progress is computed from the DOM reads only (frame crossfade opacities); iOS/native is UNVERIFIED on-device. No host, account or device effects.',
  generator: { file: 'apps/paseo-plugin/test/g-render-1-r3.browser.cjs', command: generatorCommand, evidenceDir: path.resolve(process.env.EVIDENCE_DIR) },
  renderer: {
    react: versionOf('react'), reactDom: versionOf('react-dom'), reactNativeWeb: versionOf('react-native-web'),
    lucideReact: versionOf('lucide-react'), reactNativeSvg: versionOf('react-native-svg'),
    reactNativeSvgSource: 'npm registry install into ' + renderer,
    hostReactNativeSvg: { version: SVG_VERSION, readFrom: SVG_HOST_BUNDLE, verified: hostBundleText.includes(`"react-native-svg":"${SVG_VERSION}"`) },
  },
  passes: {},
};

// position in frames (0..K), per the brief: top frame + crossfade. Mid-loop the successor fades in
// over one full frame period while its predecessor sits at plateau, so position = top - 1 + alpha
// tracks the clock linearly. At the wrap handoff (progress >= at(K-1)) the TOP frame itself fades
// OUT over the saturated base layer (running-mark.tsx: frame K-1's outputRange is [0, 1, 0] and
// frame 0 returns to 1), so there the mirror term is the true position: (K-1) + (1 - alpha_top).
// Detected without history: the top frame is K-1, the base layer is saturated (alpha_0 ~= 1), and
// the K-2 plateau has already vanished. Raw reads only — the deltas decide the gate.
const readPosition = (frames) => {
  const K = frames.length;
  const alphaOf = (i) => (frames[i] && frames[i].opacity !== null && frames[i].opacity !== undefined) ? frames[i].opacity : 0;
  const painted = frames.map((f, index) => ({ index, alpha: f.opacity ?? 0 })).filter((f) => f.alpha > 0.0005);
  if (!painted.length) return null;
  const top = painted.reduce((max, f) => (f.index > max.index ? f : max), painted[0]);
  const baseSaturated = alphaOf(0) > 0.999;
  const prevPlateauGone = !painted.some((f) => f.index === K - 2 && f.alpha > 0.999);
  if (top.index === K - 1 && baseSaturated && prevPlateauGone) {
    return (K - 1) + (1 - Math.min(1, Math.max(0, top.alpha)));
  }
  return top.index - 1 + Math.min(1, Math.max(0, top.alpha));
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
  const LOOP_MS = 2700, K = 8, SAMPLES = 11, GAP_MS = 300;
  try {
    // ---- Pass 1: reduced motion ON — the linear-clock DOM sampling across at least one wrap ----
    {
      const context = await browser.newContext({ viewport: { width: 900, height: 420 }, reducedMotion: 'reduce' });
      const page = await context.newPage();
      page.on('pageerror', error => errors.push(`reduce-on: ${error}`));
      await page.goto(server.resolvedUrls.local[0]);
      await page.waitForFunction(() => document.title === 'clock-gate-ready', undefined, { timeout: 30000 });
      await page.waitForSelector('#evidence-wrap [data-testid="composer-running-tone"]', { timeout: 15000 });
      const samples = [];
      for (let index = 0; index < SAMPLES; index++) {
        const read = await page.evaluate(() => (window).clockRead());
        samples.push({ index, at: Date.now(), position: readPosition(read.frames), frames: read.frames, gradients: read.gradients, carrierTransform: read.carrierTransform });
        if (index + 1 < SAMPLES) await page.waitForTimeout(GAP_MS);
      }
      const spanMs = samples[samples.length - 1].at - samples[0].at;
      // Durable before any assertion: failed runs keep their raw samples (D3-N6 practice).
      fs.writeFileSync(path.join(output, 'samples-reduce-on.json'), JSON.stringify(samples, null, 1));
      for (const sample of samples) assert.notEqual(sample.position, null, 'every sample paints at least one frame');
      assert.ok(spanMs >= LOOP_MS, `the samples span at least one wrap (${spanMs} ms >= ${LOOP_MS} ms)`);
      const deltas = [];
      for (let i = 1; i < samples.length; i++) {
        const interval = samples[i].at - samples[i - 1].at;
        const raw = samples[i].position - samples[i - 1].position;
        const wrapped = ((raw % K) + K) % K; // the wrap jumps 8 -> 0; modulo puts it back on the ring
        const expected = (interval / LOOP_MS) * K;
        const deviation = Math.abs(wrapped - expected);
        deltas.push({ from: i - 1, to: i, interval, rawPositionDelta: raw, wrappedProgress: Math.round(wrapped * 1000) / 1000, expectedLinear: Math.round(expected * 1000) / 1000, deviation: Math.round(deviation * 1000) / 1000, withinTolerance: deviation <= 0.15 * expected });
      }
      for (const delta of deltas) assert.ok(delta.withinTolerance, `per-sample progress within ±15% of linear (${delta.from}->${delta.to}): got ${delta.wrappedProgress} frames over ${delta.interval} ms, expected ${delta.expectedLinear}`);
      report.passes['reduce-on'] = {
        samples: samples.map(({ index, at, position, carrierTransform }) => ({ index, at, positionFrames: position === null ? null : Math.round(position * 1000) / 1000, carrierTransform })),
        gradients: samples[0].gradients,
        spanMs, deltas,
        linearCheck: 'per-sample wrapped progress within ±15% of (interval / 2700 ms) × 8',
      };
      fs.writeFileSync(path.join(output, 'samples-reduce-on.json'), JSON.stringify(samples, null, 1));
      await page.screenshot({ path: path.join(output, 'pass-reduce-on-page.png'), fullPage: true });
      await context.close();
    }
    // ---- Pass 2: reduced motion OFF — the pulse still rides the same clock (scale ~1.12 and back to 1) ----
    {
      const context = await browser.newContext({ viewport: { width: 900, height: 420 }, reducedMotion: 'no-preference' });
      const page = await context.newPage();
      page.on('pageerror', error => errors.push(`reduce-off: ${error}`));
      await page.goto(server.resolvedUrls.local[0]);
      await page.waitForFunction(() => document.title === 'clock-gate-ready', undefined, { timeout: 30000 });
      await page.waitForSelector('#evidence-wrap [data-testid="composer-running-pulse"]', { timeout: 15000 });
      const scales = [];
      for (let index = 0; index < 24; index++) {
        const read = await page.evaluate(() => (window).clockRead());
        const match = /^matrix\(([^)]+)\)$/.exec(read.carrierTransform || '');
        const a = match ? Number(match[1].split(',')[0]) : null;
        scales.push({ index, at: Date.now(), scaleA: a });
        await page.waitForTimeout(140);
      }
      const values = scales.map((s) => s.scaleA).filter((v) => v !== null);
      const max = Math.max(...values), min = Math.min(...values);
      assert.ok(max > 1.005, `the pulse lifts the carrier above 1: max ${max}`);
      assert.ok(max <= 1.121, `the pulse never exceeds the 1.12 cap: max ${max}`);
      assert.ok(min >= 0.999, `the pulse returns to 1: min ${min}`);
      report.passes['reduce-off'] = { scales, maxScale: Math.round(max * 1000) / 1000, minScale: Math.round(min * 1000) / 1000, note: 'the pulse reaches ~1.12 and returns to 1 on the same linear clock' };
      fs.writeFileSync(path.join(output, 'samples-reduce-off.json'), JSON.stringify(scales, null, 1));
      await page.screenshot({ path: path.join(output, 'pass-reduce-off-page.png'), fullPage: true });
      await context.close();
    }
    assert.deepEqual(errors, [], `no page errors: ${JSON.stringify(errors)}`);
    report.pageErrors = errors;
    report.pass = true;
  } catch (error) {
    report.pass = false;
    report.error = String(error && error.stack || error);
    report.pageErrors = errors;
  } finally {
    fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 1));
    if (server) await server.close();
    if (browser) await browser.close();
    fs.rmSync(fixture, { recursive: true, force: true });
  }
  if (!report.pass) { console.error('G-RENDER-1-r3 FAILED — see result.json'); process.exit(1); }
  console.log('G-RENDER-1-r3 PASS');
})();
