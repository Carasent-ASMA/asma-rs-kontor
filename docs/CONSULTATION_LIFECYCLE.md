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
so `codebase-memory-mcp.required` is false. The TOML edit preserves comments and
all other MCP requirements. Symlinked homes and configs are not modified. Memory
is an optional accelerator; its handshake can still time out under resource
pressure, but that failure no longer makes the Codex session itself fatal.
`toml_edit` is pinned to the already locked version to preserve operator config
without rewriting unrelated settings or adding a second TOML parser.

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
