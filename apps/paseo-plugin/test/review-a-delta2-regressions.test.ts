import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { opencodeConfigPaths } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regression tests for review-a delta-2 D5 on 8535d456. They use only synthetic directories and files under a
 * temporary root: a real global directory, a symlink to it, and a parent document that denies one provider.
 */
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

async function tree() {
  const root = await mkdtemp(join(tmpdir(), "review-a-d5-")), global = join(root, "global"), alias = join(root, "global-link");
  roots.push(root);
  await mkdir(global);
  await symlink(global, alias, "dir");
  await writeFile(join(global, "opencode.json"), "{}");
  // The parent of both paths hides DeepSeek; native skips this walk when the Location is the global directory.
  await writeFile(join(root, "opencode.json"), JSON.stringify({ disabled_providers: ["deepseek"] }));
  return { root, global, alias };
}

const host = (root: string, global: string) => ({ env: { OPENCODE_CONFIG_DIR: global }, home: join(root, "home"), system: false, stopAt: root, platform: "linux" as const });
const ids = async (root: string, global: string, cwd: string) => {
  const catalog = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [{ provider: "opencode", label: "OpenCode", enabled: true, status: "ready",
    models: [{ id: "deepseek/allowed", label: "Allowed", isSelectable: true }], modes: [] }] }) } }, cwd, { config: host(root, global) });
  return catalog.entries[0]!.models!.map((model) => model.id);
};

describe("review-a delta-2 regressions", () => {
  it("D5a treats a symlink request directory as the configured global Location", async () => {
    const { root, global, alias } = await tree();
    expect(await realpath(alias)).toBe(await realpath(global));
    expect(resolve(alias)).not.toBe(resolve(global));
    expect(await ids(root, global, alias)).toEqual(["deepseek/allowed"]);

    // The canonical comparison is only for Location identity; a physically distinct project is still walked.
    const project = join(root, "project"); await mkdir(project);
    await writeFile(join(project, "opencode.json"), JSON.stringify({ disabled_providers: ["deepseek"] }));
    expect(opencodeConfigPaths(project, host(root, global))).toContain(join(project, "opencode.json"));
    expect(await ids(root, global, project)).toEqual([]);
  });

  it("D5b treats a symlinked configured global directory as the same Location", async () => {
    const { root, global, alias } = await tree();
    expect(await ids(root, alias, global)).toEqual(["deepseek/allowed"]);
  });
});
