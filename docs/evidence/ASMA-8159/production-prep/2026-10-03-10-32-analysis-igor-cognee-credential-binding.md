# Igor reserved decision: Cognee credential supplier and target binding

> Date: 2026-10-03 Europe/Oslo
> Status: Draft proposal; Igor decision and account/security owner implementation required
> Category: specification
> Scope: ASMA-8159 source/fake preparation only; Paseo-direct
> Summary: A proposed trusted supplier boundary for an already supplied HTTP credential. No real lookup, registration, account-resolver change or runtime activation is implemented or authorized by this document.

## When to load

Load when TPM/LSA routes the concrete real-supplier proposal to Igor and the account/security owner. The packet is documentation only; synthetic composition continues within the existing disposition.

## Authority and current proof

The LSA disposition at `/private/tmp/ASMA-8155-ASMA-8159-lsa-production-prep-disposition-20261003.json` is 30,440 bytes, SHA-256 `2b2c89f3b417237dadaebbb1d3f8698eb94b60c926bc01dc4b975224d7a8c55c`, with final LF. Its sections 2, 3 and 5 allow synthetic composition through `Client::new(config, supplied_material)` and `DaemonConfig::with_memory_cognee`. Real alias resolution and child-environment extraction remain reserved. The accepted account resolver has private material and only `ResolvedAccountEnvironment::apply(Command)` delivery. This proposal leaves that contract intact.

The implemented async rebuild caller stages and binds an exact preview in a local transaction, releases the store lock, awaits `Client::qualify`, then uses the existing authoritative activation guard and late freshness/CAS in a second transaction. Its original result receipt commits atomically with activation. Completed same-key/same-canonical-request replay precedes client availability and survives restart; different bytes or project under the same key refuse with `binding_conflict`. Concurrent qualification can duplicate provider calls; there is no claim of exactly-once provider effects. A failed qualification or failed result write cannot advance the pointer.

Source/fake proof: `crates/kontor-daemon/tests/projection_rebuild.rs`. The transport uses a synthetic bearer and loopback WireMock only. Success, local-only exclusion, HTTP/timeout/malformed/canary failure, stale previews, an in-flight ledger change, concurrent same/distinct keys, immutable result failure/retry, replay after restart, absent/disabled composition and operator/observer parity are exercised. The adverse-certificate MUT-007 witness calls the same composed application helper, never direct store activation. Exact run/source/patch receipts are in [receipts](receipts/). These are writer verification, not independent acceptance.

## Decision requested from Igor, with owner routing

Choose and approve the trusted supplier owner, consumer identity and target binding before real implementation. Recommended proposed boundary A is a distinct Cognee deployment supplier owned by account/security/startup composition. It uses an approved alias-to-target policy and an explicit SecretString delivery to the fixed Cognee client consumer, while preserving account-profile child-only delivery. This is a proposal, not a newly accepted authority or secret namespace.

The proposed exact non-secret binding tuple is:

| Binding component | Proposed meaning and required approval |
| --- | --- |
| Consumer | Closed `memory_cognee_http_v1`, not arbitrary caller or runtime profile |
| Realm | Existing immutable Realm UUID read from the selected database |
| Root | Existing canonical state-root hash; no profile-controlled paths or symlink joining |
| Endpoint | Canonical scheme/host/port/base path, no userinfo/query/fragment; HTTPS for non-loopback |
| Alias | Exact validated `credential_alias`; deployment input, never a secret or target string |
| Target | Approved policy entry only; backend service/account identities redacted and never inferred from Jira |
| Scope | Dataset namespace `kontor`, exact project/snapshot digest, approved endpoint and provider egress policy |
| Lifetime | Supplier-issued policy revision, not-before/expiry and rotation generation; recheck before composition and replace client on rotation |

Igor must decide whether the scope is per Realm/root/endpoint or more restrictive per project and how revocation reaches an in-memory client. No fallback to an ambient token, anonymous deployment, alternate alias, Jira secret or default account is proposed. Refusal codes should distinguish unapproved binding, unavailable material, expiry/revocation and policy mismatch without exposing values, target identifiers, upstream bodies or source-error chains. Missing or stale policy leaves the client uncomposed; public rebuild remains typed `projection_unavailable`.

## Exact proposed interface and startup diff (documentation only)

This sketch belongs in the account/security composition owner’s module, outside all memory crates. Type constructors must validate the entire approved tuple; fields must not permit an arbitrary alias/target lookup. No Serialize or secret-bearing Debug implementation is proposed.

```rust
// PROPOSAL ONLY: owner-selected location, names and storage require approval.
pub struct CogneeCredentialBinding {
    consumer: ApprovedCogneeConsumer,
    realm: RealmId,
    canonical_root_hash: ContentHash,
    canonical_endpoint: ApprovedEndpoint,
    alias: CredentialAlias,
    policy_revision: ApprovedPolicyRevision,
}
pub struct SuppliedCogneeCredential {
    material: SecretString,
    binding: CogneeCredentialBinding,
    expires_at: Timestamp,
    rotation_generation: u64,
}
// Exact approved binding lookup only; static redacted reason, no chained errors.
pub trait CogneeCredentialSupplier {
    fn supply(&self, binding: &CogneeCredentialBinding)
        -> Result<SuppliedCogneeCredential, CogneeCredentialRefusal>;
}
```

```diff
--- crates/kontor-daemon/src/main.rs (PROPOSAL ONLY)
+++ crates/kontor-daemon/src/main.rs (PROPOSAL ONLY)
@@ trusted serve composition before start_configured
- let config = DaemonConfig::new();
+ let mut config = DaemonConfig::new();
+ // Config stays disabled by default. All policy/credential work is outside memory.
+ if approved_memory_config.enabled {
+     let binding = approved_cognee_binding_from_trusted_startup_policy(
+         realm, canonical_root, &approved_memory_config)?;
+     let supplied = approved_supplier.supply(&binding)?;
+     validate_binding_expiry_and_rotation(&binding, &supplied)?;
+     let client = Client::new(approved_memory_config, Some(supplied.material))?;
+     config = config.with_memory_cognee(client);
+ }
  Daemon::start_configured(..., config).await?;
```

The helper names above denote future owner work, not callable APIs. Proposed scope has no changes to `kontor-accounts/src/resolver.rs`, no generic secret getter, no reading `Command::get_envs`, no child-to-parent extraction, no memory-path Keychain/env access, and no process-global environment mutation. A backend may need an approved Keychain target policy, but this document assigns no service/account names and executes no lookup.

## Alternatives and the child-only contract

| Alternative | Tradeoff and disposition |
| --- | --- |
| A: dedicated trusted deployment supplier delivering SecretString to fixed HTTP consumer | Proposed; narrow HTTP delivery still requires Igor approval and account/security implementation/review. Account child-only API stays unchanged. |
| B: explicitly widen AccountResolver delivery to an approved typed HTTP consumer | Requires a separately reviewed owner diff, policy proof and Igor acceptance of a new delivery contract. An eventual `into_http_credential(approved_consumer, binding)` would need single-consumer ownership, expiry/revision checks, redaction and negative fixtures. No such method is added here. |
| C: retain child-only delivery and move Cognee access into a constrained child/service | Preserves existing delivery mechanism but adds another process boundary, protocol and deployment/rollback obligations; must separately qualify outbound scope and errors. |
| Continue synthetic supplied-material injection | Authorized now and implemented; qualifies composition behavior but does not supply real credentials or release authority. |

Extracting fields through command environments or changing resolver visibility without approval is not an alternative. Runtime profile credentials and Jira registration are not Cognee deployment inputs.

## Egress and outbound-data policy to approve

Current client has no redirects, implicit retries or proxy inheritance. HTTP is accepted only for loopback; non-loopback endpoints require HTTPS. A credential-bearing request goes only to the configured endpoint’s `/api/v1/add`, `/api/v1/cognify` and `/api/v1/search` paths. Snapshot data is store-generated, canonical and provider-safe; local-only revisions are excluded. Providers return locators and finite scores only; authoritative bytes are rehydrated from the local ledger.

Deployment approval must bind the exact endpoint/image/user/dataset ownership and its downstream embedding/LLM/vector/graph/storage providers, regions, retention/deletion, training/use policy and network allowlist. Self-hosting alone does not establish those policies. Reject redirects, insecure remote endpoints, policy drift, wrong-owner canary identities and local-only exposure. Operator logs/evidence carry only hashes, redacted binding IDs and static outcomes. DNS/TLS/service-account isolation, provider-disabled rehearsal and egress auditing remain later operational proof.

## Primary documentation, dependency and licence verification

The accepted adapter inspection pin is upstream Cognee `b32d8afc59e1064d9291b9828a8a147be9cc8bab`; it is an inspection pin, not a selected production image digest. Its [LICENSE](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/LICENSE) declares Apache-2.0. Its [pyproject](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/pyproject.toml) declares version 1.6.2 and a broad dependency set; this is not a lockfile/SBOM or a production dependency acceptance. Exact fetched public file hashes appear in `receipts/upstream-primary.json`.

The pinned [CLI HTTP client](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/cli/api_client.py) corroborates multipart `data` plus `datasetName`, blocking cognify inputs and `CHUNKS` search with datasets/top_k. Its caller-supplied headers do not prove a service bearer’s registration, expiry or identity. Our stricter no-redirect transport must be qualified against the selected deployment.

The pinned [authentication method](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/modules/users/methods/get_authenticated_user.py) derives authentication posture from server deployment configuration. It does not prove this operator’s configuration. The [search router](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/api/v1/search/routers/get_search_router.py) scopes dataset-name lookup to the authenticated sender; the actual deployment principal must own the intended datasets. The [pipeline status model](https://github.com/topoteretes/cognee/blob/b32d8afc59e1064d9291b9828a8a147be9cc8bab/cognee/modules/pipelines/models/PipelineRunInfo.py) distinguishes completed/already-completed/error states, matching the need for blocking completion and a later canary.

This source change adds no Cargo/npm/Python dependencies and does not change lockfiles. Final `cargo deny --offline check licenses bans sources` is recorded separately. The inherited yanked `yoke-derive 0.8.3` advisory remains with ASMA-8113; no duplicate correction is attempted. Production requires exact image digest, full transitive licence/SBOM/security review, selected provider documentation and deployment-specific canary/rollback proof. Public source inspection performs no live Cognee or provider access.

## Later prerequisites and acceptance evidence

Igor’s explicit recorded decision on supplier/consumer/alias/target binding and any resolver delivery change; approved account/security owner implementation plus independent review; supplied-material redaction/negative tests; selected image/provider/egress/licence evidence; independently accepted combined artifact including serialized migrations/dependency corrections; accepted real-corpus provenance and classification; verified backup and tested rollback/isolated same-Realm copy; reviewed operator activation plan and 24-case benchmark; deployment/restart/readback against that exact artifact; prompt/default/opt-out and no duplicated dispatch proof. No release or runtime activation is authorized by this packet. The full release inventory remains in the [LSA section-6 checklist](../2026-10-03-01-11-plan-release-readback-and-evidence.md).
