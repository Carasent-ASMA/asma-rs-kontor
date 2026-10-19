import { useMemo, useRef, useState } from "react";
import { ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import { AnchoredMenu, MenuItem } from "./anchored-menu";
import { SourceDetails } from "./source-details";
import { Action, Copy, type Colors } from "./page-controls";
import { FormattedPlanBlock, headingAnchor, planBlocks, tableRowAt, taskDeclaration } from "./plan-document";
import { readTime } from "./page-data";

/** Reader state kept per route: the mode, and a task declaration to reveal once in this exact revision. */
export type PlanView = { mode: "formatted" | "source"; target: { taskId: string; revision: string | null } | null; revealed: boolean };
export const DEFAULT_PLAN_VIEW: PlanView = { mode: "formatted", target: null, revealed: false };
type Props = {
  colors: Colors; compact?: boolean; sources: readonly ReadSourceSummary[]; reads: Readonly<Record<string, SourceRead>>; sourceId: string | null;
  entries: readonly DirectoryEntry[]; sourceError: string | null; query: string; setQuery: (query: string) => void;
  onChooseSource?: () => void; onDocumentLink?: (target: string) => void; onGraph?: () => void;
  /** Copies text to this client's clipboard; offered only when the host supplies it. */
  onCopy?: (text: string, what: string) => void;
  initialOffset: number; onScroll: (offset: number) => void; view?: PlanView; onView?: (view: PlanView) => void;
};
const short = (value: string | null) => value ? value.slice(0, 12) : "Unknown";

/**
 * One vertical scroller: intro, sticky reader controls, then the article. Contents open in a bounded
 * overlay so the sticky bar never changes height under the reader.
 */
export function PlanReader(props: Props) {
  const { colors } = props;
  const [view, setView] = useState<PlanView>(() => ({ ...DEFAULT_PLAN_VIEW, ...props.view }));
  const [contentsOpen, setContentsOpen] = useState(false);
  const [linkMessage, setLinkMessage] = useState<string | null>(null);
  const [rootSize, setRootSize] = useState({ width: 0, height: 0 });
  const rootRef = useRef<View | null>(null), contentsRef = useRef<View | null>(null), scroller = useRef<ScrollView | null>(null);
  const candidate = props.sourceId ? props.reads[props.sourceId] : undefined;
  const selected = candidate?.source.kind === "plan" ? candidate : undefined;
  const content = selected?.content ?? null;
  const blocks = useMemo(() => planBlocks(content ?? ""), [content]);
  const declaration = useMemo(() => view.target && content !== null ? taskDeclaration(content, view.target.taskId) : null, [content, view.target?.taskId]);
  const pendingTarget = useRef(view.target !== null && !view.revealed);
  const pendingHeading = useRef<number | null>(null);
  const position = useRef({ restoring: props.initialOffset > 0 && !pendingTarget.current, target: props.initialOffset, requested: null as number | null, height: null as number | null, viewport: null as number | null });
  const geometry = useRef({ article: null as number | null, bar: null as number | null, source: null as number | null, row: null as number | null, blocks: {} as Record<number, number> });
  const tableBlock = declaration?.kind === "table" ? blocks.find((block) => tableRowAt(block, declaration.line) !== null) : undefined;
  const formattedTarget = declaration?.kind === "heading" ? blocks.some((block) => block.kind === "heading" && block.line === declaration.line) : tableBlock !== undefined;
  // A declaration without a formatted row of its own (a plan-graph node) is revealed in Source.
  const mode = pendingTarget.current && declaration && !formattedTarget ? "source" : view.mode;
  const update = (patch: Partial<PlanView>) => { const next = { ...view, ...patch }; setView(next); props.onView?.(next); };
  const restore = () => {
    const state = position.current;
    if (!state.restoring || state.height === null || state.viewport === null) return;
    const target = Math.min(state.target, Math.max(0, state.height - state.viewport));
    if (state.requested === target) return;
    state.requested = target;
    scroller.current?.scrollTo({ y: target, animated: false });
  };
  const scrollTo = (y: number, animated: boolean) => {
    position.current.restoring = false;
    const offset = Math.max(0, Math.round(y));
    props.onScroll(offset);
    scroller.current?.scrollTo({ y: offset, animated });
  };
  const declarationY = () => {
    const shape = geometry.current;
    if (mode === "source") return shape.source;
    const top = declaration?.kind === "heading" ? shape.blocks[declaration.line] : tableBlock ? shape.blocks[tableBlock.line] : undefined;
    if (top === undefined) return null;
    if (declaration?.kind === "heading") return top;
    return shape.row === null ? null : top + 5 + shape.row;
  };
  const reveal = () => {
    const shape = geometry.current, y = declarationY();
    if (!pendingTarget.current || shape.article === null || shape.bar === null || y === null) return;
    pendingTarget.current = false;
    scrollTo(shape.article + y - shape.bar - 8, false);
    update({ revealed: true, mode });
  };
  const scrollToHeading = (line: number) => {
    const shape = geometry.current; const y = shape.blocks[line];
    if (y !== undefined) scrollTo((shape.article ?? 0) + y - (shape.bar ?? 0) - 6, true);
  };
  const navigateHeading = (line: number) => {
    setContentsOpen(false);
    if (mode === "source") { pendingHeading.current = line; update({ mode: "formatted" }); }
    else scrollToHeading(line);
  };
  const setMode = (next: PlanView["mode"]) => {
    if (next === mode) return;
    // Keep an inspected declaration in view across the switch.
    if (view.target && declaration) { pendingTarget.current = true; geometry.current.source = null; geometry.current.row = null; }
    update({ mode: next });
  };
  const needle = props.query.trim().toLocaleLowerCase();
  const headings = blocks.filter((block) => block.kind === "heading" && (block.level ?? 1) <= 3);
  const listed = headings.filter((heading) => !needle || heading.text.toLocaleLowerCase().includes(needle));
  const openLink = (target: string) => {
    if (target.startsWith("#")) {
      let anchor: string; try { anchor = decodeURIComponent(target.slice(1)); } catch { setLinkMessage("Invalid heading link."); return; }
      const match = blocks.find((block) => block.kind === "heading" && (headingAnchor(block.text) === anchor || block.text === anchor));
      if (match) navigateHeading(match.line); else setLinkMessage("This heading is not present in the exact source.");
    } else if (props.onDocumentLink) props.onDocumentLink(target);
    else setLinkMessage("Link navigation is unavailable on this host.");
  };
  const several = props.sources.filter((source) => source.kind === "plan").length > 1;
  // Empty state: one sentence and one action; the full explanation stays in its label.
  if (!selected) return <ScrollView contentContainerStyle={{ padding: 12, gap: 10 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 20, fontWeight: "600" }}>Plan</Text>
    {props.sourceError ? <Copy colors={colors}>{props.sourceError}</Copy> : null}
    <Text selectable testID="plan-empty" accessibilityLabel={`${several ? "Several explicitly linked plans. Choose an exact source." : "No plan linked. Select a configured plan in the top context picker."} Conversation prose and titles do not create links.`}
      style={{ color: colors.foreground, fontSize: 13, lineHeight: 20 }}>{several ? "Several plans are linked here." : "No plan linked."}</Text>
    {props.onChooseSource ? <View style={{ alignSelf: "flex-start" }}><Action label="Choose plan" colors={colors} onPress={props.onChooseSource} /></View> : null}
  </ScrollView>;
  const key = selected.graph?.epic.jira ?? selected.source.epic;
  const lines = content?.split("\n") ?? [];
  const matches = needle ? blocks.filter((block) => block.text.toLocaleLowerCase().includes(needle)).length : 0;
  const width = { maxWidth: 900, width: "100%" as const, alignSelf: "center" as const };
  const tasks = selected.graph ? `${selected.graph.nodes.length} plan tasks` : "task graph unavailable";
  const linked = `${props.entries.length} linked session${props.entries.length === 1 ? "" : "s"}`;
  const notInferred = "A captured document, not a live plan revision. Approval, execution and lifecycle are not inferred from its text.";
  const status = view.target ? declaration
    ? `${view.target.taskId} · declared at source line ${declaration.line + 1}${view.target.revision && view.target.revision !== selected.revision ? ` · linked from revision ${short(view.target.revision)}, now ${short(selected.revision)}` : ""}`
    : `${view.target.taskId} is not declared in this exact source revision (${short(selected.revision)}).` : null;
  return <View ref={rootRef} style={{ flex: 1 }} onLayout={(event) => { const { width: w, height } = event.nativeEvent.layout; if (w !== rootSize.width || height !== rootSize.height) setRootSize({ width: w, height }); }}>
    <ScrollView testID="plan-document" ref={scroller} stickyHeaderIndices={[1]} keyboardShouldPersistTaps="handled" contentContainerStyle={{ paddingHorizontal: 12, paddingBottom: 28 }}
      onLayout={(event) => { position.current.viewport = event.nativeEvent.layout.height; restore(); }}
      onContentSizeChange={(_width, height) => { position.current.height = height; restore(); }}
      onScrollBeginDrag={() => { position.current.restoring = false; }}
      onScroll={(event) => {
        const y = Math.max(0, event.nativeEvent.contentOffset.y);
        const state = position.current;
        if (state.restoring) {
          // Preserve a matching clamped restore for later measurement. A different
          // offset releases it: web wheel scrolling has no begin-drag callback.
          if (state.requested === null) return;
          if (Math.abs(y - state.requested) <= 1 && state.requested < state.target) return;
          state.restoring = false;
        }
        props.onScroll(y);
      }} scrollEventThrottle={100}>
      {/* One title and one meta line; the source kind, exact read time and the not-inferred statement are in Source details. */}
      <View style={{ ...width, gap: 4, paddingTop: 12, paddingBottom: 6 }}>
        {props.sourceError ? <Copy colors={colors}>{props.sourceError}</Copy> : null}
        <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: props.compact ? 19 : 22, fontWeight: "600" }}>{key ? `${key} · ${selected.source.title}` : selected.source.title}</Text>
        <SourceDetails read={selected} colors={colors}
          summary={<Text testID="plan-meta" accessibilityLabel={`Read-only ${selected.source.revisionKind} · ${tasks} · ${linked}. Read ${selected.readAt}. ${notInferred}`} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>
            {["Captured document", tasks, props.entries.length ? linked : null, `read ${readTime(selected.readAt)}`].filter(Boolean).join(" · ")}
          </Text>}
          notes={<>
            <Copy colors={colors} muted small>Read-only {selected.source.revisionKind} · {tasks} · {linked}</Copy>
            <Copy colors={colors} muted small>{notInferred}</Copy>
          </>} />
      </View>
      <View testID="plan-reader-bar" onLayout={(event) => { geometry.current.bar = event.nativeEvent.layout.height; reveal(); }} style={{ backgroundColor: colors.surface0, borderBottomWidth: 1, borderColor: colors.border, marginHorizontal: -12, paddingHorizontal: 12, paddingVertical: 6 }}>
        <View style={{ ...width, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
          <View style={{ flexDirection: "row", gap: 4 }}>
            <Action small={!props.compact} label="View" accessibilityLabel="Formatted view" colors={colors} selected={mode === "formatted"} onPress={() => setMode("formatted")} />
            <Action small={!props.compact} label="Source" accessibilityLabel="Exact Source" colors={colors} selected={mode === "source"} onPress={() => setMode("source")} />
          </View>
          <Action ref={contentsRef} small={!props.compact} label={`Contents (${listed.length})`} colors={colors} expanded={contentsOpen} disabled={!listed.length} onPress={() => setContentsOpen(!contentsOpen)} />
          <View style={{ flexGrow: 1, flexShrink: 1, flexBasis: 160, minWidth: 140, flexDirection: "row", alignItems: "center", gap: 6 }}>
            <TextInput accessibilityLabel="Search this plan" placeholder="Search this plan" placeholderTextColor={colors.foregroundMuted} value={props.query} onChangeText={(query) => { if (mode === "source") setMode("formatted"); props.setQuery(query); }}
              style={{ flex: 1, color: colors.foreground, fontSize: 12, minHeight: props.compact ? 44 : 30, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
            {needle ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{matches} match{matches === 1 ? "" : "es"}</Text> : null}
          </View>
          {props.onGraph ? <Action small={!props.compact} label="Dependency graph" colors={colors} onPress={props.onGraph} /> : null}
          {props.onCopy && content !== null ? <Action small={!props.compact} label="Copy Markdown" accessibilityLabel="Copy the exact Markdown source" colors={colors} onPress={() => props.onCopy!(content, "Plan Markdown")} /> : null}
        </View>
        {status ? <View style={{ ...width, flexDirection: "row", alignItems: "center", gap: 8, paddingTop: 4 }}>
          <Text accessibilityRole="text" testID="plan-target-status" style={{ flex: 1, color: declaration ? colors.foreground : colors.statusWarning, fontSize: 12 }}>{status}</Text>
          <Action small={!props.compact} label="Clear" accessibilityLabel={`Clear ${view.target!.taskId} highlight`} colors={colors} onPress={() => { pendingTarget.current = false; update({ target: null, revealed: false }); }} />
        </View> : null}
      </View>
      <View testID="plan-article" onLayout={(event) => { geometry.current.article = event.nativeEvent.layout.y; reveal(); }} style={{ ...width, paddingTop: 10 }}>
        {linkMessage ? <Copy colors={colors}>{linkMessage}</Copy> : null}
        {content === null ? <Copy colors={colors}>This configured plan is unavailable.</Copy> : mode === "source"
          ? declaration ? <>
            {declaration.line > 0 ? <Text selectable testID="plan-exact-source" style={sourceStyle(colors)}>{lines.slice(0, declaration.line).join("\n")}</Text> : null}
            <View testID="plan-source-target" onLayout={(event) => { geometry.current.source = event.nativeEvent.layout.y; reveal(); }} style={{ backgroundColor: colors.surface2, borderLeftWidth: 3, borderColor: colors.accent }}>
              <Text selectable accessibilityLabel={`${view.target!.taskId} declaration, source line ${declaration.line + 1}`} style={sourceStyle(colors)}>{lines[declaration.line]}</Text>
            </View>
            {declaration.line < lines.length - 1 ? <Text selectable style={sourceStyle(colors)}>{lines.slice(declaration.line + 1).join("\n")}</Text> : null}
          </> : <Text selectable testID="plan-exact-source" style={sourceStyle(colors)}>{content}</Text>
          : blocks.map((block) => {
            const row = declaration?.kind === "table" ? tableRowAt(block, declaration.line) : null;
            const target = declaration?.kind === "heading" && declaration.line === block.line;
            return <View key={block.line} testID={`plan-line-${block.line}`} onLayout={(event) => {
              geometry.current.blocks[block.line] = event.nativeEvent.layout.y;
              if (target || row !== null) reveal();
              if (pendingHeading.current === block.line) { pendingHeading.current = null; scrollToHeading(block.line); }
            }} style={{ paddingVertical: 5, ...(target ? { backgroundColor: colors.surface2, borderLeftWidth: 3, borderColor: colors.accent, paddingLeft: 6 } : {}) }}>
              <FormattedPlanBlock block={block} colors={colors} query={props.query} onLink={openLink} targetRow={row}
                onTargetLayout={row === null ? undefined : (y) => { geometry.current.row = y; reveal(); }} />
            </View>;
          })}
      </View>
    </ScrollView>
    {contentsOpen ? <AnchoredMenu testID="plan-contents-menu" anchor={contentsRef} root={rootRef} rootSize={rootSize} colors={colors} label="Plan contents" onClose={() => setContentsOpen(false)}
      placement={props.compact ? { align: "stretch", width: 0, maxHeight: Math.min(rootSize.height * 0.6, 420) } : { align: "start", width: 360, maxHeight: 420 }}>
      <ScrollView role="menu" accessibilityLabel="Plan headings" style={{ flexGrow: 0, flexShrink: 1 }}>
        {listed.map((heading) => <MenuItem key={heading.line} label={heading.text} indent={(heading.level ?? 1) - 1} tall={props.compact} colors={colors} onPress={() => navigateHeading(heading.line)} />)}
      </ScrollView>
    </AnchoredMenu> : null}
  </View>;
}
function sourceStyle(colors: Colors) {
  return { color: colors.foreground, fontFamily: "monospace", fontSize: 12, lineHeight: 20 };
}
