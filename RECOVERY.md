# Backup, restore, export and security operations

Everything here acts on one **state root** — the directory holding a realm's
`kontor.db`, `credentials.json`, `kontor.lock` and `backups/`. A realm *is* its
state root, so every command below names one.

The commands live on `kontor-daemon`, not on the `kontor` CLI. `kontor` is a
client: it holds a bearer token and talks to a running daemon. A restore
replaces the database file a daemon has open, an import writes to it directly,
and a rotation of a stopped realm needs the state root's exclusive lock — none
of which can be expressed as a request to a daemon that may not be running.

| Operation | Command | Realm may be serving |
|---|---|---|
| Snapshot + prune | `kontor-daemon --state-root <dir> snapshot [--into <dir>]` | yes |
| List snapshots | `kontor-daemon --state-root <dir> snapshots` | yes |
| Export | `kontor-daemon --state-root <dir> export [--out <file>]` | yes |
| Restore | `kontor-daemon --state-root <dir> restore --snapshot <file>` | **no** |
| Import | `kontor-daemon --state-root <dir> import --from <file> --project <id>` | **no** |
| Rotate credentials (stopped) | `kontor-daemon --state-root <dir> rotate-credentials` | **no** |
| Rotate credentials (serving) | `kill -HUP <pid>` | yes |

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
3. reopen the copy **read-only** and verify it: integrity check, schema version,
   exactly one realm row, and the same realm as the source;
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
schema version, realm identity) **before** the destination is touched. The
destination must then be uninitialized or **the same realm**; a different
initialized realm is refused with a typed error before any rename, and there is
no `--force` that changes that. Moving work between realms is a redacted
import, below.

The outgoing database is checkpointed and moved aside as
`kontor.db.superseded-<instant>` — it is never deleted, so a restore that turns
out to have been the wrong call still has something to go back to. Stale
`-wal`/`-shm` files are removed so SQLite cannot recover a stranger's WAL into
the restored file.

A restored realm has no idea what its runtimes did while the snapshot was on the
shelf. The next start therefore keeps the **scheduling barrier shut** until
startup reconciliation has classified every unsettled receipt and every open
binding. Nothing is dispatched before that completes.

## Export — versioned, redacted, deterministic

```sh
kontor-daemon --state-root ~/.kontor/realm export --out realm-export.json
```

`KontorExportV1` carries `schema_version`, `source_realm_id`, `exported_at`,
`database_schema_version`, a `redaction_summary`, a `continuity_summary`, the
`records_hash` and the `records` themselves.

* **Deterministic.** Every table is a typed struct with named columns read under
  an explicit `ORDER BY`; the document is rendered with sorted keys, compact
  UTF-8 and one trailing newline. Two exports of one unchanged realm produce
  identical record bytes and an identical `records_hash`. The volatile
  `exported_at` sits outside the hashed records.
* **Redacted.** Credential references, environment mappings, live dispatch
  claims, leases and inbound external-comment *bodies* are withheld; the
  `redaction_summary` names each one and why. Runtime transcripts, message and
  tool frames, token deltas, runtime endpoints and provider tokens are not in
  the database to begin with.
* **Scanned.** Before it is returned, the document goes through the domain's own
  credential/Zone-C canary — including every stored document parsed as
  structure, not as an opaque string — and every control payload is re-checked
  against the control-metadata rule. A match aborts the export; the refusal
  names the structural path and never the value.

An unknown `schema_version` is refused with a typed error rather than parsed.

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
  has at that version is left alone.

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
own ids, not by the credential a client authenticated with.

## Logs

Logging is allowlisted **at the sink**. A field that is not on
`kontor_daemon::logging::ALLOWED_FIELDS` is dropped, and any value that matches
the credential canary is written as `<redacted>` even under an allowed field
name. Errors carry a stable category and opaque ids; no auth header, request
body, connector payload, credential path, Zone C material, runtime frame or raw
SQLite value reaches a log line. `RUST_LOG` selects which events are emitted; it
cannot widen what a line may contain.

## Troubleshooting

| Symptom | Meaning | What to do |
|---|---|---|
| `state_root_locked` | a daemon owns the state root | stop it, or use an online operation (snapshot/export) |
| `the database failed verification` | the source or copy failed an integrity check, or is truncated/not a database | restore the newest verified snapshot into a *fresh* state root and compare before replacing anything |
| `the destination is initialized as realm …` | the snapshot belongs to another realm | this is a redacted import, not a restore |
| `the document carries material that may not be exported` | a canary matched a stored document | inspect the named path; the value is deliberately not echoed |
| `export schema version N is not one this build reads` | the document came from a newer Kontor | export again from the source with this build, or upgrade |
| scheduling stays shut after a restore | reconciliation has not completed | check the runtime fleet; a census that did not finish proves nothing, so the barrier stays shut on purpose |
