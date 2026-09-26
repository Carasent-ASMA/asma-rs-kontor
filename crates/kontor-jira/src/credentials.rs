//! Stopped-realm credential installation and the connector's shared validator.

use kontor_core::id::{ContentHash, RealmId};
use std::io::Read;
use std::path::Path;

use kontor_accounts::{KeychainFailure, KeychainTarget, KeychainWriter, SystemKeychain};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{JiraError, UnavailableReason};

/// Credential effects are isolated by immutable Realm and canonical state root.
/// A copied Realm database under another root cannot update the original root.
#[derive(Clone)]
pub struct JiraCredentialScope {
    service: String,
}

impl JiraCredentialScope {
    pub fn at(root: &Path, realm: RealmId) -> Result<Self, JiraError> {
        if !root.is_absolute() {
            return Err(refused("the credential root must be absolute"));
        }
        let root = root
            .canonicalize()
            .map_err(|_| refused("the credential root could not be canonicalized"))?;
        if !root.is_dir() {
            return Err(refused("the credential root must be a directory"));
        }
        let root_hash = ContentHash::of(root.as_os_str().as_encoded_bytes());
        Ok(Self {
            service: format!("kontor-jira:{realm}:{root_hash}"),
        })
    }

    pub(crate) fn target(&self, alias: &str) -> KeychainTarget {
        KeychainTarget::new(self.service.clone(), alias.to_owned())
    }
}

/// Static, redacted installation outcomes distinguish proven rollback from uncertainty.
#[derive(Debug, thiserror::Error)]
pub enum CredentialInstallError {
    #[error(transparent)]
    Invalid(#[from] JiraError),
    #[error("the previous credential state could not be established; no write attempted")]
    PreviousUnavailable,
    #[error("credential installation failed; the previous state was restored and verified")]
    RolledBack,
    #[error(
        "credential installation failed and rollback could not be proven; reconcile before restart"
    )]
    RollbackUnproven,
}
const MAX_DOCUMENT_BYTES: u64 = 8192;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct JiraCredentials {
    #[serde(deserialize_with = "secret_field")]
    pub(crate) email: SecretString,
    #[serde(deserialize_with = "secret_field")]
    pub(crate) api_token: SecretString,
}

fn secret_field<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SecretString, D::Error> {
    String::deserialize(deserializer).map(SecretString::from)
}

fn refused(detail: &'static str) -> JiraError {
    JiraError::unavailable("credential", UnavailableReason::Credential, detail)
}

pub(crate) fn validate_alias(alias: &str) -> Result<(), JiraError> {
    if alias.trim().is_empty() || alias.len() > 128 || alias.chars().any(char::is_control) {
        return Err(refused(
            "the credential alias is empty, oversized or unsupported",
        ));
    }
    Ok(())
}

pub(crate) fn parse_credentials(secret: &SecretString) -> Result<JiraCredentials, JiraError> {
    let invalid = || refused("the keychain credential is not the supported document");
    if secret.expose_secret().len() > MAX_DOCUMENT_BYTES as usize {
        return Err(invalid());
    }
    let credentials: JiraCredentials =
        serde_json::from_str(secret.expose_secret()).map_err(|_| invalid())?;
    let email = credentials.email.expose_secret();
    let token = credentials.api_token.expose_secret();
    if email.trim().is_empty()
        || email.len() > 320
        || email.chars().any(char::is_control)
        || token.trim().is_empty()
        || token.len() > 4096
        || token.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(credentials)
}

/// Read at most 8193 stdin bytes; refuse an oversized or malformed document.
///
/// The buffer is put under zeroizing ownership even on a failed read. Errors
/// never include input, parser diagnostics, or an underlying I/O message.
pub fn read_credential_document(reader: impl Read) -> Result<SecretString, JiraError> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refused("the credential document could not be read from standard input"))?;
    let document = match String::from_utf8(std::mem::take(&mut *bytes)) {
        Ok(document) => document,
        Err(error) => {
            error.into_bytes().zeroize();
            return Err(refused("the credential document is not supported text"));
        }
    };
    let secret = SecretString::from(document);
    parse_credentials(&secret)?;
    Ok(secret)
}

/// Install a strict stdin document and require the production reader's readback.
///
/// The caller must hold its state-root lock for this stopped-realm operation.
/// No credential, digest of a credential, or backend diagnostic is returned.
pub fn install_credentials(
    scope: &JiraCredentialScope,
    alias: &str,
    secret: SecretString,
) -> Result<(), CredentialInstallError> {
    install_credentials_with(scope, alias, secret, &SystemKeychain)
}

/// The same installation boundary with an injected keychain for contract tests.
///
/// Validation precedes both effects. Success requires exact persisted bytes,
/// not only a successful writer call; mismatches and read failures are refused.
pub fn install_credentials_with(
    scope: &JiraCredentialScope,
    alias: &str,
    secret: SecretString,
    keychain: &dyn KeychainWriter,
) -> Result<(), CredentialInstallError> {
    validate_alias(alias)?;
    let credentials = parse_credentials(&secret)?;
    #[derive(Serialize)]
    struct Document<'a> {
        email: &'a str,
        api_token: &'a str,
    }
    // Normalize only the operator input's JSON layout before OS persistence.
    // This is not evidence publication; raw native findings are never normalized.
    let canonical = SecretString::from(
        serde_json::to_string(&Document {
            email: credentials.email.expose_secret(),
            api_token: credentials.api_token.expose_secret(),
        })
        .map_err(|_| refused("the credential document could not be encoded"))?,
    );
    let target = scope.target(alias);
    let previous = match keychain.secret(&target) {
        Ok(secret) => Some(secret),
        Err(KeychainFailure::NotFound) => None,
        Err(_) => return Err(CredentialInstallError::PreviousUnavailable),
    };
    // A failed writer may already have changed the OS entry. Verify rollback
    // for writer failures as well as mismatched, malformed or denied readback.
    let installed = keychain.set_secret(&target, &canonical).is_ok()
        && keychain.secret(&target).is_ok_and(|observed| {
            observed.expose_secret() == canonical.expose_secret()
                && parse_credentials(&observed).is_ok()
        });
    if installed {
        return Ok(());
    }
    let restored = match &previous {
        Some(previous) => {
            keychain.set_secret(&target, previous).is_ok()
                && keychain
                    .secret(&target)
                    .is_ok_and(|observed| observed.expose_secret() == previous.expose_secret())
        }
        None => {
            keychain.delete_secret(&target).is_ok()
                && matches!(keychain.secret(&target), Err(KeychainFailure::NotFound))
        }
    };
    if restored {
        Err(CredentialInstallError::RolledBack)
    } else {
        Err(CredentialInstallError::RollbackUnproven)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_accounts::{KeychainBackend, KeychainFailure};
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    #[derive(Default)]
    enum ReadResult {
        #[default]
        Stored,
        Denied,
        Wrong,
        Malformed,
    }
    #[derive(Default)]
    struct Store {
        values: Mutex<BTreeMap<(String, String), SecretString>>,
        reads: Mutex<VecDeque<ReadResult>>,
        writes: Mutex<usize>,
        deletes: Mutex<usize>,
        fail_writes: Vec<usize>,
        fail_delete: bool,
    }
    fn key(target: &KeychainTarget) -> (String, String) {
        (target.service().to_owned(), target.account().to_owned())
    }
    impl KeychainBackend for Store {
        fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
            match self.reads.lock().unwrap().pop_front().unwrap_or_default() {
                ReadResult::Denied => Err(KeychainFailure::Denied),
                ReadResult::Wrong => Ok(SecretString::from(
                    r#"{"email":"operator@example.test","api_token":"different-synthetic-canary"}"#,
                )),
                ReadResult::Malformed => Ok(SecretString::from("synthetic-malformed-canary")),
                ReadResult::Stored => self
                    .values
                    .lock()
                    .unwrap()
                    .get(&key(target))
                    .cloned()
                    .ok_or(KeychainFailure::NotFound),
            }
        }
    }
    impl KeychainWriter for Store {
        fn set_secret(
            &self,
            target: &KeychainTarget,
            secret: &SecretString,
        ) -> Result<(), KeychainFailure> {
            let mut writes = self.writes.lock().unwrap();
            *writes += 1;
            // Even a failure may follow an effect, as with a transport timeout.
            self.values
                .lock()
                .unwrap()
                .insert(key(target), secret.clone());
            if self.fail_writes.contains(&*writes) {
                Err(KeychainFailure::Unavailable)
            } else {
                Ok(())
            }
        }
        fn delete_secret(&self, target: &KeychainTarget) -> Result<(), KeychainFailure> {
            *self.deletes.lock().unwrap() += 1;
            if self.fail_delete {
                return Err(KeychainFailure::Denied);
            }
            self.values.lock().unwrap().remove(&key(target));
            Ok(())
        }
    }
    fn valid() -> SecretString {
        SecretString::from(
            "{\n\"email\": \"operator@example.test\", \"api_token\": \"synthetic-canary\"\n}",
        )
    }
    fn scope(root: &Path) -> JiraCredentialScope {
        JiraCredentialScope::at(root, RealmId::generate()).unwrap()
    }
    fn previous() -> SecretString {
        SecretString::from("previous-synthetic-opaque-entry")
    }
    fn assert_redacted(error: &CredentialInstallError) {
        let diagnostic = format!("{error} {error:?}");
        for canary in [
            "synthetic-canary",
            "previous-synthetic",
            "different-synthetic",
            "synthetic-malformed",
        ] {
            assert!(!diagnostic.contains(canary));
        }
    }
    #[test]
    fn a_valid_credential_is_installed_and_read_back() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store::default();
        install_credentials_with(&scope, "work", valid(), &store).unwrap();
        assert_eq!(*store.writes.lock().unwrap(), 1);
        assert!(parse_credentials(&store.secret(&scope.target("work")).unwrap()).is_ok());
    }
    #[test]
    fn invalid_documents_and_aliases_have_no_keychain_effect() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store::default();
        for raw in [
            r#"{"email":"","api_token":"synthetic-canary"}"#,
            r#"{"email":"operator@example.test","api_token":" "}"#,
            r#"{"email":"operator@example.test","api_token":"synthetic-canary","extra":true}"#,
            r#"{"email":"operator@example.test","api_token":"synthetic-canary\ncommand"}"#,
        ] {
            let error = install_credentials_with(&scope, "work", SecretString::from(raw), &store)
                .unwrap_err();
            assert_redacted(&error);
        }
        for alias in ["", " ", "work\ncommand"] {
            assert!(install_credentials_with(&scope, alias, valid(), &store).is_err());
        }
        assert_eq!(*store.writes.lock().unwrap(), 0);
        assert_eq!(*store.deletes.lock().unwrap(), 0);
    }
    #[test]
    fn an_unreadable_previous_state_refuses_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store {
            reads: Mutex::new(VecDeque::from([ReadResult::Denied])),
            ..Store::default()
        };
        assert!(matches!(
            install_credentials_with(&scope, "work", valid(), &store),
            Err(CredentialInstallError::PreviousUnavailable)
        ));
        assert_eq!(*store.writes.lock().unwrap(), 0);
        assert_eq!(*store.deletes.lock().unwrap(), 0);
    }
    #[test]
    fn denied_mismatched_and_malformed_readback_restore_the_exact_previous_entry() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        for failure in [ReadResult::Denied, ReadResult::Wrong, ReadResult::Malformed] {
            let store = Store {
                reads: Mutex::new(VecDeque::from([
                    ReadResult::Stored,
                    failure,
                    ReadResult::Stored,
                ])),
                ..Store::default()
            };
            let old = previous();
            store
                .values
                .lock()
                .unwrap()
                .insert(key(&scope.target("work")), old.clone());
            let error = install_credentials_with(&scope, "work", valid(), &store).unwrap_err();
            assert!(matches!(error, CredentialInstallError::RolledBack));
            assert_redacted(&error);
            assert_eq!(
                store.secret(&scope.target("work")).unwrap().expose_secret(),
                old.expose_secret()
            );
            assert_eq!(*store.writes.lock().unwrap(), 2);
        }
    }
    #[test]
    fn a_failed_new_entry_is_deleted_and_absence_is_verified() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store {
            reads: Mutex::new(VecDeque::from([
                ReadResult::Stored,
                ReadResult::Denied,
                ReadResult::Stored,
            ])),
            ..Store::default()
        };
        assert!(matches!(
            install_credentials_with(&scope, "work", valid(), &store),
            Err(CredentialInstallError::RolledBack)
        ));
        assert!(matches!(
            store.secret(&scope.target("work")),
            Err(KeychainFailure::NotFound)
        ));
        assert_eq!(*store.deletes.lock().unwrap(), 1);
    }
    #[test]
    fn writer_failure_after_an_effect_also_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store {
            fail_writes: vec![1],
            ..Store::default()
        };
        let old = previous();
        store
            .values
            .lock()
            .unwrap()
            .insert(key(&scope.target("work")), old.clone());
        assert!(matches!(
            install_credentials_with(&scope, "work", valid(), &store),
            Err(CredentialInstallError::RolledBack)
        ));
        assert_eq!(
            store.secret(&scope.target("work")).unwrap().expose_secret(),
            old.expose_secret()
        );
    }
    #[test]
    fn rollback_write_delete_and_verification_failures_are_distinct_uncertainty() {
        let root = tempfile::tempdir().unwrap();
        let scope = scope(root.path());
        let store = Store {
            fail_writes: vec![1, 2],
            ..Store::default()
        };
        store
            .values
            .lock()
            .unwrap()
            .insert(key(&scope.target("work")), previous());
        let error = install_credentials_with(&scope, "work", valid(), &store).unwrap_err();
        assert!(matches!(error, CredentialInstallError::RollbackUnproven));
        assert_redacted(&error);
        let store = Store {
            fail_writes: vec![1],
            fail_delete: true,
            ..Store::default()
        };
        assert!(matches!(
            install_credentials_with(&scope, "work", valid(), &store),
            Err(CredentialInstallError::RollbackUnproven)
        ));
        let store = Store {
            reads: Mutex::new(VecDeque::from([
                ReadResult::Stored,
                ReadResult::Wrong,
                ReadResult::Denied,
            ])),
            ..Store::default()
        };
        store
            .values
            .lock()
            .unwrap()
            .insert(key(&scope.target("work")), previous());
        assert!(matches!(
            install_credentials_with(&scope, "work", valid(), &store),
            Err(CredentialInstallError::RollbackUnproven)
        ));
    }
    #[test]
    fn duplicate_aliases_in_other_roots_and_copied_realms_cannot_change_the_running_consumers_target()
     {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let realm = RealmId::generate();
        let a_scope = JiraCredentialScope::at(a.path(), realm).unwrap();
        let b_scope = JiraCredentialScope::at(b.path(), realm).unwrap();
        let c_scope = JiraCredentialScope::at(a.path(), RealmId::generate()).unwrap();
        assert_ne!(a_scope.target("work"), b_scope.target("work"));
        assert_ne!(a_scope.target("work"), c_scope.target("work"));
        let store = Store::default();
        let old = previous();
        store
            .values
            .lock()
            .unwrap()
            .insert(key(&a_scope.target("work")), old.clone());
        install_credentials_with(&b_scope, "work", valid(), &store).unwrap();
        assert_eq!(
            store
                .secret(&a_scope.target("work"))
                .unwrap()
                .expose_secret(),
            old.expose_secret()
        );
        assert!(parse_credentials(&store.secret(&b_scope.target("work")).unwrap()).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn canonical_symlink_roots_share_the_same_credential_address() {
        let root = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let link = parent.path().join("link");
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        let realm = RealmId::generate();
        assert_eq!(
            JiraCredentialScope::at(root.path(), realm)
                .unwrap()
                .target("work"),
            JiraCredentialScope::at(&link, realm)
                .unwrap()
                .target("work")
        );
        assert!(JiraCredentialScope::at(Path::new("relative"), realm).is_err());
    }
    #[test]
    fn stdin_is_bounded_and_read_errors_are_redacted() {
        assert!(
            read_credential_document(
                &b"{\"email\":\"operator@example.test\",\"api_token\":\"token\"}"[..]
            )
            .is_ok()
        );
        assert!(read_credential_document(&vec![b'x'; 8193][..]).is_err());
        assert!(read_credential_document(&b"\xff"[..]).is_err());
        let mut huge = std::io::Cursor::new(vec![b'x'; 20_000]);
        assert!(read_credential_document(&mut huge).is_err());
        assert_eq!(huge.position(), 8193);
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic-secret-canary"))
            }
        }
        let error = read_credential_document(Broken).unwrap_err();
        assert!(!format!("{error:?}").contains("synthetic-secret-canary"));
    }
}
