import { responsibilityReadSchema, type ResponsibilityRead } from "../../../paseo-plugin/shared/responsibility";

/** Synthetic roster and assignments for Working as / My work; never a detected identity. */
export function responsibilityFixture(): ResponsibilityRead {
  const now = Date.now();
  return responsibilityReadSchema.parse({
    status: "ready", revision: "fixture-responsibility-1", readAt: new Date(now).toISOString(), diagnostics: [],
    projection: {
      schema: "kontor/responsibility@1", realm: "prototype.invalid",
      generatedAt: new Date(now).toISOString(), expiresAt: new Date(now + 300_000).toISOString(),
      provenance: { collector: "synthetic-prototype", selection: ["ASMA-99101", "ASMA-99102"], complete: true,
        workstreams: { path: "fixtures/workstreams.json", sha256: "a".repeat(64) } },
      people: [
        { accountId: "fixture-alex", displayName: "Alex (fictional)", active: true, accountType: "fixture" },
        { accountId: "fixture-sam", displayName: "Sam (fictional)", active: true, accountType: "fixture" },
      ],
      issues: [
        { key: "ASMA-99101", type: "Epic", level: 1, parent: null, assignee: "fixture-alex", workstreams: ["Kontor"] },
        { key: "ASMA-99103", type: "Task", level: 0, parent: "ASMA-99101", assignee: "fixture-alex" },
        { key: "ASMA-99104", type: "Task", level: 0, parent: "ASMA-99101", assignee: "fixture-sam" },
        { key: "ASMA-99102", type: "Epic", level: 1, parent: null, assignee: "fixture-sam", workstreams: ["Kontor"] },
        { key: "ASMA-99107", type: "Task", level: 0, parent: "ASMA-99102", assignee: null },
      ],
      workstreams: [{ key: "Kontor", responsible: ["fixture-alex"], participants: ["fixture-sam"] }],
    },
  });
}
