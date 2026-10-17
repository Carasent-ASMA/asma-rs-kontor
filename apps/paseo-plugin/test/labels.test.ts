import { describe, expect, it } from "vitest";
import { isKontorSeat, seatBinding } from "../server/labels";

describe("isKontorSeat", () => {
  it("recognizes runtime-owned Kontor labels", () => {
    expect(isKontorSeat({ "kontor.seat": "seat-1" })).toBe(true);
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
  it("prefers the current seat label and reads the generation", () => {
    expect(
      seatBinding({
        "kontor.seat": "seat-1",
        "kontor.seat_binding_id": "binding-1",
        "kontor.binding_generation": "3",
      }),
    ).toEqual({ seat: "seat-1", generation: "3" });
  });

  it("reads the legacy keys when the current key is absent", () => {
    expect(seatBinding({ "kontor.seat_binding_id": "binding-1" })).toEqual({
      seat: "binding-1",
      generation: null,
    });
    expect(seatBinding({ "kontor.role_slot_id": "slot-1" })).toEqual({
      seat: "slot-1",
      generation: null,
    });
    expect(seatBinding({ "kontor.agent-run": "run-1" })).toEqual({
      seat: "run-1",
      generation: null,
    });
  });

  it("stays empty without a seat label", () => {
    expect(seatBinding({ "kontor.hosted_seat": "true" })).toEqual({
      seat: null,
      generation: null,
    });
  });
});
