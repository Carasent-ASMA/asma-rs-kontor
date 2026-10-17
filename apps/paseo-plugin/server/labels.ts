export type AgentLabels = Record<string, string> | null | undefined;

export const KONTOR_LABEL_PREFIX = "kontor.";
export const PASEO_DIRECT_ORCHESTRATION = "paseo-direct";
export const KONTOR_SEAT_LABEL = "kontor.seat";
export const KONTOR_BINDING_GENERATION_LABEL = "kontor.binding_generation";

const SEAT_LABEL_KEYS = [
  KONTOR_SEAT_LABEL,
  "kontor.seat_binding_id",
  "kontor.role_slot_id",
  "kontor.agent-run",
] as const;

export interface SeatBinding {
  readonly seat: string | null;
  readonly generation: string | null;
}

/**
 * Whether the labels mark a Kontor-managed seat.
 *
 * `kontor.*` is the runtime-owned namespace. Paseo-direct delivery seats carry
 * `orchestration=paseo-direct` instead, so both mark a seat the workspace
 * guard owns. An unlabelled session is interactive and stays untouched.
 */
export function isKontorSeat(labels: AgentLabels): boolean {
  if (!labels) {
    return false;
  }
  if (Object.keys(labels).some((key) => key.startsWith(KONTOR_LABEL_PREFIX))) {
    return true;
  }
  return labels["orchestration"] === PASEO_DIRECT_ORCHESTRATION;
}

/**
 * The seat identity carried by the labels.
 *
 * `kontor.seat` is the current key. The older keys stay readable because live
 * sessions still carry them; a labelled seat without any of them stays
 * unresolved so the pull path can settle it.
 */
export function seatBinding(labels: AgentLabels): SeatBinding {
  if (!labels) {
    return { seat: null, generation: null };
  }
  const seat = SEAT_LABEL_KEYS.map((key) => labels[key])
    .map((value) => value?.trim())
    .find((value) => value !== undefined && value.length > 0);
  const generation = labels[KONTOR_BINDING_GENERATION_LABEL]?.trim();
  return {
    seat: seat ?? null,
    generation: generation && generation.length > 0 ? generation : null,
  };
}
