import { useEffect, useState } from "react";
import { Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { applyTakeover, checkTakeover, reviewTakeover, type TakeoverOutcome, type TakeoverReview } from "../shared/session-lifecycle";
import type { PickedModel } from "./model-picker";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import type { Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { readableInk } from "./session-status-ink";
import { modelAlias } from "./session-usage";

/** One operation's route: null is Auto, each seat's own chain; a picked model applies to these seats only and changes no default. */
export type TakeoverRoute = Pick<PickedModel, "provider" | "model" | "effort" | "featureValues"> | null;

/** What every takeover does, said wherever one starts; the host's stated limits follow it there. */
export const TAKEOVER_EFFECTS = "Saves a compact handoff, archives this session, then starts a new one that continues the task.";

/**
 * - `working`: being read, reviewed or applied now; `queued` waits its turn.
 * - `done`: a verified successor continues and the previous session is archived. Never run again.
 * - `kept`: nothing was archived; the original occupant keeps the seat.
 * - `review`: ready, but the review states a limit nobody was shown before; nothing runs until the operator confirms it.
 * - `recovery`: the previous session is archived and no successor holds the seat yet.
 * - `unknown`: an effect may have happened; only Check reads it before anything else runs.
 */
export type SeatState = "queued" | "working" | "done" | "kept" | "review" | "recovery" | "unknown";
export type SeatResult = {
  readonly id: string; readonly title: string; readonly state: SeatState; readonly line: string; readonly detail: string | null;
  /** The successor's model alias once a review named it. */
  readonly model: string | null;
  readonly reviewId: string | null; readonly next: TakeoverOutcome["next"];
  /** A `review` seat's exact review: the limits it states beyond what was shown, and what confirming it sends back unchanged. */
  readonly held?: { readonly revision: number; readonly digest: string; readonly limits: readonly string[] } | null;
};
export type TakeoverRun = { readonly route: TakeoverRoute; readonly seats: readonly SeatResult[]; readonly running: boolean };

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);
const KEPT = "Not started · Original kept";
const UNCONFIRMED = "Not confirmed · Check before retrying";
const SAME = "Retry remaining skips it: asking again gets the same answer.";
const said = (message: string, why: string) => `${message} ${why}`;

/** Whether Retry remaining runs this seat again: never a continued seat, one held for review, or one nothing here can help. */
export const retryable = (seat: SeatResult) => seat.state !== "done" && seat.state !== "working" && seat.state !== "review" && seat.next !== "none";
/** An effect that happened and is not finished: kept beyond its results sheet until Check or Finish settles it. */
export const unfinished = (seat: SeatResult) => seat.state === "recovery" || seat.state === "unknown";
/** What only this seat's own step resolves: an unfinished effect, or a held review its operator has not confirmed or declined. */
export const unresolved = (seat: SeatResult) => unfinished(seat) || (seat.state === "review" && !!seat.held);

/**
 * A review that is not ready to confirm: nothing was sent, so the original keeps the seat. Only an unread answer may be
 * retried; a refusal, a chain with no available model and a chain only a profile choice could settle answer the same.
 */
export function reviewResult(seat: SeatResult, review: TakeoverReview): SeatResult {
  const base = { ...seat, state: "kept" as const, model: review.successor ? modelAlias(review.successor.model) : null, reviewId: review.reviewId };
  if (review.state === "unknown") return { ...base, line: KEPT, detail: review.message, next: "review" };
  if (review.state === "choose") return { ...base, line: KEPT, detail: said(review.message, `Choose a replacement model from the Take over model menu.`), next: "none" };
  return { ...base, line: review.state === "blocked" ? "No available model · Original kept" : KEPT, detail: said(review.message, SAME), next: "none" };
}

/** The server's outcome for one seat, in the result's own words; the message is kept as its detail. */
export function outcomeResult(seat: SeatResult, outcome: TakeoverOutcome): SeatResult {
  const base = { ...seat, detail: outcome.message, reviewId: outcome.reviewId, next: outcome.next };
  if (outcome.state === "succeeded") return { ...base, state: "done", line: "Continuing · Previous session archived" };
  // Before creation no successor exists; at verify one may, but nothing confirmed it holds the seat.
  if (outcome.state === "partial") return { ...base, state: "recovery", line: `Previous session archived · ${outcome.phase === "create" || outcome.phase === "archive" ? "No new session yet" : "New session not verified"} · Recovery pending` };
  if (outcome.state === "uncertain") return { ...base, state: "unknown", line: UNCONFIRMED, next: "check" };
  return { ...base, state: "kept", line: KEPT, detail: outcome.next === "none" ? said(outcome.message, SAME) : outcome.message };
}

/** "2 continued · 1 not started", in the order the states matter. */
export function takeoverSummary(seats: readonly SeatResult[]): string {
  const count = (state: SeatState) => seats.filter((seat) => seat.state === state).length;
  return ([["done", "continued"], ["recovery", "recovery pending"], ["unknown", "not confirmed"], ["review", "review needed"], ["kept", "not started"]] as const)
    .map(([state, label]) => count(state) ? `${count(state)} ${label}` : null)
    .concat(count("working") + count("queued") ? [`${count("working") + count("queued")} in progress`] : []).filter(Boolean).join(" · ");
}

/** Each host's latest run, outside any sheet or surface: closing results or leaving a surface never loses an unfinished seat. */
const runs = new Map<string, TakeoverRun>(), tokens = new Map<string, number>(), listeners = new Set<() => void>();
const publish = (hostId: string, run: TakeoverRun | null) => { if (run) runs.set(hostId, run); else runs.delete(hostId); for (const listener of listeners) listener(); };

/**
 * Why no new takeover may start on this host now, read from the host's run itself, so no surface, second controller
 * or stale callback can replace seats that only their own Check, Finish, Confirm or Decline resolves.
 */
export function takeoverBusy(hostId: string): string | null {
  const run = runs.get(hostId);
  return run?.running ? "A takeover is running; wait for its results." : run?.seats.some(unresolved) ? "Finish or check the unfinished takeover first." : null;
}

/**
 * Take over seats one at a time, each with its own review, confirmation by the digest the server issued, and result.
 * A ready review is confirmed at once only when every limit it states is in `disclosed`, which the surface that started
 * it showed; otherwise the seat waits as Review needed until its operator confirms that exact review. A seat with an
 * unconfirmed effect is checked by its review before anything else; Retry runs only unfinished seats. The run belongs
 * to its host, not to a sheet or surface, so a reply that lands after either closed is still recorded. Every action
 * reads the host's current run, never the one its render saw, and a new start is refused while `takeoverBusy` says why.
 */
export function useTakeoverRun(hostId: string, disclosed: readonly string[], onFinish?: (run: TakeoverRun) => void) {
  const review = useRpc(reviewTakeover), apply = useRpc(applyTakeover), check = useRpc(checkTakeover);
  const [, redraw] = useState(0);
  useEffect(() => { const listener = () => redraw((count) => count + 1); listeners.add(listener); return () => { listeners.delete(listener); }; }, []);
  const run = runs.get(hostId) ?? null;
  const confirm = async (seat: SeatResult, digest: { reviewId: string; revision: number; digest: string }): Promise<SeatResult> => {
    try { return outcomeResult(seat, await apply(digest)); }
    catch (error) {
      return { ...seat, state: "unknown", line: UNCONFIRMED, next: "check", held: null, detail: `No reply after confirming (${failure(error)}). The takeover may have started; its exact result is read before anything else runs.` };
    }
  };
  const one = async (seat: SeatResult, route: TakeoverRoute): Promise<SeatResult> => {
    if (seat.next === "check" && seat.reviewId) {
      try { return outcomeResult(seat, await check({ reviewId: seat.reviewId })); }
      catch (error) { return { ...seat, state: "unknown", line: UNCONFIRMED, detail: `Kontor could not read this takeover's result (${failure(error)}).` }; }
    }
    // The operator confirms the exact review they read; a stale one is refused by its server and needs a fresh review.
    if (seat.state === "review" && seat.held && seat.reviewId) return confirm({ ...seat, held: null }, { reviewId: seat.reviewId, revision: seat.held.revision, digest: seat.held.digest });
    let answer: TakeoverReview;
    try { answer = await review({ agentId: seat.id, bindingKey: null, ...(route ? { route } : {}) }); }
    catch (error) { return { ...seat, state: "kept", line: KEPT, detail: `Kontor could not review this takeover (${failure(error)}). Nothing was sent.`, next: "review" }; }
    if (answer.state !== "ready" || !answer.digest) return reviewResult(seat, answer);
    const reviewed = { ...seat, model: answer.successor ? modelAlias(answer.successor.model) : null, reviewId: answer.reviewId };
    const unseen = answer.limits.filter((limit) => !disclosed.includes(limit));
    if (unseen.length) return { ...reviewed, state: "review", line: "Review needed · Original kept", detail: answer.message, next: "review", held: { revision: answer.revision, digest: answer.digest, limits: unseen } };
    return confirm(reviewed, { reviewId: answer.reviewId, revision: answer.revision, digest: answer.digest });
  };
  /** Runs `seats` in order: every retryable one, or with `only`, just those, including a held review being confirmed. */
  const go = async (seats: readonly SeatResult[], route: TakeoverRoute, only?: ReadonlySet<string>) => {
    const mine = (tokens.get(hostId) ?? 0) + 1;
    tokens.set(hostId, mine);
    let current = seats;
    const put = (next: SeatResult, running = true) => { current = current.map((seat) => seat.id === next.id ? next : seat); if (tokens.get(hostId) === mine) publish(hostId, { route, seats: current, running }); };
    publish(hostId, { route, seats: current, running: true });
    for (const seat of seats) {
      if (only ? !only.has(seat.id) || !(retryable(seat) || (seat.state === "review" && seat.held)) : !retryable(seat)) continue;
      put({ ...seat, state: "working", line: "Working…" });
      put(await one(seat, route));
    }
    if (tokens.get(hostId) !== mine) return;
    const done = { route, seats: current, running: false };
    publish(hostId, done);
    onFinish?.(done);
  };
  const idle = () => { const now = runs.get(hostId); return now && !now.running ? now : null; };
  return {
    run, busy: takeoverBusy(hostId),
    /** Starts a new run, or answers why not without touching the host's run; the check and the start share one turn. */
    start: async (entries: readonly Pick<DirectoryEntry, "id" | "title">[], route: TakeoverRoute): Promise<string | null> => {
      const busy = takeoverBusy(hostId);
      if (busy) return busy;
      await go(entries.map((entry) => ({ id: entry.id, title: entry.title, state: "queued", line: "Queued", detail: null, model: null, reviewId: null, next: "review" })), route);
      return null;
    },
    retry: () => { const now = idle(); if (now) void go(now.seats, now.route); },
    /** Check, Finish or Confirm one seat of this host's run, leaving every other seat as it is. */
    act: (id: string) => { const now = idle(); if (now) void go(now.seats, now.route, new Set([id])); },
    /** The operator declines a held review: nothing was sent, the original keeps the seat, and the review stays named until Close. */
    decline: (id: string) => {
      const now = idle();
      if (now?.seats.some((seat) => seat.id === id && seat.state === "review" && seat.held)) publish(hostId, { ...now, seats: now.seats.map((seat) => seat.id !== id ? seat
        : { ...seat, state: "kept", line: "Declined · Original kept", held: null, next: "none", detail: `${seat.detail ? `${seat.detail} ` : ""}You declined this review, so nothing was sent. Take over this seat again for a fresh review.` }) });
    },
    /** Forgets finished results; a running takeover is never dropped, and an unresolved seat stays until its own step settles it. */
    clear: () => { const now = idle(); if (!now) return; const left = now.seats.filter(unresolved); publish(hostId, left.length ? { ...now, seats: left } : null); },
  };
}

/** What one seat's own button does next, or null when nothing here can. */
export function seatAction(seat: SeatResult): { readonly kind: "check" | "finish" | "confirm"; readonly label: string } | null {
  if (seat.state === "unknown" && seat.reviewId) return { kind: "check", label: "Check takeover" };
  if (seat.state === "recovery" && seat.next === "review") return { kind: "finish", label: "Finish takeover" };
  if (seat.state === "review" && seat.held) return { kind: "confirm", label: "Confirm takeover" };
  return null;
}

const ICONS: Record<SeatState, [string, "statusSuccess" | "statusWarning" | "statusDanger" | "foregroundMuted"]> = {
  queued: ["Circle", "foregroundMuted"], working: ["Loader", "foregroundMuted"], done: ["CircleCheck", "statusSuccess"],
  kept: ["CircleMinus", "statusWarning"], review: ["ShieldAlert", "statusWarning"], recovery: ["TriangleAlert", "statusDanger"], unknown: ["CircleHelp", "statusWarning"],
};

/**
 * Board D3: one result per seat, Close LEFT and Retry remaining RIGHT; continued seats are never restarted. A seat that
 * needs its own step says so with its button: Check, Finish, or Decline and Confirm under the limits it lists.
 */
export function TakeoverResults({ run, colors, onClose, onRetry, onSeat, onDecline }: { run: TakeoverRun; colors: Colors; onClose: () => void; onRetry: () => void; onSeat: (id: string) => void; onDecline: (id: string) => void }) {
  const remaining = run.seats.filter(retryable).length, muted = { color: colors.foregroundMuted, fontSize: 11, lineHeight: 15 } as const;
  const button = (testID: string, label: string, spoken: string, onPress: () => void) => <Pressable key={testID} testID={testID} accessibilityRole="button" accessibilityLabel={spoken} disabled={run.running}
    aria-disabled={run.running || undefined} accessibilityState={{ disabled: run.running }} onPress={onPress} style={{ minHeight: 44, justifyContent: "center", opacity: run.running ? 0.5 : 1 }}>
    <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 13, fontWeight: "600" }}>{label}</Text>
  </Pressable>;
  return <OperationSheet testID="takeover-results" title="Takeover results" colors={colors} onClose={onClose}
    footer={<SheetFooter colors={colors} note="Completed seats are not restarted." secondary={{ testID: "takeover-results-close", label: "Close", onPress: onClose }}
      primary={{ testID: "takeover-results-retry", label: `Retry remaining ${remaining}`, disabled: run.running || !remaining, onPress: onRetry,
        hint: run.running ? "Wait until every seat has a result." : remaining ? null : "Nothing is left to retry." }} />}>
    <Text testID="takeover-results-summary" role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{takeoverSummary(run.seats)}</Text>
    <View role="list" style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 8, overflow: "hidden" }}>
      {run.seats.map((seat, index) => {
        const [icon, ink] = ICONS[seat.state], action = seatAction(seat);
        return <View key={seat.id} role="listitem" testID={`takeover-result-${seat.id}`} accessibilityLabel={[seat.title, seat.model, seat.line, seat.detail].filter(Boolean).join(". ")}
          style={{ flexDirection: "row", gap: 10, padding: 10, borderTopWidth: index ? 1 : 0, borderColor: colors.border }}>
          <Icon name={icon} size={18} color={colors[ink]} />
          <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
            <View style={{ flexDirection: "row", gap: 6 }}>
              <Text numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{seat.title}</Text>
              {seat.model ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{seat.model}</Text> : null}
            </View>
            <Text style={{ color: colors.foreground, fontSize: 12 }}>{seat.line}</Text>
            {/* The whole reason, wrapped: why the original was kept and why Retry leaves the seat out are read here, not only spoken. */}
            {seat.detail ? <Text style={muted}>{seat.detail}</Text> : null}
            {seat.held ? <View testID={`takeover-limits-${seat.id}`} role="list" accessibilityLabel="Limits to review" style={{ gap: 2 }}>
              {seat.held.limits.map((limit) => <Text key={limit} role="listitem" style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>{`• ${limit}`}</Text>)}
            </View> : null}
            {action ? <View style={{ flexDirection: "row", gap: 16 }}>
              {/* A held review is declined LEFT of its Confirm; nothing is sent either way until one is pressed. */}
              {action.kind === "confirm" ? button(`takeover-decline-${seat.id}`, "Decline", `Decline takeover: ${seat.title}`, () => onDecline(seat.id)) : null}
              {button(`takeover-${action.kind}-${seat.id}`, action.label, `${action.label}: ${seat.title}`, () => onSeat(seat.id))}
            </View> : null}
          </View>
        </View>;
      })}
    </View>
    <Text style={muted}>{run.route ? `${modelAlias(run.route.model)} was chosen for this takeover only. Defaults stay unchanged.` : "Auto used each seat's chain."}</Text>
  </OperationSheet>;
}
