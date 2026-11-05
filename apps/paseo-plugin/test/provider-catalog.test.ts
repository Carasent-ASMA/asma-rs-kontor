import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { opencodeRouteIds, readProviderCatalog, type CatalogClient } from "../server/provider-catalog";
import type { HostConfigDeps } from "../server/opencode-config";

const dirs: string[] = [];
afterEach(async () => { for (const root of dirs.splice(0)) await rm(root, { recursive: true, force: true }); });

const model = (id: string) => ({ id, label: id, isSelectable: true });
function route(provider: string, models: readonly string[], patch: Partial<ProviderSnapshotEntry> = {}): ProviderSnapshotEntry {
  return { provider, label: provider, enabled: true, status: "ready", models: models.map(model), modes: [], ...patch } as ProviderSnapshotEntry;
}

/** A synthetic host: a snapshot queue, a refresh spy, and no real configuration directory. */
async function host(entries: ProviderSnapshotEntry[] | ProviderSnapshotEntry[][], patch: Partial<HostConfigDeps> = {}) {
  const root = await mkdtemp(join(tmpdir(), "provider-catalog-"));
  dirs.push(root);
  const queue = Array.isArray(entries[0]) ? [...entries as ProviderSnapshotEntry[][]] : [entries as ProviderSnapshotEntry[]];
  const snapshot = vi.fn(async () => ({ entries: queue.length > 1 ? queue.shift()! : queue[0]! }));
  const refresh = vi.fn(async (_options?: { cwd?: string; providers?: string[] }) => ({ requestId: "fixture", acknowledged: true }));
  const client: CatalogClient = { providers: { snapshot, refresh } };
  const deps: HostConfigDeps = { env: {}, home: join(root, "home"), system: false, stopAt: root, platform: "linux", ...patch };
  return { root, client, snapshot, refresh, deps, cwd: join(root, "project") };
}

async function files(root: string, list: Record<string, string>) {
  for (const [path, text] of Object.entries(list)) {
    const file = join(root, path);
    await mkdir(dirname(file), { recursive: true });
    await writeFile(file, text);
  }
}

describe("harness catalogue", () => {
  it("re-reads the Codex list before serving it, and serves the read that follows", async () => {
    const stale = route("codex", ["gpt-5.6-terra"]);
    const fresh = route("codex", ["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]);
    const h = await host([[stale, route("claude", ["claude-opus-5-5"])], [fresh, route("claude", ["claude-opus-5-5"])]]);
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    expect(h.refresh).toHaveBeenCalledExactlyOnceWith({ cwd: h.cwd, providers: ["codex"] });
    expect(catalog.entries.find((entry) => entry.provider === "codex")!.models!.map((entry) => entry.id)).toEqual(["gpt-6.1-sol", "gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"]);
    expect(catalog.problems).toEqual([]);
  });

  it("withholds Codex when the host has no way to re-read its own list, and offers the other routes", async () => {
    const h = await host([route("codex", ["gpt-5.6-terra"]), route("claude", ["claude-opus-5-5"])]);
    const catalog = await readProviderCatalog({ providers: { snapshot: h.snapshot } }, h.cwd, { config: h.deps });
    const codex = catalog.entries.find((entry) => entry.provider === "codex")!;
    expect(codex.models).toEqual([]);
    expect(codex).toMatchObject({ status: "error" });
    expect(catalog.problemFor("codex")).toContain("cannot re-read");
    expect(catalog.entries.find((entry) => entry.provider === "claude")!.models).toHaveLength(1);
  });

  it("withholds Codex when the re-read fails or does not answer, never serving the stale list", async () => {
    const h = await host([route("codex", ["gpt-5.6-terra"]), route("claude", ["claude-opus-5-5"])]);
    h.refresh.mockRejectedValueOnce(new Error("app-server did not start"));
    const failed = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    expect(failed.entries.find((entry) => entry.provider === "codex")!.models).toEqual([]);
    expect(failed.problemFor("codex")).toContain("did not confirm a fresh read");
    expect(JSON.stringify(failed.problems)).not.toContain("app-server");
    h.refresh.mockImplementationOnce(() => new Promise(() => {}));
    const hung = await readProviderCatalog(h.client, h.cwd, { config: h.deps, refreshMs: 20 });
    expect(hung.entries.find((entry) => entry.provider === "codex")!.models).toEqual([]);
  });

  it("does not re-read when the caller says the list is reused", async () => {
    const h = await host([route("codex", ["gpt-5.6-terra"])]);
    const catalog = await readProviderCatalog(h.client, h.cwd, { fresh: false, config: h.deps });
    expect(h.refresh).not.toHaveBeenCalled();
    expect(catalog.entries[0]!.models).toHaveLength(1);
  });

  it("offers only the OpenCode models the operator opened, read from the configuration for that working directory", async () => {
    const h = await host([route("opencode", ["openrouter/acme/one:free", "openrouter/acme/two:free", "deepseek/deepseek-flash", "deepseek/deepseek-v4-pro", "openai/gpt-6.1-terra", "anthropic/claude-haiku-5.5"])]);
    await files(h.root, {
      "home/.config/opencode/opencode.json": JSON.stringify({ enabled_providers: ["openrouter", "deepseek"], provider: { openrouter: { whitelist: ["acme/one:free"] } } }),
      "project/opencode.jsonc": `{ "provider": { "deepseek": { "blacklist": ["deepseek-v4-pro"], }, }, }`,
    });
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    expect(catalog.entries[0]!.models!.map((entry) => entry.id)).toEqual(["openrouter/acme/one:free", "deepseek/deepseek-flash"]);
    expect(catalog.problems).toEqual([]);
  });

  it("withholds the OpenCode route, never its unfiltered catalogue, when the configuration cannot be read", async () => {
    const h = await host([route("opencode", ["openrouter/acme/one:free", "deepseek/deepseek-flash"]), route("codex", ["gpt-6.1-sol"])]);
    await files(h.root, { "project/opencode.json": `{"enabled_providers": "openrouter"}` });
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    const opencode = catalog.entries.find((entry) => entry.provider === "opencode")!;
    expect(opencode.models).toEqual([]);
    expect(opencode).toMatchObject({ status: "error" });
    expect(catalog.problemFor("opencode")).toContain(join(h.cwd, "opencode.json"));
    expect(catalog.problemFor("opencode")).toContain("enabled_providers");
    // The other routes are unaffected, and Codex was re-read.
    expect(catalog.entries.find((entry) => entry.provider === "codex")!.models!.map((entry) => entry.id)).toEqual(["gpt-6.1-sol"]);
  });

  it("offers OpenAI models only through Codex and Anthropic models only through Claude, whatever the other route's list says", async () => {
    const h = await host([
      route("codex", ["gpt-6.1-sol", "gpt-6-astra", "claude-opus-5-5"]),
      route("claude", ["claude-opus-5-5", "gpt-6.1-sol"]),
      route("opencode", ["openrouter/openai/gpt-6.1-terra", "openrouter/anthropic/claude-haiku-5.5", "zai-coding-plan/glm-5.3", "deepseek/deepseek-flash"]),
      route("cursor", ["gpt-6-astra", "claude-opus-5-5", "composer-2"]),
      route("copilot", ["claude-sonnet-5", "gpt-6-astra"]),
    ]);
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    const shown = Object.fromEntries(catalog.entries.map((entry) => [entry.provider, entry.models!.map((model) => model.id)]));
    expect(shown.codex).toEqual(["gpt-6.1-sol", "gpt-6-astra"]);
    expect(shown.claude).toEqual(["claude-opus-5-5"]);
    // The OpenCode ids name OpenAI and Anthropic models, so they are OpenAI's and Anthropic's and are not offered there.
    expect(shown.opencode).toEqual(["zai-coding-plan/glm-5.3", "deepseek/deepseek-flash"]);
    expect(shown.cursor).toEqual(["composer-2"]);
    expect(shown.copilot).toEqual([]);
  });

  it("reads an account route as its harness: a Claude account keeps Anthropic models and a Codex account keeps OpenAI models", async () => {
    const h = await host([route("claude-work", ["claude-opus-5-5", "gpt-6.1-sol"]), route("codex-personal", ["gpt-6.1-sol", "claude-opus-5-5"])]);
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps, overrides: { "claude-work": { extends: "claude" }, "codex-personal": { extends: "codex" } } });
    const shown = Object.fromEntries(catalog.entries.map((entry) => [entry.provider, entry.models!.map((model) => model.id)]));
    expect(shown["claude-work"]).toEqual(["claude-opus-5-5"]);
    expect(shown["codex-personal"]).toEqual(["gpt-6.1-sol"]);
  });

  it("keeps DeepSeek on its own harness and on OpenCode, and GLM on its agent and on OpenCode, without merging the routes", async () => {
    const h = await host([
      route("deepseek-harness", ["deepseek-flash", "deepseek-v4-flash"]),
      route("glm-acp-agent", ["glm-5.3", "glm-5.3-flash"]),
      route("opencode", ["deepseek/deepseek-flash", "zai-coding-plan/glm-5.3"]),
    ]);
    await files(h.root, { "project/opencode.json": JSON.stringify({ enabled_providers: ["deepseek", "zai-coding-plan"] }) });
    const catalog = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    const shown = Object.fromEntries(catalog.entries.map((entry) => [entry.provider, entry.models!.map((model) => model.id)]));
    expect(shown["deepseek-harness"]).toEqual(["deepseek-flash", "deepseek-v4-flash"]);
    expect(shown["glm-acp-agent"]).toEqual(["glm-5.3", "glm-5.3-flash"]);
    expect(shown.opencode).toEqual(["deepseek/deepseek-flash", "zai-coding-plan/glm-5.3"]);
    expect(catalog.entries).toHaveLength(3);
  });

  it("applies the same OpenCode configuration to a route the daemon derives from the OpenCode harness", async () => {
    const h = await host([route("oc-team", ["openrouter/acme/one:free", "openrouter/acme/two:free"])]);
    await files(h.root, { "project/opencode.json": JSON.stringify({ enabled_providers: ["openrouter"], provider: { openrouter: { whitelist: ["acme/two:free"] } } }) });
    const filtered = await readProviderCatalog(h.client, h.cwd, { config: h.deps, overrides: { "oc-team": { extends: "opencode" } } });
    expect(filtered.entries[0]!.models!.map((entry) => entry.id)).toEqual(["openrouter/acme/two:free"]);
    // Without the daemon's overrides only `opencode` itself is known to run the harness.
    const unknown = await readProviderCatalog(h.client, h.cwd, { config: h.deps });
    expect(unknown.entries[0]!.models).toHaveLength(2);
  });

  it("resolves a chain of derived routes and always includes the harness route itself", () => {
    expect([...opencodeRouteIds(null)].sort()).toEqual(["opencode"]);
    const routes = opencodeRouteIds({ "oc-a": { extends: "opencode" }, "oc-b": { extends: "oc-a" }, "oc-other": { extends: "claude" }, claude: {} });
    expect([...routes].sort()).toEqual(["oc-a", "oc-b", "opencode"]);
  });
});
