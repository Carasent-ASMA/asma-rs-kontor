# ASMA-8204 open questions

## OQ-001 — NativeRoot closeout disposition — resolved

- **Attaches to:** ASMA-8204, `HIGH-SCOPE-RECORD.md` clauses 7-10,
  `ArchiveContainerRequest`, and the Paseo adapter's `archive_container` path.
- **Former ambiguity:** the current adapter archives only child workspaces and
  explicitly preserves a project-bound node, so an ESW could have been marked
  archived logically while its native Paseo project remained active.
- **Options observed:** retain the project and record logical archival; delete
  the exact ESW project; or add a new whole-epic closeout API that owns a
  separate cleanup ledger.
- **Evidence:** Paseo 0.8.0 advertises `projectRemove`, its supported protocol
  accepts `project.remove.request` for an exact `projectId`, and its CLI exposes
  `project delete`. Existing Kontor lifecycle operations already own CAS,
  ordering, idempotency, and durable receipts.
- **Resolution:** delete only the non-adopted, epic-scoped ESW project after
  exact identity/cwd and empty-occupancy readback, then prove exact-ID absence.
  Preserve the Kontor node, binding, completion evidence, and receipt. Reuse the
  existing lifecycle path; do not add a cascading API or a second ledger.

There were no unresolved scope questions at settlement.

## OQ-002 — how a native root's "zero unarchived sessions" is proved — resolved

- **Attaches to:** ASMA-8204, `HIGH-SCOPE-RECORD.md` clause 7, and
  `PaseoAdapter::archive_bound_root`.
- **Ambiguity:** clause 7 requires proving "zero remaining workspaces, and zero
  unarchived sessions" before a root is removed, but a Paseo agent carries no
  project id — only `workspaceId` and `cwd`. Because the same clause requires
  zero workspaces *first*, a session check written over the project's workspace
  set is necessarily empty by then, and would assert nothing.
- **Options observed:** (a) check sessions by the project's workspace ids and
  accept that it is vacuous after the emptiness check; (b) check whether any
  unarchived session's `cwd` lies inside the root's canonical directory;
  (c) ask Paseo for a project-scoped agent listing, which its 0.8.0 protocol
  does not expose.
- **Resolution:** option (b). `within()` compares normalized paths on whole
  components, and is reached only to **refuse** — nothing is ever selected for
  removal by a path, so clause 8's prohibition on deleting "by name, path scan,
  or cached adapter state" is untouched. An unparsable `cwd` refuses rather than
  passing, because a directory the runtime reports and the adapter cannot read
  is exactly where refusing costs least.
- **Residual risk:** a session running outside the root's directory but still
  belonging to it would not be caught here. It is caught by the preceding
  zero-workspace requirement, which is the stronger gate; the `cwd` rule exists
  to catch the *dangling* session that no workspace still lists, which is the
  case the child path already guards against by workspace id.
- **Correction after HV-001 (rejection of `0e5a0bec`):** the statement above
  understated the residual, because the *implementation* of containment was
  wrong for one admitted root. `WorkspaceRoot` accepts the filesystem root `/`
  as a spellable place. The first implementation stripped the root as a text
  prefix and then required the remainder to start with `/`, which is correct for
  `/w/epic` against `/w/epic-2` and **false** for `/` against
  `/dangling-session`. A live unarchived session was therefore read as outside a
  root it was plainly inside, and the irreversible exact-id project removal
  proceeded. That was a blocking defect in the gate, not an acceptable residual.
  Containment is now decided by walking path components, where `/` is the single
  `RootDir` component every absolute path begins with. The residual described
  above — an association that is real but not expressed in the directory tree —
  is what genuinely remains.
