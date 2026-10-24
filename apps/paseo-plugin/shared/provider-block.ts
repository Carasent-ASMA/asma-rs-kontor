import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";
import { availabilitySchema, freshAvailability } from "./session-lifecycle";

// The lifecycle classifier, split by kind without changing what it matches: QUOTA or AUTH matches exactly what the
// former single pattern matched.
const QUOTA = /quota|usage.?limit|rate.?limit|exhaust|billing|credit/i;
const AUTH = /unauthori[sz]ed|authenticat|\b401\b|\b403\b|api.?key|credential|token.expired|permission.denied/i;
// An HTTP IMF-fixdate, always GMT.
const IMF_DATE = /^(?:mon|tue|wed|thu|fri|sat|sun), \d{2} (?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec) \d{4} \d{2}:\d{2}:\d{2} gmt$/i;
const SYSTEM_ERROR = /^\s*\[System Error\]/;
/** Timeline items one recovery check reads. A transport bound: a refusal further back cannot be confirmed (G7). */
export const RECOVERY_TAIL_ITEMS = 50;

export type RefusalAgent = { provider: string; model?: string | null; runtimeInfo?: { model?: string | null } | null; status: string;
  activeTurn?: unknown; lastError?: string | null; providerUnavailable?: boolean };
const id = z.string().min(1).max(200);
export const refusalObservationSchema = z.object({
  /** `unknown`: unresolved refusal memory: a read with no recorded outcome, a record that cannot be used, or a native
   *  error with no clean read stored. Like `unavailable`, it binds only by time. */
  kind: z.enum(["quota", "auth", "unavailable", "unknown"]),
  /** From the refusal text only, else "unknown"; never a host timestamp. */
  failureAt: z.string(),
  /** Host time after the exact-ID read that saw the refusal returned: recovery counts only turns after it. Not a failure
   *  time and not provider health. */
  observedAt: z.string(),
  route: z.object({ provider: z.string(), model: z.string().nullable() }),
  message: z.string().nullable(),
});
export type RefusalObservation = z.infer<typeof refusalObservationSchema>;
export const providerRefusalSchema = refusalObservationSchema.extend({
  read: z.enum(["fresh", "stale", "unreadable"]),
  state: z.enum(["current", "history"]),
  recovery: z.enum(["confirmed", "not-confirmed"]),
});
export type ProviderRefusal = z.infer<typeof providerRefusalSchema>;
export const providerRefusalReadSchema = z.object({ availability: availabilitySchema, refusal: providerRefusalSchema.nullable() });
export type ProviderRefusalRead = z.infer<typeof providerRefusalReadSchema>;
/** Exact-ID read only: no send, archive or create. A session never inherits another session's refusal. */
export const readProviderRefusal = defineRpc({ name: "sessions.provider-refusal", input: z.object({ agentId: id }).strict(), output: providerRefusalReadSchema });

export function refusalKind(agent: Pick<RefusalAgent, "lastError" | "providerUnavailable">): RefusalObservation["kind"] | null {
  const text = agent.lastError ?? "";
  return QUOTA.test(text) ? "quota" : AUTH.test(text) ? "auth" : agent.providerUnavailable ? "unavailable" : null;
}
/** The provider's own response date: `date` in the `responseHeaders` of the outer serialized error envelope only. A
 *  nested, body or reset date, or text that is not one JSON envelope, is never a failure time. */
export function failureAt(text: string | null | undefined): string {
  const start = text?.indexOf("{") ?? -1;
  let headers: unknown = null;
  try { if (start >= 0) headers = (JSON.parse(text!.slice(start)) as { responseHeaders?: unknown } | null)?.responseHeaders; } catch { /* not one envelope: unknown */ }
  const date = headers && typeof headers === "object" ? Object.entries(headers).find(([key]) => key.toLowerCase() === "date")?.[1] : undefined;
  const time = typeof date === "string" && IMF_DATE.test(date) ? Date.parse(date) : NaN;
  return Number.isFinite(time) ? new Date(time).toISOString() : "unknown";
}
export function observeRefusal(agent: RefusalAgent, observedAt: string): RefusalObservation | null {
  const kind = refusalKind(agent);
  return kind && { kind, failureAt: failureAt(agent.lastError), observedAt,
    route: { provider: agent.provider, model: agent.runtimeInfo?.model ?? agent.model ?? null }, message: agent.lastError ?? null };
}
/** Current only when this read saw the refusal on a native error. Unseen or unreadable is history, never cleared. */
export function refusalState(observation: RefusalObservation, read: { seen: boolean; readable: boolean; error: boolean; recovered: boolean }): ProviderRefusal {
  return { ...observation, read: !read.readable ? "unreadable" : read.seen ? "fresh" : "stale",
    state: read.seen && read.error ? "current" : "history", recovery: !read.seen && read.recovered ? "confirmed" : "not-confirmed" };
}
/** A later read of one result: expiry, a negative age or a changed revision makes it stale history. Recovery never changes. */
export function refusalAsOf(result: ProviderRefusalRead, entry: { id: string; updatedAt: string }, now = Date.now()): ProviderRefusal | null {
  const refusal = result.refusal;
  if (!refusal || refusal.read !== "fresh" || freshAvailability({ ...entry, availability: result.availability }, now)) return refusal;
  return { ...refusal, read: "stale", state: "history" };
}
/** The exact read half of positive recovery: idle, so the last turn ended without a failure. Closed, running or a
 *  cleared lastError alone is not enough. */
export function quietAfterRefusal(agent: RefusalAgent): boolean {
  return agent.status === "idle" && !agent.activeTurn && !agent.lastError;
}
type Item = { type: string; text?: unknown; message?: unknown; level?: unknown };
const itemText = (item: Item) => typeof item.text === "string" ? item.text : typeof item.message === "string" ? item.message : "";
const failed = (item: Item) => item.type === "error" || (item.type === "notification" && item.level === "error") || (item.type === "assistant_message" && SYSTEM_ERROR.test(itemText(item)));
/** The timeline half (G7), bound to one stored observation by host time. Before `observedAt`, the last failure is a
 *  refusal of the stored kind with no turn started after it (the turn whose lastError was observed); an unavailable
 *  flag or an unknown kind names no turn, so only its time binds it. After `observedAt`, a new turn got a provider
 *  answer and no failure followed. An entry without a host timestamp, or out of order, proves nothing. */
export function recoveredAfterRefusal(entries: readonly unknown[], { kind, observedAt }: Pick<RefusalObservation, "kind" | "observedAt">): boolean {
  const seen = Date.parse(observedAt), items: (Item & { at: number })[] = [];
  if (!Number.isFinite(seen)) return false;
  for (const entry of entries) {
    const value = entry as { item?: unknown; timestamp?: unknown } | null, item = value?.item as Item | null | undefined;
    const at = typeof value?.timestamp === "string" ? Date.parse(value.timestamp) : NaN;
    if (!item || typeof item.type !== "string" || !Number.isFinite(at) || at < (items.at(-1)?.at ?? at)) return false;
    items.push({ ...item, at });
  }
  const split = items.findIndex((item) => item.at > seen), before = split < 0 ? items : items.slice(0, split), after = split < 0 ? [] : items.slice(split);
  const refusal = before.findLastIndex(failed);
  if ((kind === "quota" || kind === "auth") && (refusal < 0 || refusalKind({ lastError: itemText(before[refusal]!) }) !== kind || before.slice(refusal).some((item) => item.type === "user_message"))) return false;
  const lastFailure = after.findLastIndex(failed), turn = after.findIndex((item, index) => index > lastFailure && item.type === "user_message");
  return turn >= 0 && after.some((item, index) => index > turn && item.type === "assistant_message" && !failed(item));
}
