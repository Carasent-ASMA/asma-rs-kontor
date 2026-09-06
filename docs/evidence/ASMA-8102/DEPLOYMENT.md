# ASMA-8102 deployment receipt

Date: 2026-09-06

## Released source

- Pull request: [#196](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/196)
- Attested PR head: `6cbcb0e44a40bbba9edad5a06135f8259d8d5887`
- Publication attestation: `01a075a1-f5b1-7280-b8f2-01a70934b402`
- Merged and deployed master:
  `12e140b813794e77ce4de95006041a8c1f1a6e5c`

The release was built from a detached worktree at that exact master. A final
remote read confirmed that `origin/master` still named the same commit before
binary replacement.

## Installed artifacts

| Artifact | Detached build and installed SHA-256 |
| --- | --- |
| `kontor` | `1ca69f23a70e38a66e111d31249e47724fd4e96ce67e56f5d5b0d419b278175f` |
| `kontor-daemon` | `80738e10b7041091c0b46091b8109ad8d5764a3ade2e175c5e2ca8ae6d06d033` |
| `kontor-mcp` | `800afe360bcae389c8ab6f2b5840180a788abae862ea5b0cb27711c116ba67e7` |

The LaunchAgent restarted the existing realm on `127.0.0.1:7717` as daemon PID
`16258`. Realm readback retained
`01a00649-9ee6-73e0-ba1b-6a6c35cfd065`. Schema 89 returned integrity `ok` and
no rows from `PRAGMA foreign_key_check` before and after replacement.

Startup again classified 256 historical runtime bindings for review while
opening the scheduling barrier. The immediately preceding deployment reported
the same count, so this is preserved pre-existing reconciliation evidence and
not a regression from ASMA-8102.

## Native identity and title readback

The full 11-project Paseo inventory was identical before and after restart.
The two projects involved in this incident retained their exact identities,
roots and corrected titles:

| Native project | Exact title | Root suffix |
| --- | --- | --- |
| `prj_0b22d920befb2d85` | `ESW • KTHSR-8111` | `01a07495-e5e0-7ef2-b285-878ee7fb2bd9` |
| `prj_178ec14a1071b271` | `ESW • APIE-8101` | `01a0721b-ea30-7fe3-88a5-4d33ca613414` |

Post-deployment `kontor_topology_materialize` replayed ASMA-8101 epic-control
placement through the installed daemon. Receipt
`01a075aa-573b-78b3-ab13-1ff265bc006f` preserved ESW
`prj_178ec14a1071b271`, ECP `wks_fa0c5c9f12959482`, project revision 6 and the
pinned topology revision.

## Rollback evidence

The consistent database snapshot, old and new binaries, hashes, realm
readbacks, project inventories and topology receipts are retained at:

`/Users/igor/.local/state/kontor/asma/deploy-backups/20260906T073955Z-asma-8102-12e140b/`

Its machine-readable `deployment.json` asserts exact build/install hashes,
database integrity, zero foreign-key violations, preserved project inventory
and the two canonical title readbacks.
