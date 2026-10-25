import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  GOVERNED_ELSEWHERE, resolveTakeoverRoute, resolverInstant, takeoverAdapter, takeoverRouteInputSchema,
  type ResolverRun, type RouteHealth,
} from "../server/session-takeover";

const KEY = "team/01936f5a-0000-7000-8000-000000000102/implement";
const REQUEST = "0f1e2d3c-4b5a-4968-8776-655443322110";
const config = { cliCommand: "/opt/kontor", stateRoot: "/srv/kontor-state" };
const ordinary = takeoverAdapter({ labels: {} });
const health: RouteHealth = {
  as_of: "2026-10-25T09:00:00Z", max_age_seconds: 60, mode: "default",
  observations: [
    { account: "codex-work", model: "gpt-5.6-sol", effort: "xhigh", mode: "default", state: "unknown", observed_at: "2026-10-25T08:59:59Z", revision: "r1" },
    { account: "claude-personal", model: "claude-opus-5", effort: "xhigh", mode: "default", state: "available", observed_at: "2026-10-25T08:59:30Z", revision: "r2" },
  ],
};

const route = (provider: string, model: string, step: number) =>
  ({ rung: { provider, model, effort: "xhigh" }, step, sub_step: 1, vendor: provider.startsWith("codex") ? "openai" : "anthropic" });
/** A FleetSelection as the CLI prints it: keys sorted, the stated evidence echoed. */
function selection(excluded: [string | null, string | null], selected: boolean, overrides: Record<string, unknown> = {}) {
  const considered = [route("codex-work", "gpt-5.6-sol", 1), route("claude-personal", "claude-opus-5", 2)]
    .map((entry, index) => ({ ...(excluded[index] ? { excluded: excluded[index] } : {}), route: entry }));
  return { considered, eligibility: { excluded_vendors: [], unavailable_accounts: [] }, excluded_by_policy: [],
    provenance: { binding_key: KEY, chain: "codex-first", policy_hash: "a".repeat(64), schema_version: 2 },
    route_health: health, selected: selected ? route("claude-personal", "claude-opus-5", 2) : null, ...overrides };
}
const answered = (body: unknown) => ({ body, status: 200, tool: "kontor_fleet_policy_resolve" });
const blocked = (body: unknown) => ({ body: { code: "placement_blocked", message: "no route", selection: body }, status: 409, tool: "kontor_fleet_policy_resolve" });

function fake(document: unknown) {
  const calls: { command: string; args: string[] }[] = [];
  const run: ResolverRun = async (command, args) => {
    calls.push({ command, args });
    return typeof document === "string" ? document : JSON.stringify(document);
  };
  return { calls, run };
}
const resolve = (document: unknown, input: unknown = { agentId: "agent-1", bindingKey: KEY }, evidence: RouteHealth = health) => {
  const runner = fake(document);
  return resolveTakeoverRoute(config, ordinary, input, evidence, runner.run).then((result) => ({ result, calls: runner.calls }));
};

describe("takeover ownership", () => {
  it("decides the adapter only from protected ownership evidence", () => {
    expect(takeoverAdapter({ labels: {} })).toEqual({ adapter: "direct", basis: "ordinary", owner: null, createRequest: null });
    // Role, title and persona are never authority.
    expect(takeoverAdapter({ labels: { role: "TPM", "title.auto": "TPM • ASMA-1 • Topic", persona: "kontor-tpm-bound@0" } }).adapter).toBe("direct");
    expect(takeoverAdapter({ labels: { "create.request": REQUEST } })).toEqual({ adapter: "direct", basis: "created", owner: null, createRequest: REQUEST });
    expect(takeoverAdapter({ labels: { orchestration: "paseo-direct", "paseo.parent-agent-id": "owner-1" } }))
      .toEqual({ adapter: "direct", basis: "orchestrated", owner: "owner-1", createRequest: null });
    const native = { labels: { orchestration: "paseo-direct" }, parentAgentId: "owner-2" };
    expect(takeoverAdapter(native)).toMatchObject({ basis: "orchestrated", owner: "owner-2" });
    // A takeover successor of an orchestrated seat keeps its owner and names its own creation request.
    expect(takeoverAdapter({ labels: { orchestration: "paseo-direct", "paseo.parent-agent-id": "owner-1", "create.request": REQUEST } }))
      .toEqual({ adapter: "direct", basis: "orchestrated", owner: "owner-1", createRequest: REQUEST });
    expect(takeoverAdapter({ labels: { "kontor.seat_binding_id": "seat-1", "kontor.worktree": "/w" } })).toEqual({ adapter: "governed", seat: "seat-1" });
  });

  it("refuses conflicting and unverifiable ownership, and never relabels a governed claim direct", () => {
    for (const labels of [
      { "kontor.worktree": "/w" } as Record<string, string>,
      { "kontor.seat_binding_id": "seat-1", orchestration: "paseo-direct", "paseo.parent-agent-id": "owner-1" },
      { "kontor.agent-run": "run-1", "create.request": REQUEST },
      { orchestration: "paseo-direct" },
      { orchestration: "kontor" },
      { orchestration: "paseo-direct", "paseo.parent-agent-id": "owner-1", "create.request": "not-a-request" },
      { "create.request": "not-a-request" },
    ]) expect(takeoverAdapter({ labels }).adapter, JSON.stringify(labels)).toBe("refused");
  });

  it("lets only the direct adapter reach the resolver", async () => {
    for (const [owner, message] of [
      [takeoverAdapter({ labels: { "kontor.seat_binding_id": "seat-1" } }), GOVERNED_ELSEWHERE],
      [takeoverAdapter({ labels: { "kontor.worktree": "/w" } }), "not treated as a direct session"],
    ] as const) {
      const runner = fake(answered(selection([null, null], true)));
      const result = await resolveTakeoverRoute(config, owner, { agentId: "agent-1", bindingKey: KEY }, health, runner.run);
      expect(result.state).toBe("refused");
      expect(result.message).toContain(message);
      expect(runner.calls).toHaveLength(0);
    }
  });
});

describe("the fixed local fleet selection read", () => {
  it("takes the session and its configured binding, never an executable, path, tier, argument, environment, policy or evidence", async () => {
    for (const field of ["executable", "cliCommand", "command", "stateRoot", "tier", "argv", "args", "env", "policy", "routeHealth"]) {
      const input = { agentId: "agent-1", bindingKey: KEY, [field]: "x" };
      expect(takeoverRouteInputSchema.safeParse(input).success, field).toBe(false);
      const { result, calls } = await resolve(answered(selection([null, null], true)), input);
      expect(result.state, field).toBe("refused");
      expect(calls, field).toHaveLength(0);
    }
    for (const bindingKey of ["--tier=admin", "team/../x", "team/a", "team/a/b/c", "advisor/a b", "leadership/abc/lsa", "fleet/a", ""]) {
      const { result, calls } = await resolve(answered(selection([null, null], true)), { agentId: "agent-1", bindingKey });
      expect(result.state, bindingKey).toBe("refused");
      expect(calls, bindingKey).toHaveLength(0);
    }
    const { result, calls } = await resolve(answered(selection([null, null], true)), { agentId: "agent-1", bindingKey: null });
    expect(result).toMatchObject({ state: "refused", selected: null, digest: null });
    expect(result.message).toContain("never used to infer");
    expect(calls).toHaveLength(0);
  });

  it("refuses malformed server evidence before anything runs", async () => {
    for (const evidence of [
      { ...health, max_age_seconds: 0 },
      { ...health, max_age_seconds: 3601 },
      { ...health, as_of: "2026-10-25T09:00:00.000Z" },
      { ...health, observations: [{ ...health.observations[0]!, state: "ready" }] },
      { ...health, policy: {} },
    ]) {
      const { result, calls } = await resolve(answered(selection([null, null], true)), undefined, evidence as RouteHealth);
      expect(result.state, JSON.stringify(evidence)).toBe("refused");
      expect(calls).toHaveLength(0);
    }
    expect(resolverInstant(Date.parse("2026-10-25T09:00:00.999Z"))).toBe("2026-10-25T09:00:00Z");
  });

  it("runs the configured command against the server's state root with one fixed argument vector", async () => {
    const body = selection(["route_health_unknown", null], true);
    const { result, calls } = await resolve(answered(body));
    expect(calls).toEqual([{ command: "/opt/kontor", args: [
      "--state-root", "/srv/kontor-state", "--tier", "operator", "fleet-policy-resolve", `--binding-key=${KEY}`, `--route-health=${JSON.stringify(health)}`,
    ] }]);
    expect(result).toEqual({
      state: "selected", message: expect.any(String),
      provenance: { policyHash: "a".repeat(64), bindingKey: KEY, chain: "codex-first" },
      selected: { account: "claude-personal", model: "claude-opus-5", effort: "xhigh", vendor: "anthropic", step: 2, subStep: 1 },
      considered: [
        { account: "codex-work", model: "gpt-5.6-sol", effort: "xhigh", vendor: "openai", step: 1, subStep: 1, excluded: "route_health_unknown" },
        { account: "claude-personal", model: "claude-opus-5", effort: "xhigh", vendor: "anthropic", step: 2, subStep: 1, excluded: null },
      ],
      digest: createHash("sha256").update(JSON.stringify(body)).digest("hex"),
    });
  });

  it("reports unknown and stale evidence as a block, never a route", async () => {
    const body = selection(["route_health_stale", "route_unobserved"], false);
    const { result } = await resolve(blocked(body));
    expect(result.state).toBe("blocked");
    expect(result.selected).toBeNull();
    expect(result.considered.map((entry) => entry.excluded)).toEqual(["route_health_stale", "route_unobserved"]);
    expect(result.digest).toBe(createHash("sha256").update(JSON.stringify(body)).digest("hex"));
  });

  it("answers what it cannot verify as unknown or refused, never as a route", async () => {
    const refusal = { action: "activate a bundle", code: "not_found", dispatched: false, rule: "no fleet activation record exists", tool: "kontor_fleet_policy_resolve" };
    const cases: [unknown, string][] = [
      ["not json", "unknown"],
      [{ tool: "kontor_realm_get", status: 200, body: selection([null, null], true) }, "unknown"],
      [answered(selection([null, null], true, { provenance: { binding_key: "advisor/other", chain: "c", policy_hash: "a".repeat(64), schema_version: 2 } })), "unknown"],
      [answered(selection([null, null], true, { route_health: { ...health, as_of: "2026-10-25T09:00:01Z" } })), "unknown"],
      [answered(selection([null, null], true, { route_health: undefined })), "unknown"],
      [answered(selection([null, null], false)), "unknown"],
      [refusal, "refused"],
    ];
    for (const [document, state] of cases) {
      const { result } = await resolve(document);
      expect(result, JSON.stringify(document)).toMatchObject({ state, selected: null, digest: null });
    }
    const { result } = await resolve(refusal);
    expect(result.message).toContain("no fleet activation record exists");
    const failed = await resolveTakeoverRoute(config, ordinary, { agentId: "agent-1", bindingKey: KEY }, health, () => Promise.reject(new Error("spawn kontor ENOENT")));
    expect(failed).toMatchObject({ state: "unknown", selected: null });
  });
});
