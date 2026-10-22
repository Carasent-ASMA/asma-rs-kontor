import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/**
 * The host's Kontor status, as two separate facts.
 *
 * `observation` is what the realm answered to the one observer read through `kontor-mcp`:
 * - `ready`: an authenticated answer; a completed handshake alone is not readiness;
 * - `unavailable`: `kontor-mcp` reached for the realm and got no usable answer;
 * - `refused`: the realm answered and refused the read;
 * - `unknown`: anything not proven, including a missing command, credential or realm, a start failure,
 *   a timeout or an unexpected answer. It never means "no realm" or "not enabled".
 *
 * `composition` is configuration, not an observation: whether `agent.create` gives new agents the
 * `kontor-observer` MCP (`floor-off` wins over `mcp-off`). With `mcp-off`, nothing is asked.
 */
export const kontorStatusSchema = z.object({
  observation: z.enum(["ready", "unavailable", "refused", "unknown"]),
  composition: z.enum(["composed", "mcp-off", "floor-off"]),
  checkedAt: z.string(),
}).strict();

export type KontorStatus = z.infer<typeof kontorStatusSchema>;

export const readKontorStatus = defineRpc({ name: "kontor.status", input: z.object({}).strict(), output: kontorStatusSchema });
