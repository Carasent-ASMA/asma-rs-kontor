import type { IncomingMessage, ServerResponse } from "node:http";
import { z } from "zod";
import { exportTokenomics, readTokenomics, tokenomicsQuerySchema } from "../../paseo-plugin/shared/tokenomics";
import { readTokenomicsSnapshot, tokenomicsCsvExport, tokenomicsView } from "../../paseo-plugin/server/tokenomics";

const requestSchema = z.object({ method: z.enum(["tokenomics.read", "tokenomics.export"]), input: tokenomicsQuerySchema }).strict();
const MAX_REQUEST_BYTES = 2 * 1024 * 1024;
const MAX_RESPONSE_BYTES = 32 * 1024 * 1024;

/** Dev-server bridge only: the explicit operator root stays in the server process. */
export function tokenomicsMiddleware(root: string | null) {
  return async (request: IncomingMessage, response: ServerResponse, next: () => void) => {
    if (request.url !== "/__kontor_tokenomics") { next(); return; }
    const send = (status: number, body: unknown) => {
      const json = JSON.stringify(body); if (Buffer.byteLength(json) > MAX_RESPONSE_BYTES) throw new Error("Projection exceeds its response limit");
      response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" }); response.end(json);
    };
    try {
      const host = request.headers.host;
      if (!host || !["127.0.0.1", "localhost", "[::1]"].includes(new URL(`http://${host}`).hostname)
        || (request.headers.origin && request.headers.origin !== `http://${host}`)) { send(403, { error: "Foreign report origin refused" }); return; }
      if (request.method !== "POST") { send(405, { error: "Use a JSON report request" }); return; }
      if (request.headers["content-type"]?.split(";")[0] !== "application/json") { send(415, { error: "Use a JSON report request" }); return; }
      let length = 0; const chunks: Buffer[] = [];
      for await (const chunk of request) { const bytes = Buffer.from(chunk); length += bytes.length; if (length > MAX_REQUEST_BYTES) { send(413, { error: "Report request exceeds its limit" }); return; } chunks.push(bytes); }
      const parsed = requestSchema.safeParse(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      if (!parsed.success) { send(400, { error: "Invalid report request" }); return; }
      const snapshot = await readTokenomicsSnapshot(root);
      send(200, parsed.data.method === "tokenomics.read" ? readTokenomics.output.parse(tokenomicsView(snapshot, parsed.data.input)) : exportTokenomics.output.parse(tokenomicsCsvExport(snapshot, parsed.data.input)));
    } catch { send(400, { error: "Report is unavailable or the request is malformed; refresh source status" }); }
  };
}
