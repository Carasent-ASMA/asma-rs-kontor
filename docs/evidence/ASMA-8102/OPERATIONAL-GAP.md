# ASMA-8102 native epic-project title operational gap

> Status: root cause reproduced and source correction validated. Live deployment
> and post-deployment readback remain required before closure.

## Incident

On 2026-09-06 the Paseo project for Jira epic `ASMA-8111` appeared as raw
Kontor epic UUID `01a07495-e5e0-7ef2-b285-878ee7fb2bd9` instead of the pinned
Team Definition rendering `ESW • KTHSR-8111`. Its native project ID
`prj_0b22d920befb2d85`, root directory, ECP, TSW, CSW and seat identities were
otherwise correct.

The same behavior was reproduced deliberately through the supported Kontor
`epic_control` topology materialization for the ASMA-8101 prevention epic. The
native project `prj_178ec14a1071b271` was created with the raw epic UUID title
while its ECP was correctly named `ECP • APIE-8101`. This isolated the defect to
native-root project creation rather than Team Definition rendering.

## Root cause

`Services::ensure_container` supplied the correct complete ESW title in
`ContainerRequest.display_name`. `PaseoAdapter::bind_native_root` then called
`project.add`, whose live contract accepts only `cwd`; Paseo derived the visible
project title from the UUID directory basename. The adapter read the project
back by exact ID and persisted the container binding without applying or
checking `display_name`.

The native-child path already passed each caller-rendered title to workspace
creation, which is why ECP, TSW and CSW names were correct. The contract fixture
for `project.add` returned an already-correct title and therefore did not model
the live two-step contract. Existing standalone retitle support already proved
that `project.rename` preserves project ID and root, but the fresh native-root
path did not use it.

## Containment

Both affected projects were renamed in place through Paseo's supported
`project.rename` surface only after Kontor exposed the gap. Exact readback
confirmed that project IDs and root directories were preserved:

- `prj_0b22d920befb2d85` → `ESW • KTHSR-8111`
- `prj_178ec14a1071b271` → `ESW • APIE-8101`

These were bounded control-plane fallbacks. No project, workspace, seat or
session was replaced, and the wider workflows did not move to Paseo authority.

## Durable correction

Fresh native-root creation now:

1. refuses before `project.add` unless the live adapter declares project
   retitle support;
2. registers only the explicit runtime-root directory;
3. reads the created project by exact native ID and verifies its root;
4. applies the caller-rendered Team Definition title with `project.rename`;
5. validates the acknowledgement and reads the same project ID back;
6. persists no container binding unless ID, root and exact title all agree.

Regression coverage reproduces the basename-derived UUID title, proves the
successful correction, proves pre-create refusal without rename support, and
proves that an acknowledged rename with stale title readback remains unbound.
The complete Paseo adapter test run passed 312 runnable tests with six live-only
tests intentionally ignored; formatting and Clippy with warnings denied also
passed.

## Related gaps observed during recovery

- Kontor topology readback exposed `native_name: null`, so the topology surface
  could not itself show the live title mismatch. The runtime's exact title
  readback remains the binding gate in this correction; exposing observed names
  in topology inspection is separate follow-up work.
- ASMA-8102 had no persisted task worktree, so scheduler start refused before a
  TeamRun. The existing Jira-bound branch was recovered in an isolated worktree
  and kept on `feat/ASMA-8101-publication-identity-enforcement`.
- `asma worktree add` based that recovered checkout on a stale primary checkout
  rather than current `origin/master`, and `asma git status` could not resolve
  the nested worktree scope. The clean isolated checkout was fast-forwarded to
  current master through a bounded Git fallback before any source edit.

Those workflow defects did not create or rename another native object. They
remain evidence for their owning CLI and topology-inspection follow-up tasks.

## Closure requirements

- all Paseo adapter contract tests pass, including the three new native-root
  tests;
- formatting and Clippy pass with warnings denied;
- the correction is merged from a Jira-keyed branch and deployed from current
  master;
- daemon health, schema integrity and foreign keys pass after restart;
- the two contained project IDs, roots and canonical titles read back unchanged;
- a post-deployment Kontor materialization proves that no UUID-titled project is
  accepted as a bound epic root.
