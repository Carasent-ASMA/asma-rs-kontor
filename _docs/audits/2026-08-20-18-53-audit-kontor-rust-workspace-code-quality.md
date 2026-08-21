# Kontor Rust workspace code-quality audit

> **Date:** 2026-08-20 18:53
> **Status:** 🟡 In Review
> **Category:** audit
> **Scope:** `_tools/asma-rs-kontor` Rust workspace at `origin/master` commit `559c70f1f478d416e1dc28c4d4b9d8e2be72a45c`
> **Summary:** In-depth static and behavioral audit of Kontor's 240k-line Rust workspace: size legitimacy, DRY/YAGNI/SOLID, file and function size, MCP-tool count, dependency health, panic/unsafe posture, automated quality gates, and a prioritized remediation program.

---

## When to Load

**Load this document when:**

- Deciding whether Kontor's Rust size or 127-tool MCP surface is justified.
- Planning maintainability work in `kontor-daemon`, `kontor-store`, `kontor-api`, `kontor-runtime`, or `kontor-mcp`.
- Adding a new capability, API operation, MCP tool, runtime-adapter method, or large orchestration workflow.
- Setting Rust file/function-size, duplication, coverage, mutation, or dependency-quality gates.

**Do NOT load for:** A production incident requiring current runtime evidence, frontend code quality, or a security penetration test. This is a source-quality audit of one repository snapshot.

---

## 1. Executive verdict

Kontor's size is **partly legitimate and substantially better tested than the raw 220k-line number suggests**, but its current shape is **not maintainability-ready for continued growth at the same rate**.

The workspace contains 240,679 physical Rust lines. After removing 504 vendored lines, 240,175 are first-party. About 110,336 first-party lines (45.9%) are tests, inline test modules, or integration-test support. The estimated non-test first-party source is therefore about 129,839 lines. A further 4,363 lines are public fake/fixture modules compiled into product crates. The headline size is real, but it does not mean there are 220k lines of independent production behavior.

The implementation has strong foundations:

- 22 workspace members and capability-focused crate boundaries;
- no first-party `unsafe`, with `unsafe_code = "deny"`;
- formatting, standard Clippy, 1,604 non-ignored tests, `cargo audit`, and `cargo deny` all pass at the audited commit;
- exact direct dependency pins and SHA-pinned GitHub Actions;
- low broad textual duplication: 2.04% in normal comparison and 2.44% when comments are ignored;
- one central, parity-tested MCP registry rather than 127 separately maintained dispatch implementations;
- typed identifiers, immutable specifications, transactional receipts/outbox behavior, idempotency, capability discovery, and refusal instead of optimistic runtime claims.

The principal maintainability risks are concentrated rather than universal:

1. `kontor-daemon/src/applications.rs` is 18,405 lines and implements a 108-method application port.
2. `kontor-store/src/repository.rs` is 10,993 lines, while API applications and several adapters/tests are thousands of lines each.
3. Production source contains 123 recognized functions over 100 lines; the API router is 531 lines and several orchestration functions are 200–409 lines.
4. `ApplicationOperations` (108 methods) and `RuntimeAdapter` (31 methods) violate interface-segregation pressure and amplify unrelated changes.
5. Product-scope/YAGNI legitimacy cannot be established from reachability and tests alone. The repository grew to 276 commits in eleven days, status documentation already lags implementation, and no usage/adoption evidence was available.
6. CI prevents ordinary correctness regressions but does not ratchet complexity, file/function growth, duplication, coverage, mutation strength, or unused dependencies.

**Overall assessment:** credible architecture and unusually strong behavioral verification for a young control plane, with localized but severe structural debt. Do not pursue a blanket LOC-reduction target. Put a temporary growth fence around the largest modules, split the central ports by capability, and add maintainability ratchets before extending the product surface.

---

## 2. Scope and method

### 2.1 Audited source of truth

The polyrepo checkout pointed at an older detached Kontor commit and contained unrelated, user-owned untracked evidence. This audit therefore used a clean temporary worktree at the local authoritative `origin/master` commit:

```text
559c70f1f478d416e1dc28c4d4b9d8e2be72a45c
```

No production code was changed. Existing evidence and unrelated working-tree changes were excluded and preserved.

### 2.2 Checks performed

- Physical and nonblank Rust LOC, separated into first-party product source, external tests, inline tests, and vendored code.
- Per-crate and per-file size distributions.
- Lexical function-body size scan excluding `#[cfg(test)]` production modules.
- Standard and opt-in Clippy analysis, including `too_many_lines`, `cognitive_complexity`, `too_many_arguments`, `type_complexity`, `large_enum_variant`, `large_stack_arrays`, `needless_pass_by_value`, `missing_const_for_fn`, and `redundant_clone`.
- First-party scan for `unsafe`, `unwrap`, `expect`, explicit panic, and TODO/FIXME/HACK markers.
- Textual clone analysis in normal and comment-ignoring modes, followed by pair and hotspot classification.
- Cargo dependency graph, duplicated versions, advisory, license, ban, and source-policy review.
- API/MCP registry cardinality, uniqueness, method/path parity, tier/profile behavior, and existing consolidation plan review.
- Crate dependency and central trait review against SRP, OCP, LSP, ISP, DIP, DRY, and YAGNI.
- Workspace tests excluding the evidence-writing end-to-end pilot.

The repository knowledge-graph tools required by the workspace instructions were unavailable in this session, so the audit used targeted source, Cargo metadata, compiler, test, and static-analysis evidence instead of broad speculative exploration.

---

## 3. Size: what the 240k lines represent

### 3.1 Workspace composition

| Measure | Result | Interpretation |
| --- | ---: | --- |
| Cargo workspace members | 22 | 19 principal domain/runtime crates plus contract, end-to-end, and desktop members |
| Tracked Rust files | 225 | The clone scanner analyzed 223 inputs; two tracked files were excluded by its path/scanner rules |
| Total physical Rust LOC | 240,679 | Raw repository number |
| Vendored `arrayref` LOC | 504 | Third-party source maintained locally because of dependency policy |
| First-party physical Rust LOC | 240,175 | Relevant ownership surface |
| Product/source paths | 136,425 | Includes inline tests and public fake/fixture modules |
| First-party external tests | 103,750 | Excludes vendored source |
| Inline `#[cfg(test)]` modules | approximately 6,586 | Tests colocated in source files |
| First-party test code | approximately 110,336 | 45.9% of first-party Rust |
| Estimated non-test source | approximately 129,839 | 54.1% of first-party Rust |
| Public fake/fixture modules in product crates | 4,363 | Compiled/exported test support, not core runtime behavior |

This is a large MVP, but the raw number overstates production behavior by almost a factor of two. Test volume is a strength, provided those tests remain readable and mutation-sensitive.

### 3.2 LOC by component

| Component | Physical Rust LOC |
| --- | ---: |
| Store | 54,118 |
| Daemon | 43,223 |
| Core | 23,112 |
| Paseo runtime adapter | 17,302 |
| API | 13,972 |
| End-to-end tests | 11,451 |
| Runtime abstraction | 9,966 |
| Contract tests | 8,330 |
| MCP | 7,607 |
| AO runtime adapter | 7,357 |
| Teams | 6,869 |
| Codex runtime adapter | 6,397 |
| Scheduler | 5,866 |
| Accounts | 4,207 |
| Policy | 4,050 |
| Integrations | 3,868 |
| Profiles | 3,513 |
| Calendar | 3,337 |
| Context | 3,062 |
| Intake | 1,528 |
| CLI | 972 |
| Vendored `arrayref` | 504 |
| Desktop Rust shell | 68 |

Store, daemon, and core account for about half of all Rust. That concentration is understandable for a transactional control plane, but it identifies where architectural change has the highest review and regression cost.

### 3.3 File sizes

For product/source files, the median is 463 lines, p90 is 1,736, p95 is 3,132, and the maximum is 18,405. Sixty-seven product files exceed 500 lines, 33 exceed 1,000, and 10 exceed 2,000.

There is no universal professional maximum for a Rust file. For this audit, 500 lines means “review the cohesion,” 1,000 means “strong split candidate,” and 2,000 means “critical ownership/review hotspot.” Those are maintainability heuristics, not language rules.

Largest files:

| File | Lines | Assessment |
| --- | ---: | --- |
| `crates/kontor-daemon/tests/loopback_api.rs` | 19,495 | Test mega-suite; difficult failure localization and fixture reuse |
| [`crates/kontor-daemon/src/applications.rs`](../../crates/kontor-daemon/src/applications.rs) | 18,405 | Critical production hotspot and central application implementation |
| [`crates/kontor-store/src/repository.rs`](../../crates/kontor-store/src/repository.rs) | 10,993 | Critical persistence hotspot spanning many aggregates/workflows |
| [`crates/kontor-api/src/applications.rs`](../../crates/kontor-api/src/applications.rs) | 7,832 | Port and transport logic concentrated in one module |
| `crates/kontor-store/tests/repository_roundtrip.rs` | 7,203 | Test mega-suite |
| [`crates/kontor-runtime-paseo/src/adapter.rs`](../../crates/kontor-runtime-paseo/src/adapter.rs) | 5,922 | Adapter orchestration hotspot |
| `crates/kontor-runtime-paseo/tests/adapter_contract.rs` | 5,497 | Large contract suite |
| [`crates/kontor-mcp/src/registry.rs`](../../crates/kontor-mcp/src/registry.rs) | 4,434 | Mostly declarative registry; generated/segmented representation is worth evaluating |
| [`crates/kontor-core/src/spec.rs`](../../crates/kontor-core/src/spec.rs) | 3,797 | Domain specification hotspot |
| [`crates/kontor-core/src/repository.rs`](../../crates/kontor-core/src/repository.rs) | 3,652 | Domain port hotspot |

### 3.4 Function sizes and complexity

The lexical scanner recognized 2,686 non-test function bodies in first-party product paths:

| Statistic | Lines |
| --- | ---: |
| Median | 14 |
| p90 | 63 |
| p95 | 94 |
| p99 | 161 |
| Maximum | 531 |

| Threshold | Function count |
| --- | ---: |
| Over 50 lines | 376 |
| Over 75 lines | 192 |
| Over 100 lines | 123 |
| Over 150 lines | 38 |
| Over 200 lines | 14 |

[Clippy's `too_many_lines` lint](https://rust-lang.github.io/rust-clippy/master/index.html#too_many_lines) has a default threshold of 100. It is not enabled by the normal lint groups, so the standard CI pass does not reveal this pressure.

Largest recognized production functions include:

| Function | Lines |
| --- | ---: |
| API `router` | 531 |
| Daemon `seat_with_address` | 409 |
| Daemon `replace_seat` | 319 |
| Daemon `settle_runtime` | 263 |
| Daemon `settle_committee_run` | 248 |
| Daemon `settle_advisor_run` | 242 |
| Store graph `waive_role_slot` | 215 |
| Daemon `remediate_completion` | 214 |
| Daemon `read_epic` | 211 |
| Daemon `record_committee_findings` | 211 |
| Daemon `ensure_quick_session` | 206 |
| Daemon `materialize_core_team` | 205 |
| Paseo `launch_admitted` | 204 |
| Daemon `retire_predecessor` | 202 |

The opt-in lint set produced 373 unique diagnostics: 213 `too_many_lines`, 77 `missing_const_for_fn`, 62 `redundant_clone`, 15 `needless_pass_by_value`, and six `cognitive_complexity`. Of these, production paths accounted for 77 too-long functions, 65 missing-const candidates, 12 redundant clones, nine needless pass-by-value cases, and four cognitive-complexity findings. No `too_many_arguments`, `type_complexity`, `large_enum_variant`, or `large_stack_arrays` diagnostics appeared.

The four production cognitive-complexity findings were:

- `kontor-api/src/error.rs` near line 557: complexity 30;
- `kontor-daemon/src/lib.rs` near line 472: complexity 42;
- `kontor-daemon/src/lib.rs` near line 530: complexity 69 and 116 lines;
- `kontor-daemon/src/main.rs` near line 192: complexity 93.

Clippy itself cautions that this lint does not measure true human cognitive complexity and recommends considering `too_many_lines` or `excessive_nesting` instead. These four results are review pointers, not objective complexity scores.

**Finding CQ-01 — High:** File and function size are outside a sustainable review envelope in a small number of central modules. The median function is healthy; the tail is not. This calls for capability-oriented decomposition, not arbitrary line chopping.

---

## 4. DRY and reuse

### 4.1 Quantitative result

The normal clone scan found 308 clone pairs and 4,904 duplicated lines, or 2.04% of the scanned Rust. Ignoring comments increased this to 338 pairs and 5,744 duplicated lines, or 2.44%. The vendored file's effect is negligible.

Pair classification in the normal scan was:

| Pair class | Pairs | Sum of pair lengths\* |
| --- | ---: | ---: |
| Test↔test | 173 | 2,935 |
| Production↔production | 126 | 2,156 |
| Production↔test | 8 | 107 |
| Vendor-local | 1 | 14 |

\*Pair lengths overlap and must not be added to the overall unique duplicated-line percentage.

Per-file clone exposure was approximately 3.05% for production and 5.42% for tests. This is **not evidence of systemic copy-paste development**. It is low for a large contract-heavy workspace.

### 4.2 Localized hotspots

| Product file | Approximate clone exposure |
| --- | ---: |
| MCP registry | 11.55% |
| Store teams | 10.09% |
| Store policy | 9.17% |
| Core typed IDs | 8.87% |
| API memory | 8.33% |
| Store repository | 8.29% |
| Store intent | 6.93% |
| Daemon applications | 6.34% |
| Store intake | 6.06% |
| Daemon runtimes | 6.01% |
| AO adapter | 5.27% |
| API applications | 4.93% |
| Paseo adapter | 3.82% |

The most useful extraction candidates are repeated typed-ID implementations, store transaction/revision/receipt sequences, adapter preflight/attestation sequences, registry rows, and integration-test setup. Several test pairs intentionally repeat complete scenarios to preserve readability and independent contract proof; those should not be deduplicated blindly.

The comparison tool's “weak” mode ignores comments in addition to whitespace; it does **not** normalize renamed identifiers or prove semantic equivalence. The result therefore cannot rule out two differently named implementations of the same algorithm. See the [official jscpd mode documentation](https://github.com/kucherenko/jscpd/blob/master/docs/typescript.md).

**Finding CQ-02 — Medium:** DRY debt is localized and tractable, not a primary explanation for repository size. Extract abstractions only where the same domain rule is repeated, and only after the repeated shape is stable. Avoid replacing clear domain code with a generic framework solely to reduce LOC.

---

## 5. SOLID and architectural principles

### 5.1 Single responsibility

Crate-level responsibility is generally strong. `kontor-core` has no internal Kontor dependency and is reused across the workspace. Runtime integrations are separated into AO, Codex, and Paseo crates. Store, scheduler, policy, teams, accounts, profiles, context, calendar, intake, API, MCP, CLI, daemon, and desktop have recognizable ownership.

Module-level responsibility is much weaker at the composition center. The 18,405-line daemon applications module combines implementations for many unrelated application capabilities. The 10,993-line store repository and 7,832-line API applications module have similar cross-capability pressure.

### 5.2 Open/closed and Liskov substitution

These are relative strengths:

- separate runtime-adapter crates extend the system without embedding provider branches throughout the domain;
- runtime capability discovery and explicit unsupported responses avoid pretending that every adapter supports every behavior;
- shared adapter contract tests exercise substitution behavior;
- typed receipts and evidence make behavioral differences observable.

### 5.3 Interface segregation

[`ApplicationOperations`](../../crates/kontor-api/src/applications.rs) begins near line 3,923 and exposes 108 methods. Its daemon implementation begins near line 7,730 of the 18k-line applications file. [`RuntimeAdapter`](../../crates/kontor-runtime/src/adapter.rs) begins near line 420 and exposes 31 methods.

Even where default unsupported behavior makes the adapter implementable, these interfaces force broad knowledge, large mocks, broad compilation impact, and high review load. This is the clearest SOLID violation in the codebase.

### 5.4 Dependency inversion

Dependency direction is mostly sound: domain/core sits low, the daemon is the composition root, and provider integrations implement a runtime abstraction. There is one deliberate exception: `kontor-api` depends directly on `kontor-store`, and `ApiState` stores a concrete `SqliteStore`. This avoids an extra abstraction but couples transport/application composition to the persistence implementation. It is acceptable while there is exactly one store and no test friction, but should be recognized as a boundary trade-off rather than described as full port-based inversion.

**Finding CQ-03 — High:** The central ports violate interface segregation and make the daemon/application layer a change amplifier. Split them by product capability while preserving one composition root and shared cross-cutting invariants.

---

## 6. YAGNI and product-scope legitimacy

Static code can show that a capability is reachable, tested, documented in a plan, and mapped to a public operation. It cannot show that users need it.

Evidence supporting legitimacy:

- no compiler dead-code warnings under `-D warnings`;
- no TODO/FIXME/HACK backlog embedded in the source;
- MCP operations are unique and parity-tested;
- domain capabilities correspond to approved Foundation/Operational plans and explicit invariants;
- the large test share demonstrates verification intent rather than raw feature-code accumulation.

Evidence against declaring YAGNI satisfied:

- the repository was initialized on 2026-08-09 and reached 276 commits by this 2026-08-20 snapshot;
- history shows approximately 268,632 Rust additions against only 15,248 deletions, an append-heavy stabilization profile;
- the README says full pilot, backup/security, calendar, and intake work is not all complete, while several of those modules and tests already exist;
- the parent documentation index still labels the Operational MVP as planned while much of its source is present;
- no production usage telemetry, operator adoption data, or per-capability keep/remove decision record was available.

Fast delivery is not itself a defect, and reachability is not product value. The correct conclusion is **YAGNI is not demonstrated**, not “the extra code is unnecessary.”

**Finding CQ-04 — High governance risk:** Before adding another broad capability wave, create an inventory that connects every public capability to its requirement, owner, consumer, maturity, usage evidence, and keep/merge/defer decision. Update status documents so source truth and product truth agree.

---

## 7. Are 127 MCP tools legitimate?

At the audited commit, [`kontor-mcp/src/registry.rs`](../../crates/kontor-mcp/src/registry.rs) contains:

| Dimension | Count |
| --- | ---: |
| Tool specifications | 127 |
| Unique tool names | 127 |
| Unique HTTP method/path mappings | 127 |
| Read tools | 56 |
| Stream tools | 2 |
| Write tools | 69 |
| GET operations | 41 |
| POST operations | 86 |
| Observer tier | 41 |
| Operator-only addition | 42 |
| Admin-only addition | 44 |
| Worker serve profile | 16 |

The registry uses one declarative row per operation and one dispatcher. Contract, parity, cardinality, schema-closure, and localized mutation tests protect uniqueness and mapping. Therefore, “127 tools” does not mean 127 parallel implementations or 127 tools loaded into every seat.

The worker serve profile now exposes exactly 16 tools, and both listing and calling enforce the profile. This materially fixes the ordinary delivery-seat context problem. Full operator/admin surfaces remain broad, but they serve control-plane roles rather than workers.

The existing [MCP context-tax reduction plan](../../../../_docs/ai-orchestration/plans/2026-08-19-19-45-plan-kontor-mcp-context-tax-reduction.md) has already identified a safe post-fence consolidation: merge ten preview/apply pairs using `dry_run` and consolidate ten catalog list operations, reducing the surface by about 19 tools after ASMA-7869 permits it.

**Verdict:** The current semantic count is defensible, the worker context surface is well controlled, and immediate arbitrary deletion is not warranted. Continue with role-specific profiles and the planned consolidation. Require each new tool to name its consumer, tier/profile, API parity evidence, and why an existing command cannot carry the operation.

---

## 8. Rust safety, panics, and error handling

- First-party source contains no `unsafe`; the workspace denies unsafe code.
- An opt-in lint run found no `unwrap_used` or explicit `panic` warnings in library/binary targets.
- It found 45 `expect`/`expect_err` uses in shipping files and another 69 in public fake/fixture/test-support modules.
- Shipping uses include invariant assertions, poisoned mutex locks in adapter clients, and `expect_err` assumptions used as control flow.

Not every `expect` is a problem. A desktop top-level startup failure, a compile-time/static invariant, or a lock poison that makes safe recovery impossible may reasonably terminate. Request-serving and runtime-reconciliation paths should return typed internal errors or use a clearly named invariant helper that captures why termination is sound.

**Finding CQ-05 — Medium:** Adopt an explicit panic/invariant policy. Ratchet `expect` in request/runtime paths, permit narrowly documented top-level or impossible-state cases, and test the corresponding failure mapping.

---

## 9. Test quality and automated gates

### 9.1 What passed

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| `cargo test --workspace --exclude kontor-tests-e2e` | Pass outside the filesystem/network sandbox |
| Listed tests | 1,612 |
| Ignored live-runtime tests | 8 |
| Executed non-ignored tests | 1,604 |
| `cargo audit` | Exit 0; zero vulnerability advisories |
| `cargo deny check` | Exit 0; advisories, bans, licenses, and sources accepted |

The initial in-sandbox test run failed only because a CLI parity test binds a loopback socket. Running the same suite with the required OS permission passed.

The repository's [CI workflow](../../.github/workflows/ci.yml) enforces formatting, standard Clippy, workspace tests, audit, and deny. That is a solid correctness baseline.

### 9.2 Missing evidence

- No `cargo llvm-cov` installation or quantitative coverage baseline.
- No whole-workspace mutation score. MCP-specific mutant tests and four killed mutants in a plan are useful but localized.
- No `cargo udeps`/`cargo machete` unused-dependency audit.
- No CI file/function-size, cognitive-complexity, or no-new-clone ratchet.
- The end-to-end pilot was not executed because it writes into evidence directories that already contain unrelated user-owned runs. No tracked evidence bundle matching commit `559c70f` was found.

High test LOC and a green suite do not by themselves prove that assertions are strong. Coverage shows what ran; mutation testing samples whether failures are observed. Both should be added selectively, not used as vanity percentages.

**Finding CQ-06 — Medium:** CI validates ordinary correctness but not maintainability or test sensitivity. Add trend-based gates and publish evidence artifacts; avoid a one-time hard threshold that encourages gaming.

---

## 10. Dependencies and supply-chain quality

Strengths:

- direct workspace dependencies are exactly pinned;
- GitHub Actions use immutable SHAs;
- wildcard dependencies, OpenSSL, yanked crates, disallowed licenses, and unapproved sources are policy-controlled;
- current `cargo audit` reports zero vulnerability advisories;
- the previously relevant `h2` issue is fixed by the current lockfile;
- yanked `arrayref` was replaced by a 504-line local 0.3.9 vendor copy, and `cargo deny` is green.

Residual concerns:

- `Cargo.lock` contains 749 packages and 57 crate names with multiple versions, largely through Tauri, Stronghold, GTK, and platform trees. `cargo deny` reports these as warnings.
- `cargo audit` reports 18 unmaintained informational packages in transitive desktop/security trees.
- [`deny.toml`](../../deny.toml) ignores [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html), an unsoundness advisory for `glib` below 0.20. The waiver says KON-MVP-17 will re-evaluate it when the desktop shell is implemented, but the README and source now say the Tauri desktop shell exists. The stated re-evaluation trigger is stale even if the affected `VariantStrIter` API remains unreachable.
- Vendoring solves the yanked-package gate but transfers monitoring and patch ownership to Kontor.

**Finding CQ-07 — Medium:** Re-evaluate the glib waiver now. Record current call-site/reachability evidence and an upgrade owner/date, or move to a fixed dependency tree when Tauri permits. Track the vendored crate and the unmaintained transitive set explicitly.

---

## 11. Public test-support surface

Five public test-support modules live in product source:

- `kontor-runtime::fake`;
- `kontor-mcp::fake`;
- AO, Codex, and Paseo runtime `fixture` modules.

Together, public fake/fixture modules account for approximately 4,363 lines. Integration/contract tests need reusable test support, but exporting it unconditionally inflates compilation and public API and makes accidental production use possible.

**Finding CQ-08 — Medium:** Move shared fixtures to a dedicated test-support crate or guard them behind an explicit `test-support` feature enabled only by dev-dependencies and contract-test packages. Preserve cross-adapter contract reuse.

---

## 12. Prioritized remediation program

### 12.1 Immediate: 0–3 days, no behavior changes

1. Put a temporary “no net growth without decomposition” fence on:
   - `kontor-daemon/src/applications.rs`;
   - `kontor-store/src/repository.rs`;
   - `kontor-api/src/applications.rs`;
   - the API router and runtime adapter trait.
2. Add a CI report and ratchet:
   - capture current file/function-size baseline;
   - fail only on new or enlarged violations at first;
   - report opt-in `too_many_lines` and `cognitive_complexity` findings;
   - set textual duplication to no new clones or a repository ceiling near 3%, with explicit exclusions for generated/vendor code.
3. Revalidate the glib advisory waiver and update its rationale.
4. Correct README/index capability status and build a capability inventory.
5. Adopt an `expect`/panic policy for long-running service paths.

### 12.2 Near term: 1–3 weeks

1. Split `ApplicationOperations` into cohesive capability ports, for example:
   - project/catalog;
   - topology and placement;
   - capacity/accounts;
   - teams and quick sessions;
   - advisor/committee consultations;
   - completion and epic lifecycle;
   - scheduling/runtime lifecycle;
   - intake/integrations;
   - memory.
2. Move each daemon implementation into a corresponding module. Keep `Services` as the composition root if that remains useful; do not replace one giant trait with one giant generic framework.
3. Build nested API routers per capability and compose them in the top-level 531-line router.
4. Split store persistence by aggregate or transactional workflow. Share transaction/revision/receipt helpers, not a lowest-common-denominator generic repository.
5. Split `RuntimeAdapter` into a small base lifecycle port plus optional capability subtraits/objects. Preserve capability discovery and shared contract tests.
6. Feature-gate or relocate public fake/fixture modules.
7. Extract typed-ID generation and stable adapter/store sequences after confirming the same invariant occurs in at least three places.
8. Factor shared test scenario builders while keeping assertions local and readable.

### 12.3 Product-scope and MCP gate

For every public API/MCP capability, record:

- requirement/ticket and domain owner;
- known caller or role;
- maturity: complete, partial, experimental, or unsupported;
- tier and serve profile;
- last verified use or pilot evidence;
- keep, merge, defer, or remove disposition.

Then execute the already fenced approximately 19-tool consolidation and add role profiles where a caller needs less than its credential tier exposes. Reject new MCP tools without consumer, profile, parity, and consolidation rationale.

### 12.4 Longer-term quality evidence

- Add `cargo llvm-cov` as a published baseline, focusing first on core/store/policy/dispatch decision branches.
- Run bounded `cargo mutants` samples on high-value invariants rather than mutating the entire 240k-line workspace on every commit.
- Add a periodic unused-dependency audit.
- Review duplicated dependency versions and unmaintained transitive packages on a schedule tied to desktop/runtime upgrades.
- Track p90/p95 function size, hotspot file size, clone percentage, and mutation survival as trends. Do not optimize total LOC in isolation.

---

## 13. Acceptance criteria for the maintainability pass

The first pass is complete when all of the following are true:

- no capability port has more than roughly 20–25 cohesive methods without a documented exception;
- `applications.rs` no longer owns unrelated capability implementations in one file;
- the API router is composed from capability routers;
- all current >200-line production functions have been reviewed and either decomposed or explicitly justified;
- no current hotspot file grows beyond its audited baseline;
- CI publishes size/complexity/duplication trends and blocks regressions;
- public fakes/fixtures are not in the default production feature surface;
- glib waiver evidence reflects the implemented desktop state;
- README, documentation index, and capability inventory agree with source truth;
- MCP consolidation/profile decisions are recorded without reducing required operational capability;
- a coverage baseline and targeted mutation sample exist for the highest-risk decision logic.

These criteria intentionally do not demand a particular total LOC or zero duplication. The goal is lower change coupling and review cost while preserving the tested domain behavior.

---

## 14. Final scorecard

| Area | Assessment | Reason |
| --- | --- | --- |
| Behavioral correctness evidence | Strong | 1,604 passing non-ignored tests; contract/parity/receipt coverage |
| Rust safety and ordinary lint hygiene | Strong | Unsafe denied; fmt and `-D warnings` Clippy green |
| Crate architecture | Good | Clear capability crates and provider adapters; domain core low in graph |
| SOLID at central interfaces | Weak | 108-method application port and 31-method runtime port |
| File/function maintainability | Poor | Severe tail: 18k/11k/7.8k production files and 123 >100-line functions |
| DRY/reuse | Good with hotspots | 2.04–2.44% textual duplication; localized store/ID/adapter/registry repetition |
| YAGNI/product-scope proof | Not demonstrated | Broad reachable surface but no usage inventory; rapid append-heavy growth and status drift |
| MCP semantic legitimacy | Good | 127 unique one-to-one operations, central dispatcher, 16-tool worker profile |
| Dependency hygiene | Good with follow-up | Zero vulnerabilities and deny green; stale glib waiver and unmaintained transitives |
| CI maintainability gates | Incomplete | Correctness gates present; complexity, duplication, coverage, mutation, unused deps absent |

**Decision:** Continue using the current architecture, but treat capability decomposition and maintainability ratchets as prerequisite work for another large growth wave. The evidence does not justify rewriting Kontor or deleting code by percentage. It does justify refactoring the central interfaces and hotspots now, while the domain boundaries and contract tests are strong enough to make that work safe.

---

## 15. Limitations

- This is a snapshot audit at commit `559c70f`; later commits may change the figures.
- The evidence-writing end-to-end pilot and ignored live-provider tests were not run.
- No production load, latency, memory, contention, database-query-plan, or penetration test was performed.
- No live usage/adoption telemetry was available, so product-level YAGNI conclusions remain provisional.
- No quantitative coverage or whole-workspace mutation/unused-dependency tool was installed.
- Text clone detection cannot find every semantic duplicate and may count intentional declarative/test repetition.
- The React console and TypeScript code were outside the requested Rust scope.
- Function LOC was calculated with a lexical brace-aware scanner and should be treated as an accurate audit estimate, not compiler semantic data.

## 16. Reproduction commands

Core checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude kontor-tests-e2e
cargo audit
cargo deny check
```

Opt-in maintainability lint set:

```bash
cargo clippy --workspace --all-targets -- \
  -W clippy::too_many_lines \
  -W clippy::cognitive_complexity \
  -W clippy::too_many_arguments \
  -W clippy::type_complexity \
  -W clippy::large_enum_variant \
  -W clippy::large_stack_arrays \
  -W clippy::needless_pass_by_value \
  -W clippy::missing_const_for_fn \
  -W clippy::redundant_clone
```

Panic-policy check:

```bash
cargo clippy --workspace --lib --bins -- \
  -W clippy::unwrap_used \
  -W clippy::expect_used \
  -W clippy::panic
```

Clone figures were produced with jscpd against Rust sources in normal/mild and comment-ignoring/weak modes, excluding build output and unrelated evidence.
