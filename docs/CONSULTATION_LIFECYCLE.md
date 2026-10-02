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
(`SEAT B`), and it names each member's registered `role_code`. The bundled
pack declares no such container, so a realm invokes a pair only after
publishing one. The members are placed by the shared allocator on one activated
snapshot, on distinct actual vendors or not at all.

No shipped runtime composes the member surface yet: its serve profile, the
consultation guard and observed provenance. The runtime port's
`validate_planning_pair_member_surface` refuses by default and is asked before
anything is frozen or prepared. A governed invocation therefore answers
`unsupported_capability` with no native effect until an adapter proves that
surface. Member seat recovery is not yet implemented for this family.
