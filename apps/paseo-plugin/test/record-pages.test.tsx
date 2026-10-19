import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, () => {}] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
type Props = Parameters<typeof ReadOnlyPage>[0];
type Tab = "Delivery" | "Decisions";
type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  return "";
}
function recordRead(tab: Tab, url?: string, status: SourceRead["status"] = "ready"): SourceRead {
  const kind = tab === "Delivery" ? "delivery" : "decisions";
  return { source: { id: kind, title: `Configured ${kind}`, kind, path: `/configured/${kind}.json`, projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status, revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: null, graph: null, diagnostics: status === "ready" ? [] : ["Exact source read incomplete"], records: { teams: [],
    decisions: kind === "decisions" ? [{ id: "decision-1", title: "Recorded decision", state: "open", owner: "Declared owner", detail: "Recorded rationale", url }] : [],
    delivery: kind === "delivery" ? [{ id: "evidence-1", title: "Recorded delivery", kind: "check", state: "captured state", recordedAt: "2026-10-18T17:00:00Z", url }] : [],
  } };
}
function render(tab: Tab, reads: readonly SourceRead[], extra: Partial<Props> = {}) {
  const open = vi.fn();
  const props: Props = { tab, colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: reads.map((read) => read.source), reads: Object.fromEntries(reads.map((read) => [read.source.id, read])), sourceId: null, sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: 0, onScroll: vi.fn(), initialDocumentOffset: 0, onDocumentScroll: vi.fn(), onExternalLink: open, ...extra };
  const nodes = elements(ReadOnlyPage(props));
  const visible = nodes.filter((node) => node.type === "Text").map((node) => text(node.props.children as ReactNode)).join("\n");
  const links = nodes.filter((node) => node.type === "Pressable" && /^Open (decision source|evidence):/.test(String(node.props.accessibilityLabel)));
  return { nodes, visible, links, open };
}
describe("read-only recorded Delivery and Decisions", () => {
  it.each(["Delivery", "Decisions"] as const)("keeps configured %s records/read time visible and activates only the explicit evidence link", (tab) => {
    const url = "https://example.com/record?capture=historical#source";
    const read = recordRead(tab, url); const page = render(tab, [read]);
    expect(page.visible).toContain(tab === "Delivery" ? "Recorded delivery" : "Recorded decision");
    expect(page.visible).toContain(tab === "Delivery" ? "recorded state: captured state" : "Declared owner");
    expect(page.visible).toContain("ready"); expect(page.visible).toContain(`Read ${read.readAt}`);
    expect(page.visible).toContain(`Governed ${tab === "Delivery" ? "delivery" : "decision"} actions: kontord is not enabled.`);
    expect(page.visible).not.toContain("requires kontord"); expect(page.visible).not.toContain(read.source.path);
    expect(page.links).toHaveLength(1); expect(page.links[0]!.props.disabled).toBe(false);
    expect(page.links[0]!.props.accessibilityLabel).toBe(tab === "Delivery" ? "Open evidence: Recorded delivery" : "Open decision source: Recorded decision");
    (page.links[0]!.props.onPress as () => void)(); expect(page.open).toHaveBeenCalledWith(url);
  });
  it.each(["Delivery", "Decisions"] as const)("keeps empty-source %s honest without blocking the read-only page", (tab) => {
    const page = render(tab, []);
    expect(page.visible).toContain("not connected to a configured");
    expect(page.visible).toContain("actions: kontord is not enabled"); expect(page.links).toHaveLength(0);
    expect(page.visible).not.toContain("requires kontord");
  });
  it.each(["Delivery", "Decisions"] as const)("distinguishes a successfully captured empty %s source from unavailable data", (tab) => {
    const read = recordRead(tab); read.records = { teams: [], decisions: [], delivery: [] };
    const page = render(tab, [read]);
    expect(page.visible).toContain("No records in this source."); expect(page.visible).toContain("ready");
    expect(page.visible).not.toContain("not connected to a configured"); expect(page.links).toHaveLength(0);
  });
  it.each([undefined, "http://example.com", "javascript:alert(1)", "https://user:secret@example.com", "https://example..com", "https:/malformed", "https://example.com:99999"])("retains records while refusing absent/unsafe evidence URLs: %s", (url) => {
    for (const tab of ["Delivery", "Decisions"] as const) {
      const page = render(tab, [recordRead(tab, url)]);
      expect(page.visible).toContain(tab === "Delivery" ? "Recorded delivery" : "Recorded decision");
      expect(page.links).toHaveLength(0); expect(page.open).not.toHaveBeenCalled();
      if (url) expect(page.visible).toContain("Source link unavailable");
    }
  });
  it.each(["partial", "unavailable"] as const)("keeps retained records, status and request errors distinct when a source is %s", (status) => {
    for (const tab of ["Delivery", "Decisions"] as const) {
      const page = render(tab, [recordRead(tab, "https://example.com/evidence", status)], { sourceError: "Configured source request failed; retained read." });
      expect(page.visible).toContain(status); expect(page.visible).toContain("Source unavailable or partial");
      expect(page.visible).toContain("Configured source request failed; retained read.");
      expect(page.visible).toContain(tab === "Delivery" ? "Recorded delivery" : "Recorded decision");
      expect(page.visible).not.toContain("No records in this source.");
    }
  });
  it.each(["Delivery", "Decisions"] as const)("does not present an unavailable %s read as an empty capture", (tab) => {
    const read = recordRead(tab, undefined, "unavailable"); read.records = null;
    const page = render(tab, [read]); expect(page.visible).toContain("unavailable");
    expect(page.visible).not.toContain("No records in this source."); expect(page.links).toHaveLength(0);
  });
  it("keeps a standalone decision visible and reports when link opening is unavailable", () => {
    const page = render("Decisions", [recordRead("Decisions", "https://example.com/decision")], { onExternalLink: undefined });
    expect(page.visible).toContain("Recorded decision"); expect(page.visible).toContain("Source link opening is unavailable");
    expect(page.links).toHaveLength(1); expect(page.links[0]!.props.disabled).toBe(true);
    (page.links[0]!.props.onPress as () => void)(); expect(page.open).not.toHaveBeenCalled();
  });
});
