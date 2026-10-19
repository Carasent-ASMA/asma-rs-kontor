import { isValidElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (f: () => unknown) => f(), useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, () => {}] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { NativeFreshnessPill } from "../client/native-freshness";
import { ReadOnlyPage } from "../client/read-only-pages";
import { emptyDirectoryState, type DirectoryState } from "../client/session-directory";
const timestamp = Date.parse("2026-10-19T14:00:00Z");
function directory(flag: boolean | undefined): DirectoryState {
  const { inventoryComplete: ignored, ...legacy } = emptyDirectoryState();
  return { ...legacy, status: "ready", live: flag === true, lastUpdatedAt: timestamp,
    staleReason: flag === true ? null : "Live updates are unavailable; the inventory may be incomplete.",
    ...(flag === undefined ? {} : { inventoryComplete: flag }) };
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (p: Record<string, unknown>) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function overview(state: DirectoryState): string {
  const props: Parameters<typeof ReadOnlyPage>[0] = { tab: "Overview", colors: {} as Parameters<typeof ReadOnlyPage>[0]["colors"], entries: [], directory: state, sources: [], reads: {}, sourceId: null, sourceError: null, query: "", setQuery: () => {}, onSource: () => {}, onOpen: () => {}, onTab: () => {}, onEpicTab: () => {}, initialOffset: 0, onScroll: () => {} };
  return text(ReadOnlyPage(props));
}
function pill(state: DirectoryState): string {
  return NativeFreshnessPill({ state, hostStatus: "online", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"] }).props.accessibilityLabel;
}
describe("prepared integration partial inventory contract: actual display consumers", () => {
  it.each([false, undefined])("shared freshness must not describe completeness=%s as a complete read", (flag) => {
    const actual = pill(directory(flag));
    expect(actual).toContain("2026-10-19T14:00:00.000Z");
    expect(actual).not.toMatch(/last complete read/i);
  });
  it.each([false, undefined])("Overview must not describe completeness=%s as a complete read", (flag) => {
    const actual = overview(directory(flag));
    expect(actual).toContain("inventory may be incomplete");
    expect(actual).not.toMatch(/last complete read/i);
  });
  it("shared freshness may label an explicitly complete capture complete", () => {
    expect(pill(directory(true))).toMatch(/last complete read.*2026-10-19T14:00:00.000Z/i);
  });
  it("Overview may label an explicitly complete capture complete", () => {
    expect(overview(directory(true))).toMatch(/last complete read.*2026-10-19T14:00:00.000Z/i);
  });
});

// Keep the six independent QA reproductions above and cover the timestamp boundaries.
describe.each([
  { consumer: "shared freshness", render: pill },
  { consumer: "Overview", render: overview },
])("$consumer inventory read semantics", ({ render }) => {
  it("identifies a partial successful read without changing its observed timestamp", () => {
    expect(render(directory(false))).toContain("Last partial inventory read 2026-10-19T14:00:00.000Z");
  });
  it("keeps absent and explicit undefined completeness Unknown", () => {
    const absent = directory(undefined);
    expect(Object.hasOwn(absent, "inventoryComplete")).toBe(false);
    const actual = render(absent);
    expect(actual).toContain("Last inventory read 2026-10-19T14:00:00.000Z · completeness Unknown");
    expect(render({ ...absent, inventoryComplete: undefined })).toBe(actual);
  });
  it.each([true, false, undefined])("keeps a null read time Unknown/not read yet (completeness=%s)", (flag) => {
    const actual = render({ ...directory(flag), lastUpdatedAt: null });
    expect(actual).toContain("Inventory not read yet · time Unknown");
    expect(actual).not.toContain("2026-10-19T14:00:00.000Z");
    expect(actual).not.toMatch(/last complete read/i);
  });
  it("retains an epoch-zero timestamp instead of treating it as missing", () => {
    expect(render({ ...directory(false), lastUpdatedAt: 0 })).toContain("Last partial inventory read 1970-01-01T00:00:00.000Z");
  });
});
