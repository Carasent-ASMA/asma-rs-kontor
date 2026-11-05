import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ View: "View", Platform: { OS: "web" } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { PULSE_SCALE_MAX, pulseScaleEasing, pulseScaleInterpolation } from "../client/composer-pulse";

describe("composer pulse breathing", () => {
  it("caps the breathing scale at 1.12 over the whole loop (review-b S3 / D2)", () => {
    // The shared clock runs the cosine easing 0 -> 1 -> 0 per loop; the scale is 1 + (max - 1) * eased.
    // A looser cap (a seeded 1.5 mutant) must fail here before any browser runs.
    expect(PULSE_SCALE_MAX).toBeLessThanOrEqual(1.12);
    expect(pulseScaleInterpolation.outputRange).toEqual([1, PULSE_SCALE_MAX]);
    let peak = 0;
    for (let step = 0; step <= 2700; step += 5) {
      const eased = pulseScaleEasing(step / 2700);
      const scale = 1 + (PULSE_SCALE_MAX - 1) * eased;
      peak = Math.max(peak, scale);
    }
    expect(peak).toBeCloseTo(PULSE_SCALE_MAX, 9);
    expect(peak).toBeLessThanOrEqual(1.12);
  });
});
