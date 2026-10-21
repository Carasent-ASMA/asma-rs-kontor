import { describe, expect, it } from "vitest";
import { hasKontorLabels, isKontorSeat, seatBinding } from "../server/labels";

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
