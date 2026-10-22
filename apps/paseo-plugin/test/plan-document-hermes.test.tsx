import { isValidElement, type ReactElement, type ReactNode } from "react";
import * as jsxRuntime from "react/jsx-runtime";
import { describe, expect, it, vi } from "vitest";
import type { FormattedPlanBlock as Block, planBlocks as Blocks } from "../client/plan-document";
import { loadHermesScoped } from "./hermes-scoping";

type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}

describe("plan links under the iOS (Hermes) scoping of plugin bundles", () => {
  it("opens each link's own target when a paragraph has several links", () => {
    const { FormattedPlanBlock, planBlocks } = loadHermesScoped<{ FormattedPlanBlock: typeof Block; planBlocks: typeof Blocks }>("../client/plan-document.tsx", {
      "react/jsx-runtime": jsxRuntime, "react-native": { ScrollView: "ScrollView", Text: "Text", View: "View" }, "./page-controls": { Disclosure: "Disclosure" } });
    const onLink = vi.fn(), block = planBlocks("See [first](plans/a.md) and [second](plans/b.md).")[0]!;
    const links = elements(FormattedPlanBlock({ block, colors: {} as Parameters<typeof Block>[0]["colors"], query: "", onLink })).filter((node) => node.props.accessibilityRole === "link");
    expect(links.map((node) => node.props.accessibilityLabel)).toEqual(["first: plans/a.md", "second: plans/b.md"]);
    (links[0]!.props.onPress as () => void)();
    expect(onLink).toHaveBeenCalledExactlyOnceWith("plans/a.md");
  });
});
