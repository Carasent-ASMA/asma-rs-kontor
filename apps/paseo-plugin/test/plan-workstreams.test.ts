import { describe, expect, it } from "vitest";
import type { JiraHistory, ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
import { jiraCapture } from "../client/page-data";
import { planFallbackNote, planWorkstreamMappings, planWorkstreamOptions, workstreamOptionLabel } from "../client/plan-workstreams";
import { organizationScope, buildOrganizationTree, declaredScope, filterOrganizationTree, matchesScope, organizationWorkspaceCount, sessionEpic, visibleSessions } from "../client/session-organization";
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

// The census shapes with neutral identities: a desk session filed under a non-desk workstream, a
// workstream only a desk session carries, an unlabelled session in an epic's project, and a plan nothing links.
describe("workstream choices and the tree describe the same sessions", () => {
  const desk = { projectKey: "prj_desk", projectName: "DESK • ADAM" };
  const entries = [
    sessionEntry("desk-own", { ...desk, workspaceId: "wks_desk" }),
    sessionEntry("desk-filed", { ...desk, workspaceId: "wks_desk", labels: { workstream: "alpha", epic: "ASMA-1" } }),
    sessionEntry("desk-slug", { ...desk, workspaceId: "wks_task", labels: { workstream: "task-slug", epic: "ASMA-3" } }),
    sessionEntry("esw-filed", { projectKey: "prj_esw", projectName: "ESW • ASMA-1", workspaceId: "wks_esw", labels: { workstream: "alpha", epic: "ASMA-1" } }),
    sessionEntry("esw-bare", { projectKey: "prj_plan", projectName: "ESW • ASMA-2", workspaceId: "wks_plan" }),
  ];
  const plan = { ...source, id: "beta", path: "/docs/beta.md", epic: "ASMA-2" };
  const planRead: SourceRead = { ...read, source: plan, planMetadata: { workstream: "beta", epic: "ASMA-2", diagnostics: [] } };
  const mappings = planWorkstreamMappings([plan], { beta: planRead }, entries, [], {}, {});
  const options = planWorkstreamOptions([plan], { beta: planRead }, registry, entries, mappings);
  const option = (key: string) => options.find((value) => value.key === key)!;

  it("names where the tree shows desk-held sessions, never moving or double-counting them", () => {
    // No project or session name links the bare session to the ASMA-2 plan.
    expect(mappings).toEqual({});
    expect(option("alpha")).toMatchObject({ sessions: 2, workspaces: 2, shownUnder: { "DESK • ADAM": 1 }, othersInDesk: 0 });
    expect(option("task-slug")).toMatchObject({ sessions: 1, shownUnder: { "DESK • ADAM": 1 } });
    expect(option("adam")).toMatchObject({ sessions: 1, desk: true, shownUnder: {}, othersInDesk: 2 });
    expect(option("beta")).toMatchObject({ sessions: 0, workspaces: 0, plans: 1 });
    expect(option("other work")).toMatchObject({ sessions: 1, workspaces: 1 });
    expect(options.reduce((sum, value) => sum + value.sessions, 0)).toBe(entries.length);
    const tree = buildOrganizationTree({ entries, displayMappings: mappings });
    expect(Object.fromEntries(tree.map((group) => [group.label, group.totalCount]))).toEqual({ "DESK • ADAM": 3, alpha: 1, "Other work": 1 });
    // Tree count = choice count less its desk-held sessions; the desk row = its own choice plus what it holds for others.
    expect(option("alpha").sessions - option("alpha").shownUnder["DESK • ADAM"]!).toBe(1);
    expect(option("adam").sessions + option("adam").othersInDesk).toBe(3);
    // Choosing one choice shows exactly the sessions it counts, wherever the tree keeps them.
    for (const value of options) {
      const shown = visibleSessions(filterOrganizationTree(tree, (entry) => organizationScope(entry, mappings).workstream.trim().toLowerCase() === value.key));
      expect([value.key, shown.length]).toEqual([value.key, value.sessions]);
    }
    // The outside-desks shortcut keeps every non-desk session; the Other work choice only the unlinked one.
    expect(entries.filter((entry) => matchesScope(entry, "Other work", mappings)).map((entry) => entry.id)).toEqual(["esw-filed", "esw-bare"]);
  });

  it("labels kind, counts and desk placement, and states registration only when the registry is loaded", () => {
    expect(workstreamOptionLabel(option("alpha"), false)).toBe("alpha · 2 sessions (1 shown under DESK • ADAM) · 2 workspaces · 0 plans");
    expect(workstreamOptionLabel(option("adam"), true)).toBe("adam · desk category · 1 session · 1 workspace · 0 plans · desk shows 2 more sessions filed under other workstreams");
    expect(workstreamOptionLabel(option("beta"), false)).toBe("beta · plan only · 0 sessions · 0 workspaces · 1 plan");
    expect(workstreamOptionLabel(option("other work"), true)).toBe("Other work · no workstream link · 1 session · 1 workspace · 0 plans");
    expect(workstreamOptionLabel(option("task-slug"), true)).toBe("task-slug · 1 session (1 shown under DESK • ADAM) · 1 workspace · 0 plans · unregistered");
    expect(workstreamOptionLabel(option("adopus-graphql"), true)).toBe("Ad Opus GraphQL · 0 sessions · 0 workspaces · 0 plans · registered");
    expect(options.map((value) => workstreamOptionLabel(value, false)).join("\n")).not.toMatch(/registered|registration/);
  });

  it("reconciles a desk choice's workspaces with the tree: stale IDs counted, empty native workspaces named", () => {
    const review = { projectKey: "prj_review", projectName: "DESK • PR REVIEW" };
    const deskEntries = [sessionEntry("on-native", { ...review, workspaceId: "wks_a" }), sessionEntry("on-stale", { ...review, workspaceId: "wks_gone" })];
    const nativeWorkspaces = ["wks_a", "wks_empty1", "wks_empty2"].map((id) => ({ id, name: id, ...review }));
    const value = planWorkstreamOptions([], {}, undefined, deskEntries, {}, nativeWorkspaces).find((candidate) => candidate.key === "pr-review")!;
    expect(value).toMatchObject({ desk: true, sessions: 2, workspaces: 2, emptyInDesk: 2 });
    expect(organizationWorkspaceCount(buildOrganizationTree({ entries: deskEntries, displayMappings: {}, workspaces: nativeWorkspaces }))).toBe(value.workspaces + value.emptyInDesk);
    expect(workstreamOptionLabel(value, false)).toBe("pr-review · desk category · 2 sessions · 2 workspaces, 2 more empty in the desk · 0 plans");
  });

  it("says why no plan links an unlinked session, from the plans the fallback weighs", () => {
    const rival = { ...plan, id: "rival", path: "/docs/rival.md" };
    const rivalRead: SourceRead = { ...planRead, source: rival, planMetadata: { workstream: "gamma", epic: "ASMA-2", diagnostics: [] } };
    expect(planFallbackNote([plan], { beta: planRead }, null, "prj_plan")).toBe("No known ASMA epic, so no plan applies.");
    expect(planFallbackNote([plan], { beta: planRead }, "ARCH-0001", "prj_plan")).toBe("No known ASMA epic, so no plan applies.");
    expect(planFallbackNote([plan], { beta: planRead }, "asma-9", "prj_plan")).toBe("No configured plan declares ASMA-9.");
    expect(planFallbackNote([{ ...plan, projectId: "prj_other" }], { beta: planRead }, "ASMA-2", "prj_plan")).toBe("Plans for ASMA-2 are bound to other projects.");
    expect(planFallbackNote([plan, rival], { beta: planRead, rival: rivalRead }, "ASMA-2", "prj_plan")).toBe("Of 2 plans for ASMA-2, 1 declares beta, 1 declares gamma, so none applies.");
    expect(planFallbackNote([plan, rival], { beta: planRead, rival: { ...rivalRead, status: "unavailable" } }, "ASMA-2", "prj_plan")).toBe("Of 2 plans for ASMA-2, 1 declares beta, 1 is not ready, so none applies.");
    // The live ESW • ASMA-7675 shape: its one ready plan declares no workstream.
    expect(planFallbackNote([plan], { beta: { ...planRead, planMetadata: { workstream: null, epic: null, diagnostics: [] } } }, "ASMA-2", "prj_plan")).toBe("Of 1 plan for ASMA-2, 1 declares no workstream, so none applies.");
    expect(planFallbackNote([plan], { beta: planRead }, "ASMA-2", "prj_plan", ["beta", "missing"])).toBe("Its explicit plan links do not resolve to ready plans that declare one workstream.");
    expect(planFallbackNote([], {}, "ASMA-2", "prj_plan")).toBeNull();
    // The ambiguous case the note describes really leaves the session unlinked.
    expect(planWorkstreamMappings([plan, rival], { beta: planRead, rival: rivalRead }, [sessionEntry("x", { labels: { epic: "ASMA-2" } })], [], {}, {})).toEqual({});
  });

  it("counts, files and explains each epic state alike: a captured task parent links, unknown or other-project epics do not", () => {
    const generated = "2026-10-20T16:03:00+00:00";
    const history: JiraHistory = { schema: "consumption/data@2", generated, provenance: { path: "/configured/capture.json", sha256: "b".repeat(64), capturedEpics: 1 }, selection: ["ASMA-2"],
      epics: [{ key: "ASMA-2", status: "In Progress", cat: "indeterminate", children: [{ key: "ASMA-21", status: "In Progress", cat: "indeterminate" }] }] };
    const jira = jiraCapture([{ ...read, source: { ...source, id: "jira", kind: "jira" }, jiraHistory: history }]);
    const bound = { ...plan, projectId: "prj_plan" }, reads = { beta: { ...planRead, source: bound } };
    const esw = { projectKey: "prj_plan", projectName: "ESW • ASMA-2" };
    const states = [
      { ...sessionEntry("task-captured", { ...esw, workspaceId: "w1", labels: { task: "ASMA-21" } }), jira },
      { ...sessionEntry("task-uncaptured", { ...esw, workspaceId: "w2", labels: { task: "ASMA-99" } }), jira },
      { ...sessionEntry("contradictory", { ...esw, workspaceId: "w3", labels: { epic: "ASMA-3", task: "ASMA-21" } }), jira },
      sessionEntry("no-capture", { ...esw, workspaceId: "w4", labels: { task: "ASMA-21" } }),
      // The same project name on another native project: names never link it.
      sessionEntry("twin", { projectKey: "prj_twin", projectName: "ESW • ASMA-2", workspaceId: "w5", labels: { epic: "ASMA-2" } }),
    ];
    const linked = planWorkstreamMappings([bound], reads, states, [], {}, {});
    expect(linked).toEqual({ "session:task-captured": { workstream: "beta" } });
    const choices = planWorkstreamOptions([bound], reads, registry, states, linked);
    expect(choices.find((value) => value.key === "beta")).toMatchObject({ sessions: 1, workspaces: 1, plans: 1 });
    expect(choices.find((value) => value.key === "other work")).toMatchObject({ sessions: 4, workspaces: 4 });
    const tree = buildOrganizationTree({ entries: states, displayMappings: linked });
    expect(Object.fromEntries(tree.map((group) => [group.label, group.totalCount]))).toEqual({ beta: 1, "Other work": 4 });
    for (const value of choices) {
      const shown = visibleSessions(filterOrganizationTree(tree, (entry) => organizationScope(entry, linked).workstream.trim().toLowerCase() === value.key));
      expect([value.key, shown.length]).toEqual([value.key, value.sessions]);
    }
    // Each unlinked note follows the Epic line's own unknown, never claiming no label was set.
    expect(states.slice(1).map((entry) => [sessionEpic(entry, {}).note, planFallbackNote([bound], reads, declaredScope(entry, {}).epic, entry.projectKey)])).toEqual([
      [`ASMA-99 is not in the Jira capture of ${generated}`, "No known ASMA epic, so no plan applies."],
      [`sources disagree: the epic label says ASMA-3; the Jira capture of ${generated} for ASMA-21 says ASMA-2`, "No known ASMA epic, so no plan applies."],
      ["no Jira capture read for ASMA-21", "No known ASMA epic, so no plan applies."],
      ["from the epic label", "Plans for ASMA-2 are bound to other projects."],
    ]);
  });
});
