import { describe, expect, it } from "vitest";
import { PASEO_HEADER_ROWS_HELP, PASEO_HEADER_ROWS_LABEL, PASEO_HEADER_ROWS_START_OPEN } from "../src/host/sidebar-chrome";

describe("simulated Paseo sidebar header", () => {
  it("starts collapsed, as the requested Paseo feature would", () => {
    expect(PASEO_HEADER_ROWS_START_OPEN).toBe(false);
  });

  it("marks the header as a simulation of a requested Paseo feature", () => {
    expect(PASEO_HEADER_ROWS_LABEL).toContain("simulated");
    expect(PASEO_HEADER_ROWS_HELP).toContain("requested feature");
    expect(PASEO_HEADER_ROWS_HELP).toContain("starts collapsed");
  });
});
