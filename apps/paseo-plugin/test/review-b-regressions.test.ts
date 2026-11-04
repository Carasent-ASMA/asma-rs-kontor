import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { modelVendor } from "../shared/model-vendor";
import { modelVisibleOnRoute } from "../server/provider-catalog";
import { parseJsonc, readOpenCodeConfig, type HostConfigDeps } from "../server/opencode-config";

/**
 * Regression tests for review-b's S1 and N1 on 3c9522f8. Every id is an invented example of the shape Cursor,
 * OpenCode and the provider APIs write; no live list or count is asserted.
 */
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

const host = (root: string): HostConfigDeps => ({ env: {}, home: join(root, "home"), system: false, stopAt: root, platform: "linux" });

describe("review-b regressions", () => {
  it("S1 classifies Anthropic series and OpenAI computer-use names from the model id, never the route", () => {
    for (const id of ["sonnet-4.5", "opus-4.5", "haiku-4.5", "sonnet-4.5-thinking", "opus", "sonnet"]) {
      expect(modelVendor(id)).toBe("anthropic");
      expect(modelVisibleOnRoute(id, "cursor")).toBe(false);
      expect(modelVisibleOnRoute(id, "claude")).toBe(true);
    }
    // Cursor is not the only route that omits the vendor word.
    expect(modelVendor("anthropic/sonnet-5")).toBe("anthropic");
    expect(modelVisibleOnRoute("anthropic/sonnet-5", "opencode")).toBe(false);
    // A leading OpenCode provider segment names the author even when the model itself has no vendor word.
    expect(modelVendor("anthropic/experimental-model")).toBe("anthropic");
    expect(modelVendor("openai/experimental-model")).toBe("openai");
    expect(modelVisibleOnRoute("openai/experimental-model", "opencode")).toBe(false);
    // OpenAI's computer-use family carries no "gpt" or "o-series" word.
    expect(modelVendor("computer-use-preview")).toBe("openai");
    expect(modelVisibleOnRoute("computer-use-preview", "cursor")).toBe(false);
    // R2's binding scope is unchanged: DeepSeek and GLM keep their per-route placement.
    expect(modelVisibleOnRoute("deepseek/deepseek-flash", "opencode")).toBe(true);
    expect(modelVisibleOnRoute("zai-coding-plan/glm-5.3", "opencode")).toBe(true);
  });

  it("N1 accepts one leading UTF-8 BOM as OpenCode 2.2.5 does", async () => {
    expect(parseJsonc("\uFEFF{\"marker\":\"bom\"}")).toEqual({ ok: true, value: { marker: "bom" } });
    expect(parseJsonc("\uFEFF{} /* truncated")).toEqual({ ok: false });
    const root = await mkdtemp(join(tmpdir(), "review-b-bom-")); dirs.push(root);
    await writeFile(join(root, "opencode.json"), "\uFEFF{\"enabled_providers\":[\"opencode\"]}");
    const read = await readOpenCodeConfig(root, host(root));
    if ("problem" in read) throw new Error(read.problem);
    expect(read.documents.map((document) => document.info.enabled_providers)).toEqual([["opencode"]]);
  });
});
