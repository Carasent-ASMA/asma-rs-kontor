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
type DownloadLink = { href: string; download: string; rel: string; click(): void; remove(): void };
type FileGlobals = {
  document?: { body: { appendChild(link: DownloadLink): void }; createElement(tag: "a"): DownloadLink };
  Blob?: new (parts: string[], options: { type: string }) => unknown;
  URL?: { createObjectURL(blob: unknown): string; revokeObjectURL(url: string): void };
};

/** Electron uses the same web download path as Paseo's file explorer; native keeps Copy/Share. */
export function csvFileMode(): "download" | null {
  if (Platform.OS !== "web") return null;
  const web = globalThis as unknown as FileGlobals;
  return web.document?.body && typeof web.document.createElement === "function" && web.Blob && typeof web.URL?.createObjectURL === "function" && typeof web.URL.revokeObjectURL === "function" ? "download" : null;
}

export function downloadCsvFile(value: CsvFile): "requested" {
  if (!csvFileMode()) throw new Error("Client file export is unavailable.");
  const web = globalThis as unknown as FileGlobals;
  const url = web.URL!.createObjectURL(new web.Blob!([value.content], { type: "text/csv;charset=utf-8" }));
  let dispatched = false;
  try {
    const link = web.document!.createElement("a");
    link.href = url; link.download = value.filename; link.rel = "noopener";
    try { web.document!.body.appendChild(link); link.click(); dispatched = true; } finally { link.remove(); }
  } finally {
    // The browser can still be reading the Blob while its save prompt is open.
    if (dispatched) setTimeout(() => web.URL!.revokeObjectURL(url), 60_000);
    else web.URL!.revokeObjectURL(url);
  }
  return "requested";
}
