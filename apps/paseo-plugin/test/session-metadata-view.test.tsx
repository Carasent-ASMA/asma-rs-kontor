import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
const state = vi.hoisted(() => ({ expanded: false, help: [] as string[] }));
vi.mock("react", async original => ({ ...await original<typeof import("react")>(), useState: () => [state.expanded, (update: (value: boolean) => boolean) => { state.expanded = update(state.expanded); }] }));
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/page-controls", () => ({ Action: "Action" }));
vi.mock("../client/control-help", () => ({ useControlHelp: (text: string) => { state.help.push(text); return { bindings: { ref: vi.fn(), onHoverIn: vi.fn(), onHoverOut: vi.fn() } }; } }));
import { SessionUsage, SessionMetadataGuide } from "../client/session-usage-view";
import { SessionDetailSection } from "../client/session-detail-section";
type Element = ReactElement<Record<string, unknown>>;
function all(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(all);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...all(node.props.children as ReactNode)];
}
const colors = {} as Parameters<typeof SessionUsage>[0]["colors"];
const entry = { provider: "codex", model: "gpt-6.1-sol", telemetry: { lastUsage: { inputTokens: 0, outputTokens: 5 } } };
beforeEach(() => { state.expanded = false; state.help = []; });

describe("metadata presentation and requested explanations", () => {
  it("attaches separate contextual help without introducing row controls or changing activation", () => {
    const tree = all(SessionUsage({ entry, colors }));
    expect(state.help).toHaveLength(2);
    expect(state.help[0]).toContain("Configured model");
    expect(state.help[1]).not.toContain("model");
    for (const id of ["session-model-help", "session-counter-help"]) {
      const region = tree.find(node => node.props.testID === id)!;
      expect(region.props.onPointerEnter).toBeTypeOf("function");
      expect(region.props.onPress).toBeUndefined();
      expect(region.props.tabIndex).toBeUndefined();
    }
  });

  it("keeps the guide out of reading order until requested and exposes expanded state", () => {
    let tree = all(SessionMetadataGuide({ entry, colors }));
    let trigger = tree.find(node => node.props.label === "Model and usage guide")!;
    expect(trigger.props.expanded).toBe(false);
    expect(tree.find(node => node.props.testID === "session-metadata-guide")).toBeUndefined();
    (trigger.props.onPress as () => void)();
    tree = all(SessionMetadataGuide({ entry, colors }));
    trigger = tree.find(node => node.props.label === "Model and usage guide")!;
    expect(trigger.props.expanded).toBe(true);
    expect(tree.filter(node => node.props.testID === "session-metadata-guide")).toHaveLength(1);
    (trigger.props.onPress as () => void)();
    expect(all(SessionMetadataGuide({ entry, colors })).find(node => node.props.testID === "session-metadata-guide")).toBeUndefined();
  });

  it("renders a section heading before exact selectable values without truncation", () => {
    const exact = "custom/model-with-a-long-version-123456789";
    const tree = all(SessionDetailSection({ title: "Model", values: [{ label: "Model ID", value: exact }], colors }));
    const heading = tree.findIndex(node => node.props.accessibilityRole === "header");
    const value = tree.findIndex(node => node.props.children === exact);
    expect(heading).toBeLessThan(value);
    expect(tree[value]!.props.selectable).toBe(true);
    expect(tree[value]!.props.numberOfLines).toBeUndefined();
  });
});
