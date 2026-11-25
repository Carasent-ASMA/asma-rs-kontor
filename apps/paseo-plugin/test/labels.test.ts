import { describe, expect, it } from "vitest";
import { claimsWorkflow, hasKontorLabels, isKontorSeat, seatBinding } from "../server/labels";
import { epicProjectKey } from "../shared/session-create";

describe("hasKontorLabels", () => {
  it("recognizes any runtime-owned Kontor label", () => {
    expect(hasKontorLabels({ "kontor.seat_binding_id": "binding-1" })).toBe(true);
    expect(hasKontorLabels({ "kontor.worktree": "/tmp/worktree" })).toBe(true);
  });

  it("leaves a Paseo-direct-only seat unbound", () => {
    expect(hasKontorLabels({ orchestration: "paseo-direct", "asma.slot": "SWE" })).toBe(false);
    expect(hasKontorLabels({})).toBe(false);
    expect(hasKontorLabels(null)).toBe(false);
  });
});

describe("isKontorSeat", () => {
  it("recognizes runtime-owned Kontor labels", () => {
    expect(isKontorSeat({ "kontor.seat_binding_id": "binding-1" })).toBe(true);
    expect(isKontorSeat({ "kontor.agent-run": "kontor-run-1" })).toBe(true);
    expect(isKontorSeat({ "kontor.hosted_seat": "true" })).toBe(true);
  });

  it("recognizes a Paseo-direct delivery seat", () => {
    expect(isKontorSeat({ orchestration: "paseo-direct", "asma.slot": "TPM" })).toBe(true);
  });

  it("leaves unlabelled and interactive sessions alone", () => {
    expect(isKontorSeat({})).toBe(false);
    expect(isKontorSeat(null)).toBe(false);
    expect(isKontorSeat(undefined)).toBe(false);
    expect(isKontorSeat({ pr: "310" })).toBe(false);
    expect(isKontorSeat({ orchestration: "kontor" })).toBe(false);
  });
});

describe("seatBinding", () => {
  it("reads the hosted seat binding written by a Kontor launch", () => {
    expect(seatBinding({ "kontor.seat_binding_id": "binding-1" })).toEqual({
      seat: "binding-1",
    });
  });

  it("reads the delivery role-slot id", () => {
    expect(seatBinding({ "kontor.role_slot_id": "slot-1" })).toEqual({
      seat: "slot-1",
    });
  });

  it("falls back to the run correlation label", () => {
    expect(seatBinding({ "kontor.agent-run": "run-1" })).toEqual({
      seat: "run-1",
    });
  });

  it("stays empty without a writer-owned seat label", () => {
    expect(seatBinding({ "kontor.hosted_seat": "true" })).toEqual({ seat: null });
    expect(seatBinding({ "kontor.seat": "unwritten" })).toEqual({ seat: null });
    expect(seatBinding(null)).toEqual({ seat: null });
  });
});

describe("claimsWorkflow", () => {
  it("reads current and retained markers, never a summary helper's or a free one's", () => {
    const marked: Record<string, string>[] = [
      { orchestration: "paseo" },
      { orchestration: "kontor" },
      { orchestration: "paseo-direct" },
      { orchestration: "kontor-governed" },
      { workflow: "kontor" },
      { workflow: "free", orchestration: "paseo" },
      { "kontor.seat_binding_id": "binding-1" },
      { "kontor.summary.source": "a", "kontor.workspace_id": "wks_1" },
    ];
    for (const labels of marked) expect(claimsWorkflow(labels)).toBe(true);
    const unmarked: (Record<string, string> | null | undefined)[] = [null, undefined, {}, { workflow: "free" }, { orchestration: "none" }, { "kontor.summary.source": "a", "kontor.summary.job": "j" }, { pr: "310", epic: "ASMA-1" }];
    for (const labels of unmarked) expect(claimsWorkflow(labels)).toBe(false);
  });
});

describe("epicProjectKey", () => {
  it("names the epic of an ESW project only for a confirmed key", () => {
    expect(epicProjectKey("ESW • ASMA-8539")).toBe("ASMA-8539");
    expect(epicProjectKey(" esw  •  ASMA-1 ")).toBe("ASMA-1");
    for (const name of [null, undefined, "", "DESK • ADAM", "ESW • asma-8539", "ESW • ASMA-0", "ESW • ASMA-8539-slug", "ESW • ASMA-1 • X", "asma-rs-kontor", "ESW", "TICKETS • ASMA-8507", "DESK • ASMA-8507"])
      expect(epicProjectKey(name)).toBeNull();
  });
});
