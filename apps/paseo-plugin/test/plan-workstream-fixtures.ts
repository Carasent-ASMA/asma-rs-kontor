import type { ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
export const source: ReadSourceSummary = { id: "plan", title: "Declared plan", kind: "plan", path: "/docs/plan.md", epic: "ASMA-7575", workstream: null, projectId: null, revisionKind: "working copy" };
export const read: SourceRead = { source, status: "ready", revision: "a".repeat(64), readAt: "now", modifiedAt: null, content: "---\nworkstream: adopus-graphql\n---\n# Plan", graph: null, records: null, diagnostics: [], planMetadata: { workstream: "adopus-graphql", epic: "ASMA-7575", diagnostics: [] } };
export const registry: WorkstreamRegistry = { path: "/registry.json", revision: "b".repeat(64), workstreams: { "adopus-graphql": { name: "Ad Opus GraphQL" }, access: { name: "Access & login" } }, desks: { "pr-review-desk": { name: "PR Review Desk" } }, diagnostics: [] };

