import { createServer, request as httpRequest, type Server } from "node:http";
import { once } from "node:events";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { tokenomicsMiddleware } from "../server/tokenomics";
import { tokenomicsDataSchema, tokenomicsQuerySchema, tokenomicsViewSchema } from "../../paseo-plugin/shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "../../paseo-plugin/test/fixtures/tokenomics";

const servers: Server[] = [], folders: string[] = [];
afterEach(async () => { await Promise.all(servers.splice(0).map((s) => new Promise<void>((done) => s.close(() => done())))); await Promise.all(folders.splice(0).map((p) => rm(p, { recursive: true, force: true }))); });
async function start(root: string | null) {
  const middleware = tokenomicsMiddleware(root), server = createServer((req, res) => { void middleware(req, res, () => { res.writeHead(404); res.end(); }); });
  servers.push(server); server.listen(0, "127.0.0.1"); await once(server, "listening");
  const address = server.address(); if (!address || typeof address === "string") throw new Error("No test address");
  const host = `127.0.0.1:${address.port}`;
  return async (body: unknown = { method: "tokenomics.read", input: tokenomicsQuerySchema.parse({ show: "all" }) }, headers: Record<string, string> = {}, method = "POST", url = "/__kontor_tokenomics") => {
    const text = typeof body === "string" ? body : JSON.stringify(body);
    return new Promise<{ status: number; body: unknown; text: string; headers: import("node:http").IncomingHttpHeaders }>((resolve, reject) => {
      const req = httpRequest({ host: "127.0.0.1", port: address.port, path: url, method, headers: { Host: host, Origin: `http://${host}`, "Content-Type": "application/json", ...headers } }, (res) => {
        const chunks: Buffer[] = []; res.on("data", (data: Buffer) => chunks.push(data)); res.on("end", () => { const text = Buffer.concat(chunks).toString(); resolve({ status: res.statusCode!, body: text ? JSON.parse(text) : null, text, headers: res.headers }); });
      }); req.on("error", reject); req.end(method === "GET" ? undefined : text);
    });
  };
}
async function report() {
  const p = await mkdtemp(join(tmpdir(), "kontor-report-preview-")); folders.push(p); await mkdir(join(p, "_stats"));
  await writeFile(join(p, "data.json"), JSON.stringify(tokenomicsFixture())); await writeFile(join(p, "_stats/live.js"), `window.ASMA_LIVE=${JSON.stringify(tokenomicsLiveFixture())};`); return p;
}

describe("optional local native report preview", () => {
  it("defaults to unavailable without discovering a report or returning a private path", async () => {
    const call = await start(null), result = await call();
    expect(result.status).toBe(200); expect(tokenomicsViewSchema.parse(result.body).status).toBe("unavailable");
    expect(result.text).not.toContain("/Users/"); expect(result.headers["cache-control"]).toBe("no-store");
  });
  it("uses the native reader and validated page projection/export without exposing the report root", async () => {
    const root = await report(), call = await start(root), result = await call();
    const view = tokenomicsViewSchema.parse(result.body); expect(view.tiles.find((t) => t.label === "Tokens processed")!.value).toBe(1400);
    expect(result.text).not.toContain(root); expect(tokenomicsDataSchema.safeParse(result.body).success).toBe(false);
    const csv = await call({ method: "tokenomics.export", input: { page: "pivot", groups: ["model"] } });
    expect(csv.status).toBe(200); expect(csv.text).toContain("future-model"); expect(csv.headers["x-content-type-options"]).toBe("nosniff");
  });
  it("rejects foreign/rebinding hosts, foreign or null origins and unsupported requests", async () => {
    const call = await start(await report());
    const refused: Record<string, string>[] = [{ Host: "evil.example" }, { Origin: "https://evil.example" }, { Origin: "null" }];
    for (const headers of refused) expect((await call(undefined, headers)).status).toBe(403);
    expect((await call(undefined, {}, "GET")).status).toBe(405);
    expect((await call(undefined, { "Content-Type": "text/plain" })).status).toBe(415);
    expect((await call(undefined, {}, "POST", "/elsewhere")).status).toBe(404);
  });
  it("rejects client paths, executable source, malformed JSON and oversized request bodies", async () => {
    const root = await report(), call = await start(root);
    expect((await call({ method: "tokenomics.read", input: { path: "/private/secret" } })).status).toBe(400);
    expect((await call("bad JSON")).status).toBe(400);
    expect((await call("x".repeat(2 * 1024 * 1024 + 1))).status).toBe(413);
    await writeFile(join(root, "_stats/live.js"), "window.ASMA_LIVE = (() => { throw 'must not execute'; })();");
    const result = await call({ method: "tokenomics.read", input: { page: "dv-overview" } });
    const view = tokenomicsViewSchema.parse(result.body); expect(view.status).toBe("partial"); expect(view.tiles).toEqual([]);
  });
});
