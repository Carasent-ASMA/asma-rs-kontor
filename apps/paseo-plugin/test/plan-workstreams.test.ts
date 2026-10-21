import { describe, expect, it } from "vitest";
import type { ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
import { planWorkstreamMappings, planWorkstreamOptions } from "../client/plan-workstreams";
import { organizationScope, buildOrganizationTree, visibleSessions } from "../client/session-organization";
import { sessionEntry } from "./session-fixtures";

import { source, read, registry } from "./plan-workstream-fixtures";

describe("declared plan workstreams", () => {
  it("links exact epics without an ESW, keeps native placement, and counts each session once", () => {
    const entries = [sessionEntry("linked", { labels: { epic: "ASMA-7575" }, workspaceName: "SWW • existing" }), sessionEntry("unknown")];
    const mappings = planWorkstreamMappings([source], { plan: read }, entries, [], {}, {});
    expect(organizationScope(entries[0]!, mappings).workstream).toBe("adopus-graphql");
    expect(organizationScope(entries[1]!, mappings).workstream).toBe("Other work");
    const tree = buildOrganizationTree({ entries, displayMappings: mappings });
    expect(visibleSessions(tree).map((entry) => entry.id).sort()).toEqual(["linked", "unknown"]);
    expect(planWorkstreamOptions([source], { plan: read }, registry, entries, mappings).find((option) => option.key === "adopus-graphql")).toMatchObject({ registered: true, plans: 1, sessions: 1, workspaces: 1 });
  });
  it("preserves session mappings, workspace mappings, labels and native desk precedence", () => {
    const entries = [sessionEntry("mapped", { labels: { epic: "ASMA-7575", workstream: "label" } }), sessionEntry("workspace", { labels: { epic: "ASMA-7575" }, workspaceId: "other" }), sessionEntry("desk", { projectName: "DESK • PR REVIEW", labels: { epic: "ASMA-7575" } })];
    const saved = { "session:mapped": { workstream: "custom" }, "workspace:other": { workstream: "workspace" } };
    const mappings = planWorkstreamMappings([source], { plan: read }, entries, [], saved, {});
    expect(entries.map((entry) => organizationScope(entry, mappings).workstream)).toEqual(["custom", "workspace", "pr-review"]);
    expect(saved).toEqual({ "session:mapped": { workstream: "custom" }, "workspace:other": { workstream: "workspace" } });
  });
  it("does not attach conflicting, unavailable or missing plan declarations", () => {
    const entry = sessionEntry("linked", { labels: { epic: "ASMA-7575" } });
    const other = { ...source, id: "other", path: "/docs/other.md" };
    for (const second of [{ ...read, source: other, planMetadata: { ...read.planMetadata!, workstream: "tokenomics" } }, { ...read, source: other, planMetadata: { ...read.planMetadata!, workstream: null } }, { ...read, source: other, status: "unavailable" as const }]) {
      expect(planWorkstreamMappings([source, other], { plan: read, other: second }, [entry], [], {}, {})).toEqual({});
    }
    expect(planWorkstreamMappings([source, other], { plan: read, other: { ...read, source: other, planMetadata: { ...read.planMetadata!, workstream: "tokenomics" } } }, [entry], [], {}, { linked: ["plan"] })).toEqual({ "session:linked": { workstream: "adopus-graphql" } });
  });
  it("keeps registry-only, unregistered and zero-session options honest, with copy/revision deduplication", () => {
    const copy = { ...source, id: "copy", path: "/copy/plan.md", logicalPlanId: "same" };
    const canonical = { ...source, logicalPlanId: "same" };
    const options = planWorkstreamOptions([canonical, copy], { plan: read, copy: { ...read, source: copy } }, registry, [], {});
    expect(options).toHaveLength(2);
    expect(options.find((value) => value.key === "adopus-graphql")).toMatchObject({ plans: 1, sessions: 0 });
    expect(options.find((value) => value.key === "access")).toMatchObject({ registered: true, plans: 0, sessions: 0 });
    const changed = planWorkstreamOptions([source], { plan: { ...read, planMetadata: { ...read.planMetadata!, workstream: "new-key" } } }, registry, [], {});
    expect(changed.find((value) => value.key === "new-key")).toMatchObject({ registered: false, plans: 1, sessions: 0 });
    expect(changed.find((value) => value.key === "adopus-graphql")?.plans).toBe(0);
  });
});

it("keeps an explicitly linked empty workspace discoverable without inventing a session", () => {
  const workspaces = [{ id: "empty", name: "Ordinary workspace", projectKey: "project" }];
  const mappings = planWorkstreamMappings([source], { plan: read }, [], workspaces, { "workspace:empty": { epic: "ASMA-7575" } }, {});
  expect(planWorkstreamOptions([source], { plan: read }, registry, [], mappings, workspaces).find((option) => option.key === "adopus-graphql")).toMatchObject({ sessions: 0, workspaces: 1, plans: 1 });
});

it("does not use missing, conflicting or foreign-project plan associations as placement evidence", () => {
  const entry = sessionEntry("linked", { projectKey: "project", labels: { epic: "ASMA-2" } });
  expect(planWorkstreamMappings([source], { plan: read }, [entry], [], {}, { linked: ["plan"] })).toEqual({});
  const matching = { ...entry, labels: { epic: "ASMA-7575" } };
  expect(planWorkstreamMappings([source], { plan: read }, [matching], [], {}, { linked: ["plan", "missing"] })).toEqual({});
  expect(planWorkstreamMappings([{ ...source, projectId: "other" }], { plan: read }, [matching], [], {}, {})).toEqual({});
});
