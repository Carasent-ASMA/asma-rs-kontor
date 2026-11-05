import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { opencodeModelVisible, opencodeSelection, type HostConfigDeps } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regressions for the independent audit on fa3293c9. AUD-F1: a present selection key of the wrong shape is a named
 * configuration problem, never an absent setting. AUD-F2: an explicitly named OPENCODE_CONFIG source is required
 * even when it is genuinely missing. Every path, document and model id is synthetic, under a temporary root.
 */
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

const models = ["deepseek/hidden", "other/allowed"];
const client = { providers: { snapshot: async () => ({ entries: [{
  provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
  models: models.map((id) => ({ id, label: id, isSelectable: true })),
} as ProviderSnapshotEntry] }) } };
const azureClient = { providers: { snapshot: async () => ({ entries: [{
  provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
  models: ["azure-cognitive-services/deepseek-permitted", "azure-cognitive-services/deepseek-extra"]
    .map((id) => ({ id, label: id, isSelectable: true })),
} as ProviderSnapshotEntry] }) } };
const host = (root: string): HostConfigDeps => ({ home: join(root, "home"), env: {}, system: false, stopAt: root });
const ids = (catalog: Awaited<ReturnType<typeof readProviderCatalog>>) => catalog.entries.flatMap((entry) => (entry.models ?? []).map((model) => model.id));
const invalidContainers = [
  { label: "null", value: null }, { label: "array", value: [] }, { label: "string", value: "invalid-provider-map" },
];

async function freshTree() {
  const root = await mkdtemp(join(tmpdir(), "aud-failclosed-"));
  roots.push(root);
  return { root, config: join(root, "opencode.json") };
}

describe("audit fail-closed regressions", () => {
  it.each(invalidContainers)("AUD-F1 rejects an invalid provider container $label", async ({ value: invalid }) => {
    const { root, config } = await freshTree();
    await writeFile(config, JSON.stringify({ provider: { deepseek: { whitelist: [] } } }));
    expect(ids(await readProviderCatalog(client, root, { config: host(root) }))).toEqual(["other/allowed"]);

    await writeFile(config, JSON.stringify({ provider: invalid }));
    const read = await readProviderCatalog(client, root, { config: host(root) });
    expect(ids(read)).toEqual([]);
    expect(read.problemFor("opencode")).toContain(config);
  });

  it.each(invalidContainers)("AUD-F1 rejects an invalid providers container $label", async ({ value: invalid }) => {
    const { root, config } = await freshTree();
    await writeFile(config, JSON.stringify({ providers: { deepseek: { whitelist: [] } } }));
    expect(ids(await readProviderCatalog(client, root, { config: host(root) }))).toEqual(["other/allowed"]);

    await writeFile(config, JSON.stringify({ providers: invalid }));
    const read = await readProviderCatalog(client, root, { config: host(root) });
    expect(ids(read)).toEqual([]);
    expect(read.problemFor("opencode")).toContain(config);
  });

  it("AUD-F1 sweep rejects every present selection key of the wrong shape", async () => {
    const { root, config } = await freshTree();
    const wrongShapes = [
      { enabled_providers: null }, { enabled_providers: "deepseek" }, { enabled_providers: { deepseek: [] } },
      { disabled_providers: null }, { disabled_providers: 7 }, { disabled_providers: { deepseek: [] } },
      { provider: { deepseek: null } }, { provider: { deepseek: [] } }, { provider: { deepseek: "invalid-entry" } },
      { provider: { deepseek: { whitelist: null } } }, { provider: { deepseek: { whitelist: "invalid-list" } } },
      { provider: { deepseek: { blacklist: {} } } }, { providers: { deepseek: { whitelist: 5 } } },
    ];
    for (const info of wrongShapes) {
      await writeFile(config, JSON.stringify(info));
      const read = await readProviderCatalog(client, root, { config: host(root) });
      expect(ids(read), JSON.stringify(info)).toEqual([]);
      expect(read.problemFor("opencode"), JSON.stringify(info)).toContain(config);
    }
  });

  it("AUD guards keep provider aliases and literal policy characters selecting", () => {
    const alias = opencodeSelection([{ path: "/synthetic/config.json", info: {
      provider: { azure: { whitelist: ["allowed"] } },
      disabled_providers: ["google-vertex"],
    } }]);
    if ("problem" in alias) throw new Error(alias.problem);
    expect(opencodeModelVisible("azure-cognitive-services/allowed", alias.selection)).toBe(true);
    expect(opencodeModelVisible("azure-cognitive-services/hidden", alias.selection)).toBe(false);
    expect(opencodeModelVisible("google-vertex-anthropic/hidden", alias.selection)).toBe(false);

    const literal = opencodeSelection([{ path: "/synthetic/config.json", info: {
      policies: [{ action: "provider.use", resource: "house+lab", effect: "deny" }],
    } }]);
    if ("problem" in literal) throw new Error(literal.problem);
    expect(opencodeModelVisible("house+lab/hidden", literal.selection)).toBe(false);
    expect(opencodeModelVisible("houselab/allowed", literal.selection)).toBe(true);
    expect(opencodeModelVisible("houseeelab/allowed", literal.selection)).toBe(true);
  });

  it("AUD-F2 withholds when a named OPENCODE_CONFIG file is removed", async () => {
    const { root, config } = await freshTree();
    const deps = { ...host(root), env: { OPENCODE_CONFIG: config } };
    await writeFile(config, JSON.stringify({ disabled_providers: ["deepseek"] }));
    expect(ids(await readProviderCatalog(client, root, { config: deps }))).toEqual(["other/allowed"]);

    await rm(config);
    const read = await readProviderCatalog(client, root, { config: deps });
    expect(ids(read)).toEqual([]);
    expect(read.problemFor("opencode")).toContain(config);
  });

  it("AUD-F2 withholds when the parent of a named OPENCODE_CONFIG file is removed", async () => {
    const { root } = await freshTree();
    const parent = join(root, "extra");
    await mkdir(parent);
    const config = join(parent, "explicit.json");
    const deps = { ...host(root), env: { OPENCODE_CONFIG: config } };
    await writeFile(config, JSON.stringify({ disabled_providers: ["deepseek"] }));
    expect(ids(await readProviderCatalog(client, root, { config: deps }))).toEqual(["other/allowed"]);

    await rm(parent, { recursive: true });
    const read = await readProviderCatalog(client, root, { config: deps });
    expect(ids(read)).toEqual([]);
    expect(read.problemFor("opencode")).toContain(config);
  });

  it("D8-F1 rejects malformed entries shadowed by a later container", async () => {
    const { root, config } = await freshTree();
    await writeFile(config, JSON.stringify({ provider: { deepseek: { whitelist: [] } } }));
    expect(ids(await readProviderCatalog(client, root, { config: host(root) }))).toEqual(["other/allowed"]);

    const shadowed = [
      { provider: { deepseek: { whitelist: "wrong-shape" } }, providers: { deepseek: {} } },
      { provider: { deepseek: null }, providers: { deepseek: {} } },
      { provider: { deepseek: { blacklist: "wrong-shape" } }, providers: { deepseek: {} } },
      { provider: { deepseek: {} }, providers: { deepseek: { whitelist: "wrong-shape" } } },
    ];
    for (const info of shadowed) {
      await writeFile(config, JSON.stringify(info));
      const read = await readProviderCatalog(client, root, { config: host(root) });
      expect(ids(read), JSON.stringify(info)).toEqual([]);
      expect(read.problemFor("opencode"), JSON.stringify(info)).toContain(config);
    }
  });

  it("D8-F1 rejects malformed entries across container and alias collisions", async () => {
    const { root, config } = await freshTree();
    const collisions = [
      { provider: { house: { whitelist: "wrong-shape" } }, providers: { house: {} } },
      { provider: { house: {} }, providers: { house: { whitelist: "wrong-shape" } } },
      { provider: { "azure-cognitive-services": { whitelist: "wrong-shape" } }, providers: { azure: {} } },
      { provider: { azure: { whitelist: "wrong-shape" } }, providers: { "azure-cognitive-services": {} } },
      { provider: { azure: { whitelist: ["one"] }, "azure-cognitive-services": { whitelist: "wrong-shape" } } },
    ];
    for (const info of collisions) {
      await writeFile(config, JSON.stringify(info));
      const read = await readProviderCatalog(client, root, { config: host(root) });
      expect(ids(read), JSON.stringify(info)).toEqual([]);
      expect(read.problemFor("opencode"), JSON.stringify(info)).toContain(config);
    }
  });

  it("D8-F1 keeps the declared precedence for valid entries", () => {
    const path = "/synthetic/policy.fixture";
    const read = opencodeSelection([{ path, info: {
      provider: { house: { whitelist: ["one"] } }, providers: { house: { blacklist: ["two"] } },
    } }]);
    if ("problem" in read) throw new Error(read.problem);
    expect(opencodeModelVisible("house/one", read.selection)).toBe(true);
    expect(opencodeModelVisible("house/two", read.selection)).toBe(false);
    expect(opencodeModelVisible("house/three", read.selection)).toBe(true);
    expect(opencodeModelVisible("other/two", read.selection)).toBe(true);

    // A valid alias collision keeps the later declaration: providers' empty entry replaces the alias filter.
    const alias = opencodeSelection([{ path, info: {
      provider: { "azure-cognitive-services": { whitelist: ["one"] } }, providers: { azure: {} },
    } }]);
    if ("problem" in alias) throw new Error(alias.problem);
    expect(opencodeModelVisible("azure-cognitive-services/one", alias.selection)).toBe(true);
    expect(opencodeModelVisible("azure-cognitive-services/two", alias.selection)).toBe(true);
  });

  it("D9-F1 keeps the canonical legacy provider declaration over its alias in both orders", () => {
    const path = "/synthetic/merge.fixture";
    const pairs = [["azure", "azure-cognitive-services"], ["google-vertex", "google-vertex-anthropic"]] as const;
    for (const [canonical, alias] of pairs) {
      const orders = [
        { provider: { [canonical]: { whitelist: ["one"] }, [alias]: { blacklist: ["two"] } } },
        { provider: { [alias]: { blacklist: ["two"] }, [canonical]: { whitelist: ["one"] } } },
      ];
      for (const info of orders) {
        const read = opencodeSelection([{ path, info }]);
        if ("problem" in read) throw new Error(read.problem);
        expect(opencodeModelVisible(`${alias}/one`, read.selection), JSON.stringify(info)).toBe(true);
        expect(opencodeModelVisible(`${alias}/two`, read.selection), JSON.stringify(info)).toBe(false);
        expect(opencodeModelVisible(`${alias}/three`, read.selection), JSON.stringify(info)).toBe(false);
      }
      const alone = opencodeSelection([{ path, info: { provider: { [alias]: { whitelist: ["one"] } } } }]);
      if ("problem" in alone) throw new Error(alone.problem);
      expect(opencodeModelVisible(`${alias}/one`, alone.selection)).toBe(true);
      expect(opencodeModelVisible(`${alias}/two`, alone.selection)).toBe(false);
    }
  });

  it("D9-F1 keeps the canonical Azure whitelist on the route", async () => {
    const { root, config } = await freshTree();
    await writeFile(config, JSON.stringify({ provider: {
      azure: { whitelist: ["deepseek-permitted"] }, "azure-cognitive-services": { blacklist: ["unused"] },
    } }));
    const read = await readProviderCatalog(azureClient, root, { config: host(root) });
    expect(ids(read)).toEqual(["azure-cognitive-services/deepseek-permitted"]);
    expect(read.problems).toEqual([]);
  });

  it("D9-F1 lets the canonical providers container replace a legacy alias entry", () => {
    const read = opencodeSelection([{ path: "/synthetic/merge.fixture", info: {
      provider: { "azure-cognitive-services": { blacklist: ["two"] } }, providers: { azure: { whitelist: ["one"] } },
    } }]);
    if ("problem" in read) throw new Error(read.problem);
    expect(opencodeModelVisible("azure-cognitive-services/one", read.selection)).toBe(true);
    expect(opencodeModelVisible("azure-cognitive-services/two", read.selection)).toBe(false);
    expect(opencodeModelVisible("azure-cognitive-services/three", read.selection)).toBe(false);
  });

  it("D9-F1 inherits absent keys across documents without undefined filters", () => {
    const path = "/synthetic/merge.fixture";
    const read = opencodeSelection([
      { path, info: { provider: { house: { whitelist: ["one", "three"], blacklist: ["one"] } } } },
      { path, info: { providers: { house: { blacklist: ["two"] } } } },
    ]);
    if ("problem" in read) throw new Error(read.problem);
    expect(read.selection.filters.get("house")).toEqual({ whitelist: ["one", "three"], blacklist: ["two"] });
  });
});
