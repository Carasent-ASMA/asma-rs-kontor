//! Stopped-realm credential installation and the connector's shared validator.

use std::io::Read;

use kontor_accounts::{KeychainTarget, KeychainWriter, SystemKeychain};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::{JiraError, UnavailableReason};

pub(crate) const KEYCHAIN_SERVICE: &str = "kontor-jira";
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
pub fn install_credentials(alias: &str, secret: SecretString) -> Result<(), JiraError> {
    install_credentials_with(alias, secret, &SystemKeychain)
}

/// The same installation boundary with an injected keychain for contract tests.
///
/// Validation precedes both effects. Success requires exact persisted bytes,
/// not only a successful writer call; mismatches and read failures are refused.
pub fn install_credentials_with(
    alias: &str,
    secret: SecretString,
    keychain: &dyn KeychainWriter,
) -> Result<(), JiraError> {
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
    let target = KeychainTarget::new(KEYCHAIN_SERVICE, alias.to_owned());
    keychain
        .set_secret(&target, &canonical)
        .map_err(|_| refused("the keychain credential could not be installed"))?;
    let observed = keychain
        .secret(&target)
        .map_err(|_| refused("the installed credential could not be read back"))?;
    if observed.expose_secret() != canonical.expose_secret() {
        return Err(refused("the installed credential readback does not match"));
    }
    parse_credentials(&observed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_accounts::{KeychainBackend, KeychainFailure};
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Store {
        value: Mutex<Option<SecretString>>,
        writes: AtomicUsize,
        reads: AtomicUsize,
        mismatched: bool,
        unreadable: bool,
        unwritable: bool,
    }
    impl KeychainBackend for Store {
        fn secret(&self, target: &KeychainTarget) -> Result<SecretString, KeychainFailure> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            assert_eq!(target.service(), "kontor-jira");
            assert_eq!(target.account(), "work");
            if self.unreadable {
                return Err(KeychainFailure::Denied);
            }
            if self.mismatched {
                return Ok(SecretString::from(
                    r#"{"email":"operator@example.test","api_token":"different-synthetic-canary"}"#,
                ));
            }
            self.value
                .lock()
                .unwrap()
                .clone()
                .ok_or(KeychainFailure::NotFound)
        }
    }
    impl KeychainWriter for Store {
        fn set_secret(
            &self,
            _target: &KeychainTarget,
            secret: &SecretString,
        ) -> Result<(), KeychainFailure> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            if self.unwritable {
                return Err(KeychainFailure::Denied);
            }
            *self.value.lock().unwrap() = Some(secret.clone());
            Ok(())
        }
    }
    fn valid() -> SecretString {
        SecretString::from(
            "{\n\"email\": \"operator@example.test\", \"api_token\": \"synthetic-canary\"\n}",
        )
    }
    #[test]
    fn a_valid_credential_is_installed_and_read_back() {
        let store = Store::default();
        install_credentials_with("work", valid(), &store).unwrap();
        assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        assert_eq!(store.reads.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn invalid_documents_and_aliases_have_no_keychain_effect() {
        let store = Store::default();
        for raw in [
            r#"{"email":"","api_token":"synthetic-canary"}"#,
            r#"{"email":"operator@example.test","api_token":" "}"#,
            r#"{"email":"operator@example.test","api_token":"synthetic-canary","extra":true}"#,
            r#"{"email":"operator@example.test","api_token":"synthetic-canary\ncommand"}"#,
        ] {
            let error =
                install_credentials_with("work", SecretString::from(raw), &store).unwrap_err();
            assert!(!format!("{error:?}").contains("synthetic-canary"));
        }
        for alias in ["", " ", "work\ncommand"] {
            assert!(install_credentials_with(alias, valid(), &store).is_err());
        }
        assert_eq!(store.writes.load(Ordering::SeqCst), 0);
        assert_eq!(store.reads.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn writer_failure_is_redacted_and_not_success() {
        let store = Store {
            unwritable: true,
            ..Store::default()
        };
        let error = install_credentials_with("work", valid(), &store).unwrap_err();
        assert!(!format!("{error:?}").contains("synthetic-canary"));
        assert_eq!(store.reads.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn a_writer_success_does_not_replace_matching_reader_proof() {
        for store in [
            Store {
                mismatched: true,
                ..Store::default()
            },
            Store {
                unreadable: true,
                ..Store::default()
            },
        ] {
            assert!(install_credentials_with("work", valid(), &store).is_err());
            assert_eq!(store.writes.load(Ordering::SeqCst), 1);
        }
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
