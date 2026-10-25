import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [typeof initial === "function" ? (initial as () => unknown)() : initial, () => {}] }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { WorkCatalogStatus } from "../client/catalog-status";
import { ReadOnlyPage } from "../client/read-only-pages";
import { emptyDirectoryState } from "../client/session-directory";
import type { WorkCatalogNativeRead, WorkCatalogNativeView, WorkCatalogRead, WorkCatalogSource, WorkCatalogView } from "../shared/work-catalog";

function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return [node, ...elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props))];
  return [node, ...elements(node.props.children as ReactNode)];
}

const asOf = "2026-10-25T06:14:01Z";
const checkedAt = "2026-10-25T09:00:00Z";
const source = (name: "plan" | "jira" | "native", overrides: Partial<WorkCatalogSource> = {}): WorkCatalogSource => ({
  source: name,
  state: "complete",
  generation: 4,
  attemptedGeneration: 5,
  asOf,
  error: null,
  retryAt: null,
  coverage: [{ subject: name === "plan" ? "collection" : name === "jira" ? "scope" : "archive coverage", state: "complete", detail: `${name} coverage detail` }],
  conflicts: [],
  pendingVerification: name === "plan" ? 3 : 0,
  ...overrides,
});
const view: WorkCatalogView = {
  schemaVersion: 1,
  sources: [source("plan", { conflicts: [{ kind: "moved_key", subject: "ASMA-1", message: "ASMA-1 moved to ASMA-2", observedAt: asOf }] }), source("jira", { state: "not_configured", generation: 0, attemptedGeneration: 0, asOf: null, coverage: [], conflicts: [], pendingVerification: 0 }), source("native")],
  entries: [],
  entriesTruncated: false,
};
const nativeView: WorkCatalogNativeView = {
  runtimeInstance: "paseo:ws://127.0.0.1:6767/ws",
  generation: 4,
  asOf,
  source: "database",
  archiveScope: true,
  truncated: false,
  projects: [],
  workspaces: [],
  sessions: [],
};
const native = (overrides: Partial<WorkCatalogNativeRead> = {}): WorkCatalogNativeRead => ({
  mode: "catalog",
  hostId: "host-a",
  runtimeInstance: nativeView.runtimeInstance,
  checkedAt,
  view: nativeView,
  error: null,
  snapshotSavedAt: null,
  ...overrides,
});
const live: WorkCatalogRead = { mode: "catalog", checkedAt, sourceAsOf: asOf, view, error: null, snapshot: null, native: native() };

describe("the shared-catalog display", () => {
  it("shows per-source freshness, coverage, conflicts and pending verification", () => {
    const rendered = text(WorkCatalogStatus({ state: live, colors: {} as Parameters<typeof WorkCatalogStatus>[0]["colors"] }));
    expect(rendered).toContain("Shared catalog: live through the observer credential.");
    expect(rendered).toContain(`Oldest source as of ${asOf}`);
    expect(rendered).toContain(`Plans · complete · generation 4 · as of ${asOf}`);
    expect(rendered).toContain("plan coverage detail");
    expect(rendered).toContain("3 associations pending verification");
    expect(rendered).toContain("Jira · not configured");
    expect(rendered).toContain("Native Paseo · complete");
    expect(rendered).toContain("Native inventory: shared catalog paseo:ws://127.0.0.1:6767/ws · generation 4");
    expect(rendered).toContain("Conflict (moved_key, ASMA-1): ASMA-1 moved to ASMA-2");
    expect(rendered).not.toContain(checkedAt);
  });

  it("labels a last-known snapshot and keeps the source's as-of, not the check time", () => {
    const snapshot: WorkCatalogRead = {
      ...live,
      mode: "snapshot",
      checkedAt: "2026-10-25T09:00:00Z",
      error: "the observer could not be started",
      snapshot: { savedAt: "2026-10-25T08:00:00Z", sourceAsOf: asOf },
    };
    const rendered = text(WorkCatalogStatus({ state: snapshot, colors: {} as Parameters<typeof WorkCatalogStatus>[0]["colors"] }));
    expect(rendered).toContain("last-known snapshot");
    expect(rendered).toContain(`oldest source as of ${asOf}`);
    expect(rendered).toContain("the observer could not be started");
    expect(rendered).not.toContain("2026-10-25T09:00:00");
    expect(rendered).not.toMatch(/as of 2026-10-25T09/);
  });

  it("keeps an unavailable catalog truthful and says the direct readers remain", () => {
    const rendered = text(WorkCatalogStatus({
      state: { mode: "unavailable", checkedAt, sourceAsOf: null, view: null, error: "no observer", snapshot: null, native: native({ mode: "unavailable", runtimeInstance: null, view: null, error: "no native inventory" }) },
      colors: {} as Parameters<typeof WorkCatalogStatus>[0]["colors"],
    }));
    expect(rendered).toContain("Shared catalog unavailable. Direct source readers remain active.");
    expect(rendered).toContain("no observer");
  });

  it("places the catalog statement inside Overview's Sources and coverage", () => {
    const props: Parameters<typeof ReadOnlyPage>[0] = {
      tab: "Overview", colors: {} as Parameters<typeof ReadOnlyPage>[0]["colors"], entries: [], directory: emptyDirectoryState(),
      sources: [], reads: {}, sourceId: null, sourceError: null, catalog: live, query: "", setQuery: () => {},
      onSource: () => {}, onOpen: () => {}, onTab: () => {}, onEpicTab: () => {}, initialOffset: 0, onScroll: () => {},
    };
    const tree = ReadOnlyPage(props);
    const disclosure = elements(tree).find((node) => String(node.props.label).startsWith("Sources and coverage"))!;
    const rendered = text(disclosure.props.children as ReactNode);
    expect(rendered).toContain("Shared catalog: live through the observer credential.");
    expect(rendered).toContain("3 associations pending verification");
    expect(String(disclosure.props.label)).toContain("catalog live");

    const snapshot: WorkCatalogRead = {
      ...live,
      mode: "snapshot",
      error: "the observer could not be started",
      snapshot: { savedAt: "2026-10-25T08:00:00Z", sourceAsOf: asOf },
    };
    const staleTree = ReadOnlyPage({ ...props, catalog: snapshot });
    const staleDisclosure = elements(staleTree).find((node) => String(node.props.label).startsWith("Sources and coverage"))!;
    expect(String(staleDisclosure.props.label)).toContain("catalog snapshot");
    expect(String(staleDisclosure.props.accessibilityLabel)).toContain("catalog snapshot");
  });
});
