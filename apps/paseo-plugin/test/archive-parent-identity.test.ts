import { describe, expect, it } from "vitest";
import { hasSessionAttention, toDirectoryEntry, type AgentDirectoryPage } from "../client/session-directory";
import { buildOrganizationTree, workspaceLabel, type OrganizationGroup } from "../client/session-organization";
import { matchesResultSearch, sessionProvenance } from "../client/session-results";
import { matchesMembership } from "../client/session-view";
import { sessionEntry } from "./session-fixtures";

function groups(tree: readonly OrganizationGroup[]): OrganizationGroup[] {
  return tree.flatMap((group) => [group, ...groups(group.groups)]);
}

describe("returned sessions with unnamed parent workspaces", () => {
  it("keeps exact parent IDs visible and distinct in tree and result provenance", () => {
    const entries = ["wks_a", "wks_b"].map((workspaceId) => sessionEntry(workspaceId, {
      workspaceId, workspaceName: null, projectName: "Shared project",
      archivedAt: "2026-10-20T12:00:00Z", status: "closed",
    }));
    const workspaces = groups(buildOrganizationTree({ entries })).filter((group) => group.kind === "workspace");
    expect(workspaces.map((group) => group.label).sort()).toEqual(["Workspace wks_a", "Workspace wks_b"]);
    expect(new Set(workspaces.map((group) => group.key)).size).toBe(2);
    expect(entries.map((entry) => sessionProvenance(entry, {}).workspace)).toEqual(["Workspace wks_a", "Workspace wks_b"]);
    expect(matchesResultSearch(entries[0]!, ["wks_a"], {})).toBe(true);
    expect(matchesResultSearch(entries[1]!, ["wks_a"], {})).toBe(false);
  });

  it("preserves a reported workspace name", () => {
    expect(workspaceLabel(sessionEntry("named", { workspaceName: "Known workspace" }))).toBe("Known workspace");
  });

  it("keeps the returned archived session and parent ID when project metadata is unavailable", () => {
    const source = sessionEntry("archived", {
      workspaceId: "wks_archived", workspaceName: null, projectName: null,
      archivedAt: "2026-10-20T12:00:00Z", status: "closed",
      requiresAttention: true, attentionReason: "finished",
    });
    const row = toDirectoryEntry({ agent: source as unknown as AgentDirectoryPage["agent"], project: null });
    expect(row.id).toBe(source.id);
    expect(row.workspaceId).toBe("wks_archived");
    expect(workspaceLabel(row)).toBe("Workspace wks_archived");
    expect(matchesMembership(row, "archived")).toBe(true);
    expect(matchesMembership(row, "sessions")).toBe(false);
    expect(hasSessionAttention(row)).toBe(false);
  });

  it("does not infer archive membership or inaccessible history from an unnamed parent", () => {
    const row = sessionEntry("old-readable", {
      workspaceId: "wks_unknown", workspaceName: null, projectName: null,
      status: "closed", requiresAttention: true, attentionReason: "finished",
    });
    const observed = { ...row, availability: {
      agentId: row.id, revision: row.updatedAt, checkedAt: new Date().toISOString(),
      state: "readable" as const, reason: "Native metadata only", providerHistory: "unverified" as const,
    } };
    expect(matchesMembership(observed, "sessions")).toBe(true);
    expect(matchesMembership(observed, "archived")).toBe(false);
    expect(hasSessionAttention(observed)).toBe(true);
    expect(hasSessionAttention({ ...observed, availability: { ...observed.availability, state: "unknown", reason: "Read failed" } })).toBe(true);
  });
});
