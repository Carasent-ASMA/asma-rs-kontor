import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View" }));
import { FormattedPlanBlock, headingAnchor, planBlocks } from "../client/plan-document";
import { resolvePlanLink, safeHttpsUrl } from "../client/plan-links";
import { sessionEntry } from "./session-fixtures";
import type { ReadSourceSummary } from "../shared/read-sources";
type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
const source: ReadSourceSummary = { id: "source", title: "Source", kind: "plan", path: "/plans/a.md", epic: null, projectId: null, workstream: null, revisionKind: "working copy" };
describe("native read-only Plan presentation", () => {
  it("formats heading, paragraph, list, table and fenced code without changing source meaning", () => {
    const content = "# Title\n\nA **paragraph**.\n\n- first\n- second\n\n| Name | Value |\n| --- | --- |\n| item | a\\|b |\n\n```\n## not a heading\n```";
    const blocks = planBlocks(content);
    expect(blocks.map((block) => block.kind)).toEqual(["heading", "paragraph", "list", "table", "code"]);
    expect(blocks[0]).toMatchObject({ line: 0, text: "Title", level: 1 });
    expect(blocks[3]?.rows).toEqual([["Name", "Value"], ["item", "a|b"]]);
    expect(blocks[4]?.text).toBe("## not a heading"); expect(headingAnchor("Review decision")).toBe("review-decision");
  });
  it("provides link accessibility/actions and selectable native text, with no HTML or execution surface", () => {
    const onLink = vi.fn(); const colors = {} as Parameters<typeof FormattedPlanBlock>[0]["colors"];
    const nodes = elements(FormattedPlanBlock({ block: { line: 0, kind: "paragraph", text: "[Issue](https://carasent.atlassian.net/browse/ASMA-8541)" }, colors, query: "Issue", onLink }));
    const link = nodes.find((node) => node.props.accessibilityRole === "link")!;
    (link.props.onPress as () => void)(); expect(onLink).toHaveBeenCalledWith("https://carasent.atlassian.net/browse/ASMA-8541");
    expect(nodes.some((node) => node.props.selectable)).toBe(true);
  });
  it("opens only configured source paths, exact observed session IDs or safe explicit HTTPS URLs", () => {
    const other = { ...source, id: "other", path: "/plans/b.md" };
    expect(resolvePlanLink("./b.md", source, [source, other], [])).toEqual({ kind: "source", id: "other" });
    expect(resolvePlanLink("./b.md", source, [source, other, { ...other, id: "different-pin" }], []).kind).toBe("blocked");
    expect(resolvePlanLink("paseo://agents/native", source, [source], [sessionEntry("native")])).toEqual({ kind: "session", id: "native" });
    expect(resolvePlanLink("https://carasent.atlassian.net/browse/ASMA-8541", source, [source], [])).toEqual({ kind: "url", url: "https://carasent.atlassian.net/browse/ASMA-8541" });
    for (const target of ["../../secret.md", "file:///etc/passwd", "javascript:alert(1)", "data:text/html,unsafe", "https://user:secret@example.com", "paseo://agents/missing", "paseo://agents/native/extra", "https://example.com\n/path", "https://example.com:99999/a", "https://example.com\\@evil.test"]) expect(resolvePlanLink(target, source, [source], [sessionEntry("native")]).kind).toBe("blocked");
  });
  it("does not depend on RN's partial URL constructor for file and session links", () => {
    vi.stubGlobal("URL", undefined);
    try {
      expect(resolvePlanLink("file:///plans/a.md", source, [source], [])).toEqual({ kind: "source", id: "source" });
      expect(resolvePlanLink("paseo://agents/native", source, [source], [sessionEntry("native")])).toEqual({ kind: "session", id: "native" });
    } finally { vi.unstubAllGlobals(); }
  });
});

describe("shared read-only external HTTPS guard", () => {
  it.each([
    "https://example.com/decision?source=record#evidence", "https://example.com:443/a%20b", "HTTPS://example.com",
    "https://127.0.0.1/evidence", "https://[::1]/evidence", "https://[2001:db8::1]:443/evidence",
  ])("accepts explicit valid HTTPS without relying on RN URL: %s", (target) => {
    vi.stubGlobal("URL", undefined);
    try {
      expect(safeHttpsUrl(target)).toBe(target);
      expect(resolvePlanLink(target, source, [source], [])).toEqual({ kind: "url", url: target });
    } finally { vi.unstubAllGlobals(); }
  });
  it.each([
    "", "http://example.com", "javascript:alert(1)", "data:text/html,unsafe", "file:///configured/source.md", "//example.com",
    "https://user:secret@example.com", "https://user@example.com", "https:/example.com", "https://", "https://example..com",
    "https://-example.com", "https://example.com:0", "https://example.com:99999", "https://example.com\\@evil.test",
    "https://example.com\n/path", "https://example.com/a b", "https://example.com/%zz", "https://[::::]/", "https://999.0.0.1/",
  ])("rejects unsafe or malformed targets for both records and Plan: %s", (target) => {
    expect(safeHttpsUrl(target)).toBeNull();
    expect(resolvePlanLink(target, source, [source], []).kind).toBe("blocked");
  });
});
