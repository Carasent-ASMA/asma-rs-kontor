# KBI-8050 live-database migration rehearsal

Date: 2026-08-31  
Jira epic/task: ASMA-8049 / ASMA-8050  
Candidate: `b56771bcefe9c776dbb761be2aacc3c283cabb39`

## Scope

Before deployment, a consistent SQLite backup of the serving ASMA realm was
created with SQLite's online backup operation in the isolated directory
`/tmp/kbi-8050-schema73-rehearsal.uJPH79`. The source realm was schema 73 and
retained the original planned create batch
`01a0539a-a6b8-75d0-b3d0-cf653d396d5b` beside the confirmed fallback link
batch `01a0539c-839d-7003-8ef8-9ff438d276f2`.

The first offline `snapshot` attempt refused the older schema with
`the database does not carry the schema version this build writes`. This is the
intended fail-closed behavior for an offline command. The rehearsal therefore
used the real daemon startup path on isolated loopback port 17717.

## Result

Startup atomically migrated the copy from schema 73 to schema 75 and served the
same realm id `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`. Readback proved:

- `PRAGMA user_version = 75`;
- `PRAGMA integrity_check = ok`;
- `PRAGMA foreign_key_check` returned no rows;
- both new tables existed with zero invented rows;
- the original create batch remained `planned` and the fallback batch remained
  `confirmed` with its original confirmation instant;
- project `01a0064a-e056-7603-9968-ef64fdaacb75` retained its realm, name,
  root path and revision; and
- one epic binding and all existing task links remained present.

The daemon was stopped cleanly, the migrated realm produced a verified
15,126,528-byte snapshot, then restarted on the same isolated port. The second
startup retained schema 75, passed integrity again, and read the same project
identity. This proves migration and restart idempotence without contacting Jira
or the configured Paseo fleet.

The actual Jira recovery effect is deliberately reserved for the deployed live
realm: performing it from a copied control-plane database would target the real
external Jira issues from a non-authoritative realm. Its zero-create and exact
readback behavior is instead pinned by the loopback connector integration tests
and the mutation ledger.
