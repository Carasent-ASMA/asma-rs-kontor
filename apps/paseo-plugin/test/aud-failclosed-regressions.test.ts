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
});
