import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { open, realpath, stat } from "node:fs/promises";
import { isAbsolute } from "node:path";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { z } from "zod";
import { sourceSummarySchema, type ReadSourceSummary } from "../shared/read-sources";
import { readTeamsSource, teamsSourceFormatSchema, teamsSourceRequestSchema, teamsSourceSnapshotSchema,
  type TeamsSourceFormat, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import type { SourceCatalog } from "./read-sources";
import { projectTeamsFleet, projectTeamsRoster, projectTeamsTemplates } from "./teams-source-projection";
import { decodeTeamsConfigYaml } from "./teams-source-yaml";

const MAX_BYTES = 2 * 1024 * 1024;
const CONFIGURATION_STATE = {
  "fleet-authoring": "unactivated-authoring", "fleet-operator": "operator-file-snapshot",
  "core-roster": "core-roster-snapshot", "team-templates": "bundled-template-defaults",
} as const;
const catalogEntry = sourceSummarySchema.extend({
  id: z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/), kind: z.literal("teams"),
  path: z.string().refine(isAbsolute, "Teams source path must be absolute"),
});

export class TeamsSourceError extends Error {
  constructor(readonly code: string, message: string) { super(message); this.name = "TeamsSourceError"; }
}

async function readBoundedSource(source: ReadSourceSummary): Promise<{ bytes: Buffer; modifiedAt: string }> {
  const resolved = await realpath(source.path);
  // Non-blocking open prevents a catalog FIFO from hanging before the regular-file check.
  const handle = await open(resolved, constants.O_RDONLY | constants.O_NONBLOCK);
  try {
    const before = await handle.stat();
    if (!before.isFile() || before.size > MAX_BYTES) throw new TeamsSourceError("source-size-or-type", "Source must be a regular file of at most 2 MiB");
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    let offset = 0;
    while (offset < buffer.length) {
      const { bytesRead } = await handle.read(buffer, offset, buffer.length - offset, offset);
      if (!bytesRead) break;
      offset += bytesRead;
    }
    if (offset > MAX_BYTES) throw new TeamsSourceError("source-size-or-type", "Source exceeds 2 MiB");
    const after = await handle.stat();
    const current = await stat(resolved);
    if (before.size !== offset || before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs
      || before.ino !== current.ino || before.dev !== current.dev || await realpath(source.path) !== resolved) {
      throw new TeamsSourceError("source-changed", "Source changed during read; retry this source ID");
    }
    return { bytes: buffer.subarray(0, offset), modifiedAt: after.mtime.toISOString() };
  } finally { await handle.close(); }
}

async function readSnapshot(source: ReadSourceSummary, format: TeamsSourceFormat): Promise<TeamsSourceSnapshot> {
  const result: TeamsSourceSnapshot = { contractVersion: 1, source, format, status: "unavailable", sha256: null,
    readAt: new Date().toISOString(), modifiedAt: null, activation: "not-attested",
    configurationState: CONFIGURATION_STATE[format], data: null, diagnostics: [] };
  try {
    const { bytes, modifiedAt } = await readBoundedSource(source);
    result.sha256 = createHash("sha256").update(bytes).digest("hex");
    result.modifiedAt = modifiedAt;
    if (source.expectedSha256 && result.sha256 !== source.expectedSha256) {
      throw new TeamsSourceError("revision-mismatch", `Expected SHA-256 ${source.expectedSha256}, read ${result.sha256}`);
    }
    if (source.revisionKind === "approved revision" && !source.expectedSha256) {
      throw new TeamsSourceError("revision-unpinned", "An approved revision requires an expected SHA-256");
    }
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    if (format === "team-templates") {
      const projected = projectTeamsTemplates(JSON.parse(text), source.id);
      result.data = projected.data;
      result.diagnostics = projected.diagnostics;
    } else {
      const raw = decodeTeamsConfigYaml(text);
      if (format === "core-roster") result.data = projectTeamsRoster(raw, source.id);
      else {
        const projected = projectTeamsFleet(raw, source.id);
        result.data = projected.data;
        result.diagnostics = projected.diagnostics;
      }
    }
    result.status = result.diagnostics.length ? "partial" : "ready";
    return teamsSourceSnapshotSchema.parse(result);
  } catch (error) {
    // This is the source boundary: a failed file never prevents native observations
    // or another configured source from being displayed by composeTeamsSourceView.
    result.data = null;
    result.status = "unavailable";
    const code = error instanceof TeamsSourceError ? error.code : "source-read-or-decode";
    result.diagnostics = [{ code, pointer: "", message: `${source.id} (${source.path}): ${error instanceof Error ? error.message : String(error)}`.slice(0, 4096) }];
    return teamsSourceSnapshotSchema.parse(result);
  }
}

/** Freeze host-selected catalog entries/formats at construction. Neither RPC input nor
 * user preferences can supply a path, format, executable, native selection or credentials.
 * A imports this and binds only authorized teams IDs from sourceCatalog(); no new env var.
 * Empty formats means no file is read. Formats do not merge operator and authoring fleets.
 */
export function createTeamsSourceAdapter(catalog: SourceCatalog, formats: Readonly<Record<string, TeamsSourceFormat>>) {
  const sources = new Map<string, { source: ReadSourceSummary; format: TeamsSourceFormat }>();
  for (const [id, rawFormat] of Object.entries(formats)) {
    const matches = catalog.sources.filter((source) => source.id === id);
    if (matches.length !== 1) throw new TeamsSourceError("invalid-catalog", `Expected one configured teams source for ID ${id}`);
    const source = catalogEntry.parse(matches[0]);
    const format = teamsSourceFormatSchema.parse(rawFormat);
    sources.set(id, { source, format });
  }
  return {
    async read(input: unknown): Promise<TeamsSourceSnapshot> {
      const { sourceId } = teamsSourceRequestSchema.parse(input);
      const entry = sources.get(sourceId);
      if (!entry) throw new TeamsSourceError("unknown-source", "Source ID is not an authorized Teams adapter entry in the host catalog");
      return readSnapshot(entry.source, entry.format);
    },
  };
}

/** Opt-in registration only; this file does not change the plugin's existing handlers. */
export function contributeTeamsSourceAdapter(server: PluginServerContext, adapter: ReturnType<typeof createTeamsSourceAdapter>): void {
  server.handle(readTeamsSource, (input) => adapter.read(input));
}
