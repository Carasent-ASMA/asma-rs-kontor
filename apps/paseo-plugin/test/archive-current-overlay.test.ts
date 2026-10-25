import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_SESSION_FILTERS, matchesStatus, QUICK_STATUS_FILTERS } from "../client/session-view";
import { clearSessionFilterPatch, clearSessionViews, isResultMode, readSessionView, readWorkingAs, resetSessionView, resetSessionViewPatch, sessionFilterCount, sessionViewTransition, writeWorkingAs } from "../client/session-view-store";
import { QUICK_FILTER_MEANINGS } from "../client/mobile-quick-filter-options";
import { sessionEntry } from "./session-fixtures";
describe("Current overlay and the Recent default", () => {
  beforeEach(() => clearSessionViews({ work: "all", status: "current" }));
  it("keeps Current and All in the native tree; Clear widens Current to All", () => {
    const view=readSessionView("current-overlay");
    for(const status of ["current","all"] as const){expect(isResultMode({...view,status})).toBe(false);expect(sessionFilterCount({...view,status})).toBe(status === "current" ? 1 : 0);}
    const result={...view,status:"attention" as const};expect(isResultMode(result)).toBe(false);expect(clearSessionFilterPatch(result).status).toBe("all");
    expect(clearSessionFilterPatch(view).status).toBe("all");
    expect(clearSessionFilterPatch({...view,status:"all"})).toEqual({});
  });
  it("Clear preserves presentation while Reset restores the default visibility", () => {
    const view={...readSessionView("clear-reset"),query:"find",status:"attention" as const,shortcut:"desk",streams:[],task:{jira:"ASMA-8541",kontorId:null,label:"task"},includeArchived:true,grouping:"role" as const,advancedOpen:true};
    const cleared={...view,...clearSessionFilterPatch(view)};
    expect(cleared).toMatchObject({query:"",status:"all",shortcut:null,streams:null,task:null,includeArchived:false,grouping:"role",advancedOpen:true});
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(resetSessionView("clear-reset").status).toBe("current");
  });
  it("Current describes native-state visibility without certifying provider history", () => {
    expect(QUICK_FILTER_MEANINGS.current).toContain("Native-state visibility");
    expect(QUICK_FILTER_MEANINGS.current).toContain("Provider history is not verified");
  });
  it.each([
    ["all", "any"], ["mine", "any"], ["all", "me"], ["mine", "me"],
    ["all", "unknown"], ["mine", "unknown"], ["all", "person:account-b"], ["mine", "person:account-b"],
  ] as const)("Clear widens %s/%s to All/All work/Any assignment, preserves presentation and viewing identity, and is idempotent", (work, assignment) => {
    writeWorkingAs("host-a", "tracker.example", "viewer");
    const view = { ...readSessionView("host-a"), work, assignment, query: "find", status: "attention" as const, shortcut: "desk", streams: [],
      task: { jira: "ASMA-7", kontorId: null, label: "ASMA-7" }, includeArchived: true, grouping: "role" as const,
      sort: "title" as const, sortDirection: "desc" as const };
    const cleared = { ...view, ...sessionViewTransition(view, clearSessionFilterPatch(view)) };
    expect(cleared).toMatchObject({ work: "all", assignment: "any", status: "all", query: "", shortcut: null, streams: null, task: null, includeArchived: false,
      grouping: "role", sort: "title", sortDirection: "desc" });
    expect(sessionFilterCount(cleared)).toBe(0);
    expect(clearSessionFilterPatch(cleared)).toEqual({});
    expect(readWorkingAs("host-a", "tracker.example")).toBe("viewer");
  });
  it("Reset from explicit All/All work restores Recent/My work/default order and grouping without changing identity", () => {
    writeWorkingAs("host-a", "tracker.example", "viewer");
    const view = { ...readSessionView("host-a"), work: "all" as const, status: "all" as const, grouping: "flat" as const,
      sort: "created" as const, sortDirection: "desc" as const };
    const reset = { ...view, ...sessionViewTransition(view, resetSessionViewPatch(view)) };
    expect(reset).toMatchObject({ status: "recent", work: "mine", assignment: "any", sort: undefined, sortDirection: undefined, grouping: undefined });
    expect(sessionFilterCount(reset)).toBe(2);
    expect(readWorkingAs("host-a", "tracker.example")).toBe("viewer");
  });
  it("Reset clears compound narrowing while retaining the same Recent/My work defaults", () => {
    const view = { ...readSessionView("host-a"), work: "mine" as const, assignment: "person:account-b" as const, status: "error" as const, query: "find",
      shortcut: "desk", streams: [], task: { jira: "ASMA-7", kontorId: null, label: "ASMA-7" }, includeArchived: true };
    expect({ ...view, ...resetSessionViewPatch(view) }).toMatchObject({ status: "recent", work: "mine", assignment: "any", query: "", shortcut: null,
      streams: null, task: null, includeArchived: false, sort: undefined, grouping: undefined });
  });
  it("defaults to Recent while preserving explicit Current, All and Attention", () => {
    expect(DEFAULT_SESSION_FILTERS.status).toBe("recent");expect(QUICK_STATUS_FILTERS).toContain("current");expect(QUICK_STATUS_FILTERS).toContain("all");expect(QUICK_STATUS_FILTERS).toContain("attention");
    const old=sessionEntry("old",{status:"idle",updatedAt:"2001-01-01",requiresAttention:true,attentionReason:"finished"});
    const unknown={...old,id:"unknown",availability:{agentId:"unknown",state:"unknown" as const,reason:"401/timeout",checkedAt:new Date().toISOString(),revision:old.updatedAt}};
    for(const row of [old,unknown,sessionEntry("permission",{status:"error",needsInput:true})])expect(matchesStatus(row,"current")).toBe(true);
    for(const status of ["error","closed"] as const){const row=sessionEntry(status,{status});expect(matchesStatus(row,"current")).toBe(false);expect(matchesStatus(row,"all")).toBe(true);}
  });
  it("never hides readable old unread results from explicit Attention",()=>{const row=sessionEntry("old",{status:"closed",requiresAttention:true,attentionReason:"finished"});expect(matchesStatus(row,"attention")).toBe(true);});
});
