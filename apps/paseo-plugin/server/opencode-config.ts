import { readFile as readFileDefault } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

/** The two file names OpenCode loads from each configuration directory; `.json` first, `.jsonc` after it. */
export const OPENCODE_CONFIG_NAMES = ["opencode.json", "opencode.jsonc"] as const;
/** The virtual document `OPENCODE_CONFIG_CONTENT` contributes; it has no path and never carries a value into a message. */
export const OPENCODE_CONTENT_SOURCE = "OPENCODE_CONFIG_CONTENT";

/** The host reads and environment the loader uses; tests inject them so no real home directory is read. */
export type HostConfigDeps = {
  readonly home?: string;
  readonly env?: NodeJS.ProcessEnv;
  readonly platform?: NodeJS.Platform;
  /** Whether the host's system-wide configuration directory is read; tests that must not touch the machine disable it. */
  readonly system?: boolean;
  /** The outermost directory the project walk considers; the filesystem root in production. */
  readonly stopAt?: string;
  readonly readFile?: (path: string) => Promise<string>;
};

/** One parsed OpenCode configuration document, in the order OpenCode loads it. */
export type OpenCodeDocument = { readonly path: string; readonly info: Record<string, unknown> };

export type OpenCodeFilters = { readonly whitelist: readonly string[] | null; readonly blacklist: readonly string[] | null };

export type OpenCodeSelection = {
  /** Whether the configuration lets OpenCode use this provider at all. */
  readonly allows: (provider: string) => boolean;
  /** The model filters the operator declared for a provider; an absent entry declares none. */
  readonly filters: ReadonlyMap<string, OpenCodeFilters>;
};

export type OpenCodeConfigRead = { readonly documents: readonly OpenCodeDocument[] } | { readonly problem: string };
export type OpenCodeSelectionRead = { readonly selection: OpenCodeSelection } | { readonly problem: string };

const EFFECTS = new Set(["allow", "deny"]);
const POLICY_ACTIONS = new Set(["provider.use"]);

/** OpenCode's own provider-id aliases, applied before a rule or a filter is matched. */
export function normalizeOpenCodeProviderId(id: string): string {
  if (id === "azure-cognitive-services") return "azure";
  if (id === "google-vertex-anthropic") return "google-vertex";
  return id;
}

/** The configuration directory OpenCode reads: `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode`. */
export function opencodeConfigDirectory(deps: HostConfigDeps = {}): string {
  const env = deps.env ?? process.env, home = deps.home ?? homedir();
  return join(env.XDG_CONFIG_HOME?.trim() || join(home, ".config"), "opencode");
}

/** The system-wide configuration directory OpenCode reads before the user's own. */
export function opencodeSystemDirectory(deps: HostConfigDeps = {}): string {
  const env = deps.env ?? process.env, platform = deps.platform ?? process.platform;
  if (platform === "darwin") return "/Library/Application Support/opencode";
  if (platform === "win32") return join(env.ProgramData?.trim() || "C:\\ProgramData", "opencode");
  return "/etc/opencode";
}

/** `cwd`'s ancestors from the outermost inward, so a deeper directory's configuration is loaded after a shallower one. */
export function ancestorDirectories(cwd: string, stopAt?: string): string[] {
  const outermost = stopAt ? resolve(stopAt) : null, chain: string[] = [];
  let dir = resolve(cwd);
  for (;;) {
    chain.push(dir);
    const parent = dirname(dir);
    if (parent === dir || dir === outermost) break;
    dir = parent;
  }
  return chain.reverse();
}

/**
 * Every file the loader tries, in OpenCode's order of precedence: the system directory, the user's configuration
 * directory, `OPENCODE_CONFIG`, then the project walk from the outermost ancestor to `cwd` (each directory's
 * `opencode.json`, then its `opencode.jsonc`, then the same names under its `.opencode` directory).
 * `OPENCODE_CONFIG_CONTENT` is a document, not a file, and is appended by {@link readOpenCodeConfig}.
 */
export function opencodeConfigPaths(cwd: string, deps: HostConfigDeps = {}): string[] {
  const env = deps.env ?? process.env, ancestors = ancestorDirectories(cwd, deps.stopAt), paths: string[] = [];
  const directories = deps.system === false ? [opencodeConfigDirectory(deps)] : [opencodeSystemDirectory(deps), opencodeConfigDirectory(deps)];
  for (const dir of directories) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, name));
  const explicit = env.OPENCODE_CONFIG?.trim();
  if (explicit) paths.push(explicit);
  for (const dir of ancestors) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, name));
  for (const dir of ancestors) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, ".opencode", name));
  return paths;
}

export type JsoncParse = { readonly ok: true; readonly value: unknown } | { readonly ok: false };

/**
 * JSONC as OpenCode's loader accepts it: JSON with `//` and block comments and trailing commas. A string-aware
 * scan removes the comments, so a `//` inside a value stays part of the value. A text that is not JSON after that
 * is `{ ok: false }`; the text itself never leaves this function.
 */
export function parseJsonc(text: string): JsoncParse {
  let clean = "", inString = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index]!, next = text[index + 1];
    if (inString) {
      clean += char;
      if (char === "\\") { clean += next ?? ""; index += 1; }
      else if (char === "\"") inString = false;
      continue;
    }
    if (char === "\"") { inString = true; clean += char; continue; }
    if (char === "/" && next === "/") { while (index < text.length && text[index] !== "\n") index += 1; clean += "\n"; continue; }
    if (char === "/" && next === "*") { index += 2; while (index < text.length && !(text[index] === "*" && text[index + 1] === "/")) index += 1; index += 1; continue; }
    clean += char;
  }
  let sansTrailingCommas = "", quoted = false;
  for (let index = 0; index < clean.length; index += 1) {
    const char = clean[index]!;
    if (quoted) {
      sansTrailingCommas += char;
      if (char === "\\") { sansTrailingCommas += clean[index + 1] ?? ""; index += 1; }
      else if (char === "\"") quoted = false;
      continue;
    }
    if (char === "\"") { quoted = true; sansTrailingCommas += char; continue; }
    if (char === ",") {
      let ahead = index + 1;
      while (ahead < clean.length && /\s/.test(clean[ahead]!)) ahead += 1;
      if (clean[ahead] === "}" || clean[ahead] === "]") continue;
    }
    sansTrailingCommas += char;
  }
  try { return { ok: true, value: JSON.parse(sansTrailingCommas) }; } catch { return { ok: false }; }
}

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

/** A configuration key that must be a list of strings; a wrongly shaped key is a problem, never a silent no-filter. */
function stringList(value: unknown, where: string): { values: string[] } | { problem: string } {
  if (!Array.isArray(value) || value.some((entry) => typeof entry !== "string")) return { problem: `OpenCode config ${where} must be a list of strings.` };
  return { values: value as string[] };
}

/**
 * Every file OpenCode would load for this working directory, parsed. A missing file is not a document and not a
 * problem; a file that exists but cannot be read or parsed fails the whole read, so no route can fall back to
 * the unfiltered catalogue.
 */
export async function readOpenCodeConfig(cwd: string, deps: HostConfigDeps = {}): Promise<OpenCodeConfigRead> {
  const env = deps.env ?? process.env, readFile = deps.readFile ?? ((path: string) => readFileDefault(path, "utf8"));
  const documents: OpenCodeDocument[] = [];
  const load = async (path: string): Promise<{ info: Record<string, unknown> } | { problem: string } | null> => {
    let text: string;
    try { text = await readFile(path); }
    catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "ENOENT") return null;
      return { problem: `OpenCode config ${path} could not be read${typeof code === "string" ? ` (${code})` : ""}.` };
    }
    const parsed = parseJsonc(text);
    if (!parsed.ok) return { problem: `OpenCode config ${path} is not valid JSONC.` };
    const info = record(parsed.value);
    return info ? { info } : { problem: `OpenCode config ${path} is not a JSON object.` };
  };
  for (const path of opencodeConfigPaths(cwd, deps)) {
    const loaded = await load(path);
    if (!loaded) continue;
    if ("problem" in loaded) return { problem: loaded.problem };
    documents.push({ path, info: loaded.info });
  }
  const content = env.OPENCODE_CONFIG_CONTENT?.trim();
  if (content) {
    const parsed = parseJsonc(content), info = parsed.ok ? record(parsed.value) : null;
    if (!info) return { problem: `OpenCode config ${OPENCODE_CONTENT_SOURCE} is not valid JSONC.` };
    documents.push({ path: OPENCODE_CONTENT_SOURCE, info });
  }
  return { documents };
}

/** OpenCode's glob for a policy resource: `*` spans anything, `?` spans one character; everything else is literal. */
export function resourceMatches(resource: string, pattern: string): boolean {
  let glob = pattern.replaceAll("\\", "/");
  const trailingSpaceStar = glob.endsWith(" .*");
  if (trailingSpaceStar) glob = `${glob.slice(0, -3)}( .*)?`;
  const escaped = glob.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  return new RegExp(`^${escaped}$`, "s").test(resource.replaceAll("\\", "/"));
}

type Rule = { readonly resource: string; readonly effect: "allow" | "deny" };

/**
 * One document's `provider.use` rules, in the order OpenCode builds them: `enabled_providers` (a wildcard deny,
 * then one allow per named provider), `disabled_providers`, then explicit policies.
 */
function documentRules(info: Record<string, unknown>, path: string): { rules: Rule[]; problem: string } | { rules: Rule[] } {
  const rules: Rule[] = [];
  for (const key of ["enabled_providers", "disabled_providers"] as const) {
    if (!(key in info)) continue;
    const listed = stringList(info[key], `${path}: ${key}`);
    if ("problem" in listed) return { rules, problem: listed.problem };
    if (key === "enabled_providers" && listed.values.length) rules.push({ resource: "*", effect: "deny" },
      ...listed.values.map((id) => ({ resource: normalizeOpenCodeProviderId(id), effect: "allow" as const })));
    else if (key === "enabled_providers") rules.push({ resource: "*", effect: "deny" });
    else rules.push(...listed.values.map((id) => ({ resource: normalizeOpenCodeProviderId(id), effect: "deny" as const })));
  }
  const experimental = record(info.experimental), groups = [experimental?.policies, info.policies];
  for (const group of groups) {
    if (!Array.isArray(group)) continue;
    for (const entry of group) {
      const rule = record(entry);
      if (!rule || typeof rule.action !== "string" || !POLICY_ACTIONS.has(rule.action) || typeof rule.resource !== "string") continue;
      if (typeof rule.effect !== "string" || !EFFECTS.has(rule.effect)) continue;
      rules.push({ resource: rule.resource, effect: rule.effect as "allow" | "deny" });
    }
  }
  return { rules };
}

/** The last rule a document states for a provider wins; a document that states none leaves the decision open. */
function documentDecision(rules: readonly Rule[], provider: string): "allow" | "deny" | null {
  let decision: "allow" | "deny" | null = null;
  for (const rule of rules) if (resourceMatches(provider, rule.resource)) decision = rule.effect;
  return decision;
}

function providerEntries(info: Record<string, unknown>): Record<string, unknown> {
  const merged: Record<string, unknown> = {};
  for (const key of ["provider", "providers"]) {
    const group = record(info[key]);
    if (!group) continue;
    for (const [id, entry] of Object.entries(group)) merged[normalizeOpenCodeProviderId(id)] = entry;
  }
  return merged;
}

/**
 * The effective OpenCode selection: which providers the configuration allows, and the model lists the operator
 * declared. A provider's use decision is taken from the first document that states one, and that document's last
 * matching rule wins, as OpenCode 2.2 resolves the `provider.use` policies these keys compile to. A model filter
 * is taken from the last document that defines that key for that provider, as the loader folds provider entries.
 */
export function opencodeSelection(documents: readonly OpenCodeDocument[]): OpenCodeSelectionRead {
  const perDocument: Rule[][] = [], filters = new Map<string, OpenCodeFilters>();
  for (const document of documents) {
    const built = documentRules(document.info, document.path);
    if ("problem" in built) return { problem: built.problem };
    perDocument.push(built.rules);
    for (const [id, entry] of Object.entries(providerEntries(document.info))) {
      const provider = record(entry);
      if (!provider) return { problem: `OpenCode config ${document.path}: provider ${id} must be an object.` };
      const previous = filters.get(id) ?? { whitelist: null, blacklist: null };
      const next: { whitelist: readonly string[] | null; blacklist: readonly string[] | null } = { ...previous };
      for (const key of ["whitelist", "blacklist"] as const) {
        if (!(key in provider)) continue;
        const listed = stringList(provider[key], `${document.path}: provider.${id}.${key}`);
        if ("problem" in listed) return { problem: listed.problem };
        next[key] = listed.values;
      }
      filters.set(id, next);
    }
  }
  const allows = (provider: string): boolean => {
    const wanted = normalizeOpenCodeProviderId(provider);
    for (const rules of perDocument) {
      const decision = documentDecision(rules, wanted);
      if (decision !== null) return decision === "allow";
    }
    return true;
  };
  return { selection: { allows, filters } };
}

/**
 * Whether the OpenCode catalogue may offer one model id, `provider/model`. A whitelist keeps only the models it
 * names; a blacklist then removes the ones it names. An id that names no OpenCode provider is not part of
 * OpenCode's own catalogue — Paseo composes every catalogue id as `provider/model` — so no OpenCode key names it
 * and it is offered as the harness lists it.
 */
export function opencodeModelVisible(modelId: string, selection: OpenCodeSelection): boolean {
  const slash = modelId.indexOf("/");
  if (slash < 1 || slash === modelId.length - 1) return true;
  const provider = modelId.slice(0, slash), model = modelId.slice(slash + 1);
  if (!selection.allows(provider)) return false;
  const filter = selection.filters.get(normalizeOpenCodeProviderId(provider));
  if (!filter) return true;
  if (filter.whitelist !== null && !filter.whitelist.includes(model)) return false;
  return !filter.blacklist?.includes(model);
}
