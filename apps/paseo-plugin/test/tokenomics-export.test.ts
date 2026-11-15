import { Blob } from "node:buffer";
import { afterEach, describe, expect, it, vi } from "vitest";
const platform = vi.hoisted(() => ({ os: "web" }));
vi.mock("react-native", () => ({ Platform: { get OS() { return platform.os; } } }));
import { csvFileMode, saveCsvFile } from "../client/web";

const csv = { filename: "tokenomics-pivot.csv", content: 'Group,Tokens\r\n"Synthetic, team",73000\r\n"Ångström",12\r\n' };
afterEach(() => { platform.os = "web"; vi.unstubAllGlobals(); vi.useRealTimers(); });

describe("Tokenomics client file export", () => {
  it("opens the picker during the press and writes the complete unchanged UTF-8 text before reporting saved", async () => {
    const order: string[] = [], writer = { write: vi.fn(async () => { order.push("write"); }), close: vi.fn(async () => { order.push("close"); }), abort: vi.fn() };
    const picker = vi.fn(async () => { order.push("picker"); return { createWritable: async () => writer }; });
    vi.stubGlobal("showSaveFilePicker", picker);
    const load = vi.fn(async () => { order.push("load"); return csv; });
    const pending = saveCsvFile(csv.filename, load);
    expect(order).toEqual(["picker"]); expect(csvFileMode()).toBe("picker");
    await expect(pending).resolves.toBe("saved");
    expect(order).toEqual(["picker", "load", "write", "close"]);
    expect(picker).toHaveBeenCalledWith({ suggestedName: csv.filename, types: [{ description: "CSV", accept: { "text/csv": [".csv"] } }] });
    expect(writer.write).toHaveBeenCalledExactlyOnceWith(csv.content); expect(writer.abort).not.toHaveBeenCalled();
  });
  it("does not read an export after the user cancels the picker", async () => {
    const load = vi.fn(); vi.stubGlobal("showSaveFilePicker", vi.fn().mockRejectedValue(new DOMException("Cancelled", "AbortError")));
    await expect(saveCsvFile(csv.filename, load)).resolves.toBe("cancelled"); expect(load).not.toHaveBeenCalled();
  });
  it("propagates permission refusal without silently downloading elsewhere", async () => {
    const load = vi.fn(); vi.stubGlobal("showSaveFilePicker", vi.fn().mockRejectedValue(new DOMException("Denied", "NotAllowedError")));
    await expect(saveCsvFile(csv.filename, load)).rejects.toThrow("Denied"); expect(load).not.toHaveBeenCalled();
  });
  it("does not open a writable file when the complete source changed or failed", async () => {
    const createWritable = vi.fn(); vi.stubGlobal("showSaveFilePicker", vi.fn(async () => ({ createWritable })));
    await expect(saveCsvFile(csv.filename, async () => { throw new Error("csv_snapshot_changed"); })).rejects.toThrow("csv_snapshot_changed");
    expect(createWritable).not.toHaveBeenCalled();
  });
  it.each(["write", "close"] as const)("aborts a failed %s and preserves its failure even when abort also fails", async (step) => {
    const failure = new Error("Synthetic write failure"), writer = { write: vi.fn().mockResolvedValue(undefined), close: vi.fn().mockResolvedValue(undefined), abort: vi.fn().mockRejectedValue(new Error("Abort failed")) };
    writer[step].mockRejectedValue(failure); vi.stubGlobal("showSaveFilePicker", vi.fn(async () => ({ createWritable: async () => writer })));
    await expect(saveCsvFile(csv.filename, async () => csv)).rejects.toBe(failure); expect(writer.abort).toHaveBeenCalledOnce();
  });
  it("hands the entire CSV to the download manager with its filename and MIME type, without claiming a save", async () => {
    vi.useFakeTimers(); vi.stubGlobal("showSaveFilePicker", undefined); vi.stubGlobal("Blob", Blob);
    const link = { href: "", download: "", rel: "", click: vi.fn(), remove: vi.fn() }, appendChild = vi.fn(), createObjectURL = vi.fn((_blob: Blob) => "blob:synthetic"), revokeObjectURL = vi.fn();
    vi.stubGlobal("document", { body: { appendChild }, createElement: vi.fn(() => link) }); vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
    expect(csvFileMode()).toBe("download");
    await expect(saveCsvFile(csv.filename, async () => csv)).resolves.toBe("requested");
    const blob = createObjectURL.mock.calls[0]![0];
    expect(await blob.text()).toBe(csv.content); expect(blob.type).toBe("text/csv;charset=utf-8"); expect(link.download).toBe(csv.filename);
    expect(appendChild).toHaveBeenCalledWith(link); expect(link.click).toHaveBeenCalledOnce(); expect(link.remove).toHaveBeenCalledOnce();
    expect(revokeObjectURL).not.toHaveBeenCalled(); await vi.advanceTimersByTimeAsync(60_000); expect(revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:synthetic");
  });
  it("reports download dispatch errors and still removes the link and releases its URL", async () => {
    vi.useFakeTimers(); vi.stubGlobal("showSaveFilePicker", undefined); vi.stubGlobal("Blob", Blob);
    const remove = vi.fn(), revokeObjectURL = vi.fn(); vi.stubGlobal("document", { body: { appendChild: vi.fn() }, createElement: () => ({ remove, click() { throw new Error("Download refused"); } }) });
    vi.stubGlobal("URL", { createObjectURL: () => "blob:synthetic", revokeObjectURL });
    await expect(saveCsvFile(csv.filename, async () => csv)).rejects.toThrow("Download refused"); expect(remove).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(60_000); expect(revokeObjectURL).toHaveBeenCalledOnce();
  });
  it.each(["ios", "android"])("never touches file APIs or loads content on %s", async (os) => {
    platform.os = os; const picker = vi.fn(), load = vi.fn(); vi.stubGlobal("showSaveFilePicker", picker);
    expect(csvFileMode()).toBeNull(); await expect(saveCsvFile(csv.filename, load)).rejects.toThrow("unavailable");
    expect(picker).not.toHaveBeenCalled(); expect(load).not.toHaveBeenCalled();
  });
});
