# Consultation permissions and session release

ASMA-8111 exposed two independent failures: Claude consultation seats ended in
plan approval instead of submitting a finding, and idle native sessions held
codebase-memory-mcp's shared daemon connections until new Codex seats failed
their required MCP handshake.

Claude consultations now use `default` mode with a Kontor `PreToolUse` guard.
The guard permits `Read`, `Glob`, `Grep`, `ToolSearch`, and exactly the four
operations in the registry's `consultation` serve profile. Every other tool is
denied immediately, including shell execution, file edits, delegation and plan
transitions. The allowed finding writes still require the native occupant's
generation-fenced credential; tool approval grants no additional API authority.
Consultation prompts include the exact project, run, round and revision. Scoped
GETs verify the current native occupancy and return only that seat and its own
findings; peers, stale generations and unbound seats are refused. Judges receive
authorized reviewer evidence in their frozen launch prompt.
Paseo receives the scoped MCP server and exact tool approvals in the create
request. The launch payload uses Paseo's `stdio` MCP transport with a command
string and separate argument array. Codex consultations explicitly receive a read-only sandbox and `never`
approval policy. Delivery and persistent leadership modes remain unchanged.

The Claude guard is composed in the consultation worktree and excluded from
Git. Unknown settings and other hooks are preserved. The companion `kontor-mcp`
must pass the guard protocol probe before launch; disabling seat MCP composition
therefore refuses new Claude consultations rather than launching without their
permission boundary. The containment intentionally provides file-reading tools,
not arbitrary shell commands. Existing sessions retain their original launch
configuration until they are recovered through the supported seat lifecycle.

On startup, Kontor reconciles **owned** `provider-homes/codex*/config.toml` files
so `codebase-memory-mcp.required` is false and an existing `mcp_servers.kontor`
entry whitelists the name `KONTOR_AUTH` for its stdio child. The credential value
is never written to configuration: Paseo carries it only in the native session
environment, while Codex's `env_vars` setting forwards that inherited value to
the scoped MCP process. The TOML edit preserves comments, existing environment
names and all other MCP requirements. Symlinked homes and configs are not
modified. Memory is an optional accelerator; its handshake can still time out
under resource pressure, but that failure no longer makes the Codex session
itself fatal. `toml_edit` is pinned to the already locked version to preserve
operator config without rewriting unrelated settings or adding a second TOML
parser.

Schema 93 adds `consultation_session_releases`. Once startup reconciliation
opens the barrier, a resident scanner checks every 30 seconds, planning at most
16 releases per pass. Only durably **settled** Advisor and Committee runs
qualify. Idle age, a finished-looking transcript, missing runtime contact or a
recorded reviewer finding alone never authorizes release. This also discovers
settled sessions left resident by older versions.

Each release freezes the full native runtime identity before dispatch. The
adapter verifies both the run and SeatBinding labels, archives through Paseo's
supported interface, and reads back the archive stamp before confirmation. A
lost response or restart leaves a pending effect; replay inspects first and
does not re-archive an already archived session. Archive readback falls back to
Paseo's paginated include-archived directory when exact active lookup hides the
retired agent, while retaining exact native, run and seat identity checks. Attempts rotate by their last
attempt time so an unavailable runtime does not starve later releases. Findings,
run outcomes, logical seat bindings and native history are retained.

An interrupted seat recovery retains its original route and occupant fence. A
retry compares against the revision it read, accounting for the initial prepare
increment, so a peer finding or topic correction does not strand the attempt.
Progress during native effects still fails the compare-and-swap and is retried.

Install `kontor-daemon`, `kontor` and `kontor-mcp` from the same candidate. Save
the old binaries and a verified SQLite backup before restart. Rolling back this
migration requires the matching pre-upgrade database snapshot; an older binary
must not be pointed at schema 93 or forced past its schema-version check.

Validation includes registry-bound tool denial tests, real Claude read/finding
and denied-write probes, exact native correlation and archive replay contracts,
and storage tests covering restart, bounded batches and immutable settled facts.

## Governed planning pairs (ASMA-8282)

`planning_pair@1` is the third closed consultation family. It is an opt-in
protocol inside this domain: Advisor and Committee identity bytes, endpoints,
schema and historical migrations are unchanged, and no other domain is bound to
it. This is a domain contract decision (LSA disposition D-1 to D-3,
2026-10-02), not a platform ADR. The ASMA-8113 owner's compatibility review
fences acceptance of the widened identity vocabulary.

Schema 122 widens the family checks to `advisor`, `committee` and
`planning_pair` and conditions run state on the family. Advisor and Committee
keep exactly their earlier states. A planning pair is only `materializing`,
`running`, `needs_human` or `disposed`, with `result` and `settled_at` always
NULL, so it can never be settled, judged or read as a verdict. Its run shares
the semantic identity function and unique index with the other families. Its
placement receipt, canonical record revisions and member contributions live in
immutable tables keyed by the run, and `disposed` is terminal. Rolling schema
122 back requires the pre-upgrade database snapshot.

Nine registered operations cover it: the profile list, preview and apply; run
invoke and get; a member's finding and answer; and the caller's one
clarification and disposition. The registry tier is a floor. The caller is the
exact frozen caller seat at its current hosted generation. A member is the exact
frozen member seat at its current occupancy generation, and its slot comes from
its credential, never from the body. Ambient Admin and Operator credentials
reach only the catalog and an observer projection that carries no contribution.
Authentication precedes the idempotent replay, so a retired generation never
regains authority by repeating its key. Every write carries the expected run
revision. The caller sees findings and answers only once the domain releases
them. A member sees only its own words, and its serve profile,
`planning_pair_member`, holds exactly the run read, the finding and the answer.

Names and slots come only from explicit configuration. The published document
selects a `container_kind`, which the epic's pinned Team Definition must declare
read-only with exactly the display-named slots `seat-a` (`SEAT A`) and `seat-b`
(`SEAT B`), and it names each member's registered `role_code`. That code must
be a current role of the catalog the epic's frozen roster selected, read from
its persisted bytes under the roster's `catalog_hash` pin. No other catalog the
realm holds is consulted, and the store re-proves the role when it freezes the
seats. The bundled
pack declares no such container, so a realm invokes a pair only after
publishing one. The members are placed by the shared allocator on one activated
snapshot, on distinct actual vendors or not at all.

A member's surface is exactly three tools, `kontor_planning_pair_run_get`,
`kontor_planning_pair_findings_record` and `kontor_planning_pair_answer_record`.
That one list, `kontor_core::planning_pair::MEMBER_MCP_TOOLS`, generates the
registry's `planning_pair_member` serve profile, the guard's allowlist and the
runtime's creation-time tool policy. The guard runs as
`kontor-mcp --consultation-tool-guard --serve-profile planning_pair_member`.

- It permits `Read`, `Glob`, `Grep`, `ToolSearch` and exactly those three tools
  under the `mcp__kontor__` prefix. No consultation tool is ever added, and no
  fallback is taken.
- Without the profile argument the guard is the Advisor and Committee surface,
  unchanged.
- Any other argument list denies every tool.

The runtime port's `validate_planning_pair_member_surface` is asked about both
actual placed routes, once before the pair is frozen and again before its
container is prepared. Its answer is a typed refusal per route.

Through Paseo, every real route is refused today, before the launch claim and
before any plane call, composed file, process or session:

- **Claude** (`restriction_unacknowledged`): its member guard, serve profile
  and creation restriction are composable. But Paseo acknowledges no applied
  closed tool restriction for a created session. `providerOptionsApplied` is an
  optional flag about OpenCode provider options, not the session's exact tools,
  guard or ambient MCP exclusion. That composition is proved only by
  constructing it directly on source fixtures.
- **Codex** (`closed_tools_unavailable`): its sandbox and `never` approval are
  not a closed tool restriction.
- **Cursor and OpenCode** (`read_only_unenforced`): `plan` is behavioral.
- **Any other provider** (`not_composed`).

Each member launch carries a typed, non-secret context that the service derives
from durable state, and an Advisor or Committee launch must carry none. The
context names:

- the run, SeatBinding and slot;
- the seat's own occupancy generation;
- the document, topology, Team Definition and role catalog pins;
- the container and cwd;
- the frozen route and actual vendor;
- the placement and the requested fleet provenance.

The credential stays in the runtime's process-environment channel.

A member is bound, and so may contribute, only when every mandatory
member-surface field is observed as matched: its correlation, its route and its
closed tool restriction. Its readback must also observe exactly its requested
provenance. A missing observation, or any unmatched or unsupported field, is a
typed refusal: the native session is kept unbound and named as confirmation
unknown, the pair stays materializing with no receipt, and a replay meets that
same session. Account authority is never observed and is stated separately; an
account-qualified label is not credential ownership.
A pair's caller may be served under a distinct, opt-in profile,
`planning_pair_caller`. It has exactly four tools:

- `kontor_planning_pair_run_get`;
- `kontor_planning_pair_run_invoke`;
- `kontor_planning_pair_clarification_request`;
- `kontor_planning_pair_disposition_record`.

They are generated from `kontor_core::planning_pair::CALLER_MCP_TOOLS`.

The profile is selected only by naming it, as
`kontor-mcp --serve-profile planning_pair_caller`. An omitted profile is the
tier's whole surface, as before. Nothing infers the caller profile from a
seat's role, title or pinned document, and no hosted seat is composed with it:
the leadership profile, and the hosted leadership MCP built from it, are
unchanged. The caller profile is not a guard profile; a guard named with it
denies every tool.

Serving the profile grants nothing. The daemon still requires the exact frozen
caller seat at its current hosted generation, under the document's allowed
roles. A member, a TPM seat and an ambient Admin or Operator are refused under
it, and the caller cannot contribute under the member profile.

None of this is a live qualification. Member seat recovery is not yet
implemented for this family.
