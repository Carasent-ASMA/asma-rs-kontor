import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
import { registerSummaryAttachments } from "../client/summary-attachments";
import { createSessionSummaryService, savedSummaryResources } from "../server/session-summaries";
import { savedSummaryAttachments, searchSummaryAttachments } from "../shared/summary-attachments";

const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });
async function saved() {
  const root = await mkdtemp(join(tmpdir(), "summary-attachment-")); dirs.push(root);
  const effects = vi.fn(() => { throw new Error("No provider/send/draft effect allowed"); });
  const api = { agents: { ref: () => ({ refresh: async () => ({ agent: { id: "source-a", title: "Parser" } }), send: effects, run: effects }), create: effects } };
  let cutoff = "2026-10-29T00:00:00Z";
  const service = createSessionSummaryService(api as never, root, { generate: async () => ({ cutoff, sections: { task: "Review parser", decisions: "Keep grammar", currentState: "Ready", nextStep: "Review" } }) });
  const revision = await service.save("source-a");
  return { root, service, revision, effects, next: () => { cutoff = "2026-10-29T01:00:00Z"; } };
}

describe("native summary attachment source", () => {
  it("registers the public source and returns its cleanup without performing a draft/send effect", () => {
    const remove = vi.fn(), addAttachmentSource = vi.fn(() => remove), addComposerPill = vi.fn();
    const registration = registerSummaryAttachments({ addAttachmentSource, addComposerPill } as never);
    expect(registration.available).toBe(true);
    expect(addAttachmentSource).toHaveBeenCalledExactlyOnceWith(savedSummaryAttachments);
    expect(addComposerPill).not.toHaveBeenCalled(); // existing Kontor pill owns the instructions
    registration.remove(); expect(remove).toHaveBeenCalledOnce();
  });
  it.each([{}, { addComposerPill() {} }, { addAttachmentSource() {} }, { addComposerPill() {}, addAttachmentSource() { throw new Error("unsupported"); } }])("stays unavailable when registration is absent or refused", client => {
    const registration = registerSummaryAttachments(client as never);
    expect(registration.available).toBe(false); expect(() => registration.remove()).not.toThrow();
  });
  it("returns exact immutable text with stable revision identity across later saves and searches", async () => {
    const h = await saved(); const first = await savedSummaryResources(h.root, "Parser");
    expect(first.items).toHaveLength(1);
    expect(searchSummaryAttachments.output.parse(first)).toEqual(first);
    expect(first.items[0]).toMatchObject({ id: `${h.revision.summaryId}:r1`, text: h.revision.text, resourceType: "kontor-summary-revision" });
    h.next(); await h.service.save("source-a");
    expect(await savedSummaryResources(h.root, `${h.revision.summaryId}:r1`)).toEqual(first);
    expect((await savedSummaryResources(h.root, "source-a")).items).toHaveLength(2);
    expect(h.effects).not.toHaveBeenCalled();
  });
  it("returns no choices for an empty store or unmatched search; search accepts no destination authority", async () => {
    const h = await saved(); expect(await savedSummaryResources(h.root, "unmatched")).toEqual({ items: [] });
    expect(await savedSummaryResources(join(h.root, "missing"), "")).toEqual({ items: [] });
    expect(() => searchSummaryAttachments.input.parse({ query: "Parser", destinationAgentId: "elsewhere" })).toThrow();
    expect(h.effects).not.toHaveBeenCalled();
  });
  it.each(["malformed", "empty", "inconsistent", "duplicate revision"])("refuses %s persisted summary without returning partial choices", async kind => {
    const h = await saved(); const file = join(h.root, (await readdir(h.root)).find(name => name.startsWith("source-"))!);
    const store = JSON.parse(await readFile(file, "utf8"));
    if (kind === "empty") store.revisions["1"].text = "";
    if (kind === "inconsistent") store.revisions["1"].sections.task = "Foreign task";
    if (kind === "duplicate revision") store.revisions["2"] = store.revisions["1"];
    await writeFile(file, kind === "malformed" ? "not JSON" : JSON.stringify(store));
    await expect(savedSummaryResources(h.root, "")).rejects.toThrow("No attachment choices were returned");
    expect(h.effects).not.toHaveBeenCalled();
  });
});
