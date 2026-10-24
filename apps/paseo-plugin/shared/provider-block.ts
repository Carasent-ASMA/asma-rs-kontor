import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";
import { availabilitySchema, freshAvailability } from "./session-lifecycle";

// The lifecycle classifier, split by kind without changing what it matches: QUOTA or AUTH matches exactly what the
// former single pattern matched.
const QUOTA = /quota|usage.?limit|rate.?limit|exhaust|billing|credit/i;
const AUTH = /unauthori[sz]ed|authenticat|\b401\b|\b403\b|api.?key|credential|token.expired|permission.denied/i;
// The provider's own response date: an HTTP IMF-fixdate (always GMT) under a `date` header key, serialized or raw.
const RESPONSE_DATE = /(?:\\?"date\\?"\s*:\s*\\?"|^date:\s*)((?:mon|tue|wed|thu|fri|sat|sun), \d{2} (?:jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec) \d{4} \d{2}:\d{2}:\d{2} gmt)/im;
const SYSTEM_ERROR = /^\s*\[System Error\]/;
/** Timeline items one recovery check reads. A transport bound: a refusal further back cannot be confirmed (G7). */
export const RECOVERY_TAIL_ITEMS = 50;

export type RefusalAgent = { provider: string; model?: string | null; runtimeInfo?: { model?: string | null } | null; status: string;
  activeTurn?: unknown; lastError?: string | null; providerUnavailable?: boolean };
const id = z.string().min(1).max(200);
export const refusalObservationSchema = z.object({
  kind: z.enum(["quota", "auth", "unavailable"]),
  /** From the refusal text only, else "unknown"; never a host timestamp. */
  failureAt: z.string(),
  /** Host time of the exact-ID read that saw the refusal; not a failure time and not provider health. */
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
export function failureAt(text: string | null | undefined): string {
  const match = RESPONSE_DATE.exec(text ?? ""), time = match ? Date.parse(match[1]!) : NaN;
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
/** The exact read half of positive recovery: the last turn ended without a failure. A cleared lastError alone is not enough. */
export function quietAfterRefusal(agent: RefusalAgent): boolean {
  return agent.status !== "error" && agent.status !== "running" && agent.status !== "initializing" && !agent.activeTurn && !agent.lastError;
}
type Item = { type: string; text?: unknown; message?: unknown; level?: unknown };
const itemText = (item: Item) => typeof item.text === "string" ? item.text : typeof item.message === "string" ? item.message : "";
const failed = (item: Item) => item.type === "error" || (item.type === "notification" && item.level === "error") || (item.type === "assistant_message" && SYSTEM_ERROR.test(itemText(item)));
/** The timeline half (G7): the tail shows a refusal, and after the last failure a new turn whose provider answer is not a failure. */
export function recoveredAfterRefusal(entries: readonly unknown[]): boolean {
  const items = entries.flatMap((entry) => {
    const item = (entry as { item?: unknown } | null)?.item as Item | undefined;
    return item && typeof item.type === "string" ? [item] : [];
  });
  if (!items.some((item) => failed(item) && refusalKind({ lastError: itemText(item) }))) return false;
  const lastFailure = items.findLastIndex(failed), turn = items.findIndex((item, index) => index > lastFailure && item.type === "user_message");
  return turn >= 0 && items.some((item, index) => index > turn && item.type === "assistant_message" && !failed(item));
}
