import { closeSync, constants, fstatSync, openSync, readSync } from "node:fs";
import { dirname, join } from "node:path";
import { readConfig } from "./config";

const MAX_BYTES = 2 * 1024 * 1024;
/** Longest excerpt of a found key or value quoted in a diagnostic. */
const MAX_EXCERPT = 120;

export const FLOOR_MODE_EVENT = "kontor.floor.mode";
export const FLOOR_INVALID_EVENT = "kontor.floor.invalid";

export type FloorReason = "not-configured" | "explicit-on" | "explicit-off" | "invalid";

/** How the operator's `floor.json` left the floor hooks when this plugin server started. */
export interface FloorMode {
  /** Whether `agent.create`, `agent.session_open` and `workspace.create` are registered. */
  readonly enabled: boolean;
  readonly reason: FloorReason;
  readonly file: string;
  /** Why a present file did not conform; null unless `reason` is `invalid`. */
  readonly error: string | null;
}

export interface FloorLog {
  info(line: string): void;
  error(line: string): void;
}

// Both lines reach `paseo plugin logs kontor --json` as the worker's stdout and stderr.
const workerLog: FloorLog = {
  info: (line) => console.log(line),
  error: (line) => console.error(line),
};

/** The operator-owned switch: `<PASEO_HOME or ~/.paseo>/plugin-config/kontor/floor.json`. */
export function floorFile(): string {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "floor.json");
}

function excerpt(value: unknown): string {
  const text = JSON.stringify(value);
  return text.length > MAX_EXCERPT ? `${text.slice(0, MAX_EXCERPT)}…` : text;
}

/**
 * Whether text that `JSON.parse` accepted as a one-key object repeats that key.
 * A repeated member collapses in the parsed object (the last one wins), so it
 * is read from the text: any comma outside strings at depth one separates two.
 */
function repeatsMember(raw: string): boolean {
  let depth = 0;
  let inString = false;
  for (let index = 0; index < raw.length; index += 1) {
    const char = raw[index];
    if (inString) {
      if (char === "\\") index += 1;
      else if (char === '"') inString = false;
    } else if (char === '"') inString = true;
    else if (char === "{" || char === "[") depth += 1;
    else if (char === "}" || char === "]") depth -= 1;
    else if (char === "," && depth === 1) return true;
  }
  return false;
}

/**
 * Parse a switch. Strictness is on the parsed shape and value: one object with
 * exactly the `key` key, whose value is exactly the string `on` or `off`.
 * Formatting whitespace around tokens is allowed; any other spelling, type, key
 * or a repeated member is an error, so a doubtful file never turns the floor off.
 */
export function parseFloor(raw: string, key = "floor"): { value: "on" | "off" } | { error: string } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    return { error: `malformed JSON: ${error instanceof Error ? error.message : String(error)}` };
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { error: `expected a JSON object, found ${excerpt(parsed)}` };
  }
  const keys = Object.keys(parsed);
  if (keys.length !== 1 || keys[0] !== key) {
    return { error: `expected exactly the key "${key}", found keys ${excerpt(keys)}` };
  }
  if (repeatsMember(raw)) {
    return { error: `expected exactly one "${key}" member, found it repeated` };
  }
  const value = (parsed as Record<string, unknown>)[key];
  if (value !== "on" && value !== "off") {
    return { error: `"${key}" must be the string "on" or "off", found ${excerpt(value)}` };
  }
  return { value };
}

const invalid = (file: string, error: string): FloorMode => ({ enabled: true, reason: "invalid", file, error });

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

function readOpen(file: string, descriptor: number, key: string): FloorMode {
  const info = fstatSync(descriptor);
  if (!info.isFile() || info.size > MAX_BYTES) {
    return invalid(file, "must be a regular file of at most 2 MiB");
  }
  const buffer = Buffer.alloc(MAX_BYTES + 1);
  let length = 0;
  while (length < buffer.length) {
    const count = readSync(descriptor, buffer, length, buffer.length - length, null);
    if (!count) break;
    length += count;
  }
  if (length > MAX_BYTES) return invalid(file, "exceeds 2 MiB");
  const parsed = parseFloor(buffer.subarray(0, length).toString("utf8"), key);
  if ("error" in parsed) return invalid(file, parsed.error);
  return parsed.value === "off"
    ? { enabled: false, reason: "explicit-off", file, error: null }
    : { enabled: true, reason: "explicit-on", file, error: null };
}

/**
 * Read the switch once, at plugin server start; a reload starts a new worker,
 * which reads it again. The plugin only reads this file. Absent means not
 * configured. Every other doubt, a failed close included, keeps the floor
 * enabled and reports why; this never throws, so a bad file cannot stop the
 * plugin from starting.
 */
export function readFloor(file = floorFile(), key = "floor"): FloorMode {
  let descriptor: number;
  try {
    // Non-blocking open prevents a misconfigured FIFO from blocking plugin start.
    descriptor = openSync(file, constants.O_RDONLY | constants.O_NONBLOCK);
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return { enabled: true, reason: "not-configured", file, error: null };
    }
    return invalid(file, `cannot read: ${messageOf(error)}`);
  }
  let mode: FloorMode;
  try {
    mode = readOpen(file, descriptor, key);
  } catch (error) {
    mode = invalid(file, `cannot read: ${messageOf(error)}`);
  }
  try {
    closeSync(descriptor);
  } catch (error) {
    // A failed close leaves the read in doubt, so off never stands; a read error is kept.
    const closing = `cannot close: ${messageOf(error)}`;
    return invalid(file, mode.error ? `${mode.error}; ${closing}` : closing);
  }
  return mode;
}

/**
 * Report the mode on two channels. The informational mode line is always
 * written; the invalid-value diagnostic only for a present file that does not
 * conform. The mode line is the installed state's readback.
 */
export function reportFloor(mode: FloorMode, log: FloorLog = workerLog): void {
  log.info(
    JSON.stringify({
      event: FLOOR_MODE_EVENT,
      floor: mode.enabled ? "enabled" : "disabled",
      reason: mode.reason,
      file: mode.file,
    }),
  );
  if (mode.reason === "invalid") {
    log.error(
      JSON.stringify({ event: FLOOR_INVALID_EVENT, floor: "enabled", file: mode.file, error: mode.error }),
    );
  }
}
