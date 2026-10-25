//! The console's saved endpoint, kept in the operating system's credential store.
//!
//! One entry per profile: service [`SERVICE`], account `"{host}|{profile}"`,
//! holding the endpoint's base URL and realm bearer as one JSON value. The store
//! is the platform's own — Keychain Services on macOS, Credential Manager on
//! Windows, the Secret Service elsewhere — reached through the `keyring` crate.
//! This module writes no cryptography and keeps no file.
//!
//! The entry never leaves the machine. On Windows it is created with `Local`
//! persistence ([`entry_modifiers`]) rather than `keyring`'s default
//! `Enterprise`, which would roam the bearer with a roaming profile; `host` is
//! also the machine's name there. On macOS and Linux `host` is the constant
//! [`LOCAL_HOST`]: a name taken from the network could change between networks
//! and strand the entry. See [`machine_host`].
//!
//! On Linux, `keyring` looks for the Secret Service once per process. If none is
//! running at the first credential call, every call reports
//! [`CredentialError::Unavailable`] until the shell is restarted.
//!
//! Every failure is one of [`CredentialError`]'s kinds and nothing else: the
//! platform's own error text can name the service and account, so it is dropped,
//! and no value is ever logged. When the store cannot be used, the operation
//! fails; nothing falls back to a file or to another store.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The service every entry is filed under.
pub(crate) const SERVICE: &str = "kontor-console";

/// The profile used when the console names none.
const DEFAULT_PROFILE: &str = "default";

/// The longest profile name accepted.
const MAX_PROFILE_LEN: usize = 64;

/// The longest base URL accepted.
const MAX_BASE_URL_LEN: usize = 2048;

/// The longest bearer accepted. A realm's tier secret is 64 hex characters.
const MAX_TOKEN_LEN: usize = 4096;

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

/// The endpoint the console attaches to: the same shape as its `Endpoint`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Endpoint {
    /// The realm's loopback base URL.
    pub(crate) base_url: String,
    /// The realm bearer.
    pub(crate) token: String,
}

impl Endpoint {
    /// An absolute http(s) URL with a host and no userinfo, and a non-empty
    /// bearer of visible ASCII, both within their length caps.
    fn is_valid(&self) -> bool {
        let url_ok = self.base_url.len() <= MAX_BASE_URL_LEN
            && tauri::Url::parse(&self.base_url).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some_and(|host| !host.is_empty())
                    && url.username().is_empty()
                    && url.password().is_none()
            });
        let token_ok = !self.token.is_empty()
            && self.token.len() <= MAX_TOKEN_LEN
            && self.token.bytes().all(|b| b.is_ascii_graphic());
        url_ok && token_ok
    }
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

    /// Save `endpoint` for `profile`, replacing whatever was there.
    pub(crate) fn save(
        &self,
        profile: Option<&str>,
        endpoint: &Endpoint,
    ) -> Result<(), CredentialError> {
        let account = self.account(profile)?;
        if !endpoint.is_valid() {
            return Err(CredentialError::Invalid);
        }
        let value = serde_json::to_string(endpoint).map_err(|_| CredentialError::Invalid)?;
        self.backend.set(&account, &value)
    }

    /// The endpoint saved for `profile`.
    pub(crate) fn load(&self, profile: Option<&str>) -> Result<Endpoint, CredentialError> {
        let account = self.account(profile)?;
        let value = self.backend.get(&account)?;
        match serde_json::from_str::<Endpoint>(&value) {
            Ok(endpoint) if endpoint.is_valid() => Ok(endpoint),
            _ => Err(CredentialError::Invalid),
        }
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

/// The `host` of every account on a machine whose credentials do not roam.
pub(crate) const LOCAL_HOST: &str = "local";

/// The `host` part of this machine's accounts.
///
/// On Windows it is `COMPUTERNAME`, which the OS sets from the machine's own
/// name rather than from the network. Everywhere else it is [`LOCAL_HOST`].
/// Renaming a Windows machine moves the key: the old entry is then not found,
/// and the operator enters the endpoint once more.
pub(crate) fn machine_host() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").ok();
    #[cfg(not(windows))]
    let name: Option<String> = None;
    host_label(name.as_deref())
}

/// `name`'s first DNS label, lowercased, or [`LOCAL_HOST`] when there is none.
fn host_label(name: Option<&str>) -> String {
    let label = name
        .and_then(|name| name.split('.').next())
        .map(|label| label.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let usable = !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if usable { label } else { LOCAL_HOST.to_owned() }
}

/// The production backend: the operating system's credential store, under one
/// service — [`SERVICE`] in the shell.
pub(crate) struct OsKeychain {
    service: String,
}

impl OsKeychain {
    /// The OS store, filing entries under `service`.
    pub(crate) fn new(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }
}

impl SecretBackend for OsKeychain {
    fn get(&self, account: &str) -> Result<String, CredentialError> {
        entry(&self.service, account)?
            .get_password()
            .map_err(classify)
    }

    fn set(&self, account: &str, value: &str) -> Result<(), CredentialError> {
        entry(&self.service, account)?
            .set_password(value)
            .map_err(classify)
    }

    fn delete(&self, account: &str) -> Result<(), CredentialError> {
        entry(&self.service, account)?
            .delete_credential()
            .map_err(classify)
    }
}

fn entry(service: &str, account: &str) -> Result<keyring_core::Entry, CredentialError> {
    // `keyring` picks and installs the platform store once; the entry is then
    // built through `keyring-core` so it can carry store modifiers.
    if keyring::Entry::store_status().is_err() {
        return Err(CredentialError::Unavailable);
    }
    keyring_core::Entry::new_with_modifiers(service, account, &entry_modifiers()).map_err(classify)
}

/// The store modifiers every entry is built with on this platform.
pub(crate) fn entry_modifiers() -> HashMap<&'static str, &'static str> {
    modifiers_for(cfg!(windows))
}

/// The store modifiers for a Windows or a non-Windows store.
///
/// On Windows, `Local` persistence: the credential stays on this machine
/// instead of roaming with the profile. The other stores take none.
fn modifiers_for(windows: bool) -> HashMap<&'static str, &'static str> {
    if windows {
        HashMap::from([("persistence", "Local")])
    } else {
        HashMap::new()
    }
}

/// Map a `keyring` error to its kind, dropping its text.
fn classify(error: keyring::Error) -> CredentialError {
    use keyring::Error;
    match error {
        Error::NoEntry => CredentialError::NotFound,
        Error::NoStorageAccess(platform) => refine(platform.as_ref(), CredentialError::Locked),
        Error::PlatformFailure(platform) => refine(platform.as_ref(), CredentialError::Unavailable),
        Error::BadEncoding(_)
        | Error::BadDataFormat(..)
        | Error::BadStoreFormat(_)
        | Error::TooLong(..)
        | Error::Invalid(..)
        | Error::Ambiguous(_) => CredentialError::Invalid,
        _ => CredentialError::Unavailable,
    }
}

/// Refine a platform error by the reason the platform gives, else `fallback`.
///
/// Only macOS reports a reason this module can read: the OSStatus inside the
/// `security-framework` error that `keyring`'s Apple store wraps. That is the
/// same `security-framework` this crate links; the lockfile test
/// `one_security_framework_is_linked` fails if the two ever diverge.
#[cfg(target_os = "macos")]
fn refine(
    error: &(dyn std::error::Error + Send + Sync + 'static),
    fallback: CredentialError,
) -> CredentialError {
    match error
        .downcast_ref::<security_framework::base::Error>()
        .map(|e| e.code())
    {
        // errSecUserCanceled, errSecAuthFailed
        Some(-128 | -25293) => CredentialError::Denied,
        // Write permission (-61), errSecReadOnly, errSecInvalidOwnerEdit
        Some(-61 | -25292 | -25244) => CredentialError::Denied,
        // errSecInteractionNotAllowed: the keychain is locked and may not prompt.
        Some(-25308) => CredentialError::Locked,
        // errSecNotAvailable, errSecNoSuchKeychain, errSecInvalidKeychain
        Some(-25291 | -25294 | -25295) => CredentialError::Unavailable,
        _ => fallback,
    }
}

/// Refine a platform error by the reason the platform gives, else `fallback`.
#[cfg(not(target_os = "macos"))]
fn refine(
    _: &(dyn std::error::Error + Send + Sync + 'static),
    fallback: CredentialError,
) -> CredentialError {
    fallback
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
        assert_eq!(store.load(None), Err(CredentialError::NotFound));

        store.save(None, &endpoint(7717, "synthetic-one")).unwrap();
        assert_eq!(store.load(None), Ok(endpoint(7717, "synthetic-one")));

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
        let bad = |base_url: &str, token: &str| Endpoint {
            base_url: base_url.to_owned(),
            token: token.to_owned(),
        };
        let long_url = format!("http://127.0.0.1/{}", "a".repeat(MAX_BASE_URL_LEN));
        let long_token = "a".repeat(MAX_TOKEN_LEN + 1);
        for (base_url, token) in [
            ("", "synthetic"),
            ("127.0.0.1:7717", "synthetic"),
            ("ftp://127.0.0.1", "synthetic"),
            ("http://", "synthetic"),
            ("http://user:pw@127.0.0.1", "synthetic"),
            (long_url.as_str(), "synthetic"),
            ("http://127.0.0.1:7717", ""),
            ("http://127.0.0.1:7717", " "),
            ("http://127.0.0.1:7717", "two words"),
            ("http://127.0.0.1:7717", "line\nbreak"),
            ("http://127.0.0.1:7717", long_token.as_str()),
        ] {
            assert_eq!(
                store.save(None, &bad(base_url, token)),
                Err(CredentialError::Invalid),
                "{base_url:.40} / {} chars",
                token.len()
            );
        }
        assert_eq!(*backend.writes.borrow(), 0);

        backend
            .entries
            .borrow_mut()
            .insert("host-a|default".to_owned(), "not json".to_owned());
        assert_eq!(store.load(None), Err(CredentialError::Invalid));
        backend.entries.borrow_mut().insert(
            "host-a|default".to_owned(),
            r#"{"baseUrl":"http://127.0.0.1:1","token":""}"#.to_owned(),
        );
        assert_eq!(store.load(None), Err(CredentialError::Invalid));
        backend.entries.borrow_mut().insert(
            "host-a|default".to_owned(),
            r#"{"baseUrl":"file:///etc/passwd","token":"synthetic"}"#.to_owned(),
        );
        assert_eq!(store.load(None), Err(CredentialError::Invalid));
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
        assert_eq!(
            classify(Error::Ambiguous(Vec::new())),
            CredentialError::Invalid
        );
        assert_eq!(
            classify(Error::BadDataFormat(vec![0], platform())),
            CredentialError::Invalid
        );
        assert_eq!(
            classify(Error::BadStoreFormat("s".into())),
            CredentialError::Invalid
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_status_codes_refine_platform_failures() {
        use keyring::Error;
        let code = |code| Box::new(security_framework::base::Error::from_code(code));
        let failure = |c| classify(Error::PlatformFailure(code(c)));
        let no_access = |c| classify(Error::NoStorageAccess(code(c)));

        assert_eq!(failure(-128), CredentialError::Denied);
        assert_eq!(failure(-25293), CredentialError::Denied);
        assert_eq!(failure(-25308), CredentialError::Locked);
        assert_eq!(failure(-1), CredentialError::Unavailable);

        // The codes keyring files under NoStorageAccess, each mapped by meaning.
        for c in [-61, -25292, -25244] {
            assert_eq!(no_access(c), CredentialError::Denied, "{c}");
        }
        for c in [-25291, -25294, -25295] {
            assert_eq!(no_access(c), CredentialError::Unavailable, "{c}");
        }
        assert_eq!(no_access(-25308), CredentialError::Locked);
        assert_eq!(no_access(-1), CredentialError::Locked);
    }

    #[test]
    fn one_security_framework_is_linked() {
        // The macOS refinement downcasts keyring's wrapped error to this crate's
        // security-framework type. With two copies in the tree the downcast
        // would silently miss and every refined kind would fall back.
        let lock = include_str!("../../../../Cargo.lock");
        let copies = lock
            .lines()
            .filter(|line| *line == r#"name = "security-framework""#)
            .count();
        assert_eq!(copies, 1);
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
    fn host_label_is_stable() {
        assert_eq!(host_label(None), LOCAL_HOST);
        assert_eq!(host_label(Some("")), LOCAL_HOST);
        assert_eq!(host_label(Some("  ")), LOCAL_HOST);
        assert_eq!(host_label(Some("a|b")), LOCAL_HOST);
        assert_eq!(host_label(Some("DESKTOP-AB12")), "desktop-ab12");
        // A domain suffix never moves the key.
        assert_eq!(
            host_label(Some("DESKTOP-AB12.corp.example")),
            "desktop-ab12"
        );
        assert_eq!(host_label(Some("desktop-ab12.home")), "desktop-ab12");
        // Capitals and a `.local` suffix derive the same key as the bare name.
        assert_eq!(
            host_label(Some("Ops-MBP.local")),
            host_label(Some("ops-mbp"))
        );

        let backend = MockBackend::default();
        let upper = CredentialStore::new(&backend, &host_label(Some("Ops-MBP.local")));
        let lower = CredentialStore::new(&backend, &host_label(Some("ops-mbp")));
        upper.save(None, &endpoint(1, "synthetic")).unwrap();
        assert_eq!(lower.load(None), Ok(endpoint(1, "synthetic")));
    }

    #[test]
    fn entries_persist_locally() {
        // The Windows choice, checked on every OS. Never `Enterprise`,
        // keyring's default, which roams with the profile.
        assert_eq!(
            modifiers_for(true),
            HashMap::from([("persistence", "Local")])
        );
        assert!(modifiers_for(false).is_empty());
        assert_eq!(entry_modifiers(), modifiers_for(cfg!(windows)));
    }

    #[cfg(not(windows))]
    #[test]
    fn machine_host_ignores_the_network_name_off_windows() {
        assert_eq!(machine_host(), LOCAL_HOST);
    }

    #[cfg(windows)]
    #[test]
    fn machine_host_is_the_computer_name_on_windows() {
        let expected = host_label(std::env::var("COMPUTERNAME").ok().as_deref());
        assert_eq!(machine_host(), expected);
    }

    /// REAL OS store qualification, run by hand only (ASMA-8645 option C).
    ///
    /// Unlike every other test here this uses [`OsKeychain`], the production
    /// backend, through the production [`CredentialStore`]. It refuses to run
    /// unless `KONTOR_QUAL_SERVICE` starts with `kontor-console-qual-` and
    /// `KONTOR_QUAL_ACCOUNT` starts with `qual-`, so it can never address the
    /// shell's own [`SERVICE`]. Every value is synthetic, the secret is never
    /// printed, and a drop guard clears every entry it may have written.
    ///
    /// Run every step in this one process: a rebuilt binary is a new keychain
    /// identity and may be prompted for the entries the previous one wrote.
    #[test]
    #[ignore = "touches the real OS credential store; needs KONTOR_QUAL_SERVICE and KONTOR_QUAL_ACCOUNT"]
    fn real_store_qualification() {
        const QUAL_PREFIX: &str = "kontor-console-qual-";
        let service = std::env::var("KONTOR_QUAL_SERVICE").expect("KONTOR_QUAL_SERVICE");
        let host = std::env::var("KONTOR_QUAL_ACCOUNT").expect("KONTOR_QUAL_ACCOUNT");
        assert!(
            service.starts_with(QUAL_PREFIX) && service.len() > QUAL_PREFIX.len(),
            "refusing: the service must start with {QUAL_PREFIX}"
        );
        assert_ne!(service, SERVICE);
        assert!(
            host.starts_with("qual-") && !host.contains('|'),
            "refusing: the account prefix must start with qual- and hold no |"
        );

        let store = CredentialStore::new(OsKeychain::new(&service), &host);
        const PROFILES: [&str; 3] = ["default", "alt", "invalid"];
        struct Cleanup<'a>(&'a CredentialStore<OsKeychain>);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                for profile in PROFILES {
                    let outcome = self.0.clear(Some(profile));
                    println!("CLEANUP {profile}: {outcome:?}");
                }
            }
        }
        let _cleanup = Cleanup(&store);

        // 64 hex characters from this process's random hasher state.
        let token = || {
            use std::hash::{BuildHasher, Hasher};
            (0..4)
                .map(|_| {
                    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
                    hasher.write_u8(0);
                    format!("{:016x}", hasher.finish())
                })
                .collect::<String>()
        };
        let synthetic = |token: String| Endpoint {
            base_url: "https://qual.invalid".to_owned(),
            token,
        };
        let metadata = |profile: &str| {
            let output = std::process::Command::new("/usr/bin/security")
                .args([
                    "find-generic-password",
                    "-s",
                    &service,
                    "-a",
                    &format!("{host}|{profile}"),
                ])
                .output()
                .expect("run /usr/bin/security");
            // Without -g or -w, `security` prints attributes only, never the secret.
            println!(
                "METADATA {profile}: exit {:?}\n{}{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            output.status.code()
        };

        println!("SERVICE {service}\nACCOUNT PREFIX {host}|");
        assert_eq!(
            store.load(None),
            Err(CredentialError::NotFound),
            "no entry before the first write"
        );
        println!("STEP 0 load before write: not_found");

        let first = synthetic(token());
        store.save(None, &first).expect("save");
        assert_eq!(store.load(None).as_ref(), Ok(&first));
        println!("STEP 1 save then load: round trip equal");
        assert_eq!(metadata("default"), Some(0));

        let second = synthetic(token());
        assert_ne!(first, second);
        store.save(None, &second).expect("replace");
        assert_eq!(store.load(None).as_ref(), Ok(&second));
        println!("STEP 2 replace then load: the replacement");

        let alt = synthetic(token());
        store.save(Some("alt"), &alt).expect("save alt");
        assert_eq!(store.load(Some("alt")).as_ref(), Ok(&alt));
        assert_eq!(store.load(None).as_ref(), Ok(&second));
        store.clear(Some("alt")).expect("clear alt");
        assert_eq!(store.load(Some("alt")), Err(CredentialError::NotFound));
        assert_eq!(store.load(None).as_ref(), Ok(&second));
        println!(
            "STEP 3 profile separation: default and alt independent; clearing alt kept default"
        );

        for (label, endpoint) in [
            (
                "empty base_url",
                Endpoint {
                    base_url: String::new(),
                    token: token(),
                },
            ),
            (
                "non-http URL",
                Endpoint {
                    base_url: "file:///qual.invalid".to_owned(),
                    token: token(),
                },
            ),
            ("oversized token", synthetic("a".repeat(MAX_TOKEN_LEN + 1))),
        ] {
            assert_eq!(
                store.save(Some("invalid"), &endpoint),
                Err(CredentialError::Invalid),
                "{label}"
            );
            println!("STEP 4 invalid input ({label}): invalid");
        }
        assert_eq!(store.load(Some("invalid")), Err(CredentialError::NotFound));
        assert_eq!(metadata("invalid"), Some(44), "no write for invalid input");
        println!("STEP 4 invalid input: nothing written");

        store.clear(None).expect("clear");
        assert_eq!(store.load(None), Err(CredentialError::NotFound));
        assert_eq!(store.clear(None), Err(CredentialError::NotFound));
        assert_eq!(metadata("default"), Some(44));
        println!("STEP 5 clear then load: not_found; the entry is gone");
    }
}
