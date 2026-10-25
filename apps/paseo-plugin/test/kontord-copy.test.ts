import { describe, expect, it, vi } from "vitest";
import { kontordBanner, kontordLine, kontordSentence } from "../client/kontord-copy";
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

describe("kontord banner copy", () => {
  const checked = `Last checked ${new Date(0).toLocaleTimeString()}.`;
  it("says connected only when ready, and Navigation-only mode only when ready with the floor off", () => {
    expect(kontordBanner(status("ready", "floor-off"))).toEqual({ title: "Kontor connected · Navigation-only mode", checked,
      details: "You can browse sessions and project information. Automatic agent setup and workspace checks are turned off." });
    expect(kontordBanner(status("ready"))).toEqual({ title: "Kontor connected", checked,
      details: "Automatic agent setup and workspace checks are on, and eligible new agents get Kontor's read-only tools." });
    expect(kontordBanner(status("unavailable", "floor-off"))).toMatchObject({ title: "Kontor not answering", checked });
    expect(kontordBanner(status("refused")).title).toBe("Kontor refused the status check");
    expect(kontordBanner(status("unknown", "floor-off")).details).toBe("The last check could not confirm Kontor's status. You can browse sessions and project information. Automatic agent setup and workspace checks are turned off.");
    for (const observation of ["ready", "unavailable", "refused", "unknown"] as const) {
      for (const composition of ["composed", "mcp-off", "floor-off"] as const) {
        const { title, details } = kontordBanner(status(observation, composition));
        expect(title.includes("connected"), `${observation}/${composition}`).toBe(observation === "ready" && composition !== "mcp-off");
        expect(`${title} ${details}`.includes("Navigation-only"), `${observation}/${composition}`).toBe(observation === "ready" && composition === "floor-off");
      }
    }
  });

  it("names mcp-off as not checked with the floor still on, and gives null no time or mode", () => {
    expect(kontordBanner(status("unknown", "mcp-off"))).toEqual({ title: "Kontor not checked", checked: null,
      details: "The Kontor MCP is off, so this host does not check Kontor and new agents do not get its tools. Automatic agent setup and workspace checks are still on." });
    expect(kontordBanner(null)).toEqual({ title: "Kontor status unknown", checked: null,
      details: "This host has not reported Kontor's status: it is offline, not checked yet, or the check failed." });
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
