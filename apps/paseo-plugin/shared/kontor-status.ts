import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/**
 * What this host can say about its Kontor realm, asked through the observer `kontor-mcp` only.
 *
 * - `ready`: the realm answered `kontor_realm_get`.
 * - `not-answering`: `kontor-mcp` found the realm but nothing answered on its endpoint.
 * - `refused`: the realm answered and refused the observer read (`httpStatus`).
 * - `no-realm`: `kontor-mcp` stopped before serving: no realm at the configured state root.
 * - `mcp-missing`: the configured `kontor-mcp` command could not be started.
 * - `off`: `KONTOR_PLUGIN_MCP=off`; nothing is asked.
 * - `unknown`: no usable answer within the budget. Never read as absent or healthy.
 *
 * Native Sessions and Paseo-direct never depend on it.
 */
export const kontorStatusSchema = z.object({
  status: z.enum(["ready", "not-answering", "refused", "no-realm", "mcp-missing", "off", "unknown"]),
  realmId: z.string().nullable(),
  realmLabel: z.string().nullable(),
  httpStatus: z.number().int().nullable(),
  checkedAt: z.string(),
});

export type KontorStatus = z.infer<typeof kontorStatusSchema>;

export const readKontorStatus = defineRpc({ name: "kontor.status", input: z.object({}).strict(), output: kontorStatusSchema });
