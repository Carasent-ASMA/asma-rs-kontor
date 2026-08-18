# KON-OP-03 — composing CP2's first clause

Answers rejection receipt `01a00deb-0b84-79c3-9d5b-e68ba2581014` and the notes
in `REVIEW-ROUND-3.md`. Round 3 credited CP3, CP4 and semantic topology as
complete, and rejected on one family: topology specification publication,
catalog lookup and code help — the first clause of CP2 and the first table of the
handoff's uniform `/v1` contract.

All nine are composed. Refusals in `crates/kontor-daemon/src/applications.rs`
fall from **38 to 29**, and every one of the 29 is a licensed successor-ticket
contract (Completion 6, Committee 6, Advisor 5, Quick work 4, Core Team 4,
epic roster 2) plus the two agreed Advisor/Committee topology scopes.

## The coherence gap is closed

`ensure_scope_chain` calls `pinned_spec`, so the composed semantic topology was
consuming a specification with no `/v1` path to publish, read or move. There is
one now: draft → validate → publish → read, and preview → apply for the epic
pin. The Admin tier's defining Operational power — deciding which node kinds may
ever exist in a project — works, and a client can read the role catalog and the
code help rather than keeping a private glossary.

## What each operation does

| Operation | Behaviour |
| --- | --- |
| `draft_topology_spec` | Assembles a candidate from the parts a caller may state. The identity, the version and the schema generation are the server's — `base` names a lineage and the *next* version is derived, so a draft cannot be aimed at a revision something is already pinned to. |
| `validate_topology_spec` | Runs the domain's own `ProjectSessionTopologySpec::validate` and returns its verdict. |
| `publish_topology_spec` | Revalidates, proves the candidate is the document the verdict was about, refuses a revision already published with different bytes, then writes through the OP-01 store under the project's expected revision. |
| `topology_spec` | The exact published document with its canonical hash and classification. |
| `role_catalog` / `role` | The published `RoleCatalogRevision`, in its own declared order. An unknown code is refused, never guessed. |
| `code_help` | Every controlled code the epic's pinned revisions define — declared kinds, historical codes and role codes — in one projection sorted by `(category, code)`, each citing the revision it came from. |
| `preview_topology_upgrade` | Derives what moving the pin would do: kinds withdrawn or introduced, nodes left standing on a withdrawn kind, and the seats and containers stranded under them. Writes nothing. |
| `apply_topology_upgrade` | Repins the epic to the revision that still produces exactly the authorized preview. |

## Three design decisions worth naming

**The apply names a preview, not a target.** `TopologyUpgradeApplyRequest`
carries only a digest, so the server searches its own published revisions for
the one that still produces exactly those effects. A target in the request would
let an apply commit effects the caller never saw; a stored preview would let it
commit effects the Realm no longer has.

**Candidate identity is hashed from the parsed document.** A specification has
optional fields — an empty `historical_codes` is omitted rather than written as
`[]` — so one revision has more than one JSON spelling. Hashing the spelling
gave a draft, a verdict and the stored revision three different identities for
one document. The store already hashes the parsed form; this is now that same
rule stated once instead of a second one that agreed by luck.

**A published revision is judged for collision before it is judged for rules.** A
caller who edited a published revision has made one mistake, and "your vocabulary
is invalid" would send them to fix the wrong thing.

## Schema v29

Two changes, both forced by operations that did not exist at v23.

The epic pin becomes writable. v23 made it permanently unwritable, which was
right while nothing could move it. The Operational contract now has an explicit
upgrade, so the pin has to move — once, deliberately, through that operation.
Nothing about immutability is given up: the *specification revision* the pin
points at is still immutable and still permanent, and every move is recorded in
`command_receipts` with its canonical intent, which is where every other decision
in this control plane is audited. The `DELETE` guard is unchanged.

The closed command-kind list grows by two: `publish_topology_spec` (targets the
project) and `upgrade_topology` (targets the epic).

## Two defects the tests found

Both were found by write-path tests, and both were mine.

**A replay of a successful upgrade was refused for succeeding.** The intent
originally named the target, which meant deriving it — and the search had to run
before the key was judged. Once the first call moved the pin, no published
revision produced that preview any more, so an ordinary retry got
`revision_conflict`. Fixed by recording what the caller actually authorized: the
preview digest. The key is now judged first and a replay answers from what is
durable without deriving anything.

**Draft, validate and publish disagreed about a candidate's identity** — the
hashing defect above. The read-back assertion in
`a_vocabulary_is_drafted_validated_published_and_read_back` is what caught it.

## Negative proofs

All fourteen now hold. The three the review listed as partial or unexercisable:

| # | Proof | Now |
| --- | --- | --- |
| 3 | Stale revision / replay creating a second effect | `publishing_under_a_stale_revision_writes_nothing` refuses and reads back 404; `a_published_specification_cannot_change_in_place` proves a replayed publish answers the original receipt; the upgrade proves both halves on the epic pin |
| 4 | Unknown role code | `the_catalog_resolves_a_known_code_and_refuses_an_unknown_one` — a well-formed but undeclared code is `not_found` with no title in the body, and a malformed one is refused separately |
| 6 | A published or epic-pinned specification changing in place | `a_published_specification_cannot_change_in_place` — different bytes under the same identity and version are `409`, and the stored document is re-read to prove it did not move |

## Mutation checks

| Mutant | Result |
| --- | --- |
| the already-published hash comparison is dropped, so a revision may change in place | **caught** — the edited publish answers 200 instead of 409 |
| an unknown role code is filled in with a guessed title instead of refused | **caught** — the catalog answers 200 with `"standard_title":"ZZZ"` |

## Gates

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 106 suites, 1361 passed, 0 failed |
| `crates/kontor-daemon/tests/loopback_api.rs` | green — 141 passed (was 134) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| console `verify:api` / `typecheck` / `test` | fresh, clean, 278 passed |

The contract surface is unchanged: this turn added behaviour behind the 51
operations, not operations.
