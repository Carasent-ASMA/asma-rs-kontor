import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginSettings } from "@getpaseo/plugin/server";
import { contributeReadSources, readWorkstreamRegistry } from "../server/read-sources";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema } from "../shared/organization";
import { listReadSources } from "../shared/read-sources";

let root: string, config: string;
async function registry(workspace: string, key = "access") {
  const file = join(workspace, "config/orchestration/workstreams.json");
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, JSON.stringify({ schema_version: 1, workstreams: { [key]: { name: key } }, desks: { "pr-review-desk": { name: "Review" } } }));
  return file;
}
beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "kontor-registry-config-"));
  config = join(root, "plugin-config/kontor/registry.json");
  await mkdir(dirname(config), { recursive: true });
  vi.stubEnv("PASEO_HOME", root); vi.stubEnv("KONTOR_PLUGIN_ASMA_WORKSPACE", "");
});
afterEach(async () => { vi.unstubAllEnvs(); await rm(root, { recursive: true, force: true }); });

describe("operator registry configuration", () => {
  it.each([undefined, "broken", "[]", "{}", '{"workspaceRoot":"relative"}', '{"workspaceRoot":null}', '{"workspaceRoot":"/x","extra":true}', '{"workspaceRoot":"/x","workspaceRoot":"/y"}', '{"workspaceRoot":"/x","workspaceRoo\\u0074":"/y"}'])("keeps missing/invalid configuration unavailable: %s", async raw => {
    if (raw !== undefined) await writeFile(config, raw);
    const result = await readWorkstreamRegistry();
    expect(result.revision).toBeNull(); expect(result.workstreams).toEqual({}); expect(result.desks).toEqual({});
    expect(result.diagnostics.join()).toContain("registry unavailable");
  });
  it("honors the custom Paseo home and refreshes local selection and registry schema without a reload", async () => {
    const a = join(root, "a"), b = join(root, "b");
    await registry(a); const file = await registry(b, "reporting");
    await writeFile(config, JSON.stringify({ workspaceRoot: a }));
    const first = await readWorkstreamRegistry(); expect(Object.keys(first.workstreams)).toEqual(["access"]);
    await writeFile(config, JSON.stringify({ workspaceRoot: b }));
    const next = await readWorkstreamRegistry(); expect(next.path).toBe(file); expect(next.revision).not.toBe(first.revision);
    expect(Object.keys(next.workstreams)).toEqual(["reporting"]); expect(Object.keys(next.desks)).toEqual(["pr-review-desk"]);
    await writeFile(file, JSON.stringify({ schema_version: 2, workstreams: {}, desks: {} }));
    expect((await readWorkstreamRegistry()).diagnostics.join()).toContain("unavailable");
    await writeFile(config, "invalid"); expect((await readWorkstreamRegistry()).workstreams).toEqual({});
  });
  it("keeps a desk's match rule and native project and nothing else of the desk entry", async () => {
    const workspace = join(root, "workspace");
    const file = await registry(workspace);
    await writeFile(file, JSON.stringify({ schema_version: 1, workstreams: {}, desks: {
      "pr-review-desk": { name: "PR Review Desk", paseo_project_from: "config/orchestration/pr-desk.yml desk.project", match: "first", row_by: "workspace" },
      "adam-adhoc": { name: "Adam • Ad-hoc", paseo_project: "DESK • ADAM", match: "fallback", row_by: "tab", rows: "one per tab" } } }));
    expect((await readWorkstreamRegistry(workspace)).desks).toEqual({ "pr-review-desk": { name: "PR Review Desk", match: "first" },
      "adam-adhoc": { name: "Adam • Ad-hoc", paseo_project: "DESK • ADAM", match: "fallback" } });
    await writeFile(file, JSON.stringify({ schema_version: 1, workstreams: {}, desks: { odd: { name: "Odd", match: "sometimes" } } }));
    expect((await readWorkstreamRegistry(workspace)).diagnostics.join()).toContain("registry unavailable");
  });

  it("rejects a relative operator-config location instead of inferring cwd", async () => {
    const workspace = join(root, "workspace"); await registry(workspace);
    await writeFile(config, JSON.stringify({ workspaceRoot: workspace }));
    vi.stubEnv("PASEO_HOME", relative(process.cwd(), root));
    const result = await readWorkstreamRegistry();
    expect(result.diagnostics.join()).toContain("path must be absolute"); expect(result.revision).toBeNull();
  });
  it.each(["workspaceRoot", "workspaceRoo\\u0074"])("rejects repeated %s even if its last root is readable", async key => {
    const workspace = join(root, "workspace"); await registry(workspace);
    await writeFile(config, `{"workspaceRoot":${JSON.stringify(workspace)},"${key}":${JSON.stringify(workspace)}}`);
    const result = await readWorkstreamRegistry();
    expect(result.revision).toBeNull(); expect(result.path).toBeNull(); expect(result.diagnostics.join()).toContain("repeat keys");
  });
  it("rejects non-regular and oversized operator files", async () => {
    await mkdir(config);
    expect((await readWorkstreamRegistry()).diagnostics.join()).toContain("regular file");
    await rm(config, { recursive: true });
    await writeFile(config, " ".repeat(2 * 1024 * 1024 + 1));
    expect((await readWorkstreamRegistry()).diagnostics.join()).toContain("at most 2 MiB");
  });
  it("preserves explicit argument then environment precedence; invalid overrides do not fall back", async () => {
    const local = join(root, "local"), env = join(root, "env"), explicit = join(root, "explicit");
    await registry(local, "local"); await registry(env, "environment"); await registry(explicit, "explicit");
    await writeFile(config, JSON.stringify({ workspaceRoot: local }));
    vi.stubEnv("KONTOR_PLUGIN_ASMA_WORKSPACE", env);
    expect(Object.keys((await readWorkstreamRegistry()).workstreams)).toEqual(["environment"]);
    expect(Object.keys((await readWorkstreamRegistry(explicit)).workstreams)).toEqual(["explicit"]);
    expect((await readWorkstreamRegistry("")).workstreams).toEqual({});
    vi.stubEnv("KONTOR_PLUGIN_ASMA_WORKSPACE", "relative");
    expect((await readWorkstreamRegistry()).workstreams).toEqual({});
    expect(Object.keys((await readWorkstreamRegistry(explicit)).workstreams)).toEqual(["explicit"]);
  });
  it("uses local configuration through the actual list handler, ignoring caller path fields", async () => {
    const workspace = join(root, "workspace"); await registry(workspace);
    await writeFile(config, JSON.stringify({ workspaceRoot: workspace }));
    const server = { handle: vi.fn() };
    const settings: PluginSettings<typeof organizationSettingsSchema> = { read: async () => ({ status: "ready", revision: "1", values: DEFAULT_ORGANIZATION_SETTINGS }), subscribe: () => () => {} };
    const stop = contributeReadSources(server, settings, { sources: [], diagnostics: [] });
    const handler = server.handle.mock.calls.find(([rpc]) => rpc.name === listReadSources.name)![1];
    const result = await handler({ workspaceRoot: "/caller/path" });
    expect(result.registry.path).toBe(join(workspace, "config/orchestration/workstreams.json"));
    expect(Object.keys(result.registry.workstreams)).toEqual(["access"]); expect(result.sources).toEqual([]);
    stop();
  });
});
