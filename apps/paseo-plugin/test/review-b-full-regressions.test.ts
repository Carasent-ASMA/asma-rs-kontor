import { describe, expect, it, vi } from "vitest";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { modelVendor } from "../shared/model-vendor";
import type { HostConfigDeps } from "../server/opencode-config";
import { modelVisibleOnRoute, readProviderCatalog } from "../server/provider-catalog";

/**
 * Regressions for the review-b full verdict on 8535d456. The first two tests pin behaviour that was already
 * correct there (pure guards); the last two cover the two notes. Every route, path, model id and document is
 * synthetic; no live id list or count is asserted.
 */
const project = "/fixture/project";
const absent = async (): Promise<string> => { throw Object.assign(new Error("absent"), { code: "ENOENT" }); };
const host = (readFile: (path: string) => Promise<string> = absent): HostConfigDeps =>
  ({ env: {}, home: "/fixture/host-home", system: false, stopAt: "/fixture", platform: "linux", readFile });
const route = (provider: string, ids: readonly string[]): ProviderSnapshotEntry => ({
  provider, label: provider, enabled: true, status: "ready",
  models: ids.map((id) => ({ id, label: id, isSelectable: true })), modes: [],
} as ProviderSnapshotEntry);
const modelIds = (entry: ProviderSnapshotEntry | undefined) => (entry?.models ?? []).map((model) => model.id);
const catalogue = (entries: readonly ProviderSnapshotEntry[], refresh?: () => Promise<unknown>) =>
  ({ providers: { snapshot: async () => ({ entries }), ...(refresh ? { refresh } : {}) } });

describe("review-b full-verdict regressions", () => {
  it("SF1 gives the account's own config content precedence over the base route's", async () => {
    const whitelist = (model: string) => JSON.stringify({ provider: { deepseek: { whitelist: [model] } } });
    const catalog = await readProviderCatalog(
      catalogue([route("oc-work", ["deepseek/base-content", "deepseek/account-content"])]), project,
      { config: host(), overrides: {
        opencode: { env: { OPENCODE_CONFIG_CONTENT: whitelist("base-content") } },
        "oc-work": { extends: "opencode", env: { OPENCODE_CONFIG_CONTENT: whitelist("account-content") } },
      } },
    );
    expect(modelIds(catalog.entries[0])).toEqual(["deepseek/account-content"]);
  });

  it("SF1 reads the account's env.HOME, not the base's, for each route", async () => {
    const documents: Record<string, string> = {
      "/fixture/account-home/.config/opencode/opencode.json": JSON.stringify({ provider: { deepseek: { whitelist: ["account-home"] } } }),
      "/fixture/base-home/.config/opencode/opencode.json": JSON.stringify({ provider: { deepseek: { whitelist: ["base-home"] } } }),
      "/fixture/host-home/.config/opencode/opencode.json": JSON.stringify({ provider: { deepseek: { whitelist: ["host-home"] } } }),
    };
    const ids = ["deepseek/base-home", "deepseek/account-home", "deepseek/host-home"];
    const catalog = await readProviderCatalog(catalogue([route("opencode", ids), route("oc-work", ids)]), project, {
      config: host(async (path) => documents[path] ?? absent()),
      overrides: {
        opencode: { env: { HOME: "/fixture/base-home" } },
        "oc-work": { extends: "opencode", env: { HOME: "/fixture/account-home" } },
      },
    });
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "opencode"))).toEqual(["deepseek/base-home"]);
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "oc-work"))).toEqual(["deepseek/account-home"]);
  });

  it("SF2 filters two OpenCode accounts in one request by their own environments", async () => {
    const whitelist = (model: string) => JSON.stringify({ provider: { deepseek: { whitelist: [model] } } });
    const ids = ["deepseek/a", "deepseek/b"];
    const catalog = await readProviderCatalog(catalogue([route("oc-a", ids), route("oc-b", ids)]), project, {
      config: host(), overrides: {
        "oc-a": { extends: "opencode", env: { OPENCODE_CONFIG_CONTENT: whitelist("a") } },
        "oc-b": { extends: "opencode", env: { OPENCODE_CONFIG_CONTENT: whitelist("b") } },
      },
    });
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "oc-a"))).toEqual(["deepseek/a"]);
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "oc-b"))).toEqual(["deepseek/b"]);
  });

  it("N1 withholds a Codex-harness route that first appears after the fresh read", async () => {
    const snapshot = vi.fn()
      .mockResolvedValueOnce({ entries: [route("codex", ["gpt-stale"])] })
      .mockResolvedValueOnce({ entries: [route("codex", ["gpt-current"]), route("codex-work", ["gpt-work"])] });
    const refresh = vi.fn(async () => ({ acknowledged: true }));
    const catalog = await readProviderCatalog({ providers: { snapshot, refresh } }, project,
      { config: host(), overrides: { "codex-work": { extends: "codex" } } });

    expect(refresh).toHaveBeenCalledWith({ cwd: project, providers: ["codex"] });
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "codex"))).toEqual(["gpt-current"]);
    expect(modelIds(catalog.entries.find((entry) => entry.provider === "codex-work"))).toEqual([]);
    expect(catalog.problemFor("codex-work")).toContain("did not confirm a fresh read");

    // Guard: an account that was part of the refresh read is served its refreshed list.
    const both = vi.fn()
      .mockResolvedValueOnce({ entries: [route("codex", ["gpt-stale"]), route("codex-work", ["gpt-stale-work"])] })
      .mockResolvedValueOnce({ entries: [route("codex", ["gpt-current"]), route("codex-work", ["gpt-current-work"])] });
    const refreshed = await readProviderCatalog({ providers: { snapshot: both, refresh } }, project,
      { config: host(), overrides: { "codex-work": { extends: "codex" } } });
    expect(refresh).toHaveBeenCalledWith({ cwd: project, providers: ["codex", "codex-work"] });
    expect(modelIds(refreshed.entries.find((entry) => entry.provider === "codex-work"))).toEqual(["gpt-current-work"]);
    expect(refreshed.problemFor("codex-work")).toBeNull();
  });

  it("N2 classifies OpenAI speech ids as OpenAI, whatever route offers them", () => {
    for (const id of ["whisper-large-v3", "whisper-large-v3-turbo", "tts-1", "tts-1-hd", "gpt-4o-mini-tts"])
      expect(modelVendor(id)).toBe("openai");
    expect(modelVisibleOnRoute("whisper-large-v3", "opencode")).toBe(false);
    expect(modelVisibleOnRoute("tts-1", "copilot")).toBe(false);
    expect(modelVisibleOnRoute("whisper-large-v3", "codex")).toBe(true);
    // An id the tables do not name stays unknown, which no exclusivity rule hides.
    expect(modelVendor("house-model")).toBe("unknown");
    // The boundary after the family word is load-bearing: a longer word that merely starts with it is not OpenAI.
    expect(modelVendor("ttsish-1")).toBe("unknown");
    expect(modelVendor("whispering-1")).toBe("unknown");
    expect(modelVisibleOnRoute("ttsish-1", "opencode")).toBe(true);
    expect(modelVisibleOnRoute("whispering-1", "opencode")).toBe(true);
    // Real catalog shapes where the speech word is part of a non-OpenAI vendor's model name stay visible.
    expect(modelVendor("gemini-2.5-flash-tts")).toBe("google");
    expect(modelVendor("mimo-v2.5-tts")).toBe("xiaomi");
    expect(modelVendor("grok-voice-tts-1.0")).toBe("xai");
    expect(modelVendor("qwen3-tts-voicedesign")).toBe("alibaba");
    expect(modelVendor("voxtral-mini-tts")).toBe("unknown");
    for (const id of ["gemini-2.5-flash-tts", "mimo-v2.5-tts", "grok-voice-tts-1.0", "qwen3-tts-voicedesign", "voxtral-mini-tts"])
      expect(modelVisibleOnRoute(id, "opencode")).toBe(true);
  });
});
