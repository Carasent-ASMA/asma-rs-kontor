//! Approved credential references, and the short-lived environment they resolve
//! into.
//!
//! # Why a profile cannot name a path
//!
//! The obvious design is to let a profile store the directory or keychain entry
//! its credentials live in. That makes the profile table a place where writing a
//! row is equivalent to reading an arbitrary file, and it makes `..`, symlink
//! traversal and prefix confusion into live attack surface reachable from
//! whatever creates profiles.
//!
//! So a profile stores an *alias*, and a [`ResolverPolicy`] — built in memory at
//! composition time from trusted local operator configuration — is the only
//! thing that maps an alias to something real. Profile input never participates
//! in path joining: the policy canonicalizes each approved directory once, at
//! build time, and resolution is an exact lookup of an already-canonical path.
//! There is no join, so there is nothing for `..` to escape.
//!
//! # Why the resolved value has no serialized form
//!
//! [`ResolvedAccountEnvironment`] has private fields, no `Serialize`, and
//! `Debug`/`Display` that print variable *names* and a redaction marker. The
//! single exit is [`ResolvedAccountEnvironment::apply`], which writes into a
//! [`std::process::Command`]'s environment block. That is a deliberate choice of
//! mechanism, not a convenience: `std::env::set_var` would make the value
//! process-global and visible to every other resolution, a command flag or a
//! prompt would put it in argv where any local process can read it, and a shell
//! fragment would make quoting a security property.
//!
//! # Why errors carry codes
//!
//! A keychain error's `Display` can name the service and account it failed on; a
//! filesystem error's can contain the path. Both identify a user. Every failure
//! below is therefore mapped to a [`ResolutionReason`] *at the boundary*, and no
//! source error is ever wrapped or chained.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use kontor_core::id::{
    AccountProfileId, AggregateRevision, ContentHash, CredentialAlias, EnvironmentVariableName,
    RuntimeKindKey,
};
use kontor_core::repository::{AccountProfile, CredentialReference, CredentialReferenceKind};
use secrecy::{ExposeSecret, SecretString};

use crate::profile::AccountEnvironmentMap;

/// The marker every redacted rendering prints instead of a value.
const REDACTED: &str = "<redacted>";

// ---------------------------------------------------------------------------
// Keychain port
// ---------------------------------------------------------------------------

/// What a keychain lookup needs, and what a profile is never allowed to supply.
///
/// A service/account pair frequently *is* a user identifier, so this type has no
/// `Serialize`, no `Display` and a `Debug` that prints nothing but its own name.
/// It exists only inside a [`ResolverPolicy`].
#[derive(Clone, PartialEq, Eq)]
pub struct KeychainTarget {
    service: String,
    account: String,
}

impl KeychainTarget {
    /// Name one keychain entry. Called by composition code, never by a profile.
    #[must_use]
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }

    /// The service, for a backend that is about to perform the lookup.
    #[must_use]
    pub fn service(&self) -> &str {
        &self.service
    }

    /// The account, for a backend that is about to perform the lookup.
    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }
}

impl fmt::Debug for KeychainTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeychainTarget({REDACTED})")
    }
}

/// Why a keychain lookup did not produce a secret.
///
/// A closed set of codes, so a backend cannot smuggle a value, a target or a
/// source error's text out through its failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeychainFailure {
    /// The entry does not exist.
    NotFound,
    /// The keychain refused access.
    Denied,
    /// The keychain could not be reached or is not configured.
    Unavailable,
}

/// The narrow port every keychain lookup goes through.
///
/// Implementations must map their own errors to [`KeychainFailure`] rather than
/// returning or wrapping them: a `keyring` error's text can name the service and
/// the account it failed on.
pub trait KeychainBackend: Send + Sync {
    /// Read one entry.
    ///
    /// # Errors
    /// Returns a [`KeychainFailure`] code and nothing else.
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure>;
}

/// The production backend: the OS keychain, through the pinned `keyring` crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeychain;

impl KeychainBackend for SystemKeychain {
    fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
        // Both `?`-free branches below drop the `keyring` error deliberately:
        // its `Display` can contain the service and account it failed on, which
        // is exactly the identifying detail this crate refuses to surface.
        let entry = keyring::Entry::new(target.service(), target.account())
            .map_err(|_| KeychainFailure::Unavailable)?;
        match entry.get_password() {
            Ok(secret) => Ok(SecretString::from(secret)),
            Err(keyring::Error::NoEntry) => Err(KeychainFailure::NotFound),
            Err(_) => Err(KeychainFailure::Unavailable),
        }
    }
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Why a policy could not be built.
///
/// Carries the alias — an operator-chosen name, not a secret — and never the
/// path that failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PolicyError {
    /// The approved directory does not exist, is not a directory, or cannot be
    /// canonicalized.
    #[error("approved config home `{alias}` is not a readable directory")]
    UnusableConfigHome {
        /// The alias that was being approved.
        alias: CredentialAlias,
    },
    /// The same alias was approved twice.
    #[error("credential alias `{alias}` is approved more than once")]
    DuplicateAlias {
        /// The alias that repeated.
        alias: CredentialAlias,
    },
}

/// What an operator has approved, fixed for the life of the process.
///
/// Built once at daemon/runtime composition from trusted local configuration and
/// OS-keychain registration; tests construct one directly. It is never written
/// to SQLite, never exported and never derived from profile input.
#[derive(Clone, PartialEq, Eq)]
pub struct ResolverPolicy {
    harnesses: BTreeSet<RuntimeKindKey>,
    config_homes: BTreeMap<CredentialAlias, PathBuf>,
    keychain: BTreeMap<CredentialAlias, KeychainTarget>,
    environment: BTreeSet<EnvironmentVariableName>,
}

/// Render a set of approvals: the names, never what they resolve to.
///
/// A policy and a policy under construction hold the same four collections and
/// must print the same way, so there is one function rather than two `Debug`
/// bodies. That is the point: two bodies is how a redaction gets written
/// correctly in one place and forgotten in the other, and how a new field gets
/// added to one and not the other.
///
/// The alias, harness and variable names are operator-chosen labels and are safe
/// to print. The paths and keychain targets they map to are not printed at any
/// verbosity.
fn fmt_approvals(
    type_name: &'static str,
    harnesses: &BTreeSet<RuntimeKindKey>,
    config_homes: &BTreeMap<CredentialAlias, PathBuf>,
    keychain: &BTreeMap<CredentialAlias, KeychainTarget>,
    environment: &BTreeSet<EnvironmentVariableName>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    f.debug_struct(type_name)
        .field("harnesses", harnesses)
        .field(
            "config_home_aliases",
            &config_homes.keys().collect::<Vec<_>>(),
        )
        .field("keychain_aliases", &keychain.keys().collect::<Vec<_>>())
        .field("environment", environment)
        .field("targets", &REDACTED)
        .finish()
}

impl fmt::Debug for ResolverPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_approvals(
            "ResolverPolicy",
            &self.harnesses,
            &self.config_homes,
            &self.keychain,
            &self.environment,
            f,
        )
    }
}

impl ResolverPolicy {
    /// Start building a policy.
    #[must_use]
    pub fn builder() -> ResolverPolicyBuilder {
        ResolverPolicyBuilder::default()
    }

    /// Whether this policy approves `harness`.
    #[must_use]
    pub fn approves_harness(&self, harness: &RuntimeKindKey) -> bool {
        self.harnesses.contains(harness)
    }

    /// Whether this policy approves `reference`.
    #[must_use]
    pub fn approves(&self, reference: &CredentialReference) -> bool {
        match reference.kind {
            CredentialReferenceKind::ConfigHome => self.config_homes.contains_key(&reference.alias),
            CredentialReferenceKind::Keychain => self.keychain.contains_key(&reference.alias),
        }
    }

    /// Whether this policy approves filling `name`.
    #[must_use]
    pub fn approves_environment(&self, name: &EnvironmentVariableName) -> bool {
        self.environment.contains(name)
    }

    /// A digest over the policy's *non-secret* metadata, for a launch receipt.
    ///
    /// Deliberately computed over the approved names only — harnesses, aliases
    /// with their kinds, and variable names. The paths and keychain targets are
    /// not hashed, because a digest of a secret is still a fact about that
    /// secret, and a receipt is a durable, exportable artefact.
    #[must_use]
    pub fn evidence(&self) -> ContentHash {
        let mut material = String::new();
        for harness in &self.harnesses {
            material.push_str("harness:");
            material.push_str(harness.as_str());
            material.push('\n');
        }
        for alias in self.config_homes.keys() {
            material.push_str("config_home:");
            material.push_str(alias.as_str());
            material.push('\n');
        }
        for alias in self.keychain.keys() {
            material.push_str("keychain:");
            material.push_str(alias.as_str());
            material.push('\n');
        }
        for name in &self.environment {
            material.push_str("env:");
            material.push_str(name.as_str());
            material.push('\n');
        }
        ContentHash::of(material.as_bytes())
    }
}

/// Accumulates approvals into an immutable [`ResolverPolicy`].
///
/// A half-built policy holds exactly the same targets a finished one does, so it
/// gets exactly the same redaction. `Debug` is **not** derived here: a derived
/// one prints `config_homes` as raw [`PathBuf`]s and would hand out every
/// approved credential home to anything that formatted the builder — the
/// finished [`ResolverPolicy`] would still be redacted, and the leak would sit
/// one step upstream of the type everyone thinks to check.
#[derive(Default)]
pub struct ResolverPolicyBuilder {
    harnesses: BTreeSet<RuntimeKindKey>,
    config_homes: BTreeMap<CredentialAlias, PathBuf>,
    keychain: BTreeMap<CredentialAlias, KeychainTarget>,
    environment: BTreeSet<EnvironmentVariableName>,
}

impl fmt::Debug for ResolverPolicyBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_approvals(
            "ResolverPolicyBuilder",
            &self.harnesses,
            &self.config_homes,
            &self.keychain,
            &self.environment,
            f,
        )
    }
}

impl ResolverPolicyBuilder {
    /// Approve one runtime family.
    #[must_use]
    pub fn harness(mut self, harness: RuntimeKindKey) -> Self {
        self.harnesses.insert(harness);
        self
    }

    /// Approve one configuration home.
    ///
    /// The directory is canonicalized *here*, once, and the resulting absolute
    /// path is what resolution returns. Because profile input never joins onto
    /// it, `..`, a symlinked component and a shared prefix are all resolved at
    /// approval time rather than left as a resolution-time surface.
    ///
    /// # Errors
    /// Returns [`PolicyError::UnusableConfigHome`] when the path does not
    /// canonicalize to an existing directory, and
    /// [`PolicyError::DuplicateAlias`] when the alias is already approved.
    pub fn config_home(mut self, alias: CredentialAlias, path: &Path) -> Result<Self, PolicyError> {
        if self.config_homes.contains_key(&alias) {
            return Err(PolicyError::DuplicateAlias { alias });
        }
        let canonical = std::fs::canonicalize(path)
            .ok()
            .filter(|resolved| resolved.is_dir())
            .ok_or_else(|| PolicyError::UnusableConfigHome {
                alias: alias.clone(),
            })?;
        self.config_homes.insert(alias, canonical);
        Ok(self)
    }

    /// Approve one keychain entry.
    ///
    /// # Errors
    /// Returns [`PolicyError::DuplicateAlias`] when the alias is already
    /// approved.
    pub fn keychain(
        mut self,
        alias: CredentialAlias,
        target: KeychainTarget,
    ) -> Result<Self, PolicyError> {
        if self.keychain.contains_key(&alias) {
            return Err(PolicyError::DuplicateAlias { alias });
        }
        self.keychain.insert(alias, target);
        Ok(self)
    }

    /// Approve one environment variable name.
    #[must_use]
    pub fn environment(mut self, name: EnvironmentVariableName) -> Self {
        self.environment.insert(name);
        self
    }

    /// Freeze the approvals.
    #[must_use]
    pub fn build(self) -> ResolverPolicy {
        ResolverPolicy {
            harnesses: self.harnesses,
            config_homes: self.config_homes,
            keychain: self.keychain,
            environment: self.environment,
        }
    }
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// Why a resolution was refused. A closed set of codes and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResolutionReason {
    /// The policy does not approve this profile's harness.
    HarnessNotApproved,
    /// The policy does not approve the reference the profile names.
    ReferenceNotApproved,
    /// The policy does not approve filling one of the variables the profile
    /// names.
    EnvironmentNameNotApproved,
    /// The stored environment map is not readable as one.
    EnvironmentMapUnreadable,
    /// The keychain did not produce the secret.
    Keychain(KeychainFailure),
}

impl fmt::Display for ResolutionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HarnessNotApproved => f.write_str("the harness is not approved"),
            Self::ReferenceNotApproved => f.write_str("the credential reference is not approved"),
            Self::EnvironmentNameNotApproved => {
                f.write_str("an environment variable name is not approved")
            }
            Self::EnvironmentMapUnreadable => {
                f.write_str("the stored environment map is unreadable")
            }
            Self::Keychain(failure) => write!(f, "the keychain lookup failed: {failure:?}"),
        }
    }
}

/// One refused resolution.
///
/// Identifies the profile and — where one is involved — the reference kind and
/// alias, which are opaque operator-chosen names. It never carries a resolved
/// value, a path, a keychain target, file content or a child environment.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("account profile {profile_id} could not be resolved: {reason}")]
pub struct ResolutionError {
    /// The profile that was being resolved.
    pub profile_id: AccountProfileId,
    /// The reference that was refused, when the refusal is about one.
    pub reference: Option<CredentialReference>,
    /// Why.
    pub reason: ResolutionReason,
}

/// One account's credential material, alive only as long as this value is.
///
/// No `Serialize`, no public fields, redacted `Debug` and `Display`, and values
/// held in [`SecretString`] so they are zeroized on drop. The only exit is
/// [`ResolvedAccountEnvironment::apply`].
///
/// Resolution holds no global state, so two of these for different profiles are
/// completely independent — there is no "current account" for a concurrent
/// resolution to win or lose a race over.
pub struct ResolvedAccountEnvironment {
    profile_id: AccountProfileId,
    revision: AggregateRevision,
    entries: Vec<(EnvironmentVariableName, SecretString)>,
}

impl ResolvedAccountEnvironment {
    /// The profile this environment belongs to.
    #[must_use]
    pub const fn profile_id(&self) -> AccountProfileId {
        self.profile_id
    }

    /// The profile revision it was resolved at.
    ///
    /// A launch compares this against a fresh read before it authorizes
    /// anything, so a profile that was disabled during a slow lookup cannot
    /// have its stale resolution used.
    #[must_use]
    pub const fn revision(&self) -> AggregateRevision {
        self.revision
    }

    /// The variable names this environment fills — the non-secret half.
    #[must_use]
    pub fn names(&self) -> Vec<EnvironmentVariableName> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    /// How many variables it fills.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it fills nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Write the material into a child process's environment block.
    ///
    /// This is the whole exposed surface of a resolved credential, and it is
    /// [`Command::env`] rather than `std::env::set_var` on purpose: the value
    /// reaches exactly one child and never becomes visible to this process, to
    /// another concurrent resolution, or to anything reading `/proc`'s command
    /// line.
    pub fn apply(&self, command: &mut Command) {
        for (name, value) in &self.entries {
            command.env(name.as_str(), value.expose_secret());
        }
    }
}

impl fmt::Debug for ResolvedAccountEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedAccountEnvironment")
            .field("profile_id", &self.profile_id)
            .field("revision", &self.revision)
            .field("names", &self.names())
            .field("values", &REDACTED)
            .finish()
    }
}

impl fmt::Display for ResolvedAccountEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "account environment for {} ({} variables, {REDACTED})",
            self.profile_id,
            self.entries.len()
        )
    }
}

/// Resolves approved references for one policy and one keychain backend.
///
/// Holds no mutable state, so it is shared freely across threads and two
/// concurrent resolutions cannot observe each other.
pub struct AccountResolver<'policy> {
    policy: &'policy ResolverPolicy,
    keychain: &'policy (dyn KeychainBackend + 'policy),
}

impl fmt::Debug for AccountResolver<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountResolver")
            .field("policy", self.policy)
            .finish_non_exhaustive()
    }
}

impl<'policy> AccountResolver<'policy> {
    /// Bind a resolver to a policy and a backend.
    #[must_use]
    pub const fn new(
        policy: &'policy ResolverPolicy,
        keychain: &'policy (dyn KeychainBackend + 'policy),
    ) -> Self {
        Self { policy, keychain }
    }

    /// The policy this resolver enforces.
    #[must_use]
    pub const fn policy(&self) -> &'policy ResolverPolicy {
        self.policy
    }

    /// Check a profile against the policy without touching any backend.
    ///
    /// A launch calls this before it does anything expensive, so an unapproved
    /// profile is refused without a keychain prompt or a filesystem hit.
    ///
    /// # Errors
    /// Returns [`ResolutionError`] naming only the profile, the reference and a
    /// reason code.
    pub fn validate(
        &self,
        profile: &AccountProfile,
    ) -> Result<AccountEnvironmentMap, ResolutionError> {
        let refuse = |reason, reference: Option<&CredentialReference>| ResolutionError {
            profile_id: profile.id,
            reference: reference.cloned(),
            reason,
        };

        if !self.policy.approves_harness(&profile.harness) {
            return Err(refuse(ResolutionReason::HarnessNotApproved, None));
        }
        if !self.policy.approves(&profile.credential_ref) {
            return Err(refuse(
                ResolutionReason::ReferenceNotApproved,
                Some(&profile.credential_ref),
            ));
        }
        let map = AccountEnvironmentMap::from_document(&profile.environment)
            .map_err(|_| refuse(ResolutionReason::EnvironmentMapUnreadable, None))?;
        for (name, reference) in map.entries() {
            if !self.policy.approves_environment(name) {
                return Err(refuse(ResolutionReason::EnvironmentNameNotApproved, None));
            }
            if !self.policy.approves(reference) {
                return Err(refuse(
                    ResolutionReason::ReferenceNotApproved,
                    Some(reference),
                ));
            }
        }
        Ok(map)
    }

    /// Resolve a profile into a short-lived environment.
    ///
    /// Every alias is validated against the policy *first*, so an unapproved
    /// reference is refused before a backend is asked anything.
    ///
    /// # Errors
    /// As [`AccountResolver::validate`], plus a [`ResolutionReason::Keychain`]
    /// code when a lookup fails.
    pub fn resolve(
        &self,
        profile: &AccountProfile,
    ) -> Result<ResolvedAccountEnvironment, ResolutionError> {
        let map = self.validate(profile)?;
        let mut entries = Vec::with_capacity(map.names().len());
        for (name, reference) in map.entries() {
            let value = match reference.kind {
                // The approved home is handed to the child as-is. This resolver
                // never opens, reads or copies the credential files inside it:
                // the coding client remains the only reader, which keeps auth
                // file contents out of this process entirely.
                CredentialReferenceKind::ConfigHome => {
                    let home = self
                        .policy
                        .config_homes
                        .get(&reference.alias)
                        .ok_or_else(|| ResolutionError {
                            profile_id: profile.id,
                            reference: Some(reference.clone()),
                            reason: ResolutionReason::ReferenceNotApproved,
                        })?;
                    SecretString::from(home.to_string_lossy().into_owned())
                }
                CredentialReferenceKind::Keychain => {
                    let target = self.policy.keychain.get(&reference.alias).ok_or_else(|| {
                        ResolutionError {
                            profile_id: profile.id,
                            reference: Some(reference.clone()),
                            reason: ResolutionReason::ReferenceNotApproved,
                        }
                    })?;
                    self.keychain
                        .secret(target)
                        .map_err(|failure| ResolutionError {
                            profile_id: profile.id,
                            reference: Some(reference.clone()),
                            reason: ResolutionReason::Keychain(failure),
                        })?
                }
            };
            entries.push((name.clone(), value));
        }
        Ok(ResolvedAccountEnvironment {
            profile_id: profile.id,
            revision: profile.revision,
            entries,
        })
    }
}
