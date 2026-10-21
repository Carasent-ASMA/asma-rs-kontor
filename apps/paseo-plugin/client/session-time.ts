import { useEffect, useState } from "react";
import type { DirectoryEntry } from "./session-directory";
import { sessionState, STATE_LABELS, type OrganizationState } from "./session-organization";

/**
 * Session times for rows and Details, each kept to what Paseo means by it
 * (agent-manager, agent-projections and persistence-hooks in Paseo 0.12.0, the same in 0.11.4):
 * - `updatedAt` moves on any change Paseo makes to a session: activity and prompts, and also
 *   a rename, labels, a mode or model change, a restore from archive, marking it unread and
 *   clearing attention. It says when the session last changed, not that it was active.
 * - `attentionTimestamp` is set when Paseo raises attention (a finished turn, an error, Mark as
 *   unread) and is kept, unchanged, until the attention is cleared.
 * - `activeTurnStartedAt` is when the running turn started; Paseo loses it on a restart.
 * - `lastUserMessageAt` is when the last prompt was submitted.
 * Paseo records no time for a change to idle, closed or starting, nor for a permission
 * request, so how long a session has been in those states stays unknown.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
/** Clock skew read as "just now" before a time counts as ahead of this device. */
const AHEAD_TOLERANCE = MINUTE;

/** Milliseconds since the epoch, or null for an absent or unreadable value. */
export function parseTime(value: string | null | undefined): number | null {
  if (typeof value !== "string" || value.trim() === "") return null;
  const time = Date.parse(value);
  return Number.isFinite(time) ? time : null;
}

/** The time a list shows ages against, and the zone it shows times in. */
export type Clock = { readonly now: number; readonly zone: string };

function deviceZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
  } catch {
    return "";
  }
}

type Format = (at: number) => string;
type Formats = {
  readonly zone: string;
  readonly time: Format;
  readonly day: Format;
  readonly dayYear: Format;
  readonly spokenDay: Format;
  readonly exact: Format;
  readonly calendarDay: Format;
};
let formats: Formats | null = null;

/** The device's own date order and month names; 24-hour times, as elsewhere in Kontor. */
function formatsIn(zone: string): Formats {
  if (formats?.zone === zone) return formats;
  const where = zone ? { timeZone: zone } : {};
  const make = (options: Intl.DateTimeFormatOptions, locale: string | string[] = []): Format => {
    let formatter: Intl.DateTimeFormat;
    try {
      formatter = new Intl.DateTimeFormat(locale, { ...where, ...options });
    } catch {
      // A zone the runtime cannot name: the device's own zone, still labelled as the device's.
      formatter = new Intl.DateTimeFormat(locale, options);
    }
    // A Date, not a number: test clocks that wrap Intl read a bare 0 as "now".
    return (at) => formatter.format(new Date(at));
  };
  formats = {
    zone,
    time: make({ hour: "2-digit", minute: "2-digit", hourCycle: "h23" }),
    day: make({ day: "numeric", month: "short" }),
    dayYear: make({ day: "numeric", month: "short", year: "numeric" }),
    spokenDay: make({ day: "numeric", month: "long", year: "numeric" }),
    exact: make({ weekday: "short", day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23", timeZoneName: "short" }),
    calendarDay: make({ year: "numeric", month: "2-digit", day: "2-digit" }, "en-CA"),
  };
  return formats;
}

const plural = (count: number, unit: string) => `${count} ${unit}${count === 1 ? "" : "s"}`;

/** "3 hours", rounded down to the largest whole unit. */
function span(milliseconds: number): string {
  if (milliseconds < HOUR) return plural(Math.max(1, Math.floor(milliseconds / MINUTE)), "minute");
  if (milliseconds < DAY) return plural(Math.floor(milliseconds / HOUR), "hour");
  return plural(Math.floor(milliseconds / DAY), "day");
}

/** How long before `now` the time was, in words, or how far ahead of this device's clock it is. */
export function ageInWords(at: number, now: number): string {
  const ago = now - at;
  if (ago < -AHEAD_TOLERANCE) return `ahead of this device's clock by ${span(-ago)}`;
  if (ago < MINUTE) return "just now";
  return `${span(ago)} ago`;
}

/** The exact time with its weekday, seconds and zone, for Details. */
export function exactTime(at: number, zone: string): string {
  return formatsIn(zone).exact(at);
}

/** What a row shows for a session's last change: compact for a phone, wide with its name, and spoken. */
export type RowTime = { readonly compact: string; readonly wide: string; readonly spoken: string };

/**
 * A row's Updated time. On a live list it is an age ("3h ago"), up to a week; past that, ahead
 * of this device's clock, or while the list is not live, it is the time itself. A ticking age
 * would claim a list that has stopped updating is current.
 */
export function updatedRowTime(updatedAt: string | null | undefined, clock: Clock, live: boolean): RowTime | null {
  const at = parseTime(updatedAt);
  if (at === null) return null;
  const ago = clock.now - at;
  if (live && ago >= -AHEAD_TOLERANCE && ago < 7 * DAY) {
    if (ago < MINUTE) return { compact: "just now", wide: "Updated just now", spoken: "Updated just now" };
    const short = ago < HOUR ? `${Math.floor(ago / MINUTE)}m` : ago < DAY ? `${Math.floor(ago / HOUR)}h` : `${Math.floor(ago / DAY)}d`;
    return { compact: `${short} ago`, wide: `Updated ${span(ago)} ago`, spoken: `Updated ${span(ago)} ago` };
  }
  const f = formatsIn(clock.zone);
  const ahead = ago < -AHEAD_TOLERANCE;
  const aheadSpoken = ahead ? ", ahead of this device's clock" : "";
  if (f.calendarDay(at) === f.calendarDay(clock.now)) {
    const time = f.time(at);
    return { compact: ahead ? "Clock ahead" : time, wide: `Updated ${time}${ahead ? " · clock ahead" : ""}`, spoken: `Updated today at ${time}${aheadSpoken}` };
  }
  const sameYear = f.calendarDay(at).slice(0, 4) === f.calendarDay(clock.now).slice(0, 4);
  const day = (sameYear ? f.day : f.dayYear)(at);
  return { compact: ahead ? "Clock ahead" : day, wide: `Updated ${day}${ahead ? " · clock ahead" : ""}`, spoken: `Updated on ${f.spokenDay(at)}${aheadSpoken}` };
}

export type TimeLine = { readonly label: string; readonly value: string };

const timed = (at: number, clock: Clock, meaning?: string) =>
  `${exactTime(at, clock.zone)} · ${ageInWords(at, clock.now)}${meaning ? `\n${meaning}` : ""}`;

/** Why a state has no start time, by state. */
const UNRECORDED: Readonly<Record<OrganizationState, string>> = {
  needs_input: "Paseo keeps no time for a permission request",
  failed: "Paseo keeps no time for the current error starting",
  running: "Paseo reported no start for this turn, as after a Paseo restart",
  attention: "Paseo reported no time for this attention",
  initializing: "Paseo keeps no time for a session starting",
  idle: "Paseo keeps no time for a session becoming idle",
  closed: "Paseo keeps no time for a session closing",
};

/**
 * Since when the session shows its state, only where Paseo recorded that change for the cue the
 * badge shows. Attention stays raised until cleared. Error attention can survive successful
 * later turns, so its timestamp never proves when a current error started.
 * A running session has run at least since its current turn started: a prompt that
 * interrupts a turn starts a new one while the session keeps running. Anything else is named as
 * not recorded, never filled in.
 */
export function stateSinceLine(entry: DirectoryEntry, clock: Clock): TimeLine {
  const state = sessionState(entry);
  const label = `${STATE_LABELS[state]} since`;
  const raised = entry.requiresAttention ? parseTime(entry.attentionTimestamp) : null;
  if (state === "running") {
    const started = parseTime(entry.activeTurnStartedAt);
    if (started !== null) return { label, value: `At least ${timed(started, clock, "When the current turn started. A prompt that interrupts a turn starts a new one, so the session may have run longer")}` };
  } else if (state === "attention" && raised !== null) {
    return { label, value: timed(raised, clock, "When Paseo raised it, at a finished turn or Mark as unread; it has stayed raised since, through any later turn") };
  }
  return { label, value: `Not recorded: ${UNRECORDED[state]}` };
}

/** What a list read, for the times' As of line: whether it is live, and when it last read the inventory. */
export type TimeFreshness = { readonly live: boolean; readonly readAt: number | null };

function unavailableTime(value: string | null | undefined, absent: string): string {
  return typeof value === "string" && value.trim() !== "" ? "Invalid timestamp reported" : absent;
}

/** Every time Details gives for a session: exact, with its zone, and what each one means. */
export function sessionTimeLines(entry: DirectoryEntry, clock: Clock, freshness: TimeFreshness): TimeLine[] {
  const updated = parseTime(entry.updatedAt);
  const prompt = parseTime(entry.lastUserMessageAt);
  const created = parseTime(entry.createdAt);
  const archived = parseTime(entry.archivedAt);
  const lines: TimeLine[] = [
    { label: "Updated", value: updated === null ? unavailableTime(entry.updatedAt, "Not reported") : timed(updated, clock, "Paseo's last change to the session: activity or a prompt, and also a rename, a label change, a restore from archive, or attention being raised or cleared") },
    { label: "Last activity", value: "Not supplied separately. Updated includes changes to the session's name, labels and attention" },
    stateSinceLine(entry, clock),
    { label: "Last prompt", value: prompt === null ? unavailableTime(entry.lastUserMessageAt, "None reported") : timed(prompt, clock) },
    { label: "Created", value: created === null ? unavailableTime(entry.createdAt, "Not reported") : timed(created, clock) },
  ];
  if (archived !== null) lines.push({ label: "Archived", value: timed(archived, clock) });
  lines.push({ label: "Time zone", value: clock.zone ? `${clock.zone}, this device's time zone` : "This device's time zone" });
  if (!freshness.live) {
    lines.push({ label: "As of", value: freshness.readAt === null
      ? "Sessions not read yet; these times may be out of date"
      : `Last read ${exactTime(freshness.readAt, clock.zone)}; Paseo may have changed the session since` });
  }
  return lines;
}

// One clock for every row: a single timer, started by the first subscriber and stopped with the
// last, wakes just after each whole minute so every age on screen turns over together, and reads
// the device's zone again so a zone change redraws the times.
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | null = null;
let clock: Clock = { now: Date.now(), zone: deviceZone() };

function tick() {
  timer = null;
  clock = { now: Date.now(), zone: deviceZone() };
  for (const listener of [...listeners]) listener();
  schedule();
}

function schedule() {
  if (timer !== null || listeners.size === 0) return;
  timer = setTimeout(tick, MINUTE - (Date.now() % MINUTE) + 50);
  (timer as { unref?: () => void }).unref?.();
}

/** The shared clock. Without a running timer it is read fresh, at most once a second. */
export function currentClock(): Clock {
  if (timer === null && Math.abs(Date.now() - clock.now) >= 1000) clock = { now: Date.now(), zone: deviceZone() };
  return clock;
}

/** Calls the listener on each minute and zone change; returns the unsubscribe. */
export function subscribeClock(listener: () => void): () => void {
  if (listeners.size === 0) currentClock();
  listeners.add(listener);
  schedule();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };
}

/** How many components the clock is redrawing, and whether its timer runs; for tests. */
export function clockStatus(): { readonly listeners: number; readonly running: boolean } {
  return { listeners: listeners.size, running: timer !== null };
}

/** A row's Updated time, redrawn only when its text changes. */
export function useUpdatedTime(updatedAt: string | null | undefined, live: boolean): RowTime | null {
  const [, redraw] = useState(0);
  const time = updatedRowTime(updatedAt, currentClock(), live);
  const spoken = time?.spoken ?? null;
  useEffect(() => subscribeClock(() => {
    if ((updatedRowTime(updatedAt, clock, live)?.spoken ?? null) !== spoken) redraw((count) => count + 1);
  }), [updatedAt, live, spoken]);
  return time;
}

/** The shared clock, redrawn on every tick: for Details, which shows one session's ages. */
export function useClock(): Clock {
  const [, redraw] = useState(0);
  useEffect(() => subscribeClock(() => redraw((count) => count + 1)), []);
  return currentClock();
}
