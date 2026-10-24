import { baseSessions, type FixtureAgent, type FixtureSession } from "./sessions";

/** Fictional metadata only. No captured host inventory, identities or filesystem paths. */
export function modelUsageSessions(): FixtureSession[] {
  const seed = baseSessions()[0]!;
  const variants: readonly Partial<FixtureAgent>[] = [
    { title: "Model report • Codex selected model", provider: "codex", model: "gpt-6.1-sol", thinkingOptionId: "high", effectiveThinkingOptionId: "xhigh", runtimeInfo: { sessionId: null, provider: "codex", model: "gpt-6.1-sol", thinkingOptionId: "xhigh" }, lastUsage: { inputTokens: 12_400, cachedInputTokens: 10_000, outputTokens: 320 } },
    { title: "Model report • Observed runtime differs from configuration", provider: "claude", model: "claude-opus-5-5", runtimeInfo: { sessionId: null, provider: "claude", model: "claude-opus-5-5", extra: { runtimeModel: "claude-opus-5-4-20260901" } }, lastUsage: { inputTokens: 24, cachedInputTokens: 12_000, outputTokens: 1_234_567 } },
    { title: "Model report • OpenCode flash with cache counters", provider: "opencode", model: "deepseek/deepseek-flash", lastUsage: { inputTokens: 36_400, outputTokens: 501, cachedInputTokens: 31_200 } },
    { title: "Model report • GLM selected, explicit zero", provider: "opencode", model: "openrouter/z-ai/glm-5.3-flash", lastUsage: { inputTokens: 0, outputTokens: 0 } },
    { title: "Model report • Opaque route and missing model", provider: "account-route-seven", model: null, lastUsage: { outputTokens: 320 } },
    { title: "Model report • Unknown custom model and very long title for responsive layout and localization проверки", provider: "custom-route", model: "custom-model-with-a-long-exact-version-123456789012345678901234567890-and-no-known-short-name", lastUsage: { inputTokens: 1_234_567, outputTokens: 123_456 } },
    { title: "Model report • Context-only snapshot is not consumption", provider: "cursor", model: "fable", lastUsage: { contextWindowUsedTokens: 80_000, contextWindowMaxTokens: 200_000 } },
    { title: "Model report • Invalid report stays unknown", provider: "another-private-route", model: null, lastUsage: { inputTokens: -10, outputTokens: 2 } },
    { title: "Model report • Routed model keeps account in Details", provider: "opencode", model: '["fictional-route","gpt-6.1-sol"]', lastUsage: { inputTokens: 4_078, outputTokens: 1_234_567 } },
  ];
  return variants.map((variant, index) => ({ ...seed, agent: { ...seed.agent, id: `agt_model_report_${index}`, labels: { workstream: "Model report examples", role: index % 2 ? "QA" : "SWE" }, ...variant } }));
}
