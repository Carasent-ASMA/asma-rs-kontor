import { describe, expect, it } from "vitest";
import { UNSUPPORTED } from "../shared/session-navigator";
import { TAKEOVER_LIMITS } from "../server/session-lifecycle";
import { asTakeoverClient, paseoWakeScheduleReader, readInstalledNavigatorCapabilities } from "../server/session-navigator";

describe("installed navigator capabilities", () => {
  const paseo = { agents: {}, workspaces: {} } as never;

  it("qualifies takeover on the installed creator/verifier path and names wake API gap when schedules are absent", () => {
    const caps = readInstalledNavigatorCapabilities(paseo);
    expect(caps.takeover).toEqual({ supported: true, limits: expect.arrayContaining([...TAKEOVER_LIMITS, expect.stringMatching(/schedules\.forAgent/)]) });
    expect(caps.takeoverRoute).toEqual(UNSUPPORTED.takeoverRoute);
    expect(caps.summaries).toEqual(UNSUPPORTED.summaries);
    expect(caps.takeoverBulk).toEqual(UNSUPPORTED.takeoverBulk);
  });

  it("does not advertise model override for takeover on the installed host", () => {
    expect(readInstalledNavigatorCapabilities(paseo).takeoverRoute).toEqual(UNSUPPORTED.takeoverRoute);
    expect(readInstalledNavigatorCapabilities(paseo).takeover.supported).toBe(true);
  });

  it("omits wake API gap when schedules.forAgent is present on the SDK", () => {
    const withSchedules = { agents: {}, workspaces: {}, schedules: { forAgent: async () => ({ entries: [] }) } } as never;
    const caps = readInstalledNavigatorCapabilities(withSchedules);
    expect(caps.takeover).toEqual({ supported: true, limits: TAKEOVER_LIMITS });
    expect(paseoWakeScheduleReader(withSchedules)).toBeDefined();
    expect(asTakeoverClient(withSchedules).schedules).toBeDefined();
  });
});
