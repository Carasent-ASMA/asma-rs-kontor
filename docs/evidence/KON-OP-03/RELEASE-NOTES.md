# KON-OP-03 / ASMA-7872 — release notes

Date: 2026-08-17
Code revision: `6fb3736899d13e5185427586a2336a9a0c19359d`
Evidence revision: `13bbcb4e2cfc875a024824b35a737b177d679f3d`
Superproject gitlink: `67779fd13e11ae8d2bcce738af8e9cb0651f2318`

## Verdict

**PASS for release-gate evaluation.** The verifier records release evidence only;
the frozen work profile reserves the release-gate verdict for `architect`.

Independent UAT at the revisions above passed:

| Check | Result |
| --- | --- |
| `cargo test -p kontor-daemon --test loopback_api` | 141 passed, 0 failed |
| `cargo test -p kontor-api --test error_envelope` | 5 passed, 0 failed |
| `cargo test -p kontor-runtime --test retitle_container` | 4 passed, 0 failed |

The tester's broader `QA-ROUND-2.md` evidence also reports the full workspace
suite, clippy and formatting green at the same code revision.

## Released behavior

- Named `/v1` application operations, OpenAPI and the shared MCP/CLI registry
  now expose the OP-03 contract through one server-authoritative boundary. The
  generic `/v1/commands/{kind}` route is removed.
- Admins can draft, validate, publish and read immutable topology
  specifications, then preview and apply an epic pin upgrade. Role-catalog and
  server-owned code-help reads resolve exact revisions and never guess unknown
  codes.
- Semantic topology operations accept controlled scopes rather than
  caller-authored kinds, parents, native names, ids or paths. Ensure,
  materialize and retire reuse OP-02's exact-id, capability-dispatched path.
- Native capacity observations are stored separately from derived availability
  and operator overrides. Kontor no longer shells out to `asma fleet` for this
  path. Persisted adaptive admission starts at four, grows one slot after two
  distinct clean observations, and remains independently capped at seven
  active TeamRuns.
- API refusals now name a safe subject, structural path and corrective action
  without echoing rejected values. Credential-like prefixes are detected at
  token boundaries, so ordinary words containing `sk-` or `akia` are accepted
  while credential-shaped tokens remain refused.
- New task-scoped Paseo workspaces derive their visible title from the typed
  Jira/task scope rather than a topology-node id. The runtime contract now has
  an explicit identity-preserving `RetitleContainer` capability and truthful
  unsupported-capability behavior.

## Data and compatibility

The database schema advances to **29**:

- `0028_native_capacity.sql` stores raw capacity observations, derived state,
  overrides and adaptive admission state.
- `0029_topology_publication.sql` adds immutable topology publication and the
  explicit, receipt-backed epic pin upgrade path.

Existing stable API error codes remain unchanged. New refusal fields are
additive. Published specification revisions remain immutable; only an epic's
pin may move through the authorized preview/apply operation.

## Deliberate limitations

- The 27 OP-04/05/06 successor contracts for Core Team, Quick work, promotion,
  Advisor/Committee and Completion behavior return typed `unavailable` until
  their owning application services are composed. They do not report empty
  success.
- Advisor and Committee semantic topology scopes remain unavailable until
  their run aggregates exist.
- No `/v1` retitle preview/apply operation ships here. Existing workspace
  `wks_5afdf8a83682cc8e` remains `rename_pending`; its identity, parent, cwd and
  bindings are correct, but its visible title is stale. Newly created task
  workspaces use the corrected title.
