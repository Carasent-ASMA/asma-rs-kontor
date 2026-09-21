# ASMA-8120 launch-count anomaly — bounded analysis

Date: 2026-09-21
Artifact: `launch-anomaly-analysis` (immutable; supersedes nothing)
Question: `OQ-8120-07` — `operator_restart_count = 1` versus
`observed_launch_delta = 2` on deployment
`ASMA-8188-20260921T100129Z-44663e10`.
Authored from AgentRun `01a0bb74-ee78-7aa3-b6bc-bd9c43733496`.

**Scope held.** Read-only against the shared fleet. No restart, no deploy, no
Jira or topology mutation, no PASS claim. Controlled experiments ran only under
private throwaway launchd labels (`com.asma.kontor.launchtest.*`) with their own
programs, never the `com.asma.kontor.daemon` job. After the work the shared job
still reads `runs = 5`, `pid = 39728` — byte-identical to the deployment
receipt, proving the fleet was never restarted by this investigation.

## Answer: neither metric is wrong

Both numbers are correct measurements of different quantities.

- `operator_restart_count = 1` is a **literal constant** in the deploy script
  (`deploy118.py:179`). It records one *intended* restart. Accurate.
- `observed_launch_delta = 2` is computed at `deploy118.py:171` as
  `current['runs'] - previous['runs']`, parsed from launchd's own counter:
  `runs = 3` in `launchctl-before.txt`, `runs = 5` in `launchctl-after.txt`.
  launchd genuinely spawned the job twice. Accurate.

The anomaly is therefore **not a reporting defect**. A second spawn really
happened. What follows establishes what that spawn was.

## Exactly one spawn reached the program

The daemon initialises logging as the **first statement of `main()`**
(`crates/kontor-daemon/src/main.rs:99`, `logging::install()`), before argument
parsing and before any fallible startup step. Every startup failure path that
executes therefore logs — including a contended state root, which surfaces as
`LockError::Held` (`crates/kontor-daemon/src/lock.rs:63`) and is reported by
`error!(detail = %error, "kontor could not start")`
(`crates/kontor-daemon/src/main.rs:229`).

Observed in `kontor-daemon.stdout.log`:

| Time (UTC) | Event |
|---|---|
| 2026-09-21T10:01:54.858610Z | last line written by the outgoing daemon (a routine WARN) |
| — | **nothing at all** — no INFO, WARN, ERROR, panic or partial line |
| 2026-09-21T10:02:05.638511Z | `realm claimed; ... bind=127.0.0.1:7717` — one, and only one |

`kontor-daemon.stderr.log` is 0 bytes with an mtime of 2026-08-15, so nothing
was written there either.

**Conclusion:** of the two spawns launchd counted, exactly one reached `main()`.
The other terminated *before* entering the program — at the exec/load stage —
and `KeepAlive { SuccessfulExit: false }` in
`~/Library/LaunchAgents/com.asma.kontor.daemon.plist` then respawned the job,
which succeeded. The counter settled at 5 and did not loop, which is what a
single recovered failure looks like.

## Hypotheses tested and refuted

Each was tested under a private launchd label reproducing the production plist
(`RunAtLoad`, `KeepAlive{SuccessfulExit=false}`, `ThrottleInterval 5`) and the
production restart verb (`launchctl kickstart -k`).

| # | Hypothesis | Result | Verdict |
|---|---|---|---|
| 1 | `KeepAlive{SuccessfulExit=false}` makes `kickstart -k` inherently double-count | delta **1** with KeepAlive, delta **1** without | **Refuted** |
| 2 | Graceful shutdown outlasting `ExitTimeOut` adds a spawn | delta **1** (program trapped SIGTERM for 10s against a 5s timeout) | **Refuted** |
| 3 | Replacing the running binary in place before `kickstart` adds a spawn | delta identical with and without replacement, across an alternating 4-run A/B | **Refuted** |

Hypothesis 3 initially appeared confirmed at delta 4. The controlled A/B showed
the same delta **with replacement disabled**, which isolated the true variable:
that trial's program was a *copy* of `/bin/sleep`, and copying a Mach-O strips
its code signature, so it could not exec at all. The delta came from repeated
exec failure, not from replacement.

## What that accidental result established

It reproduced the exact production signature: **a spawn that fails before
entering the program increments `runs` while producing zero output, and
`KeepAlive` retries it.** A job whose program execs normally yields delta
exactly 1 under every other condition tested.

This is the only mechanism reproduced that accounts for all four observed
facts — delta 2, one silent extra run, one successful start, and a stable
counter afterwards.

## What is *not* established

The specific reason the single production spawn failed at exec is **not
proven**, and is deliberately not asserted:

- the process left no output by construction, since it never reached `main()`;
- the launchd system log no longer covers that window (`log show` over
  2026-09-21 12:01:45–12:02:15 local returns nothing), so the spawn cannot be
  observed directly.

The most plausible remaining candidate, untestable without a restart, is a
transient exec or signature-validation failure immediately after
`deploy118.py` replaced all three binaries via `os.replace()` roughly two
seconds earlier. The production binaries are correctly signed —
`codesign --verify --strict` and `verify_signatures` both passed, and the
successful spawn ran the new binary — so any such failure was transient, not a
persistent signature fault.

## No code correction is warranted

No change to `kontor` source is justified by this evidence. The daemon behaved
correctly throughout: it started once, claimed the realm, re-attested its
bindings, preserved every identity, and left the database at
`integrity_check = ok` with zero foreign-key violations. Its startup-failure
paths already log before exiting; the silent spawn was silent because it never
reached them, which no source change inside the program can alter.

The weakness is in the **deploy procedure**, which lives in root-owned tooling
outside this repository (`deploy118.py`) and was not modified here:

1. `assert launch_delta == 1` (`deploy118.py:185`) encodes the assumption that
   one intended restart yields exactly one spawn. That is false whenever
   `KeepAlive` legitimately recovers a failed spawn, so a self-healed restart is
   reported as an anomaly requiring root-cause review.
2. The procedure has no per-spawn observability. It samples `launchctl print`
   only before install and after health, so an intermediate failed spawn can
   only ever be *inferred* from a delta — which is precisely the position this
   investigation was left in.
3. `kickstart -k` overlaps kill, binary replacement and start in one verb, so
   there is no point at which the old process is known to have fully exited
   before the new one is spawned.

## Safe next action for the coordinated restart and readback

Root owns the deployment. The recommended sequence, in order:

1. **Add per-spawn evidence before changing anything else.** Capture
   `launchctl print` immediately after the kill and again immediately after the
   start, and persist both. This makes a failed first spawn directly
   observable instead of inferred, and costs nothing.
2. **Make the restart deterministic.** Replace `kickstart -k` with
   `launchctl kill SIGTERM`, poll until the job is no longer running, then
   `launchctl kickstart` without `-k`. This removes the overlap between binary
   replacement, termination and spawn.
3. **Assert the property that matters.** Replace `launch_delta == 1` with
   `launch_delta >= 1` **and** exactly one `realm claimed` line in the daemon
   log after the restart marker. One successful realm claim is the real
   invariant; the raw spawn count is not.
4. **Then perform the coordinated restart and the all-13 readback**, which
   closes `OQ-8120-06` — the ASMA-8111 restart-coverage gap — and lets the
   verifier re-check.

Until step 4 runs, `OQ-8120-06` stays open and no PASS is claimed. `OQ-8120-07`
narrows from "unexplained second launch" to "unproven exec-stage failure
reason", and stays open on that narrower question.
