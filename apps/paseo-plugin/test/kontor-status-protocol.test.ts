import { describe, expect, it, vi } from "vitest";
import { readConfig } from "../server/config";
import { observeKontord, STATUS_OUTPUT_LIMIT } from "../server/kontor-status";

type Chunk = string | Buffer;

/** What the scripted child writes at start and in reply to each method, and every frame the probe sent it. */
const child = vi.hoisted(() => ({
  replies: {} as Record<string, (string | Buffer)[]>,
  sent: [] as string[],
  killed: false,
}));

/**
 * An in-process stand-in for kontor-mcp's stdio: each reply chunk is its own read, after the frame it answers;
 * "<exit>" is the child's exit event, which Node may report before its last output.
 */
vi.mock("node:child_process", async () => {
  const { EventEmitter } = await import("node:events");
  return {
    spawn: () => {
      const stdout = new EventEmitter();
      const emit = async (chunks: (string | Buffer)[] = []) => {
        for (const chunk of chunks) {
          await new Promise(setImmediate);
          if (child.killed) return;
          if (chunk === "<exit>") running.emit("exit", 0, null);
          else stdout.emit("data", Buffer.from(chunk));
        }
      };
      const stdin = Object.assign(new EventEmitter(), {
        write: (text: string) => {
          for (const line of text.split("\n").filter(Boolean)) {
            const { method } = JSON.parse(line) as { method: string };
            child.sent.push(method);
            void emit(child.replies[method]);
          }
          return true;
        },
      });
      const running = Object.assign(new EventEmitter(), { stdin, stdout, kill: () => {
        child.killed = true;
        setImmediate(() => running.emit("close", null, "SIGKILL"));
        return true;
      } });
      void emit(child.replies.start);
      return running;
    },
  };
});

const config = readConfig({ HOME: "/home/test", KONTOR_PLUGIN_STATE_ROOT: "/srv/kontor/state", KONTOR_PLUGIN_MCP_COMMAND: "/opt/kontor/kontor-mcp" });
const frame = (message: object) => `${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`;
const initialized = frame({ id: 1, result: { protocolVersion: "2025-03-26", capabilities: { tools: {} }, serverInfo: { name: "fake", version: "0" } } });
const reply = (result: unknown) => frame({ id: 2, result });
const text = (value: string, isError?: boolean) => ({ content: [{ type: "text", text: value }], ...(isError === undefined ? {} : { isError }) });
const result = (document: object, isError?: boolean) => text(JSON.stringify(document), isError);
const realm = { tool: "kontor_realm_get", status: 200, body: { realm_id: "realm-1" } };

async function probe({ start = [], initialize = [initialized], call = [] }: { start?: Chunk[]; initialize?: Chunk[]; call?: Chunk[] }) {
  child.replies = { start, initialize, "tools/call": call };
  child.sent = [];
  child.killed = false;
  const observation = await observeKontord(config, 1_000);
  return { observation, sent: child.sent, killed: child.killed };
}

const HANDSHAKE = ["initialize", "notifications/initialized", "tools/call"];

describe("kontord status probe protocol", () => {
  it("settles null, array, scalar and malformed frames and results as unknown without throwing", async () => {
    for (const line of ["null", "[]", "5", '"text"', "not json", "{}", '{"id":1}']) {
      expect(await probe({ start: [`${line}\n`] }), line).toEqual({ observation: "unknown", sent: ["initialize"], killed: true });
    }
    const shapes = [null, [], 5, "realm", { content: "realm" }, { content: null }, { content: [] }, text("null"), text("null", true),
      text("[1]"), text("5"), text("{"), { content: [...result(realm).content, ...result(realm).content] }, { ...result(realm), isError: "false" }, { ...result(realm), isError: null }];
    for (const shape of shapes) expect((await probe({ call: [reply(shape)] })).observation, JSON.stringify(shape)).toBe("unknown");
  });

  it("takes one initialize, then one kontor_realm_get, in order, and sends nothing after the answer", async () => {
    expect(await probe({ call: [reply(result(realm))] })).toEqual({ observation: "ready", sent: HANDSHAKE, killed: true });
    // A failed or invalid initialize asks nothing more.
    expect(await probe({ initialize: [frame({ id: 1, error: { code: -32603, message: "no" } })], call: [reply(result(realm))] })).toMatchObject({ observation: "unknown", sent: ["initialize"] });
    expect(await probe({ initialize: [frame({ id: 1, result: {} })], call: [reply(result(realm))] })).toMatchObject({ observation: "unknown", sent: ["initialize"] });
    // An answer before its question, a second initialize, another id or an error ends the probe.
    expect(await probe({ start: [reply(result(realm))] })).toMatchObject({ observation: "unknown", sent: ["initialize"] });
    expect(await probe({ initialize: [initialized + initialized], call: [reply(result(realm))] })).toMatchObject({ observation: "unknown", sent: HANDSHAKE });
    expect(await probe({ call: [frame({ id: 3, result: result(realm) })] })).toMatchObject({ observation: "unknown", sent: HANDSHAKE });
    expect(await probe({ call: [frame({ id: 2, error: { code: -32603, message: "no" } })] })).toMatchObject({ observation: "unknown", sent: HANDSHAKE });
    expect(await probe({ call: [frame({ id: 2, result: result(realm), error: { code: -32603, message: "no" } })] })).toMatchObject({ observation: "unknown" });
    // A frame buffered after the answer is never acted on.
    expect(await probe({ call: [reply(result(realm)) + initialized] })).toEqual({ observation: "ready", sent: HANDSHAKE, killed: true });
    // The child's own notifications are skipped.
    const notice = frame({ method: "notifications/message", params: { level: "info", data: "starting" } });
    expect(await probe({ start: [notice], call: [notice + reply(result(realm))] })).toMatchObject({ observation: "ready", sent: HANDSHAKE });
  });

  it("reads an answer the child wrote before it exited, up to the close of its output", async () => {
    expect(await probe({ call: ["<exit>", reply(result(realm))] })).toMatchObject({ observation: "ready", sent: HANDSHAKE });
  });

  it("is ready only on kontor_realm_get's own 2xx realm, and refused only on its non-2xx error", async () => {
    const cases: [object, string][] = [
      [result(realm, false), "ready"],
      [result({ ...realm, tool: "different_tool" }), "unknown"],
      [result({ tool: "different_tool", status: 401, body: { realm_id: "" } }), "unknown"],
      [result({ ...realm, status: 401 }), "unknown"],
      [result({ ...realm, status: 503 }), "unknown"],
      [result({ ...realm, status: "200" }), "unknown"],
      [result({ ...realm, body: { realm_id: "" } }), "unknown"],
      [result(realm, true), "unknown"],
      [result({ tool: "kontor_realm_get", status: 401, body: { code: "unauthenticated" } }, true), "refused"],
      [result({ tool: "kontor_realm_get", status: 503, body: {} }, true), "refused"],
      [result({ tool: "different_tool", status: 401, body: {} }, true), "unknown"],
      [result({ tool: "kontor_realm_get", code: "unavailable", dispatched: false }, true), "unavailable"],
      [result({ tool: "different_tool", code: "unavailable", dispatched: false }, true), "unknown"],
      [result({ tool: "kontor_realm_get", code: "forbidden_tier", dispatched: false }, true), "unknown"],
    ];
    for (const [value, observation] of cases) expect((await probe({ call: [reply(value)] })).observation, JSON.stringify(value)).toBe(observation);
  });

  it("counts the output limit in bytes, not characters", async () => {
    const wide = reply(result({ ...realm, body: { realm_id: "realm-1", name: "界".repeat(32_768) } }));
    expect(wide.length).toBeLessThan(STATUS_OUTPUT_LIMIT);
    expect(Buffer.byteLength(wide)).toBeGreaterThan(STATUS_OUTPUT_LIMIT);
    expect(await probe({ call: [wide] })).toEqual({ observation: "unknown", sent: HANDSHAKE, killed: true });
    // 75,001 bytes of skipped notification before a valid answer: 25,000 characters, but past the limit.
    const euro = frame({ method: "notifications/message", params: { data: "€".repeat(25_000) } });
    expect(await probe({ call: [euro, reply(result(realm))] })).toEqual({ observation: "unknown", sent: HANDSHAKE, killed: true });
  });
});
