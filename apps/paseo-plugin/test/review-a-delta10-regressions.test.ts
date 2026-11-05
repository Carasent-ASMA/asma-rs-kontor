import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { modelFamily, modelVendor } from "../shared/model-vendor";
import { normalizeOpenCodeProviderId } from "../server/opencode-config";
import { harnessBase, readProviderCatalog, routeEnvironment } from "../server/provider-catalog";

/**
 * Review-a delta-10, D10-F1. A plain-object table read by a configuration or catalogue id must answer owned
 * entries only: `constructor`, `toString`, `hasOwnProperty`, `__proto__` and `valueOf` are inherited
 * Object.prototype members, not provider aliases, model families, model authors or route overrides. Every path,
 * document and id here is synthetic.
 */
const INHERITED = ["constructor", "toString", "hasOwnProperty", "__proto__", "valueOf"] as const;
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

const ids = (catalog: Awaited<ReturnType<typeof readProviderCatalog>>) => catalog.entries.flatMap((entry) => (entry.models ?? []).map((model) => model.id));

function snapshot(models: readonly string[]) {
  return { providers: { snapshot: async () => ({ entries: [{
    provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
    models: models.map((id) => ({ id, label: id, isSelectable: true })),
  } as ProviderSnapshotEntry] }) } };
}

describe("D10-F1 inherited Object.prototype names are not provider aliases", () => {
  it.each(INHERITED)("normalizes the unknown provider id %s unchanged", (id) => {
    expect(normalizeOpenCodeProviderId(id)).toBe(id);
  });

  it.each(INHERITED)("keeps an enabled %s provider and its valid model on the route", async (id) => {
    const root = await mkdtemp(join(tmpdir(), "review-a-delta10-"));
    roots.push(root);
    await mkdir(join(root, "home"));
    await writeFile(join(root, "opencode.json"), JSON.stringify({ enabled_providers: [id] }));
    const read = await readProviderCatalog(snapshot([`${id}/one`, "house/two"]), root, {
      config: { home: join(root, "home"), env: {}, system: false, stopAt: root },
    });
    expect(ids(read), id).toEqual([`${id}/one`]);
    expect(read.problems, id).toEqual([]);
  });
});

describe("D10-F1 inherited Object.prototype names are not model families or authors", () => {
  it.each(INHERITED)("does not classify the family word %s", (name) => {
    expect(modelFamily(name)).toBeUndefined();
  });

  it("does not classify a model word or author named constructor", () => {
    expect(modelVendor("house/constructor")).toBe("unknown");
    expect(modelVendor("openrouter/constructor/house-model")).toBe("unknown");
  });

  it("does not read __proto__ as an OpenRouter author", () => {
    expect(modelVendor("openrouter/__proto__/house-model")).toBe("unknown");
  });
});

describe("D10-F1 inherited prototype entries are not route overrides", () => {
  it("ignores an inherited extends and env entry", () => {
    const overrides: Record<string, unknown> = Object.create({
      house: { extends: "codex", env: { OPENCODE_CONFIG: "/synthetic/inherited-opencode.json" } },
    });
    expect(harnessBase("house", overrides)).toBe("house");
    expect(routeEnvironment("house", overrides)).toEqual({});
  });
});
