import { execFileSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  FLOOR_INVALID_EVENT,
  FLOOR_MODE_EVENT,
  floorFile,
  parseFloor,
  readFloor,
  reportFloor,
  type FloorMode,
} from "../server/floor";

const MAX_BYTES = 2 * 1024 * 1024;

describe("parseFloor", () => {
  it("accepts exactly on and off", () => {
    expect(parseFloor('{"floor":"on"}')).toEqual({ value: "on" });
    expect(parseFloor('{"floor":"off"}')).toEqual({ value: "off" });
  });

  it("allows JSON formatting whitespace around tokens", () => {
    expect(parseFloor('{\n  "floor": "off"\n}\n')).toEqual({ value: "off" });
    expect(parseFloor('\t { "floor"\r\n:\t"on" } ')).toEqual({ value: "on" });
  });

  it("judges escaped spellings by their parsed key and value", () => {
    expect(parseFloor('{"\\u0066loor":"\\u006fff"}')).toEqual({ value: "off" });
  });

  it.each([
    ['{"floor":" off"}', "whitespace before the value"],
    ['{"floor":"off "}', "whitespace after the value"],
    ['{"floor":"o ff"}', "whitespace inside the value"],
    ['{"floor":"OFF"}', "another case"],
    ['{"floor":"Off"}', "another case"],
    ['{"floor":"false"}', "another word"],
    ['{"floor":""}', "an empty value"],
    ['{"floor":false}', "a boolean"],
    ['{"floor":0}', "a number"],
    ['{"floor":null}', "null"],
    ['{"floor":["off"]}', "an array"],
    ['{"floor":{"off":true}}', "an object"],
  ])("refuses %s (%s)", (raw) => {
    const result = parseFloor(raw);
    expect(result).toHaveProperty("error");
    expect((result as { error: string }).error).toMatch(/^"floor" must be the string "on" or "off", found /);
  });

  it.each([
    ["{}", "no key"],
    ['{"Floor":"off"}', "another key"],
    ['{"floor":"off","note":"x"}', "an extra key"],
    ['{"note":"x","floor":"off"}', "an extra key first"],
  ])("refuses %s (%s)", (raw) => {
    expect((parseFloor(raw) as { error: string }).error).toMatch(/^expected exactly the key "floor", found keys /);
  });

  it.each(['"off"', "off", "[]", '["floor","off"]', "null", "0", "true"])("refuses the non-object %s", (raw) => {
    expect((parseFloor(raw) as { error: string }).error).toMatch(/^(expected a JSON object|malformed JSON)/);
  });

  it.each(["", " ", '{"floor":"off"', '{"floor":"off",}', "{'floor':'off'}", '\ufeff{"floor":"off"}', '{"floor":"off"} x'])(
    "refuses malformed JSON %j",
    (raw) => {
      expect((parseFloor(raw) as { error: string }).error).toMatch(/^malformed JSON: /);
    },
  );

  it.each([
    '{"floor":"on","floor":"off"}',
    '{"floor":"off","floor":"off"}',
    '{"floor":"off" , "floor":"on"}',
    // Brackets and quotes inside an earlier value must not hide the separator.
    '{"floor":"[","floor":"off"}',
    '{"floor":"{","floor":"off"}',
    '{"floor":"\\"[","floor":"off"}',
    '{"floor":"\\\\","floor":"off"}',
    '{"floor":[1,2],"floor":"off"}',
    '{"floor":{"a":[1]},"floor":"off"}',
  ])("refuses the repeated member %s, whose last value would otherwise win", (raw) => {
    expect(parseFloor(raw)).toEqual({ error: 'expected exactly one "floor" member, found it repeated' });
  });

  it.each(['{"floor":"o,ff"}', '{"floor":["on","off"]}', '{"floor":{"a":1,"b":2}}', '{"floor":[["o"],{"f":[1,2]}]}'])(
    "reads the commas inside the single member %s as part of its value",
    (raw) => {
      expect((parseFloor(raw) as { error: string }).error).toMatch(/^"floor" must be the string "on" or "off", found /);
    },
  );

  it("bounds a long found value in the error", () => {
    const error = (parseFloor(JSON.stringify({ floor: "x".repeat(10_000) })) as { error: string }).error;
    expect(error.length).toBeLessThan(220);
    expect(error.endsWith("…")).toBe(true);
  });
});

describe("readFloor", () => {
  let dir: string;
  let file: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "kontor-floor-"));
    file = join(dir, "floor.json");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("is enabled and not configured when the file is absent", () => {
    expect(readFloor(file)).toEqual({ enabled: true, reason: "not-configured", file, error: null });
  });

  it("is enabled by an explicit on", () => {
    writeFileSync(file, '{"floor":"on"}');
    expect(readFloor(file)).toEqual({ enabled: true, reason: "explicit-on", file, error: null });
  });

  it("is disabled only by an explicit off, pretty-printed or not", () => {
    writeFileSync(file, '{"floor":"off"}');
    expect(readFloor(file)).toEqual({ enabled: false, reason: "explicit-off", file, error: null });
    writeFileSync(file, '{\n  "floor": "off"\n}\n');
    expect(readFloor(file)).toEqual({ enabled: false, reason: "explicit-off", file, error: null });
  });

  it("keeps the floor enabled with the parse error for a non-conforming file", () => {
    writeFileSync(file, '{"floor":"OFF"}');
    expect(readFloor(file)).toEqual({
      enabled: true,
      reason: "invalid",
      file,
      error: '"floor" must be the string "on" or "off", found "OFF"',
    });
  });

  it("keeps the floor enabled for an empty file", () => {
    writeFileSync(file, "");
    const mode = readFloor(file);
    expect(mode.enabled).toBe(true);
    expect(mode.reason).toBe("invalid");
    expect(mode.error).toMatch(/^malformed JSON: /);
  });

  it("reads a file of exactly 2 MiB", () => {
    const body = '{"floor":"off"}';
    writeFileSync(file, body + " ".repeat(MAX_BYTES - body.length));
    expect(readFloor(file)).toMatchObject({ enabled: false, reason: "explicit-off" });
  });

  it("keeps the floor enabled for a file over 2 MiB", () => {
    const body = '{"floor":"off"}';
    writeFileSync(file, body + " ".repeat(MAX_BYTES - body.length + 1));
    expect(readFloor(file)).toEqual({
      enabled: true,
      reason: "invalid",
      file,
      error: "must be a regular file of at most 2 MiB",
    });
  });

  it("keeps the floor enabled for a directory", () => {
    mkdirSync(file);
    expect(readFloor(file)).toMatchObject({ enabled: true, reason: "invalid", error: "must be a regular file of at most 2 MiB" });
  });

  it("keeps the floor enabled for a FIFO without blocking on it", () => {
    execFileSync("mkfifo", [file]);
    expect(readFloor(file)).toMatchObject({ enabled: true, reason: "invalid", error: "must be a regular file of at most 2 MiB" });
  });

  it("follows a symlink to a regular file, as the source catalog does", () => {
    const target = join(dir, "target.json");
    writeFileSync(target, '{"floor":"off"}');
    symlinkSync(target, file);
    expect(readFloor(file)).toMatchObject({ enabled: false, reason: "explicit-off" });
  });

  it("treats a dangling symlink as absent, since it opens as ENOENT", () => {
    symlinkSync(join(dir, "missing.json"), file);
    expect(readFloor(file)).toMatchObject({ enabled: true, reason: "not-configured" });
  });

  it("keeps the floor enabled when a path component is a file", () => {
    writeFileSync(join(dir, "kontor"), "");
    const nested = join(dir, "kontor", "floor.json");
    const mode = readFloor(nested);
    expect(mode).toMatchObject({ enabled: true, reason: "invalid", file: nested });
    expect(mode.error).toMatch(/^cannot read: .*ENOTDIR/);
  });

  it("closes the file it opened, whatever it found", () => {
    const open = () => readdirSync("/dev/fd").length;
    writeFileSync(file, '{"floor":"off"}');
    const before = open();
    for (let index = 0; index < 50; index += 1) readFloor(file);
    writeFileSync(file, "not JSON");
    for (let index = 0; index < 50; index += 1) readFloor(file);
    expect(open()).toBe(before);
  });

  it.skipIf(process.getuid?.() === 0)("keeps the floor enabled for an unreadable file", () => {
    writeFileSync(file, '{"floor":"off"}');
    chmodSync(file, 0o000);
    const mode = readFloor(file);
    expect(mode).toMatchObject({ enabled: true, reason: "invalid" });
    expect(mode.error).toMatch(/^cannot read: .*EACCES/);
  });
});

describe("floorFile", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("follows PASEO_HOME", () => {
    vi.stubEnv("HOME", "/Users/test");
    vi.stubEnv("PASEO_HOME", "/srv/paseo");
    expect(floorFile()).toBe("/srv/paseo/plugin-config/kontor/floor.json");
  });

  it("defaults to ~/.paseo", () => {
    vi.stubEnv("HOME", "/Users/test");
    vi.stubEnv("PASEO_HOME", "");
    expect(floorFile()).toBe("/Users/test/.paseo/plugin-config/kontor/floor.json");
  });
});

describe("reportFloor", () => {
  it("names the two events the README documents", () => {
    expect(FLOOR_MODE_EVENT).toBe("kontor.floor.mode");
    expect(FLOOR_INVALID_EVENT).toBe("kontor.floor.invalid");
  });

  const file = "/srv/paseo/plugin-config/kontor/floor.json";
  const record = (mode: FloorMode) => {
    const info: unknown[] = [];
    const error: unknown[] = [];
    reportFloor(mode, { info: (line) => info.push(JSON.parse(line)), error: (line) => error.push(JSON.parse(line)) });
    return { info, error };
  };

  it("writes only the mode line when the file is absent", () => {
    expect(record({ enabled: true, reason: "not-configured", file, error: null })).toEqual({
      info: [{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "not-configured", file }],
      error: [],
    });
  });

  it("writes only the mode line for an explicit on", () => {
    expect(record({ enabled: true, reason: "explicit-on", file, error: null })).toEqual({
      info: [{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "explicit-on", file }],
      error: [],
    });
  });

  it("writes only the mode line for an explicit off", () => {
    expect(record({ enabled: false, reason: "explicit-off", file, error: null })).toEqual({
      info: [{ event: FLOOR_MODE_EVENT, floor: "disabled", reason: "explicit-off", file }],
      error: [],
    });
  });

  it("adds the invalid-value diagnostic on the error channel", () => {
    expect(record({ enabled: true, reason: "invalid", file, error: "exceeds 2 MiB" })).toEqual({
      info: [{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "invalid", file }],
      error: [{ event: FLOOR_INVALID_EVENT, floor: "enabled", file, error: "exceeds 2 MiB" }],
    });
  });

  it("writes to the worker's stdout and stderr by default", () => {
    const log = vi.spyOn(console, "log").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      reportFloor({ enabled: true, reason: "invalid", file, error: "exceeds 2 MiB" });
      expect(log.mock.calls.map(([line]) => JSON.parse(String(line)).event)).toEqual([FLOOR_MODE_EVENT]);
      expect(error.mock.calls.map(([line]) => JSON.parse(String(line)).event)).toEqual([FLOOR_INVALID_EVENT]);
    } finally {
      log.mockRestore();
      error.mockRestore();
    }
  });
});
