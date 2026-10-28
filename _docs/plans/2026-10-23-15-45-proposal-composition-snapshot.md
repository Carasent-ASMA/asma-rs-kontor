# Composition snapshot and mediated seat creation proposal

**Status: P1 implemented; P2 selected-folder preview implemented for review; P3–P5 remain dependent work.**
ASMA-8539. The scoped-grant future target retains amendments A1–A5. P1 exports
a pure library function; it registers no API route and adds no authentication
grant, selected-folder reader, database migration or native operation.
Configuration ownership remains with WP08/WP10. The future write target's
design disposition is not platform ADR ratification, write-operation admission,
grant issuance or an amendment to the [architecture](../../ARCHITECTURE.md).
It needs no repeated decision or new gate.

The first core is a read-only composition preview. It combines canonical
component inputs with supplied exact subject identifiers and returns
the intended naming, profile, route and attachments, bound to their inputs by a
canonical hash. A preview is evidence for review; it is never permission to
create a session. A separate, authenticated operation would revalidate those
inputs immediately before persisting intent and dispatching a native effect.

## Executable phased delivery plan

Each phase is one coherent implementation/test checkpoint on the owning issue;
no later phase is implied by a P1 pass. The existing ASMA-8539 delivery owner
coordinates prerequisites through their owners, without a new coordinator.

| Phase / owner | Next action and commit boundary | Prerequisite and lift evidence | Acceptance scope |
| --- | --- | --- | --- |
| P1 / ASMA-8539 SWE | Export the pure `composition_snapshot` module; use existing profile/fleet/naming owners; make contract tests call it. Commit the module, export, tests and this indexed plan together; publish a draft PR to `master`. | Eligible on the authorized existing branch/base. No accepted-folder or write authority is needed because P1 does not consume folders or dispatch. | Reproducible canonical preview; exact supplied pins retained as unverified; planned attachments; explicit partial coverage; false actuation. Scoped checks and mutation kills precede publication. |
| P2 / WP08 ASMA-8570 owner, coordinated by ASMA-8562 TPM; integration by ASMA-8539 SWE | Finish final-head review and merge PR #435, then integrate the accepted parser/capture API into preview on a fresh authorized base. One integration checkpoint; no copied parser. | WP08 #435 merged at `a9cf0f175a0461133c3a47089f24b222ad088bf0`; use its `ConfigFolder::select`, `RealmPack` captures and fleet owner. Neutral examples are disposable test inputs, never an implicit runtime selection. | Selected pack byte pins and separately read fleet pin joined to the preview, with named missing/invalid input refusals. No atomic folder-generation, activation, write authority or installed-folder acceptance claim. |
| P3 / WP10 ASMA-8572 owner; schema owner WP08/TPM | WP10 #3652 merged at `e1e29ebd0324d801a222277ca03c2fc8be85b8f7`. Retain its configured-folder contract; separately qualify startup/rollback and coordinate a durable operation record after the neutral write contract qualifies. | The actual-folder input receipt predates the final WP10 head and is not exact-head runtime acceptance. Current store schema is 145; 143/144 are already allocated. No next migration is reserved here. Q-02/DEC-020 remain unchanged. | Explicit folder and independently registered identities, followed by an owner-approved additive record and isolated retry/concurrency tests. No fixture substitutes for live release acceptance. |
| P4 / ASMA-8539 SWE with existing backend/identity and native-adapter owners, coordinated by the delivery TPM | Qualify the issuer, delegation and authenticated transport/caller binding, then implement one mediated seat operation using A1–A5. Separate issuer/binding, durable operation and native correlation checkpoints as those contracts qualify. | P2/P3 accepted plus the qualified neutral issuer/verifier/revocation/transactional claim boundary. Existing authority-bound helpers cannot be relabeled. Native capabilities must prove the checks actually claimed; absent atomic hooks leave the stated race/bypass limit. | The dependent matrix below, using offline transport/native fakes and isolated stores first. Exactly one seat in an existing registered observed workspace; no adoption or governance transfer. |
| P5 / existing QA, AUD and gate owners; designated merge owner and installer | Review each admitted candidate at its exact head, run the required repository gates, then route to the existing merge owner and release/install owner for supported delivery and readback. | Independent verdicts, exact-head gate evidence, accepted prerequisites for the claimed slice and release/rollback evidence. No author merge, installation or native restart from this SWE slice. | State implemented/merged/qualified/released/deployed/enabled separately. The broader mandate stays open until supported delivery is verified. |

The original routing remains QA/AUD/gates → designated merge → designated
release/installation/readback. P1 is published for that route as a draft;
the remaining phases are not silently bundled into its acceptance.

## P1 library interface and coverage

[`composition_snapshot.rs`](../../crates/kontor-daemon/src/composition_snapshot.rs)
exports through `kontor_daemon::composition_snapshot`:

```text
preview_composition(&CompositionInput, &ProfilePackSpec, &FleetSnapshot)
    -> DomainResult<CompositionSnapshot>
CompositionSnapshot::document() -> &CanonicalDocument
CompositionSnapshot::hash() -> &ContentHash
```

`CompositionInput` carries `CompositionSubject` (native project/workspace,
explicit optional parent and `RoleSlotId`), `PackCategoryKey`, explicit fleet
binding, `NativeNameTemplate`, `NameSeparator`, `NativeNameValues`, `RolePrompt`,
`Eligibility` and evaluation `Timestamp`. Optional source and policy pin maps
use `ExternalId` keys and `ContentHash` values; empty maps mean no supplied
pins. Logical source IDs are not file paths to read. The core performs no
ambient clock, environment, filesystem, transport, store or native lookup.

The canonical `composition-preview@1` document contains the exact subject,
canonical profile-semantics hash, fleet byte hash, supplied source/policy pins,
rendered naming, verified `ResolvedProfileBundle`, `FleetSelection`, planned
persona hash/role and planned skill references from the profile owner. Existing
skill definitions carry references/revisions, not content bytes; no skill
content hash is invented. The document retains the resolved profile timestamp.
Its separate immutable digest covers the entire document.

Unverified flags always name selected-folder capture, supplied pins/activation,
native identity, topology/roster/role join, persona-catalog binding, host-route
qualification, skill content, effective loading and caller authority. Supplying
pins cannot turn those flags off. `native_actuation_authorized` is always
`false`; effective loading stays `not_observed`. This is a partial projection,
not the full accepted-capture interface below. Its canonical scalar types and
component validation do not attest that a supplied native identity exists.

## P2 selected-folder preview

`preview_selected_composition(flag, environment, &SelectedCompositionInput)`
returns the existing immutable `CompositionSnapshot` or the configuration
owner's named `ConfigError`. It calls the accepted `ConfigFolder::select`;
explicit flag precedence and missing/invalid/legacy-source refusals stay with
that loader. No second parser, route, catalog, store or native adapter is added.

The request supplies native subject IDs, category, candidate naming values,
eligibility exclusions and evaluation time. The profile owner resolves and
verifies the profile and team revision. The requested slot must exist in that
team. Its logical role maps through the selected operational domain; the fleet
owner derives the team/slot key and selects its route. The role code used in the
title and persona comes from that mapping, overriding a caller's role label.
An absent optional role prompt stays `not_configured`; a present prompt becomes
a planned hash, never prompt text. Planned skill references are the slot's own
pinned requirements, not all skills from every role in the profile.

The `selected-composition-preview@1` document retains exact captured pack-source
hashes, the separately read fleet-byte hash and a canonical projection of the
project's declared ID, tracker key and repository resource roots. These roots
are repository declarations, not filesystem paths inferred from the config
folder. Neither the tracker key nor supplied native IDs establish a registered
internal `ProjectId`. An absent configured ID is explicit. Relocating identical
input bytes does not create a new identity. The enclosing snapshot hash binds
the full preview, including exact subject IDs and explicit parent absence.

The loader captures pack files separately and reads fleet afterward. The output
therefore explicitly marks `atomic_folder_generation: false`; source pins must
not be promoted into a coherent activated policy revision. Naming authority,
topology/parentage, registration, native identity, host qualification, skill
content and effective attachment remain unverified. This is a library preview,
not an exposed creation route. Its `write_refusals` disclose the unqualified
seat-bound issuer, unobserved project/workspace authority and absent tenant
binding; they are capability limits, not results from an authentication engine.
`native_actuation_authorized` remains false in every case.

Focused acceptance uses disposable neutral folders through the real loader,
including source-pin sensitivity, optional persona, explicit project-ID absence,
selection precedence, malformed/missing/legacy inputs, unknown profile/slot/fleet,
unavailable routes and unchanged input files. The existing ten P1/A1–A5
component probes remain separate from future grant enforcement. None is a
real-session, production-folder, durable-retry or installed-release test.

The next concrete write artifact is the existing identity/backend owner's
qualified issuer-to-requesting-seat operation binding, including delegation,
revocation, realm/host audience and an independently registered observed
workspace. Project/workspace find/reuse/create requires separate authority;
the seat-only grant cannot be widened. WP12 / ASMA-8574 owns registry/public-tree
compatibility; no new owner is invented here. DEP-006 is this R26 workstream;
DEP-007 (ASMA-8580), DEP-008 (ASMA-8651) and DEP-009 (ASMA-8633/lifecycle) remain
unresolved relationship keys, not invented R26 acceptance conditions. Existing
owners must supply their definitions if they become dependencies.

## One workflow and the enforcement boundary

Models coordinate in both existing orchestration modes. These deployment
combinations do not introduce a third orchestration enum value:

| Deployment | Claim |
| --- | --- |
| Paseo without kontord | Supported plugin UI, setup, skills and qualified local checks remain available. Other workflow obligations remain procedural. Shared evaluator components may be reused without claiming durable governance. |
| Paseo with selected kontord services | Only mediated operations receive their qualified deterministic checks and durable operation evidence. Backend unavailability refuses or waits that selected operation; no silent native fallback. Unrelated standalone capabilities remain available. |
| Kontor-governed | Models use supported operations within the existing admission, state, gate and recovery authority. The existing governance markers and pinned runs remain authoritative. |

Show capabilities quietly and explain a refusal at the affected action.
Attachment of a required persona or skill is enforceable when the runtime
contract supports it. Attachment does not prove model obedience or correctness.
Post-event detection is not pre-effect prevention.

## Dependent accepted-capture read interface (P2 onward)

Conceptual signature, **not an exported Rust API or registered route**:

```text
preview_composition(
    capture: AcceptedConfigurationCapture,
    subject: ObservedPlacementSubject,
    selection: CompositionSelection,
    eligibility: Eligibility,
    evaluation_time: Timestamp
) -> Result<CompositionSnapshot, CompositionRefusal>
```

The capture is produced only by the accepted WP08 loader and activation
contracts. Adapters cannot substitute client-asserted hashes for captured
configuration. The service performs no folder discovery, activation, mutation,
credential issuance or provider call. Read authorization is checked by its
transport before handing the evaluator its inputs; it confers no write right.

| Input | Exact content and existing owner |
| --- | --- |
| `capture.sources` | Map of selected-folder-relative source identifier to exact-byte `ContentHash`, captured once. Includes the entry, team manifest, operational domain, profiles, Core Team, host, referenced fleet and any referenced route qualifications. Missing referenced sources refuse. No host path, secret value or account home is copied into the result. WP08 owns traversal, containment, bounds and YAML/manifest parsing. |
| `capture.revisions` | Explicit accepted schema revisions and activation/pin evidence for each policy component. File presence and a valid hash alone are insufficient. An authoring preview is marked candidate; it cannot masquerade as activated policy. Existing pins are not re-resolved. |
| `capture.domain` | Existing `OperationalDomainPack`, exact selected `TeamDefinitionSpec`/topology revision and `RoleCatalogRevision`. No first-item guessing across multiple revisions. Reuse their validators and pure `kontor_core::naming` templates. |
| `capture.roster` | Existing resolved `CoreTeamRevision` with its exact catalog/hash binding. Mandatory slots come from that owner, never a client role label. |
| `capture.profiles` | Existing `ProfilePackSpec`. Resolve the explicitly selected `PackCategoryKey` through `kontor_profiles::resolve_profile`; retain its `ResolvedProfileBundle`, role/skill revisions and digest. |
| `capture.fleet` | Existing validated fleet policy and activation provenance. `FleetSnapshot` owns the binding resolution; `Eligibility` owns only explicit observed availability/vendor exclusions. Host route and qualification joins remain with WP08, not this proposal's probe. |
| `subject` | Native authority/host identity, exact native project/workspace/parent IDs (`ExternalId` at the boundary), kind and role-slot identity (`RoleSlotId`), observed revision/cursor and timestamp. `ProjectId` registration is required for mediated/governed effects; its explicit absence remains valid for a standalone candidate preview without write authority. Expected parent absence is explicit. Titles/cwd never substitute for IDs. These are independently observed facts, not authentication claims from labels. |
| `selection` | Explicit category, role/catalog revision, declared slot, fleet binding and naming subject. Naming token values come from the confirmed identity bindings and selected subject; unknown/ambiguous values refuse. No caller-supplied system prompt overrides policy. |
| `evaluation_time` | Explicit `Timestamp` passed to existing snapshot constructors. Same captured inputs and timestamp produce the same result. No ambient clock, cwd or environment lookup in the evaluator. |

Proposed output fields:

| Field | Meaning |
| --- | --- |
| `contract_revision` | The reviewed interface revision; no production revision is allocated by this proposal. |
| `source_pins`, `policy_state`, `subject` | Exact source hashes/revisions, candidate versus activated state, and observed subject context from the input. |
| `naming` | Selected typed template, separator and exact rendered names. Rendering uses `NativeNameTemplate::render`, not copied formatting rules. |
| `profile` | Verified `ResolvedProfileBundle`; mandatory profile/slot skill references retain exact revisions. Do not invent a role-level mandatory-skill field. |
| `route` | Existing `FleetSelection` with provenance, eligibility and exclusions, plus the accepted host-route join when available. No eligible route is an explicit blocked selection, not an implicit default. |
| `attachments` | Intended role persona hash/revision and required skill references/content hashes from their owning catalogs. Evidence is separately `planned`, `attached`, or `observed` for each attachment. Preview emits `planned`; it never invents effective readback. |
| `unverified` | Explicit unproved dependencies such as host-route qualification or effective loading. A partial result cannot assert whole-composition validity. |
| `native_actuation_authorized` | Always `false` for a preview. |
| `composition_hash` | `CanonicalDocument` hash over every preceding field. Excludes only the hash itself and transport metadata such as request ID/latency. Existing component timestamps and digests are preserved. A hash is neither a signature nor a capability. |

Refusals distinguish missing/unsupported capture, stale or mismatched pin,
unknown role/slot/category/binding, inconsistent topology/parentage, missing
required attachment and unsupported runtime capability. Reuse the existing
domain failures and map them at the eventual API boundary. A route with no
eligible account remains a typed blocked selection. No refusal echoes secrets
or configured host values.

## What the current probes establish

[`composition_snapshot_contract.rs`](../../crates/kontor-daemon/tests/composition_snapshot_contract.rs)
is an isolated test target using existing synthetic profile and neutral fleet
fixtures as compile-time strings. Its preview probes call the production P1
module, which delegates to the actual profile validator, bundle verifier,
fleet resolver and pure naming renderer. Only fixture setup parses the fleet.
There is no duplicate preview implementation in tests and no
folder argument, filesystem reader, service registration, native client or
store in the probe.

The executable `composition-preview@1` document deliberately covers only
profile/fleet/naming projection and hashing, not the full proposed interface.
Its independent fixture catalogs are not asserted to form a valid topology or
roster join. It proves reproducibility, sensitivity to exact native IDs/slot,
profile and persona changes, missing skill/name/binding rejection, stale fleet
content rejection and preservation of no-route results. The persona field
binds intended prompt bytes; effective loading stays `not_observed`.
The core hashes profile semantics canonically and fleet bytes through the
existing fleet owner. It does not manufacture a selected-folder source receipt.
Additional checks bind optional source/policy pins exactly while preserving
their unverified status, and bind evaluation time without changing prior
immutable snapshots.

Two additional payload-shape probes check that the proposed client request
cannot carry issuer/scope/launch overrides, and that the synthetic server-held
grant binding includes operation identity and every required context field.
They reuse canonical types and hashing. They neither issue nor verify grants;
no issuer, credential, transaction, dispatcher or simulated policy engine is
implemented. The original six preview probes now exercise P1, and their three
mutations target the production module; the fourth mutation still targets
strict client payload shape. These remain component/preview evidence only.
Payload-shape checks do not qualify security
or effects, and none of the dependent cases below is marked passed by them.

No test here establishes authentication, caller scope, actual ADAM placement,
leadership versus task-role eligibility, selected-folder acceptance, host-route
joining, concurrency, idempotent native dispatch, activation or deployment.
Those require the dependent contracts below. In particular, a synthetic ID in
a digest is not proof that such a native identity exists or is authorized.

## Caller authority and the smallest write slice

The accepted future target is a separately authenticated, revocable grant for
**one declared seat creation in an already registered, independently observed
workspace**, without acquiring scheduler governance or adopting a governed
SeatBinding. Workspace/project creation, rename, label mutation, movement,
adoption, restart, cascade and bulk operations are excluded. The disposition
settles the design choice below; qualifying the missing dependencies remains
necessary before production implementation or issuance.

| Option | Assessment |
| --- | --- |
| Relabel the existing hosted-seat endpoint as neutral | Reject. It relies on Kontor SeatBindings, occupancy generations, topology ownership and seat-scoped credentials. Direct-seat adoption/restart is explicitly refused. Relabeling would transfer authority while retaining those effects. |
| Give the plugin a realm operator credential and trust submitted caller labels | Reject. A tier does not prove native caller identity or scoped delegation; the plugin currently holds observer authority. This would broaden its write authority and allow self-asserted role/parentage. |
| Add a scoped domain-operation grant, served by kontord | Recommended and accepted as the future target with A1–A5. Reuse shared validation and existing durable intent/correlation mechanisms through a qualified neutral boundary; keep scheduler and existing hosted-seat ownership separate. |

### A1: issuance, transport and verification have separate responsibilities

These are proposed responsibilities, not names of existing qualified endpoints:

| Responsibility | Trusted principal and policy |
| --- | --- |
| Issuer | A kontord domain-grant issuer acting as a registered backend service principal. It reads the existing configured delegation policy, authenticates the requesting adapter and independently binds the native caller. A model may request an operation but cannot issue, edit or widen a grant. |
| Transport | The registered plugin/CLI adapter service principal, authenticated by the backend transport. It is distinct from the model's claimed role and from the native caller whose delegation is being exercised. Its authority must be bound to that caller, not shared indiscriminately among sessions. Human entry uses its qualified principal binding; conversational access to a seat is insufficient. |
| Issuance policy | The accepted delegation configuration bounds caller, resources, operation and policy revisions. Issuance is automatic inside that existing delegation; no routine human approval or new per-seat enablement toggle. Outside it, refuse rather than silently widen it. Issuer and delegation revisions are server-held. |
| Verifier | The kontord mediated-operation service checks the authenticated principal, intended verifier and server-held grant/delegation/revocation state at both admission and dispatch. A client's matching JSON fields or hash are not authentication. |

The issuer may be a qualified authenticated adapter/backend arrangement; the
backend issuer above is the recommended placement, not an assumption that it
exists today. No broad credential enters prompts, transcripts or grant
receipts. Missing qualified issuer, caller binding or verifier refuses effects.
An opaque backend reference over authenticated transport is sufficient where
qualified; no new OAuth, JWT or PKI system is mandated.

**Missing neutral boundary:** a supported way to authenticate that caller,
bind it to fresh native identity/parentage and a revocable operation scope,
qualify the issuer and its delegation, and persist/reconcile a domain-operation
intent without creating a governed SeatBinding. The future design acceptance
does not supply this boundary. Never write Paseo's internal store or expose
private `applications.rs` helpers as neutral services.

### A2: server-held bindings and exact request/response shape

| Binding | Required content |
| --- | --- |
| Identity and audience | Unique grant ID and creation operation ID; issuer principal and delegation revision; authenticated adapter principal; intended verifier, realm and native host. A grant belongs to one operation, not one HTTP attempt. |
| Exact scope | Operation kind `create_one_seat`; registered project ID plus exact native project/workspace, role, slot, caller and parent IDs. Parent absence is explicit. Bind the independently observed subject revision as well as identity. |
| Intent and configuration | Canonical intent digest and composition digest; selected source/policy pins and activation revisions. The intent contains operation kind, exact scope, native observation revision, pins and composition digest. It excludes attempt metadata, grant reference and its own digest, avoiding a circular hash. |
| Lifetime | Expiry and revocation generation checked against the server's clock and current server-held revocation/delegation state. A copied client expiry/generation is never sufficient. |

The client create body contains exactly `grant_reference`, `idempotency_key`
(`IdempotencyKey`), `expected_composition_hash` (`ContentHash`),
`expected_subject_revision` and `slot` (`RoleSlotId`). Authentication belongs
to transport, not to fields in that body. The verifier resolves the reference
to server-held state, reconstructs intent from the selected captured snapshot
and independent native observation, and checks every binding above. It derives
all launch configuration, naming, route, persona and skill attachments from
that snapshot; no client override of these fields is accepted.

The response contains operation ID, intent/composition hashes, pins, state,
native correlation and attachment evidence separately marked `planned`,
`attached`, or `observed`. It contains no credential or authority-bearing
grant material. A successful response never claims model obedience.

### A3–A4: one creation operation, durable claims and the dispatch race

| Point | Required transition or refusal |
| --- | --- |
| Admission | Freshly authenticate; check issuer/delegation, scope, audience/realm/host, expiry/revocation, pins and independently observed native identity. In one transaction, claim the grant and exact slot and insert the durable idempotent operation with frozen intent and launch inputs before dispatch is possible. Grant, operation, principal-scoped idempotency key and slot uniqueness must agree. No partial claim survives failure. |
| Retry/concurrent request | Same authenticated principal, grant, key and canonical intent resolves to the existing operation and its reconciliation path, without another claim or dispatch. Changed intent, another key/operation reusing the grant, another principal, or conflicting slot consumption refuses. Concurrent identical retries may read the one operation; concurrent re-consumption may not create a second. A retry is not a new authority source. |
| Immediately before dispatch | Repeat fresh authentication/authorization, expiry/revocation, policy pins and native identity checks. A revocation, expiry, identity drift or pin change since admission cancels the still-undispatched operation with zero dispatch. Backend unavailability cannot fall back to direct native creation. |
| Revocation versus dispatch commitment | The future store/dispatcher must serialize revocation and the durable dispatch commitment on the grant/operation. Revocation winning that order prevents dispatch. Dispatch winning records an irrevocable attempt boundary before the external call; later revocation cannot promise to cancel or reverse that possible effect. Recheck immediately before sending; a revocation/expiry observed before send stops it. Any unobservable gap between that check and remote effect remains an explicit race, unless a qualified native conditional/atomic guard closes it. This is not a claim of atomicity across the database and native runtime. |
| Unknown/lost response or crash after dispatch commitment | Reconcile by stable operation/native correlation, including after grant expiry or revocation. Do not redispatch, free the slot or issue a replacement grant just because a response or native observation is absent. Even a crash before the actual send may remain uncertain. Independently authorized observation may continue without reviving create authority; definitive evidence and an explicitly qualified recovery procedure are required to resolve it. |
| Confirmed effect | Preserve intent and observation separately. Revocation prevents future dispatch; it never reverses an already dispatched native effect. Record observed native identity and available attachment readback without promoting `planned` to `observed`. |

The exact neutral operation record and transactional mechanisms are still
dependent design/implementation work, not new schema in this proposal. Fresh
checks cannot close direct SDK/CLI bypasses or an external native mutation
between comparison and dispatch. Without qualified native guards, claim only
the mediated operation's checks and expose the residual race. Standalone
plugin capabilities remain available without kontord; a selected mediated
operation refuses/waits on backend outage rather than downgrading silently.

## Dependency lift evidence

| Dependency | Required lift evidence |
| --- | --- |
| WP08 selected-folder parser, PR #435 | Merged at `a9cf0f175a0461133c3a47089f24b222ad088bf0`; the P2 implementation calls the accepted loader without copying its parser. Captured pack bytes are pinned; the separately read fleet is not an atomic selected-folder generation. |
| WP10 / ASMA-8572 actual folder | #3652 merged at `e1e29ebd0324d801a222277ca03c2fc8be85b8f7`. Its actual-folder receipt binds earlier source; use the unchanged config bytes as input evidence only. Exact-head consumer/startup/rollback and installed readback remain separate owner obligations. No tenant or native project/workspace binding is supplied by loading a folder. |
| Durable domain-operation record | Coordinate with the schema owner only after the accepted folder and neutral write boundary qualify. Current store schema is 145: 143 is local-result validation, 144 consultation intent/readback and 145 neutral epic activation. Do not allocate 143/144 or assume 146 is reserved. This read-only slice adds no migration or record. |
| Neutral caller/operation authority | Future scoped-grant target is accepted with A1–A5; do not request the same decision again. Still required: qualified issuer/delegation, authenticated transport/caller binding, server-held grant verification, transactional claim/revocation/dispatch ordering, correlation and capability readback. Existing hosted-seat services cannot supply this by renaming. |
| Native pre-effect interception | To claim all-route prevention, qualified hooks/authorization must cover create with exact workspace/labels/owner context and every relevant rename/label/project/workspace/import/recovery mutation. Current three before-hooks do not provide that coverage. Until then, claim only the specifically mediated operation and disclose bypasses. |

Q-02/DEC-020, the PR-desk STOP and protected paths remain in force; no scope is
silently narrowed. The owning delivery remains open through implementation,
independent QA/AUD and repository gates, designated merge, release,
installation and readback. This contract does not substitute for those rungs.

## A5: dependent qualification matrix

Every row is **pending dependent implementation/qualification**. The synthetic
payload tests only check representation, not these enforcement outcomes.
Start with offline native/transport fakes and isolated future store fixtures;
demonstrate that removing each relevant guard breaks its test. No real
sessions are test subjects.

| Cases | Required outcome and evidence |
| --- | --- |
| Automatic delegated issuance | Qualified issuer and authenticated adapter, allowed existing delegation and fresh native caller proof produce exactly one scoped operation grant without a human prompt or per-seat toggle. Inspect traces to prove no broad credential reaches model content. |
| Missing/wrong issuer or delegation | Missing/unqualified issuer, spoofed issuer, out-of-delegation scope, stale delegation revision or model self-issuance refuses; zero native dispatch. Changing request labels does not alter authority. |
| Principal, verifier, realm and host | Missing authentication, another adapter/session principal, wrong verifier audience, realm or native host refuses even if all submitted hashes match; verify against server-held records, not request equality. |
| Scope and identity | Wrong registered project/native project/workspace/role/slot/caller/parent (including absent versus present parent), stale observation, and equal-title/equal-cwd distinct IDs refuse. Include leadership in a task workspace, desk-specific ADAM and task SA eligibility from the accepted owner configuration. |
| Intent, digest and pins | Modified intent/composition, stale/unactivated source or policy pin, profile/persona/skill tampering, or client launch override refuses. Server derives all launch fields from the accepted snapshot. Snapshot edits cannot silently mutate an issued operation. |
| Expiry/revocation at admission | Expired grant, changed server revocation generation, revoked grant/delegation, or client-supplied fresh-looking generation refuses with zero claims/effects. Exercise server clock boundaries and refuse stale cached state. |
| Double-use and concurrency | Same-intent retries reconcile one operation; another key/intent/principal/operation reusing a grant refuses. Concurrent slot claims and grant consumption yield one durable winner and at most one dispatch commitment. Prove transaction rollback and unique constraints, not just sequential mock behavior. |
| Revoke/expire/change pins or native identity after admission | Inject changes before the final dispatch check and on both sides of the serialized revocation/commitment boundary. Winning revocation or expiry before send yields zero dispatch; the later/unknown case never promises rollback. Assert the documented compare/send race with a fake external mutation. |
| Lost response and restart reconciliation | Drop create response; crash before/after the durable dispatch commitment; delay/lose native observation. Reconcile the same operation using correlation, including after expiry/revocation. No blind redispatch, new grant or premature slot release; absent readback remains unknown. |
| Selected backend unavailable | Outage before issuance, admission or dispatch produces zero new dispatch; outage after dispatch leaves reconciliation pending. No direct fallback. Unrelated standalone UI/skills/config/qualified local checks remain usable without kontord installed. |
| Native hooks, attachment evidence and boundaries | Qualify supported conditional context if available; otherwise record bypass/compare-send limits explicitly. Missing required attachment refuses when the runtime supports enforcement. Keep planned/attached/observed separate and qualify each readback claim. Preserve direct-seat pins; exclude project/workspace create, rename/label/move/adoption/restart/cascade/bulk. No obedience guarantee. |

The original implementation-through-release mandate remains with the existing
QA/AUD and gate owners, designated merge and installer owners. Passing any of
these component or future isolated tests alone is not real-folder, release or
physical acceptance.
