import type { OrganizationSettings } from "../../paseo-plugin/shared/organization";
import type { KeyboardSettings } from "../../paseo-plugin/shared/keyboard";
import type { SessionGrouping } from "../../paseo-plugin/client/session-view-store";

/**
 * Review scenarios. Each opens the same plugin source on a deterministic
 * fixture; `start` only seeds the plugin's own in-memory route and view, the
 * way a user would have left them.
 */
export type Scenario = {
  readonly id: string;
  readonly title: string;
  readonly summary: string;
  readonly inventory: "base" | "large" | "roles" | "none";
  readonly sources: boolean;
  readonly settings?: Partial<OrganizationSettings>;
  /** The saved keyboard document; without it the Back shortcut is off, as on a new host. */
  readonly keyboard?: Partial<KeyboardSettings>;
  readonly start?: {
    readonly tab?: "Sessions" | "Board" | "Overview" | "Plan";
    readonly sourceId?: string;
    readonly query?: string;
    readonly status?: "error" | "needs_input" | "running" | "attention";
    /** Grouping chosen in this Sessions view (kept in memory, not saved). */
    readonly grouping?: SessionGrouping;
  };
  /** Scripted native changes, applied in order and labelled simulated. */
  readonly script?: readonly { readonly afterMs: number; readonly agentId: string; readonly patch: Record<string, unknown>; readonly note: string }[];
  readonly hostOfflineAfterMs?: number;
  /** Board reads SIMULATED team, workflow and risk-tier declarations (`fixtures/teams.ts`); labelled in the banner. */
  readonly simulatedTeams?: boolean;
};

/** Role grouping saved, with only QA saved open; every other role header opens closed. */
const ROLES: Partial<OrganizationSettings> = { grouping: "role", expandedGroups: { "roles/role:QA": true } };

const EXPANDED: Partial<OrganizationSettings> = {
  expandedGroups: {
    "workstream:kontor": true,
    "workstream:kontor/project:proj_kontor_demo/epic:asma-99101": true,
  },
};

export const SCENARIOS: readonly Scenario[] = [
  { id: "baseline", title: "Baseline", summary: "Default inventory, Sessions tree with one epic open.", inventory: "base", sources: true, settings: EXPANDED },
  { id: "filtered-errors", title: "Filtered: errors", summary: "Error filter on: runtime errors and an earlier error cue on an idle session.", inventory: "base", sources: true, settings: EXPANDED, start: { status: "error" } },
  { id: "needs-input", title: "Filtered: needs input", summary: "Needs input filter on: a task QA session and a desk reviewer.", inventory: "base", sources: true, settings: EXPANDED, start: { status: "needs_input" } },
  { id: "provenance", title: "Search: provenance", summary: "Search “swe”: matches across epics, a desk, a session without a workspace and one with no labels.", inventory: "base", sources: true, settings: EXPANDED, start: { query: "swe" } },
  { id: "empty-results", title: "Empty results", summary: "A search that matches nothing.", inventory: "base", sources: true, settings: EXPANDED, start: { query: "no such session" } },
  { id: "clear-restore", title: "Clear restores the tree", summary: "84 sessions. Open groups, scroll, filter, then clear: the tree and scroll come back.", inventory: "large", sources: true, settings: EXPANDED },
  { id: "board-tasks", title: "Board: task sessions", summary: "Plan ASMA-99101 on Board: zero, one and several linked sessions per task.", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99101" } },
  { id: "team-declared", title: "Board: declared teams (simulated)", summary: "Plan ASMA-99101 with SIMULATED team, workflow and risk declarations: parallel QA and AUD, missing roles, a partial declaration and undeclared tasks.", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99101" }, simulatedTeams: true },
  { id: "team-unordered", title: "Board: team without order (simulated)", summary: "Plan ASMA-99102 with SIMULATED declarations: roles with no declared handoff, and a declared team with no session yet.", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99102" }, simulatedTeams: true },
  { id: "parallel-roles", title: "Board: parallel roles", summary: "Plan ASMA-99102: two builders and two QA sessions working at once.", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99102" } },
  {
    id: "simulated-updates", title: "Scripted changes", summary: "Native changes every few seconds from a script (simulated, not live).", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99101" },
    script: [
      { afterMs: 4000, agentId: "agt_qa_99103", patch: { pendingPermissions: [], attentionReason: null, requiresAttention: false, status: "running" }, note: "QA permission answered; running" },
      { afterMs: 8000, agentId: "agt_swe_99103", patch: { status: "idle", requiresAttention: true, attentionReason: "finished" }, note: "SWE finished; idle" },
      { afterMs: 12000, agentId: "agt_aud_99103", patch: { status: "error", requiresAttention: true, attentionReason: "error" }, note: "AUD failed" },
      { afterMs: 16000, agentId: "agt_qa_99104", patch: { status: "running" }, note: "QA started on ASMA-99104" },
    ],
  },
  { id: "role-view", title: "Roles: one level", summary: "Role grouping: one header per role, singletons and No role included. QA gathers case and spacing variants; QA1 stays apart; two sessions disagree with their titles or labels. A session row's Details button gives its full place; one AUD session has a long title and workstream.", inventory: "roles", sources: true, settings: ROLES },
  { id: "role-view-offline", title: "Roles: host goes offline", summary: "Role picked in this view; the host drops at 4 s, saved preferences become unavailable, and the role headers give last known counts.", inventory: "roles", sources: true, settings: ROLES, start: { grouping: "role" }, hostOfflineAfterMs: 4000 },
  { id: "host-offline", title: "Host goes offline", summary: "The host drops after 4 s: last known states, marked stale.", inventory: "base", sources: true, start: { tab: "Board", sourceId: "plan-99101" }, hostOfflineAfterMs: 4000 },
  { id: "keyboard-back", title: "Keyboard: Back shortcut", summary: "The Back shortcut is saved as Cmd+` (Cmd+< on Norwegian). Open another view, then press it to go back one view. It does nothing while you type in Search or with a menu open. Settings › Keyboard changes it and has Test shortcut.", inventory: "base", sources: true, settings: EXPANDED, keyboard: { backShortcut: "cmd-backquote" }, start: { tab: "Board", sourceId: "plan-99101" } },
  { id: "no-sessions", title: "No sessions", summary: "An empty host: Board tasks have no linked sessions.", inventory: "none", sources: true, start: { tab: "Board", sourceId: "plan-99101" } },
];
