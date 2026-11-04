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
  /** The daemon config's provider overrides, which say which routes Paseo derives from the OpenCode harness. */
  readonly overrides?: ProviderOverrides | null;
  /** A snapshot the caller already read, so a readiness wait is not repeated; the fresh read replaces it when used. */
  readonly snapshot?: { readonly entries: readonly ProviderSnapshotEntry[] };
  /** How long the forced re-read may take; {@link CATALOG_REFRESH_MS} unless a test bounds it tighter. */
  readonly refreshMs?: number;
};

/**
 * The harness a route runs: the route itself when it derives from nothing, else the route it derives from, followed
 * through a chain. Paseo composes an account route from its base provider through `extends`, so `claude-work` is the
 * Claude harness and `codex-personal` is the Codex harness — a vendor's exclusivity belongs to the harness, not to one
 * account route's own id. Without the daemon config only the base ids are known.
 */
export function harnessBase(route: string, overrides: ProviderOverrides | null | undefined): string {
  const parent = (id: string): string | null => {
    const entry = overrides && id in overrides && typeof overrides[id] === "object" && overrides[id] !== null ? overrides[id] as Record<string, unknown> : null;
    return typeof entry?.extends === "string" ? entry.extends : null;
  };
  const seen = new Set<string>();
  let current = route;
  while (!seen.has(current)) {
    seen.add(current);
    const next = parent(current);
    if (!next) break;
    current = next;
  }
  return current;
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
 * - the routes in {@link FRESH_CATALOG_PROVIDERS} are re-read from their own harness before the snapshot is used, and
 *   a host that cannot confirm that fresh read offers none of their models;
 * - the OpenCode route keeps only the models its own configuration opens for this working directory, and offers none
 *   when that configuration cannot be read;
 * - a vendor that one harness exclusively owns is offered only through that harness, accounts included.
 */
export async function readProviderCatalog(client: CatalogClient, cwd: string, options: CatalogOptions = {}): Promise<ProviderCatalog> {
  let entries: readonly ProviderSnapshotEntry[] = options.snapshot?.entries ?? (await client.providers.snapshot({ cwd })).entries;
  const problems = new Map<string, string>();
  if (options.fresh !== false) {
    const refresh = client.providers.refresh, present = FRESH_CATALOG_PROVIDERS.filter((provider) => entries.some((entry) => entry.provider === provider));
    if (present.length) {
      if (typeof refresh !== "function") {
        for (const provider of present) problems.set(provider, withheld(provider, "This host cannot re-read the harness's own model list."));
      } else {
        try {
          await bounded(refresh.call(client.providers, { cwd, providers: [...present] }), options.refreshMs ?? CATALOG_REFRESH_MS);
          entries = (await client.providers.snapshot({ cwd })).entries;
        } catch {
          for (const provider of present) problems.set(provider, withheld(provider, "The host did not confirm a fresh read of the harness's own model list."));
        }
      }
    }
  }
  let selection: OpenCodeSelection | null = null, opencodeProblem: string | null = null;
  const opencodeRoutes = opencodeRouteIds(options.overrides);
  if (entries.some((entry) => opencodeRoutes.has(entry.provider))) {
    const read = await readOpenCodeConfig(cwd, options.config);
    if ("problem" in read) opencodeProblem = read.problem;
    else {
      const selected = opencodeSelection(read.documents);
      if ("problem" in selected) opencodeProblem = selected.problem;
      else selection = selected.selection;
    }
    if (opencodeProblem) for (const route of opencodeRoutes) if (entries.some((entry) => entry.provider === route)) problems.set(route, withheld(route, opencodeProblem));
  }
  const visible = entries.map((entry) => {
    const problem = problems.get(entry.provider) ?? null;
    const models = problem ? [] : (entry.models ?? []).filter((model) => modelVisibleOnRoute(model.id, harnessBase(entry.provider, options.overrides))
      && (!opencodeRoutes.has(entry.provider) || (selection !== null && opencodeModelVisible(model.id, selection))));
    // `createProviders` frames the error as a sentence and adds its own full stop.
    return problem ? { ...entry, models, status: "error", error: problem.replace(/\.$/, "") } : { ...entry, models };
  });
  return { entries: visible, problems: [...problems.values()], problemFor: (provider) => problems.get(provider) ?? null };
}
