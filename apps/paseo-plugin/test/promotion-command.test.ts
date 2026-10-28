import { describe, expect, it, vi } from "vitest";
import { contributePromotionCommand, parsePromotionArguments } from "../client/promotion-command";
import { promoteSessionInput } from "../shared/session-promotion";

describe("promote-adhoc runtime caller", () => {
  it("parses an explicit epic, revision and summary and rejects incomplete usage", () => {
    expect(parsePromotionArguments("ASMA-100 rev-7 Scope: keep it")).toEqual({ epic: "ASMA-100", revision: "rev-7", summary: "Scope: keep it" });
    expect(parsePromotionArguments("ASMA-100 rev-7")).toBeNull();
    expect(parsePromotionArguments("ASMA-100  Scope: keep it")).toBeNull();
  });

  it("sends a refusal or unknown back to the source instead of discarding it", async () => {
    let submitted: ((context: Record<string, unknown>) => Promise<void>) | null = null;
    const send = vi.fn(async () => undefined);
    const rpc = vi.fn(async (_fn: unknown, _input: unknown) => ({ requestId: "00000000-0000-4000-8000-000000000001", state: "refused" as const,
      agentId: "agent", epic: "ASMA-100", canonicalAgentId: null, native: null, logical: null, summarySha256: null,
      summaryRevision: null, acknowledgement: "not-required" as const, message: "not allowed", checkedAt: "2026-10-28T00:00:00Z" }));
    contributePromotionCommand({ addSlashCommand: (contribution: { onSubmit: (context: Record<string, unknown>) => Promise<void> }) => { submitted = contribution.onSubmit; return () => {}; } } as never);
    await submitted!({ args: "ASMA-100 rev-7 Scope: keep it", agent: { id: "agent" }, rpc, paseo: { agents: { ref: () => ({ send }) } } });
    expect(rpc).toHaveBeenCalledOnce();
    expect(promoteSessionInput.safeParse(rpc.mock.calls[0]?.[1]).success).toBe(true);
    expect(send).toHaveBeenCalledWith(expect.stringContaining("Promotion refused for ASMA-100: not allowed"));
  });
});
