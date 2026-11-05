import { realpathSync } from "node:fs";
import { lstat, readFile as readFileDefault, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";

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
  /** Whether an explicitly selected configuration directory is available; a test that invents paths answers true. */
  readonly isDirectory?: (path: string) => Promise<boolean>;
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

/** OpenCode's legacy provider-id aliases, applied before a rule or a filter is matched. This is the one map both
 * the normalizer and the legacy-container precedence read. */
const PROVIDER_ALIASES: Readonly<Record<string, string>> = {
  "azure-cognitive-services": "azure",
  "google-vertex-anthropic": "google-vertex",
};

export function normalizeOpenCodeProviderId(id: string): string {
  // Only the alias map's own entries are aliases: an unknown id such as `constructor` or `__proto__` keeps its
  // string unchanged, as native OpenCode does, instead of reading an inherited Object.prototype member.
  return Object.hasOwn(PROVIDER_ALIASES, id) ? PROVIDER_ALIASES[id]! : id;
}

/**
 * OpenCode's environment flag: `1` or `true`, invariant of case, and nothing else. The installed 2.2.5 binary
 * parses every `OPENCODE_*` on/off switch through this one predicate.
 */
export function opencodeFlag(value: string | undefined): boolean {
  return value === "1" || value?.toLowerCase() === "true";
}

/**
 * The configuration directory OpenCode reads: `OPENCODE_CONFIG_DIR` when set, else `$XDG_CONFIG_HOME/opencode`,
 * else `~/.config/opencode`. The installed binary gives the custom directory to its global path service, so it
 * replaces the user directory rather than adding one.
 */
export function opencodeConfigDirectory(deps: HostConfigDeps = {}): string {
  const env = deps.env ?? process.env, home = deps.home ?? homedir();
  const custom = env.OPENCODE_CONFIG_DIR?.trim();
  if (custom) return resolve(custom);
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
 * A directory's identity for native discovery's Location check: its real path when it exists, and its lexical
 * absolute path when it does not, as OpenCode 2.2.5 resolves both sides through `realPath` with a not-found fallback.
 * A symlink to the global directory is therefore the same Location as that directory.
 */
function canonicalDirectory(path: string): string {
  try { return realpathSync(path); } catch { return resolve(path); }
}

/**
 * Whether an explicitly selected configuration directory is available to read. A missing directory, a stat error
 * and a symlink whose target is gone are all unavailable; a symlink to an existing directory is available.
 */
async function directoryAvailable(path: string): Promise<boolean> {
  try {
    const entry = await lstat(path);
    if (entry.isSymbolicLink()) return (await stat(path)).isDirectory();
    return entry.isDirectory();
  } catch { return false; }
}

/**
 * What an ENOENT from reading a configuration file means. Walk upward from the leaf with `lstat` to the first path
 * that exists. Only when that path is an ancestor that resolves to a real directory are all names below it —
 * including the leaf — genuinely missing, which is ordinary absence. The leaf itself existing (a dangling link,
 * for example), a broken ancestor link, a non-directory ancestor, or a walk error other than ENOENT is a source
 * problem, never permission to load without that source's restrictions.
 */
async function missingSource(path: string): Promise<"absent" | "problem"> {
  const leaf = resolve(path);
  for (let current = leaf; ; current = dirname(current)) {
    try {
      await lstat(current);
      if (current === leaf) return "problem";
      try { return (await stat(current)).isDirectory() ? "absent" : "problem"; }
      catch { return "problem"; }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") return "problem";
    }
    if (dirname(current) === current) return "problem";
  }
}

/** Whether `path` is the explicitly selected directory or a document path constructed underneath it. */
function insideDirectory(path: string, directory: string): boolean {
  const base = resolve(directory), target = resolve(path);
  return target === base || target.startsWith(`${base}${sep}`);
}

const unavailableDirectory = (directory: string) => `OpenCode config directory ${directory} could not be read as a directory.`;

/**
 * Every file the loader tries, in OpenCode's order of precedence: the system directory, the user's configuration
 * directory, `OPENCODE_CONFIG`, then the project walk from the outermost ancestor to `cwd` (each directory's
 * `opencode.json`, then its `opencode.jsonc`, then the same names under its `.opencode` directory).
 * `OPENCODE_CONFIG_CONTENT` is a document, not a file, and is appended by {@link readOpenCodeConfig}.
 *
 * Two environment controls change that set, as the installed binary applies them: `OPENCODE_CONFIG_DIR` replaces
 * the user directory (the global source) but does not itself stop the project walk; the walk is skipped only when
 * the request directory is itself one of the loaded global/system directories, or when
 * `OPENCODE_CONFIG_PROJECT_DISABLE` (alias `OPENCODE_DISABLE_PROJECT_CONFIG`) is set.
 */
export function opencodeConfigPaths(cwd: string, deps: HostConfigDeps = {}): string[] {
  const env = deps.env ?? process.env, paths: string[] = [];
  const projectDisabled = opencodeFlag(env.OPENCODE_CONFIG_PROJECT_DISABLE ?? env.OPENCODE_DISABLE_PROJECT_CONFIG);
  const globalDirectories = deps.system === false ? [opencodeConfigDirectory(deps)] : [opencodeSystemDirectory(deps), opencodeConfigDirectory(deps)];
  // The native Location directory is the one the request runs in; a custom global directory alone is not a reason
  // to skip the project's own documents. It only is when that request directory is the global directory itself.
  // Both sides are canonical real paths, so an alias of the global directory is still that directory.
  const location = canonicalDirectory(cwd), atGlobalDirectory = globalDirectories.some((dir) => canonicalDirectory(dir) === location);
  const ancestors = projectDisabled || atGlobalDirectory ? [] : ancestorDirectories(cwd, deps.stopAt);
  for (const dir of globalDirectories) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, name));
  const explicit = env.OPENCODE_CONFIG?.trim();
  if (explicit) paths.push(explicit);
  for (const dir of ancestors) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, name));
  for (const dir of ancestors) for (const name of OPENCODE_CONFIG_NAMES) paths.push(join(dir, ".opencode", name));
  return paths;
}

export type JsoncParse = { readonly ok: true; readonly value: unknown } | { readonly ok: false };

/**
 * JSONC as OpenCode's loader accepts it: JSON with `//` and block comments and trailing commas. One leading
 * U+FEFF byte-order mark is removed first, as OpenCode 2.2.5 accepts a BOM-prefixed document. A string-aware
 * scan removes the comments, so a `//` inside a value stays part of the value. A text that is not JSON after that
 * is `{ ok: false }`; the text itself never leaves this function.
 */
export function parseJsonc(text: string): JsoncParse {
  const source = text.startsWith("\uFEFF") ? text.slice(1) : text;
  let clean = "", inString = false;
  for (let index = 0; index < source.length; index += 1) {
    const char = source[index]!, next = source[index + 1];
    if (inString) {
      clean += char;
      if (char === "\\") { clean += next ?? ""; index += 1; }
      else if (char === "\"") inString = false;
      continue;
    }
    if (char === "\"") { inString = true; clean += char; continue; }
    if (char === "/" && next === "/") { while (index < source.length && source[index] !== "\n") index += 1; clean += "\n"; continue; }
    if (char === "/" && next === "*") {
      // An unterminated block comment is a truncated source, not a comment that runs to the end: OpenCode rejects
      // the document, so this parser must not hand JSON.parse the text before the opening marker.
      const end = source.indexOf("*/", index + 2);
      if (end < 0) return { ok: false };
      index = end + 1;
      continue;
    }
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
 * Every file OpenCode would load for this working directory, parsed. Every source path is classified through one
 * ENOENT rule: a file is absent only when its nearest existing ancestor resolves to a real directory and every name
 * below it is genuinely missing. An existing leaf that cannot be read, a broken ancestor link or any other read
 * error fails the whole read, so no route can fall back to the unfiltered catalogue. An explicitly selected
 * `OPENCODE_CONFIG_DIR` is checked when it is selected and again if a document beneath it is missing, because its
 * restrictions did not simply disappear with the directory.
 */
export async function readOpenCodeConfig(cwd: string, deps: HostConfigDeps = {}): Promise<OpenCodeConfigRead> {
  const env = deps.env ?? process.env, readFile = deps.readFile ?? ((path: string) => readFileDefault(path, "utf8"));
  // An explicitly selected directory is a source the user named; losing it is a config problem, not permission to
  // load the catalogue with that source's restrictions gone. A default location is only read when present.
  const selected = env.OPENCODE_CONFIG_DIR?.trim() ? opencodeConfigDirectory(deps) : null;
  const namedConfig = env.OPENCODE_CONFIG?.trim() ? resolve(env.OPENCODE_CONFIG.trim()) : null;
  const isDirectory = deps.isDirectory ?? directoryAvailable;
  // One predicate for both checks, so the explicit selection cannot lose its enforcement between them.
  const selectedAvailable = async (): Promise<boolean> => selected === null || (await isDirectory(selected));
  if (selected !== null && !(await selectedAvailable())) return { problem: unavailableDirectory(selected) };
  const documents: OpenCodeDocument[] = [];
  const load = async (path: string): Promise<{ info: Record<string, unknown> } | { problem: string } | null> => {
    let text: string;
    try { text = await readFile(path); }
    catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code === "ENOENT") {
        // Every enumerated source — the global directories, OPENCODE_CONFIG and the project walk — is read through
        // here. An explicitly named file is required even when genuinely absent; optional default candidates are not.
        // ENOENT is ordinary absence only when the nearest existing ancestor is a real directory and every name
        // below it is genuinely missing; the selected directory is then re-checked in case it vanished.
        if (namedConfig !== null && resolve(path) === namedConfig)
          return { problem: `OpenCode config ${path} could not be read (ENOENT).` };
        if (await missingSource(path) === "problem")
          return { problem: `OpenCode config ${path} could not be read (ENOENT).` };
        if (selected !== null && insideDirectory(path, selected) && !(await selectedAvailable()))
          return { problem: unavailableDirectory(selected) };
        return null;
      }
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
 * One present policy group's `provider.use` rules. A malformed container, entry or recognized rule field is a
 * source-named problem, never an absent restriction; an action outside POLICY_ACTIONS is not ours to validate and
 * stays ignored.
 */
function policyRules(group: unknown, where: string): { rules: Rule[] } | { problem: string } {
  if (!Array.isArray(group)) return { problem: `OpenCode config ${where} must be a list of policy objects.` };
  const rules: Rule[] = [];
  for (const entry of group) {
    const rule = record(entry);
    if (!rule) return { problem: `OpenCode config ${where} entries must be objects.` };
    if (typeof rule.action !== "string") return { problem: `OpenCode config ${where} entries must state a string action.` };
    if (!POLICY_ACTIONS.has(rule.action)) continue;
    if (typeof rule.resource !== "string") return { problem: `OpenCode config ${where} provider.use entries must state a string resource.` };
    if (typeof rule.effect !== "string" || !EFFECTS.has(rule.effect)) return { problem: `OpenCode config ${where} provider.use entries must state an allow or deny effect.` };
    rules.push({ resource: rule.resource, effect: rule.effect as "allow" | "deny" });
  }
  return { rules };
}

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
  const experimental = record(info.experimental), groups: { where: string; group: unknown }[] = [];
  if (experimental && Object.hasOwn(experimental, "policies")) groups.push({ where: `${path}: experimental.policies`, group: experimental.policies });
  if (Object.hasOwn(info, "policies")) groups.push({ where: `${path}: policies`, group: info.policies });
  for (const { where, group } of groups) {
    const parsed = policyRules(group, where);
    if ("problem" in parsed) return { rules, problem: parsed.problem };
    rules.push(...parsed.rules);
  }
  return { rules };
}

/** The last rule a document states for a provider wins; a document that states none leaves the decision open. */
function documentDecision(rules: readonly Rule[], provider: string): "allow" | "deny" | null {
  let decision: "allow" | "deny" | null = null;
  for (const rule of rules) if (resourceMatches(provider, rule.resource)) decision = rule.effect;
  return decision;
}

/** The selection keys one declared provider entry states; an absent key says nothing about that filter. */
type DeclaredFilters = { readonly whitelist?: readonly string[]; readonly blacklist?: readonly string[] };

/** Validate one declared provider entry before any merge can shadow it. Only its stated selection keys are kept. */
function declaredFilters(id: string, entry: unknown, path: string): { filters: DeclaredFilters } | { problem: string } {
  const provider = record(entry);
  if (!provider) return { problem: `OpenCode config ${path}: provider ${id} must be an object.` };
  const filters: { whitelist?: readonly string[]; blacklist?: readonly string[] } = {};
  for (const key of ["whitelist", "blacklist"] as const) {
    if (!(key in provider)) continue;
    const listed = stringList(provider[key], `${path}: provider.${id}.${key}`);
    if ("problem" in listed) return { problem: listed.problem };
    filters[key] = listed.values;
  }
  return { filters };
}

/**
 * The provider entries a document declares, from either container. Every entry and its selection keys are
 * validated while the containers are read, before normalization or precedence can let a later entry shadow a
 * malformed one. A present container of the wrong shape is a configuration problem, not an absent setting.
 */
function providerEntries(info: Record<string, unknown>, path: string): { entries: ReadonlyMap<string, DeclaredFilters> } | { problem: string } {
  const declarations: { rawId: string; id: string; filters: DeclaredFilters }[] = [];
  for (const key of ["provider", "providers"]) {
    if (!(key in info)) continue;
    const group = record(info[key]);
    if (!group) return { problem: `OpenCode config ${path}: ${key} must be an object.` };
    // Validate every declaration before precedence can skip one, so a malformed alias still fails closed.
    for (const [rawId, entry] of Object.entries(group)) {
      const id = normalizeOpenCodeProviderId(rawId), validated = declaredFilters(id, entry, path);
      if ("problem" in validated) return validated;
      declarations.push({ rawId, id, filters: validated.filters });
    }
  }
  // A document that declares the canonical spelling of an id keeps it: an alias-spelled declaration of the same
  // normalized id never replaces or weakens it, whichever container either declaration is in. Canonical
  // declarations keep their container order (`providers` after `provider`, whole-entry last-wins); an alias is
  // used only when the document declares no canonical spelling. Validation above still covers a skipped alias.
  const canonical = new Set(declarations.filter((entry) => entry.rawId === entry.id).map((entry) => entry.id));
  const merged = new Map<string, DeclaredFilters>();
  for (const entry of declarations) {
    if (entry.rawId !== entry.id && canonical.has(entry.id)) continue;
    merged.set(entry.id, entry.filters);
  }
  return { entries: merged };
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
    const entries = providerEntries(document.info, document.path);
    if ("problem" in entries) return { problem: entries.problem };
    for (const [id, declared] of entries.entries) {
      const previous = filters.get(id) ?? { whitelist: null, blacklist: null };
      filters.set(id, {
        whitelist: "whitelist" in declared ? declared.whitelist! : previous.whitelist,
        blacklist: "blacklist" in declared ? declared.blacklist! : previous.blacklist,
      });
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
