import { lstat, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { readOpenCodeConfig, type HostConfigDeps } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regression tests for review-a delta-4 E1 on 7f220dd4: ENOENT may be treated as ordinary absence only when no
 * directory entry exists, and a selected directory that disappears between the checks is a config problem.
 * Every directory, file and symlink is synthetic, under a temporary root.
 */
const roots: string[] = [];
afterEach(async () => { for (const root of roots.splice(0)) await rm(root, { recursive: true, force: true }); });

const source = (): ProviderSnapshotEntry => ({
  provider: "opencode", label: "OpenCode", enabled: true, status: "ready", modes: [],
  models: [{ id: "deepseek/hidden", label: "DeepSeek", isSelectable: true }, { id: "other/permitted", label: "Other", isSelectable: true }],
} as ProviderSnapshotEntry);
const client = { providers: { snapshot: async () => ({ entries: [source()] }) } };
const deps = (root: string, selected?: string): HostConfigDeps =>
  ({ env: selected ? { OPENCODE_CONFIG_DIR: selected } : {}, home: join(root, "home"), system: false, stopAt: root, platform: "linux" });
const ids = (catalog: Awaited<ReturnType<typeof readProviderCatalog>>) => catalog.entries[0]!.models!.map((model) => model.id);
const policy = JSON.stringify({ disabled_providers: ["deepseek"] });

async function tree() {
  const root = await mkdtemp(join(tmpdir(), "review-a-e1-")), selected = join(root, "selected"), project = join(root, "project");
  roots.push(root);
  await mkdir(selected);
  await mkdir(project);
  return { root, selected, project };
}

describe("review-a delta-4 regressions", () => {
  it("E1 race: a directory removed between the check and the read withholds the OpenCode route", async () => {
    const { root, selected, project } = await tree();
    const file = join(selected, "opencode.json");
    await writeFile(file, policy);

    // The restrictive source works while it exists.
    const base = deps(root, selected);
    expect(ids(await readProviderCatalog(client, project, { config: base }))).toEqual(["other/permitted"]);

    // The injected read removes the directory after the directory check succeeded and before the file is opened.
    let removed = false;
    const racing: HostConfigDeps = {
      ...base,
      readFile: async (path: string) => {
        if (path === file && !removed) { removed = true; await rm(selected, { recursive: true }); }
        return readFile(path, "utf8");
      },
    };
    const after = await readProviderCatalog(client, project, { config: racing });
    expect(removed).toBe(true);
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain("could not be read as a directory");
  });

  it("E1 file: a dangling opencode.json symlink inside an existing explicit directory withholds the route", async () => {
    const { root, selected, project } = await tree();
    const target = join(root, "policy.json"), file = join(selected, "opencode.json");
    await writeFile(target, policy);
    await symlink(target, file);

    // The restrictive source works while its link target exists.
    expect(ids(await readProviderCatalog(client, project, { config: deps(root, selected) }))).toEqual(["other/permitted"]);

    await rm(target);
    // The directory and the entry still exist; only the link target is gone.
    expect((await lstat(selected)).isDirectory()).toBe(true);
    expect((await lstat(file)).isSymbolicLink()).toBe(true);

    const after = await readProviderCatalog(client, project, { config: deps(root, selected) });
    expect(ids(after)).toEqual([]);
    expect(after.problemFor("opencode")).toContain(file);
  });

  it("E1 guard: an absent file in an existing explicit directory stays absent", async () => {
    const { root, selected, project } = await tree();
    expect(await readOpenCodeConfig(project, deps(root, selected))).toEqual({ documents: [] });
    const catalog = await readProviderCatalog(client, project, { config: deps(root, selected) });
    expect(ids(catalog)).toEqual(["deepseek/hidden", "other/permitted"]);
    expect(catalog.problems).toEqual([]);
  });

  it("E1 helper: an invented explicit directory still applies its document without bypassing filters", async () => {
    const selected = "/synthetic/selected", project = "/synthetic/project";
    const read = await readOpenCodeConfig(project, {
      env: { OPENCODE_CONFIG_DIR: selected }, home: "/synthetic", system: false,
      stopAt: "/synthetic", isDirectory: async () => true,
      readFile: async (path: string) => {
        if (path === `${selected}/opencode.json`) return policy;
        throw Object.assign(Error("missing"), { code: "ENOENT" });
      },
    });
    if ("problem" in read) throw Error(read.problem);
    expect(read.documents.map((document) => document.info.disabled_providers)).toEqual([["deepseek"]]);
  });
});
