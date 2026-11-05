import { describe, expect, it, vi } from "vitest";
import type { Colors } from "../client/page-controls";

const rn = vi.hoisted(() => ({ OS: "web" }));
const stateInits = vi.hoisted(() => ({ seen: [] as unknown[] }));

vi.mock("react-native", () => ({
  View: "View",
  Platform: { get OS() { return rn.OS; } },
  Animated: {
    Value: class { setValue() {} interpolate() { return {}; } },
    View: "Animated.View",
    timing: () => ({ start: () => {}, stop: () => {} }),
    loop: () => ({ start: () => {}, stop: () => {} }),
  },
  AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
  AccessibilityInfo: { isReduceMotionEnabled: async () => false, addEventListener: () => ({ remove() {} }) },
  LayoutChangeEvent: undefined,
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("react", () => ({
  // Capture every useState INITIAL so the visibility default is pinned without a renderer.
  useState: (initial: unknown) => { stateInits.seen.push(initial); return [undefined, () => {}]; },
  useRef: () => ({ current: null }),
  useCallback: (fn: unknown) => fn,
  useMemo: () => undefined,
  useEffect: () => {},
  useSyncExternalStore: () => false,
}));
import { ComposerPulse } from "../client/composer-pulse";

const colors = {
  surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A",
  foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b",
} as Colors;

describe("composer pulse initial viewport state", () => {
  it("waits for the web viewport observer, and trusts the measured consumer gate on native", () => {
    // review-a F3 / review-b S4: the native branch has no IntersectionObserver, so the consumer's measured
    // visibility decides; on the web the carrier must WAIT for its observer — the "always in viewport"
    // mutant (initial true on every platform) fails the web assertion here.
    rn.OS = "web";
    stateInits.seen = [];
    ComposerPulse({ colors, size: 22, name: "Building2", enabled: true });
    expect(stateInits.seen[0]).toBe(false); // layout visibility starts unknown
    expect(stateInits.seen[1]).toBe(false); // viewport starts unknown on the web
    rn.OS = "ios";
    stateInits.seen = [];
    ComposerPulse({ colors, size: 22, name: "Building2", enabled: true });
    expect(stateInits.seen[1]).toBe(true); // native: the consumer's measured gate decides
  });
});
