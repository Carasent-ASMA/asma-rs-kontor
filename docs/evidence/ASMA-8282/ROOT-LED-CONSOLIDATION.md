implement • ASMA-8282 • Consultation coordination

# Root-led module consolidation — ASMA-8278 / ASMA-8282

## Authority and inputs

Igor's 2026-10-03 instruction assigns root direct delivery of ASMA-8278, permits
parallel owned child work, and consolidates QA/audit at the end. The existing TPM
and LSA handed publication ownership to root and stopped automatic dispatch.
This source contribution does not appoint another integration writer.

The destination is the clean epic module clone at `10f345251c77e1babcd638c99457a226f7d6ee19`
(tree `c6a4914c28bdca898e59ea30fa6a77b191cfb45e`). Accepted W3a remains intact.
The TASK002 source is `6beb14fedccabde5a5e52e65045656a4b8909cd7`
(tree `ba65c4a2b2dbdc1bd811af26ea5552c92376b1d5`), relative to `545b44a1a693b3cc054bf453709a1dc4149b7440`.
Its existing technical, instruction-impact and HANDOFF evidence is consumed
within its original scope; its missing independent audit is not represented as
completed by this integration.

## Integration

Only the ten-path baseline-relative TASK002 formal-placement contribution is
applied. Nine paths apply without reconstruction. In fleet-activation's tests,
the reviewed cap-test suffix is appended after the W3a-added planning-pair tests;
those newer tests and rule assertions are preserved. The original TASK002/004
checkouts, protected untracked adapters and other owners' work are untouched.
The imported `COMMITTEE-CAP-HANDOFF.md` remains the source receipt, not an
integration or live qualification receipt.

## Shared recovery and contribution sequences (W3b)

`kontor-runtime::planning_pair::application::commands` now coordinates:

- Member finding/answer: restore, authenticate the exact member, intent/replay,
  revision, domain transition, record/contribution CAS, receipt and projection.
- Caller clarification/disposition: restore, authenticate the frozen caller,
  intent/replay, revision, domain transition, record CAS, receipt and projection.
- Same-native member recovery: activity guard, restore, caller authentication,
  frozen member/context, intent/replay, terminal/revision/assertion checks,
  route guard, readback, scoped withdrawal or atomic requalification/receipt.

The daemon's `CommandPorts` delegates authentication, reads, persistence,
receipts, clock, native activity, runtime and rendering to the same existing
functions. Pure API-to-domain conversion stays in the daemon. Fingerprints,
refusal literals, revision precedence, dissent/sealed projections and receipt
classification remain the existing contracts. The moved API-to-domain
conversions are pure and total; moving them into thin daemon adapters introduces
no read, refusal or effect before authentication. `Services::state` is a
OnceLock-backed immutable composition, so repeated access by the ports returns
the same state and realm. New owner-trace tests explicitly
refuse fenced actors before replay, and exact contribution replays before writes.

The daemon remains the sole production owner. No new direct host, trust issuer,
key custody, credential, authority constructor, permission grant, native effect
engine, registry operation, API field, schema or dependency is introduced.

## Validation and limits

The combined library suites pass: daemon 159, fleet 36, fleet-activation 20 and
runtime 107. The daemon planning-pair loopbacks pass 44/44 and the formal
committee-evidence loopbacks pass 7/7: 373 distinct tests, zero failures.
Formatting and diff whitespace checks pass. Clippy passes with `-D warnings`
on all five changed crates, including their tests. After lint-equivalent
corrections, the three new command tests and both full-body recovery loopbacks
pass again on the final source. Exact path hashes and logs are recorded in the
root handoff manifest. Tests use disposable fake-runtime realms
and an isolated Cargo target; no provider, live daemon, database initialization,
installed R3, activation or deployment is consumed or changed.

The extraction implements the reusable source sequence. A direct production
owner still needs a trusted current actor and fenced writer; B2 custody and
native-effect authority remain separate. Unsupported real member routes still
refuse before effects. Loading, native restriction/late-effect fencing, external
succession and installed-target qualification remain unproved by this source
work. No epic/task closure or independent vendor verdict is claimed here.

## Consolidated root mutation check

Root seeded three authorization defects in the extracted contribution/recovery
sequences: omitted member authentication, caller authentication and recovering
caller authentication. All three were killed by the focused command tests.
Each source file was restored byte-identical; the restored source tests pass.
Packet: `/private/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/asma-8278-w3b-mutations-inmbcres/MANIFEST.json`.
These are command-boundary source checks, not native-effect or live trust proof.
