// Focused web evidence for the rainbow running mark: the real plugin client modules on the pinned renderer
// (React 19.1.0 / React DOM 19.1.0 / react-native-web 0.21.4 / lucide-react 1.50.0, the host's pin).
// No daemon, host account, native session or device effects. Evidence stays outside the repository.
// The K lab measures flow for K = 4, 6 and 8 from the shipped frame paint and crossfade curve; the shipped
// RAINBOW_FRAMES constant must equal the chosen K (R37_CHOSEN_K, default 6).
const KLAB = [4, 6, 8];
const SIZES = [16, 18, 20, 22];
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createRequire } = require('node:module');
const plugin = path.resolve(__dirname, '..');
const consoleDeps = createRequire(path.join(plugin, '../console/package.json'));
assert.ok(process.env.EVIDENCE_DIR, 'Supply an external EVIDENCE_DIR');
const output = path.resolve(process.env.EVIDENCE_DIR);
assert.ok(!output.startsWith(plugin + path.sep), 'Evidence stays outside the plugin tree');
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || consoleDeps.resolve('@playwright/test'));
const store = path.join(plugin, '../../node_modules/.pnpm');
const renderer = path.join(output, 'renderer');
fs.rmSync(renderer, { recursive: true, force: true });
fs.mkdirSync(path.join(renderer, 'node_modules'), { recursive: true });
fs.writeFileSync(path.join(renderer, 'package.json'), '{"name":"r37-pinned-renderer","private":true,"type":"module"}\n');
const pinned = {
  react: 'react@19.1.0/node_modules/react',
  'react-dom': 'react-dom@19.1.0_react@19.1.0/node_modules/react-dom',
  'react-native-web': 'react-native-web@0.21.4_react-dom@19.1.0_react@19.1.0__react@19.1.0/node_modules/react-native-web',
  'lucide-react': 'lucide-react@1.50.0_@types+react@19.2.18_react@19.1.0/node_modules/lucide-react',
};
for (const [name, dir] of Object.entries(pinned)) fs.symlinkSync(path.join(store, dir), path.join(renderer, 'node_modules', name), 'dir');
const rootOf = name => path.join(renderer, 'node_modules', name);
for (const [name, version] of [['react', '19.1.0'], ['react-dom', '19.1.0'], ['react-native-web', '0.21.4'], ['lucide-react', '1.50.0']]) {
  assert.equal(require(path.join(rootOf(name), 'package.json')).version, version, `pinned ${name}`);
}
const reference = process.env.R37_REFERENCE
  ? `data:image/png;base64,${fs.readFileSync(process.env.R37_REFERENCE).toString('base64')}` : '';
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-rainbow-mark-'));
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(fixture, 'package.json'), '{"private":true,"type":"module"}\n');
fs.writeFileSync(path.join(fixture, 'index.html'), '<html><meta name="viewport" content="width=device-width, initial-scale=1"><div id="root"></div><script type="module" src="/main.tsx"></script></html>');
fs.writeFileSync(path.join(fixture, 'sdk-client.ts'), 'export const getPaseoClient = () => { throw new Error("not used by this evidence"); };\n');
// The host draws a Lucide glyph for `name`, and inline plugin SVG through SvgXml at width/height = size.
fs.writeFileSync(path.join(fixture, 'icons.tsx'), `import React from 'react';
import Play from ${JSON.stringify(path.join(rootOf('lucide-react'), 'dist/esm/icons/play.mjs'))};
export const Icon = ({name, icon, size = 16, color}) => {
  if (icon && typeof icon.svg === 'string') return <div aria-hidden="true" data-kontor-svg="true" style={{width:size, height:size, display:'inline-flex'}} dangerouslySetInnerHTML={{__html: icon.svg}} />;
  if (name === 'Play') return <Play size={size} color={color} strokeWidth={2} aria-hidden />;
  return <div style={{width:size, height:size}} />;
};
`);
const THEMES = {
  dark: { surface0:'#181B1A', surface1:'#1E2120', surface2:'#272A29', border:'#252B2A', foreground:'#fafafa', foregroundMuted:'#A1A5A4', accent:'#20744A', accentForeground:'#ffffff', statusSuccess:'#6cb17b', statusWarning:'#c09664', statusDanger:'#d8847b' },
  light: { surface0:'#ffffff', surface1:'#fafafa', surface2:'#f4f4f5', border:'#e4e4e7', foreground:'#1a1a1e', foregroundMuted:'#71717a', accent:'#20744A', accentForeground:'#ffffff', statusSuccess:'#3e704a', statusWarning:'#7b5d39', statusDanger:'#9d433b' },
};
fs.writeFileSync(path.join(fixture, 'main.tsx'), `import React from 'react';
import { createRoot } from 'react-dom/client';
import { View, Text } from 'react-native';
import Play from ${JSON.stringify(path.join(rootOf('lucide-react'), 'dist/esm/icons/play.mjs'))};
import { ComposerPulse } from ${JSON.stringify(path.join(plugin, 'client/composer-pulse.tsx'))};
import { RunningRainbowMark, rainbowMarkSvg, RAINBOW_FRAMES } from ${JSON.stringify(path.join(plugin, 'client/running-mark.tsx'))};
const THEMES = ${JSON.stringify(THEMES)};
const SIZES = ${JSON.stringify(SIZES)};
const KLAB = ${JSON.stringify(KLAB)};
const REFERENCE = ${JSON.stringify(reference)};
const BASE_ANGLE = 20;
// The K lab re-uses the shipped frame-0 paint with only the rotation step parameterised, so K = 4, 6 and 8
// are measured on exactly the gradient, stops and crossfade curve the component ships.
const labSvg = (theme: string, k: number, frame: number) =>
  rainbowMarkSvg('play', THEMES[theme], 0).replace(/rotate\\([\\d.]+ 12 12\\)/, 'rotate(' + (BASE_ANGLE + (360 / k) * frame).toFixed(2) + ' 12 12)');
const oldSvg = (color: string) => '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="600" height="600" fill="none" stroke="' + color + '" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/></svg>';
const ink = (theme: string) => theme === 'dark' ? '#9cbfff' : '#245bba';
const report: Record<string, unknown> = { frames: RAINBOW_FRAMES, iou: {}, rms: {} };
(window as any).r37 = report;
(window as any).r37LabSvg = labSvg;

const loadImage = (markup: string) => new Promise<HTMLImageElement>((resolve, reject) => {
  const image = new Image(); image.onload = () => resolve(image); image.onerror = reject;
  image.src = 'data:image/svg+xml;utf8,' + encodeURIComponent(markup);
});

function Lab({theme}: {theme: string}) {
  const refs = React.useRef<(HTMLCanvasElement | null)[]>([]);
  React.useEffect(() => {
    let cancelled = false;
    (async () => {
      for (const k of KLAB) {
        if (cancelled) return;
        const frames = [];
        for (let frame = 0; frame < k; frame++) frames.push(await loadImage(labSvg(theme, k, frame)));
        const step = 96, columns = 8;
        const canvas = refs.current[KLAB.indexOf(k)]!;
        canvas.width = step * columns; canvas.height = step;
        const ctx = canvas.getContext('2d')!;
        let previous: Uint8ClampedArray | null = null; let rmsSum = 0, rmsCount = 0;
        for (let column = 0; column < columns; column++) {
          const t = column / columns, value = (1 - Math.cos(t * Math.PI * 2)) / 2;
          const phase = value * k, base = Math.floor(phase) % k, blend = phase - Math.floor(phase);
          ctx.clearRect(column * step, 0, step, step);
          ctx.globalAlpha = 1 - blend; ctx.drawImage(frames[base]!, column * step, 0, step, step);
          ctx.globalAlpha = blend; ctx.drawImage(frames[(base + 1) % k]!, column * step, 0, step, step);
          ctx.globalAlpha = 1;
          const next = ctx.getImageData(column * step, 0, step, step);
          if (previous) {
            let sum = 0;
            for (let index = 0; index < next.data.length; index += 4) {
              if (Math.max(next.data[index + 3]!, previous[index + 3]!) > 16) {
                sum += (next.data[index]! - previous[index]!) ** 2 + (next.data[index + 1]! - previous[index + 1]!) ** 2 + (next.data[index + 2]! - previous[index + 2]!) ** 2;
              }
            }
            rmsSum += Math.sqrt(sum / (next.data.length / 4)); rmsCount++;
          }
          previous = next.data;
        }
        (report.rms as any)[theme] ??= {};
        (report.rms as any)[theme]['k' + k] = Math.round((rmsSum / Math.max(1, rmsCount)) * 100) / 100;
      }
      for (const size of SIZES) {
        if (cancelled) return;
        const scale = 8, big = size * scale;
        const old = await loadImage(oldSvg(ink(theme)));
        const current = await loadImage(rainbowMarkSvg('play', THEMES[theme], 0));
        const canvas = document.createElement('canvas'); canvas.width = big; canvas.height = big;
        const ctx = canvas.getContext('2d')!;
        ctx.drawImage(old, 0, 0, big, big);
        const before = ctx.getImageData(0, 0, big, big);
        ctx.clearRect(0, 0, big, big);
        ctx.drawImage(current, 0, 0, big, big);
        const after = ctx.getImageData(0, 0, big, big);
        let shared = 0, only = 0;
        const overlay = document.createElement('canvas'); overlay.width = big; overlay.height = big;
        const paint = overlay.getContext('2d')!, image = paint.createImageData(big, big);
        for (let index = 0; index < before.data.length; index += 4) {
          const a = before.data[index + 3]! > 24, b = after.data[index + 3]! > 24;
          if (a && b) { shared++; image.data[index] = 250; image.data[index + 1] = 250; image.data[index + 2] = 250; image.data[index + 3] = 255; }
          else if (a || b) { only++; image.data[index] = a ? 220 : 40; image.data[index + 1] = a ? 40 : 200; image.data[index + 2] = 60; image.data[index + 3] = 255; }
        }
        paint.putImageData(image, 0, 0);
        (report.iou as any)[theme + '-' + size] = Math.round((shared / (shared + only)) * 10000) / 10000;
        const thumb = new Image(); thumb.src = overlay.toDataURL('image/png');
        thumb.style.width = size * 3 + 'px'; thumb.style.height = size * 3 + 'px'; thumb.style.imageRendering = 'pixelated';
        const host = document.getElementById('overlay-' + theme + '-' + size);
        if (host) { host.innerHTML = ''; host.appendChild(thumb); }
      }
      document.title = 'r37-rainbow-ready';
    })();
    return () => { cancelled = true; };
  }, [theme]);
  return <View style={{gap: 6}}>
    {KLAB.map((k, index) => <View key={k} style={{flexDirection:'row', alignItems:'center', gap:8}}>
      <Text style={{color:'#999', fontSize:12}}>K={k}</Text>
      <canvas ref={(node) => { refs.current[index] = node; }} style={{width: 384, height: 48, imageRendering: 'pixelated'}} />
    </View>)}
  </View>;
}

function Cell({theme, size}: {theme: string; size: number}) {
  return <View style={{flexDirection:'row', alignItems:'center', gap:10}} testID={'cell-' + theme + '-' + size}>
    <View testID={'old-' + theme + '-' + size}><Play size={size} color={ink(theme)} strokeWidth={2} aria-hidden /></View>
    <View testID={'static-' + theme + '-' + size}><RunningRainbowMark colors={THEMES[theme]} size={size} shape="play" /></View>
    <View testID={'live-' + theme + '-' + size}><ComposerPulse colors={THEMES[theme]} size={size} name="Play" /></View>
  </View>;
}

function App() {
  return <div style={{display:'flex', gap:24, padding:12, flexWrap:'wrap'}}>
    {Object.entries(THEMES).map(([theme, colors]) => <div key={theme} id={'theme-' + theme} style={{backgroundColor: colors.surface0, padding: 12}}>
      <div style={{color: colors.foreground, fontFamily:'monospace', fontSize:13, marginBottom:6}}>{theme}: current ink | static rainbow | live @ 16/18/20/22</div>
      <div style={{display:'flex', flexDirection:'column', gap:8}}>
        {SIZES.map((size) => <Cell key={size} theme={theme} size={size} />)}
      </div>
      <div style={{marginTop:10, color: colors.foreground, fontFamily:'monospace', fontSize:12}}>crossfade lab: one 2700ms cycle in 8 columns</div>
      <Lab theme={theme} />
      <div style={{marginTop:10, display:'flex', gap:12, alignItems:'flex-end'}}>
        <img src={REFERENCE} width={96} height={96} style={{display: REFERENCE ? 'block' : 'none'}} alt="reference frame" />
        <div id={'live-frames-' + theme} style={{display:'flex', gap:4, alignItems:'center'}} />
        {SIZES.map((size) => <div key={size} id={'overlay-' + theme + '-' + size} style={{display:'flex', flexDirection:'column', alignItems:'center', color: colors.foreground, fontFamily:'monospace', fontSize:10}}>
          <span>{size}px overlay</span>
        </div>)}
      </div>
    </div>)}
  </div>;
}
createRoot(document.getElementById('root')!).render(<App />);
`);

let server, browser;
const report = { scope: 'Web-only evidence on the pinned renderer with the real client modules; native/phone unverified. No host, account or device effects.', cases: [] };
(async () => {
  const vite = await import(consoleDeps.resolve('vite'));
  try {
    server = await vite.createServer({ root: fixture, configFile: false,
      define: { global: 'globalThis', __DEV__: 'false', 'process.env.NODE_ENV': '"development"' },
      server: { host: '127.0.0.1', port: 0, fs: { allow: [fixture, plugin, store, renderer] } },
      resolve: { alias: [
        { find: /^react$/, replacement: rootOf('react') }, { find: /^react\/(.*)$/, replacement: rootOf('react') + '/$1' },
        { find: /^react-dom$/, replacement: rootOf('react-dom') }, { find: /^react-dom\/(.*)$/, replacement: rootOf('react-dom') + '/$1' },
        { find: /^react-native$/, replacement: path.join(rootOf('react-native-web'), 'dist/index.js') },
        { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(fixture, 'sdk-client.ts') },
        { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(fixture, 'icons.tsx') },
      ] },
    });
    await server.listen();
    browser = await chromium.launch({ headless: true });
    const videos = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-rainbow-video-'));
    const context = await browser.newContext({ viewport: { width: 1280, height: 940 }, reducedMotion: 'no-preference', recordVideo: { dir: videos, size: { width: 1280, height: 940 } } });
    const page = await context.newPage();
    const video = page.video();
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.goto(server.resolvedUrls.local[0]);
    await page.waitForFunction(() => document.title === 'r37-rainbow-ready', undefined, { timeout: 30000 });
    const state = await page.evaluate(() => (window).r37);
    report.frames = state.frames;
    report.iou = state.iou;
    report.rms = state.rms;
    assert.equal(state.frames, Number(process.env.R37_CHOSEN_K ?? state.frames), 'the shipped K equals the chosen K');
    for (const [key, value] of Object.entries(state.iou)) assert.ok(value >= 0.98, `silhouette IoU ${key} = ${value}`);
    for (const theme of Object.keys(state.rms)) {
      for (const k of KLAB) assert.ok(state.rms[theme]['k' + k] >= 0, `rms recorded for K=${k}`);
      assert.ok(state.rms[theme].k6 > 0, 'the crossfade changes pixels');
    }
    const live = page.getByTestId('composer-running-tone');
    await live.first().waitFor({ timeout: 10000 });
    assert.equal(await live.count(), 8, 'four live marks per theme animate');
    const strokes = await page.locator('[data-testid="composer-running-tone"] svg').evaluateAll(nodes => nodes.map(node => node.getAttribute('stroke')));
    assert.ok(strokes.every(value => value && value.startsWith('url(#kontor-rainbow-')), 'every live frame paints the gradient');
    const carrier = page.locator('[data-testid="composer-running-pulse"]').first();
    const samples = new Set();
    for (let index = 0; index < 5; index++) { samples.add(await carrier.evaluate(node => getComputedStyle(node).transform)); await page.waitForTimeout(160); }
    assert.ok(samples.size > 1, 'the mark breathes');
    await page.waitForFunction(() => Number(document.querySelector('[data-testid="composer-running-frame-0"]')?.style.opacity) > 0.9, undefined, { timeout: 4000 });
    await page.waitForFunction(() => Number(document.querySelector('[data-testid="composer-running-frame-0"]')?.style.opacity) < 0.1, undefined, { timeout: 4000 });
    report.motion = { breathing: samples.size > 1, crossfade: true };
    // Discrete frames of the real live mark across one 2700ms cycle, next to the reference frame.
    for (const theme of ['dark', 'light']) {
      const mark = page.locator(`[data-testid="live-${theme}-22"]`);
      const frame = await mark.boundingBox();
      const strip = fs.mkdtempSync(path.join(os.tmpdir(), 'kontor-rainbow-frames-'));
      const shots = [];
      for (let index = 0; index < 6; index++) {
        const file = path.join(strip, `frame-${index}.png`);
        await mark.screenshot({ path: file });
        shots.push(fs.readFileSync(file).toString('base64'));
        await page.waitForTimeout(450);
      }
      await page.evaluate(({ shots, theme }) => {
        const host = document.getElementById('live-frames-' + theme);
        for (const shot of shots) {
          const image = new Image();
          image.src = 'data:image/png;base64,' + shot;
          image.style.width = '72px'; image.style.height = '72px'; image.style.imageRendering = 'auto';
          host.appendChild(image);
        }
      }, { shots, theme });
    }
    await page.waitForTimeout(300);
    for (const theme of ['dark', 'light']) {
      const box = await page.locator('#live-frames-' + theme).boundingBox();
      if (box) await page.screenshot({ path: path.join(output, `live-frames-${theme}.png`), clip: { ...box, height: box.height + 4, y: Math.max(0, box.y - 2) } });
    }
    for (const theme of Object.keys(THEMES)) {
      const box = await page.locator('#theme-' + theme).boundingBox();
      if (box) await page.screenshot({ path: path.join(output, `comparison-${theme}.png`), clip: box });
    }
    await page.screenshot({ path: path.join(output, 'flow-lab-and-overlays.png'), fullPage: true });
    await page.waitForTimeout(3000); // the recording keeps at least one full cycle
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await live.first().waitFor({ state: 'detached', timeout: 5000 });
    const resting = await page.locator('[data-kontor-svg] svg[stroke^="url(#kontor-rainbow-"]').count();
    assert.ok(resting >= 8, `reduced motion keeps the static rainbow (${resting} marks)`);
    report.reducedMotion = { toneStacks: 0, staticRainbows: resting };
    await page.screenshot({ path: path.join(output, 'reduced-motion-static-rainbow.png'), fullPage: true });
    report.errors = errors;
    assert.deepEqual(errors, []);
    report.cases.push({ name: 'web-evidence', status: 'PASS' });
    await context.close();
    if (video) await video.saveAs(path.join(output, 'rainbow-motion.webm'));
  } catch (error) {
    report.cases.push({ status: 'FAIL', error: String(error && error.stack || error) });
    report.pass = false;
    fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
    throw error;
  } finally {
    if (browser) await browser.close();
    if (server) await server.close();
    fs.rmSync(fixture, { recursive: true, force: true });
  }
  report.pass = report.cases.length === 1 && report.cases[0].status === 'PASS';
  fs.writeFileSync(path.join(output, 'result.json'), JSON.stringify(report, null, 2) + '\n');
  assert.ok(report.pass, JSON.stringify(report));
  console.log(JSON.stringify(report));
})().catch(error => { console.error(error); process.exitCode = 1; });
