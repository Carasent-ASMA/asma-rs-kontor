/**
 * Paseo 0.17.4 always shows its sidebar header rows and gives a sidebar view no way to collapse them.
 * The prototype models the collapsible header the upstream request asks for, and starts it collapsed.
 */
export const PASEO_HEADER_ROWS_START_OPEN = false;

/** The visible label keeps the simulation and the requested Paseo feature unmistakable in every screenshot. */
export const PASEO_HEADER_ROWS_LABEL = "Paseo header · simulated";

export const PASEO_HEADER_ROWS_HELP =
  "Simulated Paseo sidebar header. Paseo cannot collapse its own header yet; this models the requested feature and starts collapsed.";
