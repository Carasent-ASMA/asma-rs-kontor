import { describe, expect, it, vi } from "vitest";
import {
  activeFilterCount,
  attentionLabel,
  archivedLabel,
  DEFAULT_SESSION_FILTERS,
  filterDirectoryEntries,
  hasActiveView,
  matchesQuery,
  matchesMembership,
  matchesStatus,
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
      status: "attention",
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
      includeArchived: true,
      membership: "archived" as const,
    };
    expect(activeFilterCount(filters)).toBe(2);
    expect(hasActiveView(filters)).toBe(true);
    expect(hasActiveView({ ...DEFAULT_SESSION_FILTERS, query: "  " })).toBe(true);
    expect(hasActiveView({ ...DEFAULT_SESSION_FILTERS, status: "all", query: "  " })).toBe(false);
    expect(hasActiveView({ ...DEFAULT_SESSION_FILTERS, query: "x" })).toBe(true);
  });

  it("separates Needs input, Error and Attention, with a permission wait as Needs input", () => {
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
    expect(matchesStatus(pending, "needs_input")).toBe(true);
    expect(matchesStatus(permissionReason, "needs_input")).toBe(true);
    expect(matchesStatus(idle, "needs_input")).toBe(false);
    expect(matchesStatus(pending, "attention")).toBe(false);
    expect(matchesStatus(idle, "favorites", ["idle"])).toBe(true);
    expect(matchesStatus(idle, "favorites")).toBe(false);
    expect(attentionLabel(pending)).toBe("Needs input");
    expect(attentionLabel(permissionReason)).toBe("Needs input");
    expect(
      attentionLabel(
        sessionEntry("failed", {
          status: "error",
          requiresAttention: true,
          attentionReason: "error",
        }),
      ),
    ).toBe("Error");
    expect(
      attentionLabel(
        sessionEntry("failed-idle", {
          status: "idle",
          requiresAttention: false,
          attentionReason: "error",
        }),
      ),
    ).toBeNull();
    expect(
      matchesStatus(
        sessionEntry("failed-idle", {
          status: "idle",
          requiresAttention: false,
          attentionReason: "error",
        }),
        "error",
      ),
    ).toBe(false);
    expect(
      attentionLabel(
        sessionEntry("finished", {
          status: "idle",
          requiresAttention: true,
          attentionReason: "finished",
        }),
      ),
    ).toBe("Finished");
    expect(
      attentionLabel(
        sessionEntry("attention", {
          status: "idle",
          requiresAttention: true,
        }),
      ),
    ).toBe("Attention");
    expect(attentionLabel(sessionEntry("closed", { status: "closed" }))).toBeNull();

    expect(
      filterDirectoryEntries([pending, permissionReason, idle], {
        ...DEFAULT_SESSION_FILTERS,
        status: "needs_input",
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
    expect(statusFilterLabel("initializing")).toBe("Starting");
    expect(statusFilterLabel("needs_input")).toBe("Needs input");
    expect(statusFilterLabel("favorites")).toBe("★ Favorites");
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

describe("explicit archive membership", () => {
  const archivedAt = "2026-10-18T11:00:00Z";
  const rows = [sessionEntry("running", { status: "running" }),
    sessionEntry("idle-unread", { requiresAttention: true, attentionReason: "finished" }),
    sessionEntry("closed-readable", { status: "closed" }),
    sessionEntry("archived-running", { status: "running", archivedAt }),
    sessionEntry("archived-idle", { archivedAt, requiresAttention: true, attentionReason: "finished" })];

  it.each([
    ["all", ["running", "idle-unread", "closed-readable"], ["archived-running", "archived-idle"]],
    ["current", ["running", "idle-unread"], []],
    ["running", ["running"], ["archived-running"]],
    ["idle", ["idle-unread"], ["archived-idle"]],
    ["closed", ["closed-readable"], []],
    ["attention", ["idle-unread"], []],
  ] as const)("composes %s with disjoint ordinary and archived sets", (status, ordinary, archived) => {
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status }).map((row) => row.id)).toEqual(ordinary);
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status, membership: "archived" }).map((row) => row.id)).toEqual(archived);
  });

  it("treats includeArchived as query scope only, while search and favorites stay within the selected membership", () => {
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status: "all", includeArchived: true }).map((row) => row.id)).toEqual(["running", "idle-unread", "closed-readable"]);
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status: "all", query: "archived", membership: "archived" }).map((row) => row.id)).toEqual(["archived-running", "archived-idle"]);
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status: "favorites" }, ["running", "archived-running"]).map((row) => row.id)).toEqual(["running"]);
    expect(filterDirectoryEntries(rows, { ...DEFAULT_SESSION_FILTERS, status: "favorites", membership: "archived" }, ["running", "archived-running"]).map((row) => row.id)).toEqual(["archived-running"]);
  });

  it("requires authoritative archive metadata or fresh exact-ID readable evidence, preserving unknown and closed rows", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(Date.parse("2026-10-22T09:00:00Z"));
      const ordinary = sessionEntry("exact", { status: "closed", updatedAt: "2001-01-01" });
      const evidence = { agentId: ordinary.id, revision: ordinary.updatedAt, state: "readable" as const, checkedAt: new Date().toISOString(), reason: "native fixture", archivedAt };
      expect(matchesMembership({ ...ordinary, availability: evidence }, "archived")).toBe(true);
      for (const availability of [{ ...evidence, agentId: "different-id" }, { ...evidence, revision: "different-revision" },
        { ...evidence, state: "unknown" as const, reason: "401/timeout; archive is not known" }, { ...evidence, checkedAt: "2026-10-22T08:00:00Z" }]) {
        expect(matchesMembership({ ...ordinary, availability }, "archived")).toBe(false);
        expect(matchesMembership({ ...ordinary, availability })).toBe(true);
      }
      vi.advanceTimersByTime(60_001);
      expect(matchesMembership({ ...ordinary, availability: evidence }, "archived")).toBe(false);
      expect(matchesMembership({ ...ordinary, archivedAt, availability: evidence }, "archived")).toBe(true);
      expect(matchesStatus({ ...ordinary, archivedAt, status: "idle" }, "current")).toBe(false);
    } finally { vi.useRealTimers(); }
  });
});
