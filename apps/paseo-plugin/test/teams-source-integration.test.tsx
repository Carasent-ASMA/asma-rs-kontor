import { fileURLToPath, URL } from "node:url";
import { isValidElement, type ReactNode } from "react";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { AUTHORIZED_TEAMS_FORMATS, teamsSourceBindings } from "../shared/teams-source-bindings";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import type { SourceRead } from "../shared/read-sources";
import plugin from "../index.server";
import { sessionEntry } from "./session-fixtures";
const model = vi.hoisted(() => ({ open: false, query: null as any, rpc: new Map<string, ReturnType<typeof vi.fn>>() }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: () => [model.open, (value: boolean) => { model.open = value; }] }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => model.rpc.get(contract.name) }));
vi.mock("@tanstack/react-query", () => ({ useQuery: (options: unknown) => { model.query = options; return { data: undefined }; } }));
import { TeamsSourcePage } from "../client/teams-source-page";
import { useReadSources } from "../client/use-read-sources";
const filenames = ["authoring-fleet.yml", "operator-fleet.yml", "core-team.yml", "mvp-team-pack.json"];
const catalog = sourceCatalog(JSON.stringify(Object.keys(AUTHORIZED_TEAMS_FORMATS).map((id, index) => ({ id, title: id, kind: "teams", path: fileURLToPath(new URL(`./fixtures/teams-source/${filenames[index]}`, import.meta.url)) }))));
const legacy = { ...catalog.sources[0]!, id: "legacy-json", path: "/configured/legacy.json" };
const colors = {} as Parameters<typeof TeamsSourcePage>[0]["colors"];
let snapshots: Record<string, TeamsSourceSnapshot>;
beforeEach(async () => {
  model.open = false; model.rpc.clear();
  const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
  snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
});
afterEach(() => vi.unstubAllEnvs());
function visible(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(visible).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return visible((node.type as (props: any) => ReactNode)(node.props));
  return visible(node.props.children as ReactNode);
}
function page(overrides: Partial<Parameters<typeof TeamsSourcePage>[0]> = {}) {
  return TeamsSourcePage({ sources: catalog.sources, snapshots, available: true, reads: {}, entries: [sessionEntry("native-exact")], observedAt: "2026-10-18T10:00:00Z", colors, children: "Native rows retained", ...overrides });
}
describe("A-owned Teams source integration", () => {
  it("maps only the four exact authorized teams IDs, without deriving paths or replacements", () => {
    expect(teamsSourceBindings(catalog.sources)).toEqual({ formats: AUTHORIZED_TEAMS_FORMATS, diagnostics: [] });
    expect(teamsSourceBindings([]).formats).toEqual({});
    const mismatch = teamsSourceBindings([{ ...catalog.sources[0]!, kind: "plan" }, { ...catalog.sources[1]!, id: "operator-renamed" }]);
    expect(mismatch.formats).toEqual({}); expect(mismatch.diagnostics).toHaveLength(4);
    expect(mismatch.diagnostics.join(" ")).toContain("no path or replacement ID inferred");
    expect(teamsSourceBindings([catalog.sources[0]!, catalog.sources[0]!]).formats).toEqual({});
  });
  it("registers the typed handler from the existing catalog while preserving sources.read JSON semantics", async () => {
    vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", JSON.stringify(catalog.sources));
    const handlers = new Map<string, (input: unknown) => any>(); const hooks: string[] = [];
    const stop = plugin({ registerSettings: () => ({ read: async () => ({ status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS }), subscribe: () => () => {} }), handle: (rpc: { name: string }, handler: (input: unknown) => any) => handlers.set(rpc.name, handler), before: (name: string) => hooks.push(name) } as unknown as PluginServerContext);
    try {
      expect(hooks).toEqual(["agent.create", "agent.session_open", "workspace.create"]);
      expect((await handlers.get("sources.list")!({})).sources).toEqual(catalog.sources);
      expect((await handlers.get("teams.sources.read")!({ sourceId: "authoring" })).data.kind).toBe("fleet");
      // The old RPC does not silently reinterpret YAML as the new format.
      expect((await handlers.get("sources.read")!({ sourceId: "authoring" })).status).toBe("unavailable");
      await expect(handlers.get("teams.sources.read")!({ sourceId: "renamed" })).rejects.toThrow();
    } finally { stop(); }
  });
  it("keeps an empty host catalog opt-in with no invented source", async () => {
    vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", "");
    const handlers = new Map<string, (input: unknown) => any>();
    const stop = plugin({ registerSettings: () => ({ read: async () => ({ status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS }), subscribe: () => () => {} }), handle: (rpc: { name: string }, handler: (input: unknown) => any) => handlers.set(rpc.name, handler), before: () => {} } as unknown as PluginServerContext);
    try { expect((await handlers.get("sources.list")!({})).sources).toEqual([]); await expect(handlers.get("teams.sources.read")!({ sourceId: "authoring" })).rejects.toThrow(); }
    finally { stop(); }
  });
  it("dispatches ID-only typed reads separately from legacy JSON and isolates a failed source", async () => {
    const list = vi.fn(async () => ({ sources: [...catalog.sources, legacy], diagnostics: [] }));
    const json = { source: legacy, records: { teams: [] } } as unknown as SourceRead;
    const read = vi.fn(async () => json);
    const teams = vi.fn(async ({ sourceId }: { sourceId: string }) => { if (sourceId === "operator") throw new Error("operator unavailable"); return snapshots[sourceId]!; });
    model.rpc.set("sources.list", list); model.rpc.set("sources.read", read); model.rpc.set("teams.sources.read", teams);
    useReadSources("host-A", true);
    expect(model.query.queryKey).toEqual(["kontor-read-sources", "host-A"]);
    const result = await model.query.queryFn({ signal: new AbortController().signal });
    expect(teams.mock.calls.map(([input]) => input).sort((a, b) => a.sourceId.localeCompare(b.sourceId))).toEqual(Object.keys(AUTHORIZED_TEAMS_FORMATS).sort().map((sourceId) => ({ sourceId })));
    expect(read).toHaveBeenCalledExactlyOnceWith({ sourceId: "legacy-json" });
    expect(Object.keys(result.teamsSnapshots)).toHaveLength(3); expect(result.teamsErrors).toEqual({ operator: "operator unavailable" });
    expect(result.reads).toEqual({ "legacy-json": json });
    useReadSources("host-B", false); expect(model.query.queryKey).toEqual(["kontor-read-sources", "host-B"]); expect(model.query.enabled).toBe(false);
  });
  it("keeps source states and times visible while hiding full paths and hashes behind Source details", () => {
    const text = visible(page());
    expect(text).toContain("Source state: ready"); expect(text).toContain("source observation time: Not supplied");
    expect(text).toContain(snapshots.authoring!.readAt); expect(text).toContain("activation: not-attested");
    for (const snapshot of Object.values(snapshots)) { expect(text).not.toContain(snapshot.source.path); expect(text).not.toContain(snapshot.sha256); }
    expect(text).toContain("Source details"); expect(text).not.toContain("Rung 1:");
    model.open = true; const expanded = visible(page());
    expect(expanded).toContain(snapshots.authoring!.source.path); expect(expanded).toContain(snapshots.authoring!.sha256);
    expect(expanded).toContain("/domains/"); expect(expanded).toContain("Rung 1:");
  });
  it("preserves template versions, unresolved identities and distinct native observations", () => {
    model.open = true; const text = visible(page());
    expect(text).toContain("74 binding identities remain unresolved");
    expect(text).toContain("Binding does not pin a template version"); expect(text).toContain("slot alone is insufficient");
    expect(text).toContain(" v1"); expect(text).toContain(" v2");
    expect(text).toContain("Native observations: 1 sessions · observed 2026-10-18T10:00:00Z");
    expect(text).toContain("No configuration-to-session assignment is inferred"); expect(text).toContain("Native rows retained");
    expect(text).toContain("kontord is not enabled"); expect(text).not.toContain("Connect");
  });
  it("keeps partial/unavailable/stale configurations independent and native rows usable", () => {
    const partial = { ...snapshots.authoring!, status: "partial" as const, diagnostics: [{ code: "probe", pointer: "/models/missing", message: "Unknown definition" }] };
    const unavailable = { ...snapshots.operator!, status: "unavailable" as const, data: null };
    const text = visible(page({ snapshots: { ...snapshots, authoring: partial, operator: unavailable }, available: false }));
    expect(text).toContain("Source state: partial · stale"); expect(text).toContain("Source state: unavailable · stale");
    expect(text).toContain("18 chains"); expect(text).toContain("Other sources and native sessions remain usable"); expect(text).toContain("Native rows retained");
    expect(text).not.toContain("Unknown definition"); model.open = true;
    expect(visible(page({ snapshots: { authoring: partial } }))).toContain("probe · /models/missing: Unknown definition");
  });
  it("retains native content and separate JSON source behavior when mappings are missing", () => {
    const text = visible(page({ sources: [legacy], snapshots: {}, reads: {}, available: false }));
    expect(text).toContain("No authorized Teams adapter IDs configured"); expect(text).toContain("JSON record source"); expect(text).toContain("Native rows retained");
    expect(text).not.toContain(legacy.path); model.open = true;
    expect(visible(page({ sources: [], snapshots: {} }))).toContain("ID missing from the configured catalog");
  });
});
