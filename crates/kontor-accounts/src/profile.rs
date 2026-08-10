//! The validated account profile and its project-scoped CRUD service.
//!
//! [`kontor_core::repository::AccountProfile`] *is* the public projection. There
//! is deliberately no second, richer "internal" shape that a doctor, list, API
//! or export view is derived from, because a second shape is where a resolver
//! target eventually gets added by accident. Everything the store holds is
//! non-secret by construction, so the safe projection and the stored record can
//! be the same value.
//!
//! The service is thin on purpose: the compare-and-swap, the referential
//! refusal and the credential-identity freeze are enforced by the repository and
//! the schema. What lives here is the validation that has to happen *before* a
//! row exists — that the environment map names only approved variables and
//! well-formed references, and that an identity change is refused rather than
//! translated into an update.

use std::collections::BTreeMap;

use kontor_core::DomainError;
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CanonicalDocument, CredentialAlias,
    EnvironmentVariableName, ExternalId, ExternalName, ProjectId, RuntimeKindKey, Timestamp,
};
use kontor_core::repository::{
    AccountProfile, AccountProfileUpdate, CredentialReference, CredentialReferenceKind,
    NewAccountProfile, ProjectRepository, RepositoryError,
};
use serde::{Deserialize, Serialize};

/// The `schema_version` every environment-reference document carries.
pub const ENVIRONMENT_SCHEMA: u32 = 1;

/// Everything the account service can refuse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AccountError {
    /// The domain rejected a value before any row was written.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// The repository refused.
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    /// A credential-affecting field was asked to change on an existing profile.
    ///
    /// Rotation is a new profile id, so a queued, active or historical run's pin
    /// cannot start meaning a different account.
    #[error("account profile {subject} is immutable: rotate by creating a new profile")]
    ImmutableIdentity {
        /// Which field was asked to change.
        subject: &'static str,
    },
}

/// One environment variable name and the approved reference that fills it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EnvironmentEntry {
    name: EnvironmentVariableName,
    kind: CredentialReferenceKind,
    alias: CredentialAlias,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EnvironmentDocument {
    schema_version: u32,
    variables: Vec<EnvironmentEntry>,
}

/// Environment variable *names* mapped to opaque approved references.
///
/// The values those variables will carry are not in this type and have no
/// persisted representation anywhere. That is the whole point: the map is the
/// non-secret half of a credential delivery, and it is the half that is safe to
/// store, list, hash into a receipt and export.
///
/// The document form is an array of `{name, kind, alias}` objects rather than an
/// object keyed by variable name, so that a variable legitimately called
/// `API_KEY` is a *value* in a `name` field instead of a JSON key — which
/// [`kontor_core::id::reject_sensitive_material`] would refuse outright.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountEnvironmentMap {
    entries: BTreeMap<EnvironmentVariableName, CredentialReference>,
}

impl AccountEnvironmentMap {
    /// An empty map. A profile may legitimately deliver nothing through the
    /// environment — a config-home harness reads its own credentials.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one mapping, replacing any earlier one for the same variable.
    #[must_use]
    pub fn with(mut self, name: EnvironmentVariableName, reference: CredentialReference) -> Self {
        self.entries.insert(name, reference);
        self
    }

    /// The mappings, in variable-name order.
    pub fn entries(
        &self,
    ) -> impl Iterator<Item = (&EnvironmentVariableName, &CredentialReference)> {
        self.entries.iter()
    }

    /// The variable names alone — the part a receipt records.
    #[must_use]
    pub fn names(&self) -> Vec<EnvironmentVariableName> {
        self.entries.keys().cloned().collect()
    }

    /// Whether the map delivers nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Canonicalize for storage.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the map does not canonicalize — which for a
    /// map of validated names and aliases means only that it is oversized.
    pub fn to_document(&self) -> Result<CanonicalDocument, DomainError> {
        CanonicalDocument::from_serializable(&EnvironmentDocument {
            schema_version: ENVIRONMENT_SCHEMA,
            variables: self
                .entries
                .iter()
                .map(|(name, reference)| EnvironmentEntry {
                    name: name.clone(),
                    kind: reference.kind,
                    alias: reference.alias.clone(),
                })
                .collect(),
        })
    }

    /// Read a stored map back.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for an unknown schema version or a
    /// document that does not match the expected shape.
    pub fn from_document(document: &CanonicalDocument) -> Result<Self, DomainError> {
        let stored: EnvironmentDocument = document.deserialize()?;
        if stored.schema_version != ENVIRONMENT_SCHEMA {
            return Err(DomainError::invalid(
                "AccountEnvironmentMap",
                "was written under a schema version this binary does not read",
            ));
        }
        let mut entries = BTreeMap::new();
        for entry in stored.variables {
            if entries
                .insert(
                    entry.name,
                    CredentialReference {
                        kind: entry.kind,
                        alias: entry.alias,
                    },
                )
                .is_some()
            {
                return Err(DomainError::invalid(
                    "AccountEnvironmentMap",
                    "names the same environment variable twice",
                ));
            }
        }
        Ok(Self { entries })
    }
}

/// Everything a caller may choose when creating a profile.
///
/// Note what is absent: there is no field for a filesystem path, a keychain
/// service or account, an environment *value*, or a raw reference string. A
/// caller can only name an alias, and an alias resolves through
/// [`crate::ResolverPolicy`] or not at all — so a caller cannot widen what the
/// resolver is willing to look at, only pick from what an operator already
/// approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountProfileDraft {
    /// Owning project.
    pub project_id: ProjectId,
    /// Human label.
    pub label: ExternalName,
    /// The runtime family this account authenticates against.
    pub harness: RuntimeKindKey,
    /// The approved reference this account's credentials live behind.
    pub credential_ref: CredentialReference,
    /// Environment variable names mapped to approved references.
    pub environment: AccountEnvironmentMap,
    /// Non-secret routing metadata.
    pub routing: CanonicalDocument,
    /// Non-secret declared account capabilities.
    pub capability: CanonicalDocument,
    /// The external account id this profile authenticates as, if any.
    pub external_account_id: Option<ExternalId>,
    /// A non-secret provider identity hint, if the deployment records one.
    pub provider_identity: Option<ExternalId>,
}

/// Project-scoped account-profile CRUD over a repository port.
#[derive(Debug)]
pub struct AccountService<'store, R: ProjectRepository> {
    repository: &'store R,
}

impl<'store, R: ProjectRepository> AccountService<'store, R> {
    /// Bind the service to one repository.
    pub const fn new(repository: &'store R) -> Self {
        Self { repository }
    }

    /// Create a profile at [`AggregateRevision::INITIAL`].
    ///
    /// # Errors
    /// Returns [`AccountError::Domain`] for a map that will not canonicalize and
    /// [`AccountError::Repository`] for a duplicate id or an unknown project.
    pub fn create(
        &self,
        id: AccountProfileId,
        draft: &AccountProfileDraft,
        created_at: Timestamp,
    ) -> Result<AccountProfile, AccountError> {
        let request = NewAccountProfile {
            id,
            project_id: draft.project_id,
            label: draft.label.clone(),
            external_account_id: draft.external_account_id.clone(),
            harness: draft.harness.clone(),
            credential_ref: draft.credential_ref.clone(),
            environment: draft.environment.to_document()?,
            routing: draft.routing.clone(),
            capability: draft.capability.clone(),
            provider_identity: draft.provider_identity.clone(),
            enabled: true,
            created_at,
        };
        Ok(self.repository.create_account_profile(&request)?)
    }

    /// Read one profile, scoped to its project.
    ///
    /// # Errors
    /// Backend failures only; a profile from another project is `Ok(None)`.
    pub fn get(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
    ) -> Result<Option<AccountProfile>, AccountError> {
        Ok(self.repository.get_account_profile(project_id, id)?)
    }

    /// List one project's profiles.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list(&self, project_id: ProjectId) -> Result<Vec<AccountProfile>, AccountError> {
        Ok(self.repository.list_account_profiles(project_id)?)
    }

    /// Rename a profile under a compare-and-swap, leaving its enabled state.
    ///
    /// # Errors
    /// As [`AccountService::apply`].
    pub fn rename(
        &self,
        profile: &AccountProfile,
        label: ExternalName,
        now: Timestamp,
    ) -> Result<AccountProfile, AccountError> {
        self.apply(profile, label, profile.enabled, now)
    }

    /// Enable or disable a profile under a compare-and-swap.
    ///
    /// Disabling is the retirement path for a profile that runs still
    /// reference and that therefore cannot be deleted. It takes effect at the
    /// next launch check, *including* the recheck a launch performs after
    /// resolution — so a disable that lands during a slow keychain lookup still
    /// stops that launch.
    ///
    /// # Errors
    /// As [`AccountService::apply`].
    pub fn set_enabled(
        &self,
        profile: &AccountProfile,
        enabled: bool,
        now: Timestamp,
    ) -> Result<AccountProfile, AccountError> {
        self.apply(profile, profile.label.clone(), enabled, now)
    }

    /// Apply the only two changes a profile accepts.
    ///
    /// # Errors
    /// Returns [`AccountError::Repository`] with a
    /// [`DomainError::RevisionConflict`] when the stored revision moved, and
    /// [`RepositoryError::NotFound`] when the profile is not in this project. On
    /// refusal nothing is written.
    pub fn apply(
        &self,
        profile: &AccountProfile,
        label: ExternalName,
        enabled: bool,
        now: Timestamp,
    ) -> Result<AccountProfile, AccountError> {
        Ok(self
            .repository
            .update_account_profile(&AccountProfileUpdate {
                project_id: profile.project_id,
                id: profile.id,
                expected_revision: profile.revision,
                label,
                enabled,
                updated_at: now,
            })?)
    }

    /// Rotate an identity by creating a *new* profile and disabling the old one.
    ///
    /// This is the only supported way to change a harness, reference,
    /// environment map, routing, capability or provider identity, and it is
    /// deliberately not an update: the predecessor keeps its id, its revision
    /// history and every run that ever pinned it.
    ///
    /// # Errors
    /// Returns [`AccountError::ImmutableIdentity`] when the draft names another
    /// project, and otherwise as [`AccountService::create`] and
    /// [`AccountService::apply`].
    pub fn rotate(
        &self,
        predecessor: &AccountProfile,
        successor_id: AccountProfileId,
        draft: &AccountProfileDraft,
        now: Timestamp,
    ) -> Result<(AccountProfile, AccountProfile), AccountError> {
        if draft.project_id != predecessor.project_id {
            return Err(AccountError::ImmutableIdentity {
                subject: "owning project",
            });
        }
        let successor = self.create(successor_id, draft, now)?;
        let retired = self.set_enabled(predecessor, false, now)?;
        Ok((successor, retired))
    }

    /// Delete an *unreferenced* profile under a compare-and-swap.
    ///
    /// # Errors
    /// Returns [`AccountError::Repository`] with a
    /// [`RepositoryError::Conflict`] when any run, gate evaluation or override
    /// still names the profile — such a profile is disabled, never deleted.
    pub fn delete(
        &self,
        project_id: ProjectId,
        id: AccountProfileId,
        expected_revision: AggregateRevision,
    ) -> Result<(), AccountError> {
        Ok(self
            .repository
            .delete_account_profile(project_id, id, expected_revision)?)
    }
}
