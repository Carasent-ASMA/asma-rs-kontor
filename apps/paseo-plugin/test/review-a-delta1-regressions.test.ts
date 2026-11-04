import { describe, expect, it, vi } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { opencodeConfigPaths, type HostConfigDeps } from "../server/opencode-config";
import { readProviderCatalog } from "../server/provider-catalog";

/**
 * Regression tests for review-a delta-1 N1 and N2 on f5b4a23f. The host, config documents and model ids are
 * synthetic; no live directory, whitelist or model list is read.
 */
const cwd = "/synthetic/project";
const absent = async (): Promise<string> => { throw Object.assign(new Error("absent"), { code: "ENOENT" }); };
const host: HostConfigDeps = { env: {}, home: "/synthetic/home", system: false, stopAt: "/synthetic", platform: "linux", readFile: absent };
const route = (provider: string, ids: readonly string[], patch: Partial<ProviderSnapshotEntry> = {}): ProviderSnapshotEntry => ({
  provider, label: provider, enabled: true, status: "ready",
  models: ids.map((id) => ({ id, label: id, isSelectable: true })), modes: [], ...patch,
} as ProviderSnapshotEntry);

describe("review-a delta-1 regressions", () => {
  it("N1 withholds an unclassified account from the final snapshot, whether it is late, new or still empty", async () => {
    const account = (ids: readonly string[], patch: Partial<ProviderSnapshotEntry> = {}) => route("oc-work", ids, patch);
    const queue = (first: readonly ProviderSnapshotEntry[], second: readonly ProviderSnapshotEntry[]) => ({
      providers: { snapshot: vi.fn().mockResolvedValueOnce({ entries: first }).mockResolvedValue({ entries: second }),
        refresh: async () => ({ acknowledged: true }) },
    });

    // It was loading with no models before the Codex refresh and arrives with models after it.
    const late = await readProviderCatalog(queue(
      [route("codex", ["gpt-stale"]), account([], { status: "loading" })],
      [route("codex", ["gpt-current"]), account(["deepseek/allowed", "deepseek/hidden"])],
    ), cwd, { config: host, overridesProblem: "Provider authority unavailable." });
    expect(late.entries.find((entry) => entry.provider === "oc-work")!.models).toEqual([]);
    expect(late.problemFor("oc-work")).toContain("unavailable");

    // It first appears in the post-refresh snapshot.
    const added = await readProviderCatalog(queue(
      [route("codex", ["gpt-stale"])],
      [route("codex", ["gpt-current"]), account(["deepseek/hidden"])],
    ), cwd, { config: host, overridesProblem: "Provider authority unavailable." });
    expect(added.entries.find((entry) => entry.provider === "oc-work")!.models).toEqual([]);
    expect(added.problemFor("oc-work")).toContain("unavailable");

    // An empty model list is not a reason to leave the unconfirmed route available.
    const empty = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [account([])] }) } }, cwd,
      { config: host, overridesProblem: "Provider authority unavailable." });
    expect(empty.problemFor("oc-work")).toContain("unavailable");
  });

  it("N2 keeps a distinct project's configuration when a custom global directory is set", async () => {
    const config: HostConfigDeps = { ...host, env: { OPENCODE_CONFIG_DIR: "/synthetic/custom" }, isDirectory: async () => true,
      readFile: async (path: string) => path === "/synthetic/custom/opencode.json" ? "{}"
        : path === `${cwd}/opencode.json` ? JSON.stringify({ disabled_providers: ["deepseek"] }) : absent() };
    const catalog = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [route("opencode", ["deepseek/hidden", "other/model"])] }) } }, cwd, { config });
    expect(catalog.entries[0]!.models!.map((model) => model.id)).toEqual(["other/model"]);

    // The project walk starts at the request directory even with a custom global directory…
    const paths = opencodeConfigPaths(cwd, config);
    expect(paths).toContain(`${cwd}/opencode.json`);
    expect(paths).toContain("/synthetic/custom/opencode.json");
    // …and is skipped when the request directory is that global directory itself, as native discovery does.
    const atGlobal = opencodeConfigPaths("/synthetic/custom", { ...config, stopAt: "/synthetic" });
    expect(atGlobal.filter((path) => path === "/synthetic/custom/opencode.json")).toHaveLength(1);
  });
});
