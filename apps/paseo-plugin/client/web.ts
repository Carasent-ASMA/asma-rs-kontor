import { Platform } from "react-native";

type EscapeKey = { key: string; defaultPrevented?: boolean; preventDefault(): void; stopPropagation(): void };
type KeyDocument = {
  addEventListener(type: "keydown", listener: (event: EscapeKey) => void): void;
  removeEventListener(type: "keydown", listener: (event: EscapeKey) => void): void;
};

/** Web Escape complements native Back; a nested disclosure consumes it first. */
export function onEscape(close: () => void): () => void {
  if (Platform.OS !== "web") return () => {};
  const document = (globalThis as { document?: KeyDocument }).document;
  const listener = (event: EscapeKey) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault(); event.stopPropagation(); close();
  };
  document?.addEventListener("keydown", listener);
  return () => document?.removeEventListener("keydown", listener);
}

type CsvFile = { filename: string; content: string };
type CsvWriter = { write(content: string): Promise<void>; close(): Promise<void>; abort(): Promise<void> };
type DownloadLink = { href: string; download: string; rel: string; click(): void; remove(): void };
type FileGlobals = {
  showSaveFilePicker?: (options: { suggestedName: string; types: { description: string; accept: Record<string, string[]> }[] }) => Promise<{ createWritable(): Promise<CsvWriter> }>;
  document?: { body: { appendChild(link: DownloadLink): void }; createElement(tag: "a"): DownloadLink };
  Blob?: new (parts: string[], options: { type: string }) => unknown;
  URL?: { createObjectURL(blob: unknown): string; revokeObjectURL(url: string): void };
};

/** Electron uses the same web download path as Paseo's file explorer; native keeps Copy/Share. */
export function csvFileMode(): "picker" | "download" | null {
  if (Platform.OS !== "web") return null;
  const web = globalThis as unknown as FileGlobals;
  if (typeof web.showSaveFilePicker === "function") return "picker";
  return web.document?.body && typeof web.document.createElement === "function" && web.Blob && typeof web.URL?.createObjectURL === "function" && typeof web.URL.revokeObjectURL === "function" ? "download" : null;
}

export async function saveCsvFile(suggestedName: string, load: () => Promise<CsvFile>): Promise<"saved" | "cancelled" | "requested"> {
  const mode = csvFileMode();
  if (!mode) throw new Error("Client file export is unavailable.");
  const web = globalThis as unknown as FileGlobals;
  if (mode === "picker") {
    // Open during the press, before awaiting the complete export, to retain user activation.
    let handle: Awaited<ReturnType<NonNullable<FileGlobals["showSaveFilePicker"]>>>;
    try { handle = await web.showSaveFilePicker!({ suggestedName, types: [{ description: "CSV", accept: { "text/csv": [".csv"] } }] }); }
    catch (error) { if (error instanceof Error && error.name === "AbortError") return "cancelled"; throw error; }
    const value = await load(), writer = await handle.createWritable();
    try { await writer.write(value.content); await writer.close(); }
    catch (error) { try { await writer.abort(); } catch { /* Keep the original write failure. */ } throw error; }
    return "saved";
  }
  const value = await load(), url = web.URL!.createObjectURL(new web.Blob!([value.content], { type: "text/csv;charset=utf-8" }));
  try {
    const link = web.document!.createElement("a");
    link.href = url; link.download = value.filename; link.rel = "noopener";
    try { web.document!.body.appendChild(link); link.click(); } finally { link.remove(); }
  } finally {
    // The browser can still be reading the Blob while its save prompt is open.
    setTimeout(() => web.URL!.revokeObjectURL(url), 60_000);
  }
  return "requested";
}
