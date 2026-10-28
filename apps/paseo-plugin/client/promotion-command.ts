import type { PluginClientContext } from "@getpaseo/plugin/client";
import { newRequestId } from "./request-id";
import { promoteSession, type PromotionOutcome } from "../shared/session-promotion";

export function parsePromotionArguments(args: string): { epic: string; revision: string; summary: string } | null {
  const text = args.trim(), first = text.indexOf(" "), second = first < 0 ? -1 : text.indexOf(" ", first + 1);
  if (first < 0 || second < 0 || !text.slice(first + 1, second).trim() || !text.slice(second + 1).trim()) return null;
  return { epic: text.slice(0, first).trim(), revision: text.slice(first + 1, second).trim(), summary: text.slice(second + 1).trim() };
}

const resultText = (outcome: PromotionOutcome) => `Promotion ${outcome.state}${outcome.epic ? ` for ${outcome.epic}` : ""}: ${outcome.message}`;

/** `/promote-adhoc <epic> <revision> <summary>` is the runtime caller for same-session promotion. */
export function contributePromotionCommand(client: PluginClientContext): void {
  client.addSlashCommand({
    name: "promote-adhoc",
    description: "Promote this ADHOC LSA in place, or hand its exact summary to the existing epic LSA.",
    argumentHint: "ASMA-123 <summary-revision> <Scope/Artifacts/Verified/Unverified/Next-action summary>",
    context: "agent",
    async onSubmit(context) {
      const parsed = parsePromotionArguments(context.args);
      const requestId = newRequestId();
      let message: string;
      if (!parsed) message = `Promotion not sent (${requestId}): use /promote-adhoc <epic> <summary-revision> <complete summary>.`;
      else {
        try {
          const outcome = await context.rpc(promoteSession, { requestId, agentId: context.agent.id, epic: parsed.epic,
            summary: { revision: parsed.revision, text: parsed.summary } });
          message = `${resultText(outcome)} Request ${requestId}.`;
        } catch (failure) { message = `Promotion state unknown for request ${requestId}: ${failure instanceof Error ? failure.message : String(failure)}. Do not retry blindly; read the request first.`; }
      }
      try { await context.paseo.agents.ref(context.agent.id).send(message); }
      catch (failure) { throw new Error(`Promotion result for request ${requestId} could not be delivered to the source: ${failure instanceof Error ? failure.message : String(failure)}.`); }
    },
  });
}
