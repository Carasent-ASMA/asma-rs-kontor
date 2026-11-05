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
import { RunningRainbowMark, rainbowMarkSvg, rainbowFrameInterpolation, RAINBOW_FRAMES } from ${JSON.stringify(path.join(plugin, 'client/running-mark.tsx'))};
import { appearanceColors } from ${JSON.stringify(path.join(plugin, 'client/kontor-appearance.ts'))};
const THEMES = ${JSON.stringify(THEMES)};
const SIZES = ${JSON.stringify(SIZES)};
const KLAB = ${JSON.stringify(KLAB)};
const REFERENCE = ${JSON.stringify(reference)};
const BASE_ANGLE = 20;
// The K lab re-uses the shipped frame-0 paint with only the rotation step parameterised, so K = 4, 6 and 8
// are measured on exactly the gradient, stops and crossfade curve the component ships.
const labSvg = (theme: string, k: number, frame: number) =>
  rainbowMarkSvg('play', THEMES[theme], 0).replace(/rotate\\([\\d.]+ 12 12\\)/, 'rotate(' + (BASE_ANGLE + (360 / k) * frame).toFixed(2) + ' 12 12)');
// The shipped compositing read, as Animated performs it on the shared clock value.
const opacityAt = (config: { inputRange: number[]; outputRange: number[] }, value: number): number => {
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
const oldSvg = (color: string) => '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="600" height="600" fill="none" stroke="' + color + '" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/></svg>';
const ink = (theme: string) => theme === 'dark' ? '#9cbfff' : '#245bba';
const report: Record<string, unknown> = { frames: RAINBOW_FRAMES, iou: {}, rms: {}, composite: {}, hue: {} };
(window as any).r37 = report;
(window as any).r37LabSvg = labSvg;
(window as any).r37FrameMarkup = (theme: string, frame: number) => rainbowMarkSvg('play', THEMES[theme], frame);
(window as any).r37CompositeAlphas = (theme: string) => {
  const midpoint = 2.5 / RAINBOW_FRAMES;
  return [rainbowFrameInterpolation(2, RAINBOW_FRAMES), rainbowFrameInterpolation(3, RAINBOW_FRAMES)].map((config) => opacityAt(config, midpoint));
};

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
          const configs = [];
          for (let frame = 0; frame < k; frame++) configs.push(rainbowFrameInterpolation(frame, k));
          const step = 96, columns = 8;
          const canvas = refs.current[KLAB.indexOf(k)]!;
          canvas.width = step * columns; canvas.height = step;
          const ctx = canvas.getContext('2d')!;
          let previous: Uint8ClampedArray | null = null; let rmsSum = 0, rmsCount = 0;
          for (let column = 0; column < columns; column++) {
            const t = column / columns;
            ctx.clearRect(column * step, 0, step, step);
            for (let frame = 0; frame < k; frame++) {
              const alpha = opacityAt(configs[frame]!, t);
              if (alpha <= 0) continue;
              ctx.globalAlpha = alpha; ctx.drawImage(frames[frame]!, column * step, 0, step, step);
            }
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
      // F1/B1 regression on the composited frame: at a handoff midpoint the stroke must be fully covered
      // (no 0.75-coverage thinning) and every interior pixel must keep >=3:1 contrast over EVERY surface
      // the marks really sit on: host tokens AND the Kontor appearance overrides (review-b S2).
      {
        const frameA = await loadImage(rainbowMarkSvg('play', THEMES[theme], 2));
        const frameB = await loadImage(rainbowMarkSvg('play', THEMES[theme], 3));
        const midpoint = 2.5 / RAINBOW_FRAMES;
        const alphaA = opacityAt(rainbowFrameInterpolation(2, RAINBOW_FRAMES), midpoint);
        const alphaB = opacityAt(rainbowFrameInterpolation(3, RAINBOW_FRAMES), midpoint);
        const big = 600;
        const layerCanvas = document.createElement('canvas'); layerCanvas.width = big; layerCanvas.height = big;
        const lctx = layerCanvas.getContext('2d')!;
        lctx.globalAlpha = alphaA; lctx.drawImage(frameA, 0, 0, big, big);
        lctx.globalAlpha = alphaB; lctx.drawImage(frameB, 0, 0, big, big);
        lctx.globalAlpha = 1;
        const layers = lctx.getImageData(0, 0, big, big).data;
        let interior = 0, thinned = 0;
        const solid = new Uint8Array(big * big);
        for (let at = 0, index = 0; at < solid.length; at++, index += 4) {
          const alpha = layers[index + 3]!;
          if (alpha >= 254) { solid[at] = 1; interior++; }
          else if (alpha > 100) { thinned++; }
        }
        const coverage = interior + thinned === 0 ? 1 : interior / (interior + thinned);
        (report.composite as any)[theme] ??= {};
        (report.composite as any)[theme].coverage = Math.round(coverage * 10000) / 10000;
        const lum = (r: number, g: number, b: number) => {
          const linear = (channel: number) => { const value = channel / 255; return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4; };
          return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
        };
        const lumOf = (hex: string) => lum(parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16));
        const surfaceSets: Record<string, string[]> = {
          host: [THEMES[theme].surface0, THEMES[theme].surface1, THEMES[theme].surface2],
          kontor: [appearanceColors(THEMES[theme], theme).surface0, appearanceColors(THEMES[theme], theme).surface1, appearanceColors(THEMES[theme], theme).surface2],
        };
        let minContrast = Infinity; const perSurface: Record<string, number> = {};
        for (const [setName, surfaces] of Object.entries(surfaceSets)) for (const surface of surfaces) {
          const surfaceLum = lumOf(surface);
          const over = document.createElement('canvas'); over.width = big; over.height = big;
          const octx = over.getContext('2d')!;
          octx.fillStyle = surface; octx.fillRect(0, 0, big, big);
          octx.globalAlpha = alphaA; octx.drawImage(frameA, 0, 0, big, big);
          octx.globalAlpha = alphaB; octx.drawImage(frameB, 0, 0, big, big);
          octx.globalAlpha = 1;
          const pixels = octx.getImageData(0, 0, big, big).data;
          let surfaceMin = Infinity;
          for (let at = 0, index = 0; at < solid.length; at++, index += 4) {
            if (!solid[at]) continue;
            const value = lum(pixels[index]!, pixels[index + 1]!, pixels[index + 2]!);
            const ratio = (Math.max(value, surfaceLum) + 0.05) / (Math.min(value, surfaceLum) + 0.05);
            minContrast = Math.min(minContrast, ratio); surfaceMin = Math.min(surfaceMin, ratio);
          }
          perSurface[setName + ' ' + surface] = Math.round(surfaceMin * 1000) / 1000;
        }
        (report.composite as any)[theme].minContrast = Math.round(minContrast * 1000) / 1000;
        (report.composite as any)[theme].perSurface = perSurface;
      }
      // B1: the composited stack alpha over the FULL clock is exactly 1, and the composited contrast is
      // reported at the frame peaks vs the handoff midpoints (review-b's crossfade method, ours re-run).
      {
        const configs = Array.from({ length: RAINBOW_FRAMES }, (_, frame) => rainbowFrameInterpolation(frame, RAINBOW_FRAMES));
        let minStackAlpha = 1;
        for (let step = 0; step <= 2400; step++) {
          const value = step / 2400;
          let stack = 0;
          for (const config of configs) { const a = opacityAt(config, value); stack = a + (1 - a) * stack; }
          minStackAlpha = Math.min(minStackAlpha, stack);
        }
        const stats = (values: number[]) => {
          const sorted = [...values].sort((a, b) => a - b);
          const at = (share: number) => sorted[Math.min(sorted.length - 1, Math.floor(share * sorted.length))]!;
          return { min: Math.round(at(0) * 1000) / 1000, p05: Math.round(at(0.05) * 1000) / 1000, median: Math.round(at(0.5) * 1000) / 1000 };
        };
        const peakAt: number[] = [0]; const crossfadeAt: number[] = [];
        for (let frame = 1; frame < RAINBOW_FRAMES; frame++) { peakAt.push(frame / RAINBOW_FRAMES); crossfadeAt.push((frame - 0.5) / RAINBOW_FRAMES); }
        crossfadeAt.push(((RAINBOW_FRAMES - 1) / RAINBOW_FRAMES + 1) / 2); // the wrap handoff midpoint
        const small = 240;
        const frames = [];
        for (let frame = 0; frame < RAINBOW_FRAMES; frame++) frames.push(await loadImage(rainbowMarkSvg('play', THEMES[theme], frame)));
        const lum = (r: number, g: number, b: number) => {
          const linear = (channel: number) => { const value = channel / 255; return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4; };
          return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
        };
        const surface = THEMES[theme].surface0;
        const surfaceLum = lum(parseInt(surface.slice(1, 3), 16), parseInt(surface.slice(3, 5), 16), parseInt(surface.slice(5, 7), 16));
        const contrastAt = async (t: number) => {
          const over = document.createElement('canvas'); over.width = small; over.height = small;
          const octx = over.getContext('2d')!;
          octx.fillStyle = surface; octx.fillRect(0, 0, small, small);
          for (let frame = 0; frame < RAINBOW_FRAMES; frame++) {
            const alpha = opacityAt(configs[frame]!, t);
            if (alpha <= 0) continue;
            octx.globalAlpha = alpha; octx.drawImage(frames[frame]!, 0, 0, small, small);
          }
          octx.globalAlpha = 1;
          const pixels = octx.getImageData(0, 0, small, small).data;
          const ratios: number[] = [];
          for (let at = 0, index = 0; at < solidAt.length; at++, index += 4) {
            if (!solidAt[at]) continue;
            const value = lum(pixels[index]!, pixels[index + 1]!, pixels[index + 2]!);
            ratios.push((Math.max(value, surfaceLum) + 0.05) / (Math.min(value, surfaceLum) + 0.05));
          }
          return stats(ratios);
        };
        // The sweep reads only the opaque stroke interior: one frame's alpha>=254 mask. All K frames share
        // the exact same geometry, so the interior is identical for every frame; the union of six renders
        // would admit boundary pixels whose partial coverage drags the contrast down.
        const solidCanvas = document.createElement('canvas'); solidCanvas.width = small; solidCanvas.height = small;
        const sctx = solidCanvas.getContext('2d')!;
        sctx.drawImage(frames[0]!, 0, 0, small, small);
        const solidData = sctx.getImageData(0, 0, small, small).data;
        const solidAt = new Uint8Array(small * small);
        for (let at = 0, index = 0; at < solidAt.length; at++, index += 4) solidAt[at] = solidData[index + 3]! >= 254 ? 1 : 0;
        const peak = []; const crossfade = [];
        for (const t of peakAt) peak.push(await contrastAt(t));
        for (const t of crossfadeAt) crossfade.push(await contrastAt(t));
        const fold = (entries: { min: number; p05: number; median: number }[]) => ({
          min: Math.min(...entries.map((entry) => entry.min)),
          p05: Math.min(...entries.map((entry) => entry.p05)),
          median: Math.round(entries.reduce((sum, entry) => sum + entry.median, 0) / entries.length * 1000) / 1000,
        });
        (report.composite as any)[theme].minStackAlpha = Math.round(minStackAlpha * 1000000) / 1000000;
        (report.composite as any)[theme].peak = fold(peak);
        (report.composite as any)[theme].crossfade = fold(crossfade);
      }
      // S1: the frame-0 hue extremes. The 4..20 gradient span shows the wrapped violet ends the reference
      // frame shows; the 0..24 span clips them to magenta/blue (review-b's measurement, re-run here).
      {
        const render = 176; // 8x the 24-unit viewBox
        const canvas = document.createElement('canvas'); canvas.width = render; canvas.height = render;
        const ctx = canvas.getContext('2d')!;
        ctx.drawImage(await loadImage(rainbowMarkSvg('play', THEMES[theme], 0)), 0, 0, render, render);
        const pixels = ctx.getImageData(0, 0, render, render).data;
        // Frame 0's gradient axis after rotate(20): the hue extremes live at the projection extremes of the
        // stroke, so each sample window returns the most extreme pixel along that axis (max-saturation would
        // bias to blue over violet).
        const radians = 20 * Math.PI / 180, dirX = Math.cos(radians), dirY = Math.sin(radians);
        const hueAt = (vx: number, vy: number, extreme: "min" | "max" | "near") => {
          let best: { rank: number; hue: number } | null = null;
          const unit = render / 24;
          const centerX = vx * unit, centerY = vy * unit;
          for (let y = Math.max(0, Math.round((vy - 1.4) * unit)); y < Math.min(render, Math.round((vy + 1.4) * unit)); y++) {
            for (let x = Math.max(0, Math.round((vx - 1.4) * unit)); x < Math.min(render, Math.round((vx + 1.4) * unit)); x++) {
              const index = (y * render + x) * 4;
              if (pixels[index + 3]! < 128) continue;
              const r = pixels[index]! / 255, g = pixels[index + 1]! / 255, b = pixels[index + 2]! / 255;
              const max = Math.max(r, g, b), min = Math.min(r, g, b), delta = max - min;
              if (!delta) continue;
              const rank = extreme === "near" ? (x - centerX) ** 2 + (y - centerY) ** 2 : extreme === "min" ? x * dirX + y * dirY : -(x * dirX + y * dirY);
              if (best && rank >= best.rank) continue;
              let hue = 0;
              if (max === r) hue = 60 * (((g - b) / delta) % 6);
              else if (max === g) hue = 60 * ((b - r) / delta + 2);
              else hue = 60 * ((r - g) / delta + 4);
              if (hue < 0) hue += 360;
              best = { rank, hue };
            }
          }
          return best ? Math.round(best.hue) : null;
        };
        (report.hue as any)[theme] = {
          // The two extremes sit at the stroke's projection extremes; the mid-edge hues are read at the
          // window centre (the extreme pick would slide along the edge toward the gradient's violet start).
          leftTop: hueAt(5.9, 4.9, "min"), leftMiddle: hueAt(5.2, 12, "near"), leftBottom: hueAt(5.5, 18.7, "near"), rightTip: hueAt(18.9, 12.3, "max"),
        };
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
    report.composite = state.composite;
    report.hue = state.hue;
    report.midpointExport = state.midpointExport;
    assert.equal(state.frames, Number(process.env.R37_CHOSEN_K ?? state.frames), 'the shipped K equals the chosen K');
    for (const [key, value] of Object.entries(state.iou)) assert.ok(value >= 0.98, `silhouette IoU ${key} = ${value}`);
    for (const theme of Object.keys(state.rms)) {
      for (const k of KLAB) assert.ok(state.rms[theme]['k' + k] >= 0, `rms recorded for K=${k}`);
      assert.ok(state.rms[theme].k6 > 0, 'the crossfade changes pixels');
    }
    // F1/B1: the composited stroke stays fully covered, the stack alpha is exactly 1 over the whole clock,
    // and the composited contrast stays >=3 at every surface (host tokens AND Kontor overrides, review-b S2).
    for (const theme of Object.keys(THEMES)) {
      assert.ok(state.composite[theme].coverage >= 0.6, `full-coverage crossfade on ${theme}: ${state.composite[theme].coverage}`);
      assert.equal(state.composite[theme].minStackAlpha, 1, `stack alpha exactly 1 over the loop on ${theme}`);
      assert.ok(state.composite[theme].minContrast >= 3, `composited contrast on ${theme}: ${state.composite[theme].minContrast}`);
      for (const [surface, ratio] of Object.entries(state.composite[theme].perSurface)) assert.ok(ratio >= 3, `midpoint contrast ${ratio} on ${surface} (${theme})`);
      assert.ok(state.composite[theme].crossfade.min >= 3, `full-loop crossfade contrast on ${theme}: ${state.composite[theme].crossfade.min}`);
      assert.ok(state.composite[theme].peak.min >= 3, `full-loop peak contrast on ${theme}: ${state.composite[theme].peak.min}`);
      // S1: the wrapped violet ends are visible at the frame-0 extremes (reference: ~252 left top, ~255 right tip).
      const hue = state.hue[theme];
      const inBand = (value, low, high) => value !== null && value >= low && value <= high;
      assert.ok(inBand(hue.leftTop, 235, 295), `violet left top on ${theme}: ${hue.leftTop}`);
      assert.ok(inBand(hue.rightTip, 235, 295), `violet right tip on ${theme}: ${hue.rightTip}`);
      const mod360 = (value) => ((value % 360) + 360) % 360;
      const inWrap = (value, low, high) => value !== null && (mod360(value - low) <= mod360(high - low));
      assert.ok(inWrap(hue.leftMiddle, 300, 30), `magenta/red left middle on ${theme}: ${hue.leftMiddle}`);
      assert.ok(inWrap(hue.leftBottom, 330, 70), `orange/amber left bottom on ${theme}: ${hue.leftBottom}`);
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
    // Rendered crossfade frames at the 2→3 handoff midpoint, at 22 px and 240 px, in both themes.
    // r37CompositeAlphas returns exactly two values [outgoing, incoming]; the exporter destructures them
    // and asserts the effective computed opacity of both layers before export (review-a F6: reading
    // [2]/[3] yielded undefined and the "midpoint" PNGs showed a fully opaque top frame).
    const midpointExport = await page.evaluate(({ themes, sizes }) => {
      const host = document.createElement('div');
      host.id = 'crossfade-frames';
      host.style = 'position:fixed;left:0;top:0;z-index:99999;display:flex;gap:24px;padding:16px;background:#333';
      document.body.append(host); // attached first, so the layers have computed opacity when asserted
      const exported = {};
      for (const theme of themes) {
        const [alphaA, alphaB] = (window).r37CompositeAlphas(theme);
        if (![alphaA, alphaB].every((value) => typeof value === 'number' && value >= 0 && value <= 1)) throw new Error(`midpoint alphas on ${theme} are not [opaque, half]: ${JSON.stringify([alphaA, alphaB])}`);
        if (Math.abs(alphaA - 1) > 1e-3 || Math.abs(alphaB - 0.5) > 1e-3) throw new Error(`midpoint alphas on ${theme} are not the handoff pair [1, 0.5]: ${JSON.stringify([alphaA, alphaB])}`);
        for (const size of sizes) {
          const box = document.createElement('div');
          box.id = `crossfade-${theme}-${size}`;
          box.style = `position:relative;width:${size}px;height:${size}px;background:${theme === 'dark' ? '#181B1A' : '#ffffff'}`;
          host.append(box); // attached before the layers, so computed opacity is available
          const layerOpacity = [];
          for (const [frame, alpha] of [[2, alphaA], [3, alphaB]]) {
            const layer = document.createElement('div');
            layer.style = `position:absolute;inset:0;opacity:${alpha}`;
            layer.innerHTML = (window).r37FrameMarkup(theme, frame);
            box.append(layer);
            layerOpacity.push(getComputedStyle(layer).opacity);
          }
          // The effective computed opacities must match the interpolation pair before any export.
          if (layerOpacity[0] !== String(alphaA) || Number(layerOpacity[1]) < 0.499 || Number(layerOpacity[1]) > 0.501) throw new Error(`layer opacity mismatch on ${theme}: ${JSON.stringify(layerOpacity)}`);
          exported[theme + '-' + size] = { alphas: [alphaA, alphaB], layerOpacity };
        }
      }
      return exported;
    }, { themes: Object.keys(THEMES), sizes: [22, 240] });
    report.midpointExport = midpointExport;
    for (const theme of Object.keys(THEMES)) for (const size of [22, 240]) {
      await page.locator(`#crossfade-${theme}-${size}`).screenshot({ path: path.join(output, `crossfade-${theme}-${size}.png`) });
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
