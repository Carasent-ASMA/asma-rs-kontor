import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ open: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useEffect: () => {}, useState: (initial: unknown) => [typeof initial === "boolean" && model.open ? true : initial, () => {}] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { ReadOnlyPage } from "../client/read-only-pages";
import { readTime } from "../client/page-data";
import { sessionEntry } from "./session-fixtures";
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
function render(tab: Props["tab"], reads: readonly SourceRead[], extra: Partial<Props> = {}) {
  const open = vi.fn();
  const props: Props = { tab, colors: {} as Props["colors"], entries: [], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: null, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: reads.map((read) => read.source), reads: Object.fromEntries(reads.map((read) => [read.source.id, read])), sourceId: null, sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: 0, onScroll: vi.fn(), onExternalLink: open, ...extra };
  const nodes = elements(ReadOnlyPage(props));
  const visible = nodes.filter((node) => node.type === "Text").map((node) => text(node.props.children as ReactNode)).join("\n");
  const links = nodes.filter((node) => node.type === "Pressable" && /^Open (decision source|evidence):/.test(String(node.props.accessibilityLabel)));
  const labels = nodes.flatMap((node) => typeof node.props.accessibilityLabel === "string" ? [node.props.accessibilityLabel] : []).join("\n");
  return { nodes, visible, links, open, labels };
}
describe("read-only recorded Delivery and Decisions", () => {
  it.each(["Delivery", "Decisions"] as const)("keeps configured %s records/read time visible and activates only the explicit evidence link", (tab) => {
    const url = "https://example.com/record?capture=historical#source";
    const read = recordRead(tab, url); const page = render(tab, [read]);
    expect(page.visible).toContain(tab === "Delivery" ? "Recorded delivery" : "Recorded decision");
    expect(page.visible).toContain(tab === "Delivery" ? "captured state · recorded 18 Oct" : "Declared owner");
    // One short meta line: the count and a short read time; the status, revision kind, exact read and page explanation move into labels and Source details.
    expect(page.visible).toContain(`1 record · read ${readTime(read.readAt)}`);
    expect(page.visible).not.toContain(read.readAt); expect(page.visible).not.toContain("recorded state:"); expect(page.visible).not.toContain("recorded open");
    expect(page.visible).not.toContain("kontord"); expect(page.visible).not.toContain("working copy");
    const meta = page.nodes.find((node) => node.props.testID === `record-meta-${read.source.id}`)!;
    expect(meta.props.accessibilityLabel).toBe(`1 record · ready · working copy · Read ${read.readAt}`);
    expect(page.labels).toContain(tab === "Delivery" ? "recorded state: captured state · recorded 2026-10-18" : "Owner: Declared owner · recorded open");
    model.open = true;
    const opened = render(tab, [read]).visible;
    model.open = false;
    expect(opened).toContain(`Governed ${tab === "Delivery" ? "delivery" : "decision"} actions: kontord is not enabled.`);
    expect(opened).toContain(tab === "Delivery" ? "States are as captured, not live; delivery and acceptance are not inferred." : "States are as captured, not live approval requests.");
    expect(opened).toContain(`Read ${read.readAt}`); expect(opened).toContain(read.source.path);
    expect(page.visible).not.toContain("requires kontord"); expect(page.visible).not.toContain(read.source.path);
    expect(page.links).toHaveLength(1); expect(page.links[0]!.props.disabled).toBe(false);
    expect(page.links[0]!.props.accessibilityLabel).toBe(tab === "Delivery" ? "Open evidence: Recorded delivery" : "Open decision source: Recorded decision");
    (page.links[0]!.props.onPress as () => void)(); expect(page.open).toHaveBeenCalledWith(url);
  });
  it.each(["Delivery", "Decisions"] as const)("keeps empty-source %s honest without blocking the read-only page", (tab) => {
    const page = render(tab, []);
    expect(page.visible).toContain(`${tab === "Delivery" ? "Delivery" : "Decision"} records not captured: no source connected.`);
    // The full statement is one tap away in About this page, visible once opened rather than only in a label.
    expect(page.visible).toContain("▸ About this page"); expect(page.links).toHaveLength(0);
    expect(page.visible).not.toContain("kontord"); expect(page.visible).not.toContain("requires kontord");
    model.open = true;
    const opened = render(tab, []);
    model.open = false;
    expect(opened.visible).toContain("not connected to a configured");
    expect(opened.visible).toContain("An empty page here means not captured, not that nothing exists.");
    expect(opened.visible).toContain("actions: kontord is not enabled");
  });
  it.each(["Delivery", "Decisions"] as const)("distinguishes a successfully captured empty %s source from unavailable data", (tab) => {
    const read = recordRead(tab); read.records = { teams: [], decisions: [], delivery: [] };
    const page = render(tab, [read]);
    expect(page.visible).toContain("No records in this source."); expect(page.visible).not.toContain("0 records");
    expect(page.labels).toContain("0 records · ready");
    expect(page.visible).not.toContain("records not captured"); expect(page.links).toHaveLength(0);
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
  it("shows a stamped source title exactly and lists each delivery record's exact state and recorded time in Source details", () => {
    const read = recordRead("Delivery");
    read.source = { ...read.source, title: "ASMA-8539 delivery — captured 2026-10-19T10:59:22.131040+00:00" };
    read.records!.delivery = [{ id: "check-1", title: "publication-branch-title", kind: "check", state: "Captured COMPLETED: SUCCESS; completed 2026-10-19T06:40:43Z.", recordedAt: "2026-10-19T10:59:22.131040+00:00" }];
    const page = render("Delivery", [read]);
    const header = page.nodes.find((node) => node.props.accessibilityRole === "header" && text(node.props.children as ReactNode) === read.source.title);
    expect(header).toBeDefined(); expect(header!.props.accessibilityLabel).toBeUndefined();
    expect(page.visible).not.toContain("captured 19 Oct");
    // The row keeps its short dates; the exact values are one tap away, not only in a label.
    expect(page.visible).toContain("Captured COMPLETED: SUCCESS; completed 19 Oct. · recorded 19 Oct");
    expect(page.visible).not.toContain("Exact recorded values"); expect(page.visible).not.toContain("06:40:43Z");
    model.open = true;
    const opened = render("Delivery", [read]);
    model.open = false;
    const exact = opened.nodes.find((node) => node.props.testID === `record-exact-${read.source.id}`)!;
    expect(elements(exact).filter((node) => node.type === "Text").map((node) => text(node.props.children as ReactNode))).toEqual([
      "Exact recorded values", "publication-branch-title · Captured COMPLETED: SUCCESS; completed 2026-10-19T06:40:43Z. · recorded 2026-10-19T10:59:22.131040+00:00"]);
    const unknown = recordRead("Delivery"); delete unknown.records!.delivery[0]!.recordedAt;
    model.open = true;
    const undated = render("Delivery", [unknown]).visible;
    expect(render("Decisions", [recordRead("Decisions")]).nodes.some((node) => String(node.props.testID).startsWith("record-exact-"))).toBe(false);
    model.open = false;
    expect(undated).toContain("Recorded delivery · captured state · recorded Unknown");
  });
  it("keeps a standalone decision visible and reports when link opening is unavailable", () => {
    const page = render("Decisions", [recordRead("Decisions", "https://example.com/decision")], { onExternalLink: undefined });
    expect(page.visible).toContain("Recorded decision"); expect(page.visible).toContain("Source link opening is unavailable");
    expect(page.links).toHaveLength(1); expect(page.links[0]!.props.disabled).toBe(true);
    (page.links[0]!.props.onPress as () => void)(); expect(page.open).not.toHaveBeenCalled();
  });
});

describe("primary surfaces keep moved statements in labels and Source details", () => {
  const planRead: SourceRead = { ...recordRead("Delivery"), source: { id: "plan", title: "Configured plan", kind: "plan", path: "/configured/plan.md", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, content: "# Plan", records: null };
  const byId = (page: ReturnType<typeof render>, id: string) => page.nodes.find((node) => node.props.testID === id)!;
  it("shows the plan title once and one meta line; the revision kind, exact read and not-inferred statement are in the label and Source details", () => {
    const page = render("Plan", [planRead], { sourceId: "plan" });
    expect(page.visible.split("Configured plan")).toHaveLength(2);
    expect(text(byId(page, "plan-meta").props.children as ReactNode)).toBe(`Captured document · Workstream: unknown · task graph unavailable · read ${readTime(planRead.readAt)}`);
    for (const moved of ["Read-only working copy", "not a live plan revision", planRead.readAt, "0 linked sessions"]) expect(page.visible).not.toContain(moved);
    expect(byId(page, "plan-meta").props.accessibilityLabel).toBe(`Read-only working copy · Workstream: unknown · task graph unavailable · 0 linked sessions. Read ${planRead.readAt}. A captured document, not a live plan revision. Approval, execution and lifecycle are not inferred from its text.`);
    model.open = true;
    const opened = render("Plan", [planRead], { sourceId: "plan" }).visible;
    model.open = false;
    expect(opened).toContain("A captured document, not a live plan revision. Approval, execution and lifecycle are not inferred from its text.");
    expect(opened).toContain("Read-only working copy · Workstream: unknown · task graph unavailable · 0 linked sessions");
    expect(opened).toContain(`Read ${planRead.readAt}`);
  });
  it("keeps the no-plan empty state to one sentence and one action, with the explanation in its label", () => {
    const page = render("Plan", [], { sourceId: null, onChooseSource: vi.fn() });
    expect(text(byId(page, "plan-empty").props.children as ReactNode)).toBe("No plan linked.");
    expect(byId(page, "plan-empty").props.accessibilityLabel).toBe("No plan linked. Select a configured plan in the top context picker. Conversation prose and titles do not create links.");
    expect(page.visible).not.toContain("Conversation prose");
  });
  it("shows only a non-zero session count on AI teams & sessions and keeps the observation statement in the header label", () => {
    const empty = render("AI teams & sessions", [], { contextLabel: "Context plan" });
    expect(empty.visible).toContain("No sessions in this context.");
    expect(empty.visible).not.toContain("0 native sessions"); expect(empty.visible).not.toContain("Context plan");
    const header = empty.nodes.find((node) => node.props.accessibilityRole === "header" && text(node.props.children as ReactNode) === "Sessions in this context")!;
    expect(header.props.accessibilityLabel).toBe("Native sessions in Context plan. Native sessions and labels are observations, not activated configured teams. Planned or governed assignment: Unknown.");
    const one = render("AI teams & sessions", [], { entries: [sessionEntry("a")] });
    expect(one.visible).toContain("1 session"); expect(one.visible).not.toContain("No sessions in this context.");
  });
  it("shows a short partial-coverage cue and short times on Activity, with the full statement and each exact update time inside the cue", () => {
    const entry = sessionEntry("a"); const older = sessionEntry("b", { updatedAt: "2026-10-17T08:15:30.250Z" });
    const page = render("Activity", [], { entries: [entry], directory: { entries: [entry], status: "ready", live: true, lastUpdatedAt: Date.parse("2026-10-19T10:00:00Z"), error: null, staleReason: null, pages: 1, includeArchived: false } });
    expect(page.visible).toContain("Latest update per session · not a full log");
    expect(page.visible).toContain(`${readTime(entry.updatedAt)} · ${entry.status}`);
    for (const moved of ["Partial coverage", "audit log", entry.updatedAt, "native "]) expect(page.visible).not.toContain(moved);
    model.open = true;
    const opened = render("Activity", [], { entries: [older, entry], directory: { entries: [older, entry], status: "ready", live: true, lastUpdatedAt: Date.parse("2026-10-19T10:00:00Z"), error: null, staleReason: null, pages: 1, includeArchived: false } });
    const cue = elements(byId(opened, "activity-coverage")).filter((node) => node.type === "Text").map((node) => text(node.props.children as ReactNode));
    model.open = false;
    expect(cue).toEqual([
      "▾ Latest update per session · not a full log", "Partial coverage: current session updatedAt values only, as of 2026-10-19T10:00:00.000Z. This is not a durable audit log or a transcript history.",
      `${entry.title} · updated ${entry.updatedAt}`, `${older.title} · updated 2026-10-17T08:15:30.250Z`]);
    expect(page.labels).toContain(`Open session ${entry.title}, updated ${entry.updatedAt}, native ${entry.status}`);
    expect(render("Activity", []).visible).toContain("No sessions in this context.");
  });
  it("moves the Inbox and Search intros behind About this page, visible once opened", () => {
    const inbox = render("Inbox", []);
    expect(inbox.visible).toContain("Nothing needs input or attention in this read.");
    expect(inbox.visible).not.toContain("Unseen routing"); expect(inbox.visible).toContain("▸ About this page");
    expect(byId(inbox, "page-title").props.accessibilityLabel).toBeUndefined();
    model.open = true;
    const [openInbox, openSearch] = [render("Inbox", []), render("Search", [planRead])];
    model.open = false;
    expect(openInbox.visible).toContain("Native needs-input, errors and attention in the current inventory. Opening a session uses Paseo’s own controls. Unseen routing is not captured.");
    expect(openSearch.visible).toContain("Scope: available session and workspace names, configured plan text and canonical task records. No private transcript search.");
    const search = render("Search", [planRead]);
    const field = search.nodes.find((node) => node.type === "TextInput")!;
    expect(field.props.placeholder).toBe("Search sessions, plans and tasks");
    expect(field.props.accessibilityHint).toBe("Scope: available session and workspace names, configured plan text and canonical task records. No private transcript search.");
    expect(search.visible).not.toContain("Scope:"); expect(search.visible).not.toContain("Plan: ");
    expect(search.labels).toContain("Plan: Configured plan");
  });
});
