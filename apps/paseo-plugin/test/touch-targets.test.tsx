import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: (initial: unknown) => [initial, () => {}] }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
const colors = {} as never;
const read: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: "/configured/plan.md", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Plan", graph: null, records: null, diagnostics: [] };
/** Loads the controls fresh, so the pointer probe runs against the stubbed environment. */
async function heights(pointer: "none" | "coarse" | "fine") {
  vi.resetModules();
  if (pointer !== "none") vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(pointer: coarse)" && pointer === "coarse" }));
  const { Disclosure } = await import("../client/page-controls");
  const { SourceDetails } = await import("../client/source-details");
  const height = (tree: ReactNode, label: string) => (elements(tree).find((node) => node.type === "Pressable" && String(node.props.accessibilityLabel).startsWith(label))!.props.style as { minHeight: number }).minHeight;
  return {
    disclosure: height(Disclosure({ label: "Configured chains", colors, children: null }), "Configured chains"),
    small: height(Disclosure({ label: "Legend", colors, children: null, small: true }), "Legend"),
    details: height(SourceDetails({ read, colors }), "Source details"),
  };
}
afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
describe("disclosure touch targets", () => {
  it.each([["none", "a native app"], ["coarse", "a phone or tablet browser"]] as const)("gives every disclosure a 44pt row with no web pointer probe or a coarse pointer (%s: %s)", async (pointer, _where) => {
    expect(await heights(pointer)).toEqual({ disclosure: 44, small: 44, details: 44 });
  });
  it("keeps the denser rows for a mouse or trackpad", async () => {
    expect(await heights("fine")).toEqual({ disclosure: 36, small: 30, details: 30 });
  });
});
