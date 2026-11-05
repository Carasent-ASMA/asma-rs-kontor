import { modelVendor } from "../shared/model-vendor";
import type { ProviderSnapshotEntry } from "../shared/session-create";
import { opencodeModelVisible, opencodeSelection, readOpenCodeConfig, type HostConfigDeps, type OpenCodeSelection } from "./opencode-config";

/** The routes whose own model list must be re-read when a picker opens rather than served from the host's snapshot. */
export const FRESH_CATALOG_PROVIDERS = ["codex"] as const;
/** How long a forced re-read may take before the route is withheld; the host's own refresh budget stays its own. */
export const CATALOG_REFRESH_MS = 15_000;
/** The one harness a vendor's models may come through: OpenAI through Codex, Anthropic through Claude. */
export const EXCLUSIVE_HARNESSES: Readonly<Record<string, string>> = { openai: "codex", anthropic: "claude" };
/** The OpenCode harness's own Paseo provider id; its models are `provider/model` inside OpenCode's catalogue. */
export const OPENCODE_ROUTE = "opencode";
/**
 * The routes Paseo defines itself. Every other route id comes from the daemon configuration, so without that
 * configuration its harness — and therefore its configuration environment — is unconfirmed.
 */
export const BUILTIN_ROUTES: ReadonlySet<string> = new Set(["claude", "codex", "copilot", "opencode", "pi", "omp"]);
/** The daemon config's provider overrides: Paseo derives a route from another through `extends`. */
export type ProviderOverrides = Readonly<Record<string, unknown>>;

export type CatalogClient = {
  providers: {
    snapshot(options?: { cwd?: string }): Promise<{ entries: readonly ProviderSnapshotEntry[] }>;
    refresh?: (options?: { cwd?: string; providers?: string[] }) => Promise<unknown>;
  };
};

/** A route's catalogue as the picker may offer it, and why a route offers nothing. */
export type ProviderCatalog = {
  readonly entries: readonly ProviderSnapshotEntry[];
  /** One message per withheld route, naming the route and the source, never a value from it. */
  readonly problems: readonly string[];
  /** The message that withholds this route, or null when its catalogue was read. */
  problemFor(provider: string): string | null;
};

export type CatalogOptions = {
  /** Re-read the routes whose own list changes between sessions; default true. */
  readonly fresh?: boolean;
  /** The OpenCode configuration's host reads; tests inject them. */
  readonly config?: HostConfigDeps;
  /** The daemon config's provider overrides, which say which routes Paseo derives from another and each route's environment. */
  readonly overrides?: ProviderOverrides | null;
  /**
   * Why the daemon config's provider overrides could not be read. A route that Paseo does not define itself is then
   * withheld: without the overrides its harness is unconfirmed, so its list cannot be filtered by its own rules.
   */
  readonly overridesProblem?: string | null;
  /** A snapshot the caller already read, so a readiness wait is not repeated; the fresh read replaces it when used. */
  readonly snapshot?: { readonly entries: readonly ProviderSnapshotEntry[] };
  /** How long the forced re-read may take; {@link CATALOG_REFRESH_MS} unless a test bounds it tighter. */
  readonly refreshMs?: number;
};

function overrideEntry(overrides: ProviderOverrides | null | undefined, id: string): Record<string, unknown> | null {
  // Only an entry the configuration itself declares is an override: a route id that happens to name an inherited
  // Object.prototype member (`__proto__`) must not borrow that member as its `extends`/`env` record.
  const entry = overrides && Object.hasOwn(overrides, id) ? overrides[id] : null;
  return typeof entry === "object" && entry !== null && !Array.isArray(entry) ? entry as Record<string, unknown> : null;
}

function extendsFrom(overrides: ProviderOverrides | null | undefined, id: string): string | null {
  const base = overrideEntry(overrides, id)?.extends;
  return typeof base === "string" ? base : null;
}

/** A route and every route it derives from, outermost harness first and the route itself last. */
function routeChain(route: string, overrides: ProviderOverrides | null | undefined): string[] {
  const chain: string[] = [], seen = new Set<string>();
  let current: string | null = route;
  while (current !== null && !seen.has(current)) {
    seen.add(current);
    chain.unshift(current);
    current = extendsFrom(overrides, current);
  }
  return chain;
}

/**
 * The harness a route runs: the route itself when it derives from nothing, else the route it derives from, followed
 * through a chain. Paseo composes an account route from its base provider through `extends`, so `claude-work` is the
 * Claude harness and `codex-personal` is the Codex harness — a vendor's exclusivity belongs to the harness, not to one
 * account route's own id. Without the daemon config only the base ids are known.
 */
export function harnessBase(route: string, overrides: ProviderOverrides | null | undefined): string {
  return routeChain(route, overrides)[0] ?? route;
}

/**
 * A route's effective configuration environment: each provider in its `extends` chain contributes its own `env`,
 * the harness first and the route last, as Paseo composes the route from its base. The installed
 * `ProviderOverrideSchema` carries `env` beside `extends`; a route's own OpenCode configuration therefore comes
 * from that environment, not from the process environment shared by every route.
 */
export function routeEnvironment(route: string, overrides: ProviderOverrides | null | undefined): Readonly<Record<string, string>> {
  const env: Record<string, string> = {};
  for (const id of routeChain(route, overrides)) {
    const own = overrideEntry(overrides, id)?.env;
    if (typeof own !== "object" || own === null || Array.isArray(own)) continue;
    for (const [key, value] of Object.entries(own)) if (typeof value === "string") env[key] = value;
  }
  return env;
}

/** The host reads for one route: the injected base with its environment overlaid, and a `HOME` override honoured. */
function routeConfigDeps(base: HostConfigDeps | undefined, env: Readonly<Record<string, string>>): HostConfigDeps {
  const home = env.HOME?.trim() || base?.home;
  return { ...base, env: { ...(base?.env ?? process.env), ...env }, home };
}

/** Every route that runs the OpenCode harness: `opencode` itself and each route the daemon config derives from it. */
export function opencodeRouteIds(overrides: ProviderOverrides | null | undefined): ReadonlySet<string> {
  const routes = new Set<string>([OPENCODE_ROUTE]);
  for (const id of Object.keys(overrides ?? {})) if (harnessBase(id, overrides) === OPENCODE_ROUTE) routes.add(id);
  return routes;
}

/**
 * A model's vendor decides which harness may offer it; any other vendor is offered wherever its harness lists it.
 * A vendor's exclusivity is settled from the model's own id (see {@link modelVendor}), never from a route's label.
 */
export function modelVisibleOnRoute(modelId: string, harness: string): boolean {
  const exclusive = EXCLUSIVE_HARNESSES[modelVendor(modelId)];
  return exclusive === undefined || exclusive === harness;
}

function withheld(provider: string, reason: string): string {
  return `${provider}: ${reason} Its models are withheld rather than shown from an unstated list.`;
}

function bounded<T>(work: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const expiry = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("refresh did not answer")), ms);
  });
  return Promise.race([work, expiry]).finally(() => { if (timer) clearTimeout(timer); });
}

/**
 * The host's provider catalogue as every Kontor surface must offer it:
 * - every route a {@link FRESH_CATALOG_PROVIDERS} harness owns — the base route and its account routes — is re-read
 *   from that harness before the snapshot is used, and a host that cannot confirm that fresh read offers none of
 *   their models;
 * - each OpenCode route keeps only the models its own configuration environment opens for this working directory,
 *   and offers none when that configuration cannot be read;
 * - a vendor that one harness exclusively owns is offered only through that harness, accounts included;
 * - when the daemon configuration itself cannot be read, a route Paseo does not define is withheld rather than
 *   read as an independent harness.
 */
export async function readProviderCatalog(client: CatalogClient, cwd: string, options: CatalogOptions = {}): Promise<ProviderCatalog> {
  let entries: readonly ProviderSnapshotEntry[] = options.snapshot?.entries ?? (await client.providers.snapshot({ cwd })).entries;
  const overrides = options.overrides ?? null, problems = new Map<string, string>();
  const refreshed = new Set<string>();
  let freshReadConfirmed = false;
  let unconfirmedReason = "The host did not confirm a fresh read of the harness's own model list.";
  if (options.fresh !== false) {
    const refresh = client.providers.refresh;
    const present = [...new Set(entries.map((entry) => entry.provider)
      .filter((provider) => (FRESH_CATALOG_PROVIDERS as readonly string[]).includes(harnessBase(provider, overrides))))];
    if (present.length) {
      if (typeof refresh !== "function") {
        unconfirmedReason = "This host cannot re-read the harness's own model list.";
      } else {
        try {
          await bounded(refresh.call(client.providers, { cwd, providers: present }), options.refreshMs ?? CATALOG_REFRESH_MS);
          entries = (await client.providers.snapshot({ cwd })).entries;
          for (const provider of present) refreshed.add(provider);
          freshReadConfirmed = true;
        } catch { /* the final-snapshot check below withholds every fresh-harness route */ }
      }
    }
  }
  // The final snapshot is the one served. A fresh-harness route that was not part of a confirmed read — including
  // one that first appears after it — is withheld, so no route is served from a list no fresh read covers.
  if (options.fresh !== false)
    for (const entry of entries) {
      if (!(FRESH_CATALOG_PROVIDERS as readonly string[]).includes(harnessBase(entry.provider, overrides))) continue;
      if (freshReadConfirmed && refreshed.has(entry.provider)) continue;
      problems.set(entry.provider, withheld(entry.provider, unconfirmedReason));
    }
  // An account may finish loading, or first appear, while a fresh Codex read was in flight. Without the daemon's
  // provider overrides every route Paseo does not define itself is withheld — including one whose model list is
  // still empty at this moment.
  if (options.overridesProblem)
    for (const entry of entries)
      if (!BUILTIN_ROUTES.has(entry.provider))
        problems.set(entry.provider, withheld(entry.provider, options.overridesProblem));
  const opencodeRoutes = opencodeRouteIds(overrides), selections = new Map<string, { selection?: OpenCodeSelection; problem?: string }>();
  async function selectionFor(route: string): Promise<{ selection?: OpenCodeSelection; problem?: string }> {
    const env = routeEnvironment(route, overrides), deps = routeConfigDeps(options.config, env);
    const key = JSON.stringify([env, deps.home ?? null, deps.system ?? null, deps.stopAt ?? null, deps.platform ?? null]);
    const cached = selections.get(key);
    if (cached) return cached;
    const read = await readOpenCodeConfig(cwd, deps);
    let result: { selection?: OpenCodeSelection; problem?: string };
    if ("problem" in read) result = { problem: read.problem };
    else {
      const selected = opencodeSelection(read.documents);
      result = "problem" in selected ? { problem: selected.problem } : { selection: selected.selection };
    }
    selections.set(key, result);
    return result;
  }
  const routeSelections = new Map<string, OpenCodeSelection>();
  for (const entry of entries) {
    if (!opencodeRoutes.has(entry.provider) || problems.has(entry.provider)) continue;
    const read = await selectionFor(entry.provider);
    if (read.selection) routeSelections.set(entry.provider, read.selection);
    else if (read.problem) problems.set(entry.provider, withheld(entry.provider, read.problem));
  }
  const visible = entries.map((entry) => {
    const problem = problems.get(entry.provider) ?? null;
    const selection = routeSelections.get(entry.provider);
    const models = problem ? [] : (entry.models ?? []).filter((model) => modelVisibleOnRoute(model.id, harnessBase(entry.provider, overrides))
      && (!opencodeRoutes.has(entry.provider) || (selection !== undefined && opencodeModelVisible(model.id, selection))));
    // `createProviders` frames the error as a sentence and adds its own full stop.
    return problem ? { ...entry, models, status: "error", error: problem.replace(/\.$/, "") } : { ...entry, models };
  });
  return { entries: visible, problems: [...problems.values()], problemFor: (provider) => problems.get(provider) ?? null };
}
