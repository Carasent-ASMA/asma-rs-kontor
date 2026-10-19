# Backup, restore, export and security operations

Everything here acts on one **state root** — the directory holding a realm's
`kontor.db`, `credentials.json`, `kontor.lock` and `backups/`. A realm *is* its
state root, so every command below names one.

Verified against the `kontor-daemon` argument parser
(`crates/kontor-daemon/src/main.rs`) at `0971674d` (2026-10-11).

The commands live on `kontor-daemon`, not on the `kontor` CLI. `kontor` is a
client: it holds a bearer token and talks to a running daemon. A restore
replaces the database file a daemon has open, an import writes to it directly,
and a rotation of a stopped realm or a Jira credential install needs the state
root's exclusive lock — none of which can be expressed as a request to a daemon
that may not be running.

| Operation | Command | Realm may be serving |
|---|---|---|
| Snapshot + prune | `kontor-daemon --state-root <dir> snapshot [--into <dir>]` | yes |
| List snapshots | `kontor-daemon --state-root <dir> snapshots [--into <dir>]` | yes |
| Export | `kontor-daemon --state-root <dir> export [--out <file>]` | yes |
| Restore | `kontor-daemon --state-root <dir> restore --snapshot <file>` | **no** |
| Import | `kontor-daemon --state-root <dir> import --from <file> --project <id>` | **no** |
| Rotate credentials (stopped) | `kontor-daemon --state-root <dir> rotate-credentials` | **no** |
| Rotate credentials (serving) | `kill -HUP <pid>` | yes |
| Install a Jira credential | `kontor-daemon --state-root <absolute dir> install-jira-credential --alias <alias>` (document on stdin) | **no** |

An offline command takes the same exclusive lock a daemon takes. If a daemon
owns the state root, the command refuses (`state_root_locked`) rather than
racing it.

## Backup

```sh
kontor-daemon --state-root ~/.kontor/realm snapshot
```

The copy is `VACUUM INTO` on a dedicated connection, so it is a transactionally
consistent point in time while the daemon keeps writing. The order is:

1. `PRAGMA integrity_check` on the source — a damaged database is never copied;
2. copy into a unique `.partial` in the destination directory;
3. reopen the copy **read-only** and verify it: integrity check, a schema
   version equal to this build's, exactly one realm row, and the same realm as
   the source;
4. `fsync` the copy, write and `fsync` its manifest;
5. rename both into place and `fsync` the directory.

A published snapshot is never overwritten and a failed run replaces nothing: on
any refusal the directory keeps every file it already had, minus the partial
this run created.

Each snapshot is `kontor-<realm-id>-<instant>.db` with a
`…​.db.manifest.json` beside it holding the manifest format version, realm id,
database schema version, creation instant, byte length and SHA-256.

**Retention** keeps the newest **7** verified snapshots *per realm*, and runs
only after a new snapshot has been published. It ignores partials, files with
no or unreadable manifests, files that do not match their manifest, and other
realms' snapshots — none of those is ever deleted — and it never deletes the
newest verified snapshot whatever the count says.

## Restore — same realm only

```sh
# stop the daemon first
kontor-daemon --state-root ~/.kontor/realm restore --snapshot ~/.kontor/realm/backups/kontor-….db
```

The snapshot is validated completely (manifest, length, digest, integrity,
schema version, realm identity) **before** the destination is touched; its
schema version must equal this build's (see
[Upgrade and binary rollback](#upgrade-and-binary-rollback)). The
destination must then be uninitialized or **the same realm**; a different
initialized realm is refused with a typed error before any rename, and there is
no `--force` that changes that. Moving work between realms is a redacted
import, below.

The outgoing database is checkpointed and moved aside as
`kontor.db.superseded-<instant>` — it is never deleted, so a restore that turns
out to have been the wrong call still has something to go back to. A
**non-empty** `-wal` beside the snapshot or the destination is refused before
anything changes, because it may hold committed transactions; empty `-wal`/`-shm`
files are removed so SQLite cannot recover a stranger's WAL into the restored
file. A daemon that stopped cleanly leaves no WAL frames behind.

A restored realm has no idea what its runtimes did while the snapshot was on the
shelf. The next start therefore keeps the **scheduling barrier shut** until
startup reconciliation has classified every unsettled receipt and every open
binding. Nothing is dispatched before that completes.

## Upgrade and binary rollback

A build runs only against its own schema generation (`0134` at this revision).
Opening an older database migrates it forward in one transaction; a newer one
is refused (`database schema version N is newer than this binary understands`).
Snapshot and restore go further: the source, the copy and an occupied restore
destination must each be at exactly the running build's generation.

1. Before installing a new build, take a snapshot **with the build that is
   installed now**.
2. Install and start the new build; its first open migrates the database.
3. To roll back: stop the daemon and put the previous build back. Move the
   migrated `kontor.db`, with any `-wal`/`-shm` beside it, out of the state
   root and keep it — the previous build refuses to restore over a database at
   the newer generation. Then restore the pre-upgrade snapshot with the
   previous build. Anything written after the upgrade is not in that snapshot.

`export` and `snapshots` open the database through the ordinary path, so a
newer build run against a stopped older realm migrates it. Run them with the
build that owns the realm. `install-jira-credential` reads the realm read-only,
migrates nothing and needs the build's exact generation, so after an upgrade
start the new daemon once and stop it before installing.

## Export — versioned, redacted, deterministic

```sh
kontor-daemon --state-root ~/.kontor/realm export --out realm-export.json
```

Use `--out`. Without it the document goes to standard output, which is also
where log lines go — the `realm exported` line at the default `info` level
precedes the document — so a redirected standard output is not a clean
document.

The Rust envelope type remains named `KontorExportV1`, but the current wire
generation is **17** and this build reads generations **2 through 17**
(`EXPORT_SCHEMA_VERSION` in `crates/kontor-store/src/backup/export.rs`); the
`schema_version` field, not the Rust type name, is the compatibility authority.
The document carries `source_realm_id`, `exported_at`,
`database_schema_version`, a `redaction_summary`, a `continuity_summary`, the
`records_hash` and the `records` themselves.

* **Deterministic.** Every table is a typed struct with named columns read under
  an explicit `ORDER BY`; the document is rendered with sorted keys, compact
  UTF-8 and one trailing newline. Two exports of one unchanged realm produce
  identical record bytes and an identical `records_hash`. The volatile
  `exported_at` sits outside the hashed records.
* **Redacted.** Credential references, environment mappings, live dispatch
  claims, leases and inbound external-comment *bodies* are withheld; so is
  runtime placement identity — execution scopes, hosted leadership seats and
  turn-correlation challenges — which only a same-realm snapshot carries. The
  `redaction_summary` names each one and why. Runtime transcripts, message and
  tool frames, token deltas, runtime endpoints and provider tokens are not in
  the database to begin with.
* **Scanned.** Before it is returned, the document goes through the domain's own
  credential/Zone-C canary — including every stored document parsed as
  structure, not as an opaque string. Runtime and census observations are
  re-checked against the control-metadata rule. A command intent must equal its
  exported command receipt exactly — project, payload bytes and stored and
  recomputed hash — so a command document is exported only under the authority
  that recorded it (ASMA-8121, PR #274, `dddb7287`). An unknown event kind is
  refused. A match aborts the export; the refusal names the structural path and
  never the value.

An unknown `schema_version` is refused with a typed error rather than parsed.

Export and restore both refuse a database whose public attestation-key or
prepared-token ledger (schema v129/v130, ASMA-8278) holds rows: neither the
export nor the restore path carries that ledger. No daemon, MCP or CLI surface
at this revision writes it.

## Import — a different realm, explicitly

```sh
# stop the destination daemon first
kontor-daemon --state-root ~/.kontor/other import --from realm-export.json --project <destination-project-id>
```

An import needs a **separately initialized** destination realm and a
destination project that already exists. A realm never imports its own export —
that is a restore.

What an import does:

* mints the destination's **own** import receipt, naming the source realm, the
  export generation, the source database generation, the records digest, the
  source instant and the destination instant;
* records every source record as append-only lineage: kind, source identity,
  source digest and disposition;
* **materializes** only versioned specifications (calendar profiles, work
  profiles, team templates, persona scenarios, ticket field specs, external
  workflow specs, trigger specs), each re-validated through this build's own
  domain types and re-canonicalized. A document that does not reproduce its
  source digest is refused, not trusted. A specification the destination already
  has at that version is left alone;
* validates generation-3 profile-selection outcomes against their source
  receipt, task, workflow and immutable specification rows, then records their
  exact hash-bound lineage under the destination's import receipt. They are
  non-executable history and never become destination authority;
* keeps the content of source Core Team route successions (generation 13 and
  later) as readable, immutable evidence beside their lineage, so another realm
  can inspect what a route correction did. The evidence authorizes and replays
  nothing (ASMA-8187, PR #290).

What an import never does: write a source command, status-transition or dispatch
receipt into the destination's own receipt tables; recreate a live lease, an
active dispatch claim or a credential binding; or claim freshness. Imported
evidence begins stale and the destination's own reconciliation is what makes
anything current again. Re-importing the same document into the same project is
refused rather than duplicated.

## Credentials

A realm mints one secret per authority tier (`observer`, `operator`, `admin`),
32 bytes of platform entropy each, in a `0600` `credentials.json`.

Rotation regenerates **all three**: the file is written to a temporary file in
the same directory, flushed and renamed, and only then does the running process
swap its in-memory set. A crash between the two steps leaves a realm whose next
start authorizes the new tokens — the operator has them and they work.

Every previously issued token is refused from the next authorization onwards.
In-flight calls already past the auth layer finish; a long-lived SSE subscriber
reconnects with the new token like any other client. Native runtime sessions,
bindings and command receipts are untouched — they are identified by the realm's
own ids, not by the credential a client authenticated with. Seat bearers
(`kontor-seat-v1.…`/`-v2`) are derived from the operator secret, so rotation
also refuses every bearer already handed to a running native seat; only a seat
launched after the rotation carries one derived from the new set.

### Jira credentials

The native Jira connector reads its API credential from the OS keychain. Install
it into a **stopped** realm, with a build that includes PR #278 (`f95e2065`):

```sh
# stop the daemon first; the document is read from stdin only
<command that prints the document> | kontor-daemon --state-root /absolute/realm install-jira-credential --alias <alias>
```

The command holds the state-root lock and refuses before reading stdin unless
the root is absolute, holds an initialized realm at this build's schema, and its
`jira.json` names `<alias>`. A write counts only after an exact readback; a
failed one is rolled back, and an outcome saying the rollback could not be
proven must be reconciled before the next start. The document format, keychain
address and refusals are in
[Configuration](docs/CONFIGURATION.md#jira-connector-and-credentials).

## Logs

Logging is allowlisted **at the sink**. A field that is not on
`kontor_daemon::logging::ALLOWED_FIELDS` is dropped, and any value that matches
the credential canary is written as `<redacted>` even under an allowed field
name. Errors carry a stable category and opaque ids; no auth header, request
body, connector payload, credential path, Zone C material, runtime frame or raw
SQLite value reaches a log line. `RUST_LOG` selects which events are emitted
(default `info`); it cannot widen what a line may contain. Both the serving
daemon and the operator commands write log lines to **standard output**; where
they persist is up to whatever runs the process.

## Live control-plane recovery

Recovery of runs, seats, gates, consultations and desks inside a serving realm
is not a `kontor-daemon` subcommand. A running daemon serves it through its tool
registry (MCP, the generated `kontor` CLI and `/v1`). These operations are
preview/apply pairs bound to the revisions and hashes the preview returned, or
revision-fenced, idempotency-keyed commands; either way, read the receipt and
the aggregate back afterwards. Examples are
`kontor_turn_correlation_challenge_preview` / `_apply` (ASMA-8119),
`kontor_container_recovery_preview` / `_apply`, `kontor_seat_recover` and
`kontor_desk_seats_reconcile`. The owning documents:

- seat supervision, quota succession and `kontor_seat_recover`:
  [Configuration](docs/CONFIGURATION.md#seat-supervision);
- consultation and planning-pair seat recovery:
  [Consultation lifecycle](docs/CONSULTATION_LIFECYCLE.md);
- legacy-import code correction:
  [Native naming](docs/NATIVE_NAMING.md#legacy-import-code-correction);
- durable desks, desk seats, desk Adams and PR reviews, including their
  binary-rollback notes:
  [durable desks](_docs/reference/2026-10-10-23-55-reference-kontord-durable-desks.md),
  [desk seats](_docs/reference/2026-10-11-02-20-reference-kontord-durable-desk-seats.md),
  [desk Adams](_docs/reference/2026-10-11-05-36-reference-kontord-desk-adams.md),
  [PR reviews](_docs/reference/2026-10-11-11-35-reference-kontord-pr-reviews.md).

## Deploy into a launchd job

`scripts/kontor-deploy.py` is the one deploy script for a daemon that runs as a
launchd job. It names the job only by explicit arguments: `--label` and
`--domain` have no default, and `--plist` has no default but is required for
`bootstrap` and `replace`. It reads the daemon path, state root and port from
the plist's `ProgramArguments`; a missing `--port` uses the daemon default 7717,
and a relative `--state-root` is resolved against the plist's
`WorkingDirectory`, as launchd runs it. `--timeout` caps each subprocess
(default 120 s) and `--wait` bounds the port and `realm-get` polls (default
15 s).

```sh
python3 scripts/kontor-deploy.py --label <label> --domain gui/<uid> --plist <plist> \
  replace --candidates <dir> --backup-dir <new dir> --realm-id <realm>
```

`replace` backs up the installed binaries and boots the job out. It snapshots
the realm into the backup directory with the installed daemon, installs the
candidates, bootstraps the job and waits until `realm-get` returns the named
realm. If a step fails after the bootout, it restores the old binaries,
bootstraps the old job and writes `rollback.json`. It does not restore the
database; restore the snapshot as above. `bootout` needs only `--label` and
`--domain`, so it can stop a job whose plist is missing or truncated; without a
readable plist it skips the port wait. `bootstrap` runs one step.

Before it changes any launchctl job, the script refuses (exit 3):

* the production label `com.asma.kontor.daemon`, unless the deploy is declared
  with `--production-deploy <JIRA-KEY>`;
* a declared production deploy outside the `gui/<uid>` domain of the user whose
  home holds the canonical plist, or from any plist but that user's
  `~/Library/LaunchAgents/com.asma.kontor.daemon.plist`;
* `--production-deploy` with any other label;
* a non-production `bootstrap` or `replace` whose daemon or state root is the
  production one, or that uses the production port (7717 when unset), compared
  against the canonical plist, the recorded target and the loaded
  `com.asma.kontor.daemon` job. An unreadable canonical plist is skipped; a
  loaded production job whose `launchctl print` cannot be parsed is refused;
* a plist whose `Label` differs from `--label`. `launchctl bootstrap` registers
  the label written in the plist, and that is how a test once replaced the
  production job.

### What `replace` verifies

Before installing, the installed daemon takes a snapshot: it runs SQLite
`integrity_check` on the live database and on the copy, and checks that both
carry the expected schema version and exactly one realm row. A damaged source is
never copied. `integrity_check` is the stronger form of `quick_check`. After the
restart, `replace` requires `realm-get` to return the expected realm id.

The deploy deliberately does not also run `foreign_key_check` or config-hash
comparisons. Foreign-key enforcement and the migration-time
`foreign_key_check` are the store's contract, and a pre-existing violation is a
state-root condition that the deploy neither creates nor masks. The deploy
installs binaries and does not touch the state root or config. The old
ASMA-8121/8196 checks belonged to their copy scripts, not to this path.

### Timing

`--timeout` bounds every subprocess and `--wait` bounds the port and
`realm-get` polls. A snapshot that exceeds the timeout fails the deploy after
the bootout; the rollback restores the old binaries and restarts the old job.
The operator can rerun with a larger `--timeout` once the production snapshot
duration is known. The defaults are 120 s and 15 s.

Tests and rehearsals use a disposable label such as
`com.asma.kontor.test.daemon`. The script's own tests inject a recording runner
and never reach launchctl:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -v
```

## Troubleshooting

| Symptom | Meaning | What to do |
|---|---|---|
| `state_root_locked` | a daemon owns the state root | stop it, or use an online operation (snapshot/export) |
| `the database failed verification` | the source or copy failed an integrity check, or is truncated/not a database | restore the newest verified snapshot into a *fresh* state root and compare before replacing anything |
| `the destination is initialized as realm …` | the snapshot belongs to another realm | this is a redacted import, not a restore |
| `the document carries material that may not be exported` | a canary matched a stored document | inspect the named path; the value is deliberately not echoed |
| `export schema version N is not one this build reads` | the document is outside this build's supported generation range (currently 2–17) | export again from a compatible source or upgrade the reader |
| `database schema version N is newer than this binary understands` | a newer build already migrated this database | run the newer build, or roll back as in [Upgrade and binary rollback](#upgrade-and-binary-rollback) |
| `the database does not carry the schema version this build writes` | the snapshot source, the snapshot being restored or the restore destination is at another build's generation | run the command with the build whose generation it carries; for a rollback, move the newer destination aside first |
| `public key authority ledger could not be inspected without changing files` | a restore found a non-empty `-wal` beside the snapshot or the destination | beside the destination: let the daemon start and stop cleanly first, or restore into a fresh state root; beside a snapshot: something opened it for writing, so use another verified snapshot |
| `public attestation ledger continuity is unsupported` | the attestation-key or prepared-token ledger holds rows | neither export nor restore carries that ledger at this revision; keep the database and its snapshots as they are |
| category `jira_credential` | the root is relative or uninitialized, the alias is not in `jira.json`, or the document is invalid | fix the named input; nothing was read from stdin or written when the preflight refused |
| scheduling stays shut after a restore | reconciliation has not completed | check the runtime fleet; a census that did not finish proves nothing, so the barrier stays shut on purpose |
