import { randomUUID } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CreateRequest, ProviderSnapshotEntry } from "../shared/session-create";
import { readProviderCatalog } from "../server/provider-catalog";
import { opencodeModelVisible, opencodeSelection, parseJsonc, readOpenCodeConfig, type HostConfigDeps } from "../server/opencode-config";
import { selectSummaryRoute } from "../server/stock-summary-generator";
import { createSessionCreateService } from "../server/session-create";

/**
 * Regression tests for the six review findings on 2aad3593. Every fixture is invented here: a route id, a model id
 * and a synthetic directory only; no live id list and no count is asserted. A test never reads the machine.
 */
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

const absent = async (): Promise<string> => { throw Object.assign(new Error("absent"), { code: "ENOENT" }); };
const syntheticHost = (stopAt: string, patch: Partial<HostConfigDeps> = {}): HostConfigDeps =>
  ({ env: {}, home: join(stopAt, "home"), system: false, stopAt, platform: "linux", readFile: absent, ...patch });
const route = (provider: string, ids: readonly string[]): ProviderSnapshotEntry => ({
  provider, label: provider, enabled: true, status: "ready",
  models: ids.map((id) => ({ id, label: id, isSelectable: true })),
  modes: [{ id: "default", label: "Default" }, { id: "auto", label: "Auto" }],
} as ProviderSnapshotEntry);
const modelIds = (entry: ProviderSnapshotEntry) => (entry.models ?? []).map((model) => model.id);

/** The reviewer's minimal desk: one ADAM checkout is enough for the form and for a creation's first checks. */
const DESK = [{
  id: "wks-adam", projectId: "prj-adam", projectDisplayName: "DESK • ADAM", projectRootPath: "/fixture/project",
  projectKind: "directory", workspaceKind: "local_checkout", name: "ADAM", title: null, workspaceDirectory: "/fixture/project",
  archivingAt: null, status: "done", statusEnteredAt: null, activityAt: null, scripts: [],
}];

describe("review-a regressions", () => {
  it("F1 re-reads a derived Codex account and withholds it when that read cannot be confirmed", async () => {
    const stale = route("codex-work", ["gpt-stale"]), fresh = route("codex-work", ["gpt-current"]);
    const snapshot = vi.fn().mockResolvedValueOnce({ entries: [stale] }).mockResolvedValueOnce({ entries: [fresh] });
    const refresh = vi.fn(async () => ({ acknowledged: true }));
    const catalog = await readProviderCatalog({ providers: { snapshot, refresh } }, "/fixture/project", {
      config: syntheticHost("/fixture"), overrides: { "codex-work": { extends: "codex" } },
    });
    expect(refresh).toHaveBeenCalledExactlyOnceWith({ cwd: "/fixture/project", providers: ["codex-work"] });
    expect(modelIds(catalog.entries[0]!)).toEqual(["gpt-current"]);
    expect(catalog.problems).toEqual([]);

    const offline = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [stale] }) } }, "/fixture/project", {
      config: syntheticHost("/fixture"), overrides: { "codex-work": { extends: "codex" } },
    });
    expect(modelIds(offline.entries[0]!)).toEqual([]);
    expect(offline.problemFor("codex-work")).toContain("cannot re-read");
  });

  it("F2 withholds an unclassified account in options and in creation when the daemon config cannot be read", async () => {
    const stateRoot = await mkdtemp(join(tmpdir(), "review-a-f2-")); dirs.push(stateRoot);
    const configPath = join("/fixture/project", "opencode.json");
    const host = syntheticHost("/fixture", { readFile: async (path: string) => path === configPath
      ? JSON.stringify({ enabled_providers: ["deepseek"], provider: { deepseek: { whitelist: ["allowed"] } } }) : absent() });
    const config = { get: vi.fn(async () => ({ config: { providers: { "oc-work": { extends: "opencode" } } } })) };
    const client = {
      workspaces: { list: async () => ({ entries: DESK, pageInfo: { hasMore: false, nextCursor: null } }) },
      providers: { snapshot: async () => ({ entries: [route("oc-work", ["deepseek/allowed", "deepseek/hidden"])] }) },
      config,
    };
    const service = createSessionCreateService(client as never, stateRoot, join(stateRoot, "plugin-config", "kontor", "create.json"),
      undefined, undefined, join(stateRoot, "summaries"), host);

    const served = await service.options({ projectId: "prj-adam", start: "ad-hoc" });
    expect(served.providers.find((provider) => provider.id === "oc-work")!.models.map((model) => model.id)).toEqual(["deepseek/allowed"]);

    config.get.mockRejectedValueOnce(new Error("config unavailable"));
    const withheld = await service.options({ projectId: "prj-adam", start: "ad-hoc" });
    const account = withheld.providers.find((provider) => provider.id === "oc-work")!;
    expect(account.models).toEqual([]);
    expect(account).toMatchObject({ available: false, status: "error" });
    expect(withheld.notes.join("\n")).toContain("harness is unconfirmed");

    config.get.mockRejectedValueOnce(new Error("config unavailable"));
    const request: CreateRequest = { requestId: randomUUID(), role: "LSA", start: "ad-hoc", projectId: "prj-adam",
      topic: "Route review", provider: "oc-work", model: "deepseek/allowed", effort: null, modeId: "default" };
    const refused = await service.create(request);
    expect(refused).toMatchObject({ state: "refused", message: expect.stringContaining("harness is unconfirmed") });
  });

  it("F3 rejects an unterminated block comment instead of parsing the text before it", async () => {
    expect(parseJsonc("{} /* truncated")).toEqual({ ok: false });
    const project = "/fixture/project";
    const catalog = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [route("opencode", ["deepseek/hidden"])] }) } }, project, {
      config: syntheticHost("/fixture", { readFile: async (path: string) => path === join(project, "opencode.json") ? "{} /* truncated" : absent() }),
    });
    expect(modelIds(catalog.entries[0]!)).toEqual([]);
    expect(catalog.problemFor("opencode")).toContain("not valid JSONC");
  });

  it("F4 reads OPENCODE_CONFIG_DIR and honours both project-disable names", async () => {
    const project = "/fixture/project";
    const custom = await readOpenCodeConfig(project, syntheticHost("/fixture", { env: { OPENCODE_CONFIG_DIR: "/fixture/custom" },
      readFile: async (path: string) => path === "/fixture/custom/opencode.json" ? JSON.stringify({ disabled_providers: ["deepseek"] }) : absent() }));
    if ("problem" in custom) throw new Error(custom.problem);
    expect(custom.documents.map((document) => document.path)).toEqual(["/fixture/custom/opencode.json"]);
    const selection = opencodeSelection(custom.documents);
    if ("problem" in selection) throw new Error(selection.problem);
    expect(opencodeModelVisible("deepseek/hidden", selection.selection)).toBe(false);

    for (const variable of ["OPENCODE_DISABLE_PROJECT_CONFIG", "OPENCODE_CONFIG_PROJECT_DISABLE"]) {
      const read = await readOpenCodeConfig(project, syntheticHost("/fixture", { env: { [variable]: "true" },
        readFile: async (path: string) => path === join(project, "opencode.json") ? JSON.stringify({ enabled_providers: [] }) : absent() }));
      if ("problem" in read) throw new Error(read.problem);
      expect(read.documents).toEqual([]);
    }
  });

  it("F5 filters each OpenCode route by its own environment, not the process environment", async () => {
    const project = "/fixture/project";
    const config = syntheticHost("/fixture", { readFile: async (path: string) => path === join(project, "opencode.json")
      ? JSON.stringify({ provider: { deepseek: { whitelist: ["global"] } } }) : absent() });
    const catalog = await readProviderCatalog({ providers: { snapshot: async () => ({ entries: [route("oc-work", ["deepseek/global", "deepseek/account"])] }) } }, project, {
      config,
      overrides: { "oc-work": { extends: "opencode", env: { OPENCODE_CONFIG_CONTENT: JSON.stringify({ provider: { deepseek: { whitelist: ["account"] } } }) } } },
    });
    expect(modelIds(catalog.entries[0]!)).toEqual(["deepseek/account"]);
  });

  it("F6 selects a summary route only from the fresh allowed Codex list", async () => {
    const stale = route("codex", ["gpt-stale-mini"]), other = route("claude", ["claude-haiku-5.5"]);
    const snapshot = vi.fn().mockResolvedValueOnce({ entries: [stale] }).mockResolvedValue({ entries: [other] });
    const refresh = vi.fn(async () => ({ acknowledged: true }));
    const api = { providers: { snapshot, waitForReady: async () => ({ entries: [stale] }), refresh }, config: { get: async () => ({ config: {} }) } };
    const chosen = await selectSummaryRoute(api as never, "/fixture/project", syntheticHost("/fixture"));
    expect(refresh).toHaveBeenCalledOnce();
    expect(chosen.provider).toBe("claude");

    const failing = { providers: { snapshot: async () => ({ entries: [stale, other] }), waitForReady: async () => ({ entries: [stale, other] }),
      refresh: async () => { throw new Error("app-server did not start"); } }, config: { get: async () => ({ config: {} }) } };
    const fallback = await selectSummaryRoute(failing as never, "/fixture/project", syntheticHost("/fixture"));
    expect(fallback.provider).toBe("claude");
  });
});
