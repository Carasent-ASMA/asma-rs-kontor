import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { readOpenCodeConfig, type HostConfigDeps } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regression tests for review-a delta-3 E1 on fb92d545: an explicitly selected config directory that becomes
 * unavailable must withhold the OpenCode route, while an absent default location stays ordinary absence.
 * Every directory and symlink is synthetic, under a temporary root.
 */
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

const source = (): ProviderSnapshotEntry => ({
  provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
  models: [{ id: "deepseek/allowed", label: "Allowed", isSelectable: true }, { id: "other/allowed", label: "Other", isSelectable: true }],
} as ProviderSnapshotEntry);
const client = { providers: { snapshot: async () => ({ entries: [source()] }) } };
const deps = (root: string, selected?: string): HostConfigDeps =>
  ({ env: selected ? { OPENCODE_CONFIG_DIR: selected } : {}, home: join(root, "home"), system: false, stopAt: root, platform: "linux" });
const ids = (catalog: Awaited<ReturnType<typeof readProviderCatalog>>) => catalog.entries[0]!.models!.map((model) => model.id);

describe("review-a delta-3 regressions", () => {
  it("E1 a removed explicit config directory withholds the OpenCode route", async () => {
    const root = await mkdtemp(join(tmpdir(), "review-a-e1-")); roots.push(root);
    const selected = join(root, "configured-global");
    await mkdir(selected);
    await writeFile(join(selected, "opencode.json"), JSON.stringify({ disabled_providers: ["deepseek"] }));

    expect(ids(await readProviderCatalog(client, join(root, "project"), { config: deps(root, selected) }))).toEqual(["other/allowed"]);
    await rm(selected, { recursive: true });
    const after = await readProviderCatalog(client, join(root, "project"), { config: deps(root, selected) });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("could not be read as a directory");
  });

  it("E1 a broken symlink target withholds the OpenCode route", async () => {
    const root = await mkdtemp(join(tmpdir(), "review-a-e1-")); roots.push(root);
    const target = join(root, "configured-target"), selected = join(root, "configured-global");
    await mkdir(target);
    await writeFile(join(target, "opencode.json"), JSON.stringify({ disabled_providers: ["deepseek"] }));
    await symlink(target, selected, "dir");

    expect(ids(await readProviderCatalog(client, join(root, "project"), { config: deps(root, selected) }))).toEqual(["other/allowed"]);
    await rm(target, { recursive: true });
    const after = await readProviderCatalog(client, join(root, "project"), { config: deps(root, selected) });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("could not be read as a directory");
  });

  it("E1 an absent default config location stays absent and adds no problem", async () => {
    const root = await mkdtemp(join(tmpdir(), "review-a-e1-")); roots.push(root);
    expect(await readOpenCodeConfig(join(root, "project"), deps(root))).toEqual({ documents: [] });
    const catalog = await readProviderCatalog(client, join(root, "project"), { config: deps(root) });
    expect(ids(catalog)).toEqual(["deepseek/allowed", "other/allowed"]);
    expect(catalog.problems).toEqual([]);
  });
});
