import { describe, expect, it } from "vitest";
import { goBack, hostNavigation, linkedPlans, navigate } from "../client/navigation-state";
import { inboxEntries, nativeSummary, overviewRows, searchRecords } from "../client/page-data";
import { reorderedSiblings } from "../client/organization-order";
import { sessionEntry } from "./session-fixtures";

describe("read-only navigation and data truth", () => {
  it("keeps per-host context and back routes; Overview is always global", () => {
    navigate("one", { projectId: "project", tab: "Plan", sourceId: "source" });
    navigate("one", { tab: "Overview" });
    expect(hostNavigation("one").route.projectId).toBeNull();
    expect(goBack("one")).toMatchObject({ projectId: "project", tab: "Plan", sourceId: "source" });
    expect(hostNavigation("two").route.projectId).toBeNull();
  });
  it("uses explicit native conversation/source associations, including drafts without an epic", () => {
    expect(linkedPlans({ native: ["draft", "draft", "deleted"] }, "native", new Set(["draft"]))).toEqual(["draft"]);
    expect(linkedPlans({}, "ASMA-1 in a title", new Set(["draft"]))).toEqual([]);
  });
  it("keeps idle/attention separate from completion and governance; only structured confirmed keys form rows", () => {
    const entries = [sessionEntry("one", { status: "idle", requiresAttention: true, labels: { epic: "ASMA-1" } }), sessionEntry("two", { title: "ASMA-1", labels: { epic: "unconfirmed" } })];
    expect(nativeSummary(entries)).toMatchObject({ idle: 2, attention: 1 });
    expect(overviewRows(entries, []).map((row) => row.key)).toEqual(["ASMA-1", "Unassigned / ad-hoc"]);
    expect(inboxEntries(entries).map((entry) => entry.id)).toEqual(["one"]);
  });
  it("searches available labels/names, never an unprovided transcript", () => {
    const entries = [sessionEntry("one", { title: "SWE • package", labels: { ticket: "ASMA-12" } })];
    expect(searchRecords(entries, [], "ASMA-12").sessions).toHaveLength(1);
    expect(searchRecords(entries, [], "private prompt").sessions).toHaveLength(0);
  });
  it("reorders siblings without dropping rows, including removed IDs and out-of-range gestures", () => {
    expect(reorderedSiblings(["a", "b", "c"], "b", 50)).toEqual(["a", "c", "b"]);
    expect(reorderedSiblings(["a", "b", "c"], "gone", 1)).toEqual(["a", "b", "c"]);
    expect(reorderedSiblings(["a", "b", "c"], "c", -50)).toEqual(["c", "a", "b"]);
  });
});
