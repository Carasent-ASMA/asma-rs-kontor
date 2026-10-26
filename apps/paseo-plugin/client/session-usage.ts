import type { PaseoAgent } from "@getpaseo/client";

/** Native snapshot fields, copied with each directory update, never accumulated. */
export type SessionTelemetry = Pick<PaseoAgent,
  "runtimeInfo" | "lastUsage" | "thinkingOptionId" | "effectiveThinkingOptionId"
>;

export type SessionUsageEntry = {
  readonly provider: string;
  readonly model: string | null;
  readonly telemetry?: SessionTelemetry;
};

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** Only known model IDs get a family label. Provider/account names are not models. */
export function shortModel(model: string | null): string {
  if (model === null) return "Model unknown";
  if (/^gpt-\d+(?:\.\d+)*-sol(?:-\d{4}-\d{2}-\d{2})?$/.test(model)) return "sol";
  if (/^claude-opus-\d+(?:[-.]\d+)*(?:-\d{8})?$/.test(model)) return "opus";
  if (model === "deepseek/deepseek-flash" || model === "deepseek-flash") return "ds-flash";
  if (/^(?:openrouter\/)?z-ai\/glm-\d+(?:\.\d+)*-flash$/.test(model)) return "glm-flash";
  // Unknown/custom IDs retain their full spelling and may wrap. Never slice two models into one label.
  return model;
}

/** A routed provider names its model as the text `["account","model"]`: the model element, or null for any other value. */
export function routeModel(model: string | null): { readonly account: string; readonly model: string } | null {
  if (!model?.startsWith("[")) return null;
  try {
    const route: unknown = JSON.parse(model);
    return Array.isArray(route) && route.length === 2 && route.every((part) => typeof part === "string" && part.trim()) ? { account: route[0].trim(), model: route[1].trim() } : null;
  } catch { return null; }
}

export function modelPresentation(entry: SessionUsageEntry) {
  const runtime = entry.telemetry?.runtimeInfo;
  const observed = text(runtime?.extra?.runtimeModel);
  const configured = text(entry.model);
  const selection = text(runtime?.model);
  const model = observed ?? selection ?? configured;
  let source = "Model unknown";
  if (observed) source = "Last reported runtime model";
  else if (selection) source = "Runtime model selection";
  else if (configured) source = "Configured model";
  const qualifier = observed ? "reported" : "configured";
  // The row names the route's model; `model` keeps the full route for Details and help.
  const route = routeModel(model), short = shortModel(route?.model ?? model);
  return {
    model, source, configured, selection, route, short,
    label: model === null ? short : `${short} · ${qualifier}`,
  };
}

export type Vendor = "openai" | "anthropic" | "zai" | "deepseek" | "xai" | "google" | "cursor" | "moonshot" | "alibaba"
  | "nvidia" | "cohere" | "liquid" | "xiaomi" | "minimax" | "meituan" | "inclusionai" | "thinkingmachines" | "unknown";

/** Family words that name a vendor rather than a variant: [vendor, short mark used when a model has no version]. */
const FAMILY: Record<string, readonly [Vendor, string]> = {
  gpt: ["openai", "G"], codex: ["openai", "CX"], claude: ["anthropic", "C"], glm: ["zai", "G"], deepseek: ["deepseek", "DS"], grok: ["xai", "G"],
  gemini: ["google", "G"], composer: ["cursor", "C"], kimi: ["moonshot", "K"], qwen: ["alibaba", "Q"],
};
/** Families matched only as a whole word, since their names occur inside other words ("ling" in "inkling"). */
const WORDS: Record<string, readonly [Vendor, string]> = {
  gemma: ["google", "G"], nemotron: ["nvidia", "N"], longcat: ["meituan", "LC"], mimo: ["xiaomi", "MM"], minimax: ["minimax", "MX"],
  inkling: ["thinkingmachines", "I"], ling: ["inclusionai", "L"], lfm: ["liquid", "L"],
};
/** The model author an `openrouter/<author>/…` route names; `openrouter/openrouter/…` is the router choosing, so no author. */
const AUTHORS: Record<string, Vendor> = {
  openai: "openai", anthropic: "anthropic", "z-ai": "zai", deepseek: "deepseek", "x-ai": "xai", google: "google", moonshotai: "moonshot", qwen: "alibaba",
  nvidia: "nvidia", cohere: "cohere", liquid: "liquid", xiaomi: "xiaomi", minimax: "minimax", meituan: "meituan", inclusionai: "inclusionai", thinkingmachines: "thinkingmachines",
};
const family = (part: string) => FAMILY[part] ?? WORDS[part];

/** A route's pricing tag (":free", "-free") and an instruction-tuned suffix are not the model; a lone word stays. */
const IGNORED = new Set(["free", "it"]);
const words = (model: string) => {
  const all = model.toLocaleLowerCase().split("/").pop()!.replace(/:free$/, "").split(/[-_\s]+/).filter(Boolean);
  const kept = all.filter((part) => !IGNORED.has(part));
  return kept.length ? kept : all;
};

/** The vendor that made the model, from its own ID: a route (OpenCode, an account alias) never decides it; OpenRouter's author segment does. */
export function modelVendor(model: string | null): Vendor {
  if (!model) return "unknown";
  const id = model.toLocaleLowerCase(), segments = id.split("/");
  if (segments[0] === "openrouter" && segments.length > 2) return AUTHORS[segments[1]!] ?? (segments[1] === "openrouter" ? "unknown" : modelVendor(segments.slice(2).join("/")));
  if (/(^|\/)o\d/.test(id)) return "openai";
  for (const part of words(id)) if (WORDS[part]) return WORDS[part]![0];
  for (const [name, [vendor]] of Object.entries(FAMILY)) if (id.includes(name)) return vendor;
  if (id.includes("z-ai") || id.includes("zhipu")) return "zai";
  if (id.includes("gemma")) return "google";
  return "unknown";
}

/**
 * At most four characters: the variant's initial and its version ("gpt-6-astra" → "A6", "claude-opus-5-5" → "O5.5"),
 * the family's mark and the variant's initial for a model with no version ("deepseek-flash" → "DSF"). A 1M-context
 * variant drops the version's dot for an M ("claude-opus-4-8[1m]" → "O48M"); a model whose only variant is its size
 * adds that size ("gemma-4-31b-it" → "G431"). Distinct models of one vendor differ; one model on several routes keeps
 * one alias, and its route tells them apart. The full ID stays in the picker, Details and search.
 */
export function modelAlias(model: string | null): string {
  if (!model) return "?";
  const wide = /\[1m\]$/i.test(model);
  const parts = words(model.replace(/\[1m\]$/i, ""));
  const size = (part: string) => /^a?\d+(\.\d+)?b$/.test(part);
  // Dates and build numbers are not versions.
  const version = parts.filter((part) => /^\d{1,3}(\.\d{1,3})*$/.test(part)).join(".");
  const named = parts.filter((part) => !/^\d/.test(part) && !size(part) && !family(part));
  const known = parts.find((part) => family(part));
  const initial = (named[0] ?? known ?? parts[0] ?? "?")[0]!.toLocaleUpperCase();
  const sized = !named.length ? parts.find((part) => /^\d+(\.\d+)?b$/.test(part))?.slice(0, -1).replace(".", "") ?? "" : "";
  const alias = version ? wide ? `${initial}${version.replace(/\./g, "")}M` : `${initial}${version}${sized}`
    : known ? `${family(known)![1]}${named.map((part) => part[0]!.toLocaleUpperCase()).join("")}` : (named[0] ?? "?").toLocaleUpperCase();
  return alias.slice(0, 4).replace(/\.$/, "");
}

const EFFORTS: Record<string, string> = { minimal: "MIN", low: "L", medium: "M", high: "H", xhigh: "XH", max: "MAX" };

/** "XH" for xhigh; an effort the table does not know keeps its first three letters. */
export function effortShort(effort: string | null | undefined): string | null {
  if (!effort) return null;
  return EFFORTS[effort.toLocaleLowerCase()] ?? effort.slice(0, 3).toLocaleUpperCase();
}

const ROUTES: Record<string, string> = { codex: "Codex", claude: "Claude Code", opencode: "OpenCode", cursor: "Cursor", "glm-acp-agent": "GLM Agent", copilot: "Copilot" };

/** The route a model runs through, as a subtitle: "via OpenCode". */
export function routeLabel(provider: string, label?: string | null): string {
  return `via ${label?.trim() || ROUTES[provider] || provider}`;
}

/** The row's model line: the vendor of the model it reports, its alias, effort and the full ID for help. */
export function rowModel(entry: SessionUsageEntry) {
  const presentation = modelPresentation(entry);
  const model = presentation.route?.model ?? presentation.model;
  const effort = effortShort(entry.telemetry?.effectiveThinkingOptionId ?? entry.telemetry?.thinkingOptionId ?? entry.telemetry?.runtimeInfo?.thinkingOptionId ?? null);
  return { vendor: modelVendor(model), alias: modelAlias(model), effort, full: model ?? "Model unknown", route: routeLabel(entry.telemetry?.runtimeInfo?.provider ?? entry.provider), source: presentation.source };
}

export function validTokenCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

// Compact row units stay K/M across device locales; exact Details keep localized numbers.
// Hermes on iOS ignores Intl's compact notation (it printed 4,078), so the units are computed here exactly as
// "en" compact with one decimal: half away from zero, and a value that rounds up to 1000 moves to the next unit.
const UNITS = [[1e3, "K"], [1e6, "M"], [1e9, "B"], [1e12, "T"]] as const;
const exactTokenNumber = new Intl.NumberFormat();

export function formatTokens(value: number, _locale?: string): string {
  if (value < 1_000) return String(value);
  let unit = value >= 1e12 ? 3 : value >= 1e9 ? 2 : value >= 1e6 ? 1 : 0;
  // An integer over size / 10 is exact at .5, so Math.round rounds half up like Intl.
  const tenths = (index: number) => Math.round(value / (UNITS[index]![0] / 10)) / 10;
  if (tenths(unit) >= 1_000 && unit < UNITS.length - 1) unit++;
  return `${tenths(unit)}${UNITS[unit]![1]}`;
}

/** Format an exact report, preserving absent/invalid values and explicit zero. */
export function exactTokens(value: unknown, locale?: string): string {
  if (value === undefined || value === null) return "Not reported";
  const formatter = locale === undefined ? exactTokenNumber : new Intl.NumberFormat(locale);
  return validTokenCount(value) ? formatter.format(value) : "Invalid report (not shown)";
}

// An opaque route cannot establish which underlying adapter is in use.
const adapterOf = (entry: SessionUsageEntry) => entry.telemetry?.runtimeInfo?.provider ?? entry.provider;

/**
 * The one reading of Paseo's lastUsage: the latest provider report, which each report replaces.
 * Paseo supplies no session total, so none is derived here. Only the adapter contract decides
 * whether cache reads overlap input; counter values never do, so nothing is counted twice.
 */
export function latestReport(entry: SessionUsageEntry) {
  const usage = entry.telemetry?.lastUsage;
  const reported = validTokenCount(usage?.inputTokens) ? usage.inputTokens : null;
  const cached = validTokenCount(usage?.cachedInputTokens) ? usage.cachedInputTokens : null;
  const adapter = adapterOf(entry);
  // Claude and OpenCode report cache reads apart from input. Codex input includes them.
  const separate = adapter === "claude" || adapter === "opencode";
  const added = separate && reported !== null && cached !== null;
  const sum = added ? reported + cached : reported;
  const inputOverflow = sum !== null && !Number.isSafeInteger(sum);
  return {
    input: inputOverflow ? null : sum,
    inputOverflow,
    addedCacheReads: added ? cached : null,
    // An unidentified route does not say whether input includes cache reads, so they stay apart.
    unplacedCacheReads: separate || adapter === "codex" ? null : cached,
    // Only Codex input is complete (cache reads included, no cache writes); elsewhere it is a lower bound.
    lowerBound: reported !== null && adapter !== "codex",
    output: validTokenCount(usage?.outputTokens) ? usage.outputTokens : null,
  };
}

/** Row text and spoken label for the latest report; never a session total. */
export function usagePresentation(entry: SessionUsageEntry, locale?: string) {
  const usage = entry.telemetry?.lastUsage;
  const report = latestReport(entry);
  const input = report.input === null ? "—" : `${report.lowerBound ? "≥" : ""}${formatTokens(report.input, locale)}`;
  const output = report.output === null ? "—" : formatTokens(report.output, locale);
  const unplaced = report.unplacedCacheReads === null ? "" : ` · ${formatTokens(report.unplacedCacheReads, locale)} cached`;
  const spokenInput = report.inputOverflow ? "unknown; input plus cache reads exceeds the exact integer range"
    : report.input === null ? exactTokens(usage?.inputTokens, locale)
      : `${report.lowerBound ? "at least " : ""}${exactTokens(report.input, locale)}${report.addedCacheReads === null ? "" : `, including ${exactTokens(report.addedCacheReads, locale)} cache reads`}`;
  const spokenCache = report.unplacedCacheReads === null ? "" : ` Cache reads: ${exactTokens(report.unplacedCacheReads, locale)}; whether input includes them is unknown for this route.`;
  return {
    label: report.input === null && report.output === null && report.unplacedCacheReads === null ? "—" : `Last report ${input} ↓ · ${output} ↑${unplaced}`,
    spoken: `Last usage report, not a session total. Input tokens: ${spokenInput}. Output tokens: ${exactTokens(usage?.outputTokens, locale)}.${spokenCache}`,
  };
}

export function sessionUsageLabel(entry: SessionUsageEntry): string {
  const model = modelPresentation(entry);
  return `${model.source}: ${model.model ?? "Unknown"}. ${usagePresentation(entry).spoken}`;
}

/** Reporting limits for the provider actually identified by this snapshot. */
export function usageScope(entry: SessionUsageEntry): string {
  const provider = adapterOf(entry);
  if (provider === "codex") return "Codex tokenUsage.last: the latest model request, not the turn or the session. Cached input is included in input; it is not added again. Output includes reasoning. Paseo does not pass on the thread total.";
  if (provider === "claude") return "Claude result for the latest turn. Input excludes cache reads and cache writes; the row adds cache reads. Cache-write tokens are not exposed, so input is a lower bound. Output includes thinking. Streaming reports may contain context only.";
  if (provider === "opencode") return "OpenCode report. v1 reports the latest step; v2 projects session counters. This snapshot does not identify the adapter version. Input excludes cache reads; the row adds them. Cache-write and reasoning counts are not exposed, so counts are lower bounds.";
  return "Provider-specific latest report; the reporting window and cache inclusion are not established for this route. Cache reads are shown apart and never added to input. No session total can be calculated.";
}

export function sessionUsageLines(entry: SessionUsageEntry, stale = false, locale?: string): { label: string; value: string }[] {
  const model = modelPresentation(entry);
  const native = entry.telemetry;
  return [
    { label: "Model", value: model.model ?? "Unknown: the native snapshot did not supply a model" },
    { label: "Model source", value: model.source === "Last reported runtime model" ? "Native runtimeInfo.extra.runtimeModel (last observed). May precede a model switch; not proof of the active call's model." : `${model.source}. runtimeInfo.model can be configuration; actual runtime execution is not established.` },
    { label: "Configured model", value: model.configured ?? "Not reported" },
    { label: "Runtime selection", value: model.selection ?? "Not reported" },
    { label: "Provider / route", value: entry.provider },
    { label: "Runtime provider", value: text(native?.runtimeInfo?.provider) ?? "Not reported" },
    { label: "Effort", value: `Effective: ${text(native?.effectiveThinkingOptionId) ?? "not reported"}; configured: ${text(native?.thinkingOptionId) ?? "not reported"}; runtime selection: ${text(native?.runtimeInfo?.thinkingOptionId) ?? "not reported"}` },
    { label: "Reported input tokens", value: exactTokens(native?.lastUsage?.inputTokens, locale) },
    { label: "Reported output tokens", value: exactTokens(native?.lastUsage?.outputTokens, locale) },
    { label: "Reported cache-read tokens", value: `${exactTokens(native?.lastUsage?.cachedInputTokens, locale)}. Inclusion depends on the adapter (see report scope). Added to the row's input only for adapters that report them apart (Claude, OpenCode).` },
    { label: "Report source and scope", value: `Native lastUsage. ${usageScope(entry)}` },
    { label: "Report freshness", value: `${stale ? "Host inventory is stale/offline; showing last known fields. " : ""}No usage observation timestamp is supplied. Session last activity is not the usage time; age is unknown, even on a live connection.` },
    { label: "Report limits", value: "Input and output are separate reported counters, not a whole-session consumed total or context occupancy. Model attribution is unknown, including after model/provider switches. Repeated reports replace the snapshot; decreases/resets are not negative consumption. Missing counters stay unknown; explicit zero stays zero. No history is reconstructed." },
  ];
}
