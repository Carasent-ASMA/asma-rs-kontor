export type AgentLabels = Record<string, string> | null | undefined;

export const KONTOR_LABEL_PREFIX = "kontor.";
export const PASEO_DIRECT_ORCHESTRATION = "paseo-direct";

/**
 * The labels a real Kontor launch writes for the seat identity.
 *
 * `kontor.seat_binding_id` is the persistent hosted seat. A delivery seat
 * carries `kontor.role_slot_id`; `kontor.agent-run` is the run correlation and
 * stays readable as the last fallback. There is no `kontor.seat` label in the
 * writer (`crates/kontor-runtime-paseo/src/wire.rs`).
 */
const SEAT_LABEL_KEYS = [
  "kontor.seat_binding_id",
  "kontor.role_slot_id",
  "kontor.agent-run",
] as const;

export interface SeatBinding {
  readonly seat: string | null;
}

/**
 * Whether the labels carry a Kontor-owned label (`kontor.*`).
 *
 * Only these labels name a native project and canonical worktree the guard can
 * enforce. A Paseo-direct delivery seat carries `orchestration=paseo-direct`
 * alone, is still guarded, and has no binding of its own.
 */
export function hasKontorLabels(labels: AgentLabels): boolean {
  if (!labels) {
    return false;
  }
  return Object.keys(labels).some((key) => key.startsWith(KONTOR_LABEL_PREFIX));
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
  if (hasKontorLabels(labels)) {
    return true;
  }
  return labels["orchestration"] === PASEO_DIRECT_ORCHESTRATION;
}

/**
 * The seat identity carried by the labels.
 *
 * Only keys the Kontor writer actually plants are read. A labelled seat without
 * any of them stays unresolved so the pull path can settle it.
 */
export function seatBinding(labels: AgentLabels): SeatBinding {
  if (!labels) {
    return { seat: null };
  }
  const seat = SEAT_LABEL_KEYS.map((key) => labels[key])
    .map((value) => value?.trim())
    .find((value) => value !== undefined && value.length > 0);
  return { seat: seat ?? null };
}
