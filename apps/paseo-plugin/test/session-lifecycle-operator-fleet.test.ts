import { describe, expect, it, vi } from "vitest";

const operatorRead = vi.hoisted(() => vi.fn(async () => ({
  status: "ready" as const,
  sha256: "fleet-sha",
  readAt: "2026-10-25T09:00:00.000Z",
  data: {
    kind: "fleet" as const,
    bindings: [{ chainResolved: true, reference: "advisor/route-review", chainName: "claude-first" }],
    domains: [{ accounts: ["claude-a", "claude-b"] }],
  },
  diagnostics: [] as const,
})));

vi.mock("../server/teams-source-adapter", () => ({
  createTeamsSourceAdapter: vi.fn(() => ({ read: operatorRead })),
}));

vi.mock("../server/read-sources", async (importOriginal) => {
  const orig = await importOriginal<typeof import("../server/read-sources")>();
  return {
    ...orig,
    sourceCatalog: () => ({ sources: [{ id: "operator", title: "operator", kind: "teams" as const, path: "/fixture/operator-fleet.yml" }], diagnostics: [] }),
  };
});

import { readOperatorFleetDiscovery } from "../server/session-lifecycle";

describe("operator fleet discovery for takeover", () => {
  it("derives profile choices and account catalog from one configured-source read", async () => {
    operatorRead.mockClear();
    const result = await readOperatorFleetDiscovery();
    expect(operatorRead).toHaveBeenCalledTimes(1);
    expect(operatorRead).toHaveBeenCalledWith({ sourceId: "operator" });
    expect(result.catalog.revision).toBe("fleet-sha");
    expect(result.choices).toEqual([{ bindingKey: "advisor/route-review", chain: "claude-first" }]);
    expect(result.catalog.accounts.has("claude-a")).toBe(true);
  });
});
