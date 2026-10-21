import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { parseDocument } from "yaml";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readResponsibility, responsibilitySchema, type ResponsibilityRead } from "../shared/responsibility";
import { readConfig } from "./config";
import { boundedRead } from "./read-sources";

/** Operator-owned, beside registry.json; clients cannot supply the path. */
export function responsibilityPath(): string {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "responsibility.json");
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).slice(0, 600);

/** One bounded read per request; no cache, refresh or freshness inferred from the file. */
export async function readResponsibilityProjection(file = responsibilityPath()): Promise<ResponsibilityRead> {
  const readAt = new Date().toISOString();
  let raw: string;
  try {
    raw = await boundedRead(file);
  } catch (error) {
    const missing = !!error && typeof error === "object" && "code" in error && error.code === "ENOENT";
    return { status: "unavailable", projection: null, revision: null, readAt,
      diagnostics: [missing ? "No responsibility projection is installed on this host." : `Responsibility projection unreadable: ${message(error)}`] };
  }
  try {
    if (parseDocument(raw).errors.length) throw new Error("Projection must not repeat keys");
    const projection = responsibilitySchema.parse(JSON.parse(raw));
    return { status: "ready", projection, revision: createHash("sha256").update(raw).digest("hex"), readAt, diagnostics: [] };
  } catch (error) {
    return { status: "invalid", projection: null, revision: null, readAt, diagnostics: [`Responsibility projection invalid: ${message(error)}`] };
  }
}

export function contributeResponsibility(server: Pick<PluginServerContext, "handle">): void {
  server.handle(readResponsibility, () => readResponsibilityProjection());
}
