import { ScrollView, Text, View } from "react-native";
import { Disclosure, type Colors } from "./page-controls";
export type PlanBlock = { line: number; kind: "heading" | "paragraph" | "list" | "table" | "code"; text: string; level?: number; rows?: string[][]; frontMatter?: boolean };
const heading = /^(#{1,6})\s+(.+)$/;
const list = /^\s*(?:[-*+]|\d+[.)])\s+/;
function cells(line: string): string[] {
  const value = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  const result: string[] = []; let cell = "";
  for (let index = 0; index < value.length; index++) {
    if (value[index] === "\\" && value[index + 1] === "|") { cell += "|"; index++; }
    else if (value[index] === "|") { result.push(cell.trim()); cell = ""; }
    else cell += value[index];
  }
  result.push(cell.trim()); return result;
}
/** Presentation of Markdown blocks only; the original content remains the Source view. */
export function planBlocks(content: string): PlanBlock[] {
  const lines = content.split("\n"); const blocks: PlanBlock[] = [];
  // YAML front matter stays one exact, monospace block instead of loose paragraphs.
  const closing = lines[0]?.trim() === "---" ? lines.findIndex((value, index) => index > 0 && value.trim() === "---") : -1;
  if (closing > 0) blocks.push({ line: 0, kind: "code", text: lines.slice(1, closing).join("\n"), frontMatter: true });
  for (let line = closing > 0 ? closing + 1 : 0; line < lines.length; line++) {
    const value = lines[line]!; if (!value.trim()) continue;
    const title = value.match(heading);
    if (title) { blocks.push({ line, kind: "heading", text: title[2]!, level: title[1]!.length }); continue; }
    if (/^\s*(```|~~~)/.test(value)) {
      const start = line; const fence = value.trim().slice(0, 3); const body: string[] = [];
      while (++line < lines.length && !lines[line]!.trim().startsWith(fence)) body.push(lines[line]!);
      blocks.push({ line: start, kind: "code", text: body.join("\n") }); continue;
    }
    if (value.includes("|") && lines[line + 1] && cells(lines[line + 1]!).every((cell) => /^:?-{3,}:?$/.test(cell))) {
      const start = line; const rows = [cells(value)]; line++;
      while (lines[line + 1]?.includes("|") && lines[line + 1]!.trim()) rows.push(cells(lines[++line]!));
      blocks.push({ line: start, kind: "table", text: rows.map((row) => row.join(" | ")).join("\n"), rows }); continue;
    }
    const kind = list.test(value) ? "list" : "paragraph"; const start = line; const body = [value];
    while (lines[line + 1]?.trim() && !heading.test(lines[line + 1]!) && !/^\s*(```|~~~)/.test(lines[line + 1]!) && list.test(lines[line + 1]!) === (kind === "list") && !lines[line + 1]!.includes("|")) body.push(lines[++line]!);
    blocks.push({ line: start, kind, text: body.join("\n") });
  }
  return blocks;
}
/**
 * Line of a task's declaration in this exact source: the first table row whose first cell is the id,
 * else a heading naming it, else its node in the plan-graph block. No fuzzy or title matching.
 */
export function taskDeclaration(content: string, taskId: string): { line: number; kind: "table" | "heading" | "graph" } | null {
  const id = taskId.trim(); if (!id) return null;
  const lines = content.split("\n");
  const token = new RegExp(`(^|[^\\w-])${id.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}([^\\w-]|$)`);
  const row = lines.findIndex((value) => value.trim().startsWith("|") && cells(value)[0] === id);
  if (row >= 0) return { line: row, kind: "table" };
  const title = lines.findIndex((value) => { const match = value.match(heading); return !!match && token.test(match[2]!); });
  if (title >= 0) return { line: title, kind: "heading" };
  let graph = false;
  for (let line = 0; line < lines.length; line++) {
    const value = lines[line]!.trim();
    if (/^(```|~~~)/.test(value)) { graph = !graph && /^(```|~~~)\s*plan-graph\b/.test(value); continue; }
    if (graph && value.replace(/\s+/g, "").startsWith(`"id":${JSON.stringify(id)}`)) return { line, kind: "graph" };
  }
  return null;
}
export function headingAnchor(text: string): string { return text.toLocaleLowerCase().replace(/[^\w\u00c0-\uffff\s-]/g, "").trim().replace(/\s+/g, "-"); }
function Inline({ text, colors, onLink }: { text: string; colors: Colors; onLink: (target: string) => void }) {
  const pattern = /\[([^\]\n]+)\]\(([^\s()]+)\)|\*\*([^*]+)\*\*|`([^`]+)`/g;
  const pieces = []; let cursor = 0;
  for (const match of text.matchAll(pattern)) {
    pieces.push(text.slice(cursor, match.index));
    if (match[1] && match[2]) { const target = match[2]; pieces.push(<Text key={match.index} accessibilityRole="link" accessibilityLabel={`${match[1]}: ${target}`} onPress={() => onLink(target)} style={{ color: colors.accent, textDecorationLine: "underline" }}>{match[1]}</Text>); }
    else pieces.push(<Text key={match.index} style={match[3] ? { fontWeight: "700" } : { fontFamily: "monospace" }}>{match[3] ?? match[4]}</Text>);
    cursor = match.index + match[0].length;
  }
  pieces.push(text.slice(cursor)); return <>{pieces}</>;
}
/** Table row index of a source line inside a table block: header, separator, then one row per line. */
export function tableRowAt(block: PlanBlock, line: number): number | null {
  const row = line - block.line - 1;
  return block.kind === "table" && row >= 1 && row < (block.rows?.length ?? 0) ? row : null;
}
/** Content-sized columns, like the prototype's auto table layout: id columns stay narrow and prose wraps less. */
export function tableColumnWidths(rows: readonly (readonly string[])[]): number[] {
  const count = Math.max(0, ...rows.map((row) => row.length));
  return Array.from({ length: count }, (_, column) => Math.min(360, Math.max(64, 16 + 7 * Math.max(0, ...rows.map((row) => row[column]?.length ?? 0)))));
}
export function FormattedPlanBlock({ block, colors, query, onLink, targetRow = null, onTargetLayout }: { block: PlanBlock; colors: Colors; query: string; onLink: (target: string) => void; targetRow?: number | null; onTargetLayout?: (y: number) => void }) {
  const highlight = query.trim() && block.text.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
  const style = { color: colors.foreground, lineHeight: 22, fontSize: 14, backgroundColor: highlight ? colors.surface2 : undefined };
  if (block.kind === "table") { const widths = tableColumnWidths(block.rows ?? []); return <ScrollView horizontal accessibilityLabel="Read-only plan table"><View>
    {block.rows?.map((row, index) => <View key={index} testID={index === targetRow ? "plan-target-row" : undefined} onLayout={index === targetRow ? (event) => onTargetLayout?.(event.nativeEvent.layout.y) : undefined}
      style={{ flexDirection: "row", ...(index === targetRow ? { backgroundColor: colors.surface2, borderLeftWidth: 3, borderColor: colors.accent } : {}) }}>{row.map((cell, column) => <View key={column} style={{ width: widths[column], paddingHorizontal: 8, paddingVertical: 6, borderBottomWidth: 1, borderColor: colors.border, backgroundColor: index === 0 ? colors.surface2 : undefined }}><Text selectable style={{ ...style, fontSize: 12, lineHeight: 17, fontWeight: index === 0 ? "700" : "400" }}><Inline text={cell} colors={colors} onLink={onLink} /></Text></View>)}</View>)}
  </View></ScrollView>; }
  if (block.kind === "list") return <View>{block.text.split("\n").map((item, index) => <Text key={index} selectable style={{ ...style, paddingLeft: 8, paddingVertical: 2 }}><Inline text={item} colors={colors} onLink={onLink} /></Text>)}</View>;
  const code = <Text selectable style={{ ...style, fontFamily: "monospace", padding: 8, backgroundColor: colors.surface2 }}>{block.text}</Text>;
  if (block.kind === "code" && block.frontMatter) return <Disclosure small label={`Front matter · ${block.text.split("\n").length} lines`} colors={colors}>{code}</Disclosure>;
  if (block.kind === "code") return code;
  return <Text selectable accessibilityRole={block.kind === "heading" ? "header" : "text"} style={{ ...style, fontSize: block.kind === "heading" ? Math.max(16, 25 - (block.level ?? 1) * 2) : 14, fontWeight: block.kind === "heading" ? "700" : "400" }}><Inline text={block.text} colors={colors} onLink={onLink} /></Text>;
}
