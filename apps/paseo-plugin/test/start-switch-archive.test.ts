import { describe, expect, it, vi } from "vitest";
import type { LifecycleRow } from "../shared/session-lifecycle";
import { sessionEntry } from "./session-fixtures";

vi.mock("react-native", () => ({ Platform: { OS: "web" }, Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: vi.fn() }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", ScrollView: "ScrollView" }));
import { archiveDispositions } from "../client/start-switch-archive";
import { withLocalDispositions } from "../client/use-session-availability";

const entry = sessionEntry("exact-a", { updatedAt: "2026-10-25T12:00:00Z" });
const row = (patch: Partial<LifecycleRow> = {}) => ({ id: entry.id, outcome: "archived", availability: { agentId: entry.id, state: "readable", archivedAt: "2026-10-25T12:01:00Z", checkedAt: "2026-10-25T12:01:00Z", revision: entry.updatedAt, reason: "Exact native read." }, ...patch }) as LifecycleRow;

describe("Start/switch archive membership", () => {
  it("reuses exact native membership without claiming a runtime stop, including an uncertain reply with a confirmed native archive", () => {
    for (const result of [row(), row({ outcome: "uncertain", runtime: "unconfirmed" })]) {
      const values = archiveDispositions([entry], [result], entry);
      expect(withLocalDispositions([entry], values)[0]).toMatchObject({ archivedAt: result.availability!.archivedAt });
      expect(withLocalDispositions([entry], values)[0]!.status).toBe(entry.status);
    }
  });
  it.each(["blocked", "pending", "uncertain"] as const)("does not remove a %s row without native membership evidence", (outcome) => {
    expect(archiveDispositions([entry], [row({ outcome, availability: undefined })], entry)).toEqual({});
  });
  it("rejects mismatched native IDs and does not carry a settled result over a newer native revision", () => {
    const wrong = row(); wrong.availability = { ...wrong.availability!, agentId: "other" };
    expect(withLocalDispositions([entry], archiveDispositions([entry], [wrong], entry))[0]!.archivedAt).toBeNull();
    const values = archiveDispositions([entry], [row()], entry);
    expect(withLocalDispositions([{ ...entry, updatedAt: "2026-10-25T12:02:00Z" }], values)[0]!.archivedAt).toBeNull();
    expect(archiveDispositions([entry], [row({ id: "foreign" })], entry)).toEqual({});
  });
  it("rejects an older archive reply when the native row advanced before settlement", () => {
    const newer = { ...entry, updatedAt: "2026-10-25T12:02:00Z" };
    const values = archiveDispositions([newer], [row()], entry);
    expect(values).toEqual({});
    expect(withLocalDispositions([newer], values)[0]).toEqual(newer);
  });
  it("removes an absent row only for the existing explicit local-removal receipt", () => {
    const absent = row({ outcome: "absent", availability: { agentId: entry.id, state: "missing", reason: "Confirmed absent.", checkedAt: "2026-10-25", revision: entry.updatedAt, localRemoval: true } });
    expect(withLocalDispositions([entry], archiveDispositions([entry], [absent], entry))).toEqual([]);
    expect(archiveDispositions([entry], [row({ outcome: "absent", availability: { ...absent.availability!, localRemoval: false } })], entry)).toEqual({});
  });
});
