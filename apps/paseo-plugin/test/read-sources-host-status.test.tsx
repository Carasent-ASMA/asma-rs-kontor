import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginHostSummary, PluginScreenProps } from "@getpaseo/plugin/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import { sessionEntry } from "./session-fixtures";
import { GraphPage } from "../client/graph-page";
import type { SourceRead } from "../shared/read-sources";
import { useReadSources } from "../client/use-read-sources";
type SourceData = Pick<ReturnType<typeof useReadSources>, "sources" | "reads" | "teamsSnapshots" | "teamsErrors" | "teamsBindingDiagnostics"> & { diagnostics: string[] };
const model = vi.hoisted(() => ({
  entries: [] as DirectoryEntry[],
  status: "online" as PluginHostSummary["status"], data: undefined as SourceData | undefined, error: null as Error | null,
  queryOptions: null as { queryKey: readonly unknown[]; enabled: boolean; refetchInterval: number | false; retry: boolean; staleTime: number } | null,
  rpc: vi.fn(), refetch: vi.fn(async () => ({})),
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: (initial: unknown) => [typeof initial === "function" ? initial() : initial, vi.fn()], useRef: (current: unknown) => ({ current }), useMemo: (factory: () => unknown) => factory(), useCallback: (callback: unknown) => callback, useEffect: () => {} }));
vi.mock("react-native", () => ({ SafeAreaView: "SafeAreaView", KeyboardAvoidingView: "KeyboardAvoidingView", ScrollView: "ScrollView", Text: "Text", View: "View", Pressable: "Pressable", Linking: { openURL: vi.fn() } }));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => [{ serverId: "source-host", label: "Host", status: model.status }], useRpc: () => model.rpc }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Modal: Object.assign(() => null, { Content: () => null }), TextInput: "TextInput", ScrollView: "ScrollView", FlatList: "FlatList", Icon: "Icon", useToast: () => ({ error: vi.fn() }) }));
vi.mock("@tanstack/react-query", () => ({ useQuery: (options: typeof model.queryOptions) => { model.queryOptions = options; return { data: model.data, isError: model.error !== null, error: model.error, isFetching: false, refetch: model.refetch }; } }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: { entries: model.entries, status: "ready", live: true, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false }, refresh: vi.fn() }) }));
vi.mock("../client/use-workspace-directory", () => ({ useWorkspaceDirectory: () => ({ entries: [], error: null, readAt: null }) }));
vi.mock("../client/use-organization-settings", () => ({ useOrganizationSettings: () => ({ state: { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, saving: false, saveError: null } }) }));
vi.mock("../client/use-keyboard-settings", () => ({ useKeyboardSettings: () => ({ state: { status: "loading" }, save: async () => true }) }));
import { KontorScreen } from "../client/kontor-screen";
import { NativeFreshnessPill } from "../client/native-freshness";
import { ReadOnlyPage } from "../client/read-only-pages";
import { readTime } from "../client/page-data";
import { navigate } from "../client/navigation-state";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
const delivery: SourceRead = { source: { id: "delivery", title: "Captured delivery", kind: "delivery", path: "/configured/delivery.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, graph: null, content: null, diagnostics: [], records: { teams: [], decisions: [], delivery: [{ id: "retained-check", title: "Retained delivery record", kind: "check", state: "captured", recordedAt: "2026-10-18T17:00:00Z" }] } };
const decisions: SourceRead = { ...delivery, source: { ...delivery.source, id: "decisions", title: "Captured decisions", kind: "decisions", path: "/configured/decisions.json" }, records: { teams: [], delivery: [], decisions: [{ id: "retained-decision", title: "Retained decision record", state: "open" }] } };
beforeEach(() => {
  vi.clearAllMocks(); model.entries = []; model.status = "online"; model.error = null; model.queryOptions = null;
  model.data = { sources: [delivery.source, decisions.source], reads: { delivery, decisions }, diagnostics: [], teamsSnapshots: {}, teamsErrors: {}, teamsBindingDiagnostics: [] };
});
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  return isValidElement<Record<string, unknown>>(node) ? [node, ...elements(node.props.children as ReactNode)] : [];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function screen(tab: "Delivery" | "Decisions") {
  navigate("source-host", { tab });
  const props: PluginScreenProps = { host: { id: "source-host", label: "Host" }, params: {}, layout: { compact: true, platform: "ios" }, theme: { colors: {} } as PluginScreenProps["theme"] };
  const outer = KontorScreen(props); const tree = (outer.type as (props: PluginScreenProps) => ReactNode)(props);
  const nodes = elements(tree);
  const pill = nodes.find((node) => node.type === NativeFreshnessPill)!;
  const page = nodes.find((node) => node.type === ReadOnlyPage)!;
  return { indicator: text(NativeFreshnessPill(pill.props as Parameters<typeof NativeFreshnessPill>[0])), visible: text(ReadOnlyPage(page.props as Parameters<typeof ReadOnlyPage>[0])), page };
}
describe("actual source hook and native indicator agree on SDK host status", () => {
  it.each([
    { status: "connecting", host: "reconnecting", indicator: "Reconnecting" },
    { status: "error", host: "error", indicator: "Host error" },
    { status: "idle", host: "idle", indicator: "Host idle" },
    { status: "offline", host: "offline", indicator: "Host offline" },
  ] as const)("retains records and an honest stale banner through online → $status → online", ({ status, host, indicator }) => {
    for (const tab of ["Delivery", "Decisions"] as const) {
      const retained = model.data!.reads;
      expect(screen(tab).indicator).toContain("Native · Live");
      expect(model.queryOptions).toMatchObject({ enabled: true, refetchInterval: 30_000 });
      model.status = status;
      const disconnected = screen(tab);
      const banner = `Paseo host ${host}. Configured sources unavailable; retained reads are stale.`;
      expect(disconnected.indicator).toContain(`Native · ${indicator}`);
      expect(disconnected.visible).toContain(banner); expect(disconnected.page.props.sourceError).toBe(banner);
      if (status !== "offline") expect(disconnected.visible).not.toContain("Paseo host offline");
      expect(disconnected.page.props.reads).toEqual(retained);
      expect(disconnected.visible).toContain(tab === "Delivery" ? "Retained delivery record" : "Retained decision record");
      // The retained read time stays visible as a short cue; the exact time is in the meta label and Source details.
      expect(disconnected.visible).toContain(`read ${readTime(delivery.readAt)}`);
      expect(model.queryOptions).toMatchObject({ queryKey: ["kontor-read-sources", "source-host"], enabled: false, refetchInterval: false, retry: false, staleTime: 0 });
      model.status = "online";
      expect(screen(tab).page.props.sourceError).toBeNull();
      expect(model.queryOptions).toMatchObject({ enabled: true, refetchInterval: 30_000 });
    }
    expect(model.rpc).not.toHaveBeenCalled();
  });
  it("keeps the online query-error message and retained records unchanged", () => {
    model.error = new Error("Source request failed");
    const page = screen("Delivery");
    expect(page.visible).toContain("Configured source service unavailable: Source request failed. Retained reads are stale.");
    expect(page.visible).toContain("Retained delivery record"); expect(page.indicator).toContain("Native · Live");
    expect(useReadSources("source-host", "online").available).toBe(false);
    expect(model.queryOptions).toMatchObject({ enabled: true, refetchInterval: 30_000 });
  });
  it("keeps online catalog diagnostics, read identity and explicit refetch forwarding unchanged", async () => {
    model.data!.diagnostics = ["First diagnostic.", "Second diagnostic."];
    const result = useReadSources("source-host", "online");
    expect(result.error).toBe("First diagnostic. Second diagnostic."); expect(result.available).toBe(true);
    expect(result.reads).toBe(model.data!.reads);
    await result.refresh(); expect(model.refetch).toHaveBeenCalledTimes(1); expect(model.rpc).not.toHaveBeenCalled();
    expect(screen("Decisions").visible).toContain("First diagnostic. Second diagnostic.");
  });
  it("keeps an online not-yet-read query distinct from a disconnected host", () => {
    model.data = undefined;
    const result = useReadSources("source-host", "online");
    expect(result.error).toBeNull(); expect(result.available).toBe(false); expect(result.reads).toEqual({});
    expect(model.queryOptions).toMatchObject({ enabled: true, refetchInterval: 30_000 });
  });
});

describe("project source scope across the actual page router", () => {
  it("keeps the same canonical plan in Overview/Plan/Board/Graph and exact delivery records, without a sibling epic inference", () => {
    const plan: SourceRead = { ...delivery, source: { ...delivery.source, id: "canonical", title: "Canonical plan", kind: "plan", epic: "ASMA-1" }, content: "# Canonical plan", records: null };
    const ownDelivery = { ...delivery, source: { ...delivery.source, epic: "ASMA-1" } };
    const other: SourceRead = { ...plan, source: { ...plan.source, id: "other", epic: "ASMA-2" } };
    model.entries = [sessionEntry("labelled", { projectKey: "project", labels: { epic: "ASMA-1" } }), sessionEntry("unlabelled", { projectKey: "project", title: "ASMA-2" })];
    model.data = { sources: [plan.source, ownDelivery.source, other.source], reads: { canonical: plan, delivery: ownDelivery, other }, diagnostics: [], teamsSnapshots: {}, teamsErrors: {}, teamsBindingDiagnostics: [] };
    const props: PluginScreenProps = { host: { id: "source-host", label: "Host" }, params: {}, layout: { compact: true, platform: "ios" }, theme: { colors: {} } as PluginScreenProps["theme"] };
    for (const tab of ["Overview", "Plan", "Board", "Graph", "Delivery"] as const) {
      navigate("source-host", { tab, projectId: "project", epicKey: null, sourceId: tab === "Plan" || tab === "Board" || tab === "Graph" ? "canonical" : null, originAgentId: null });
      const outer = KontorScreen(props);
      const tree = (outer.type as (props: PluginScreenProps) => ReactNode)(props);
      const page = elements(tree).find((node) => node.type === ReadOnlyPage || node.type === GraphPage)!;
      expect(page.props.sources).toEqual([plan.source, ownDelivery.source]);
      expect(page.props.reads).toEqual({ canonical: plan, delivery: ownDelivery });
      if (tab === "Overview") {
        const visible = text(ReadOnlyPage(page.props as Parameters<typeof ReadOnlyPage>[0]));
        expect(visible).toContain("Canonical plan");
        expect(visible).not.toContain("ASMA-2");
      }
      if (tab === "Delivery") expect(text(ReadOnlyPage(page.props as Parameters<typeof ReadOnlyPage>[0]))).toContain("Retained delivery record");
    }
  });
});
