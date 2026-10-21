import type { OrganizationProject, OrganizationWorkspace } from "../../client/session-organization";
import { sessionEntry } from "../session-fixtures";

/** Entirely fictional native IDs, names and associations; no live inventory. */
export const NATIVE_DESK_PROJECTS: OrganizationProject[] = [
  { id: "prj_desk", name: "DESK • ADAM" },
  { id: "prj_twin", name: "DESK • ADAM" },
  { id: "prj_review", name: "DESK • PR REVIEW" },
  { id: "prj_empty", name: "DESK • RESEARCH" },
  { id: "prj_control", name: "ESW • ASMA-99101" },
];
export const NATIVE_DESK_WORKSPACES: OrganizationWorkspace[] = [
  { id: "w_plan", name: "Active plan format • ASMA-99101", projectKey: "prj_desk", projectName: "DESK • ADAM" },
  { id: "w_csw", name: "CSW • ASMA-99102 • review", projectKey: "prj_desk", projectName: "DESK • ADAM" },
  { id: "w_adam", name: "ADAM", projectKey: "prj_desk", projectName: "DESK • ADAM" },
  { id: "w_asw", name: "ASW • ASMA-99103 • advice", projectKey: "prj_desk", projectName: "DESK • ADAM" },
  { id: "w_idle", name: "ADAM", projectKey: "prj_desk", projectName: "DESK • ADAM" },
  { id: "w_twin", name: "ADAM", projectKey: "prj_twin", projectName: "DESK • ADAM" },
  { id: "w_review", name: "RSW • ASMA-99104", projectKey: "prj_review", projectName: "DESK • PR REVIEW" },
  { id: "w_control", name: "TSW • ASMA-99101", projectKey: "prj_control", projectName: "ESW • ASMA-99101" },
];
export function nativeDeskSessions() {
  const session = (id: string, workspaceId: string, labels: Record<string, string> = {}, title = `LSA • ${id}`) => {
    const workspace = NATIVE_DESK_WORKSPACES.find((workspace) => workspace.id === workspaceId)!;
    return sessionEntry(id, { workspaceId, workspaceName: workspace.name, projectKey: workspace.projectKey, projectName: workspace.projectName, labels, title });
  };
  return [
    session("desk-epic-a", "w_adam", { epic: "ASMA-99101", workstream: "alpha" }),
    session("desk-epic-b", "w_adam", { "jira.epic": "ASMA-99102", workstream: "beta" }),
    session("desk-epic-c", "w_adam", { "origin.epic": "ASMA-99103" }),
    session("desk-epic-d", "w_adam", { epic: "ASMA-99104", desk: "pr-review" }),
    session("desk-unlabelled", "w_adam"), session("desk-plan", "w_plan"),
    session("desk-csw", "w_csw"), session("desk-asw", "w_asw"),
    session("twin", "w_twin"), session("review", "w_review", { desk: "pr-review", epic: "ASMA-99105" }),
    session("control", "w_control", { epic: "ASMA-99101", workstream: "control-stream" }, "SWE • control"),
  ];
}
