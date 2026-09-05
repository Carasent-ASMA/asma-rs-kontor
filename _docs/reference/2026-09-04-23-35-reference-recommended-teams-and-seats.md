# Recommended teams and seats setup

> **Date:** 2026-09-04 23:35
> **Status:** 🟡 Recommendation; implementation limits stated below
> **Category:** reference
> **Scope:** Kontor team design, role responsibilities and model-evaluation policy
> **Summary:** Recovers the model-independent fleet recommendations from the documentation branch, while distinguishing design policy from implemented enforcement.

## When to Load

Load when designing a team, reviewing seat responsibilities or evaluating a model for a role. Read the selected Kontor definitions for the actual running configuration.

## Implementation boundary

This reference derives from the 2026-08-29 fleet design. The source was
`docs/RECOMMENDED-TEAMS-AND-SEATS.md` at commit `e6da2708e455` of the stranded
documentation branch. It is not evidence that the complete proposed fleet
policy is currently enforced.

| Shipped control | Remaining design policy |
| --- | --- |
| Immutable team/profile pins, declared slots, explicit provider/model routes, durable quota observations, account-before-rung routing, evidence and verdict gates | Automatic enforcement of every four-rung budget/platform constraint, calibration gate, vision attestation and post-activation canary rule described below |
| Configured committee templates and read-only consultation authority | The illustrative fleet size, role mixes and context targets recommended here |

Changing a live selection requires a new supported definition and explicit
preview/apply where that contract provides it. Preserve historical pins and
receipts. A role name or a paragraph in this document grants no authority.

**This is a recommendation to start from, not a prescription.** Teams, seats,
chains, committees and advisors are versioned configuration; every adopter is
expected to tailor them to their own work types, risk profile, providers and
budgets — add teams, drop seats, re-cut chains, rename roles. What this
document offers is a proposed operating shape and the reasoning behind it, so tailoring
starts from explicit responsibilities and tradeoffs. The
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

The shapes below are **configuration recommendations**. In Kontor, teams, role
catalogs, chains, committees, advisors and completion profiles are versioned
data published through `/v1` (see [CONFIGURATION.md](../../docs/CONFIGURATION.md)); the historical AgentsRoom manifests are import/design references, not live
Kontor authority. The current supported model catalog and its validator remain
code-reviewed and tested; publishing a new selection does not rewrite existing pins. ASMA policy and historical instantiation references live in the deployment repo
(`asma-modules/_docs/ai-orchestration/architecture/2026-08-05-01-07-architecture-agent-fleet-roles-model-policy.md`
§0, and `asma-modules/_tools/ai-orchestration/manifest/{teams,advisors}/`) and
are expected to change without this document changing.

## 1. Capability dimensions — the vocabulary

Every candidate model is scored on these axes. Seat requirements below are
written only in this vocabulary.

| Dimension | What it measures | How it is established |
|---|---|---|
| **Reasoning class** | Depth on novel, multi-constraint problems: `frontier` (best available judgment), `strong` (reliable on hard but bounded work), `mid` (competent on well-specified work), `floor` (cheap breadth, low trust) | Public benchmarks are a *screen*, never an admission: class is confirmed by seat-class trial (§6) |
| **Context class** | Usable window under real seat load | Deterministic classes per the seat context/compaction policy — `lean` (128K) / `standard` (256K) / `deep` (512K) / `extended` (720K) / `native` (explicit escape hatch only); measured, not vendor-quoted. These five values are implemented; approved `large` (400K) remains unimplemented and cannot be sent as an API value. Per-seat recommendations: §2.6 |
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

### 1.1 Recommended fleet size — how many vendors, how many models

If you can run multiple vendors and multiple models (and today almost everyone
can), size the fleet from the rules it must satisfy rather than from taste.
The minimums below are *derived*, not aesthetic: a four-seat team needs a
different vendor on every rung 1 (§4.3), every chain needs four distinct
budgets on at least three platforms (§4.2), and verdict seats must survive the
collision rule (§4.4) — an audit excluded from the builder's vendor must still
have an admissible model left.

| Resource | Minimum viable | Recommended | Why |
|---|---|---|---|
| **Budget domains ("vendors")** | 4 | **5–6** | 4 is forced by team composition + chain grammar; the 5th and 6th buy outage slack — with exactly 4, one vendor outage plus one collision skip can leave a verdict seat below rung 2, parking the work. (The v4 review recorded precisely this as an accepted residual on its own Judge chain.) |
| **Platforms** | 3 | **4+** | ≥3 per chain is the grammar; a 4th decorrelates the chains that share a platform behind distinct budgets |
| **Models total** | ~7 | **8–12** | Enough to cover every class below twice; capped by what your periodic model review can genuinely re-score — an uncalibrated model in the catalog is a liability, not capacity |

Per capability class, hold **coverage**, not just counts — the invariant is:
*every capability class that any mandatory seat requires at rung 1 must have at
least two admissible models on two different budget domains (and ideally two
platforms), so that no single outage, quota event or collision skip removes the
class from the fleet.*

| Capability class | Hold at least | Why |
|---|---|---|
| `frontier` reasoning | **2, on different budget domains** | Final gates and judges need it, and the self-review ban means a committee judging frontier output needs a *different* frontier model |
| `strong` reasoning | **2** | Architect/audit fallback rungs; collision skips land here |
| Cost-elastic `mid` (fast/flash class) | **2–3** | The volume seats (build, verify, pr-check) live here; this is where cheaper weekly arrivals usually slot first |
| `floor` (free/near-free) | 0–2, floor rungs only | Optional; mind shared account caps on free tiers — a per-account daily cap shared by four seats is one budget, not four |
| **Vision-attested** | **2, on different budget domains** | Browser-verifying seats need one at rung 1 *and* an admissible fallback that isn't the same vendor |
| Very-large-context (`extended`-capable) | **1+** | LSA / research / high-stakes build ceilings |

Overlap is expected — one model may be both `strong` and vision-attested; the
table counts capabilities, not exclusive slots. A historical instantiation of this sizing (six budget domains, ten models)
lives in the deployment repo referenced above. Revalidate its routes and
constraints before using it as a current configuration.

**Within a class, diversity is counted in vendors, not in models.** Two
sibling models from the same vendor count as *one* toward every floor in the
table above, for three reasons that compound:

1. **Availability correlation.** Siblings share a budget domain and usually a
   platform — one billing lapse, quota event or outage removes all of them in
   the same instant, which is exactly the event the floors exist to survive.
2. **Collision amplification.** The vendor-collision rule (§4.4) skips by
   *vendor*, not by model: a class stocked entirely with one vendor's siblings
   is erased from a review chain by a single collision with the builder's
   vendor, no matter how many models it contains.
3. **Behavioral correlation.** Models from one vendor may share training lineage and failure modes.
   Independent providers are a useful review precaution; measure defect
   detection instead of treating vendor diversity as proof of independence. This is why committee seats and judges should never be
   siblings of each other or of the seat under review, even when the letter of
   the collision rule would allow a same-vendor different-model pairing.

Holding several models from one vendor is still legitimate — as cost/latency
tiers inside a class for worker seats (a frontier and a fast variant from the
same house), and as quota headroom. They add **capacity**, never **coverage**.
Rule of thumb: at most two models per vendor within a class, and never satisfy
a class's coverage floor from a single vendor.

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
| **TPM / Orchestrator** | **Mandatory in the recommended ASMA definition** — one per epic ECP | Dispatch, sequencing, typed EMERGENCY declarations, handoffs, escalation briefs; touches every ticket, judges none | `mid` reasoning is enough; cheapest capable tier; the one seat where *reduced* effort is the designed exception (it runs constantly) |
| **LSA (Lead/Epic Architect)** | Epic-local, mandatory per epic | Owns the epic's architecture narrative across tickets; consistency between per-ticket plans | `frontier`/`strong`; largest context class in the fleet |
| **PR-check** | **Static single seat — no merge verdict, no pass flags, ever** | Run the owning submodule's checks, read the diff, post review comments | `mid`+ reasoning; high-volume cost tier; because it issues no verdict, it deliberately does NOT carry the non-waivable collision clause — do not "fix" it in |
| **PR Gatekeeper** | Standalone reviewer | Review lane for PRs; on high-stakes it escalates to the human rather than covering acceptance | `strong`+; independence from the Builder |
| **Inspector (standalone)** | Roaming audit seat | As team Inspector, callable outside a team run | As team Inspector; collision-skip against whoever built |
| **Manual Test Lead** | Standalone | Design and drive manual test passes humans or QA Bots execute | `strong`; browser familiarity; vision helpful |
| **Analyst (model review)** | Standalone, periodic | Runs the model review itself; proposes chain changes | `frontier`/`strong`; must never run on a route whose placement it is judging (self-review ban) |

### 2.4 Consultation seats

The table illustrates the shipped Independent Review template. Cardinality,
role labels, diversity, quorum and rounds come from the immutable template and
Team Definition pinned to the consultation. It is not a universal three-seat
rule. Advisors and committee members remain read-only; authorized callers
record dispositions through Kontor. An advisor profile governs consultation
permissions and recursion, rather than this illustrative table.

| Seat | Responsibilities | Required capabilities |
|---|---|---|
| **Advisor** (Architecture / Security / UX / Cost-Capacity / Performance) | One bounded second opinion; recursion and invocation limits come from the pinned advisor profile | Reasoning class matched to domain stakes (Security advisor = highest verdict-trust tier; Cost advisor can be cheap); UX advisor needs attested vision |
| **Committee Seat A / Seat B** (reviewers) | Independent findings, recorded before seeing each other's; evidence with file/line; explicit verdict recommendation | `strong`+ reasoning; **provider diversity between the seats is a template constraint**; verdict trust matters — a reviewer asserting a clean sweep that is not clean is a calibration failure |
| **Committee Judge** | Verify load-bearing claims itself (never accept, never average, never restart the debate); aggregate by the declared deterministic rule; preserve dissent verbatim; return the result to the initiating caller, which records its disposition and any authorized memory proposal | `frontier` — its entire output is the decision; must not share either debater's actually-run model (and should not share their vendors); read-only |

Use a committee when the pinned completion profile requires a committee
verdict. A requirement for independent review alone does not prescribe a
committee shape. The historical fleet-policy review used three rounds; that
receipt is not a universal round count or invocation policy.

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

**Approved residual: `large` (400K).** This design retains a proposed class
between `standard` and `deep` for pricing boundaries and measured windows near
400K. It is absent from the implemented `ContextWindowClass` enum at released
commit `1b0b27e7db758a8788efb3d722bcdd6d7a1d54e3`; do not submit `large` in
configuration or describe the proposed override as available.

When scoring a candidate model (§6), its measured usable window must cover the
seat's maximum class, not just its default. Map down to an implemented class
that it fully covers: a model with a measured 400K usable window currently fits
`standard`, not `deep`. A future implementation of `large` needs its own schema,
validation and capability evidence before these recommendations can use it.

## 3. Recommended teams — what each team is for

| Team | Designed for | Seats (mandatory in bold) | Notes |
|---|---|---|---|
| **Plan, build, verify** | Standard feature/bug delivery in a polyrepo: plan-first, checkpointed build, verified against the plan | **Architect, Builder, QA, Spec Audit** | The default team; four seats, four distinct rung-1 vendors |
| **High-stakes** | Security-critical, tenant-isolation, auth, migration, money-touching tickets | **Architect, Builder (high-stakes tier), QA, Spec Audit** | Same shape, escalated capability classes; the Audit seat here is the most protected verdict in the fleet — any capability downgrade on it requires an explicit gate pair: pre-admission calibration + post-activation control (first N verdicts re-read by a higher class seat) |
| **UX · Design & Prototype** | Prototype-driven UX: the coded prototype is the artifact | **Research & Design, Prototype Build, Verify (browser), Spec Audit** | Verify's rung 1 must be vision-attested before the team may launch at all |
| **PR Check** | Static review of every PR: checks + diff comments | **pr-check** (single) | No verdict authority by design; exists so review coverage never depends on a team run |
| **Committee: Independent Review** | Completion verdicts, policy-change verdicts, contested findings | **Seat A, Seat B, Judge** | Read-only; independent findings precede aggregation; scope, invocation limits and round cap come from the pinned templates |
| **Advisors** | Bounded one-shot second opinions per domain | One advisor seat per domain | Never a substitute for the committee on completion truth |

All teams are **templates** — versioned, importable, replaceable. Adding a
team for a new work type (research, docs, operations) is a data change: define
seats in this vocabulary, apply §4 to cut chains, and publish.

## 4. Chain composition principles

A **chain** is the ordered fallback list for one seat; one entry is a
**rung** (rung 1 … rung 4). The chain is a proposed policy representation. Kontor routes the stored
provider/account/model candidates deterministically; the full four-budget,
three-platform grammar below is a recommendation, not a shipped universal validator.

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
  data.** Changing a selected model requires a new immutable definition and an
  admission receipt. A previously unsupported model also requires a reviewed
  catalog/validator change in the current implementation.
- **What IS code** are the safety invariants the data cannot override: one
  non-terminal session per role slot, proposal ≠ authority, verdict gates,
  frozen-evidence hashing, and the guardrail that no advisor, committee or
  agent grants missing authority.
- **Recommended fail-closed interim.** While any rung is gated (calibration pending,
  balance drained), live teams keep their previous pins; new surfaces whose
  rung 1 is inadmissible do not launch; import of a manifest whose gates are
  open is itself blocked. Absence of a receipt means *no*, not *probably fine*.

## 6. The model evaluation pipeline — how a new model enters the fleet

Run this whenever a promising model appears. The proposed output is durable review and calibration evidence. Do not claim
that admission enforces a calibration receipt unless the pinned contract and
its regression tests demonstrate that check.

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

- [CONFIGURATION.md](../../docs/CONFIGURATION.md) — where each specification lives and
  the invariants-versus-data split.
- [QUOTA-FALLBACK-PLAN.md](../../docs/QUOTA-FALLBACK-PLAN.md) — the durable quota state
  behind the rung walk (account-before-rung, blocked-until, headroom).
- Deployment-side policy and historical instantiation references: `asma-modules/_docs/ai-orchestration/architecture/2026-08-05-01-07-architecture-agent-fleet-roles-model-policy.md` §0
  and `asma-modules/_tools/ai-orchestration/manifest/{teams,advisors}/`.
- Seat context classes: the seat context-window and compaction policy in the
  deployment repo's `_docs/ai-orchestration/architecture/`.
