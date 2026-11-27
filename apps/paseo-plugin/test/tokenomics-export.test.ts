import { Blob } from "node:buffer";
import { afterEach, describe, expect, it, vi } from "vitest";
const platform = vi.hoisted(() => ({ os: "web" }));
vi.mock("react-native", () => ({ Platform: { get OS() { return platform.os; } } }));
import { csvFileMode, downloadCsvFile } from "../client/web";
import { tokenomicsCsvPreparation } from "../client/tokenomics-export";

const csv = { filename: "tokenomics-pivot.csv", content: 'Group,Tokens\r\n"Synthetic, team",73000\r\n"Ångström",12\r\n', revision: "source-a" };
afterEach(() => { platform.os = "web"; vi.unstubAllGlobals(); vi.useRealTimers(); });
function browser() {
  vi.useFakeTimers(); vi.stubGlobal("Blob", Blob);
  const picker = vi.fn(() => { throw new Error("Unsafe native picker must never run"); });
  vi.stubGlobal("showSaveFilePicker", picker);
  const link = { href: "", download: "", rel: "", click: vi.fn(), remove: vi.fn() }, appendChild = vi.fn();
  const createObjectURL = vi.fn((_blob: Blob) => "blob:synthetic"), revokeObjectURL = vi.fn();
  vi.stubGlobal("document", { body: { appendChild }, createElement: vi.fn(() => link) });
  vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
  return { picker, link, appendChild, createObjectURL, revokeObjectURL };
}

describe("staged complete CSV preparation", () => {
  it("opens/cancels without a read, prepares without a destination, and downloads on a distinct gesture without another RPC", async () => {
    const web = browser(), preparation = tokenomicsCsvPreparation(), load = vi.fn(async () => csv);
    preparation.update("selection-a"); preparation.cancel();
    expect(load).not.toHaveBeenCalled(); expect(web.createObjectURL).not.toHaveBeenCalled();
    await expect(preparation.prepare(load, "source-a")).resolves.toBe(csv);
    expect(load).toHaveBeenCalledOnce(); expect(preparation.prepared).toBe(csv);
    expect(web.createObjectURL).not.toHaveBeenCalled(); expect(web.picker).not.toHaveBeenCalled();
    expect(downloadCsvFile(preparation.prepared!)).toBe("requested");
    expect(load).toHaveBeenCalledOnce(); expect(web.link.click).toHaveBeenCalledOnce(); expect(web.picker).not.toHaveBeenCalled();
  });
  it.each(["query", "source", "offline", "close"])("discards a late response after %s changes without dispatching a file", async (change) => {
    const web = browser(), preparation = tokenomicsCsvPreparation(); preparation.update("selection-a");
    let resolve!: (value: typeof csv) => void;
    const pending = preparation.prepare(() => new Promise((done) => { resolve = done; }), "source-a");
    if (change === "close") preparation.cancel(); else preparation.update(change === "offline" ? null : change);
    resolve(csv); await expect(pending).resolves.toBeNull();
    expect(preparation.prepared).toBeNull(); expect(web.createObjectURL).not.toHaveBeenCalled(); expect(web.picker).not.toHaveBeenCalled();
  });
  it("disables preparation offline and invalidates prepared bytes when the observed scope changes", async () => {
    const preparation = tokenomicsCsvPreparation(), load = vi.fn(async () => csv);
    await expect(preparation.prepare(load, "source-a")).resolves.toBeNull(); expect(load).not.toHaveBeenCalled();
    preparation.update("selection-a"); await preparation.prepare(load, "source-a"); const version = preparation.version;
    preparation.update("selection-b"); expect(preparation.prepared).toBeNull(); expect(preparation.current(version)).toBe(false);
  });
  it.each(["RPC refused", "Export exceeds the bounded response limit", "Invalid export response"])("%s cannot dispatch a file or leave prepared bytes", async (message) => {
    const web = browser(), preparation = tokenomicsCsvPreparation(); preparation.update("selection-a");
    await expect(preparation.prepare(async () => { throw new Error(message); }, "source-a")).rejects.toThrow(message);
    expect(preparation.prepared).toBeNull(); expect(web.createObjectURL).not.toHaveBeenCalled(); expect(web.picker).not.toHaveBeenCalled();
  });
  it("refuses a changed server revision before any download action", async () => {
    const web = browser(), preparation = tokenomicsCsvPreparation(); preparation.update("selection-a");
    await expect(preparation.prepare(async () => ({ ...csv, revision: "source-b" }), "source-a")).rejects.toThrow("csv_snapshot_changed");
    expect(preparation.prepared).toBeNull(); expect(web.createObjectURL).not.toHaveBeenCalled();
  });
  it("does not let an old failure overwrite the newer selection's prepared result", async () => {
    const preparation = tokenomicsCsvPreparation(); preparation.update("selection-a");
    let reject!: (error: Error) => void;
    const old = preparation.prepare(() => new Promise((_resolve, fail) => { reject = fail; }), "source-a");
    preparation.update("selection-b"); await preparation.prepare(async () => csv, "source-a");
    reject(new Error("Late failure")); await expect(old).resolves.toBeNull(); expect(preparation.prepared).toBe(csv);
  });
});

describe("supported browser download dispatch", () => {
  it("dispatches complete UTF-8 bytes and filename synchronously, then cleans up without claiming a save", async () => {
    const web = browser(); expect(csvFileMode()).toBe("download"); expect(downloadCsvFile(csv)).toBe("requested");
    const blob = web.createObjectURL.mock.calls[0]![0];
    expect(await blob.text()).toBe(csv.content); expect(blob.type).toBe("text/csv;charset=utf-8");
    expect(web.link.download).toBe(csv.filename); expect(web.appendChild).toHaveBeenCalledWith(web.link);
    expect(web.link.click).toHaveBeenCalledOnce(); expect(web.link.remove).toHaveBeenCalledOnce(); expect(web.picker).not.toHaveBeenCalled();
    expect(web.revokeObjectURL).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(60_000);
    expect(web.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:synthetic");
  });
  it.each(["append", "click"])("cleans up a refused %s immediately without reporting dispatch", (step) => {
    const web = browser(), fail = () => { throw new Error("Dispatch refused"); };
    (step === "click" ? web.link.click : web.appendChild).mockImplementation(fail);
    expect(() => downloadCsvFile(csv)).toThrow("Dispatch refused"); expect(web.link.remove).toHaveBeenCalledOnce();
    expect(web.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:synthetic");
  });
  it.each(["ios", "android"])("keeps native %s outside browser file APIs", (os) => {
    const web = browser(); platform.os = os;
    expect(csvFileMode()).toBeNull(); expect(() => downloadCsvFile(csv)).toThrow("unavailable");
    expect(web.picker).not.toHaveBeenCalled(); expect(web.createObjectURL).not.toHaveBeenCalled();
  });
});
