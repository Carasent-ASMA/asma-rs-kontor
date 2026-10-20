/**
 * Fictional sessions and workspaces. Keys in the ASMA-991xx/992xx range, people,
 * paths and titles are invented; nothing here was read from a real host.
 *
 * Labels use only keys that real sessions or the Kontor runtime wire already
 * carry (`epic`, `workstream`, `desk`, `role`, `task`, `jira.issue`,
 * `jira.epic`, `kontor.role`, `kontor.team_run`, `takeover.predecessor`).
 * They are unenforced session labels, and the prototype shows them as such.
 */
export type FixtureAgent = {
  id: string;
  title: string;
  status: "initializing" | "idle" | "running" | "error" | "closed";
  requiresAttention?: boolean;
  attentionReason?: "finished" | "error" | "permission" | null;
  attentionTimestamp?: string | null;
  pendingPermissions?: unknown[];
  archivedAt?: string | null;
  createdAt: string;
  updatedAt: string;
  cwd: string;
  workspaceId: string | null;
  provider: string;
  model: string | null;
  parentAgentId?: string | null;
  labels: Record<string, string>;
};
export type FixtureProject = { projectKey: string; projectName: string; workspaceName: string | null };
export type FixtureSession = { agent: FixtureAgent; project: FixtureProject | null };
export type FixtureWorkspace = {
  id: string; projectId: string; projectDisplayName: string; projectCustomName: string | null;
  title: string | null; name: string; directory: string; status: "active";
};

const PROJECT = { projectKey: "proj_kontor_demo", projectName: "kontor-demo" };
const MOBILE = { projectKey: "proj_mobile_demo", projectName: "mobile-demo" };
const at = (minutesAgo: number) => new Date(Date.parse("2026-10-20T09:00:00Z") - minutesAgo * 60_000).toISOString();

export const WORKSPACES: FixtureWorkspace[] = [
  ws("wks_esw_99101", "ESW • ASMA-99101 Sessions organisation", PROJECT),
  ws("wks_tsw_99103", "TSW • ASMA-99103 Flat filtered results", PROJECT),
  ws("wks_tsw_99104", "TSW • ASMA-99104 Board team chain", PROJECT),
  ws("wks_tsw_99105", "TSW • ASMA-99105 Restore tree on clear", PROJECT),
  ws("wks_rsw_99104", "RSW • ASMA-99104 Review room", PROJECT),
  ws("wks_tsw_99107", "TSW • ASMA-99107 Parallel verification", PROJECT),
  ws("wks_tsw_99202", "TSW • ASMA-99202 Sign-in flow", MOBILE),
  ws("wks_desk_review", "DESK • PR Review", PROJECT),
  ws("wks_desk_adam", "DESK • Adam", PROJECT),
  // An empty workspace: listed by the workspace inventory, no sessions.
  ws("wks_tsw_99109", "TSW • ASMA-99109 Not started", PROJECT),
];

function ws(id: string, name: string, project: { projectKey: string; projectName: string }): FixtureWorkspace {
  return { id, projectId: project.projectKey, projectDisplayName: project.projectName, projectCustomName: null, title: name, name, directory: `/work/${project.projectName}/${id}`, status: "active" };
}

type Seed = {
  id: string; title: string; ws: string | null; status?: FixtureAgent["status"]; minutes: number;
  labels?: Record<string, string>; needsInput?: boolean; attention?: FixtureAgent["attentionReason"];
  archived?: boolean; project?: { projectKey: string; projectName: string } | null; model?: string;
};

function session(seed: Seed): FixtureSession {
  const workspace = WORKSPACES.find((value) => value.id === seed.ws) ?? null;
  const project = seed.project === null ? null : seed.project ?? PROJECT;
  return {
    agent: {
      id: seed.id, title: seed.title, status: seed.status ?? "idle",
      requiresAttention: seed.attention !== undefined && seed.attention !== null,
      attentionReason: seed.needsInput ? "permission" : seed.attention ?? null,
      attentionTimestamp: seed.attention || seed.needsInput ? at(seed.minutes) : null,
      pendingPermissions: seed.needsInput ? [{ id: `perm_${seed.id}`, kind: "tool" }] : [],
      archivedAt: seed.archived ? at(seed.minutes - 1) : null,
      createdAt: at(seed.minutes + 240), updatedAt: at(seed.minutes),
      cwd: workspace?.directory ?? `/work/scratch/${seed.id}`, workspaceId: workspace?.id ?? null,
      provider: "claude", model: seed.model ?? "claude-opus-5-5", parentAgentId: null, labels: seed.labels ?? {},
    },
    project: project ? { ...project, workspaceName: workspace?.name ?? null } : null,
  };
}

const kontor = (epic: string, extra: Record<string, string>) => ({ workstream: "Kontor", epic, ...extra });

/** The default inventory: one host, three workstreams, two desks and honest gaps. */
export function baseSessions(): FixtureSession[] {
  return [
    // Epic coordination: LSA and TPM work side by side.
    session({ id: "agt_lsa_99101", title: "LSA • ASMA-99101 sessions organisation", ws: "wks_esw_99101", status: "idle", minutes: 35, labels: kontor("ASMA-99101", { role: "lsa" }) }),
    session({ id: "agt_tpm_99101", title: "TPM • ASMA-99101 evidence", ws: "wks_esw_99101", status: "running", minutes: 2, labels: kontor("ASMA-99101", { role: "tpm" }) }),

    // ASMA-99103: scope, build, then QA and AUD in parallel; QA waits on a permission.
    session({ id: "agt_sa_99103", title: "SA • ASMA-99103 scope", ws: "wks_tsw_99103", status: "closed", minutes: 300, labels: kontor("ASMA-99101", { role: "sa", task: "ASMA-99103" }) }),
    session({ id: "agt_swe_99103", title: "SWE • ASMA-99103 flat results", ws: "wks_tsw_99103", status: "running", minutes: 1, labels: kontor("ASMA-99101", { role: "swe", task: "ASMA-99103" }) }),
    session({ id: "agt_qa_99103", title: "QA • ASMA-99103 verify results", ws: "wks_tsw_99103", status: "idle", minutes: 6, needsInput: true, labels: kontor("ASMA-99101", { role: "qa", task: "ASMA-99103" }) }),
    session({ id: "agt_aud_99103", title: "AUD • ASMA-99103 spec audit", ws: "wks_tsw_99103", status: "running", minutes: 4, labels: kontor("ASMA-99101", { role: "aud", task: "ASMA-99103" }) }),

    // ASMA-99104: the first builder failed and was replaced; a review room in a second workspace.
    session({ id: "agt_swe_99104_a", title: "SWE • ASMA-99104 chain (first)", ws: "wks_tsw_99104", status: "error", minutes: 90, attention: "error", labels: kontor("ASMA-99101", { "kontor.role": "SWE", "jira.issue": "ASMA-99104", "kontor.team_run": "run_7c1e" }) }),
    session({ id: "agt_swe_99104_b", title: "SWE • ASMA-99104 chain (replacement)", ws: "wks_tsw_99104", status: "running", minutes: 3, labels: kontor("ASMA-99101", { "kontor.role": "SWE", "jira.issue": "ASMA-99104", "kontor.team_run": "run_7c1e", "takeover.predecessor": "agt_swe_99104_a" }) }),
    session({ id: "agt_qa_99104", title: "QA • ASMA-99104 chain checks", ws: "wks_tsw_99104", status: "idle", minutes: 50, labels: kontor("ASMA-99101", { "kontor.role": "QA", "jira.issue": "ASMA-99104", "kontor.team_run": "run_7c1e" }) }),
    session({ id: "agt_reva_99104", title: "REV A • ASMA-99104 review", ws: "wks_rsw_99104", status: "idle", minutes: 20, labels: kontor("ASMA-99101", { role: "rev-a", task: "ASMA-99104" }) }),

    // ASMA-99105: idle now, with an earlier error still flagged (a historical cue, not the runtime state).
    session({ id: "agt_swe_99105", title: "SWE • ASMA-99105 restore tree", ws: "wks_tsw_99105", status: "idle", minutes: 15, attention: "error", labels: kontor("ASMA-99101", { role: "swe", task: "ASMA-99105" }) }),
    // ASMA-99106: only an archived session remains.
    session({ id: "agt_qa_99106", title: "QA • ASMA-99106 legacy check", ws: "wks_tsw_99105", status: "closed", minutes: 2000, archived: true, labels: kontor("ASMA-99101", { role: "qa", task: "ASMA-99106" }) }),

    // ASMA-99107 (another epic): two builders and two QA sessions in parallel.
    session({ id: "agt_swe1_99107", title: "SWE • ASMA-99107 parser", ws: "wks_tsw_99107", status: "running", minutes: 2, labels: kontor("ASMA-99102", { role: "swe", task: "ASMA-99107" }) }),
    session({ id: "agt_swe2_99107", title: "SWE • ASMA-99107 renderer", ws: "wks_tsw_99107", status: "initializing", minutes: 0, labels: kontor("ASMA-99102", { role: "swe", task: "ASMA-99107" }) }),
    session({ id: "agt_qa1_99107", title: "QA • ASMA-99107 parser checks", ws: "wks_tsw_99107", status: "idle", minutes: 12, attention: "finished", labels: kontor("ASMA-99102", { role: "qa", task: "ASMA-99107" }) }),
    session({ id: "agt_qa2_99107", title: "QA • ASMA-99107 renderer checks", ws: "wks_tsw_99107", status: "idle", minutes: 30, labels: kontor("ASMA-99102", { role: "qa", task: "ASMA-99107" }) }),

    // Mobile workstream.
    session({ id: "agt_swe_99202", title: "SWE • ASMA-99202 sign-in", ws: "wks_tsw_99202", status: "running", minutes: 5, project: MOBILE, labels: { workstream: "Mobile", epic: "ASMA-99201", role: "swe", "jira.issue": "ASMA-99202" } }),
    session({ id: "agt_qa_99202", title: "QA • ASMA-99202 sign-in checks", ws: "wks_tsw_99202", status: "error", minutes: 25, attention: "error", project: MOBILE, labels: { workstream: "Mobile", epic: "ASMA-99201", role: "qa", "jira.issue": "ASMA-99202" } }),

    // Desks: durable seats outside any epic.
    session({ id: "agt_reva_pr812", title: "REV A • PR 812", ws: "wks_desk_review", status: "running", minutes: 1, labels: { desk: "PR Review", pr: "812" } }),
    session({ id: "agt_revb_pr812", title: "REV B • PR 812", ws: "wks_desk_review", status: "idle", minutes: 9, needsInput: true, labels: { desk: "PR Review", pr: "812" } }),
    session({ id: "agt_judge_pr812", title: "JUDGE • PR 812", ws: "wks_desk_review", status: "idle", minutes: 40, labels: { desk: "PR Review", pr: "812" } }),
    session({ id: "agt_adam", title: "ADAM • triage SWE handoffs", ws: "wks_desk_adam", status: "idle", minutes: 60, labels: { desk: "Adam" } }),

    // Honest gaps: no labels at all; a workspace binding the inventory cannot name;
    // a title that mentions a task key without any label (never joined to the task).
    session({ id: "agt_scratch", title: "Explore SWE graph layout options", ws: null, status: "idle", minutes: 400, project: null }),
    session({ id: "agt_unbound", title: "SWE • retry flaky export", ws: null, status: "running", minutes: 7, labels: { epic: "ASMA-99101", workstream: "Kontor", role: "swe" } }),
    session({ id: "agt_title_only", title: "Notes on ASMA-99108 rollout", ws: "wks_esw_99101", status: "idle", minutes: 120, labels: kontor("ASMA-99101", {}) }),
  ];
}

/** A bigger inventory for scroll and restore checks: 60 more ordinary sessions under three epics. */
export function scrollSessions(): FixtureSession[] {
  const extra: FixtureSession[] = [];
  const epics = ["ASMA-99110", "ASMA-99111", "ASMA-99112"];
  for (let index = 0; index < 60; index += 1) {
    const epic = epics[index % 3]!;
    const role = ["swe", "qa", "aud"][index % 3]!;
    extra.push(session({ id: `agt_bulk_${index}`, title: `${role.toUpperCase()} • ${epic} batch ${index + 1}`, ws: null, status: index % 7 === 0 ? "running" : "idle", minutes: 100 + index,
      labels: { workstream: index % 2 ? "Kontor" : "Platform", epic, role } }));
  }
  return [...baseSessions(), ...extra];
}

/**
 * The Roles review inventory: the default one plus role variants. Case and spacing
 * variants share a header, a numbered slot keeps its own, and two sessions whose
 * sources disagree show the label's role with a conflict cue.
 */
export function roleSessions(): FixtureSession[] {
  return [
    ...baseSessions(),
    session({ id: "agt_qa_mixed", title: "Qa • ASMA-99105 restore checks", ws: "wks_tsw_99105", status: "running", minutes: 8, labels: kontor("ASMA-99101", { role: "Qa", task: "ASMA-99105" }) }),
    session({ id: "agt_qa_spaced", title: "ASMA-99202 sign-in recheck", ws: "wks_tsw_99202", status: "idle", minutes: 14, project: MOBILE, labels: { workstream: "Mobile", epic: "ASMA-99201", "kontor.role": "  qa ", "jira.issue": "ASMA-99202" } }),
    session({ id: "agt_qa1_slot", title: "QA1 • ASMA-99107 parser rerun", ws: "wks_tsw_99107", status: "error", minutes: 11, attention: "error", labels: kontor("ASMA-99102", {}) }),
    session({ id: "agt_conflict_title", title: "AUD • ASMA-99104 spot check", ws: "wks_rsw_99104", status: "idle", minutes: 18, needsInput: true, labels: kontor("ASMA-99101", { role: "qa", task: "ASMA-99104" }) }),
    session({ id: "agt_conflict_labels", title: "ASMA-99103 release notes", ws: "wks_tsw_99103", status: "running", minutes: 3, labels: kontor("ASMA-99101", { "kontor.role": "SWE", role: "tpm", task: "ASMA-99103" }) }),
    session({ id: "agt_revb_label", title: "Second look at PR 812", ws: "wks_desk_review", status: "idle", minutes: 22, labels: { desk: "PR Review", pr: "812", role: "rev_b" } }),
  ];
}
