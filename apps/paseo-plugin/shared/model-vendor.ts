/**
 * The vendor that made a model, decided from the model's own ID. A route (OpenCode, an account alias)
 * never decides it: `openrouter/openai/gpt-6.1-terra` is OpenAI's model offered through OpenRouter, and
 * `opencode/glm-5.3-flash-free` is Z.ai's model offered through OpenCode's own gateway. Shared by the
 * session presentation and by the server's route rule, so both read one definition.
 */
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
/**
 * Anthropic's series names. Cursor writes `sonnet-4.5`/`opus-4.5` and OpenCode writes `anthropic/sonnet-5`
 * without the vendor word; they are whole words, so "octopus" is not an Opus model. They classify the vendor
 * only: the model-alias table keeps reading the series as a variant name, as it always has.
 */
const ANTHROPIC_SERIES = new Set(["sonnet", "opus", "haiku"]);
/** The model author an `openrouter/<author>/…` route names; `openrouter/openrouter/…` is the router choosing, so no author. */
const AUTHORS: Record<string, Vendor> = {
  openai: "openai", anthropic: "anthropic", "z-ai": "zai", deepseek: "deepseek", "x-ai": "xai", google: "google", moonshotai: "moonshot", qwen: "alibaba",
  nvidia: "nvidia", cohere: "cohere", liquid: "liquid", xiaomi: "xiaomi", minimax: "minimax", meituan: "meituan", inclusionai: "inclusionai", thinkingmachines: "thinkingmachines",
};
/** A route's pricing tag (":free", "-free") and an instruction-tuned suffix are not the model; a lone word stays. */
const IGNORED = new Set(["free", "it"]);

/** The model's identifying words: the last path segment, without a pricing tag or an instruction-tune suffix. */
export function modelWords(model: string): string[] {
  const all = model.toLocaleLowerCase().split("/").pop()!.replace(/:free$/, "").split(/[-_\s]+/).filter(Boolean);
  const kept = all.filter((part) => !IGNORED.has(part));
  return kept.length ? kept : all;
}

/** The table's own entry for a model word; an inherited Object.prototype name is not a family or an author. */
function ownEntry<T>(table: Readonly<Record<string, T>>, key: string): T | undefined {
  return Object.hasOwn(table, key) ? table[key] : undefined;
}

/** The vendor a family word names, if it names one: `glm` → Z.ai, `claude` → Anthropic. */
export function modelFamily(part: string): readonly [Vendor, string] | undefined {
  return ownEntry(FAMILY, part) ?? ownEntry(WORDS, part);
}

/**
 * The vendor that made the model, from its own ID: a route (OpenCode, an account alias) never decides it;
 * OpenRouter's author segment does. Ids the tables do not name are `unknown`, which no rule hides.
 */
export function modelVendor(model: string | null): Vendor {
  if (!model) return "unknown";
  const id = model.toLocaleLowerCase(), segments = id.split("/");
  if (segments[0] === "openrouter" && segments.length > 2) {
    const author = ownEntry(AUTHORS, segments[1]!);
    return author ?? (segments[1] === "openrouter" ? "unknown" : modelVendor(segments.slice(2).join("/")));
  }
  // OpenCode's own ids name the provider first: `anthropic/sonnet-5` is Anthropic's model and `openai/…` is
  // OpenAI's, exactly as `openrouter/<author>/…` names the author.
  if (segments.length > 1 && (segments[0] === "openai" || segments[0] === "anthropic")) return segments[0];
  if (/(^|\/)o\d/.test(id)) return "openai";
  // OpenAI's computer-use family has no vendor word in the model name.
  if (/(^|\/)computer-use(-|$)/.test(id)) return "openai";
  // OpenAI's speech families omit it too: `whisper-large-v3`, `tts-1`.
  if (/(^|\/)(whisper|tts)(-|$)/.test(id)) return "openai";
  for (const part of modelWords(id)) if (ANTHROPIC_SERIES.has(part)) return "anthropic";
  for (const part of modelWords(id)) { const word = ownEntry(WORDS, part); if (word) return word[0]; }
  for (const [name, [vendor]] of Object.entries(FAMILY)) if (id.includes(name)) return vendor;
  if (id.includes("z-ai") || id.includes("zhipu")) return "zai";
  if (id.includes("gemma")) return "google";
  return "unknown";
}
