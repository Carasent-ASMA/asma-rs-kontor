import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { Icon, Modal } from "@getpaseo/plugin/client/ui";
import { findSvgIconProblem } from "@getpaseo/plugin";
import { Pressable, ScrollView, Text, View } from "react-native";
import type { Colors } from "./page-controls";
import type { TokenomicsQuery, TokenomicsTable } from "../shared/tokenomics";
import { reportMoney, reportNumber, reportValue } from "./tokenomics-format";

type Cell = string | number | null;
type Flag = NonNullable<TokenomicsTable["rows"][number]["highlight"]>;
const ChartInteraction = createContext<{ flag: Flag | null; pinned: Flag | null; pin: (flag: Flag | null) => void; hover: (flag: Flag | null) => void; inspect: () => void }>({ flag: null, pinned: null, pin: () => {}, hover: () => {}, inspect: () => {} });
export const useTokenomicsChartInteraction = () => useContext(ChartInteraction);
export function TokenomicsChartInteractions({ children, onInspect, resetKey }: { children: ReactNode; onInspect: () => void; resetKey: string }) {
  const [pinned, setPinned] = useState<Flag | null>(null), [hovered, setHovered] = useState<Flag | null>(null);
  useEffect(() => { setPinned(null); setHovered(null); }, [resetKey]);
  return <ChartInteraction.Provider value={{ flag: hovered ?? pinned, pinned, pin: setPinned, hover: setHovered, inspect: onInspect }}>{children}</ChartInteraction.Provider>;
}
type Hit = { x: number; y: number; width: number; height: number; label: string; values: string[]; toggle?: string; open?: boolean };
export type ChartScene = { width: number; height: number; parts: string[]; hits: Hit[]; legend: { label: string; color: string; line?: boolean; opacity?: number }[] };
const esc = (s: unknown) => String(s).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&apos;");
const n = (x: number) => { if (!Number.isFinite(x)) throw new Error("Chart coordinate is not finite"); return Math.round(x * 1000) / 1000; };
const shape = (tag: "rect" | "path" | "line" | "circle", attrs: Record<string, string | number>) => `<${tag} ${Object.entries(attrs).map(([k, v]) => `${k}="${typeof v === "number" ? n(v) : esc(v)}"`).join(" ")}/>`;
const text = (x: number, y: number, value: unknown, color: string, anchor = "start", size = 11, weight = 400) => `<text x="${n(x)}" y="${n(y)}" fill="${esc(color)}" font-size="${size}" font-family="system-ui" font-weight="${weight}" text-anchor="${anchor}">${esc(value)}</text>`;
const nice = (x: number) => { const p = 10 ** Math.floor(Math.log10(x || 1)); return ([1, 2, 2.5, 5, 10].find((v) => v * p >= x) ?? 10) * p; };
const topPath = (x: number, y: number, w: number, h: number, r: number) => `M${n(x)},${n(y+h)}V${n(y+r)}Q${n(x)},${n(y)} ${n(x+r)},${n(y)}H${n(x+w-r)}Q${n(x+w)},${n(y)} ${n(x+w)},${n(y+r)}V${n(y+h)}Z`;
const daysBetween = (from: string, to: string, step = 1) => { const a: string[] = []; for (let t = Date.parse(from), end = Date.parse(to); t <= end && a.length <= 10000; t += step * 86400000) a.push(new Date(t).toISOString().slice(0, 10)); return a; };
const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
export function chartColors(colors: Colors) {
  const dark = parseInt(colors.surface0.slice(1,3),16) < 128;
  return { muted: dark ? "#8f8e86" : "#7a7974", series: dark ? ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"] : ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#4a3aa7", "#e34948"], other: dark ? "#5d5c57" : "#b9b8b2", idle: dark ? "#222220" : "#efeee9", todo: dark ? "#44433f" : "#d4d3cd", seq: dark ? ["#184f95", "#256abf", "#3987e5", "#6da7ec", "#b7d3f6"] : ["#b7d3f6", "#6da7ec", "#2a78d6", "#1c5cab", "#0d366b"] };
}
// The host sizes SVG icons square. A nested viewport keeps chart units at 1:1; only unused square space is clipped.
export function chartSvgLayers(scene: ChartScene): string[] {
  const size = Math.max(scene.width, scene.height), start = `<svg xmlns="http://www.w3.org/2000/svg" width="${n(size)}" height="${n(size)}" viewBox="0 0 ${n(size)} ${n(size)}"><svg width="${n(scene.width)}" height="${n(scene.height)}" viewBox="0 0 ${n(scene.width)} ${n(scene.height)}">`, end = "</svg></svg>";
  const layers: string[] = []; let totalBytes = 0, content = "", bytes = new TextEncoder().encode(start + end).length;
  for (const part of scene.parts) {
    const length = new TextEncoder().encode(part).length; totalBytes += length;
    if (totalBytes > 32 * 1024 * 1024) throw new Error("Chart composition exceeds its 32 MiB limit; narrow the selection. No series was omitted.");
    if (length + new TextEncoder().encode(start + end).length > 65536) throw new Error("One chart path exceeds the host SVG bound; no series was omitted.");
    if (bytes + length > 64000 && content) { layers.push(start + content + end); content = ""; bytes = new TextEncoder().encode(start + end).length; }
    content += part; bytes += length;
  }
  layers.push(start + content + end);
  if (layers.reduce((bytes, svg) => bytes + new TextEncoder().encode(svg).length, 0) > 32 * 1024 * 1024) throw new Error("Chart composition exceeds its 32 MiB limit; narrow the selection. No series was omitted.");
  for (const svg of layers) { const problem = findSvgIconProblem(svg); if (problem) throw new Error(`Host refused chart SVG: ${problem}`); }
  return layers;
}
function stacked(table: TokenomicsTable, width: number, colors: Colors, query: TokenomicsQuery): ChartScene {
  const rows = table.plot?.rows ?? [], cp = chartColors(colors), review = table.id === "review-trend", normalized = review && query.page === "cr-fix", delivery = table.id === "delivery-trend" || table.id === "bug-trend";
  const height = table.id === "all-chart" ? 320 : review ? normalized ? 240 : 280 : table.id === "delivery-trend" ? 260 : table.id === "bug-trend" ? 220 : 300;
  const ml = delivery || review ? 48 : 64, mr = 12, mt = table.id === "all-chart" ? 16 : 12, mb = delivery || review ? 32 : table.id === "all-chart" ? 40 : 44;
  const scene: ChartScene = { width, height, parts: [], hits: [], legend: [] };
  if (!rows.length) return scene;
  const totals = new Map<string, number>(), by = new Map<string, Map<string, number>>();
  const outcome = ["Fixed", "Rejected", "Merged unfixed", "No answer", "Open"];
  for (const row of rows) {
    const entries: [string, number][] = review ? outcome.map((k, i) => [k, Number(row[i+1]) || 0]) : [[String(row[1]), Number(row[2]) || 0]];
    const day = String(row[0]), group = by.get(day) ?? new Map<string, number>(); by.set(day, group);
    for (const [k, v] of entries) { group.set(k,(group.get(k) ?? 0)+v); totals.set(k,(totals.get(k) ?? 0)+v); }
  }
  const bugKinds = ["traced", "fix adds lines only", "fix only touches tests, docs or generated files", "changed lines are newer than the report", "no fix found"];
  const ranked = review ? outcome : table.id === "bug-trend" ? bugKinds : [...totals].sort((a,b)=>b[1]-a[1]).map(([k])=>k), shown = review ? ranked : (table.plot?.order ?? ranked).slice(0,7), tail = ranked.filter((k)=>!shown.includes(k));
  const series = [...shown,...(tail.length?["Other"]:[])];
  if(tail.length)for(const group of by.values()){group.set("Other",sum(tail.map(k=>group.get(k)??0)));}
  const color = (key: string) => table.id === "bug-trend" ? [cp.series[7], cp.series[3], cp.series[6], cp.series[4], cp.other][bugKinds.indexOf(key)]! : review ? ["#0ca30c",cp.series[7]!,cp.series[1]!,cp.other,cp.series[0]!][outcome.indexOf(key)]! : key === "Other" ? cp.other : cp.series[(table.id === "delivery-trend" ? [0,1,2,3,4,6,7][shown.indexOf(key)] : shown.indexOf(key)) ?? 0]!;
  const observed = [...by.keys()].sort(), weekly = table.columns[0] === "Week", dates = table.id === "trend" ? observed : daysBetween(observed[0]!, observed.at(-1)!,weekly?7:1), values = dates.map(d=>sum(series.map(k=>by.get(d)?.get(k)??0))), step = normalized ? 0.25 : review || delivery ? Math.max(1,Math.round(nice(Math.max(...values,1)/4))) : nice(Math.max(...values,1)/4), top = normalized ? 1 : Math.ceil(Math.max(...values,1)/step)*step;
  const y=(v:number)=>mt+(height-mt-mb)*(1-v/top), bw=(width-ml-mr)/Math.max(1,dates.length), barW=Math.max(2,Math.min(table.id === "all-chart" ? 40 : 36,bw-4));
  for(let v=0;v<=top+1e-9;v+=step)scene.parts.push(shape("line",{x1:ml,x2:width-mr,y1:y(v),y2:y(v),stroke:colors.border}),text(ml-6,y(v)+4,normalized?`${Math.round(v*100)}%`:review||delivery?String(v):table.id === "trend" && query.measure === "act" ? `${v.toFixed(0)}h` : reportValue(v,table.columns.at(-1)),cp.muted,"end"));
  const every=Math.ceil(dates.length/Math.max(1,Math.floor((width-ml)/(table.id === "all-chart" ? 60 : 70))));
  dates.forEach((day,i)=>{let acc=0;const group=by.get(day), present=series.filter(k=>(group?.get(k)??0)>0), x=ml+i*bw+(bw-barW)/2;
    present.forEach((key,j)=>{const raw=group!.get(key)!,v=normalized?raw/(values[i]||1):raw,y0=y(acc),y1=y(acc+v);acc+=v;const h=Math.max(0,y0-y1-(j<present.length-1?review||delivery?1.5:2:0));scene.parts.push(shape("path",{d:topPath(x,y1,barW,h,j===present.length-1?Math.min(4,h/2,barW/2):0),fill:color(key)}));});
    if(i%every===0)scene.parts.push(text(x+barW/2,height-mb+16,`${weekly && table.id !== "all-chart" ? "wk " : ""}${day.slice(5)}`,cp.muted,"middle"));
    scene.hits.push({x:ml+i*bw,y:mt,width:bw,height:height-mt-mb,label:day,values:[...present.map(k=>`${k}: ${reportValue(group!.get(k)!,review||delivery?"Count":table.columns.at(-1))}`), ...(review ? [`${values[i]} findings · fix rate ${values[i]! - (group?.get("Open") ?? 0) ? (100 * (group?.get("Fixed") ?? 0) / (values[i]! - (group?.get("Open") ?? 0))).toFixed(0) + "%" : "—"}`] : [])]});
  });
  if (table.id === "all-chart" && table.plot?.events?.[0]?.[0]) { const start = String(table.plot.events[0][0]), bucket = weekly ? new Date(Date.parse(start) - (new Date(start).getUTCDay() + 6) % 7 * 86400000).toISOString().slice(0, 10) : start, index = dates.indexOf(bucket); if (index >= 0) { const x = ml + index * bw; scene.parts.push(shape("line", { x1: x, x2: x, y1: mt, y2: height - mb, stroke: colors.foreground, "stroke-dasharray": "4 3" }), text(x + 4, mt + 10, `orchestrated work begins ${start}`, colors.foreground)); } }
  scene.legend=series.filter(k=>review ? (totals.get(k)??0)>0 : totals.has(k)||k==="Other").map(label=>({label: table.id === "delivery-trend" && label === "Other" ? "Others" : table.id === "bug-trend" ? ["Introducing commit found", "Fix only adds lines", "Fix outside code", "Changed lines newer than the report", "No fix PR or commit names it"][bugKinds.indexOf(label)] ?? label : label,color:color(label)}));return scene;
}
function parallel(table: TokenomicsTable, width: number, colors: Colors): ChartScene {
  const rows=table.plot?.rows??[],height=340,ml=58,mr=58,mt=28,mb=40,cp=chartColors(colors),scene:ChartScene={width,height,parts:[],hits:[],legend:[{label:"Active seat-hours (left axis)",color:cp.series[0]!},{label:"Peak parallel seats (right axis)",color:cp.series[1]!,line:true},{label:"Idle day",color:cp.idle}]};
  if(!rows.length)return scene;
  const active = rows.filter((r) => r[3] !== "yes").length;
  scene.legend.push({ label: `${rows[0]![0]} → ${rows.at(-1)![0]}: ${rows.length} days, ${active} active, ${rows.length - active} idle · max ${Math.max(...rows.map((r) => Number(r[2])))} seats in parallel`, color: "transparent" });
  const sh=nice(Math.max(...rows.map(r=>Number(r[1])),1)/4),sp=Math.max(1,Math.ceil(nice(Math.max(...rows.map(r=>Number(r[2])),1)/4))),topH=Math.ceil(Math.max(...rows.map(r=>Number(r[1])),1)/sh)*sh,topP=Math.ceil(Math.max(...rows.map(r=>Number(r[2])),1)/sp)*sp;
  const yh=(v:number)=>mt+(height-mt-mb)*(1-v/topH),yp=(v:number)=>mt+(height-mt-mb)*(1-v/topP),cw=(width-ml-mr)/rows.length,bw=Math.max(2,Math.min(16,cw-3));
  rows.forEach((r,i)=>{if(r[3]==="yes")scene.parts.push(shape("rect",{x:ml+i*cw,y:mt,width:cw,height:height-mt-mb,fill:cp.idle}));});
  for(let v=0;v<=topH+1e-9;v+=sh)scene.parts.push(shape("line",{x1:ml,x2:width-mr,y1:yh(v),y2:yh(v),stroke:colors.border}),text(ml-6,yh(v)+4,`${v.toFixed(v<10&&sh<1?1:0)} h`,cp.muted,"end"));
  for(let v=0;v<=topP;v+=sp)scene.parts.push(text(width-mr+6,yp(v)+4,v,cp.muted));
  scene.parts.push(text(4,mt-14,"▮ Active seat-hours (left)",cp.muted),text(width-4,mt-14,"Peak parallel seats (right) —",cp.muted,"end"));
  rows.forEach((r,i)=>{const value=Number(r[1]),x=ml+i*cw;if(value>0){const h=yh(0)-yh(value);scene.parts.push(shape("path",{d:topPath(x+(cw-bw)/2,yh(value),bw,h,Math.min(4,h/2,bw/2)),fill:cp.series[0]!}));}if(new Date(String(r[0])).getUTCDay()===1)scene.parts.push(text(x+cw/2,height-mb+16,String(r[0]).slice(5),cp.muted,"middle"));scene.hits.push({x,y:mt,width:cw,height:height-mt-mb,label:String(r[0]),values:[`${reportValue(value,"Active seat-hours")} seat-time`,`${r[2]} peak parallel seats`,r[3]==="yes"?"Idle — no recorded model activity":"Recorded activity"]});});
  scene.parts.push(shape("path",{d:"M"+rows.map((r,i)=>`${n(ml+(i+0.5)*cw)},${n(yp(Number(r[2])))}`).join("L"),fill:"none",stroke:cp.series[1]!,"stroke-width":2,"stroke-linejoin":"round"}));
  rows.forEach((r,i)=>{if(Number(r[2]))scene.parts.push(shape("circle",{cx:ml+(i+0.5)*cw,cy:yp(Number(r[2])),r:4,fill:cp.series[1]!,stroke:colors.surface1,"stroke-width":2}));});return scene;
}
export function activityChartValue(rows: Cell[][], metric: TokenomicsQuery["metric"]): number | null {
  const total = (i: number) => sum(rows.map((r) => Number(r[i]) || 0));
  switch (metric) {
    case "act": return total(6); case "cost": return total(7); case "tok": return total(8); case "peak": return Math.max(0, ...rows.map((r) => Number(r[9]) || 0));
    case "cph": return total(6) >= 1 / 6 ? total(7) / total(6) : null;
    case "hit": return total(13) ? 100 * total(14) / total(13) : null;
    case "prs": return total(10); case "cm": return total(11); case "lines": return total(12); case "cpp": return total(10) ? total(7) / total(10) : null;
  }
}
export function activityChartSummary(rows: Cell[][], metric: TokenomicsQuery["metric"], end: string, flags: Cell[][], since: string) {
  const recent = new Date(Date.parse(end) - 6 * 86400000).toISOString().slice(0, 10), previous = new Date(Date.parse(end) - 13 * 86400000).toISOString().slice(0, 10);
  const current = activityChartValue(rows.filter((r) => String(r[4]) >= recent && String(r[4]) <= end), metric), prior = activityChartValue(rows.filter((r) => String(r[4]) >= previous && String(r[4]) < recent), metric);
  const signed = (n: number, suffix: string, digits = 0) => Math.abs(n) < (digits ? .05 : .5) ? `±0${suffix}` : `${n > 0 ? "+" : "−"}${Math.abs(n).toFixed(digits)}${suffix}`;
  const delta = current == null || prior == null ? "—" : metric === "hit" ? signed(current - prior, " pp", 1) : metric === "peak" ? signed(current - prior, "") : !prior ? current ? "new" : "—" : signed(100 * (current - prior) / prior, "%");
  const episodes = new Map<string, { type: string; to: string }[]>();
  for (const f of [...flags].sort((a, b) => String(a[3]).localeCompare(String(b[3])))) {
    const type = String(f[0]); if (!["Spike", "Efficiency drop"].includes(type)) continue;
    const key = JSON.stringify([type, f[1], f[5]]), list = episodes.get(key) ?? [], last = list.at(-1); episodes.set(key, list);
    if (last && Date.parse(String(f[3])) - Date.parse(last.to) <= 2 * 86400000) last.to = String(f[4]); else list.push({ type, to: String(f[4]) });
  }
  const counts = ["Spike", "Efficiency drop", "Spend without progress", "Overdue"].map((type) => ({ type, count: ["Spike", "Efficiency drop"].includes(type) ? [...episodes.values()].flat().filter((e) => e.type === type && e.to >= since).length : flags.filter((f) => f[0] === type && String(f[4]) >= since).length }));
  return { current, prior, delta, flags: counts.filter((f) => f.count) };
}
function timeline(table: TokenomicsTable, width: number, colors: Colors, query: TokenomicsQuery): ChartScene {
  const data = table.plot?.rows ?? [], cp = chartColors(colors), gaps = table.plot?.gaps ?? [], spans = table.plot?.spans ?? [], flags = table.plot?.flags ?? [], events = table.plot?.events ?? [];
  const personRows = (p: string) => data.filter((r) => r[0] === "Person" && r[1] === p);
  const people = [...new Set([...data.filter((r) => r[0] === "Person").map((r) => String(r[1])), ...gaps.map((r) => String(r[0]))])].sort((a, b) => sum(personRows(b).map((r) => Number(r[6]))) - sum(personRows(a).map((r) => Number(r[6]))) || a.localeCompare(b));
  const personColor = (p: string) => [cp.series[0], cp.series[2], cp.series[4], cp.series[5]][(table.plot?.order ?? [...people].sort()).indexOf(p)] ?? cp.other;
  const narrow = width < 700, labelW = Math.round(Math.max(104, Math.min(300, width * (narrow ? 0.34 : 0.21)))), gx1 = width - (narrow ? 0 : 262) - 6;
  const observed = data.map((r) => String(r[4])).filter(Boolean).sort(), end = table.plot?.end ?? observed.at(-1) ?? "";
  const first = [...observed, ...spans.map((r) => r[2]).filter((v): v is string => typeof v === "string" && !!v)].sort()[0] ?? end;
  const range = query.range === "auto" ? narrow ? "30" : "all" : query.range;
  const since = range === "all" ? first : new Date(Date.parse(end) + (1 - Number(range)) * 86400000).toISOString().slice(0, 10);
  const days = first ? daysBetween(first < since ? since : first, end) : [], cw = (gx1 - labelW) / Math.max(1, days.length), xi = new Map(days.map((d, i) => [d, i]));
  type Line = { label: string; data: Cell[][]; depth: number; person: string; toggle?: string; open?: boolean; heading?: boolean; height: number };
  const lines: Line[] = [], closed = (key: string, defaultOpen: boolean) => query.expanded.includes(`closed:${key}`) || (!query.expanded.includes(key) && !defaultOpen);
  for (const person of people) {
    const key = `tl:p:${person}`, open = !closed(key, people.length === 1 && !query.expanded.includes("tl:closed"));
    lines.push({ label: person, data: personRows(person), depth: 0, person, toggle: key, open, height: 36 });
    if (!open) continue;
    const all = data.filter((r) => r[0] === "Epic / desk row" && r[1] === person);
    for (const gap of gaps.filter((r) => r[0] === person)) if (spans.some((r) => r[1] === gap[2]) && !all.some((r) => r[2] === gap[1] && r[3] === gap[2])) all.push(["Epic / desk row", person, gap[1]!, gap[2]!, gap[3]!, null, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    const knownOrder = table.plot?.epicOrder ?? [];
    all.sort((a, b) => { const x = knownOrder.indexOf(String(a[3])), y = knownOrder.indexOf(String(b[3])); return x < 0 && y < 0 ? 0 : x < 0 ? 1 : y < 0 ? -1 : x - y; });
    for (const ws of [...new Set([...(table.plot?.workstreams ?? []), ...all.map((r) => String(r[2]))])].filter((ws) => all.some((r) => r[2] === ws))) {
      const ww = all.filter((r) => r[2] === ws), key = `tl:w:${person}:${ws}`, epics = [...new Set(ww.map((r) => String(r[3])))];
      const open = !closed(key, !(table.plot?.desks ?? []).includes(ws) || epics.length <= 8);
      const group = data.filter((r) => r[0] === "Workstream" && r[1] === person && r[2] === ws);
      lines.push({ label: open ? ws : `${ws} · ${epics.length} rows`, data: group, depth: 1, person, toggle: key, open, heading: open, height: open ? 22 : 28 });
      if (open) { for (const epic of epics) lines.push({ label: epic, data: ww.filter((r) => r[3] === epic), depth: 2, person, height: 28 }); lines.push({ label: "", data: [], depth: 3, person, heading: true, height: 4 }); }
    }
  }
  const scene: ChartScene = { width, height: 102 + sum(lines.map((r) => r.height)), parts: [], hits: [], legend: [] };
  if (!people.length) return { width, height: 40, parts: [text(8, 24, "No recorded activity for the current filters.", colors.foregroundMuted)], hits: [], legend: [] };
  if (!days.length) return scene;
  const x = (d: string) => labelW + (xi.get(d < days[0]! ? days[0]! : d > end ? end : d) ?? 0) * cw;
  const metric = (rows: Cell[][]) => activityChartValue(rows, query.metric);
  const fmt = (v: number | null) => v == null ? "—" : query.metric === "hit" ? `${v.toFixed(1)}%` : query.metric === "act" ? `${v.toFixed(1)} h` : ["cost", "cph", "cpp"].includes(query.metric) ? reportMoney(v) + (query.metric === "cph" ? "/h" : "") : reportNumber(v);
  for (const day of days) if (new Date(day).getUTCDay() === 1) {
    scene.parts.push(shape("line", { x1: x(day), x2: x(day), y1: 18, y2: scene.height, stroke: colors.border }));
    if (cw * 7 >= 34 && x(day) + 32 <= width) scene.parts.push(text(x(day) + 2, 14, day.slice(5), colors.foregroundMuted));
  }
  if (!narrow) scene.parts.push(text(gx1 + 80, 14, "Last 7 d", colors.foregroundMuted, "end", 11, 600), text(gx1 + 152, 14, "vs prior 7 d", colors.foregroundMuted, "end", 11, 600), text(width - 4, 14, "Flags", colors.foregroundMuted, "end", 11, 600));
  const drawSummary = (cy: number, rows: Cell[][], rowFlags: Cell[][], strong: boolean) => {
    if (narrow) return;
    const result = activityChartSummary(rows, query.metric, end, rowFlags, days[0]!);
    scene.parts.push(text(gx1 + 80, cy + 4, fmt(result.current), strong ? colors.foreground : colors.foregroundMuted, "end", 11, strong ? 600 : 400), text(gx1 + 152, cy + 4, result.delta, colors.foregroundMuted, "end"));
    if (!result.flags.length) { scene.parts.push(text(width - 4, cy + 4, "—", colors.foregroundMuted, "end")); return; }
    const kinds = ["Spike", "Efficiency drop", "Spend without progress", "Overdue"], glyphs = ["▲", "◆", "▬", "▼"];
    let right = width - 4;
    for (const flag of [...result.flags].reverse()) { const index = kinds.indexOf(flag.type), value = `${glyphs[index]}${flag.count}`; scene.parts.push(text(right, cy + 4, value, cp.series[[7, 3, 1, 6][index]!]!, "end", 11, 650)); right -= value.length * 6.5 + 8; }
  };
  const team = data.filter((r) => r[0] === "Team total" && xi.has(String(r[4]))), max = Math.max(...team.map((r) => Number(r[5]) || 0), 1), min = query.metric === "hit" ? Math.floor(Math.min(...team.map((r) => Number(r[5]) || 100), 100) / 5) * 5 : 0;
  scene.parts.push(text(8, 42, "Team total", colors.foreground, "start", 12.5, 650), text(8, 58, `${people.length} ${people.length === 1 ? "person" : "people"}${!narrow ? ` · top day ${fmt(max)}` : ""}`, colors.foregroundMuted), text(8, 88, narrow ? "✓ resolved" : "✓ Jira issues resolved", "#0ca30c"), shape("line", { x1: labelW, x2: gx1, y1: 74.5, y2: 74.5, stroke: colors.border }));
  const additive = ["act", "cost", "tok", "prs", "cm", "lines"].includes(query.metric);
  for (const r of team) {
    const day = String(r[4]); let yy = 74;
    const entries = additive ? people.map((p) => ({ label: p, value: Number(personRows(p).find((r) => r[4] === day)?.[5]) || 0, color: personColor(p) })) : [{ label: "Team", value: Number(r[5]) || 0, color: cp.series[0]! }];
    for (const p of entries) { if (p.value <= 0) continue; const h = additive ? 42 * p.value / max : Math.max(1, 42 * (p.value - min) / Math.max(1, max - min)), inset = Math.min(1, cw * 0.15); scene.parts.push(shape("rect", { x: x(day) + inset, y: yy - h, width: Math.max(1, cw - 2 * inset), height: h, fill: p.color })); yy -= h; }
    const resolved = events.filter((e) => e[1] === day).length;
    if (resolved) scene.parts.push(text(x(day) + cw / 2, 88, "✓", "#0ca30c", "middle", 10, 700));
    scene.hits.push({ x: x(day), y: 24, width: cw, height: 74, label: day, values: [...entries.map((p) => `${p.label}: ${fmt(p.value)}`), `Jira issues resolved: ${resolved}`] });
  }
  drawSummary(50, data.filter((r) => r[0] === "Team total"), flags, true);
  scene.parts.push(shape("line", { x1: 0, x2: width, y1: 98.5, y2: 98.5, stroke: colors.border }));
  const vals = data.filter((r) => ["Person", "Epic / desk row"].includes(String(r[0])) && xi.has(String(r[4]))).map((r) => Number(r[5])).filter((v) => v > 0).sort((a, b) => a - b);
  const bins = [.2, .4, .6, .8].map((q) => Number((vals[Math.min(vals.length - 1, Math.floor(q * vals.length))] ?? 0).toPrecision(2))).filter((v, i, a) => v > 0 && (!i || v > a[i - 1]!));
  let yy = 98;
  for (const line of lines) {
    const cy = yy + line.height / 2, inset = line.depth === 0 ? 36 : line.depth === 1 ? 28 : 36, maxChars = Math.max(1, Math.floor((labelW - inset - 6) / (line.depth === 1 ? 6.8 : line.depth === 2 ? 6.3 : 7.4))), label = line.label.length > maxChars ? line.label.slice(0, maxChars - 1) + "…" : line.label;
    if (line.label) scene.parts.push(text(inset, cy + 4, label, line.depth ? colors.foregroundMuted : colors.foreground, "start", line.depth === 2 ? 11 : line.depth === 1 ? 11.5 : 12.5, line.depth === 1 ? 600 : line.depth === 0 ? 650 : 400));
    if (line.toggle) { scene.parts.push(text(line.depth ? 16 : 6, cy + 5, line.open ? "▾" : "▸", colors.foregroundMuted, "start", 13)); scene.hits.push({ x: 0, y: yy, width: labelW - 6, height: line.height, label: line.label, values: ["Open or close workstreams and epics"], toggle: line.toggle, open: line.open }); }
    if (!line.depth) scene.parts.push(shape("circle", { cx: 25, cy, r: 5, fill: personColor(line.person) }));
    if (!line.heading) {
      const by = new Map<string, Cell[][]>(); for (const r of line.data) { const day = String(r[4]), values = by.get(day) ?? []; values.push(r); by.set(day, values); }
      const span = line.depth === 2 ? spans.find((r) => r[1] === line.label) : undefined, recorded = line.data.filter((r) => Number(r[15]) > 0).map((r) => String(r[4])).sort(), start = [span?.[2], recorded[0]].filter((d): d is string => typeof d === "string" && !!d).sort()[0], stop = span?.[6] ? [String(span[6]), recorded.at(-1) ?? ""].sort().at(-1)! : end;
      const relevantFlags = flags.filter((f) => line.depth === 0 ? f[1] === line.person : line.depth === 2 ? line.label.startsWith(String(f[2]) + " ·") : false);
      if (line.depth !== 1 && start && stop >= days[0]!) scene.parts.push(shape("line", { x1: x(start) + cw / 2, x2: x(stop) + cw / 2, y1: cy, y2: cy, stroke: cp.todo, "stroke-width": 2 }));
      for (const flag of relevantFlags.filter((f) => ["Spend without progress", "Overdue"].includes(String(f[0])))) { if (String(flag[4]) < days[0]!) continue; scene.parts.push(shape("rect", { x: x(String(flag[9] || flag[3])), y: cy - 10, width: Math.max(cw, x(String(flag[4])) + cw - x(String(flag[9] || flag[3]))), height: 20, rx: 4, fill: cp.series[flag[0] === "Overdue" ? 6 : 1]!, opacity: flag[0] === "Overdue" ? .22 : .28 })); }
      for (const day of days) {
        const rows = by.get(day) ?? [], value = rows.length && rows.some((r) => r[5] != null) ? metric(rows) : null, dayFlags = relevantFlags.filter((f) => String(f[3]) <= day && String(f[4]) >= day), spike = dayFlags.some((f) => f[0] === "Spike"), efficiency = dayFlags.some((f) => f[0] === "Efficiency drop"), mx = x(day) + cw / 2;
        if (value != null) { let i = 0; while (i < bins.length && value >= bins[i]!) i++; const h = line.depth === 0 ? 16 : line.depth === 1 ? 12 : 14; scene.parts.push(shape("rect", { x: x(day) + 1, y: cy - h / 2, width: Math.max(1, cw - 2), height: h, rx: Math.min(3, cw / 3), fill: cp.seq[Math.round(i * 4 / Math.max(1, bins.length))]!, ...(spike || efficiency ? { stroke: cp.series[spike ? 7 : 3]!, "stroke-width": 2 } : {}) })); }
        if (spike) scene.parts.push(shape("path", { d: `M${n(mx-4)},${n(cy-10)}L${n(mx+4)},${n(cy-10)}L${n(mx)},${n(cy-17)}Z`, fill: cp.series[7]! }));
        if (efficiency) scene.parts.push(shape("path", { d: `M${n(mx)},${n(cy+10)}L${n(mx+3.5)},${n(cy+13.5)}L${n(mx)},${n(cy+17)}L${n(mx-3.5)},${n(cy+13.5)}Z`, fill: cp.series[3]! }));
        const missing = line.depth === 2 ? gaps.filter((g) => g[0] === line.person && g[2] === line.label && g[3] === day) : [];
        if (missing.length) scene.parts.push(shape("path", { d: `M${n(mx)},${n(cy-15)}L${n(mx+4)},${n(cy-11)}L${n(mx)},${n(cy-7)}L${n(mx-4)},${n(cy-11)}Z`, fill: colors.surface1, stroke: colors.foregroundMuted, "stroke-width": 1.2 }));
        const resolved = line.depth === 2 ? events.filter((e) => e[0] === line.label && e[1] === day).length : 0;
        if (resolved) scene.parts.push(text(mx, cy + 14, "✓", "#0ca30c", "middle", 10, 700));
        scene.hits.push({ x: x(day), y: yy, width: cw, height: line.height, label: `${day} · ${line.label}`, values: [value == null ? "Idle — no recorded activity" : fmt(value), ...rows.map((r) => `${reportValue(r[6], "Seat-hours")} · ${reportMoney(Number(r[7]))} · ${reportNumber(Number(r[8]))} tokens`), ...dayFlags.map((f) => `${f[0]}: ${f[5]} ${reportValue(f[6], String(f[5]))}`), ...missing.map((g) => `Missing evidence: ${g[4]}`), ...(resolved ? [`${resolved} Jira issues resolved`] : [])] });
      }
      if (line.depth === 2 && span) {
        if (start && xi.has(start)) scene.parts.push(shape("circle", { cx: x(start) + cw / 2, cy, r: 4, fill: colors.foregroundMuted }));
        if (span[6] && xi.has(String(span[6]))) scene.parts.push(shape("rect", { x: x(String(span[6])) + cw / 2 - 4, y: cy - 4, width: 8, height: 8, fill: "#0ca30c" }));
        if (span[7] && xi.has(String(span[7]))) { const dx = x(String(span[7])) + cw / 2; scene.parts.push(shape("line", { x1: dx, x2: dx, y1: cy - 9, y2: cy + 11, stroke: cp.series[6]!, "stroke-width": 1.5, "stroke-dasharray": "2 2" }), shape("path", { d: `M${n(dx-4.5)},${n(cy-15)}L${n(dx+4.5)},${n(cy-15)}L${n(dx)},${n(cy-8)}Z`, fill: cp.series[6]! })); }
      }
      if (!narrow) drawSummary(cy, line.data, relevantFlags, line.depth === 0);

    }
    yy += line.height;
    if (lines[lines.indexOf(line) + 1]?.depth === 0 || line === lines.at(-1)) scene.parts.push(shape("line", { x1: 0, x2: width, y1: yy + .5, y2: yy + .5, stroke: colors.border }));
  }
  scene.legend = [{ label: `Cells — ${table.columns[5]} per day:`, color: "transparent" }, ...bins.flatMap((v, i) => [...(!i ? [{ label: `< ${fmt(v)}`, color: cp.seq[0]! }] : []), { label: i === bins.length - 1 ? `≥ ${fmt(v)}` : `${fmt(v)}–${fmt(bins[i + 1]!)}`, color: cp.seq[Math.round((i + 1) * 4 / Math.max(1, bins.length))]! }]), ...(additive ? [{ label: "Team bars:", color: "transparent" }, ...people.map((label) => ({ label, color: personColor(label) }))] : []), ...["▲ spike", "◆ efficiency drop", "▬ spend without progress", "▼ Jira due date", "overdue (> due + 10 d)"].map((label, i) => ({ label, color: cp.series[[7, 3, 1, 6, 6][i]!]! })), { label: "● created  ■ resolved  ✓ child issue resolved  ◇ seat launched, tokens unrecoverable  —— idle day", color: "transparent" }];
  return scene;
}
export function roleShareColor(index: number): string {
  if (index < 0 || index >= 12) return "#b9b8b2";
  const hue = (Math.round(index * 137.508) % 360) / 60, c = (1 - Math.abs(2 * .46 - 1)) * .62, x = c * (1 - Math.abs(hue % 2 - 1)), m = .46 - c / 2;
  const rgb = hue < 1 ? [c, x, 0] : hue < 2 ? [x, c, 0] : hue < 3 ? [0, c, x] : hue < 4 ? [0, x, c] : hue < 5 ? [x, 0, c] : [c, 0, x];
  return `#${rgb.map((v) => Math.round((v + m) * 255).toString(16).padStart(2, "0")).join("")}`;
}
export function tokenomicsChartScene(table:TokenomicsTable,width:number,colors:Colors,query:TokenomicsQuery):ChartScene{
  if(table.id==="parallel")return parallel(table,width,colors);
  if(table.id==="timeline")return timeline(table,width,colors,query);
  if(table.id==="share"){
    const rows = [...(table.plot?.rows ?? [])].sort((a, b) => Number(b[1]) - Number(a[1])), total = sum(rows.map((r) => Number(r[1]))), scene: ChartScene = { width, height: 26, parts: [], hits: [], legend: [] };
    const tail = rows.slice(12), shown = [...rows.slice(0, 12), ...(tail.length ? [[`Other (${tail.length})`, sum(tail.map((r) => Number(r[1]))), "", sum(tail.map((r) => Number(r[3]))), sum(tail.map((r) => Number(r[4])))]] : [])]; let x = 0;
    shown.forEach((r, i) => { const w = total ? width * Number(r[1]) / total : 0, color = i === 12 ? chartColors(colors).other : roleShareColor(i); scene.parts.push(shape("rect", { x, y: 0, width: Math.max(0, w - 1), height: 26, fill: color })); scene.hits.push({ x, y: 0, width: w, height: 26, label: String(r[0]), values: [`${total ? (100 * Number(r[1]) / total).toFixed(1) : "0"}% of consumed tokens`, `${reportNumber(Number(r[1]))} tokens · ${reportMoney(Number(r[3]))} · ${Number(r[4]).toLocaleString("en-US")} responses`] }); x += w; }); return scene;
  }
  return stacked(table,width,colors,query);
}
export const activityEveryoneKeys = (table: TokenomicsTable) => (table.plot?.order ?? [...new Set(table.plot?.rows.filter((r) => r[0] === "Person").map((r) => String(r[1]))) ?? []]).map((p) => `tl:p:${p}`);
export function flagHighlightFrames(scene: ChartScene, flag: Flag): { x: number; y: number; width: number; height: number }[] {
  const days = scene.hits.filter((h) => /^\d{4}-\d{2}-\d{2}$/.test(h.label)).sort((a, b) => a.label.localeCompare(b.label));
  if (!days.length) return [];
  const clip = (day: string) => day < days[0]!.label ? days[0]! : day > days.at(-1)!.label ? days.at(-1)! : days.find((h) => h.label === day)!;
  if (flag.to < days[0]!.label || flag.from > days.at(-1)!.label) return [];
  const first = clip(flag.from), last = clip(flag.to), x = Math.max(0, first.x);
  return [{ x, y: 18, width: Math.min(scene.width, last.x + last.width) - x, height: Math.max(0, scene.height - 22) }];
}
export function chartDefaultPoint(points: Hit[]): Hit | undefined {
  const latest = points.map((h) => h.label.slice(0, 10)).filter((d) => /^\d{4}-\d{2}-\d{2}$/.test(d)).sort().at(-1);
  return points.find((h) => h.label === latest) ?? points.find((h) => latest && h.label.startsWith(latest)) ?? points.at(-1);
}
export function TokenomicsChart({ table, colors, query, change, expanded = false }: { table: TokenomicsTable; colors: Colors; query: TokenomicsQuery; change: (q: Partial<TokenomicsQuery>) => void; expanded?: boolean }) {
  const [width, setWidth] = useState(0), [selected, setSelected] = useState<Hit | null>(null), [full, setFull] = useState(false), [legend, setLegend] = useState(false), [valuesOpen, setValuesOpen] = useState(false);
  const compact = width > 0 && width < 700 && !expanded, interaction = useTokenomicsChartInteraction();
  const built = useMemo(() => { if (width < 100) return null; try { const scene = tokenomicsChartScene(table, width, colors, query); return { scene, layers: chartSvgLayers(scene), error: null }; } catch (error) { return { error: error instanceof Error ? error.message : "Chart unavailable", scene: null, layers: [] }; } }, [table, width, colors, query]);
  const scene = built?.scene, points = scene?.hits.filter((h) => !h.toggle) ?? [], current = selected ?? chartDefaultPoint(points), index = current ? points.indexOf(current) : -1;
  useEffect(() => { setSelected(null); setValuesOpen(false); }, [scene]);
  const select = (x: number, y: number, press = false) => { const hit = scene?.hits.find((h) => x >= h.x && x < h.x + h.width && y >= h.y && y < h.y + h.height); if (press && hit?.toggle) { change({ expanded: [...query.expanded.filter((k) => k !== hit.toggle && k !== `closed:${hit.toggle}`), ...(hit.open ? [`closed:${hit.toggle}`] : [hit.toggle]), "tl:closed"] }); setSelected(null); } else setSelected(hit ?? null); };
  const action = (label: string, onPress: () => void, disabled = false) => <Pressable accessibilityRole="button" accessibilityLabel={label} disabled={disabled} accessibilityState={{ disabled }} onPress={onPress} style={{ minHeight: 44, minWidth: 44, padding: 10, borderWidth: 1, borderColor: colors.border, borderRadius: 6, justifyContent: "center", opacity: disabled ? 0.4 : 1 }}><Text style={{ color: colors.accent, fontSize: 14 }}>{label}</Text></Pressable>;
  return <View testID={`report-chart-${table.id}`} onLayout={(e) => setWidth(Math.floor(e.nativeEvent.layout.width))}>
    {built?.error ? <Text accessibilityRole="alert" style={{ color: colors.foreground, fontSize: 13 }}>{built.error} The complete values remain available in Chart data and CSV.</Text> : scene ? <>
      <View style={{ width: scene.width, height: scene.height, overflow: "hidden" }}>
        {built.layers.map((svg, i) => <View key={i} pointerEvents="none" accessible={false} style={{ position: "absolute", left: 0, top: 0 }}><Icon icon={{ svg }} size={Math.max(scene.width, scene.height)} color={colors.foreground} /></View>)}
        {interaction.flag && table.id === "timeline" ? flagHighlightFrames(scene, interaction.flag).map((r, i) => <View key={i} testID="report-flag-highlight" pointerEvents="none" style={{ position: "absolute", left: r.x, top: r.y, width: r.width, height: r.height, borderWidth: 0, borderColor: chartColors(colors).series[interaction.flag!.kind === "Spike" ? 7 : interaction.flag!.kind === "Efficiency drop" ? 3 : interaction.flag!.kind === "Overdue" ? 6 : 1], backgroundColor: chartColors(colors).series[interaction.flag!.kind === "Spike" ? 7 : interaction.flag!.kind === "Efficiency drop" ? 3 : interaction.flag!.kind === "Overdue" ? 6 : 1] + "29" }} />) : null}
        {selected ? <View pointerEvents="none" style={{ position: "absolute", left: selected.x, top: selected.y, width: Math.max(1, selected.width), height: selected.height, borderWidth: 1, borderColor: colors.accent }} /> : null}
        <Pressable accessibilityRole="button" accessibilityLabel={`Inspect ${table.title}. Use previous and next point or Chart data for exact values.`} onPress={(e) => select(e.nativeEvent.locationX, e.nativeEvent.locationY, true)} onPointerMove={(e) => { if (!compact) select(e.nativeEvent.offsetX, e.nativeEvent.offsetY); }} style={{ position: "absolute", left: 0, top: 0, width: scene.width, height: scene.height }} />
        {selected && !compact ? <View testID="report-chart-tooltip" style={{ position: "absolute", left: Math.max(0, Math.min(scene.width - 280, selected.x + 14)), top: Math.max(0, Math.min(scene.height - 90, selected.y + 12)), maxWidth: 280, padding: 9, borderWidth: 1, borderRadius: 8, borderColor: colors.border, backgroundColor: colors.surface1, gap: 3 }}><Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{selected.label}</Text>{selected.values.map((v, i) => <Text key={i} style={{ color: colors.foreground, fontSize: 12 }}>{v}</Text>)}{action("Close inspection", () => setSelected(null))}</View> : null}
      </View>
      <View style={{ flexDirection: "row", flexWrap: "wrap", columnGap: compact ? 10 : 14, rowGap: compact ? 4 : 6, marginTop: 8 }}>
        {!compact || legend ? scene.legend.map((s, i) => <View key={`${s.label}-${i}`} style={{ flexDirection: "row", alignItems: "center", gap: 5, maxWidth: "100%", flexShrink: 1 }}>{s.color === "transparent" ? null : /^[▲◆▼]/.test(s.label) ? <Text style={{ color: s.color, fontSize: 12 }}>{s.label[0]}</Text> : <View style={{ width: 10, height: s.line ? 2 : 10, borderRadius: s.line ? 0 : 2, opacity: s.opacity ?? 1, backgroundColor: s.color }} />}<Text style={{ fontSize: compact ? 13 : 12, color: colors.foregroundMuted, flexShrink: 1 }}>{/^[▲◆▼]/.test(s.label) ? s.label.slice(2) : s.label}</Text></View>) : null}
      </View>
      {compact || selected ? <View style={{ flexDirection: "row", alignItems: "center", gap: 4, marginTop: 6 }}><Pressable accessibilityRole="button" accessibilityLabel={`${current?.label ?? "No data"}: ${current?.values.join("; ") ?? ""}. Show all values`} accessibilityState={{ expanded: valuesOpen }} onPress={() => setValuesOpen(!valuesOpen)} style={{ flex: 1, minHeight: 44, justifyContent: "center" }}><Text style={{ fontSize: 13, lineHeight: 19, color: colors.foreground, fontWeight: "600" }}>{current?.label ?? "No recorded data"}</Text><Text numberOfLines={2} style={{ fontSize: 13, lineHeight: 19, color: colors.foregroundMuted }}>{current?.values.join(" · ")}</Text></Pressable>{[["Previous point", -1], ["Next point", 1]].map(([label, step]) => <Pressable key={String(label)} accessibilityRole="button" accessibilityLabel={String(label)} disabled={Number(step) < 0 ? index <= 0 : index >= points.length - 1} onPress={() => setSelected(points[Math.max(0, Math.min(points.length - 1, index + Number(step)))] ?? null)} style={{ minHeight: 44, minWidth: 44, alignItems: "center", justifyContent: "center", borderWidth: 1, borderColor: colors.border, borderRadius: 6 }}><Icon name={Number(step) < 0 ? "ChevronLeft" : "ChevronRight"} size={18} color={colors.foreground} /></Pressable>)}</View> : null}
      {valuesOpen ? <View style={{ padding: 12, borderWidth: 1, borderColor: colors.border, borderRadius: 8, gap: 6 }}>{current?.values.map((v, i) => <Text key={i} style={{ color: colors.foreground, fontSize: 14, lineHeight: 21 }}>{v}</Text>)}{action("Close values", () => setValuesOpen(false))}</View> : null}
      {interaction.pinned && table.id === "timeline" ? <Text accessibilityLiveRegion="polite" style={{ fontSize: 14, lineHeight: 21, color: colors.foreground }}>{`${interaction.pinned.kind}: ${interaction.pinned.from} – ${interaction.pinned.to} · ${interaction.pinned.epic || interaction.pinned.person}`}</Text> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8, marginTop: compact ? 4 : 0 }}>{!compact && !selected ? action("Inspect values", () => setSelected(chartDefaultPoint(points) ?? null)) : null}{compact ? action(legend ? "Hide legend" : "Legend", () => setLegend(!legend)) : null}{!expanded ? action("Expand chart", () => setFull(true)) : null}{interaction.pinned && table.id === "timeline" ? action("Clear signal highlight", () => interaction.pin(null)) : null}</View>
    </> : null}
    {!expanded ? <Modal title={table.title} open={full} onOpenChange={setFull}><Modal.Content contentContainerStyle={{ padding: 12, gap: 8 }}><ScrollView horizontal><View style={{ width: Math.max(1000, width), paddingBottom: 8 }}><TokenomicsChart table={table} colors={colors} query={query} change={change} expanded /></View></ScrollView>{action("Close expanded chart", () => setFull(false))}</Modal.Content></Modal> : null}
  </View>;
}
