import { describe, expect, it } from "vitest";
import {
  activeFilterCount,
  attentionLabel,
  archivedLabel,
  DEFAULT_SESSION_FILTERS,
  filterDirectoryEntries,
  hasActiveView,
  matchesQuery,
  needsAttention,
  statusFilterLabel,
} from "../client/session-view";
import { sessionEntry } from "./session-fixtures";

describe("session search and filters", () => {
  const entries = [
    sessionEntry("a", {
      title: "SWE • ASMA-8540 • find sessions",
      status: "running",
      requiresAttention: true,
      attentionReason: "permission",
      workspaceName: "ASMA-8540 navigator",
      projectName: "asma-rs-kontor",
      cwd: "/tmp/asma-rs-kontor",
    }),
    sessionEntry("b", {
      title: "QA • ASMA-8540 • qualify sessions",
      status: "idle",
      workspaceName: "ASMA-8540 navigator",
      projectName: "asma-rs-kontor",
    }),
    sessionEntry("c", {
      title: "Unlabelled work",
      status: "closed",
      workspaceName: null,
      projectName: "Scratch",
      cwd: "/tmp/scratch",
    }),
  ];

  it("matches the full title and workspace context, not only the visible prefix", () => {
    expect(matchesQuery(entries[0]!, "ASMA-8540")).toBe(true);
    expect(matchesQuery(entries[0]!, "navigator")).toBe(true);
    expect(matchesQuery(entries[0]!, "kontor")).toBe(true);
    expect(matchesQuery(entries[0]!, "swe 8540")).toBe(true);
    expect(matchesQuery(entries[0]!, "missing")).toBe(false);
  });

  it("filters by native state and attention independently", () => {
    const running = filterDirectoryEntries(entries, {
      ...DEFAULT_SESSION_FILTERS,
      status: "running",
    });
    expect(running.map((entry) => entry.id)).toEqual(["a"]);

    const attention = filterDirectoryEntries(entries, {
      ...DEFAULT_SESSION_FILTERS,
      attentionOnly: true,
    });
    expect(attention.map((entry) => entry.id)).toEqual(["a"]);

    const closed = filterDirectoryEntries(entries, {
      ...DEFAULT_SESSION_FILTERS,
      status: "closed",
    });
    expect(closed.map((entry) => entry.id)).toEqual(["c"]);
  });

  it("counts only advanced choices and exposes the clear state", () => {
    const filters = {
      ...DEFAULT_SESSION_FILTERS,
      status: "running" as const,
      attentionOnly: true,
      includeArchived: true,
    };
    expect(activeFilterCount(filters)).toBe(3);
    expect(hasActiveView(filters)).toBe(true);
    expect(hasActiveView({ ...DEFAULT_SESSION_FILTERS, query: "  " })).toBe(false);
    expect(hasActiveView({ ...DEFAULT_SESSION_FILTERS, query: "x" })).toBe(true);
  });

  it("includes a permission wait in attention even when requiresAttention is false", () => {
    const pending = sessionEntry("pending", {
      status: "running",
      requiresAttention: false,
      pendingPermissionCount: 1,
    });
    const permissionReason = sessionEntry("permission-reason", {
      status: "running",
      requiresAttention: false,
      attentionReason: "permission",
    });
    const idle = sessionEntry("idle", { status: "idle" });

    expect(pending.needsInput).toBe(true);
    expect(permissionReason.needsInput).toBe(true);
    expect(needsAttention(pending)).toBe(true);
    expect(needsAttention(permissionReason)).toBe(true);
    expect(needsAttention(idle)).toBe(false);
    expect(attentionLabel(pending)).toBe("Needs input");
    expect(attentionLabel(permissionReason)).toBe("Needs input");

    expect(
      filterDirectoryEntries([pending, permissionReason, idle], {
        ...DEFAULT_SESSION_FILTERS,
        attentionOnly: true,
      }).map((entry) => entry.id),
    ).toEqual(["pending", "permission-reason"]);

    expect(
      filterDirectoryEntries([pending, permissionReason, idle], {
        ...DEFAULT_SESSION_FILTERS,
        status: "running",
      }).map((entry) => entry.id),
    ).toEqual(["pending", "permission-reason"]);
  });

  it("labels native state and attention without interpreting completion", () => {
    expect(statusFilterLabel("all")).toBe("All");
    expect(statusFilterLabel("initializing")).toBe("Initializing");
    expect(attentionLabel(entries[0]!)).toBe("Needs input");
    expect(attentionLabel(entries[1]!)).toBeNull();
    expect(archivedLabel(entries[0]!)).toBeNull();
    expect(
      archivedLabel(
        sessionEntry("archived", { archivedAt: "2026-10-18T11:00:00.000Z" }),
      ),
    ).toBe("Archived");
  });
});
