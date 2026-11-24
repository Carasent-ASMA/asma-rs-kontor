import { afterEach, describe, expect, it, vi } from "vitest";
import { reconcileEndpoint } from "../server/summary-container-reconcile";

const urls: string[] = [];
const hosts: Array<string | null> = [];
vi.mock("@getpaseo/client", () => ({
  createPaseoClient: (options: { url: string }) => { urls.push(options.url); return { connect: async () => {}, close: async () => {} }; },
}));
vi.mock("../server/summary-container-reconcile", async importActual => ({
  ...(await importActual<typeof import("../server/summary-container-reconcile")>()),
  reconcileSummaryContainers: async (_paseo: unknown, _root: string, host: string | null) => { hosts.push(host); return { dryRun: true }; },
}));

const argv = process.argv;
afterEach(() => { process.argv = argv; urls.length = 0; hosts.length = 0; vi.resetModules(); vi.restoreAllMocks(); });

/** Runs the real runner script with one daemon-port argument; the client and the reconcile are stubbed, so no socket opens. */
async function runner(port: string) {
  process.argv = ["bun", "scripts/summary-container-reconcile.ts", port, "/fixture/summary-root"];
  vi.spyOn(process.stdout, "write").mockImplementation(() => true);
  await import("../scripts/summary-container-reconcile");
}

const REJECTED = [
  ["an authority", "6767@example.invalid:80"], ["a host", "example.invalid:6767"], ["a path", "6767/ws"], ["a query", "6767?x"],
  ["leading whitespace", " 6767"], ["trailing whitespace", "6767 "], ["shell punctuation", "6767;true"], ["a sign", "+6767"],
  ["a decimal point", "6767.0"], ["an exponent", "1e3"], ["hex", "0x1a7f"], ["a leading zero", "06767"],
  ["zero", "0"], ["out of range", "65536"], ["far out of range", "999999"], ["empty", ""],
] as const;

describe("summary reconcile runner endpoint (ASMA-8580)", () => {
  it.each(REJECTED)("rejects %s before building any URL", async (_name, port) => {
    expect(() => reconcileEndpoint(port)).toThrow(/decimal integer from 1 to 65535/);
    await expect(runner(port)).rejects.toThrow(/decimal integer from 1 to 65535/);
    expect(urls).toEqual([]);
  });

  it("rejects a missing port", () => {
    expect(() => reconcileEndpoint(undefined)).toThrow(/decimal integer from 1 to 65535/);
  });

  it.each([["1", 1], ["6767", 6767], ["65535", 65535]] as const)("uses the normalised port %s for the loopback URL and the closeout host", async (port, value) => {
    expect(reconcileEndpoint(port)).toEqual({ url: `ws://127.0.0.1:${value}/ws`, host: `127.0.0.1:${value}` });
    await runner(port);
    expect(urls).toEqual([`ws://127.0.0.1:${value}/ws`]);
    expect(hosts).toEqual([`127.0.0.1:${value}`]);
  });
});
