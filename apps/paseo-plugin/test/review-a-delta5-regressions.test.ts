import { chmod, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import type { HostConfigDeps } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regression tests for review-a delta-5 E2 on 3ad2de37: a config leaf behind a broken directory link is an
 * unreadable source, not ordinary absence, for the default location and for OPENCODE_CONFIG. Every directory,
 * link and document is synthetic, under a temporary root; the real filesystem is used for the loader's checks.
 */
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

const source = (): ProviderSnapshotEntry => ({
  provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
  models: [{ id: "deepseek/hidden", label: "DeepSeek", isSelectable: true }, { id: "other/permitted", label: "Other", isSelectable: true }],
} as ProviderSnapshotEntry);
const client = { providers: { snapshot: async () => ({ entries: [source()] }) } };
const policy = JSON.stringify({ disabled_providers: ["deepseek"] });
const deps = (root: string, home: string, env: Record<string, string> = {}): HostConfigDeps =>
  ({ env, home, system: false, stopAt: root, platform: "linux" });
const ids = (catalog: Awaited<ReturnType<typeof readProviderCatalog>>) => catalog.entries[0]!.models!.map((model) => model.id);

async function tree() {
  const root = await mkdtemp(join(tmpdir(), "review-a-e2-")), home = join(root, "home"), project = join(root, "project");
  roots.push(root);
  await mkdir(home);
  await mkdir(project);
  return { root, home, project };
}

describe("review-a delta-5 regressions", () => {
  it("E2 a broken default config-directory link withholds the route", async () => {
    const { root, home, project } = await tree();
    const target = join(root, "target"), link = join(home, ".config", "opencode");
    await mkdir(join(home, ".config"));
    await mkdir(target);
    await writeFile(join(target, "opencode.json"), policy);
    await symlink(target, link, "dir");

    const config = deps(root, home);
    expect(ids(await readProviderCatalog(client, project, { config }))).toEqual(["other/permitted"]);

    await rm(target, { recursive: true });
    const after = await readProviderCatalog(client, project, { config });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("ENOENT");
  });

  it("E2 a broken ancestor of the default config file withholds the route", async () => {
    const { root, home, project } = await tree();
    const target = join(root, "target");
    await mkdir(join(target, "opencode"), { recursive: true });
    await writeFile(join(target, "opencode", "opencode.json"), policy);
    await symlink(target, join(home, ".config"), "dir");

    const config = deps(root, home);
    expect(ids(await readProviderCatalog(client, project, { config }))).toEqual(["other/permitted"]);

    await rm(target, { recursive: true });
    const after = await readProviderCatalog(client, project, { config });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("ENOENT");
  });

  it("E2 a broken extra-source directory link withholds the route", async () => {
    const { root, home, project } = await tree();
    const global = join(root, "global"), target = join(root, "target"), alias = join(global, "extra-directory");
    await mkdir(global);
    await mkdir(target);
    await writeFile(join(target, "policy.json"), policy);
    await symlink(target, alias, "dir");
    const config = deps(root, home, { OPENCODE_CONFIG_DIR: global, OPENCODE_CONFIG: join(alias, "policy.json") });

    expect(ids(await readProviderCatalog(client, project, { config }))).toEqual(["other/permitted"]);

    await rm(target, { recursive: true });
    const after = await readProviderCatalog(client, project, { config });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("ENOENT");
  });

  it("E2 a broken ancestor of the extra source withholds the route", async () => {
    const { root, home, project } = await tree();
    const global = join(root, "global"), target = join(root, "target");
    await mkdir(global);
    await mkdir(join(target, "inner"), { recursive: true });
    await writeFile(join(target, "inner", "policy.json"), policy);
    await symlink(target, join(global, "outer"), "dir");
    const config = deps(root, home, { OPENCODE_CONFIG_DIR: global, OPENCODE_CONFIG: join(global, "outer", "inner", "policy.json") });

    expect(ids(await readProviderCatalog(client, project, { config }))).toEqual(["other/permitted"]);

    await rm(target, { recursive: true });
    const after = await readProviderCatalog(client, project, { config });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("ENOENT");
  });

  it("E2 guards: genuine absence at every depth stays absent with no problem", async () => {
    const { root, home, project } = await tree();
    const both = ["deepseek/hidden", "other/permitted"];

    // A real default directory with no document.
    await mkdir(join(home, ".config", "opencode"), { recursive: true });
    const empty = await readProviderCatalog(client, project, { config: deps(root, home) });
    expect(ids(empty)).toEqual(both);
    expect(empty.problems).toEqual([]);

    // A missing intermediate default directory under a real home.
    const shallowHome = join(root, "shallow-home");
    await mkdir(shallowHome);
    const shallow = await readProviderCatalog(client, project, { config: deps(root, shallowHome) });
    expect(ids(shallow)).toEqual(both);
    expect(shallow.problems).toEqual([]);

    // A missing intermediate directory under a real explicit root.
    const global = join(root, "global");
    await mkdir(global);
    const missingMiddle = await readProviderCatalog(client, project, {
      config: deps(root, home, { OPENCODE_CONFIG_DIR: global, OPENCODE_CONFIG: join(global, "missing", "inner", "policy.json") }),
    });
    expect(ids(missingMiddle)).toEqual(both);
    expect(missingMiddle.problems).toEqual([]);

    // A missing name under a valid directory link.
    const realDirectory = join(root, "real-directory");
    await mkdir(realDirectory);
    await symlink(realDirectory, join(global, "alias"), "dir");
    const linked = await readProviderCatalog(client, project, {
      config: deps(root, home, { OPENCODE_CONFIG_DIR: global, OPENCODE_CONFIG: join(global, "alias", "policy.json") }),
    });
    expect(ids(linked)).toEqual(both);
    expect(linked.problems).toEqual([]);
  });

  it("E2 guard: an lstat error after ENOENT stays closed", async () => {
    const { root, home, project } = await tree();
    const parent = join(root, "blocked"), file = join(parent, "missing", "policy.json");
    await mkdir(join(parent, "missing"), { recursive: true });
    // The read sees ENOENT; by the time the classifier checks the path, search on the ancestor is denied.
    const config: HostConfigDeps = {
      ...deps(root, home, { OPENCODE_CONFIG: file }),
      readFile: async (path: string) => {
        if (path === file) { await chmod(parent, 0); throw Object.assign(new Error("absent"), { code: "ENOENT" }); }
        return readFile(path, "utf8");
      },
    };
    try {
      const after = await readProviderCatalog(client, project, { config });
      expect(ids(after)).toEqual([]);
      expect(after.problemFor("opencode")).toContain(file);
    } finally { await chmod(parent, 0o700); }
  });
});
