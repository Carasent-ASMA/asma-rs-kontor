import type { PluginClientContext } from "@getpaseo/plugin/client";
import { newRequestId } from "./new-lsa-form";
import { promoteSession } from "../shared/session-promotion";

/** `/promote-adhoc <epic> <revision> <summary>` is the runtime caller for same-session promotion. */
export function contributePromotionCommand(client: PluginClientContext): void {
  client.addSlashCommand({
    name: "promote-adhoc",
    description: "Promote this ADHOC LSA in place, or hand its exact summary to the existing epic LSA.",
    argumentHint: "ASMA-123 <summary-revision> <Scope/Artifacts/Verified/Unverified/Next-action summary>",
    context: "agent",
    async onSubmit(context) {
      const text = context.args.trim(), first = text.indexOf(" "), second = first < 0 ? -1 : text.indexOf(" ", first + 1);
      if (first < 0 || second < 0 || !text.slice(second + 1).trim()) return;
      await context.rpc(promoteSession, {
        requestId: newRequestId(), agentId: context.agent.id, epic: text.slice(0, first).trim(),
        summary: { revision: text.slice(first + 1, second).trim(), text: text.slice(second + 1).trim() },
      });
    },
  });
}
