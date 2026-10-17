//! ASMA-8015 macOS credential platform harness: test-only, explicitly ignored.
//!
//! This target proves, on a macOS host and only when a human runs it
//! deliberately, that the stopped-realm credential installer behaves against
//! the real `SystemKeychain` backend the way its fake-backend unit tests claim:
//! an owned previous entry is restored byte-for-byte, a fresh entry is deleted,
//! the production outcome is `RolledBack`, and the built daemon binary plus the
//! production `JiraConnectors` reader present the installed credential to a
//! loopback-only responder.
//!
//! # Why every case is `#[ignore]`
//!
//! Every case below performs real OS effects (login keychain writes, a
//! disposable Realm under a temporary root, a loopback HTTP responder, and a
//! subprocess of the built daemon binary). None may run as part of an ordinary
//! `cargo test`/CI invocation, and none is reachable through test discovery or
//! compilation: the file defines no initializer, no lazy static and no setup
//! hook, and every test body is guarded by `#[ignore]`. A deliberate run is:
//!
//! ```text
//! cargo test -p kontor-jira --test macos_credential_platform -- --ignored
//! ```
//!
//! The binary case additionally needs the exact daemon binary built at the
//! qualified source revision, supplied as a non-secret absolute path, and
//! refuses to run if any recognized proxy variable is present:
//!
//! ```text
//! KONTOR_ASMA8015_DAEMON_BIN=/abs/path/to/kontor-daemon \
//!   cargo test -p kontor-jira --test macos_credential_platform -- --ignored --exact \
//!   the_built_daemon_installs_and_the_shared_production_readers_present_the_installed_credential
//! ```
//!
//! # Normal-host invariants (no test effect without `--ignored`)
//!
//! * Binary: every test in this file carries `#[ignore]`; running the target
//!   normally executes zero tests, and nothing here runs at compile time,
//!   during test discovery, or as global setup.
//! * Config: the harness writes only inside its own `TempDir` (a fresh
//!   `jira.json` and the disposable Realm export); it never edits configuration
//!   under the canonical checkout or the user's home.
//! * Service: every keychain address it can form is
//!   `kontor-jira:<fresh RealmId>:<canonical temporary root hash>`, derived
//!   exactly like `JiraCredentialScope::at`. Cleanup starts disarmed, the
//!   absence fence proves the exact target `NotFound` through the real backend,
//!   and only then is one single-target delete armed. Any other fence outcome
//!   (an existing or unreadable entry) stops the harness before it writes and
//!   with zero delete attempts, including during unwinding. A service-wide,
//!   legacy/global or unknown pre-existing entry is never listed, overwritten
//!   or deleted.
//! * Credential: every synthetic email and token is generated inside the
//!   running test, and the credential document travels only through an
//!   anonymous stdin pipe to the built daemon (or through `install_credentials_with`
//!   in process). It is never placed in the environment, argv, a file, a log, or
//!   an assertion diagnostic, and the pipe buffer is zeroized.
//! * Output: the binary case captures at most a bounded stderr excerpt, redacts
//!   the synthetic email and token substrings, strips control characters, and
//!   never captures request headers or Authorization values. The loopback
//!   responder records only a path, a presence bit and a match bit; the
//!   transient request-head and derived Authorization bytes are compared in
//!   place and gain no diagnostic path.
//!
//! # Ambiguity and recovery
//!
//! A `RollbackUnproven` outcome, an unproven built-daemon outcome (including a
//! spawn, stdin-handle or wait failure), an unproven absence fence, or any
//! cleanup uncertainty is a hard stop: the harness does not retry, does not
//! claim a verified rollback, keeps the disposable root by disabling its
//! cleanup, and emits a static, structured reconciliation block naming the exact
//! non-secret `service`/`account`/`Realm`/root hash and whether ownership is
//! proven (`PROVEN_AFTER_ABSENCE_FENCE`) or `UNPROVEN` with zero writes and zero
//! deletes. Every delegated read, delegated write, final read and assertion on
//! an owned target is explicit `Result`/`Option` handling; the type carries no
//! destructor effect, so no panic-driven unwinding can delete without
//! verification or context. The wording after any failure states the remaining
//! scoped state as UNKNOWN rather than claiming the entry was preserved. The
//! synthetic credential document, token and email are deliberately not
//! retained.
//!
//! # Known limitations, stated rather than papered over
//!
//! * The fault injection is a deterministic one-shot read failure after the
//!   first successful set. It does not prove crash atomicity: a process killed
//!   between the write and the rollback is not simulated here.
//! * An ACL prompt surfaces as a bounded `Denied`/`Unavailable` failure and is
//!   a hard stop, never a retry.
//! * Diagnostic redaction covers only this harness's own synthetic email and
//!   token, is length-bounded, and is not a general credential redactor; it
//!   makes no claim about arbitrary credential material.
//! * The binary case exercises the happy path only; it is not credited to
//!   library fault injection and shares no wrapper with the deterministic cases.

#![cfg(target_os = "macos")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use kontor_accounts::{
    KeychainBackend, KeychainFailure, KeychainTarget, KeychainWriter, SystemKeychain,
};
use kontor_core::id::{ContentHash, ExternalId, ProjectId, RealmId};
use kontor_jira::{
    CredentialInstallError, JiraConnectors, JiraCredentialScope, install_credentials_with,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use zeroize::{Zeroize, Zeroizing};

/// Non-secret path input naming the daemon binary built at the qualified source.
const DAEMON_BINARY_ENV: &str = "KONTOR_ASMA8015_DAEMON_BIN";
/// The disposable Realm export the harness reads its fresh realm id from.
const REALM_EXPORT_FILE: &str = "asma-8015-realm-export.json";
/// The connector's inbound-comment page path for [`ISSUE_KEY`].
const COMMENTS_PATH: &str = "/rest/api/3/issue/ASMA-1/comment";
/// The session reference path the shared readers are compared against.
const SHARED_READER_REFERENCE_PATH: &str = "/asma-8015-shared-reader-reference";
/// A synthetic issue key inside the responder's project.
const ISSUE_KEY: &str = "ASMA-1";
/// The project key the responder answers for.
const PROJECT_KEY: &str = "ASMA";
/// The service namespace `JiraCredentialScope` prepends to realm and root.
const SERVICE_PREFIX: &str = "kontor-jira";
/// Who owns the scoped entry a reconciliation must resolve.
const HARNESS_OWNER: &str = "ASMA-8015 macOS credential platform harness \
     (crates/kontor-jira/tests/macos_credential_platform.rs)";
/// Bounded stderr capture from the built daemon.
const DIAGNOSTIC_LIMIT_BYTES: usize = 16 * 1024;
/// Bounded emitted excerpt of the redacted daemon diagnostic.
const DIAGNOSTIC_EXCERPT_CHARS: usize = 2048;
/// Bounded request head the loopback responder reads.
const REQUEST_HEAD_LIMIT_BYTES: usize = 16 * 1024;
/// The synthetic comment page the responder serves.
const COMMENTS_BODY: &str = r#"{"startAt":0,"maxResults":100,"total":0,"comments":[]}"#;
/// The synthetic reference response body.
const REFERENCE_BODY: &str = "{}";

/// Every proxy environment key the production HTTP stack can read.
///
/// hyper-util 0.1.20 (`client/proxy/matcher.rs:231-235`) reads
/// `ALL_PROXY`/`HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` in upper and lower case
/// for the legacy client reqwest 0.13.4 uses, and reqwest 0.13.4
/// (`proxy.rs:477-478`) also reads `NO_PROXY`/`no_proxy` when resolving
/// exclusions. Presence of any key would be enough for a credential-bearing
/// request to leave loopback, so the harness hard-stops before any credential
/// effect; the value itself is never used or printed.
const PROXY_ENVIRONMENT_KEYS: [&str; 8] = [
    "ALL_PROXY",
    "all_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "no_proxy",
];

fn canonical_root(temp: &tempfile::TempDir) -> PathBuf {
    temp.path()
        .canonicalize()
        .expect("the harness temporary root could not be canonicalized")
}

/// The exact scoped service `JiraCredentialScope::at` derives, rebuilt from the
/// public id primitives so the harness can address (and only address) its own
/// fresh entry.
fn scope_service(root: &Path, realm: RealmId) -> String {
    let canonical = root
        .canonicalize()
        .expect("the harness scope root could not be canonicalized");
    format!(
        "{SERVICE_PREFIX}:{realm}:{}",
        ContentHash::of(canonical.as_os_str().as_encoded_bytes())
    )
}

fn scoped_target(root: &Path, realm: RealmId, alias: &str) -> KeychainTarget {
    KeychainTarget::new(scope_service(root, realm), alias.to_owned())
}

fn fresh_alias() -> String {
    format!("asma-8015-harness-{}", RealmId::generate())
}

fn fresh_email() -> SecretString {
    SecretString::from(format!(
        "asma-8015-{}@synthetic.invalid",
        RealmId::generate()
    ))
}

fn fresh_token() -> SecretString {
    SecretString::from(format!("synthetic-{}", RealmId::generate()))
}

#[derive(Serialize)]
struct CredentialDocument<'a> {
    email: &'a str,
    api_token: &'a str,
}

/// The production canonical document, built the same way
/// `install_credentials_with` does: compact JSON, `email` then `api_token`.
fn credential_document(email: &SecretString, token: &SecretString) -> Zeroizing<String> {
    let document = CredentialDocument {
        email: email.expose_secret(),
        api_token: token.expose_secret(),
    };
    Zeroizing::new(
        serde_json::to_string(&document)
            .expect("the synthetic credential document could not be encoded"),
    )
}

fn synthetic_install_document() -> SecretString {
    let email = fresh_email();
    let token = fresh_token();
    let canonical = credential_document(&email, &token);
    SecretString::from(canonical.as_str().to_owned())
}

/// An owned, synthetic previous entry. It is opaque to the production
/// installer, which only has to restore it byte-for-byte.
fn synthetic_previous_document() -> SecretString {
    SecretString::from(format!(
        r#"{{"email":"previous-{}@synthetic.invalid","api_token":"previous-{}"}}"#,
        RealmId::generate(),
        RealmId::generate()
    ))
}

/// Explicit handling for a delegated backend call: a failure is a hard stop
/// with retained root and structured context, never a panic whose unwind could
/// run an unverified destructor delete.
fn delegated<T>(
    result: Result<T, KeychainFailure>,
    reason: &'static str,
    reconciliation: &Reconciliation,
    temp: &mut tempfile::TempDir,
) -> T {
    match result {
        Ok(value) => value,
        Err(_) => hard_stop(reason, reconciliation, Ownership::Fenced, temp, ""),
    }
}

fn confirm_secret(
    observed: &SecretString,
    expected: &SecretString,
    reason: &'static str,
    reconciliation: &Reconciliation,
    temp: &mut tempfile::TempDir,
) {
    if observed.expose_secret() != expected.expose_secret() {
        hard_stop(reason, reconciliation, Ownership::Fenced, temp, "");
    }
}

/// How much the harness can prove about the exact scoped target at a hard stop.
#[derive(Clone, Copy)]
enum Ownership {
    /// The absence fence proved the target empty before the harness wrote, so
    /// any entry now at that exact target was created by this harness.
    Fenced,
    /// No absence proof and no write or delete; the target may hold an unknown
    /// pre-existing entry that is not this harness's to touch.
    Unproven,
}

/// The non-secret facts an operator needs to reconcile one abandoned scoped
/// entry. It never carries the synthetic credential document, token or email.
struct Reconciliation {
    state_root: PathBuf,
    realm: RealmId,
    service: String,
    account: String,
    root_hash: ContentHash,
}

impl Reconciliation {
    fn new(root: &Path, realm: RealmId, target: &KeychainTarget) -> Self {
        let canonical = root
            .canonicalize()
            .expect("the reconciliation root could not be canonicalized");
        let root_hash = ContentHash::of(canonical.as_os_str().as_encoded_bytes());
        Self {
            state_root: canonical,
            realm,
            service: target.service().to_owned(),
            account: target.account().to_owned(),
            root_hash,
        }
    }

    /// Emit a static, structured block: exact ownership, the retained root, and
    /// the one-scope recovery boundary. No credential material is included; the
    /// detail is a bounded, redacted daemon excerpt or a static cleanup label.
    fn emit(&self, reason: &'static str, ownership: Ownership, retained_root: &Path, detail: &str) {
        eprintln!("ASMA-8015 RECONCILIATION REQUIRED");
        eprintln!("  owner: {HARNESS_OWNER}");
        eprintln!("  reason: {reason}");
        eprintln!("  state_root: {}", self.state_root.display());
        eprintln!("  retained_root: {}", retained_root.display());
        eprintln!("  realm: {}", self.realm);
        eprintln!("  service: {}", self.service);
        eprintln!("  account: {}", self.account);
        eprintln!("  root_hash: {}", self.root_hash);
        match ownership {
            Ownership::Fenced => eprintln!(
                "  ownership: PROVEN_AFTER_ABSENCE_FENCE (the exact target was absent before this harness wrote)"
            ),
            Ownership::Unproven => eprintln!(
                "  ownership: UNPROVEN (zero writes and zero deletes were attempted; inspect before any operator action)"
            ),
        }
        if !detail.is_empty() {
            eprintln!("  detail: {detail}");
        }
        eprintln!("  scope_boundary: act only on this exact service/account; touch nothing else");
        eprintln!(
            "  credential: the synthetic document is not retained or written; reconcile with a fresh synthetic document"
        );
    }
}

/// Stop without any unverified delete or overwrite: retain the disposable root
/// by disabling its cleanup, emit the reconciliation block, and fail the test.
/// The wording states the remaining scoped state as UNKNOWN; it never claims the
/// entry was preserved or that a failed effect did not land.
fn hard_stop(
    reason: &'static str,
    reconciliation: &Reconciliation,
    ownership: Ownership,
    temp: &mut tempfile::TempDir,
    detail: &str,
) -> ! {
    temp.disable_cleanup(true);
    reconciliation.emit(reason, ownership, temp.path(), detail);
    panic!(
        "{reason}: the harness retained the disposable root and stopped without a retry; the remaining scoped state is UNKNOWN and must be reconciled from the emitted ASMA-8015 block"
    );
}

/// The one exact keychain target this harness proved absent and then owned.
/// Effectful cleanup is explicit only: there is no destructor effect, so no
/// panic-driven unwinding can ever delete without verification or context.
struct OwnedScopedTarget {
    target: KeychainTarget,
    armed: bool,
}

impl OwnedScopedTarget {
    /// Cleanup starts disarmed: an entry the harness has not proven absent must
    /// survive every path out of the test, including unwinding.
    fn new(target: KeychainTarget) -> Self {
        Self {
            target,
            armed: false,
        }
    }

    /// The read-before-write absence fence. Only an exact `NotFound` from the
    /// real backend arms the single-target cleanup; an existing or unreadable
    /// entry retains the root and emits an UNPROVEN reconciliation block with
    /// zero writes and zero deletes.
    fn ensure_absent(&mut self, reconciliation: &Reconciliation, temp: &mut tempfile::TempDir) {
        match SystemKeychain.secret(&self.target) {
            Err(KeychainFailure::NotFound) => self.armed = true,
            _ => hard_stop(
                "absence_fence_unproven",
                reconciliation,
                Ownership::Unproven,
                temp,
                "",
            ),
        }
    }

    /// Explicit, truthful cleanup: disarm first so the single attempt can never
    /// be retried, then delete exactly once and require `NotFound`. The
    /// disposable root is held until both steps succeed; any uncertainty routes
    /// through the same secret-free reconciliation block, and the wording never
    /// asserts that the entry still exists.
    fn cleanup(
        &mut self,
        reason: &'static str,
        reconciliation: &Reconciliation,
        temp: &mut tempfile::TempDir,
    ) {
        if !self.armed {
            hard_stop(
                "cleanup_without_absence_fence",
                reconciliation,
                Ownership::Unproven,
                temp,
                reason,
            );
        }
        self.armed = false;
        if SystemKeychain.delete_secret(&self.target).is_err() {
            hard_stop(
                "cleanup_delete_uncertain",
                reconciliation,
                Ownership::Fenced,
                temp,
                reason,
            );
        }
        match SystemKeychain.secret(&self.target) {
            Err(KeychainFailure::NotFound) => {}
            _ => hard_stop(
                "cleanup_verification_uncertain",
                reconciliation,
                Ownership::Fenced,
                temp,
                reason,
            ),
        }
    }
}

/// The deterministic one-shot wrapper: it delegates the initial read and the
/// first successful set to the real `SystemKeychain`, injects exactly one typed
/// read failure immediately after that set, then delegates rollback
/// restore/delete and verification normally. It records counts and addresses
/// (never values) so the tests can prove the fault fired on the production path.
#[derive(Default)]
struct ReadFailureAfterSet {
    set_completed: AtomicBool,
    failure_spent: AtomicBool,
    injected: AtomicBool,
    delegated_reads: AtomicUsize,
    sets: AtomicUsize,
    deletes: AtomicUsize,
    set_targets: Mutex<Vec<KeychainTarget>>,
    delete_targets: Mutex<Vec<KeychainTarget>>,
}

impl ReadFailureAfterSet {
    fn injected(&self) -> bool {
        self.injected.load(Ordering::SeqCst)
    }

    fn delegated_reads(&self) -> usize {
        self.delegated_reads.load(Ordering::SeqCst)
    }

    fn sets(&self) -> usize {
        self.sets.load(Ordering::SeqCst)
    }

    fn deletes(&self) -> usize {
        self.deletes.load(Ordering::SeqCst)
    }

    /// The production installer must have taken exactly this path: the initial
    /// delegated read, one successful delegated set, the injected read, then
    /// the rollback restore-or-delete and one verification read. Counts are
    /// asserted, not the values that crossed them. Failure is returned, never
    /// panicked: the caller decides the effectful disposition first.
    fn fault_fidelity(
        &self,
        target: &KeychainTarget,
        expected_sets: usize,
        expected_deletes: usize,
    ) -> Result<(), &'static str> {
        if !self.injected() {
            return Err("the deterministic wrapper did not inject its typed read failure");
        }
        if self.delegated_reads() != 2 {
            return Err(
                "the wrapper did not delegate exactly the initial read and the rollback verification",
            );
        }
        if self.sets() != expected_sets {
            return Err("the wrapper did not observe the expected delegated writes");
        }
        if self.deletes() != expected_deletes {
            return Err("the wrapper did not observe the expected delegated deletes");
        }
        let set_targets = self
            .set_targets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if set_targets.len() != expected_sets
            || set_targets.iter().any(|recorded| recorded != target)
        {
            return Err("a delegated write addressed a target outside the owned scoped entry");
        }
        drop(set_targets);
        let delete_targets = self
            .delete_targets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if delete_targets.len() != expected_deletes
            || delete_targets.iter().any(|recorded| recorded != target)
        {
            return Err("a delegated delete addressed a target outside the owned scoped entry");
        }
        Ok(())
    }
}

impl KeychainBackend for ReadFailureAfterSet {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        if self.set_completed.load(Ordering::SeqCst)
            && !self.failure_spent.swap(true, Ordering::SeqCst)
        {
            self.injected.store(true, Ordering::SeqCst);
            return Err(KeychainFailure::Unavailable);
        }
        self.delegated_reads.fetch_add(1, Ordering::SeqCst);
        SystemKeychain.secret(target)
    }
}

impl KeychainWriter for ReadFailureAfterSet {
    fn set_secret(
        &self,
        target: &KeychainTarget,
        secret: &SecretString,
    ) -> Result<(), KeychainFailure> {
        self.sets.fetch_add(1, Ordering::SeqCst);
        self.set_targets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(target.clone());
        let result = SystemKeychain.set_secret(target, secret);
        if result.is_ok() {
            self.set_completed.store(true, Ordering::SeqCst);
        }
        result
    }

    fn delete_secret(&self, target: &KeychainTarget) -> Result<(), KeychainFailure> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        self.delete_targets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(target.clone());
        SystemKeychain.delete_secret(target)
    }
}

#[derive(Clone, Copy)]
enum ProductionOutcome {
    RolledBack,
    RollbackUnproven,
    PreviousUnavailable,
    Invalid,
}

fn classify(error: &CredentialInstallError) -> ProductionOutcome {
    match error {
        CredentialInstallError::RolledBack => ProductionOutcome::RolledBack,
        CredentialInstallError::RollbackUnproven => ProductionOutcome::RollbackUnproven,
        CredentialInstallError::PreviousUnavailable => ProductionOutcome::PreviousUnavailable,
        CredentialInstallError::Invalid(_) => ProductionOutcome::Invalid,
    }
}

/// The genuine production outcome must be `RolledBack`. `RollbackUnproven` is a
/// hard stop that retains the disposable root with an explicit reconciliation
/// block; an ACL prompt reaches here as a typed failure and also stops. An
/// outcome that is not `RolledBack` gets an explicit, verified cleanup attempt
/// (uncertainty route to the same block) before the test fails, so no panic ever
/// drives an unverified delete and no root is dropped ahead of it.
fn assert_rolled_back(
    error: &CredentialInstallError,
    owned: &mut OwnedScopedTarget,
    reconciliation: &Reconciliation,
    temp: &mut tempfile::TempDir,
) {
    match classify(error) {
        ProductionOutcome::RolledBack => {}
        ProductionOutcome::RollbackUnproven => {
            hard_stop(
                "rollback_unproven",
                reconciliation,
                Ownership::Fenced,
                temp,
                "",
            );
        }
        ProductionOutcome::PreviousUnavailable | ProductionOutcome::Invalid => {
            owned.cleanup("production_outcome_not_rolled_back", reconciliation, temp);
            panic!("the genuine production outcome was not RolledBack");
        }
    }
}

fn run_injected_install(
    scope: &JiraCredentialScope,
    alias: &str,
    wrapper: &ReadFailureAfterSet,
    reconciliation: &Reconciliation,
    temp: &mut tempfile::TempDir,
) -> CredentialInstallError {
    match install_credentials_with(scope, alias, synthetic_install_document(), wrapper) {
        Ok(()) => hard_stop(
            "injected_failure_missing",
            reconciliation,
            Ownership::Fenced,
            temp,
            "",
        ),
        Err(error) => error,
    }
}

/// Case 1: an owned, pre-existing scoped entry is restored byte-for-byte and
/// the production outcome is `RolledBack`.
#[test]
#[ignore = "macOS platform harness: writes the real login keychain through SystemKeychain; run deliberately with --ignored"]
fn an_owned_previous_scoped_entry_is_restored_byte_for_byte() {
    let mut temp = tempfile::tempdir().expect("the harness could not create its temporary root");
    let root = canonical_root(&temp);
    let realm = RealmId::generate();
    let alias = fresh_alias();
    let scope = JiraCredentialScope::at(&root, realm)
        .expect("a fresh absolute root must form a valid credential scope");
    let target = scoped_target(&root, realm, &alias);
    let reconciliation = Reconciliation::new(&root, realm, &target);
    let mut owned = OwnedScopedTarget::new(target.clone());
    owned.ensure_absent(&reconciliation, &mut temp);

    let previous = synthetic_previous_document();
    delegated(
        SystemKeychain.set_secret(&target, &previous),
        "the harness could not seed its owned synthetic entry",
        &reconciliation,
        &mut temp,
    );
    let seeded = delegated(
        SystemKeychain.secret(&target),
        "the harness could not read back its owned synthetic entry",
        &reconciliation,
        &mut temp,
    );
    confirm_secret(
        &seeded,
        &previous,
        "the seeded synthetic entry did not read back byte-for-byte",
        &reconciliation,
        &mut temp,
    );
    drop(seeded);

    let wrapper = ReadFailureAfterSet::default();
    let error = run_injected_install(&scope, &alias, &wrapper, &reconciliation, &mut temp);
    assert_rolled_back(&error, &mut owned, &reconciliation, &mut temp);
    if let Err(reason) = wrapper.fault_fidelity(&target, 2, 0) {
        owned.cleanup(reason, &reconciliation, &mut temp);
        panic!("{reason}");
    }

    let restored = delegated(
        SystemKeychain.secret(&target),
        "the production backend could not be read after rollback",
        &reconciliation,
        &mut temp,
    );
    confirm_secret(
        &restored,
        &previous,
        "the previous entry was not restored byte-for-byte",
        &reconciliation,
        &mut temp,
    );
    drop(restored);

    owned.cleanup("the previous-entry case", &reconciliation, &mut temp);
}

/// Case 2: a fresh scoped entry is deleted and absence is verified.
#[test]
#[ignore = "macOS platform harness: writes the real login keychain through SystemKeychain; run deliberately with --ignored"]
fn a_fresh_scoped_entry_is_deleted_after_rollback() {
    let mut temp = tempfile::tempdir().expect("the harness could not create its temporary root");
    let root = canonical_root(&temp);
    let realm = RealmId::generate();
    let alias = fresh_alias();
    let scope = JiraCredentialScope::at(&root, realm)
        .expect("a fresh absolute root must form a valid credential scope");
    let target = scoped_target(&root, realm, &alias);
    let reconciliation = Reconciliation::new(&root, realm, &target);
    let mut owned = OwnedScopedTarget::new(target.clone());
    owned.ensure_absent(&reconciliation, &mut temp);

    let wrapper = ReadFailureAfterSet::default();
    let error = run_injected_install(&scope, &alias, &wrapper, &reconciliation, &mut temp);
    assert_rolled_back(&error, &mut owned, &reconciliation, &mut temp);
    if let Err(reason) = wrapper.fault_fidelity(&target, 1, 1) {
        owned.cleanup(reason, &reconciliation, &mut temp);
        panic!("{reason}");
    }

    match SystemKeychain.secret(&target) {
        Err(KeychainFailure::NotFound) => {}
        _ => hard_stop(
            "fresh_entry_still_present",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        ),
    }

    owned.cleanup("the fresh-entry case", &reconciliation, &mut temp);
}

/// Case 3: the genuine production outcome is exactly `RolledBack` for both
/// directions, and the final real-backend state is restored or `NotFound`.
#[test]
#[ignore = "macOS platform harness: writes the real login keychain through SystemKeychain; run deliberately with --ignored"]
fn the_production_outcome_is_rolled_back_and_the_backend_state_is_final() {
    {
        let mut temp =
            tempfile::tempdir().expect("the harness could not create its temporary root");
        let root = canonical_root(&temp);
        let realm = RealmId::generate();
        let alias = fresh_alias();
        let scope = JiraCredentialScope::at(&root, realm)
            .expect("a fresh absolute root must form a valid credential scope");
        let target = scoped_target(&root, realm, &alias);
        let reconciliation = Reconciliation::new(&root, realm, &target);
        let mut owned = OwnedScopedTarget::new(target.clone());
        owned.ensure_absent(&reconciliation, &mut temp);

        let previous = synthetic_previous_document();
        delegated(
            SystemKeychain.set_secret(&target, &previous),
            "the harness could not seed its owned synthetic entry",
            &reconciliation,
            &mut temp,
        );
        let wrapper = ReadFailureAfterSet::default();
        let error = run_injected_install(&scope, &alias, &wrapper, &reconciliation, &mut temp);
        assert_rolled_back(&error, &mut owned, &reconciliation, &mut temp);
        if let Err(reason) = wrapper.fault_fidelity(&target, 2, 0) {
            owned.cleanup(reason, &reconciliation, &mut temp);
            panic!("{reason}");
        }
        let final_state = delegated(
            SystemKeychain.secret(&target),
            "the production backend could not be read after rollback",
            &reconciliation,
            &mut temp,
        );
        confirm_secret(
            &final_state,
            &previous,
            "the real backend did not end restored to the previous entry",
            &reconciliation,
            &mut temp,
        );
        drop(final_state);
        owned.cleanup(
            "the production-outcome previous-entry case",
            &reconciliation,
            &mut temp,
        );
    }

    {
        let mut temp =
            tempfile::tempdir().expect("the harness could not create its temporary root");
        let root = canonical_root(&temp);
        let realm = RealmId::generate();
        let alias = fresh_alias();
        let scope = JiraCredentialScope::at(&root, realm)
            .expect("a fresh absolute root must form a valid credential scope");
        let target = scoped_target(&root, realm, &alias);
        let reconciliation = Reconciliation::new(&root, realm, &target);
        let mut owned = OwnedScopedTarget::new(target.clone());
        owned.ensure_absent(&reconciliation, &mut temp);

        let wrapper = ReadFailureAfterSet::default();
        let error = run_injected_install(&scope, &alias, &wrapper, &reconciliation, &mut temp);
        assert_rolled_back(&error, &mut owned, &reconciliation, &mut temp);
        if let Err(reason) = wrapper.fault_fidelity(&target, 1, 1) {
            owned.cleanup(reason, &reconciliation, &mut temp);
            panic!("{reason}");
        }
        match SystemKeychain.secret(&target) {
            Err(KeychainFailure::NotFound) => {}
            _ => hard_stop(
                "final_state_still_present",
                &reconciliation,
                Ownership::Fenced,
                &mut temp,
                "",
            ),
        }
        owned.cleanup(
            "the production-outcome fresh-entry case",
            &reconciliation,
            &mut temp,
        );
    }
}

/// Presence-only check of every proxy key the production HTTP stack can read.
/// The value is never bound, used or printed; a hit is a hard stop before any
/// credential effect, so a proxy can never carry a credential off loopback.
fn assert_no_proxy_environment() {
    for key in PROXY_ENVIRONMENT_KEYS {
        if std::env::var_os(key).is_some() {
            panic!(
                "the harness refuses to run with `{key}` present: a proxy could take a credential-bearing request off loopback"
            );
        }
    }
}

fn daemon_binary() -> PathBuf {
    let Some(path) = std::env::var_os(DAEMON_BINARY_ENV) else {
        panic!(
            "set `{DAEMON_BINARY_ENV}` to the absolute path of the daemon binary built at the qualified revision"
        );
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        panic!("`{DAEMON_BINARY_ENV}` must name an existing absolute file");
    }
    path
}

/// Materialize a disposable Realm through the built daemon's own operator path.
///
/// `export` opens the state-root database with the production store
/// (creating and migrating a fresh file when absent), which is the only binary
/// path that initializes a Realm without serving it. The export document is
/// also the harness's non-secret source for the fresh realm id; no credential
/// exists at this point.
fn initialize_disposable_realm(daemon: &Path, root: &Path) -> RealmId {
    let export = root.join(REALM_EXPORT_FILE);
    let status = Command::new(daemon)
        .arg("--state-root")
        .arg(root)
        .arg("export")
        .arg("--out")
        .arg(&export)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("the built daemon could not be started to initialize the disposable Realm");
    if !status.success() {
        panic!(
            "the built daemon could not initialize a disposable Realm under a fresh absolute root"
        );
    }
    let bytes = std::fs::read(&export).expect("the disposable Realm export could not be read");
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the disposable Realm export is not JSON");
    let realm = document
        .get("source_realm_id")
        .and_then(serde_json::Value::as_str)
        .expect("the disposable Realm export does not carry its realm id");
    RealmId::parse(realm).expect("the disposable Realm export carries a non-canonical realm id")
}

/// Only non-secret configuration: endpoint, project identity and alias.
fn write_jira_config(root: &Path, endpoint: &str, project: ProjectId, alias: &str) {
    let document = serde_json::json!({
        "schema_version": 1,
        "projects": [{
            "project_id": project,
            "endpoint": endpoint,
            "project_key": PROJECT_KEY,
            "credential_alias": alias,
        }],
    });
    std::fs::write(
        root.join("jira.json"),
        serde_json::to_vec(&document).expect("the synthetic jira.json could not be encoded"),
    )
    .expect("the synthetic jira.json could not be written inside the temporary root");
}

/// Drain a child pipe fully so it can never deadlock, keeping at most `limit`
/// bytes for the redacted diagnostic.
fn drain_bounded(reader: &mut impl Read, limit: usize) -> Vec<u8> {
    let mut stored = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if stored.len() < limit {
                    let room = limit - stored.len();
                    stored.extend_from_slice(&chunk[..room.min(read)]);
                }
            }
            Err(_) => break,
        }
    }
    stored
}

/// Keep only a bounded, redacted excerpt: the synthetic email and token are
/// replaced before anything is returned, control characters are stripped, and
/// the result is length-capped. The raw capture is zeroized.
fn redact_diagnostic(raw: &mut Vec<u8>, email: &SecretString, token: &SecretString) -> String {
    let text = Zeroizing::new(String::from_utf8_lossy(raw).into_owned());
    raw.zeroize();
    let email_redacted = Zeroizing::new(text.replace(email.expose_secret(), "[redacted]"));
    let token_redacted =
        Zeroizing::new(email_redacted.replace(token.expose_secret(), "[redacted]"));
    let cleaned: String = token_redacted
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    cleaned.chars().take(DIAGNOSTIC_EXCERPT_CHARS).collect()
}

/// What the harness can prove about a child it had to terminate before any
/// credential delivery. Only a successful `wait` establishes reaping; a failed
/// `wait` leaves the OS process state unknown and must not be overstated.
#[derive(Clone, Copy)]
enum ReapState {
    /// `wait` returned, so the child is no longer running and is reaped.
    Reaped,
    /// `wait` failed; the process status could not be established.
    Unknown,
}

/// The child's outcome is either a proven exit or an explicit unknown: spawn,
/// stdin-handle and wait failures must never become panics, because unwinding
/// would drop the disposable root without the retained reconciliation block.
/// The `StdinUnavailable` state carries the non-secret child PID and the
/// proven-or-unknown reap state, so the caller never claims cleanup it did not
/// establish.
#[derive(Clone, Copy)]
enum InstallOutcome {
    Exited(ExitStatus),
    WaitFailed,
    SpawnFailed,
    StdinUnavailable { pid: u32, reap: ReapState },
}

struct InstallAttempt {
    outcome: InstallOutcome,
    delivered: bool,
    diagnostic: String,
}

/// Run the built daemon's stopped-Realm install with the credential document
/// delivered through an anonymous stdin pipe. The pipe buffer is zeroized; the
/// credential never reaches argv, the environment or a file. Only a bounded,
/// redacted stderr excerpt is returned for the failure path.
///
/// Nothing before or after delivery unwinds: spawning, taking the stdin handle,
/// taking the stderr pipe and waiting are explicit `Option`/`Result` handling,
/// never `expect`. A child spawned without a usable stdin handle is terminated
/// before returning; a successful `wait` proves it is reaped, while a failed
/// `wait` is reported as process status UNKNOWN with the child PID rather than
/// claimed as cleanup. Zero credential bytes were delivered on that path.
fn run_daemon_install(
    daemon: &Path,
    root: &Path,
    alias: &str,
    email: &SecretString,
    token: &SecretString,
) -> InstallAttempt {
    let mut payload = Zeroizing::new(
        serde_json::to_vec(&CredentialDocument {
            email: email.expose_secret(),
            api_token: token.expose_secret(),
        })
        .expect("the synthetic credential document could not be encoded"),
    );
    let mut child = match Command::new(daemon)
        .arg("--state-root")
        .arg(root)
        .arg("install-jira-credential")
        .arg("--alias")
        .arg(alias)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => {
            payload.zeroize();
            return InstallAttempt {
                outcome: InstallOutcome::SpawnFailed,
                delivered: false,
                diagnostic: String::new(),
            };
        }
    };
    let Some(mut stdin) = child.stdin.take() else {
        let pid = child.id();
        // The kill request is best-effort evidence only: the `wait` below is
        // what establishes that the child is no longer running and is reaped.
        let kill_request = child.kill();
        let reap = match child.wait() {
            Ok(_) => ReapState::Reaped,
            Err(_) => ReapState::Unknown,
        };
        drop(kill_request);
        payload.zeroize();
        return InstallAttempt {
            outcome: InstallOutcome::StdinUnavailable { pid, reap },
            delivered: false,
            diagnostic: String::new(),
        };
    };
    let delivery = stdin.write_all(payload.as_slice());
    drop(stdin);
    let mut raw = match child.stderr.take() {
        Some(mut stderr) => drain_bounded(&mut stderr, DIAGNOSTIC_LIMIT_BYTES),
        None => Vec::new(),
    };
    let outcome = match child.wait() {
        Ok(status) => InstallOutcome::Exited(status),
        Err(_) => InstallOutcome::WaitFailed,
    };
    payload.zeroize();
    let diagnostic = redact_diagnostic(&mut raw, email, token);
    InstallAttempt {
        outcome,
        delivered: delivery.is_ok(),
        diagnostic,
    }
}

/// One request as the loopback responder records it: a path and two bits.
/// Raw headers, and specifically any Authorization value, are never stored.
struct RecordedRequest {
    path: String,
    authorization_present: bool,
    authorization_matches: bool,
}

#[derive(Default)]
struct LocalEndpoint {
    recorded: Mutex<Vec<RecordedRequest>>,
    shutdown: AtomicBool,
}

/// Owns the responder thread; stopping is explicit and bounded.
struct EndpointGuard {
    state: Arc<LocalEndpoint>,
    handle: Option<JoinHandle<()>>,
}

impl EndpointGuard {
    /// (path, authorization present, authorization matched the expected bytes).
    fn requests(&self) -> Vec<(String, bool, bool)> {
        self.state
            .recorded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|request| {
                (
                    request.path.clone(),
                    request.authorization_present,
                    request.authorization_matches,
                )
            })
            .collect()
    }
}

impl Drop for EndpointGuard {
    fn drop(&mut self) {
        self.state.shutdown.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Serve the loopback-only responder on an already-bound listener. The expected
/// Authorization bytes are compared in place and zeroized on drop; only the
/// boolean outcome is recorded.
fn start_local_endpoint(
    listener: TcpListener,
    expected_authorization: Zeroizing<Vec<u8>>,
) -> EndpointGuard {
    listener
        .set_nonblocking(true)
        .expect("the local responder listener could not be set non-blocking");
    let state = Arc::new(LocalEndpoint::default());
    let thread_state = Arc::clone(&state);
    let handle = std::thread::spawn(move || {
        let expected = expected_authorization;
        loop {
            if thread_state.shutdown.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok((stream, _peer)) => serve_connection(stream, expected.as_slice(), &thread_state),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });
    EndpointGuard {
        state,
        handle: Some(handle),
    }
}

fn serve_connection(mut stream: TcpStream, expected: &[u8], state: &LocalEndpoint) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut head = Vec::new();
    let mut chunk = [0u8; 1024];
    while head.len() < REQUEST_HEAD_LIMIT_BYTES {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                head.extend_from_slice(&chunk[..read]);
                if head.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let (path, authorization_present, authorization_matches) =
        parse_request_head(&head, expected).unwrap_or_default();
    state
        .recorded
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(RecordedRequest {
            path: path.clone(),
            authorization_present,
            authorization_matches,
        });
    let (status_line, body) = match path.as_str() {
        COMMENTS_PATH => ("HTTP/1.1 200 OK", COMMENTS_BODY),
        SHARED_READER_REFERENCE_PATH => ("HTTP/1.1 200 OK", REFERENCE_BODY),
        _ => ("HTTP/1.1 404 Not Found", REFERENCE_BODY),
    };
    let response = format!(
        "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Parse only the path and the Authorization match bit from a request head.
/// The transient header bytes are compared to `expected` and dropped; they are
/// never stored or formatted.
fn parse_request_head(head: &[u8], expected: &[u8]) -> Option<(String, bool, bool)> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let target = request_line.split_whitespace().nth(1)?;
    let path = target.split('?').next().unwrap_or(target).to_owned();
    let mut authorization_present = false;
    let mut authorization_matches = false;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("authorization")
        {
            authorization_present = true;
            authorization_matches = value.trim().as_bytes() == expected;
        }
    }
    Some((path, authorization_present, authorization_matches))
}

/// Build the exact Authorization value the production reader must present,
/// using the same reqwest encoder the connector uses, from a reference client
/// that explicitly disables proxies. Nothing is sent here; later the reference
/// request proves the encoder is live against the same responder.
fn expected_authorization(
    reference_url: &str,
    email: &SecretString,
    token: &SecretString,
) -> Zeroizing<Vec<u8>> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the session reference client must be constructible without proxies");
    let request = client
        .get(reference_url)
        .basic_auth(email.expose_secret(), Some(token.expose_secret()))
        .build()
        .expect("the session reference request must be constructible");
    let value = request
        .headers()
        .get(reqwest::header::AUTHORIZATION)
        .expect("the session reference request must carry an authorization header");
    Zeroizing::new(value.as_bytes().to_vec())
}

/// Read through one production reader, routing every failure to the same
/// retained-context hard stop instead of a panic-driven cleanup.
async fn comments_or_stop(
    connectors: &JiraConnectors,
    project: ProjectId,
    issue: &ExternalId,
    reconciliation: &Reconciliation,
    temp: &mut tempfile::TempDir,
) {
    let Some(connector) = connectors.for_project(project) else {
        hard_stop(
            "reader_project_missing",
            reconciliation,
            Ownership::Fenced,
            temp,
            "",
        );
    };
    if connector.comments(issue).await.is_err() {
        hard_stop(
            "reader_request_failed",
            reconciliation,
            Ownership::Fenced,
            temp,
            "",
        );
    }
}

/// Case 4: the built daemon binary's happy path installs a fresh scoped
/// credential, and two independent production `JiraConnectors` readers present
/// that same credential to a loopback-only responder. This case shares no fault
/// injection with the deterministic cases above: it is evidence for the real
/// end-to-end path only. Every unsuccessful or unproven child outcome is
/// preserved, never deleted, because the exit status does not prove a verified
/// rollback.
#[tokio::test]
#[ignore = "macOS platform harness: writes the real login keychain and runs the built daemon; set KONTOR_ASMA8015_DAEMON_BIN, unset proxy variables, and run deliberately with --ignored"]
async fn the_built_daemon_installs_and_the_shared_production_readers_present_the_installed_credential()
 {
    assert_no_proxy_environment();
    let daemon = daemon_binary();
    let mut temp = tempfile::tempdir().expect("the harness could not create its temporary root");
    let root = canonical_root(&temp);

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .expect("the local responder could not bind explicit loopback");
    let address = listener
        .local_addr()
        .expect("the local responder address must be readable");
    let endpoint = format!("http://{address}");
    let parsed = url::Url::parse(&endpoint).expect("the local responder endpoint must parse");
    if parsed.scheme() != "http" || parsed.host_str() != Some("127.0.0.1") {
        panic!("the local responder endpoint must be explicit loopback HTTP");
    }
    let reference_url = format!("{endpoint}{SHARED_READER_REFERENCE_PATH}");

    let realm = initialize_disposable_realm(&daemon, &root);
    let issue = ExternalId::parse(ISSUE_KEY).expect("the harness issue key is canonical");

    let email = fresh_email();
    let token = fresh_token();
    let expected = credential_document(&email, &token);
    let expected_header = expected_authorization(&reference_url, &email, &token);
    let endpoint_guard = start_local_endpoint(listener, expected_header);
    let reference_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the session reference client must be constructible without proxies");

    let project = ProjectId::generate();
    let alias = fresh_alias();
    write_jira_config(&root, &endpoint, project, &alias);

    let target = scoped_target(&root, realm, &alias);
    let reconciliation = Reconciliation::new(&root, realm, &target);
    let mut owned = OwnedScopedTarget::new(target.clone());
    owned.ensure_absent(&reconciliation, &mut temp);

    let attempt = run_daemon_install(&daemon, &root, &alias, &email, &token);
    let proven_success = attempt.delivered
        && matches!(attempt.outcome, InstallOutcome::Exited(status) if status.success());
    if !proven_success {
        let (reason, detail) = match attempt.outcome {
            InstallOutcome::SpawnFailed => (
                "daemon_install_spawn_failed",
                "zero credential delivery; no delete or cleanup attempt; state UNKNOWN".to_owned(),
            ),
            InstallOutcome::StdinUnavailable { pid, reap } => (
                "daemon_install_stdin_unavailable",
                match reap {
                    ReapState::Reaped => format!(
                        "child pid {pid} reaped (wait succeeded); zero credential delivery; no delete or cleanup attempt; state UNKNOWN"
                    ),
                    ReapState::Unknown => format!(
                        "child pid {pid} process status UNKNOWN (wait failed); zero credential delivery; no delete or cleanup attempt; operator reconciliation required; state UNKNOWN"
                    ),
                },
            ),
            InstallOutcome::WaitFailed => {
                ("daemon_install_wait_uncertain", attempt.diagnostic.clone())
            }
            InstallOutcome::Exited(_) => ("daemon_install_failed", attempt.diagnostic.clone()),
        };
        hard_stop(
            reason,
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            &detail,
        );
    }

    let stored = match SystemKeychain.secret(&target) {
        Ok(secret) => secret,
        Err(_) => hard_stop(
            "daemon_install_verification_uncertain",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            &attempt.diagnostic,
        ),
    };
    if stored.expose_secret() != expected.as_str() {
        drop(stored);
        hard_stop(
            "daemon_install_readback_mismatch",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            &attempt.diagnostic,
        );
    }
    drop(stored);

    let first = match JiraConnectors::read(&root, realm) {
        Ok(connectors) => connectors,
        Err(_) => hard_stop(
            "reader_load_failed",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        ),
    };
    comments_or_stop(&first, project, &issue, &reconciliation, &mut temp).await;
    let second = match JiraConnectors::read(&root, realm) {
        Ok(connectors) => connectors,
        Err(_) => hard_stop(
            "reader_load_failed",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        ),
    };
    comments_or_stop(&second, project, &issue, &reconciliation, &mut temp).await;

    let reference = match reference_client
        .get(&reference_url)
        .basic_auth(email.expose_secret(), Some(token.expose_secret()))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => hard_stop(
            "reference_request_failed",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        ),
    };
    if !reference.status().is_success() {
        hard_stop(
            "reference_request_refused",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        );
    }

    let requests = endpoint_guard.requests();
    let mut comment_requests = 0_usize;
    let mut reference_requests = 0_usize;
    let mut all_matched = true;
    for (path, present, matches) in &requests {
        match path.as_str() {
            COMMENTS_PATH => {
                comment_requests += 1;
                all_matched &= *present && *matches;
            }
            SHARED_READER_REFERENCE_PATH => {
                reference_requests += 1;
                all_matched &= *present && *matches;
            }
            _ => all_matched = false,
        }
    }
    if comment_requests != 2 || reference_requests != 1 || !all_matched {
        hard_stop(
            "shared_reader_credential_mismatch",
            &reconciliation,
            Ownership::Fenced,
            &mut temp,
            "",
        );
    }

    owned.cleanup("the built-daemon case", &reconciliation, &mut temp);
}
