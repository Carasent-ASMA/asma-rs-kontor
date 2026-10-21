# Kontor deploy, readback, rollback and host repair runbook

One runbook for the live daemon at `$HOME/.local/state/kontor/asma`:
deploy with the one deploy script, read the realm back, roll back by snapshot
restore, repair a launchd incident, and keep the daily snapshot and the daemon
log under control.

`com.asma.kontor.daemon` is the only production label. A production deploy is
declared with `--production-deploy <JIRA-KEY>`; every other command in this
runbook addresses a label, domain and plist passed explicitly. The deploy
script refuses the production label without that declaration and refuses to
bootstrap a plist whose `Label` differs from `--label`.

**No step in this runbook is a rehearsal by itself.** Rehearse on a disposable
realm and a disposable label first (section 9). A live deploy is KR-14's
decision, not this runbook's.

## 1. Preconditions

| Fact | Where it comes from |
|---|---|
| Deploy script | `scripts/kontor-deploy.py` (KR-05) |
| Readback and build-commit tool | `operations/scripts/kontor-ops.py` |
| Host agents | `operations/launchd/*.plist.in`, installed by `operations/scripts/install-launchd-agents.sh` |
| Production label | `com.asma.kontor.daemon` |
| Production plist | `$HOME/Library/LaunchAgents/com.asma.kontor.daemon.plist` |
| Production domain | `gui/$(id -u)` |
| State root | from the plist's `ProgramArguments`; never assumed here |
| Install directory | the directory holding the configured `kontor-daemon`; `kontor` and `kontor-mcp` live beside it |
| Pre-upgrade snapshot | the `snapshot` named in `<backup-dir>/deployment.json`, taken by the installed daemon during `replace` |

The schema generation is the one the candidate build writes
(`SCHEMA_VERSION` in `crates/kontor-store/src/migrations.rs`; `134` at this
revision). A snapshot can be restored only by the build whose generation it
carries. That is what makes the binary rollback in section 5 an ordered pair:
the old binaries and the pre-upgrade snapshot go back together.

## 2. Build the candidates and record their source

Build the three binaries from the exact commit that is being deployed. Record
the commit and tree beside them immediately; the `0.2.1` version string names no
source on its own.

```sh
cargo build --release --locked \
  --bin kontor-daemon --bin kontor --bin kontor-mcp

commit=$(git rev-parse HEAD)
tree=$(git rev-parse 'HEAD^{tree}')
python3 operations/scripts/kontor-ops.py record-build \
  --install-dir target/release --commit "$commit" --tree "$tree"
```

This writes `kontor-daemon.build-commit.json`, `kontor.build-commit.json` and
`kontor-mcp.build-commit.json` beside the candidates. Each record holds the
binary name, its SHA-256, the source commit and the source tree. Use the hashes
printed by this command (or the qualified build receipt, never a fresh hash of
a copied artifact) as the `--expected-sha256` values below.

Keep the build receipt outside the repository. `record-build` is also the record
step for the *installed* binaries after a deploy (section 3, step 6).

## 3. Deploy

`replace` backs up the installed binaries, boots the job out, takes a verified
snapshot with the installed daemon, installs the candidates, bootstraps the job
and waits for `realm-get` to answer for the expected realm. A failure after the
bootout restores the old binaries and restarts the old job, and writes
`<backup-dir>/rollback.json`.

```sh
python3 scripts/kontor-deploy.py \
  --label "$LABEL" --domain "gui/$(id -u)" --plist "$PLIST" \
  replace \
  --candidates "$PWD/target/release" \
  --backup-dir "$RECEIPT_DIR/deploy-backup" \
  --realm-id "$REALM_ID"
```

A production deploy adds the declaration and pins every candidate. Pin a
rehearsal too; the readback below refuses a candidate that differs from its
sidecar and from the hash passed here.

```sh
python3 scripts/kontor-deploy.py \
  --label com.asma.kontor.daemon --domain "gui/$(id -u)" \
  --plist "$HOME/Library/LaunchAgents/com.asma.kontor.daemon.plist" \
  --production-deploy "$JIRA_KEY" \
  replace \
  --candidates "$PWD/target/release" \
  --backup-dir "$RECEIPT_DIR/deploy-backup" \
  --realm-id "$REALM_ID" \
  --expected-sha256 "kontor-daemon=$DAEMON_SHA256" \
  --expected-sha256 "kontor=$CLI_SHA256" \
  --expected-sha256 "kontor-mcp=$MCP_SHA256"
```

Before a production deploy:

1. Take a fresh snapshot and record its path and SHA-256 (the deploy's own
   snapshot is the rollback source; a second one is the belt to that pair of
   braces).
2. Run the readback set (section 4) against the **current** build and keep it.
   It is the pre-deploy baseline the rollback is compared with. If that build
   predates build-commit sidecars — the N-08 CLI whose installed hash differs
   from its deployment record is the current example — run the baseline with
   `--require-build-commit` **omitted**, pass only hashes that match the
   installed binaries, and record the missing provenance in the receipt. The
   `--require-build-commit` form in section 4 is for a deployed build that has
   sidecars; using it on the pre-sidecar baseline refuses at the binary check,
   after health, realm and CLI have already answered.
3. Copy the installed sidecars into `$RECEIPT_DIR/baseline-sidecars/`, or
   record the previous build receipt's commit beside the installed binaries. The
   rollback restores them with the old binaries.
4. Confirm the realm id in `/v1/realm` and in the snapshot manifest agree.

After `replace` returns 0:

5. Read `<backup-dir>/deployment.json`. It names the snapshot, its SHA-256, the
   before/after binary hashes and the readback body.
6. Record the build commit beside the *installed* binaries and check the
   hashes match the receipt:

```sh
python3 operations/scripts/kontor-ops.py record-build \
  --install-dir "$INSTALL_DIR" --commit "$commit" --tree "$tree"
python3 operations/scripts/kontor-ops.py verify-build \
  --install-dir "$INSTALL_DIR" --commit "$commit"
```

### Operational notes from the 2026-10-21 live run

- `replace` performs the gated stop itself. A `bootout` before it makes the
  script exit 1 (`EXIT_FAILED`, its `launchctl bootout` fails); a
  `launchctl bootout` of an already-unloaded label exits 3. Call `replace` with
  the job loaded and let it stop the daemon.
- The Jira credential installer takes the state-root lock, so it cannot run
  while the daemon holds it: after `replace` returns, run one declared
  `bootout` → `"$INSTALL_DIR/kontor-daemon" install-jira-credential
  --alias "$ALIAS" --state-root "$STATE_ROOT"` with the credential document on
  stdin (never a flag, never a file, never printed) → `bootstrap`, then
  continue with the readback. Do not pass the value through a shell argument.
- `replace`'s pre-upgrade snapshot is the rollback source. Under a no-backups
  order, skip the optional second snapshot; the mandatory one is transient and
  is deleted after a green readback. Remove the snapshot payload and the
  binaries in `deploy-backup/`, but keep `deployment.json` and the recorded
  hashes there (the sidecars live beside the installed binaries): section 8's
  evidence set needs them. That removes the
  binary/DB rollback source by design — record it in the receipt and do not
  recreate it later.
- If `attention.yml` is present, set the live attention `.env` to mode 600 and
  owned by the state-root owner before the restart; the daemon refuses to start
  otherwise. Check key names only, never values.

## 4. Readback set

Run all five checks after every deploy. The tool performs the HTTP, CLI and MCP
calls and fails on the first disagreement. The command below is the post-deploy
form and requires a sidecar beside each binary. A pre-deploy baseline on a build
without sidecars omits `--require-build-commit` and any `--expected-sha256`
value that does not match the installed binary (see section 3, step 2).

```sh
python3 operations/scripts/kontor-ops.py readback \
  --state-root "$STATE_ROOT" --base-url "http://127.0.0.1:$PORT" \
  --expected-realm "$REALM_ID" --expected-schema 134 \
  --install-dir "$INSTALL_DIR" \
  --expected-sha256 "kontor-daemon=$DAEMON_SHA256" \
  --expected-sha256 "kontor=$CLI_SHA256" \
  --expected-sha256 "kontor-mcp=$MCP_SHA256" \
  --require-build-commit \
  --json-out "$RECEIPT_DIR/readback.json"
```

| Check | What is proved |
|---|---|
| Health | `GET /v1/health` answers with the observer credential, `live` is true, the realm id is the expected one and `schema_version` is the persisted generation (`134`). The answer also records the reconciliation barrier and `scheduling_open`; after a restore the barrier may stay shut until startup reconciliation finishes, and that is not a failure of this readback. |
| `realm-get` | `kontor realm-get` against the same base URL returns `status` 200 and the same realm id. |
| Schema | `/v1/health` and, for an offline cross-check, `sqlite3 -readonly "$STATE_ROOT/kontor.db" 'PRAGMA user_version;'` both report `134`; `PRAGMA quick_check;` reports `ok`. |
| Binary hashes | Each installed binary matches the hash in `deployment.json` and the hash in its `.build-commit.json` sidecar. |
| MCP parity canary | A real `kontor-mcp` stdio session at observer tier serves `kontor_realm_get`; the call returns the same realm id as `/v1/realm`, `/v1/health` and the CLI. |

The raw surfaces remain useful during an incident:

```sh
curl -fsS -H "Authorization: Bearer $(python3 -c \
  'import json,sys; print(json.load(open(sys.argv[1]))["observer"])' \
  "$STATE_ROOT/credentials.json")" "http://127.0.0.1:$PORT/v1/health"

kontor realm-get --state-root "$STATE_ROOT" --base-url "http://127.0.0.1:$PORT"
```

Do not put the bearer in a shell history, a log line or a receipt. The tool
reads it from the realm's `0600` file and never prints it.

## 5. Roll back

A rollback restores the pre-deploy **binaries and database** together. The
database is restored from the snapshot `replace` took before it installed
anything. Anything written after that snapshot is not in it.

1. Stop the job with the deploy script, not with `launchctl` directly:

```sh
python3 scripts/kontor-deploy.py --label "$LABEL" --domain "gui/$(id -u)" bootout
```

For the production rollback, add the declaration; `bootout` needs only the
label and domain. Without `--production-deploy` the KR-05 guard refuses the
production label with exit 3 before it calls `launchctl`:

```sh
python3 scripts/kontor-deploy.py \
  --label com.asma.kontor.daemon --domain "gui/$(id -u)" \
  --production-deploy "$JIRA_KEY" bootout
```

2. Move the migrated database aside. Keep it: the previous build refuses to
   restore over a newer generation, and the file is the evidence of what the new
   build wrote.

```sh
stamp=$(date -u +%Y%m%dT%H%M%SZ)
mv "$STATE_ROOT/kontor.db" "$STATE_ROOT/kontor.db.migrated-$stamp"
[ ! -e "$STATE_ROOT/kontor.db-wal" ] || mv "$STATE_ROOT/kontor.db-wal" "$STATE_ROOT/kontor.db.migrated-$stamp-wal"
[ ! -e "$STATE_ROOT/kontor.db-shm" ] || mv "$STATE_ROOT/kontor.db-shm" "$STATE_ROOT/kontor.db.migrated-$stamp-shm"
```

3. Put the old binaries back from `<backup-dir>`. `replace` stores
   `kontor-daemon.old`, `kontor.old` and `kontor-mcp.old`; the snapshot file is
   the `snapshot` value in `<backup-dir>/deployment.json`. If the installed
   binary was a symlink, `replace` also stored `<name>.old-link`; recreate the
   link instead of copying over the link target.

```sh
for name in kontor-daemon kontor kontor-mcp; do
  if [ -f "$RECEIPT_DIR/deploy-backup/$name.old-link" ]; then
    ln -sfn "$(cat "$RECEIPT_DIR/deploy-backup/$name.old-link")" "$INSTALL_DIR/$name"
  else
    cp -p "$RECEIPT_DIR/deploy-backup/$name.old" "$INSTALL_DIR/$name"
  fi
done
cp -p "$RECEIPT_DIR/baseline-sidecars/"*.build-commit.json "$INSTALL_DIR/"
```

If no sidecar was saved because the previous build predates them, record the
previous commit from its build receipt **only when the receipt's hash matches
the installed binary**. Otherwise leave the sidecar absent, run the readback
without `--require-build-commit`, and record the gap in the receipt. A sidecar
that names the wrong commit is worse than no sidecar.

4. Restore the snapshot with the **old** daemon, which is the build that
   carries the snapshot's generation.

```sh
"$RECEIPT_DIR/deploy-backup/kontor-daemon.old" \
  --state-root "$STATE_ROOT" restore \
  --snapshot "$RECEIPT_DIR/deploy-backup/$SNAPSHOT_NAME"
```

A non-empty WAL beside the snapshot or the destination is refused; stop the
process that still owns the state root, wait for it to exit, and retry. The
outgoing database is kept as `kontor.db.superseded-<instant>`.

5. Start the old job from the same plist:

```sh
python3 scripts/kontor-deploy.py --label "$LABEL" --domain "gui/$(id -u)" \
  --plist "$PLIST" bootstrap
```

The production form carries the declaration and the canonical production plist:

```sh
python3 scripts/kontor-deploy.py \
  --label com.asma.kontor.daemon --domain "gui/$(id -u)" \
  --plist "$HOME/Library/LaunchAgents/com.asma.kontor.daemon.plist" \
  --production-deploy "$JIRA_KEY" bootstrap
```

6. Read back with the **previous** schema generation and compare with the
   pre-deploy baseline. The `--require-build-commit` flag assumes step 3
   restored the baseline sidecars; omit it when step 3 recorded the N-08 gap:

```sh
python3 operations/scripts/kontor-ops.py readback \
  --state-root "$STATE_ROOT" --base-url "http://127.0.0.1:$PORT" \
  --expected-realm "$REALM_ID" --expected-schema "$PREVIOUS_SCHEMA" \
  --install-dir "$INSTALL_DIR" --require-build-commit \
  --json-out "$RECEIPT_DIR/rollback-readback.json"
```

The realm id, `created_at`, schema generation and the `/v1/realm` and
`realm-get` bodies must match the baseline. A differing realm id means the
wrong snapshot; stop and do not bootstrap anything else. The restored realm keeps scheduling shut until
startup reconciliation finishes; that is expected and is not a rollback
failure.

Record in the receipt: the backup snapshot path and SHA-256, the moved-aside
database path, the restore outcome, the rollback readback and the baseline it
was compared with.

## 6. Launchd incident repair

Triage read-only first:

```sh
launchctl print "gui/$(id -u)/$LABEL"
lsof -nP -iTCP:"$PORT" -sTCP:LISTEN
ps -axo pid,ppid,etime,command | grep '[k]ontor-daemon'
tail -n 100 "$STATE_ROOT/kontor-daemon.stderr.log"
```

| Symptom | Meaning | Repair |
|---|---|---|
| `launchctl print` says the service is not found | the job is not loaded, or the label is wrong | confirm the label in the plist's `Label`, then `bootstrap` with the deploy script |
| the plist is missing, truncated or fails `plutil -lint` | launchd has nothing safe to load | restore a known-good copy, run `plutil -lint`, then `bootstrap`; `bootout` still works with only `--label` and `--domain` |
| `plist_label_mismatch` from the deploy script (exit 3) | the plist registers a different label; `launchctl bootstrap` would act on that label, not `--label` | fix the plist's `Label`, lint it, run `bootstrap` again |
| the job is loaded but `state` is not `running` and the process is gone | a crash or a clean exit under `KeepAlive` | read stderr, fix the cause, then `bootout` and `bootstrap`; do not edit the plist while the job is loaded |
| the job is booted out but the port still listens | an orphan from an earlier run, or an unrelated listener | identify the pid; proceed only when its executable and `--state-root` match this job. Then terminate that pid, confirm the port frees, and `bootstrap` |
| the job restarts every few seconds | `KeepAlive` with a failing start | read stderr and the health endpoint; stop with `bootout`, keep the job down while fixing the cause |
| a deploy refuses with `production_target` | a non-production deploy names the production daemon, install directory, state root or port | stop. That refusal is the guard working; a rehearsal must use a disposable copy of the binaries, another state root and another port |

Never work around the guard by editing it or by invoking `launchctl` with the
production label outside a declared deploy. If the production job itself must be
restarted, that is a declared production operation under KR-14, with a `bootout`
and `bootstrap` pair from this runbook and a readback after each.

## 7. Daily snapshot and log rotation on the host

Two host agents, both installed and read back under D-1 item 4. Neither stops,
restarts or reconfigures the daemon; the snapshot is online and the rotation
keeps the same file descriptor open.

| Agent | Schedule | Action | Retention |
|---|---|---|---|
| `com.asma.kontor.snapshot` | 04:15 daily | `kontor-daemon --state-root $HOME/.local/state/kontor/asma snapshot` | the store's own: newest 7 verified snapshots per realm |
| `com.asma.kontor.logrotate` | 04:30 daily | copy-truncate `kontor-daemon.stdout.log` | 7 numbered copies |

Install, idempotently:

```sh
operations/scripts/install-launchd-agents.sh --home "$HOME"
```

Read back:

```sh
launchctl print "gui/$(id -u)/com.asma.kontor.snapshot"
launchctl print "gui/$(id -u)/com.asma.kontor.logrotate"
plutil -lint "$HOME/Library/LaunchAgents/com.asma.kontor.snapshot.plist"
plutil -lint "$HOME/Library/LaunchAgents/com.asma.kontor.logrotate.plist"
```

Prove each one runs:

```sh
launchctl kickstart "gui/$(id -u)/com.asma.kontor.snapshot"
"$HOME/.local/bin/kontor-daemon" --state-root "$HOME/.local/state/kontor/asma" snapshots
```

`snapshots` must list the new verified snapshot and at most seven. For rotation,
record the live log size, kickstart the agent, and record the sizes again:

```sh
launchctl kickstart "gui/$(id -u)/com.asma.kontor.logrotate"
ls -l "$HOME/.local/state/kontor/asma/kontor-daemon.stdout.log" \
      "$HOME/.local/state/kontor/asma/kontor-daemon.stdout.log.1"
```

The live file must be smaller and `.1` must exist. Then repeat the health and
`realm-get` readback: a rotation that disturbs the daemon is a failed rotation.

Remove both agents (leaves the logs and snapshots alone):

```sh
operations/scripts/install-launchd-agents.sh --home "$HOME" --remove
```

## 8. Evidence

Write one receipt directory per deploy or rehearsal, outside the repository:

```
$HOME/.local/state/kontor/paseo-direct/ASMA-8501/evidence/<date>-<ticket>-<slug>/
  commands.txt                 every command and its exit status
  baseline-readback.json       pre-deploy readback
  deploy-backup/deployment.json
  deploy-backup/rollback.json  only when replace failed
  readback.json                post-deploy readback
  rollback-readback.json       post-rollback readback
  agents/                      launchctl readback for both host agents
  rotation/                    sizes and health before/after
  NOTES.md                     what was proved, what was not
```

The repository is public. Do not commit receipts, host paths with a user name,
credentials or log extracts to it. The runbook cites receipts by path and hash.
The deploy script's own `deployment.json` already carries the binary hashes,
snapshot hash and readback body; keep it beside the evidence.

## 9. Rehearsal record

Rehearsed 2026-10-21 on a disposable copy of the KR-01 snapshot, a disposable
install directory, the label `com.asma.kontor.rehearsal.asma8519`, and port
7791. The live daemon (`com.asma.kontor.daemon`, pid and realm untouched) was
not deployed, stopped, booted or modified.

| Step | Result |
|---|---|
| KR-01 snapshot | `$HOME/.local/state/kontor/recovery/ASMA-8502-KR-01-20261017T141321Z/kontor-01a00649-9ee6-73e0-ba1b-6a6c35cfd065-20261017T141321938285Z.db`, SHA-256 `496aca35d2dfe5f9f40f2b1dfc0651b3bd975a8eebd2fea9efa3ae34e76d2e08`, realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, schema 119 |
| Baseline | old build started on the copied realm; health, `realm-get` and the 50-tool MCP surface read back at schema 119 |
| Deploy | `replace` installed the `9e4524be` candidates, took the pre-deploy snapshot (`adff86cfc56957b2c9df30eb06c3f1872bb6adb81e485f49aa9447732be1bd1e`) and read the realm back at schema 134 |
| Readback | health (`live`, schema 134), `realm-get`, `PRAGMA user_version` 134 and `quick_check` ok, three binary hashes with sidecars naming `9e4524be`, and the 61-tool MCP parity canary |
| Rollback | bootout, migrated database and its WAL/SHM moved aside, old binaries restored by hash, snapshot restored with the old daemon, old job bootstrapped and read back at schema 119 with realm bodies equal to the baseline |
| Incident repair | mismatched-label plist refused with exit 3 before any launchctl mutation; lost-plist `bootout` worked and the correct plist bootstrapped and answered after 2 s |
| Snapshot agent | installed and read back; kickstart published a 211,001,344-byte verified snapshot and left 3 verified snapshots (at most 7) |
| Log rotation | installed and read back; kickstart moved 240,232,393 bytes to `.1`, truncated the live file, and the same pid kept fd 1u and answered health, `realm-get` and MCP parity |

The receipt directory is named in the delivery report. The runbook steps above
are the rehearsed commands; no step was shortened for the rehearsal.
