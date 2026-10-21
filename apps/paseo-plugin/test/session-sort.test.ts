import { describe, expect, it } from "vitest";
import { defaultSortDirection, explicitLeafOrder, sessionComparator, sortDirectionLabel } from "../client/session-sort";
import { buildOrganizationTree, sessionKey, type OrganizationGroup } from "../client/session-organization";
import { sessionEntry } from "./session-fixtures";

const ids = (entries: readonly { id: string }[]) => entries.map(({ id }) => id);
const at = (time: string) => ({ updatedAt: time, createdAt: time });

describe("explicit session order", () => {
  const entries = [
    sessionEntry("c", at("2026-10-20T10:00:00Z")),
    sessionEntry("b", at("2026-10-21T10:00:00Z")),
    sessionEntry("a", at("2026-10-20T10:00:00Z")),
    sessionEntry("missing", { updatedAt: "", createdAt: "" }),
    sessionEntry("invalid", { updatedAt: "not a time", createdAt: "not a time" }),
  ];

  it("keeps Current order as no comparator", () => {
    expect(sessionComparator("current", "desc")).toBeNull();
    expect(explicitLeafOrder(entries, "current", "desc")).toBeUndefined();
  });

  it.each(["updated", "created"] as const)("sorts %s numerically, missing and invalid times last both ways, ties by ID ascending", (sort) => {
    expect(ids([...entries].sort(sessionComparator(sort, "desc")!))).toEqual(["b", "a", "c", "invalid", "missing"]);
    expect(ids([...entries].sort(sessionComparator(sort, "asc")!))).toEqual(["a", "c", "b", "invalid", "missing"]);
  });

  it("compares offsets as instants, not text", () => {
    const zoned = [sessionEntry("later", at("2026-10-21T09:30:00+00:00")), sessionEntry("earlier", at("2026-10-21T10:00:00+02:00"))];
    expect(ids([...zoned].sort(sessionComparator("updated", "desc")!))).toEqual(["later", "earlier"]);
  });

  it("sorts titles case- and number-aware, then by original title, then by ID; same titles stay apart", () => {
    const titled = [sessionEntry("2", { title: "task 10" }), sessionEntry("1", { title: "Task 9" }), sessionEntry("4", { title: "task 9" }),
      sessionEntry("3", { title: "task 9" }), sessionEntry("5", { title: "Alpha" })];
    expect(ids([...titled].sort(sessionComparator("title", "asc")!))).toEqual(["5", "1", "3", "4", "2"]);
    expect(ids([...titled].sort(sessionComparator("title", "desc")!))).toEqual(["2", "3", "4", "1", "5"]);
  });

  it("starts times newest first and titles A–Z", () => {
    expect([defaultSortDirection("updated"), defaultSortDirection("created"), defaultSortDirection("title")]).toEqual(["desc", "desc", "asc"]);
    expect([sortDirectionLabel("updated", "desc"), sortDirectionLabel("title", "desc")]).toEqual(["Newest first", "Z–A"]);
  });
});

describe("explicit order in the organization tree", () => {
  const entries = [
    sessionEntry("old-a", { ...at("2026-10-19T10:00:00Z"), labels: { workstream: "alpha" }, title: "SWE • old" }),
    sessionEntry("new-a", { ...at("2026-10-21T10:00:00Z"), labels: { workstream: "alpha" }, title: "SWE • new" }),
    sessionEntry("mid-b", { ...at("2026-10-20T10:00:00Z"), labels: { workstream: "beta" }, title: "QA • mid" }),
  ];
  const sessionsOf = (groups: readonly OrganizationGroup[]): string[][] => groups.flatMap((group) => [ids(group.sessions), ...sessionsOf(group.groups)]).filter((list) => list.length);
  const labels = (groups: readonly OrganizationGroup[]): string[] => groups.flatMap((group) => [group.label, ...labels(group.groups)]);

  it("orders within each group and keeps saved group order, ignoring saved session order without rewriting it", () => {
    const base = buildOrganizationTree({ entries });
    const keys = (group: OrganizationGroup): [string, string[]][] => [[group.key, group.sessions.map((entry) => sessionKey(entry.id)).reverse()], ...group.groups.flatMap(keys)];
    // Saved: groups reversed, sessions newest first.
    const siblingOrder = { root: base.map((group) => group.key).reverse(), ...Object.fromEntries(base.flatMap(keys)) };
    const saved = JSON.stringify(siblingOrder);
    const current = buildOrganizationTree({ entries, siblingOrder });
    expect(sessionsOf(current)).toContainEqual(["new-a", "old-a"]);
    const sorted = buildOrganizationTree({ entries, siblingOrder, leafOrder: explicitLeafOrder(entries, "updated", "asc") });
    expect(labels(sorted)).toEqual(labels(current));
    expect(labels(sorted)).not.toEqual(labels(base));
    expect(sessionsOf(sorted)).toContainEqual(["old-a", "new-a"]);
    expect(JSON.stringify(siblingOrder)).toBe(saved);
  });

  it("orders flat and Role views without moving role headers, and leaves the default path unchanged", () => {
    expect(sessionsOf(buildOrganizationTree({ entries, grouping: "flat", leafOrder: explicitLeafOrder(entries, "updated", "desc") }))).toEqual([["new-a", "mid-b", "old-a"]]);
    const roles = (leafOrder?: ReadonlyMap<string, number>) => buildOrganizationTree({ entries, grouping: "role", leafOrder }).map((group) => [group.label, ids(group.sessions)]);
    expect(roles(explicitLeafOrder(entries, "created", "asc"))).toEqual([["SWE", ["old-a", "new-a"]], ["QA", ["mid-b"]]]);
    expect(roles(explicitLeafOrder(entries, "created", "desc"))).toEqual([["SWE", ["new-a", "old-a"]], ["QA", ["mid-b"]]]);
    expect(buildOrganizationTree({ entries, grouping: "flat" })).toEqual(buildOrganizationTree({ entries, grouping: "flat", leafOrder: undefined }));
  });
});
