# KON-OP-22 implementation

Date: 2026-09-03

Status: delivered to `master` and verified live.

## Delivered behavior

- Jira convergence is resident. After the startup reconciliation barrier it
  scans confirmed task and epic bindings on committed control-plane wakes and
  on a bounded 30-second backstop.
- Task workflow selection is exact to the task's frozen work-profile revision.
  Epic workflow selection is generic and epic-specific; an epic never borrows
  a task profile.
- The Jira transport is entity-neutral and binds apply authority to the exact
  observed issue, live transition route, projection revision, specification
  hashes and intent. An applied result is accepted only with confirmed
  refetch evidence.
- Confirmed Jira task identities resolve through one canonical ledger. Legacy
  aliases remain migration input, not a second active identity.
- Epic conflicts and transition intents are first-class, append-only evidence
  with closed conflict kinds, timestamp and identifier checks, receipt
  references, and one-shot resolution.
- Replayed conflicts and failed external applies do not create an immediate
  self-wake loop; the bounded backstop owns retry.
- Jira milestone rules may declare an exact, acyclic status route. Kontor
  selects a hop only when the observed status matches one route source exactly,
  persists the actual hop destination in the intent, and requires exact
  destination readback before confirming it. Missing, ambiguous, cyclic or
  non-terminating routes fail closed.
- The generic ASMA epic workflow is revision 2. It owns the verified Jira route
  `New (10227)` -> `DRAFT (10237)` -> `TO BE GROOMED (10236)` ->
  `Groomed (10233)` -> `READY FOR DEVELOPMENT (10213)` ->
  `In Development (10214)`. Revision 1 remains readable and hash-stable for
  historical installations, but is no longer bundled as the current epic
  workflow.
- Completion creation or advancement, any derived forward profile, all TPM
  wake intents and the local command receipt commit atomically.
- Ticket work added or reopened after an epic leaves the ticket phase,
  including after `Done` or `NeedsHuman`, deterministically starts a new
  completion generation and returns the epic to `Tickets`. Prior integration,
  deliberation, remediation and closeout evidence remains immutable history
  attributed to its original generation.
- Startup and resident completion scans discover returned work without an
  operator command.
- Interrupted Jira materialization can resume an identical mixed Link/Create
  request, not only a link-only request. When legacy attempts split the exact
  ordinal set across several pending batches, recovery requires one complete,
  non-overlapping cover and retains each original batch in the immutable
  recovery ledger without rewriting item ownership. It never creates a
  replacement Jira issue merely to escape durable local history.
- Jira issue creation includes bounded, project-configured additional fields
  per issue kind while preserving Kontor's exclusive ownership of project,
  type, summary, description, marker labels and parent. This lets ASMA supply
  its required Product option without embedding an ASMA custom-field id in the
  generic connector.
- Jira 400 validation failures are classified as schema mismatch and report
  only safe rejected field identifiers. Jira error prose is never reflected.

## Schema

- v81: canonical Jira task-link ledger and exclusive active identity.
- v82: epic Jira conflict and transition-intent evidence.
- v83: completion-generation attribution for remediation proposals and replay
  claims, with generation-one backfill.

All migrations are append-only and covered by empty-database, deployed-lineage,
direct-SQL constraint, restart, export and preservation tests.

## Delivery receipt

- Implementation commit: `1951cac3102197ef46b5547be4f41088a1d42572`.
- PR: `https://github.com/Carasent-ASMA/asma-rs-kontor/pull/156`.
- Merge commit: `7c27f4d7a8e2aa37c1b1ddc576fe60387e95cf47`.
- The complete clean-archive verifier passed against the merge commit, including
  a byte-identical regenerated `Cargo.lock`.
- The live `kontor`, `kontor-daemon` and `kontor-mcp` hashes match the release
  build. LaunchAgent `com.asma.kontor.daemon` restarted as PID `18681` on
  `127.0.0.1:7717`; schema v83, `PRAGMA integrity_check = ok` and an empty
  foreign-key check were read back.
- Generic ASMA epic workflow revision 2 installed with receipt
  `01a067cf-beda-72c2-ac30-6042125a1f89`, project revision 5 and definition hash
  `21b1a100d832d688fbf99c4140f63aac8c8f7d9980aa1e7174288a3c2cf0c40e`.
- The resident controller moved `ASMA-8049` from `DRAFT` through the four
  remaining configured hops to `In Development`. All four distinct revision-2
  intents carry exact confirmed readback; a later backstop created no duplicate
  intent or conflict.
- `ASMA-8050` and `ASMA-8062` each return `converged: true` with an empty diff
  through the supported Kontor reconciliation plan.
- Historical revision-1 conflict
  `01a06761-49a2-7832-a11c-2b91e491a9a4` was resolved only after target
  confirmation, by receipt `01a067d0-6c17-7d02-a46d-602f57b1e5f3`.
- Final Jira create-contract commit
  `7d13379999fe6f9aa45ba7fd00a85bbc7311741d` merged through PR #162 as
  `d224b153bd6430f1df37e6c9a96a59cec9ab17b0`. Its complete local release
  verifier passed; the merge tree was identical to the verified head.
- The exact merge daemon hash
  `248e7a385a5cafcaa990087b9d7f1e42fb914348a5506b34d35caf0f180b8744`
  runs as PID `43055`. Its rollback unit is
  `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T232131Z-kon-op-22-d224b15/`.
- The recovered materialization created `ASMA-8088`, `ASMA-8089` and
  `ASMA-8090` once. Exact replay returned receipt
  `01a0683f-7579-7d82-90b7-00781902f8b3` and activation
  `01a06994-ff6e-7501-82d5-1199259eea08` unchanged.
- All 21 task Jira reconciliation plans for `ASMA-7869` are converged with
  empty diffs. The epic and three new tasks are `In Development`; the epic has
  no unresolved Jira conflict.

## Explicit adjacent scope decision

The pre-existing open-question authoring/disposition API and MCP surface is not
part of KON-OP-22. Completion continues to read and enforce any persisted open
questions, and the live realm currently has none. This delivery neither removes
nor disguises that separate control-surface gap.

## 2026-09-06 naming-migration recovery correction

The live KOP migration exposed one final lifecycle case not covered by the
original delivery: an exact Advisor native was already archived while its
logical SeatBinding remained active and its immutable migration target was
`rename_pending`. The generic migration fence correctly prevented retirement,
but that also prevented the only governed path to remove the non-live subject
from the live census.

The correction adds exact persistent-seat lifecycle inspection to the runtime
contract and Paseo adapter, then admits one atomic store operation for the exact
SeatBinding and exact in-flight migration only after fresh readback proves the
frozen native identity is archived or missing. Live natives and all generic
lifecycle calls remain fenced. Confirmation compares the migration against the
complete still-live native census; a retired historical target remains
immutable evidence and is never relabelled as a successful rename. Replaying
the same migration key reuses its original intent while freshly planning only
the remaining live subjects.

Local focused store, Paseo and daemon contracts pass, daemon compilation and
formatting pass, and an inverted lifecycle-state mutation was killed. This
section records source behavior only: no production binary was built or
installed, no daemon was restarted, and the live KOP migration remains paused
until the designated runtime owner deploys the merged newest-master artifact
and returns an exact health/readback checkpoint.

Source correction PR `#187` merged on current master as
`4127257d2b44a59032458acb981774bfcdc9ef78`, with prior master
`b2b5cad6e920fca7e5282873faef45da61e7328c` as its first parent and verified
feature head `b8a48382824cf6d05cc7664805a63f8867ce9c14` as its second parent.
Deployment and live migration are deliberately not claimed by this receipt.

Deployed master `c4b92f434f56eb511bb40c607525f6d302cfb048`
subsequently proved one narrower compatibility gap without changing live state:
the archived Advisor retained its predecessor workspace while the recovered
ASW and desired migration target named its successor workspace. Exact lifecycle
inspection now keeps native identity and provider-session correlation
unconditional, requires the desired parent only while the native is live, and
accepts the predecessor parent only as archived history. The strengthened
Paseo boundary test failed before this correction, passes after it, and killed
the inverted parent-state mutant. This follow-up is source-only until a later
newest-master deployment receipt explicitly says otherwise.

## 2026-09-06 final live naming-migration closeout

The source-only checkpoint above is superseded. PR #190 merged the archived
historical-parent compatibility correction as
`98940604d9a554aa24e0d87474bf0807232499e5`, and the designated runtime owner
deployed that exact master. The installed daemon is byte-identical to the
detached release build (`fbecb4715218169ce1377ecfb5a9cc72fd9453552d1dc3b2072c8af872bb90d9`),
runs as PID `5040`, serves realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`,
and read back schema v88, `integrity_check = ok`, zero foreign-key violations,
and preserved project revision 6. The coherent deployment receipt is
`/Users/igor/.local/state/kontor/asma/deploy-backups/20260905T221625Z-asma-8090-8100-9894060/deployment.json`.

From that exact safe-resume checkpoint, Kontor replayed retirement key
`asma-8090-retire-archived-advisor-after-c4b92f4` for SeatBinding
`01a02d6e-4dba-7ab2-9534-cc1b20df6917`. Fresh runtime readback proved archived
native `64233745-6091-4b8d-a184-407c785dac0e`; receipt
`01a073a6-b2ce-7eb1-b145-4e4cbb158622` retired the logical seat at revision 3
without changing ASW node `01a02d6e-4db9-7372-b2b8-c815da222dc5` or recovered
workspace `wks_6b2fdfdfff62cd89`.

The original migration key `01a07350-8090-7a11-8b22-123456789abc` and original
preview hash `2919a00526776b78e71bb7301722e7686073e176b3dde2ef7de7ad732ca794cb`
then replayed idempotently. Receipt `01a073a8-5d2e-7c53-81c1-a8d352d067bc`
confirmed project revision 6 on Team Definition
`01936f5a-2000-7000-8000-000000000001` v2, hash
`31cdff80e27cbe1e4043e150d2cdbc43ff79a8fa7fd8d892d2b5049a600f2e13`.
The replay changed zero already-correct live titles; fresh preview and readback
reported every remaining live target unchanged. The retired Advisor remains
immutable historical `rename_pending` evidence rather than a fabricated rename.

The previously open OP-22 TeamRun was reconciled through Kontor and now reads
`succeeded`. ASMA-8090 completed at revision 3 under lifecycle receipt
`01a073bb-c50d-7263-8350-b9ed463787c7`. Its confirmed Jira link remains exactly
`ASMA-8090`; the final deterministic reconciliation plan is converged with an
empty diff (projection
`8eae69b52d3aa2fac5fa2770617ee551f0b6e624522b8e195ed68a262fe91525`),
and both task and epic conflict ledgers are empty. No direct Jira, Paseo or
database mutation, binary replacement, build, deployment or daemon restart was
performed by the migration-resume session.
