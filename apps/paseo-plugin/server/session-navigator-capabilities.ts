import { UNSUPPORTED, type NavigatorCapabilities } from "../shared/session-navigator";
import { SUMMARY_LIMITS } from "./session-summaries";

/** Installed summary store qualifies summaries; other navigator owners stay explicit. */
export function readInstalledNavigatorCapabilities(): NavigatorCapabilities {
  return {
    usageTotals: UNSUPPORTED.usageTotals,
    summaries: { supported: true, limits: SUMMARY_LIMITS },
    draftAttach: UNSUPPORTED.draftAttach,
    takeover: UNSUPPORTED.takeover,
    takeoverBulk: UNSUPPORTED.takeoverBulk,
    takeoverRoute: UNSUPPORTED.takeoverRoute,
  };
}
