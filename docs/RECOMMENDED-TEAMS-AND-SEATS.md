# Recommended teams and seats setup

Status: **Recommended starting point — not the only possible setup.** Derived
from the 2026-08-29 v4 fleet restructure (three-round independent-review
committee, final verdict `compliant`).
Date: 2026-08-29

**This is a recommendation to start from, not a prescription.** Teams, seats,
chains, committees and advisors are versioned configuration; every adopter is
expected to tailor them to their own work types, risk profile, providers and
budgets — add teams, drop seats, re-cut chains, rename roles. What this
document offers is a proven shape and the reasoning behind it, so tailoring
starts from something that already works rather than from a blank page. The
only parts that are not yours to tailor are the code-enforced safety
invariants listed in §5 (one non-terminal session per role slot, proposal ≠
authority, verdict gates, frozen evidence) — everything else is data and is
meant to be changed.

This document is deliberately **model-free**. New, more capable and cheaper
models appear weekly; any document that names one is stale on arrival. What
does not churn is the *work*: the types of tickets a team exists for, the
responsibilities each seat must discharge, and the capabilities a model must
demonstrably have before it may hold that seat. This recommendation records those
three things, plus the principles for ordering a fallback chain and the
evaluation pipeline that turns any new model into a placement decision.

Everything below is **configuration, not code**. In Kontor, teams, role
catalogs, chains, committees, advisors and completion profiles are versioned
data published through `/v1` (see [CONFIGURATION.md](CONFIGURATION.md)); in the
AgentsRoom/Paseo manifests they are JSON files. No model name belongs in
source. The current concrete instantiation — which model sits on which rung
today — lives in the deployment repo
(`asma-modules/_docs/ai-orchestration/architecture/2026-08-05-01-07-architecture-agent-fleet-roles-model-policy.md`
§0, and `asma-modules/_tools/ai-orchestration/manifest/{teams,advisors}/`) and
is expected to change without this document changing.

## 1. Capability dimensions — the vocabulary

Every candidate model is scored on these axes. Seat requirements below are
written only in this vocabulary.

| Dimension | What it measures | How it is established |
|---|---|---|
| **Reasoning class** | Depth on novel, multi-constraint problems: `frontier` (best available judgment), `strong` (reliable on hard but bounded work), `mid` (competent on well-specified work), `floor` (cheap breadth, low trust) | Public benchmarks are a *screen*, never an admission: class is confirmed by seat-class trial (§6) |
| **Context class** | Usable window under real seat load | Deterministic classes per the seat context/compaction policy — `lean` (128K) / `standard` (256K) / `large` (400K) / `deep` (512K) / `extended` (720K) / `native` (explicit escape hatch only); measured, not vendor-quoted. Per-seat recommendations: §2.6 |
| **Vision** | Can it *judge pixels* — screenshots, layout, contrast, state | **Attested only**: a vendor "multimodal" claim or a null capability flag is not vision; a calibration receipt with real screenshot judgments is |
| **Tool/agentic reliability** | Long tool chains, edit discipline, no drift, no fabricated tool results | Trial tickets with transcript audit |
| **Verdict trust** | The audit-class trait: false-pass rate on seeded defects; does it assert clean sweeps that are not clean | Calibration with deliberately seeded defects (mutation-style); a model that misses a seeded P0, or asserts a false negative sweep, may work but may not judge |
| **Effort lever** | Does the effort/thinking setting actually change behavior on this route | Empirical mapping per route; a no-op lever must be recorded as a no-op |
| **Cost & latency tier** | Marginal cost per delivered unit and turnaround | Live pricing + measured turnaround; re-checked at every model review |
| **Budget domain** | Which account/payment pool it drains (the *money* axis) | Account topology facts; ruled by the operator, never inferred from a route name |
| **Platform** | Whose infrastructure outage kills it (the *availability* axis) | Route topology facts; two budget-distinct routes can still share a platform — state the correlation |
| **Route provenance** | One canonical, dispatchable, round-trip-attestable model id under a provider that exists in the runtime catalog | Live catalog readback + a dispatch check; prose spellings must equal the machine id character-for-character |

Two axes deserve emphasis because they are independent and both matter:
**budget** protects against quota/spend exhaustion; **platform** protects
against outage. A chain diversified on one can still be concentrated on the
other. Every chain must state its known correlations rather than hide them.

## 2. Recommended seats

A **seat** is a role slot in a team run: stable id, one non-terminal session at
a time (code-enforced invariant), authority defined by its role — never by
what the occupant claims. Seats divide into **worker seats** (produce
artifacts), **verdict seats** (set pass flags others depend on), **static
seats** (fixed function, no verdict), and **mechanism seats** (deterministic
sub-processes with model-driven steps).

The single most important split: **verdict authority**. A worker seat degraded
to a weak model still produces useful work; a verdict seat degraded to a weak
model produces a *false pass*, which in a clinical product is the expensive
failure. Every rule below that seems pedantic exists to protect verdict seats.

### 2.1 Delivery-team seats

| Seat | Responsibilities | Verdict? | Required capabilities |
|---|---|---|---|
| **Architect** (Scope & ADR) | Turn the ticket into an executable plan: scope, ADR conformance, file-level design, acceptance criteria, risk register | No pass flag, but its plan gates everything downstream | `frontier`/`strong` reasoning; large context class (reads whole subsystems); high tool reliability for exploration |
| **Builder / Implement** | Produce the change exactly to plan; checkpoint commits; report deviations rather than absorb them | No | Reasoning class by ticket tier (see §2.5); strong edit discipline; context class fitting the touched surface; cost-elastic — this is the highest-volume seat |
| **QA / Verify** | Execute the plan's verification steps; run the owning module's tests; exercise UI flows in the real browser; state explicitly what was NOT verified | **Yes — `qaPassed`** | `strong`+ reasoning; **attested vision when the seat verifies in a browser**; high verdict trust; honesty about coverage gaps is the trait, not test-writing skill |
| **Spec Audit / Inspector** | Independent read of the diff against the approved intent: did we build the thing we said, are the risks addressed, is the evidence real | **Yes — `auditPassed`** (the final gate) | Highest verdict trust in the team; `strong`+ reasoning; must be *independent* — never the Builder's vendor (non-waivable, §4) |

### 2.2 UX-team seats (prototype-driven charter)

The UX team exists for work whose artifact is a **coded prototype**, not a
document: research and design intent feed directly into running UI, revised
live.

| Seat | Responsibilities | Verdict? | Required capabilities |
|---|---|---|---|
| **Research & Design** | Design intent directly buildable as a prototype: screens/states, component mapping to the design system, tokens, breakpoints, a11y requirements, flows to verify | No | Very large context class (design systems + long docs); `strong`+ reasoning; vision helpful, not required (it reads specs more than pixels) |
| **Prototype Build** | Code the prototype from intent, revise live; reuse existing components — a second implementation of an existing component is a defect | No | `mid`+ reasoning at high volume; strong frontend tool reliability; cost-elastic |
| **Verify (browser)** | Entire evidence is pixels: exercise every flow, judge layout/contrast/states at declared breakpoints | **Yes — `qaPassed`** | **Attested vision at rung 1 is mandatory** — a text-only model here can only ever pass on text-snapshot evidence and must withhold on pixel judgment |
| **Spec Audit** | As delivery-team Inspector, over design intent + prototype | **Yes** | As delivery-team Inspector |

### 2.3 Static and mandatory standalone seats

| Seat | Nature | Responsibilities | Required capabilities |
|---|---|---|---|
| **TPM / Orchestrator** | **Mandatory** — one per project scope | Dispatch, sequencing, typed EMERGENCY declarations, handoffs, escalation briefs; touches every ticket, judges none | `mid` reasoning is enough; cheapest capable tier; the one seat where *reduced* effort is the designed exception (it runs constantly) |
| **LSA (Lead/Epic Architect)** | Epic-local, mandatory per epic | Owns the epic's architecture narrative across tickets; consistency between per-ticket plans | `frontier`/`strong`; largest context class in the fleet |
| **PR-check** | **Static single seat — no merge verdict, no pass flags, ever** | Run the owning submodule's checks, read the diff, post review comments | `mid`+ reasoning; high-volume cost tier; because it issues no verdict, it deliberately does NOT carry the non-waivable collision clause — do not "fix" it in |
| **PR Gatekeeper** | Standalone reviewer | Review lane for PRs; on high-stakes it escalates to the human rather than covering acceptance | `strong`+; independence from the Builder |
| **Inspector (standalone)** | Roaming audit seat | As team Inspector, callable outside a team run | As team Inspector; collision-skip against whoever built |
| **Manual Test Lead** | Standalone | Design and drive manual test passes humans or QA Bots execute | `strong`; browser familiarity; vision helpful |
| **Analyst (model review)** | Standalone, periodic | Runs the model review itself; proposes chain changes | `frontier`/`strong`; must never run on a route whose placement it is judging (self-review ban) |

### 2.4 Consultation seats

| Seat | Responsibilities | Required capabilities |
|---|---|---|
| **Advisor** (Architecture / Security / UX / Cost-Capacity / Performance) | One bounded second opinion; depth 1 — may not summon further advisors or committees | Reasoning class matched to domain stakes (Security advisor = highest verdict-trust tier; Cost advisor can be cheap); UX advisor needs attested vision |
| **Committee Seat A / Seat B** (reviewers) | Independent findings, recorded before seeing each other's; evidence with file/line; explicit verdict recommendation | `strong`+ reasoning; **provider diversity between the seats is a template constraint**; verdict trust matters — a reviewer asserting a clean sweep that is not clean is a calibration failure |
| **Committee Judge** | Verify load-bearing claims itself (never accept, never average, never restart the debate); aggregate by the declared deterministic rule; preserve dissent verbatim; own the memory write | `frontier` — its entire output is the decision; must not share either debater's actually-run model (and should not share their vendors); read-only |

The committee is **mandatory in the ASMA setup** wherever the completion
profile names an independent verdict: epic completion, and any change to the
fleet policy itself (this document's own lineage — three rounds, dissent
preserved, owner rulings recorded as overrulings rather than agreement).

### 2.5 Builder tiers and mechanism seats

**Builder tiers** route the same seat to different capability classes by
ticket risk: `chore` (floor/mid class, cheapest route that passes trial),
`standard` (mid/strong, cost-elastic), `high-stakes` (frontier; security,
tenant-isolation, auth, migration, money — with a human-called escalation
route outside the chain walk for the truly exceptional case).

**Mechanism seats** are governed too — nothing model-driven is exempt:

- **QA Bot** (browser execution + `submit_verdict`): follows the vision rule;
  a text-only rung may drive it **only on text-snapshot evidence**; pixel
  judgment escalates to a vision-attested rung.
- **Research mechanism** (Researcher A/B, Research Judge, Synthesizer, Final
  Reviewer): inherit the caller's chain until given explicit chains, **except**
  independence binds inside the mechanism — the Research Judge never shares a
  researcher's actually-run model.

### 2.6 Recommended context class per seat

Classes are auto-compaction trigger targets, not model-window declarations —
the runtime never overrides a model's physical window. A seat gets the
smallest class that fits its evidence discipline: durable state belongs in the
control plane, verdict evidence outside the transcript; chat history is not a
database. `extended` and `native` always require an explicit work-profile,
role-slot or authorized run override — a model may not promote itself because
it judges the task hard.

| Seat | Default class | Max automatic class | Rationale |
|---|---|---|---|
| TPM / Orchestrator | `lean` | `standard` | Scheduler/reconciliation state is durable in the control plane, not chat |
| Advisor (every domain) | `lean` | `standard` | One bounded second opinion on a bounded evidence bundle |
| Builder — chore | `lean` | `standard` | Narrow mechanical work |
| Builder — standard / Prototype Build | `standard` | `deep` | Normal code-and-test surface |
| Builder — high-stakes | `deep` | `extended` | Security, tenancy, migrations: the whole blast radius must fit |
| Architect (Scope & ADR) | `deep` | `extended` | Cross-ticket decisions and integration surface |
| LSA (epic architect) | `deep` | `extended` | The epic-wide narrative; the largest sustained context need in the fleet |
| UX Research & Design | `deep` | `extended` | Design systems plus long intent documents |
| QA / Verify (incl. UX browser Verify) | `standard` | `deep` | Preserve current defect evidence, not all exploration noise |
| Spec Audit / Inspector / PR Gatekeeper / Manual Test Lead | `standard` | `deep` | Verdict evidence is durable outside the transcript |
| PR-check (static) | `lean` | `standard` | One diff plus the owning module's checks |
| Committee Seat A / Seat B / Judge | `standard` | `deep` | Each receives the bounded evidence bundle, never every source transcript |
| Research mechanism / Analyst | `deep` | `extended` | Large source sets, only when the work profile declares them |
| QA Bot (mechanism) | `lean` | `standard` | Snapshot evidence in, verdict out |

**Where `large` (400K) fits.** No seat defaults to it, deliberately: it is the
explicit pricing-boundary / provider-tier selection *between* `standard` and
`deep`. An operator (or work profile) picks it in two situations: a
`standard→deep` seat whose evidence bundle keeps overflowing 256K but whose
provider prices the next tier punitively — `large` buys the headroom without
paying the deep tier; or a model whose **measured** usable window sits near
400K, which can hold a `deep`-default seat only at `large` with that ceiling
recorded on the seat as a documented override, never silently.

When scoring a candidate model (§6), its *measured* usable window must cover
the seat's **max automatic class**, not just the default — otherwise the seat
cannot legally grow into its own ceiling under load. A window landing between
classes maps **down** to the class it fully covers (a ~400K model is a `large`
model, not a `deep` one); rounding up is how a seat ends up compacting in the
middle of the work it was sized for.

## 3. Recommended teams — what each team is for

| Team | Designed for | Seats (mandatory in bold) | Notes |
|---|---|---|---|
| **Plan, build, verify** | Standard feature/bug delivery in a polyrepo: plan-first, checkpointed build, verified against the plan | **Architect, Builder, QA, Spec Audit** | The default team; four seats, four distinct rung-1 vendors |
| **High-stakes** | Security-critical, tenant-isolation, auth, migration, money-touching tickets | **Architect, Builder (high-stakes tier), QA, Spec Audit** | Same shape, escalated capability classes; the Audit seat here is the most protected verdict in the fleet — any capability downgrade on it requires an explicit gate pair: pre-admission calibration + post-activation control (first N verdicts re-read by a higher class seat) |
| **UX · Design & Prototype** | Prototype-driven UX: the coded prototype is the artifact | **Research & Design, Prototype Build, Verify (browser), Spec Audit** | Verify's rung 1 must be vision-attested before the team may launch at all |
| **PR Check** | Static review of every PR: checks + diff comments | **pr-check** (single) | No verdict authority by design; exists so review coverage never depends on a team run |
| **Committee: Independent Review** | Completion verdicts, policy-change verdicts, contested findings | **Seat A, Seat B, Judge** | Read-only; findings independent-then-aggregated; one committee per ticket without owner approval; hard round cap |
| **Advisors** | Bounded one-shot second opinions per domain | One advisor seat per domain | Never a substitute for the committee on completion truth |

All teams are **templates** — versioned, importable, replaceable. Adding a
team for a new work type (research, docs, operations) is a data change: define
seats in this vocabulary, apply §4 to cut chains, and publish.

## 4. Chain composition principles

A **chain** is the ordered fallback list for one seat; one entry is a
**rung** (rung 1 … rung 4). The chain is data on the seat; the walk is
performed by deterministic routing at import/admission time — the model never
chooses its own rung.

1. **Rung 1 is the designed best fit, not the best model.** Score candidates
   on §1 and place by *capability-fit × budget elasticity*: scarce frontier
   capacity goes where judgment is dearest (audit, judge, high-stakes build);
   elastic cheap capacity goes to high-volume seats (build, verify, pr-check).
   The seat that catches bugs must never be the weakest model available.
2. **Four rungs; four distinct budget domains; at least three distinct
   platforms; no repeats within a chain.** State every known correlation
   (two budget-distinct rungs on one platform) in the chain itself.
3. **Team composition: every seat a different vendor at rung 1.** Same-vendor
   sharing exists only under a **typed EMERGENCY**: the designed rung-1 vendor
   inadmissible across ≥2 vendors at once, declared by the TPM with evidence,
   recorded, expiring at the next admission window.
4. **Collision-skip, and the non-waivable clause.** An audit never runs the
   dev's vendor; a judge never a debater's; an inspector never the builder's —
   skip to the next rung, never wait. The predicate is the model the other
   seat **actually ran**, not its designed rung; committee/advisor seats
   resolve jointly against seats already live. If the collision cannot be
   skipped, the verdict seat **does the work, reports everything, and leaves
   its pass flag false regardless of rung depth** — an EMERGENCY declaration
   does not lift this. This clause belongs in the seat's own instructions (the
   text the model reads), not only in policy.
5. **Vision rule.** A browser-verifying seat requires an *attested* vision
   model at rung 1. Text-only models are marked `textOnly` on their rungs and
   may serve such chains only in degraded tails, passing only on text-snapshot
   evidence, withholding on any pixel judgment.
6. **Verdict gate.** Below rung 2, a seat may work but may not pass or merge.
   Degraded tails exist for work-continuity (never-wait applies to *doing*),
   never for judgment (always-wait applies to *passing*).
7. **Effort symmetry.** Cheap fast-class models run at maximum effort
   everywhere (their cost leaves no reason not to); the TPM/Orchestrator is
   the sole designed reduced-effort exception. Where a route's effort lever is
   a no-op, record that fact instead of pretending the setting works.
8. **Machine-readable or it does not exist.** Every rung carries structured
   `provider`, canonical `model`, `effort`, `vendor` (budget), `platform`, and
   where applicable `textOnly`, `visionRequired`, `calibrationGate`. The seat's
   node fields equal its designed rung 1 exactly; import-time routing assigns
   the highest *admissible* rung; round-trip readback must attest
   provider+model+effort character-for-character. Prose is commentary; fields
   are the contract. (The v4 review found its two worst defects exactly here:
   prose said one thing, the one readable field said another.)

## 5. Configurability contract

- **Chains, teams, committees, advisors, completion profiles are versioned
  data.** Changing a model pin is a manifest/spec edit plus an admission
  receipt — never a code change, never hardcoded.
- **What IS code** are the safety invariants the data cannot override: one
  non-terminal session per role slot, proposal ≠ authority, verdict gates,
  frozen-evidence hashing, and the guardrail that no advisor, committee or
  agent grants missing authority.
- **Fail-closed interim.** While any rung is gated (calibration pending,
  balance drained), live teams keep their previous pins; new surfaces whose
  rung 1 is inadmissible do not launch; import of a manifest whose gates are
  open is itself blocked. Absence of a receipt means *no*, not *probably fine*.

## 6. The model evaluation pipeline — how a new model enters the fleet

Run this whenever a promising model appears. The output is receipts, and the
receipts are what admission reads.

1. **Route attestation.** Establish the one canonical id under a provider that
   exists in the runtime catalog (live catalog readback, not documentation).
   Run a dispatch check: the route answers, identifies itself, and reasons.
   Record the id; every manifest spells it identically from then on.
2. **Capability scoring.** Score §1's dimensions with evidence: context class
   under real load; tool-chain trial; effort-lever mapping (empirical); cost
   and latency measured. Vendor claims are hypotheses.
3. **Vision attestation** (only if the model will ever serve a vision rung):
   real screenshot-judgment tasks with recorded outcomes. `null` or
   vendor-claimed multimodality is *unattested* and keeps the model
   inadmissible on vision-required rungs at any depth.
4. **Seat-class trial (calibration gate).** Disposable ticket(s) matching the
   target seat class. For **verdict seats**, calibration must include seeded
   defects the model must catch and a check that its negative claims ("no X
   remains") are actually true — a reviewer that asserts a false clean sweep
   has failed audit calibration regardless of what it builds.
5. **Placement decision.** Assign rung and seat(s) by §4.1; map budget domain
   and platform; declare correlations; set `calibrationGate` fields naming the
   receipts.
6. **Admission and canary.** Record receipts; update manifests (data change);
   re-attest live balances for paid routes at admission; **atomic
   publish-and-canary** — never edit a live team in place. For verdict seats,
   add a **post-activation control**: the first N verdicts after activation are
   re-read by a seat of the class being replaced before they stand.
7. **Periodic review and demotion.** The monthly model review re-scores
   incumbents; a cheaper model that passes the same receipts takes the rung.
   Demotion is the same data change in reverse; a model that loses its route,
   its budget, or its calibration validity becomes inadmissible fail-closed.

## 7. Related documents

- [CONFIGURATION.md](CONFIGURATION.md) — where each specification lives and
  the invariants-versus-data split.
- [QUOTA-FALLBACK-PLAN.md](QUOTA-FALLBACK-PLAN.md) — the durable quota state
  behind the rung walk (account-before-rung, blocked-until, headroom).
- Deployment-side instantiation (current pins, live chains, calibration
  tickets): `asma-modules/_docs/ai-orchestration/architecture/2026-08-05-01-07-architecture-agent-fleet-roles-model-policy.md` §0
  and `asma-modules/_tools/ai-orchestration/manifest/{teams,advisors}/`.
- Seat context classes: the seat context-window and compaction policy in the
  deployment repo's `_docs/ai-orchestration/architecture/`.
