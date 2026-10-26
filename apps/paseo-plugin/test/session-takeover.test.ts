import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  GOVERNED_ELSEWHERE, configuredAccountCatalogFromFleet, joinSessionLocalRefusalToRouteHealth, resolveTakeoverRoute, resolverInstant,
  routeEvidenceDigest, routeEvidenceFreshEnough, routeEvidenceSemanticPayload, routeHealthFromProviderSnapshot, sessionLocalRefusalForTakeover,
  takeoverAdapter, takeoverPinnedMatches, takeoverRouteInputSchema, type ResolverRun, type RouteHealth,
} from "../server/session-takeover";

const KEY = "team/01936f5a-0000-7000-8000-000000000102/implement";
const REQUEST = "0f1e2d3c-4b5a-4968-8776-655443322110";
const config = { cliCommand: "/opt/kontor", stateRoot: "/srv/kontor-state" };
const ordinary = takeoverAdapter({ labels: {} });
const health: RouteHealth = {
  as_of: "2026-10-25T09:00:00Z", max_age_seconds: 60, mode: "default",
  observations: [
    { account: "codex-b", model: "gpt-5.6-sol", effort: "xhigh", mode: "default", state: "unknown", observed_at: "2026-10-25T08:59:59Z", revision: "r1" },
    { account: "claude-a", model: "claude-opus-5", effort: "xhigh", mode: "default", state: "available", observed_at: "2026-10-25T08:59:30Z", revision: "r2" },
  ],
};

const route = (provider: string, model: string, step: number) =>
  ({ rung: { provider, model, effort: "xhigh" }, step, sub_step: 1, vendor: provider.startsWith("codex") ? "openai" : "anthropic" });
/** A FleetSelection as the CLI prints it: keys sorted, the stated evidence echoed. */
function selection(excluded: [string | null, string | null], selected: boolean, overrides: Record<string, unknown> = {}) {
  const considered = [route("codex-b", "gpt-5.6-sol", 1), route("claude-a", "claude-opus-5", 2)]
    .map((entry, index) => ({ ...(excluded[index] ? { excluded: excluded[index] } : {}), route: entry }));
  return { considered, eligibility: { excluded_vendors: [], unavailable_accounts: [] }, excluded_by_policy: [],
    provenance: { binding_key: KEY, chain: "codex-first", policy_hash: "a".repeat(64), schema_version: 2 },
    selected_validity: selected ? { observed_at: "2026-10-25T08:59:30Z", valid_until: "2026-10-25T09:00:30Z" } : undefined,
    route_health: health, selected: selected ? route("claude-a", "claude-opus-5", 2) : null, ...overrides };
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
      selected: { account: "claude-a", model: "claude-opus-5", effort: "xhigh", vendor: "anthropic", step: 2, subStep: 1 },
      validity: { observedAt: "2026-10-25T08:59:30Z", validUntil: "2026-10-25T09:00:30Z" },
      considered: [
        { account: "codex-b", model: "gpt-5.6-sol", effort: "xhigh", vendor: "openai", step: 1, subStep: 1, excluded: "route_health_unknown" },
        { account: "claude-a", model: "claude-opus-5", effort: "xhigh", vendor: "anthropic", step: 2, subStep: 1, excluded: null },
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
      ...[undefined, { observed_at: "2026-10-25T09:01:00Z", valid_until: "2026-10-25T09:00:30Z" }, { observed_at: "bad", valid_until: "bad" }].map((selected_validity): [unknown, string] => [answered(selection([null, null], true, { selected_validity })), "unknown"]),
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

describe("route health from provider snapshot", () => {
  const catalog = configuredAccountCatalogFromFleet([{ accounts: ["claude-a", "codex-b", "codex-a"] }], "fleet-rev-1")!;
  const asOf = Date.parse("2026-10-25T09:00:00.000Z");
  const snapshot = { requestId: "snap-1", generatedAt: "2026-10-25T08:59:00.000Z", entries: [
    { provider: "claude-a", enabled: true, status: "ready", modes: [{ id: "default" }],
      models: [{ id: "claude-opus-5", thinkingOptions: [{ id: "xhigh" }, { id: "not-an-effort" }] }] },
    { provider: "codex-b", enabled: true, status: "loading", models: [{ id: "gpt-5.6-sol", thinkingOptions: [{ id: "xhigh" }] }] },
    { provider: "codex-a", enabled: false, status: "ready", models: [{ id: "gpt-5.6-sol", thinkingOptions: [{ id: "xhigh" }] }] },
  ] };

  it("maps ready accounts to available routes and others to unknown or unavailable", () => {
    const built = routeHealthFromProviderSnapshot(snapshot, "default", catalog, asOf);
    expect(built.max_age_seconds).toBe(300);
    expect(built.mode).toBe("default");
    expect(built.observations).toEqual([
      { account: "claude-a", model: "claude-opus-5", effort: "xhigh", mode: "default", state: "available", observed_at: "2026-10-25T08:59:00Z", revision: "snap-1@2026-10-25T08:59:00.000Z" },
      { account: "codex-b", model: "gpt-5.6-sol", effort: "xhigh", mode: "default", state: "unknown", observed_at: "2026-10-25T08:59:00Z", revision: "snap-1@2026-10-25T08:59:00.000Z" },
      { account: "codex-a", model: "gpt-5.6-sol", effort: "xhigh", mode: "default", state: "unavailable", observed_at: "2026-10-25T08:59:00Z", revision: "snap-1@2026-10-25T08:59:00.000Z" },
    ]);
  });

  it("treats a ready provider without the session mode as unknown for that route", () => {
    const built = routeHealthFromProviderSnapshot(snapshot, "bypassPermissions", catalog, asOf);
    expect(built.observations.find((entry) => entry.account === "claude-a")?.state).toBe("unknown");
  });

  it("never treats an unmapped harness provider id as a configured account", () => {
    const adversarial = routeHealthFromProviderSnapshot({ ...snapshot, entries: [{ provider: "claude", enabled: true, status: "ready", modes: [{ id: "default" }], models: [{ id: "claude-opus-5" }] }] }, "default", catalog, asOf);
    expect(adversarial.observations[0]?.state).toBe("unknown");
  });

  it.each([
    ["invalid generatedAt", { requestId: "snap-2", generatedAt: "not-a-date" }],
    ["missing requestId", { requestId: "  ", generatedAt: "2026-10-25T08:59:00.000Z" }],
    ["future generatedAt", { requestId: "snap-3", generatedAt: "2026-10-25T10:00:00.000Z" }],
  ] as const)("fail-closes %s without substituting review time as freshness", (_label, timing) => {
    const closed = routeHealthFromProviderSnapshot({ ...snapshot, ...timing }, "default", catalog, asOf);
    expect(closed.observations.every((entry) => entry.state === "unknown")).toBe(true);
    expect(closed.observations[0]?.revision).toBe("unqualified-snapshot");
  });

  it("fail-closes ready routes when modes evidence is missing for a requested session mode", () => {
    const noModes = routeHealthFromProviderSnapshot({ ...snapshot, entries: [{ provider: "claude-a", enabled: true, status: "ready", models: [{ id: "claude-opus-5", thinkingOptions: [{ id: "xhigh" }] }] }] }, "bypassPermissions", catalog, asOf);
    expect(noModes.observations.every((entry) => entry.state === "unknown")).toBe(true);
    const emptyModes = routeHealthFromProviderSnapshot({ ...snapshot, entries: [{ provider: "claude-a", enabled: true, status: "ready", modes: [], models: [{ id: "claude-opus-5" }] }] }, "bypassPermissions", catalog, asOf);
    expect(emptyModes.observations.every((entry) => entry.state === "unknown")).toBe(true);
  });
});

describe("route evidence semantic identity", () => {
  const catalog = configuredAccountCatalogFromFleet([{ accounts: ["claude-a"] }], "fleet-rev-1")!;
  const asOf = Date.parse("2026-10-25T09:00:00.000Z");
  const snap = (requestId: string) => routeHealthFromProviderSnapshot({ requestId, generatedAt: "2026-10-25T08:59:00.000Z", entries: [
    { provider: "claude-a", enabled: true, status: "ready", modes: [{ id: "default" }], models: [{ id: "claude-opus-5", thinkingOptions: [{ id: "xhigh" }] }] },
  ] }, "default", catalog, asOf);

  it("ignores transport requestId when digesting evidence", () => {
    const a = snap("req-a"), b = snap("req-b");
    expect(routeEvidenceDigest(a, null, catalog.revision)).toBe(routeEvidenceDigest(b, null, catalog.revision));
    expect(routeEvidenceSemanticPayload(a, null, catalog.revision)).toEqual(routeEvidenceSemanticPayload(b, null, catalog.revision));
  });

  it("ignores refusal observation clock when digesting canonical refusal identity", () => {
    const health = snap("req-a");
    const refusalA = { kind: "quota" as const, failureAt: "unknown", observedAt: "2026-10-25T09:00:00.000Z", message: "429", route: { provider: "claude-a", model: "claude-opus-5" } };
    const refusalB = { ...refusalA, observedAt: "2026-10-25T09:00:01.000Z" };
    expect(routeEvidenceDigest(health, refusalA, catalog.revision)).toBe(routeEvidenceDigest(health, refusalB, catalog.revision));
  });

  it("changes digest on semantic snapshot or refusal identity drift", () => {
    const base = snap("req-a");
    const stale = routeHealthFromProviderSnapshot({ requestId: "req-a", generatedAt: "2026-10-25T08:59:00.000Z", entries: [
      { provider: "claude-a", enabled: true, status: "error", modes: [{ id: "default" }], models: [{ id: "claude-opus-5" }] },
    ] }, "default", catalog, asOf);
    expect(routeEvidenceDigest(stale, null, catalog.revision)).not.toBe(routeEvidenceDigest(base, null, catalog.revision));
    const refusal = { kind: "quota" as const, failureAt: "unknown", observedAt: "2026-10-25T09:00:00.000Z", message: "429", route: { provider: "codex-b", model: "gpt-5.6-sol" } };
    expect(routeEvidenceDigest(base, refusal, catalog.revision)).not.toBe(routeEvidenceDigest(base, null, catalog.revision));
  });

  it("rejects future or over-max-age observations for freshness revalidation", () => {
    const fresh = snap("req-a");
    expect(routeEvidenceFreshEnough(fresh, asOf)).toBe(true);
    const future = { ...fresh, observations: fresh.observations.map((entry) => ({ ...entry, observed_at: resolverInstant(asOf + 60_000) })) };
    expect(routeEvidenceFreshEnough(future, asOf)).toBe(false);
  });
});

describe("session-local refusal join", () => {
  const catalog = configuredAccountCatalogFromFleet([{ accounts: ["claude-a", "codex-b"] }], "fleet-rev-1")!;
  const base = routeHealthFromProviderSnapshot({ requestId: "s", generatedAt: "2026-10-25T09:00:00.000Z", entries: [
    { provider: "claude-a", enabled: true, status: "ready", modes: [{ id: "default" }], models: [{ id: "claude-opus-5", thinkingOptions: [{ id: "xhigh" }] }] },
    { provider: "codex-b", enabled: true, status: "ready", modes: [{ id: "default" }], models: [{ id: "gpt-5.6-sol", thinkingOptions: [{ id: "xhigh" }] }] },
  ] }, "default", catalog, Date.parse("2026-10-25T09:00:00.000Z"));

  it("marks matching account observations unavailable on explicit quota evidence without touching other accounts", () => {
    const joined = joinSessionLocalRefusalToRouteHealth(base, {
      kind: "quota", failureAt: "unknown", observedAt: "2026-10-25T09:00:00.000Z", message: "429 quota",
      route: { provider: "claude-a", model: "claude-opus-5" },
    });
    expect(joined.observations.find((entry) => entry.account === "claude-a")?.state).toBe("unavailable");
    expect(joined.observations.find((entry) => entry.account === "codex-b")?.state).toBe("available");
  });

  it("leaves health unchanged when the predecessor carries no explicit refusal", () => {
    expect(joinSessionLocalRefusalToRouteHealth(base, null)).toBe(base);
  });

  it("does not let generic ready clear a correlated quota refusal on another account", () => {
    const workRefusal = sessionLocalRefusalForTakeover({ provider: "claude-a", model: "claude-opus-5", status: "error", lastError: "429 quota" }, "default", "2026-10-25T09:00:00.000Z");
    const joined = joinSessionLocalRefusalToRouteHealth(base, workRefusal, "default");
    expect(joined.observations.find((entry) => entry.account === "claude-a")?.state).toBe("unavailable");
    expect(joined.observations.find((entry) => entry.account === "codex-b")?.state).toBe("available");
  });

  it("does not join refusal when no route observation matches account and model", () => {
    const refusal = sessionLocalRefusalForTakeover({ provider: "claude-a", model: "claude-opus-5", status: "error", lastError: "429 quota" }, "default", "2026-10-25T09:00:00.000Z");
    const mismatchedModel = joinSessionLocalRefusalToRouteHealth(base, refusal, "default");
    const noObservation = routeHealthFromProviderSnapshot({ requestId: "s", generatedAt: "2026-10-25T09:00:00.000Z", entries: [
      { provider: "claude-a", enabled: true, status: "ready", modes: [{ id: "default" }], models: [{ id: "other-model", thinkingOptions: [{ id: "xhigh" }] }] },
    ] }, "default", catalog, Date.parse("2026-10-25T09:00:00.000Z"));
    expect(joinSessionLocalRefusalToRouteHealth(noObservation, refusal, "default")).toBe(noObservation);
    expect(mismatchedModel.observations.find((entry) => entry.account === "claude-a")?.state).toBe("unavailable");
  });
});

describe("takeover pinned equivalence", () => {
  const pin = (evidence: string, selectedAccount = "claude-a") => ({
    predecessor: "pred", owner: takeoverAdapter({ labels: {} }), ownerRevision: null, created: null, bindingKey: KEY,
    route: { provenance: { policyHash: "a".repeat(64), bindingKey: KEY, chain: "c" }, selected: { account: selectedAccount, model: "m", effort: "xhigh", vendor: "v", step: 1, subStep: 1 }, considered: [], validity: { observedAt: "2026-10-25T09:00:00Z", validUntil: "2026-10-25T09:05:00Z" } },
    evidence, continuity: { agentId: "a", requestId: REQUEST, revision: "r", observedAt: "2026-10-25T09:00:00.000Z", surface: "direct" as const, wakes: "clear" as const, owner: null },
    spec: { requestId: REQUEST, title: "t", provider: selectedAccount, model: "m", effort: "xhigh", modeId: "default", labels: {}, workspaceId: "w", anchorWorkspaceId: "w", workspaceName: "w", cwd: "/c",
      expectedPlacement: { workspaceId: "w", cwd: "/c" }, createWorkspace: null, revalidate: null, persona: { text: "", supported: false }, bootstrap: "b",
      effectValidity: { route: { observedAt: "2026-10-25T09:00:00Z", validUntil: "2026-10-25T09:00:30Z" }, continuity: { observedAt: "2026-10-25T09:00:00.000Z", validUntil: "2026-10-25T09:01:00.000Z" } } },
  });

  it("accepts refreshed effect deadlines when semantic evidence and route selection are unchanged", () => {
    const reviewed = pin("evidence-1");
    const current = pin("evidence-1");
    current.route.validity = { observedAt: "2026-10-25T09:00:01Z", validUntil: "2026-10-25T09:05:01Z" };
    current.spec.effectValidity = { route: { observedAt: "2026-10-25T09:00:01Z", validUntil: "2026-10-25T09:05:01Z" }, continuity: current.spec.effectValidity.continuity };
    expect(takeoverPinnedMatches(reviewed, current)).toBe(true);
  });

  it("refuses semantic route or evidence drift", () => {
    const reviewed = pin("evidence-1");
    expect(takeoverPinnedMatches(reviewed, pin("evidence-2"))).toBe(false);
    expect(takeoverPinnedMatches(reviewed, pin("evidence-1", "codex-b"))).toBe(false);
  });

  it("accepts a fresh continuity observation when semantic identity is unchanged", () => {
    const reviewed = pin("evidence-1");
    const current = pin("evidence-1");
    current.continuity = { ...current.continuity, observedAt: "2026-10-25T09:00:01.100Z" };
    expect(takeoverPinnedMatches(reviewed, current)).toBe(true);
  });

  it("refuses continuity semantic drift including wakes", () => {
    const reviewed = pin("evidence-1");
    const current = pin("evidence-1");
    expect(takeoverPinnedMatches(reviewed, {
      ...current,
      continuity: { agentId: "a", requestId: REQUEST, revision: "r2", observedAt: "2026-10-25T09:00:00.000Z", surface: "direct", wakes: "preserved", owner: null },
    })).toBe(false);
  });
});
