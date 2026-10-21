import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const hooks = vi.hoisted(() => ({
  effects: [] as (() => void | (() => void))[],
  redraws: 0,
}));

// A one-slot stand-in for React: the hook's state setter only counts redraws, and its effect is run by the test.
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useState: (initial: unknown) => [initial, () => { hooks.redraws += 1; }],
  useEffect: (effect: () => void | (() => void)) => { hooks.effects.push(effect); },
}));

import type { DirectoryEntry } from "../client/session-directory";
import {
  ageInWords,
  clockStatus,
  currentClock,
  exactTime,
  parseTime,
  sessionTimeLines,
  stateSinceLine,
  subscribeClock,
  updatedRowTime,
  useUpdatedTime,
  type Clock,
} from "../client/session-time";
import { sessionEntry } from "./session-fixtures";

const NOW = Date.parse("2026-10-20T12:00:00Z");
const OSLO: Clock = { now: NOW, zone: "Europe/Oslo" };
const ago = (milliseconds: number) => new Date(NOW - milliseconds).toISOString();
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** The same formats the module uses, so these checks hold in any test locale. */
const format = (at: number, zone: string, options: Intl.DateTimeFormatOptions) => new Intl.DateTimeFormat([], { timeZone: zone, ...options }).format(at);
const clockTime = (at: number, zone = "Europe/Oslo") => format(at, zone, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
const shortDay = (at: number, zone = "Europe/Oslo") => format(at, zone, { day: "numeric", month: "short" });

function entry(id: string, overrides: Partial<DirectoryEntry> = {}): DirectoryEntry {
  return { ...sessionEntry(id), ...overrides };
}

describe("parseTime", () => {
  it("reads ISO times and turns absent, blank or unreadable values into null", () => {
    expect(parseTime("2026-10-20T12:00:00.000Z")).toBe(NOW);
    expect([parseTime(null), parseTime(undefined), parseTime(""), parseTime("  "), parseTime("not a time")]).toEqual([null, null, null, null, null]);
  });
});

describe("updatedRowTime on a live list", () => {
  const live = (milliseconds: number) => updatedRowTime(ago(milliseconds), OSLO, true);

  it("gives the age, rounded down, in a short form for phones and words for wide rows and speech", () => {
    expect(live(0)).toEqual({ compact: "just now", wide: "Updated just now", spoken: "Updated just now" });
    expect(live(59_999)).toMatchObject({ compact: "just now" });
    expect(live(MINUTE)).toEqual({ compact: "1m ago", wide: "Updated 1 minute ago", spoken: "Updated 1 minute ago" });
    expect(live(59 * MINUTE + 59_999)).toEqual({ compact: "59m ago", wide: "Updated 59 minutes ago", spoken: "Updated 59 minutes ago" });
    expect(live(HOUR)).toEqual({ compact: "1h ago", wide: "Updated 1 hour ago", spoken: "Updated 1 hour ago" });
    expect(live(3 * HOUR + 59 * MINUTE)).toMatchObject({ compact: "3h ago", wide: "Updated 3 hours ago" });
    expect(live(DAY - 1)).toMatchObject({ compact: "23h ago" });
    expect(live(DAY)).toEqual({ compact: "1d ago", wide: "Updated 1 day ago", spoken: "Updated 1 day ago" });
    expect(live(7 * DAY - 1)).toMatchObject({ compact: "6d ago", wide: "Updated 6 days ago" });
  });

  it("reads up to a minute ahead of this device's clock as just now", () => {
    expect(live(-MINUTE)).toMatchObject({ compact: "just now" });
  });

  it("gives the date from a week on, with the year only when it is another year", () => {
    const week = NOW - 7 * DAY;
    expect(live(7 * DAY)).toEqual({ compact: shortDay(week), wide: `Updated ${shortDay(week)}`, spoken: `Updated on ${format(week, "Europe/Oslo", { day: "numeric", month: "long", year: "numeric" })}` });
    const lastYear = Date.parse("2025-12-30T12:00:00Z");
    expect(updatedRowTime(new Date(lastYear).toISOString(), OSLO, true)!.compact).toBe(format(lastYear, "Europe/Oslo", { day: "numeric", month: "short", year: "numeric" }));
  });

  it("gives a time more than a minute ahead of this device's clock as the time itself, and says so", () => {
    const ahead = NOW + 2 * MINUTE;
    expect(live(-2 * MINUTE)).toEqual({ compact: "Clock ahead", wide: `Updated ${clockTime(ahead)} · clock ahead`, spoken: `Updated today at ${clockTime(ahead)}, ahead of this device's clock` });
    const tomorrow = NOW + DAY;
    expect(live(-DAY)!.spoken).toBe(`Updated on ${format(tomorrow, "Europe/Oslo", { day: "numeric", month: "long", year: "numeric" })}, ahead of this device's clock`);
  });

  it("shows nothing for an absent or unreadable time", () => {
    expect([updatedRowTime(undefined, OSLO, true), updatedRowTime("", OSLO, true), updatedRowTime("yesterday", OSLO, true)]).toEqual([null, null, null]);
  });
});

describe("updatedRowTime on a list that is not live", () => {
  it("gives the time itself instead of a ticking age: today's time, else the date", () => {
    const earlier = NOW - 3 * HOUR;
    expect(updatedRowTime(ago(3 * HOUR), OSLO, false)).toEqual({ compact: clockTime(earlier), wide: `Updated ${clockTime(earlier)}`, spoken: `Updated today at ${clockTime(earlier)}` });
    expect(updatedRowTime(ago(30 * HOUR), OSLO, false)!.compact).toBe(shortDay(NOW - 30 * HOUR));
  });

  it("decides today in the device's own zone, so the same instants read differently after a zone change", () => {
    // 23:30 UTC on the 19th and 00:10 UTC on the 20th: one day apart in UTC, the same morning in Oslo.
    const at = Date.parse("2026-10-19T23:30:00Z");
    const now = Date.parse("2026-10-20T00:10:00Z");
    const iso = new Date(at).toISOString();
    expect(updatedRowTime(iso, { now, zone: "Europe/Oslo" }, false)!.compact).toBe(clockTime(at));
    expect(clockTime(at)).toBe("01:30");
    expect(updatedRowTime(iso, { now, zone: "UTC" }, false)!.compact).toBe(shortDay(at, "UTC"));
  });
});

describe("ageInWords and exactTime", () => {
  it("names the age in whole units, and how far ahead a future time is", () => {
    expect([ageInWords(NOW, NOW), ageInWords(NOW - MINUTE, NOW), ageInWords(NOW - 2 * HOUR, NOW), ageInWords(NOW - 40 * DAY, NOW)])
      .toEqual(["just now", "1 minute ago", "2 hours ago", "40 days ago"]);
    expect([ageInWords(NOW + MINUTE, NOW), ageInWords(NOW + 5 * MINUTE, NOW), ageInWords(NOW + 3 * DAY, NOW)])
      .toEqual(["just now", "ahead of this device's clock by 5 minutes", "ahead of this device's clock by 3 days"]);
  });

  it("gives the weekday, date, seconds and zone", () => {
    const at = Date.parse("2026-10-20T12:32:05Z");
    expect(exactTime(at, "Europe/Oslo")).toBe(format(at, "Europe/Oslo", { weekday: "short", day: "numeric", month: "short", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23", timeZoneName: "short" }));
    expect(exactTime(at, "Europe/Oslo")).toContain("14:32:05");
    expect(exactTime(at, "UTC")).toContain("12:32:05");
  });
});

describe("stateSinceLine", () => {
  const exact = (milliseconds: number) => exactTime(NOW - milliseconds, "Europe/Oslo");

  it("gives a running session the start of its current turn as a lower bound", () => {
    const line = stateSinceLine(entry("run", { status: "running", activeTurnStartedAt: ago(12 * MINUTE), updatedAt: ago(0) }), OSLO);
    expect(line).toEqual({ label: "Running since", value: `At least ${exact(12 * MINUTE)} · 12 minutes ago\nWhen the current turn started. A prompt that interrupts a turn starts a new one, so the session may have run longer` });
    expect(stateSinceLine(entry("restarted", { status: "running", activeTurnStartedAt: null }), OSLO))
      .toEqual({ label: "Running since", value: "Not recorded: Paseo reported no start for this turn, as after a Paseo restart" });
  });

  it("gives Attention the time Paseo raised it, never the Updated time", () => {
    const raised = entry("finished", { status: "idle", requiresAttention: true, attentionReason: "finished", attentionTimestamp: ago(2 * HOUR), updatedAt: ago(5 * MINUTE) });
    expect(stateSinceLine(raised, OSLO)).toEqual({ label: "Attention since", value: `${exact(2 * HOUR)} · 2 hours ago\nWhen Paseo raised it, at a finished turn or Mark as unread; it has stayed raised since, through any later turn` });
    const cleared = entry("cleared", { status: "idle", requiresAttention: false, attentionReason: "finished", attentionTimestamp: ago(2 * HOUR), updatedAt: ago(5 * MINUTE) });
    expect(stateSinceLine(cleared, OSLO)).toEqual({ label: "Idle since", value: "Not recorded: Paseo keeps no time for a session becoming idle" });
    const timeless = entry("timeless", { status: "idle", requiresAttention: true, attentionReason: "finished", attentionTimestamp: null, updatedAt: ago(5 * MINUTE) });
    expect(stateSinceLine(timeless, OSLO)).toEqual({ label: "Attention since", value: "Not recorded: Paseo reported no time for this attention" });
  });

  it("never uses sticky error attention as the start of a current error", () => {
    const earlier = entry("earlier", { status: "idle", requiresAttention: true, attentionReason: "error", attentionTimestamp: ago(3 * HOUR) });
    expect(stateSinceLine({ ...earlier, status: "error" }, OSLO)).toEqual({ label: "Error since", value: "Not recorded: Paseo keeps no time for the current error starting" });
    // An error while a finished-turn attention was already raised keeps that attention's time, which is not the error's.
    const afterFinished = entry("after-finished", { status: "error", requiresAttention: true, attentionReason: "finished", attentionTimestamp: ago(3 * HOUR) });
    expect(stateSinceLine(afterFinished, OSLO)).toEqual({ label: "Error since", value: "Not recorded: Paseo keeps no time for the current error starting" });
  });

  it("names every other state as not recorded", () => {
    const lines = [
      entry("input", { pendingPermissionCount: 1, needsInput: true, requiresAttention: true, attentionReason: "finished", attentionTimestamp: ago(HOUR) }),
      entry("idle", { status: "idle", updatedAt: ago(MINUTE) }),
      entry("closed", { status: "closed" }),
      entry("starting", { status: "initializing" }),
    ].map((value) => stateSinceLine(value, OSLO));
    expect(lines).toEqual([
      { label: "Needs input since", value: "Not recorded: Paseo keeps no time for a permission request" },
      { label: "Idle since", value: "Not recorded: Paseo keeps no time for a session becoming idle" },
      { label: "Closed since", value: "Not recorded: Paseo keeps no time for a session closing" },
      { label: "Starting since", value: "Not recorded: Paseo keeps no time for a session starting" },
    ]);
  });
});

describe("sessionTimeLines", () => {
  const exact = (milliseconds: number) => exactTime(NOW - milliseconds, "Europe/Oslo");

  it("gives every time exactly, with its age, meaning and the device's zone", () => {
    const value = entry("a", { updatedAt: ago(3 * HOUR), lastUserMessageAt: ago(4 * HOUR), createdAt: ago(2 * DAY), archivedAt: ago(HOUR) });
    expect(sessionTimeLines(value, OSLO, { live: true, readAt: NOW })).toEqual([
      { label: "Updated", value: `${exact(3 * HOUR)} · 3 hours ago\nPaseo's last change to the session: activity or a prompt, and also a rename, a label change, a restore from archive, or attention being raised or cleared` },
      { label: "Last activity", value: "Not supplied separately. Updated includes changes to the session's name, labels and attention" },
      { label: "Idle since", value: "Not recorded: Paseo keeps no time for a session becoming idle" },
      { label: "Last prompt", value: `${exact(4 * HOUR)} · 4 hours ago` },
      { label: "Created", value: `${exact(2 * DAY)} · 2 days ago` },
      { label: "Archived", value: `${exact(HOUR)} · 1 hour ago` },
      { label: "Time zone", value: "Europe/Oslo, this device's time zone" },
    ]);
  });

  it("names absent times, leaves out Archived for a session that is not archived, and gives an As of line only off live", () => {
    const value = entry("b", { updatedAt: "", lastUserMessageAt: undefined, createdAt: "garbage" });
    const labelled = (freshness: Parameters<typeof sessionTimeLines>[2]) => Object.fromEntries(sessionTimeLines(value, OSLO, freshness).map(({ label, value: text }) => [label, text]));
    expect(labelled({ live: true, readAt: NOW })).toEqual({
      Updated: "Not reported", "Last activity": "Not supplied separately. Updated includes changes to the session's name, labels and attention", "Idle since": "Not recorded: Paseo keeps no time for a session becoming idle", "Last prompt": "None reported", Created: "Invalid timestamp reported", "Time zone": "Europe/Oslo, this device's time zone",
    });
    expect(labelled({ live: false, readAt: NOW - 5 * MINUTE })["As of"]).toBe(`Last read ${exact(5 * MINUTE)}; Paseo may have changed the session since`);
    expect(labelled({ live: false, readAt: null })["As of"]).toBe("Sessions not read yet; these times may be out of date");
    expect(sessionTimeLines(value, { now: NOW, zone: "" }, { live: true, readAt: NOW }).at(-1)).toEqual({ label: "Time zone", value: "This device's time zone" });
  });
});

describe("the shared clock", () => {
  const zone = process.env.TZ;
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW + 20_000);
    hooks.effects.length = 0;
    hooks.redraws = 0;
  });
  afterEach(() => {
    vi.useRealTimers();
    if (zone === undefined) delete process.env.TZ;
    else process.env.TZ = zone;
  });

  it("runs one timer for every subscriber, wakes after each whole minute and stops with the last one", () => {
    expect(clockStatus()).toEqual({ listeners: 0, running: false });
    const first = vi.fn();
    const second = vi.fn();
    const stopFirst = subscribeClock(first);
    const stopSecond = subscribeClock(second);
    expect(clockStatus()).toEqual({ listeners: 2, running: true });
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(40_000 + 49);
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([0, 0]);
    vi.advanceTimersByTime(1);
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([1, 1]);
    expect(currentClock().now).toBe(NOW + MINUTE + 50);
    vi.advanceTimersByTime(MINUTE);
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([2, 2]);
    expect(vi.getTimerCount()).toBe(1);
    stopFirst();
    expect(clockStatus()).toEqual({ listeners: 1, running: true });
    stopSecond();
    expect(clockStatus()).toEqual({ listeners: 0, running: false });
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reads the device's zone again on each tick", () => {
    process.env.TZ = "Europe/Oslo";
    const stop = subscribeClock(() => undefined);
    expect(currentClock().zone).toBe("Europe/Oslo");
    process.env.TZ = "Asia/Tokyo";
    vi.advanceTimersByTime(MINUTE);
    expect(currentClock().zone).toBe("Asia/Tokyo");
    stop();
  });

  it("reads the time fresh while no timer runs", () => {
    expect(currentClock().now).toBe(NOW + 20_000);
    vi.setSystemTime(NOW + HOUR);
    expect(currentClock().now).toBe(NOW + HOUR);
  });

  it("redraws a row only when its text changes, and unsubscribes it on cleanup", () => {
    // 3h 05m ago at 12:00:20. Ticks come at 50 ms past each minute: at 12:55:00 it reads
    // 3h 59m 40s, still 3h, and at 12:56:00 4h 00m 40s, the first change.
    const updatedAt = new Date(NOW + 20_000 - 3 * HOUR - 5 * MINUTE).toISOString();
    expect(useUpdatedTime(updatedAt, true)).toMatchObject({ compact: "3h ago" });
    const cleanup = hooks.effects[0]!();
    expect(clockStatus().listeners).toBe(1);
    vi.advanceTimersByTime(55 * MINUTE);
    expect(hooks.redraws).toBe(0);
    vi.advanceTimersByTime(MINUTE);
    expect(hooks.redraws).toBe(1);
    expect(typeof cleanup).toBe("function");
    (cleanup as () => void)();
    expect(clockStatus()).toEqual({ listeners: 0, running: false });
  });

  it("does not tick a row on a list that is not live", () => {
    expect(useUpdatedTime(new Date(NOW).toISOString(), false)).toMatchObject({ compact: clockTime(NOW, process.env.TZ ?? Intl.DateTimeFormat().resolvedOptions().timeZone) });
    const cleanup = hooks.effects[0]!() as () => void;
    vi.advanceTimersByTime(90 * MINUTE);
    expect(hooks.redraws).toBe(0);
    cleanup();
  });
});
