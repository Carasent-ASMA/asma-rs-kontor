import { describe, expect, it, vi } from "vitest";
import { kontordLine, kontordSentence } from "../client/kontord-copy";
import { useKontorStatus } from "../client/use-kontor-status";
import type { KontorStatus } from "../shared/kontor-status";

const query = vi.hoisted(() => ({ options: null as null | { queryKey: unknown[]; enabled: boolean; retry: boolean; refetchInterval: number | false }, data: undefined as unknown, isError: false }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => async () => query.data }));
vi.mock("@tanstack/react-query", () => ({ useQuery: (options: typeof query.options) => { query.options = options; return { data: query.data, isError: query.isError }; } }));

const at = new Date(0).toISOString();
const status = (observation: KontorStatus["observation"], composition: KontorStatus["composition"] = "composed"): KontorStatus => ({ observation, composition, checkedAt: at });

describe("kontord copy", () => {
  it("names each observation apart from composition, and never says not enabled", () => {
    expect(kontordSentence(status("ready"))).toBe("kontord on this host: answering.");
    expect(kontordSentence(status("unavailable"))).toBe("kontord on this host: not answering.");
    expect(kontordSentence(status("refused"))).toBe("kontord on this host: refused the observer read.");
    expect(kontordSentence(status("unknown"))).toBe("kontord on this host: unknown.");
    expect(kontordSentence(status("unknown", "mcp-off"))).toBe("kontord on this host: not checked; the Kontor MCP is off.");
    expect(kontordSentence(status("ready", "floor-off"))).toBe("kontord on this host: answering.");
    const checked = `Checked ${new Date(0).toLocaleTimeString()}.`;
    expect(kontordLine(status("ready"))).toBe(`kontord on this host: answering. Kontor adds its observer MCP to eligible new agents. ${checked}`);
    expect(kontordLine(status("unknown", "mcp-off"))).toBe(`kontord on this host: not checked; the Kontor MCP is off. Kontor does not add its observer MCP: the Kontor MCP is off. ${checked}`);
    expect(kontordLine(status("unavailable", "floor-off"))).toBe(`kontord on this host: not answering. Kontor does not add its observer MCP: the floor is off. ${checked}`);
    for (const observation of ["ready", "unavailable", "refused", "unknown"] as const) {
      for (const composition of ["composed", "mcp-off", "floor-off"] as const) expect(kontordLine(status(observation, composition))).not.toContain("not enabled");
    }
  });

  it("reads an offline, unread or failed status as unknown, with no composition or check time", () => {
    expect(kontordSentence(null)).toBe("kontord on this host: unknown.");
    expect(kontordLine(null)).toBe("kontord on this host: unknown.");
  });
});

describe("useKontorStatus", () => {
  it("reads only while the host is online, every 30 seconds, and gives null for offline, unread or failed", () => {
    query.data = status("ready"); query.isError = false;
    expect(useKontorStatus("host-a", "online")).toEqual(status("ready"));
    expect(query.options).toMatchObject({ queryKey: ["kontor-status", "host-a"], enabled: true, retry: false, refetchInterval: 30_000 });
    expect(useKontorStatus("host-a", "offline")).toBeNull();
    expect(query.options).toMatchObject({ enabled: false, refetchInterval: false });
    query.isError = true;
    expect(useKontorStatus("host-a", "online")).toBeNull();
    query.data = undefined; query.isError = false;
    expect(useKontorStatus("host-a", "online")).toBeNull();
  });
});
