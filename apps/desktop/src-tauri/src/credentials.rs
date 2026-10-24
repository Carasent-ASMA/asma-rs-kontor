//! The console's saved endpoint, kept in the operating system's credential store.
//!
//! One entry per machine and profile: service [`SERVICE`], account
//! `"{host}|{profile}"`, holding the endpoint's base URL and realm bearer as one
//! JSON value. The store is the platform's own — Keychain Services on macOS,
//! Credential Manager on Windows, the Secret Service elsewhere — reached through
//! the `keyring` crate. This module writes no cryptography and keeps no file.
//!
//! The host is part of the account because a Windows credential roams with the
//! user's profile, while the endpoint it holds is a loopback address that only
//! means something on the machine that saved it.
//!
//! Every failure is one of [`CredentialError`]'s kinds and nothing else: the
//! platform's own error text can name the service and account, so it is dropped,
//! and no value is ever logged. When the store cannot be used, the operation
//! fails; nothing falls back to a file or to another store.

use serde::{Deserialize, Serialize};

/// The service every entry is filed under.
pub(crate) const SERVICE: &str = "kontor-console";

/// The profile used when the console names none.
const DEFAULT_PROFILE: &str = "default";

/// The longest profile name accepted.
const MAX_PROFILE_LEN: usize = 64;

/// Why a credential operation failed. Serialized as `{"kind": "<kind>"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CredentialError {
    /// There is no usable credential store on this machine.
    Unavailable,
    /// The store exists but is locked or otherwise refuses access for now.
    Locked,
    /// The operator, or the platform on their behalf, refused access.
    Denied,
    /// Nothing is saved for this machine and profile.
    NotFound,
    /// The request or the saved value is malformed.
    Invalid,
}

/// Whether anything is saved for a profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CredentialStatus {
    /// A well-formed endpoint is saved.
    Stored,
    /// Nothing is saved.
    Empty,
}

/// The endpoint the console attaches to: the same shape as its `Endpoint`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Endpoint {
    /// The realm's loopback base URL.
    pub(crate) base_url: String,
    /// The realm bearer.
    pub(crate) token: String,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Where entries are actually kept. The OS store in production; a fake in tests.
pub(crate) trait SecretBackend {
    /// Read the value of one entry.
    fn get(&self, account: &str) -> Result<String, CredentialError>;
    /// Create or replace one entry.
    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError>;
    /// Delete one entry.
    fn delete(&self, account: &str) -> Result<(), CredentialError>;
}

/// The saved-endpoint store for one machine.
pub(crate) struct CredentialStore<B> {
    backend: B,
    host: String,
}

impl<B: SecretBackend> CredentialStore<B> {
    /// A store for `host`, kept in `backend`.
    pub(crate) fn new(backend: B, host: &str) -> Self {
        Self {
            backend,
            host: host.to_owned(),
        }
    }

    /// Whether an endpoint is saved for `profile`.
    pub(crate) fn status(
        &self,
        profile: Option<&str>,
    ) -> Result<CredentialStatus, CredentialError> {
        match self.load(profile) {
            Ok(_) => Ok(CredentialStatus::Stored),
            Err(CredentialError::NotFound) => Ok(CredentialStatus::Empty),
            Err(error) => Err(error),
        }
    }

    /// Save `endpoint` for `profile`, replacing whatever was there.
    pub(crate) fn save(
        &self,
        profile: Option<&str>,
        endpoint: &Endpoint,
    ) -> Result<(), CredentialError> {
        let account = self.account(profile)?;
        if endpoint.base_url.trim().is_empty() || endpoint.token.trim().is_empty() {
            return Err(CredentialError::Invalid);
        }
        let value = serde_json::to_string(endpoint).map_err(|_| CredentialError::Invalid)?;
        self.backend.set(&account, &value)
    }

    /// The endpoint saved for `profile`.
    pub(crate) fn load(&self, profile: Option<&str>) -> Result<Endpoint, CredentialError> {
        let account = self.account(profile)?;
        let value = self.backend.get(&account)?;
        serde_json::from_str(&value).map_err(|_| CredentialError::Invalid)
    }

    /// Forget the endpoint saved for `profile`.
    pub(crate) fn clear(&self, profile: Option<&str>) -> Result<(), CredentialError> {
        let account = self.account(profile)?;
        self.backend.delete(&account)
    }

    fn account(&self, profile: Option<&str>) -> Result<String, CredentialError> {
        let profile = match profile.map(str::trim) {
            None | Some("") => DEFAULT_PROFILE,
            Some(name) => name,
        };
        let valid = profile.len() <= MAX_PROFILE_LEN
            && profile
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !valid {
            return Err(CredentialError::Invalid);
        }
        Ok(format!("{}|{profile}", self.host))
    }
}

/// This machine's name as it appears in the account: its first DNS label,
/// lowercased.
///
/// Only the first label is kept because macOS reports a node name whose domain
/// can change with the network (`name.local`, `name.home`), and the entry must
/// not move with it. Renaming the machine does move it: the old entry is then
/// not found, and the operator enters the endpoint once more.
pub(crate) fn machine_host() -> String {
    #[cfg(unix)]
    let raw = rustix::system::uname()
        .nodename()
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    let raw = std::env::var("COMPUTERNAME").unwrap_or_default();
    #[cfg(not(any(unix, windows)))]
    let raw = String::new();

    let label = raw
        .split('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if label.is_empty() {
        "localhost".to_owned()
    } else {
        label
    }
}

/// The production backend: the operating system's credential store.
pub(crate) struct OsKeychain;

impl SecretBackend for OsKeychain {
    fn get(&self, account: &str) -> Result<String, CredentialError> {
        entry(account)?.get_password().map_err(classify)
    }

    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError> {
        entry(account)?.set_password(value).map_err(classify)
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        entry(account)?.delete_credential().map_err(classify)
    }
}

fn entry(account: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(SERVICE, account).map_err(classify)
}

/// Map a `keyring` error to its kind, dropping its text.
fn classify(error: keyring::Error) -> CredentialError {
    use keyring::Error;
    match error {
        Error::NoEntry => CredentialError::NotFound,
        Error::NoStorageAccess(_) => CredentialError::Locked,
        Error::PlatformFailure(platform) => classify_platform(platform.as_ref()),
        Error::BadEncoding(_)
        | Error::BadDataFormat(..)
        | Error::BadStoreFormat(_)
        | Error::TooLong(..)
        | Error::Invalid(..)
        | Error::Ambiguous(_) => CredentialError::Invalid,
        _ => CredentialError::Unavailable,
    }
}

/// A platform failure, refined where the platform says why.
#[cfg(target_os = "macos")]
fn classify_platform(error: &(dyn std::error::Error + Send + Sync + 'static)) -> CredentialError {
    match error
        .downcast_ref::<security_framework::base::Error>()
        .map(|e| e.code())
    {
        // errSecUserCanceled, errSecAuthFailed
        Some(-128 | -25293) => CredentialError::Denied,
        // errSecInteractionNotAllowed: the keychain is locked and may not prompt.
        Some(-25308) => CredentialError::Locked,
        _ => CredentialError::Unavailable,
    }
}

/// A platform failure, refined where the platform says why.
#[cfg(not(target_os = "macos"))]
fn classify_platform(_: &(dyn std::error::Error + Send + Sync + 'static)) -> CredentialError {
    CredentialError::Unavailable
}

#[cfg(test)]
mod tests {
    //! These tests run against [`MockBackend`], an in-memory map. They prove this
    //! module's keying, validation, error mapping and refusal to fall back. They
    //! are not OS proof: no test here touches a real credential store.

    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// MOCK credential backend: an in-memory map, or a fixed failure.
    #[derive(Default)]
    struct MockBackend {
        entries: RefCell<HashMap<String, String>>,
        fail_with: Option<CredentialError>,
        writes: RefCell<usize>,
    }

    impl MockBackend {
        fn failing(error: CredentialError) -> Self {
            Self {
                fail_with: Some(error),
                ..Self::default()
            }
        }
    }

    impl SecretBackend for &MockBackend {
        fn get(&self, account: &str) -> Result<String, CredentialError> {
            if let Some(error) = self.fail_with {
                return Err(error);
            }
            self.entries
                .borrow()
                .get(account)
                .cloned()
                .ok_or(CredentialError::NotFound)
        }

        fn set(&self, account: &str, value: &str) -> Result<(), CredentialError> {
            *self.writes.borrow_mut() += 1;
            if let Some(error) = self.fail_with {
                return Err(error);
            }
            self.entries
                .borrow_mut()
                .insert(account.to_owned(), value.to_owned());
            Ok(())
        }

        fn delete(&self, account: &str) -> Result<(), CredentialError> {
            if let Some(error) = self.fail_with {
                return Err(error);
            }
            self.entries
                .borrow_mut()
                .remove(account)
                .map(|_| ())
                .ok_or(CredentialError::NotFound)
        }
    }

    /// A synthetic endpoint; never a real bearer.
    fn endpoint(port: u16, token: &str) -> Endpoint {
        Endpoint {
            base_url: format!("http://127.0.0.1:{port}"),
            token: token.to_owned(),
        }
    }

    #[test]
    fn save_load_replace_clear() {
        let backend = MockBackend::default();
        let store = CredentialStore::new(&backend, "host-a");
        assert_eq!(store.status(None), Ok(CredentialStatus::Empty));
        assert_eq!(store.load(None), Err(CredentialError::NotFound));

        store.save(None, &endpoint(7717, "synthetic-one")).unwrap();
        assert_eq!(store.load(None), Ok(endpoint(7717, "synthetic-one")));
        assert_eq!(store.status(None), Ok(CredentialStatus::Stored));

        store.save(None, &endpoint(7718, "synthetic-two")).unwrap();
        assert_eq!(store.load(None), Ok(endpoint(7718, "synthetic-two")));
        assert_eq!(
            backend.entries.borrow().len(),
            1,
            "a save replaces, never adds"
        );

        store.clear(None).unwrap();
        assert_eq!(store.load(None), Err(CredentialError::NotFound));
        assert_eq!(store.clear(None), Err(CredentialError::NotFound));
    }

    #[test]
    fn entries_are_keyed_by_host_and_profile() {
        let backend = MockBackend::default();
        let a = CredentialStore::new(&backend, "host-a");
        let b = CredentialStore::new(&backend, "host-b");

        a.save(None, &endpoint(1, "synthetic-a-default")).unwrap();
        a.save(Some("ops"), &endpoint(2, "synthetic-a-ops"))
            .unwrap();
        b.save(None, &endpoint(3, "synthetic-b-default")).unwrap();
        b.save(Some("ops"), &endpoint(4, "synthetic-b-ops"))
            .unwrap();

        let mut accounts: Vec<_> = backend.entries.borrow().keys().cloned().collect();
        accounts.sort();
        assert_eq!(
            accounts,
            [
                "host-a|default",
                "host-a|ops",
                "host-b|default",
                "host-b|ops"
            ]
        );
        assert_eq!(a.load(Some("")), Ok(endpoint(1, "synthetic-a-default")));
        assert_eq!(a.load(Some("ops")), Ok(endpoint(2, "synthetic-a-ops")));
        assert_eq!(
            b.load(Some("default")),
            Ok(endpoint(3, "synthetic-b-default"))
        );
        assert_eq!(b.load(Some("ops")), Ok(endpoint(4, "synthetic-b-ops")));

        a.clear(Some("ops")).unwrap();
        assert_eq!(a.load(Some("ops")), Err(CredentialError::NotFound));
        assert_eq!(b.load(Some("ops")), Ok(endpoint(4, "synthetic-b-ops")));
    }

    #[test]
    fn malformed_requests_and_values_are_invalid() {
        let backend = MockBackend::default();
        let store = CredentialStore::new(&backend, "host-a");
        for profile in ["a|b", "a b", "../x", &"p".repeat(MAX_PROFILE_LEN + 1)] {
            assert_eq!(
                store.load(Some(profile)),
                Err(CredentialError::Invalid),
                "{profile}"
            );
        }
        assert_eq!(
            store.save(None, &endpoint(1, " ")),
            Err(CredentialError::Invalid)
        );
        assert_eq!(*backend.writes.borrow(), 0);

        backend
            .entries
            .borrow_mut()
            .insert("host-a|default".to_owned(), "not json".to_owned());
        assert_eq!(store.load(None), Err(CredentialError::Invalid));
        assert_eq!(store.status(None), Err(CredentialError::Invalid));
    }

    #[test]
    fn backend_failures_surface_and_nothing_falls_back() {
        for error in [
            CredentialError::Unavailable,
            CredentialError::Locked,
            CredentialError::Denied,
        ] {
            let backend = MockBackend::failing(error);
            let store = CredentialStore::new(&backend, "host-a");
            assert_eq!(store.save(None, &endpoint(1, "synthetic")), Err(error));
            assert_eq!(store.load(None), Err(error));
            assert_eq!(store.status(None), Err(error));
            assert_eq!(store.clear(None), Err(error));
            // The one write went to the backend and was refused; there is no
            // other sink for it to land in.
            assert_eq!(*backend.writes.borrow(), 1);
            assert!(backend.entries.borrow().is_empty());
        }
    }

    #[test]
    fn keyring_errors_map_to_kinds() {
        use keyring::Error;
        let platform = || -> Box<dyn std::error::Error + Send + Sync> { "synthetic".into() };
        assert_eq!(classify(Error::NoEntry), CredentialError::NotFound);
        assert_eq!(
            classify(Error::NoStorageAccess(platform())),
            CredentialError::Locked
        );
        assert_eq!(
            classify(Error::PlatformFailure(platform())),
            CredentialError::Unavailable
        );
        assert_eq!(
            classify(Error::NoDefaultStore),
            CredentialError::Unavailable
        );
        assert_eq!(
            classify(Error::NotSupportedByStore("synthetic".into())),
            CredentialError::Unavailable
        );
        assert_eq!(
            classify(Error::BadEncoding(vec![0xff])),
            CredentialError::Invalid
        );
        assert_eq!(
            classify(Error::TooLong("user".into(), 1)),
            CredentialError::Invalid
        );
        assert_eq!(
            classify(Error::Invalid("p".into(), "r".into())),
            CredentialError::Invalid
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_status_codes_refine_platform_failures() {
        use keyring::Error;
        let status = |code| {
            Error::PlatformFailure(Box::new(security_framework::base::Error::from_code(code)))
        };
        assert_eq!(classify(status(-128)), CredentialError::Denied);
        assert_eq!(classify(status(-25293)), CredentialError::Denied);
        assert_eq!(classify(status(-25308)), CredentialError::Locked);
        assert_eq!(classify(status(-1)), CredentialError::Unavailable);
    }

    #[test]
    fn nothing_returned_or_printed_carries_the_token() {
        let token = "synthetic-secret-token";
        let printed = format!("{:?}", endpoint(1, token));
        assert!(!printed.contains(token), "{printed}");
        for error in [
            CredentialError::Unavailable,
            CredentialError::Locked,
            CredentialError::Denied,
            CredentialError::NotFound,
            CredentialError::Invalid,
        ] {
            let json = serde_json::to_string(&error).unwrap();
            assert!(
                json.starts_with(r#"{"kind":""#) && !json.contains(token),
                "{json}"
            );
        }
        assert_eq!(
            serde_json::to_string(&CredentialError::NotFound).unwrap(),
            r#"{"kind":"not_found"}"#
        );
    }

    #[test]
    fn machine_host_is_one_lowercase_label() {
        let host = machine_host();
        assert!(!host.is_empty());
        assert!(!host.contains('.') && !host.contains('|'));
        assert_eq!(host, host.to_ascii_lowercase());
    }
}
