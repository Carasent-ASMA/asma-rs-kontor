Showing 2 of 94 activities (limited to 2)

**ASMA-8278 — independent bounded SOURCE findings (read-only SEAT B)**
Runtime: OpenCode/DeepSeek `deepseek/deepseek-flash`, max, Plan, `auto_accept=false`. CSW `wks_2ed7b2da12401827`, project `prj_e9f8052597f78919`.

**Candidates reviewed (exact, immutable)**
- Module `dfeaa8a533db46a8d5b85cd05d491bb8bb531b4e` (tree `7f8d0178…`), base `545b44a1…`, cwd.
- Root `c8e35955be230c5cc4ed9917302a781d94e39308` (tree `25991e2b…`), base `884f9f66…`, `/tmp/asma-8278-root-publication-20261002.xzG34B`. Verified `git diff 228b3004..c8e35955` = 2 docs only (unnumbered proposal + shared-orchestration plan); no runtime source delta.
- All reads by `git show <exactSHA>:<path>` / frozen patch only; no HEAD, no working files, no test execution, no writes.

**Reviewed hashes (git blob SHA-1 / sha256; sha256 matches FINAL-REVIEW-MANIFEST.json)**
```
c76a7d90…  crates/kontor-fleet/src/allocation.rs                  3b964b06…  ✓
e7eb1425…  crates/kontor-fleet-activation/src/lib.rs              96515ee7…  ✓
178a1f38…  crates/kontor-runtime/src/planning_pair/application.rs 25855976…  ✓
652cfd7e…  …/application/commands/contribution.rs                 45b94796…  ✓
45cbf617…  …/application/commands/recovery.rs                     48c11962…  ✓
3e8be8f0…  …/application/materialize.rs                           b7825531…  ✓
453576c2…  crates/kontor-runtime/src/planning_pair/recovery.rs     97cfb247…  ✓
4f2631de…  crates/kontor-runtime/src/planning_pair/intent.rs       401ae5ed…  ✓
8e977713…  crates/kontor-daemon/src/applications/planning_pair.rs  12436473…  ✓
f22b205e…  …/applications/planning_pair/invocation.rs             b7fd9c9d…  ✓
192d4470…  …/applications/planning_pair/refusal.rs                 da5419d6…  ✓
2e78caf3…  …/applications/planning_pair/commands.rs               bcf6d560…  ✓
05e1a336…  crates/kontor-core/src/naming.rs                       16be6e62…  ✓
02d83156…  crates/kontor-core/tests/native_naming.rs              d8985a29…  ✓
c13b3c75…  _tools/ai-orchestration/tpm_supervision.py              77546a71…  ✓
e92a8f8e…  _tools/ai-orchestration/tests/test_tpm_supervision.py   020e6555…  ✓
1965bfd4…  config/orchestration/tpm-supervision.yml               7d5c1d0c…  ✓
```

**Findings (severity: High=reachable correctness/safety, Medium=reachable degradation, Low=latent/consistency/availability)**
- **S-01 · Low · latent receipt classification divergence (W3b).** `contribution.rs:134-136` (and `finish` `:288-301`) render `Applied::Created` unconditionally, while `ContributionOwner::record` returns only `CommandReceiptId` (`commands.rs:50-58`; daemon `commands.rs:107-118` → `applications.rs:6294-6305` drops the `inserted` flag from `record_classified` `:6315-6359`). W3a's invoke classifies exactly (`application.rs:552-564`). No reachable duplicate/misclassification at candidate: run CAS (`planning_pair.rs:1044-1057,1062-1097`) and domain immutability (`kontor-core/planning_pair.rs:793-807,817-839,847-870,881-910`) refuse the second append before `record` can meet an existing key. Consistency gap; live qualification unmet.
- **S-02 · Low · unbounded backtracking in the cap allocator.** `allocation.rs:400-487` walks exhaustively with no memoization/step budget; the CLI-facing request validates only non-empty and unique `slot_id` (`kontor-fleet-activation/src/lib.rs:740-756`; request shape `:620-647`). Planning-pair path is fixed at two slots (`:900-923`), so exposure is the generic direct-mode allocation. Not a correctness defect; no live load evidence.
- **S-03 · Low · guard asymmetry in the TPM checkpoint helper.** `tpm_supervision.py:464-466` (`rebind`) has no status guard, unlike `settle` `:448-451`, `resume` `:452-456`, `transfer` `:457-463`. A closed checkpoint stays closed and delivery-ineligible (`:479-483`), so no delivery effect; whether this is intentional is UNKNOWN. All 21 supervision tests enumerated and match the manifest; they exercise the other guards, not this asymmetry.
- **R-01 · Review boundary (not a defect) · member re-occupancy fencing.** Write authentication compares the presented generation to the frozen stored member generation and requires an observed native identity (`applications/planning_pair.rs:645-675`) but does not re-read `hosted_topology_seat_occupancy_generation` as caller auth does (`:437-462`); credential extraction (`kontor-api/src/planning_pair.rs:455-467`) was not read in this bounded pass. Assessment UNKNOWN pending that path and live re-occupancy qualification.

**Focus areas with no source defect found**
- **Cap allocator rank/vendor/cap**: rank `step==0`→`RungUnknown`, `step>2`→`RungBeyondVerdict`, unknown maker cannot evade cap, cap counts reviewers+judge, constraints derived from protocol identity only (`allocation.rs:360-398,435-441,565-605`; `lib.rs:768-794`). Tests-as-source `fleet/src/tests.rs:1155-1436` cover judge counting, joint-not-greedy, unknown-evasion, rung floor/unstated rung, scope; read only, not executed.
- **W3a/W3b order**: document-hash → authenticate → intent → replay → CAS → effect (`application.rs:483-546`; `contribution.rs:88-136,189-229,240-282`; `commands/recovery.rs:136-200,206-278`); order asserted by `application/tests.rs:179-226`, `commands/tests.rs:275,292,324`.
- **Parity/refusal semantics**: `refusal.rs:141-232` with baseline-literal tests (`:238-596`: 22 invocation, 11 caller, 10 context, 5 recovery, 6 withdrawal).
- **Retired-title renderer**: scalar-count truncation, cap ≥ prefix+2 and ≤512, repeated leading-prefix strip, ellipsis rejection, final bound re-parse (`naming.rs:304-357`); 18 tests = 18 `#[test]`s in `native_naming.rs`.
- **Blocked/owner/revision/event guards, wake renew/shutdown**: owner before dedup before revision, payload-hash contradiction, journal cap (`tpm_supervision.py:430-470`); advisory renew vs shutdown (`:204-227`).

**Source-only assessment**: no High or Medium source defect substantiated in the named areas; 3 Low observations above. UNKNOWN/unread: store-level CAS SQL (migrations 0122/0123, `kontor-store` repository), loopback/parity suites (not executed), API credential verification path, daemon `applications.rs` beyond cited slices. No invented PASS; no epic closure.

**Preserved limits (verbatim intent)**: all 27 paired live cases remain unmet/partial; B2 direct owner not implemented at candidate; native restrictions/fencing/possession unsupported; frozen R3 DB absent/runtime UNKNOWN/init UNMET; MUT-002/004 real boundaries unperformed. Igor approved exact c8 D1–D4 and the DeepSeek/GLM/CodexPersonal alternatives; no global fleet/calibration/pin change or deployed capability inferred. This is user-authorized bounded source findings, not a complete IndependentReview aggregate and not a formal calibration waiver.

Root notified by persistence of this reply.
