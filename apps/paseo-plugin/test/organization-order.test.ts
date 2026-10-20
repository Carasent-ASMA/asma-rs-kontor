import { describe, expect, it } from "vitest";
import { reorderSavedSiblingOrder } from "../client/organization-order";

describe("complete sibling order persistence", () => {
  it("preserves saved IDs outside the only movable durable-desk partition", () => {
    expect(reorderSavedSiblingOrder(["Z", "Y", "X", "W", "Adam"], ["Adam"], ["Adam"], "Adam", 1)).toEqual(["Z", "Y", "X", "W", "Adam"]);
  });
  it("moves only allowed slots and appends newly discovered siblings without losing saved IDs", () => {
    expect(reorderSavedSiblingOrder(["Z", "Adam", "QA", "Y"], ["Adam", "QA", "Z", "Y", "new"], ["Adam", "QA"], "Adam", 1)).toEqual(["Z", "QA", "Adam", "Y", "new"]);
  });
});
