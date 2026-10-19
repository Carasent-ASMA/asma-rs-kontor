import { beforeEach, describe, expect, it } from "vitest";
import {
  clearSessionViews,
  patchSessionView,
  readSessionView,
  resetSessionView,
} from "../client/session-view-store";
import { DEFAULT_SESSION_FILTERS } from "../client/session-view";

describe("host-scoped session view state", () => {
  beforeEach(() => {
    clearSessionViews();
  });

  it("restores query, filters, expansion and scroll for the same host", () => {
    patchSessionView("host-a", {
      query: "ASMA-8540",
      status: "running",
      attentionOnly: true,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });

    expect(readSessionView("host-a")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      query: "ASMA-8540",
      status: "running",
      attentionOnly: true,
      includeArchived: true,
      advancedOpen: true,
      scrollOffset: 240,
    });
    expect(readSessionView("host-b")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    });
  });

  it("resets only the selected host", () => {
    patchSessionView("host-a", { query: "keep", scrollOffset: 80 });
    patchSessionView("host-b", { query: "other" });

    resetSessionView("host-a");

    expect(readSessionView("host-a")).toEqual({
      ...DEFAULT_SESSION_FILTERS,
      scrollOffset: 0,
    });
    expect(readSessionView("host-b").query).toBe("other");
  });
});
