import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  ancestorDirectories, opencodeConfigPaths, opencodeModelVisible, opencodeSelection, parseJsonc, readOpenCodeConfig,
  resourceMatches, type HostConfigDeps, type OpenCodeDocument,
} from "../server/opencode-config";

const dirs: string[] = [];
afterEach(async () => { for (const root of dirs.splice(0)) await rm(root, { recursive: true, force: true }); });

/** A synthetic project tree; every fixture is invented here and never read from the machine running the test. */
async function tree(files: Record<string, string> = {}): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "opencode-config-"));
  dirs.push(root);
  for (const [path, text] of Object.entries(files)) {
    const file = join(root, path);
    await mkdir(dirname(file), { recursive: true });
    await writeFile(file, text);
  }
  return root;
}

/** The host reads with no real home and no system directory, so only the fixture tree can contribute a document. */
const host = (root: string): HostConfigDeps => ({ env: {}, home: join(root, "home"), system: false, stopAt: root, platform: "linux" });

const documents = (list: Record<string, unknown>[]): OpenCodeDocument[] => list.map((info, index) => ({ path: `/fixture/opencode-${index}.json`, info }));
type Selection = Extract<ReturnType<typeof opencodeSelection>, { selection: unknown }>["selection"];
const ids = (models: readonly string[], selection: Selection) => models.filter((id) => opencodeModelVisible(id, selection));

describe("OpenCode configuration loading", () => {
  it("loads the system and user directories, OPENCODE_CONFIG, the project walk and its .opencode directories, then the content", async () => {
    const root = await tree({
      "home/.config/opencode/opencode.json": `{"marker":"user-json"}`,
      "explicit/opencode.json": `{"marker":"explicit"}`,
      "opencode.json": `{"marker":"outer"}`,
      "opencode.jsonc": `{"marker":"outer-jsonc"}`,
      "proj/opencode.json": `{"marker":"inner"}`,
      "proj/.opencode/opencode.json": `{"marker":"inner-dot-opencode"}`,
    });
    const read = await readOpenCodeConfig(join(root, "proj", "sub"), { ...host(root), env: { OPENCODE_CONFIG: join(root, "explicit/opencode.json"), OPENCODE_CONFIG_CONTENT: `{"marker":"content"}` } });
    expect("problem" in read).toBe(false);
    const markers = (read as { documents: OpenCodeDocument[] }).documents.map((document) => [document.path.startsWith(root) ? document.path.slice(root.length + 1) : document.path, document.info.marker]);
    expect(markers).toEqual([
      ["home/.config/opencode/opencode.json", "user-json"],
      ["explicit/opencode.json", "explicit"],
      ["opencode.json", "outer"],
      ["opencode.jsonc", "outer-jsonc"],
      ["proj/opencode.json", "inner"],
      ["proj/.opencode/opencode.json", "inner-dot-opencode"],
      ["OPENCODE_CONFIG_CONTENT", "content"],
    ]);
  });

  it("walks every ancestor of the working directory and names candidates outermost first", async () => {
    const root = await tree();
    expect(ancestorDirectories(join(root, "a", "b"), root)).toEqual([root, join(root, "a"), join(root, "a", "b")]);
    const paths = opencodeConfigPaths(join(root, "a"), host(root));
    expect(paths.slice(0, 2)).toEqual([join(root, "home", ".config", "opencode", "opencode.json"), join(root, "home", ".config", "opencode", "opencode.jsonc")]);
    // Every ancestor's own files, outermost first, then every ancestor's .opencode directory, outermost first.
    expect(paths.slice(-8)).toEqual([join(root, "opencode.json"), join(root, "opencode.jsonc"), join(root, "a", "opencode.json"), join(root, "a", "opencode.jsonc"),
      join(root, ".opencode", "opencode.json"), join(root, ".opencode", "opencode.jsonc"), join(root, "a", ".opencode", "opencode.json"), join(root, "a", ".opencode", "opencode.jsonc")]);
  });

  it("reads JSONC comments and trailing commas, and keeps a // inside a value", () => {
    expect(parseJsonc(`{
      // a line comment
      "url": "https://example.invalid/a//b", /* a block comment */
      "list": ["a", "b",],
    }`)).toEqual({ ok: true, value: { url: "https://example.invalid/a//b", list: ["a", "b"] } });
    expect(parseJsonc("{ not json }")).toEqual({ ok: false });
    expect(parseJsonc("null")).toEqual({ ok: true, value: null });
  });

  it("skips absent files but names the path and key of one it cannot read, never its content", async () => {
    const root = await tree();
    expect(await readOpenCodeConfig(root, host(root))).toEqual({ documents: [] });
    await writeFile(join(root, "opencode.json"), `{"model": "do-not-quote-this"`);
    const read = await readOpenCodeConfig(root, host(root));
    expect(read).toEqual({ problem: `OpenCode config ${join(root, "opencode.json")} is not valid JSONC.` });
    expect(JSON.stringify(read)).not.toContain("do-not-quote-this");
    await writeFile(join(root, "opencode.json"), `[]`);
    expect(await readOpenCodeConfig(root, host(root))).toEqual({ problem: `OpenCode config ${join(root, "opencode.json")} is not a JSON object.` });
    // A path that exists but cannot be read as a file is a problem, never an absent document.
    await rm(join(root, "opencode.json"));
    await mkdir(join(root, "opencode.json"));
    const unreadable = await readOpenCodeConfig(root, host(root));
    expect("problem" in unreadable).toBe(true);
    expect((unreadable as { problem: string }).problem).toContain(join(root, "opencode.json"));
  });

  it("reports a key of the wrong shape instead of falling back to an unfiltered list", async () => {
    const root = await tree({ "opencode.json": `{"enabled_providers": ["openrouter"], "provider": {"openrouter": {"whitelist": "one-model"}}}` });
    const read = await readOpenCodeConfig(root, host(root));
    expect("problem" in read).toBe(false);
    expect(opencodeSelection((read as { documents: OpenCodeDocument[] }).documents)).toEqual({ problem: `OpenCode config ${join(root, "opencode.json")}: provider.openrouter.whitelist must be a list of strings.` });
  });
});

describe("OpenCode selection", () => {
  type Selection = Extract<ReturnType<typeof opencodeSelection>, { selection: unknown }>["selection"];
  const selectionOf = (read: ReturnType<typeof opencodeSelection>): Selection => {
    if ("problem" in read) throw new Error(read.problem);
    return read.selection;
  };

  it("keeps only enabled providers, then the whitelist, then the blacklist", () => {
    const selection = selectionOf(opencodeSelection(documents([{ enabled_providers: ["openrouter", "zai-coding-plan"], provider: {
      openrouter: { whitelist: ["acme/one", "acme/two"] }, "zai-coding-plan": { whitelist: ["glm-a", "glm-b"], blacklist: ["glm-b"] } } }])));
    expect(ids(["openrouter/acme/one", "openrouter/acme/two", "openrouter/acme/three", "zai-coding-plan/glm-a", "zai-coding-plan/glm-b", "deepseek/deepseek-flash"], selection))
      .toEqual(["openrouter/acme/one", "openrouter/acme/two", "zai-coding-plan/glm-a"]);
  });

  it("treats an empty enabled list as opening nothing and an empty whitelist as keeping none", () => {
    expect(ids(["openrouter/acme/one"], selectionOf(opencodeSelection(documents([{ enabled_providers: [] }]))))).toEqual([]);
    expect(ids(["openrouter/acme/one"], selectionOf(opencodeSelection(documents([{ enabled_providers: ["openrouter"], provider: { openrouter: { whitelist: [] } } }]))))).toEqual([]);
  });

  it("lets the first document that states a provider rule decide, as OpenCode resolves provider.use policies", () => {
    // OpenCode 2.2.5 compiles enabled_providers into `provider.use` policies (deny *, then allow each) and resolves
    // them over the documents reversed: the earliest document with a matching rule wins, its last match inside it.
    const first = selectionOf(opencodeSelection(documents([{ enabled_providers: ["opencode"] }, { enabled_providers: ["deepseek"] }])));
    expect(ids(["opencode/beam-free", "deepseek/deepseek-flash"], first)).toEqual(["opencode/beam-free"]);
    const later = selectionOf(opencodeSelection(documents([{}, { disabled_providers: ["opencode"] }])));
    expect(ids(["opencode/beam-free"], later)).toEqual([]);
  });

  it("takes a model filter from the last document that defines that key, and reads provider and providers", () => {
    const selection = selectionOf(opencodeSelection(documents([
      { provider: { openrouter: { whitelist: ["acme/one", "acme/two"], blacklist: ["acme/two"] } } },
      { provider: { openrouter: { models: {} } }, providers: { "zai-coding-plan": { whitelist: ["glm-a"] } } },
      { providers: { openrouter: { whitelist: ["acme/three", "acme/two"] } } },
    ])));
    expect(ids(["openrouter/acme/one", "openrouter/acme/three", "zai-coding-plan/glm-a", "zai-coding-plan/glm-b"], selection)).toEqual(["openrouter/acme/three", "zai-coding-plan/glm-a"]);
    expect(selection.filters.get("openrouter")).toEqual({ whitelist: ["acme/three", "acme/two"], blacklist: ["acme/two"] });
  });

  it("honours an explicit provider.use policy in either policy key, and matches resources by OpenCode's glob", () => {
    expect(resourceMatches("openrouter", "*")).toBe(true);
    expect(resourceMatches("openrouter", "open?outer")).toBe(true);
    expect(resourceMatches("openrouter", "openrouterx")).toBe(false);
    const denied = selectionOf(opencodeSelection(documents([{ policies: [{ action: "provider.use", resource: "openrouter", effect: "deny" }, { action: "tool.use", resource: "*", effect: "deny" }] }])));
    expect(ids(["openrouter/acme/one", "opencode/beam-free"], denied)).toEqual(["opencode/beam-free"]);
    const allowed = selectionOf(opencodeSelection(documents([{ experimental: { policies: [{ action: "provider.use", resource: "*", effect: "deny" }, { action: "provider.use", resource: "opencode", effect: "allow" }] } }])));
    expect(ids(["openrouter/acme/one", "opencode/beam-free"], allowed)).toEqual(["opencode/beam-free"]);
  });

  it("leaves a model the configuration does not place, such as a route's own added model, as the harness lists it", () => {
    const selection = selectionOf(opencodeSelection(documents([{ enabled_providers: ["opencode"], provider: { opencode: { whitelist: ["beam-free"] } } }])));
    expect(ids(["opencode/beam-free", "opencode/not-listed", "added-by-paseo"], selection)).toEqual(["opencode/beam-free", "added-by-paseo"]);
  });
});
