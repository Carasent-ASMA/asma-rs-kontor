# KON-OP-09 code review

Status: **passed** at `ffeffc3` (round 3).
Scope: the OP-09 console surface, reviewed against `ARCHITECTURE.md`
Seat: inspector (`code` work profile, `code-review` phase)

| Round | Date | Head | Verdict |
| --- | --- | --- | --- |
| 1 | 2026-08-18 | `4211bb0` | rejected — 2 blocking |
| 2 | 2026-08-18 | `47948b6` | passed |
| 3 | 2026-08-18 | `ffeffc3` | **passed** — QA gaps closed |

## Round 3 — passed

Re-review of `ffeffc3` "fix(asma-7878): Hold one idempotency key per intent and
decouple the panels", answering the two blocking acceptance gaps in `QA.md`
(recorded at `efd4670`). Console only: `ProjectView.tsx` and its test, plus a
`REMEDIATION.md`. No contract, no `.rs` file, no server route.

The QA seat was right to escalate. Round 2 recorded both of these as required
architecture proofs but scoped them non-blocking *for the code-review gate*;
QA correctly held that a required OP-09 acceptance proof cannot be signed off as
complete while unmet, whatever a sibling gate chose to let through. This review
adopts that reading: both are now closed on their merits, not waived.

### Gates at `ffeffc3`

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **pass** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **pass** |
| `cargo test --workspace` | **pass** — 1394 passed, 0 failed |

`tests/e2e/pilot.rs` green; bundle records `verdict: accept`, `pass 42 · fail 0`,
`session.no-direct-runtime` passing. Supplementary: no `openapi-typescript`
drift, `tsc --noEmit` clean, `vitest run` 290 passed (285 → 290).

### QA finding 1 — idempotency replay — cleared

`useIntentKey` (`ProjectView.tsx:74`) holds one key per intent in a `useRef`,
derived from `JSON.stringify` of the request: identical fingerprint returns the
held key, a changed fingerprint mints a new one, and `release()` drops it once a
receipt confirms. All seven activation sites route through it and
`crypto.randomUUID()` now appears exactly once in the file, inside the hook.
Each panel holds a distinct key per command, so advance cannot inherit
remediate's key. The fingerprint is order-stable because each request object is
built literally at one site.

The semantics are right for the reason the commit gives: `quick-sessions:ensure`
carries no `expected_revision`, so it is the one command where a
freshly-minted retry key is not merely unrecognized but a second durable
workspace.

### QA finding 2 — sibling suppression — cleared

Core Team renders on `data.coreTeam.value` alone; the role catalog now feeds
only the editor, passed in as `roles`/`rolesError`, and its absence degrades the
editor (no options, `Add to preview` disabled) while the roster stays visible
under the catalog's own refusal text. Completion is split into two independent
children — `CompletionProfiles` and `CompletionPanel` — each with its own
ready-or-refused result.

I checked the residual coupling deliberately: Quick Sessions still requires
`data.roles.value`, which is correct rather than missed. That panel has no valid
sibling projection to erase — without the quick-roles projection there is no
eligible role to select, and inventing one is what the architecture forbids.

### The five new tests are effective

I did not take the commit's mutation claim on trust; I re-ran it myself, four
mutations, each killing exactly the intended test and nothing else:

| Mutation | Result |
| --- | --- |
| `apply.keyFor(request)` → `crypto.randomUUID()` | kills `replays one uncertain intent…` only |
| `keyFor` never replaces (`held.current === null`) | kills `mints a new idempotency key…` only |
| recouple Core Team to `roles.value` | kills `keeps the Core Team roster…` only |
| recouple both completion children | kills both completion independence tests |

Every mutation was reverted and `ProjectView.tsx` verified byte-identical to
`ffeffc3` afterwards.

### Open, non-blocking

**New — `release()` has no test.** Dropping `apply.release()` or
`ensure.release()` leaves all 290 tests green. The shipped code is correct, so
this is a coverage gap rather than a defect, but it sits on the most delicate
edge of this fix. It matters most for `quick-sessions:ensure`: because that
request carries no `expected_revision`, a second genuinely-new session with the
same purpose and role has a fingerprint identical to the first, so `release()`
is the only thing that stops it being replayed into the first session's receipt.
For the revision-carrying commands the fingerprint advances on its own and the
release is belt-and-braces. Worth one test, given that making this class of bug
visible to the suite was the point of the round.

**Observed once — an unidentified vitest flake.** The first full `vitest run` of
this review failed 1 of 290; the identity was not captured, and 17 subsequent
full runs plus four-way parallel contention were all green. A background
`cargo test` was under way during both the failing and the passing runs, so load
alone does not explain it. Recorded as an observation, not a finding: unverified
and non-reproducing, but the new tests chain several `waitFor`/`findBy` awaits
and are the plausible site if it recurs.

**Items 5 and 6 remain open**, confirmed unchanged at `ffeffc3`: no client
method for `seats:materialize` or the consultation settle contracts, and
`table-scroll` (`:302`) still without `tabindex`/`role`/label.

---

## Round 2 — passed

Re-review of `47948b6` "fix(asma-7878): Clear the OP-09 code-review blockers",
a 6-file remediation: `client.ts`, `client.test.ts`, `ProjectView.tsx`,
`ProjectView.test.tsx` and the two committed screenshots. No `.rs` file
changed in either round.

### Gates at `47948b6`

All three green, run in this worktree against the reviewed HEAD:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **pass** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **pass** |
| `cargo test --workspace` | **pass** — 1394 passed, 0 failed |

`tests/e2e/pilot.rs` is green and its bundle records `verdict: accept`,
`pass 42 · fail 0`, with `session.no-direct-runtime` passing.

Supplementary, also green: `openapi-typescript` regeneration byte-identical to
the committed `schema.d.ts` (no drift), `tsc --noEmit` clean, `vitest run`
285 passed (284 in round 1, plus the new verb test).

### Blocking 1 — cleared

No `paseo` remains anywhere under `apps/console/src`. The fix drops the vendor
noun and keeps the distinction the architecture requires: "An ESW is a separate
native project; its ECP is one ordinary workspace, not a nested native project"
and "it does not nest one native project inside another".

I verified the scanner's scope directly rather than accepting the claim:
`tests/e2e/pilot_sections/session.rs:949` scans exactly
`["apps/console/src", "apps/desktop/src-tauri/src"]`, and
`RUNTIME_NEEDLES` (`:144`) is `["paseo", "agent-orchestrator",
"runtime_endpoint"]`, matched case-insensitively with no exemption for prose or
fixtures. So `apps/console/e2e/project.spec.ts`, which still carries
`paseo.project` in its Playwright mocks, is genuinely outside the guard — the
commit message's claim is accurate.

That residue is correct rather than merely tolerated, and the regenerated
desktop screenshot shows why: the topology panel still renders
`desired: paseo.project`, because that is the `runtime_kind` the *server*
returned. The console displays the runtime's vocabulary as data while naming it
nowhere in its own source, which is exactly the boundary the criterion protects.
Worth recording that the guard's reach stops at `src`, so an e2e fixture could
drift without the gate noticing.

### Blocking 2 — cleared

`client.ts:355` now sends `{ method: 'POST' }`. The handler
(`applications.rs:4784`) takes no body extractor and the contract marks no
`Idempotency-Key`, so a body-less, keyless POST is the right shape.

I re-derived the full verb map independently instead of trusting the
cross-check: every one of the 24 client call sites was matched against
`crates/kontor-api/contract/openapi.json` by path shape and method. **No
mismatches remain.**

The new test (`client.test.ts`) asserts verb and path together for ten
Operational routes and pins the preview's absent key and body. I confirmed it is
a real test rather than a passing assertion: reintroducing the original defect
(removing `{ method: 'POST' }`) turns it red at `client.test.ts:143` with
`GET`≠`POST` on the promotion path; restoring the line returns it to green, and
the working tree was left clean.

### Still open — non-blocking, unchanged

> Superseded by round 3: items 3 and 4 were escalated by the QA gate and closed
> at `ffeffc3`. The paragraph below records the state at `47948b6`.

Items 3-6 below are untouched and were explicitly deferred by the builder to
OP-10. Confirmed still present at `47948b6`: seven inline `crypto.randomUUID()`
sites, the two coupled panel guards at `ProjectView.tsx:177` and `:218`, no
client method for `seats:materialize` or the consultation settle contracts, and
`table-scroll` (`:271`) still without `tabindex`/`role`/label.

None of these blocks the gate. The idempotency-replay item (3) is the one I
would not carry much further than OP-10: it is the only open item that can
create a duplicate durable write rather than merely degrade a view.

Housekeeping, not a finding: `apps/console/test-results/` and the retained
`docs/evidence/KON-MVP-18/run-*` bundles are untracked build artifacts in the
worktree.

---

## Round 1 — rejected

Reviewed range: `3d2dfca..4211bb0` — the seed for this worktree was the OP-04
remediation head. The diff was frontend-only: no `.rs` file changes, 18 files,
+1856/-18. Retained in full as the record of what the remediation answers.

### Verdict

**Rejected.** The mandated gate is red at HEAD, and the defect that turns it red
was introduced by this diff. Separately, one shipped operator flow — Promote to
Epic — cannot succeed at runtime, because its preview call uses the wrong HTTP
method against a POST-only route.

The rest of the change is good work. The wire layer is disciplined, the panels
are honest about server ownership, and the accessibility of `CodeHelp` is real
rather than decorative. The blocking items are narrow and mechanical; none of
them requires rethinking the design.

### Gate results at `4211bb0`

Run in this worktree against the exact reviewed HEAD:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **pass** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **pass** |
| `cargo test --workspace` | **FAIL** (exit 101) |

Supplementary checks, all green:

| Check | Result |
| --- | --- |
| `openapi-typescript` regeneration vs committed `schema.d.ts` | pass — byte-identical, no drift |
| `tsc --noEmit` | pass |
| `vitest run` | pass — 16 files, 284 tests |

A second `cargo test --workspace --no-fail-fast` from another seat was running
in this worktree during the run. It does not explain the failure: the failing
criterion is a static scan of the source tree, and its finding is reproducible
with `git grep` alone (see Blocking 1).

### Blocking

#### 1. `cargo test --workspace` is red: the console now names the runtime it must not name

`tests/e2e/pilot.rs` fails criterion `session.no-direct-runtime` — *"no client
path reaches Paseo, AO or a runtime endpoint"*. Pilot verdict for the run:
`pass 41 · fail 1`, overall `reject`.

Flagged occurrences, all added by this diff:

- `apps/console/src/views/ProjectView.tsx:268` — "A Paseo ESW is a separate native project"
- `apps/console/src/views/ProjectView.tsx:438` — "it does not nest one Paseo project inside another"
- `apps/console/src/views/ProjectView.test.tsx:32` — `runtime_kind: 'paseo.project'`
- `apps/console/src/views/ProjectView.test.tsx:33` — `runtime_kind: 'paseo.project'`

This is a regression, not a pre-existing condition:

```
git grep -in "paseo" 3d2dfca -- apps/console/src   # no matches
git grep -in "paseo" 4211bb0 -- apps/console/src   # 4 matches
```

The tension is genuine and worth stating plainly, because the fix should not be
chosen carelessly. `ARCHITECTURE.md` *requires* the logical/native distinction
to be visible to the operator ("Each ESW is a separate native Paseo project…
ECP is one workspace… it is not a folder of role workspaces"), and the builder
wrote exactly that copy. The pilot criterion forbids the console source from
naming the runtime at all. Both cannot hold as written.

Resolve it deliberately, not by deleting the guard:

- the two prose strings can keep the whole distinction while dropping the vendor
  noun — "native placement", "a separate native project", "not nested native
  projects" — which is what the criterion is actually protecting; and
- the two test literals are server-returned `runtime_kind` values in a fixture.
  If the scan is meant to exempt fixture data, that exemption belongs in the
  criterion, argued on its own terms; otherwise the fixture should carry the
  value the same way the panel receives it, without spelling it in console
  source.

Either way the gate must be green at the reviewed commit before this lands.

#### 2. Promotion preview sends GET to a POST-only route; the whole promote flow is unreachable

`apps/console/src/api/client.ts:356`:

```ts
async previewPromotion(projectId: string, quickSessionId: string): Promise<PromotionPreview> {
  return this.#json<PromotionPreview>(
    `/v1/projects/…/quick-sessions/${…}/promotion:preview`,
  )
}
```

`#json` passes no `init`, so `fetch` defaults to **GET**. The contract declares
this path **POST** only, and the router registers it as such —
`crates/kontor-api/src/lib.rs:391` → `post(applications::preview_promotion)`.
A GET therefore returns 405 before any handler runs.

Consequence: "Preview promotion" (`ProjectView.tsx:457`) always fails, `preview`
stays `null`, and "Promote confirmed preview" stays disabled forever. The
promotion half of section 4 — one of the seven sections the architecture
requires — cannot be exercised at all.

The handler takes no request body (`applications.rs:4784` — `State`, `Caller`,
`Path` only), so the fix is a body-less POST, not a new request DTO. Note that
`#post` as written would send `Content-Type: application/json` with an absent
body; prefer `this.#json(path, { method: 'POST' })` or a body-less variant.

Why no test caught it: `ProjectView.test.tsx` mocks the client object, so the
transport never runs; `client.test.ts:100` covers topology, code-help, Core Team
preview and apply but not `previewPromotion`, and asserts no HTTP verbs
anywhere; and `e2e/project.spec.ts` fulfills by pathname only, returning
`{ realm_id }` for anything unmatched — so a wrong-method request is answered
200 in the browser test too. The committed screenshots never reach the
promotion controls, which only render after a Quick session exists.

Please add a verb assertion alongside the fix; without one this class of defect
stays invisible to the whole suite.

### Non-blocking, but expected before OP-10

#### 3. Idempotency keys are minted per click, so a retry is a second intent

Every mutation generates its key inline at activation —
`ProjectView.tsx:400, 445, 462, 498, 503, 582, 586`, all
`crypto.randomUUID()` in the `onClick`/`onSubmit` expression.

The in-flight lock works and is tested: a double activation sends one request
(`ProjectView.test.tsx` — apply is disabled on first click, `applyCoreTeam`
called once). That covers the first half of the requirement.

The second half is not met. `ARCHITECTURE.md` rule 2 — *"Reuse the same
idempotency key when retrying one uncertain request"* — and the required proof
*"replay uses the original idempotency key"* both fail: after a transport
failure or an ambiguous response, `act()` clears `busy`, the operator clicks
again, and a **new** key is generated. For `quick-sessions:ensure` and
`core-team:apply` that is precisely the duplicate durable write idempotency
exists to prevent, and the daemon has no way to recognize the retry.

Hold the key in state per pending intent; mint a new one only when the intent
itself changes (which the code already tracks — editing seats clears the
preview).

#### 4. A failed sibling projection erases a successful one

`ProjectView.tsx:177` renders the Core Team panel only when
`data.coreTeam.value && data.roles.value` are both present. If `/quick-roles`
fails while `/core-team` succeeds, the returned Core Team roster — a valid
server projection — is replaced by the refusal banner from the *other* read.
`ProjectView.tsx:218` couples completion state to the completion-profile catalog
the same way, hiding phase, revision and outstanding blockers when only the
catalog is unavailable.

This contradicts *"One OP-05/06 `unavailable`… must not suppress already-valid
topology, Core Team or capacity evidence"* and the proof *"a failed panel does
not erase successful sibling projections."*

The existing test for this behaviour (`'keeps independent projection refusals
visible while rendering the rest'`) only fails `committeeTemplates`, which is
the one genuinely independent case — so it passes while the coupled pairs stay
broken. Render the roster and the completion state on their own reads, and let
the editor affordance degrade instead of the evidence.

#### 5. Contracts the architecture assigns browser behavior, with no client method

Absent from `client.ts` and the view:

- `POST /epics/{epic_id}/core-team/seats:materialize` — the contracts table
  requires an explicit materialize action ("never imply that project Core Team
  apply created a live seat"). The panel correctly shows "not materialized", but
  offers no way to act on it.
- `POST /advisor-runs/{advisor_run_id}:settle`
- `POST /committee-runs/{committee_run_id}/findings:record` and `…/settle`

Runs can be invoked and never settled from the console. If deferring these to
OP-10 is intended, record the deferral; right now the omission is silent.

#### 6. The horizontally scrolling topology region is not keyboard reachable

`ProjectView.tsx:271` — `<div className="table-scroll">` with
`overflow-x: auto` and `min-width: 52rem` on the table. At phone width the
committed screenshot confirms the table is clipped mid-column ("Lead Softw…").
The div carries no `tabindex`, `role` or accessible name, so a keyboard-only
operator cannot scroll it. That is the *"keyboard-only desktop and phone-width
paths reach the same data"* proof. `tabindex={0}` plus
`role="region"` and a label is the whole fix.

### What is sound

Recorded so the next round does not re-litigate it:

- **No generated-client drift.** `schema.d.ts` was regenerated, not hand-edited:
  `CoreTeamSeatSelectionDto` was already in `contract/openapi.json` at the seed
  (the OP-04 correction) and the committed file now matches generator output
  byte for byte.
- **Types are pure aliases.** All 30 additions in `api/types.ts` alias
  `Schemas[...]`; no DTO is redefined or reshaped.
- **One transport class.** Idempotency lives only in `#command`, and the
  header/verb mapping is right everywhere else: every `#command` target is a
  route the contract marks `Idempotency-Key`-required, and `core-team:preview`
  correctly uses the keyless `#post`.
- **`catalogRevision` is server-owned, not inferred.** The fallback
  `help.find(e => e.category === 'role')?.source` is legitimate: the daemon
  stamps every role entry with one `catalog_source`
  (`kontor-daemon/src/applications.rs:5411` — "the same one every seat in it is
  recorded under"), so this reads a returned revision rather than guessing one.
- **The rename is done properly.** `Teams` → `Delivery Teams` changes labels
  only; the view id stays `teams`, preserving route and identity as instructed.
- **`CodeHelp` is genuinely accessible.** Real `<button>`, `aria-expanded`,
  `aria-describedby`, category participating in lookup, unknown codes left
  visible and named as unknown, and disclosure on hover, `:focus-within` and
  click alike — all three modalities, not a bare `title`.
- **All seven required sections are present** and, per the committed phone
  screenshot, stack to one column at 390px with no section or action dropped.
- **No foreign authority.** Scans find no Jira, AgentsRoom or Paseo endpoint and
  no non-`/v1` fetch from console sources.

### Required to clear this gate

1. Make `cargo test --workspace` green at the reviewed commit (Blocking 1).
2. Send `promotion:preview` as POST, with a test that asserts the verb
   (Blocking 2).

Items 3–6 should be addressed or explicitly deferred with a reason before OP-10
takes this surface as its integrated-proof baseline.
