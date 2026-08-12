//! Who is calling, what they may do, and where they are allowed to call from.
//!
//! Three checks run before any handler, in this order, and each one can refuse
//! without the later ones having happened:
//!
//! 1. **Where from.** The `Host` header must name a loopback address, and a
//!    present `Origin` must be one this Realm was configured for. Both are
//!    checked because a loopback socket is not a loopback *caller*: a browser on
//!    the same machine will happily send a page's requests to `127.0.0.1`, and
//!    DNS rebinding turns an arbitrary hostname into one.
//! 2. **Who.** A `Bearer` credential, compared in constant time against the
//!    Realm's own secrets.
//! 3. **What.** The tier the matched secret was minted for.
//!
//! # Why three secrets rather than one
//!
//! The architecture asks for one bearer credential per Realm state root *and*
//! for an observer/operator/admin authority model. One secret cannot express
//! three authorities: whoever holds it holds all of them, and the tiers become
//! documentation. So the Realm's credential file holds one secret per tier,
//! generated together on first start. A read-only dashboard gets the observer
//! secret and cannot launch anything with it; only the admin secret reaches the
//! credential- and policy-authority routes.

use std::str::FromStr;

use axum::http::header::{AUTHORIZATION, HOST, HeaderMap, ORIGIN};
use axum::http::uri::Authority;
use kontor_core::closed_enum;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

closed_enum! {
    /// How much of the control plane one caller may reach.
    ///
    /// The ordering between tiers is [`CallerCapability::rank`] and nothing else,
    /// so reordering the variants cannot silently promote a caller.
    CallerCapability, "CallerCapability" {
        /// Read liveness, realm identity, snapshots, persisted events and session
        /// content.
        Observer => "observer",
        /// Everything an observer may do, plus control-plane writes, session
        /// messages and permission responses.
        Operator => "operator",
        /// Everything an operator may do, plus credential, account and
        /// policy-authority routes.
        Admin => "admin",
    }
}

impl CallerCapability {
    /// The explicit policy rank. Higher reaches more.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Observer => 0,
            Self::Operator => 1,
            Self::Admin => 2,
        }
    }

    /// Whether this tier reaches everything `required` reaches.
    #[must_use]
    pub const fn at_least(self, required: Self) -> bool {
        self.rank() >= required.rank()
    }
}

/// One generation of the Realm's three secrets.
#[derive(Debug)]
struct Secrets {
    observer: SecretString,
    operator: SecretString,
    admin: SecretString,
}

/// The Realm's bearer secrets, one per authority tier.
///
/// The values are [`SecretString`]s: they do not appear in `Debug`, they are
/// zeroized on drop, and the only way to read one is [`ExposeSecret`], which
/// happens in exactly one function in this crate.
///
/// # Why the set is behind a lock
///
/// Rotation replaces all three at once, in a running process, and it has to be
/// *atomic with respect to a request*: a caller must be authorized against one
/// whole generation, never against a mixture of the old admin secret and the
/// new operator one. A single lock over the set is what makes that true, and it
/// is why [`RealmCredentials::replace`] takes `&self` rather than `&mut self` —
/// the credentials live inside a shared `ApiState` that no caller can get a
/// unique borrow of.
#[derive(Debug)]
pub struct RealmCredentials {
    secrets: std::sync::RwLock<Secrets>,
}

impl RealmCredentials {
    /// Take ownership of three already-minted secrets.
    ///
    /// Minting them, writing them to a `0600` file and reading them back is the
    /// daemon's job. This crate never generates a credential and never learns
    /// where one is stored.
    #[must_use]
    pub const fn new(observer: SecretString, operator: SecretString, admin: SecretString) -> Self {
        Self {
            secrets: std::sync::RwLock::new(Secrets {
                observer,
                operator,
                admin,
            }),
        }
    }

    /// The tier a presented secret carries, or `None` when it is not one of
    /// this Realm's.
    ///
    /// Every tier is compared, and the comparison is constant time in the secret
    /// bytes: an attacker learns neither which tier a near-miss was closest to
    /// nor how many leading bytes it got right.
    #[must_use]
    pub fn authority(&self, presented: &str) -> Option<CallerCapability> {
        let held = self
            .secrets
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut granted = None;
        for (secret, capability) in [
            (&held.observer, CallerCapability::Observer),
            (&held.operator, CallerCapability::Operator),
            (&held.admin, CallerCapability::Admin),
        ] {
            if constant_time_eq(secret.expose_secret().as_bytes(), presented.as_bytes()) {
                granted = Some(capability);
            }
        }
        granted
    }

    /// Swap in a whole new generation of secrets.
    ///
    /// Every tier moves together, and the previous generation is dropped — and
    /// therefore zeroized — as this returns. From the next authorization
    /// onwards, every old token is simply not one of this Realm's secrets, which
    /// is the same answer an invented one gets: there is no revocation list to
    /// consult and no window in which an old token is "expiring".
    ///
    /// What this does *not* touch is deliberate. A native runtime session, its
    /// binding and its command receipts are identified by the Realm's own ids,
    /// not by the credential a client authenticated with, so rotation changes
    /// who may call in and changes nothing about what is already running.
    pub fn replace(&self, next: Self) {
        let replacement = next
            .secrets
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut held = self
            .secrets
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *held = replacement;
    }
}

/// Compare two byte strings without an early exit on the first difference.
///
/// The lengths are compared first and that *is* a leak — of the length, which is
/// fixed by the generator and therefore not secret. The bytes are not leaked.
fn constant_time_eq(expected: &[u8], presented: &[u8]) -> bool {
    if expected.len() != presented.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(presented) {
        difference |= left ^ right;
    }
    difference == 0
}

/// Which hosts and origins this Realm answers to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressPolicy {
    /// Origins that may be presented by a browser-hosted caller. An empty set
    /// means no `Origin` is acceptable at all.
    pub allowed_origins: Vec<String>,
}

impl Default for IngressPolicy {
    /// Loopback hostnames only, and the Tauri desktop shell's own origin.
    fn default() -> Self {
        Self {
            allowed_origins: vec![
                "tauri://localhost".to_owned(),
                "http://tauri.localhost".to_owned(),
                "https://tauri.localhost".to_owned(),
            ],
        }
    }
}

/// Why an ingress check refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressRefusal {
    /// The `Host` header is absent, unparseable or not a loopback name.
    Host,
    /// An `Origin` header was presented that this Realm does not answer to.
    Origin,
    /// No `Authorization: Bearer` credential was presented, or it is not one of
    /// this Realm's.
    Credential,
}

impl IngressPolicy {
    /// Check where a request claims to come from.
    ///
    /// # Errors
    /// Returns [`IngressRefusal::Host`] for a non-loopback or missing `Host`, and
    /// [`IngressRefusal::Origin`] for a presented origin outside the allowlist.
    /// A *missing* `Origin` is accepted: it is what a CLI, the MCP server and
    /// `curl` send, and a browser always supplies one for the cross-origin
    /// requests this check exists to stop.
    pub fn admit(&self, headers: &HeaderMap) -> Result<(), IngressRefusal> {
        let host = headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .ok_or(IngressRefusal::Host)?;
        if !is_loopback_host(host) {
            return Err(IngressRefusal::Host);
        }
        if let Some(origin) = headers.get(ORIGIN) {
            let origin = origin.to_str().map_err(|_| IngressRefusal::Origin)?;
            if !self.allowed_origins.iter().any(|allowed| allowed == origin) {
                return Err(IngressRefusal::Origin);
            }
        }
        Ok(())
    }
}

/// Whether a `Host` header names this machine, in a spelling that is well formed
/// all the way to its last byte.
///
/// Two independent things are checked, and skipping either one accepts a header
/// that should have been refused.
///
/// **The authority must be well formed.** `Host` is parsed as a URI authority
/// rather than split on the last colon. Splitting is what lets `evil.example:80`
/// arrive as a hostname nobody looked at, and it is also how a trailing-junk
/// value slips through.
///
/// **The authority must round-trip.** `Authority` is deliberately permissive
/// about what it keeps versus what it *parses*: every one of
///
/// | presented | `host()` | `port()` |
/// | --- | --- | --- |
/// | `localhost:bad` | `localhost` | `None` |
/// | `localhost:` | `localhost` | `None` |
/// | `localhost:99999` | `localhost` | `None` |
/// | `[::1]junk` | `[::1]` | `None` |
/// | `evil@127.0.0.1` | `127.0.0.1` | `None` |
///
/// parses successfully and reports a loopback host, with the malformed or hostile
/// remainder silently retained in the authority and dropped from `host()`. Reading
/// `host()` alone therefore *accepts every row of that table*. Rebuilding the
/// authority from the parts that were actually understood, and demanding it equal
/// what was presented, is what refuses them: anything the parser did not account
/// for changes the reassembly.
///
/// Beyond well-formedness, only the loopback spellings are accepted. A hostname
/// that happens to resolve to `127.0.0.1` today is exactly the DNS-rebinding case,
/// so it is refused however it resolves — and a decimal or hexadecimal spelling of
/// a loopback address (`2130706433`, `0x7f.1`) is refused too, because it is not a
/// form [`std::net::IpAddr`] accepts and a control plane should not be inventing a
/// second address grammar.
#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    let Some(name) = well_formed_authority(host) else {
        return false;
    };
    if name.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // A bracketed IPv6 literal is the authority spelling; the address parser wants
    // it bare.
    let literal = name
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(name);
    literal
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

/// The host of a `Host` header that is well formed in its entirety, or `None`.
///
/// Returns the host *as the authority spells it* — brackets included for an IPv6
/// literal — so the caller can tell the two address families apart without
/// re-parsing.
fn well_formed_authority(host: &str) -> Option<&str> {
    // Userinfo is not part of a `Host` header at all, and `Authority` would parse
    // it and hand back only what follows the `@`. Refusing it here keeps the
    // reassembly below comparing like with like.
    if host.is_empty() || host.contains('@') {
        return None;
    }
    let authority = Authority::from_str(host).ok()?;
    // Every byte the parser did not account for has to be absent, or the value was
    // not the authority it claimed to be.
    let understood = match authority.port_u16() {
        Some(port) => format!("{}:{port}", authority.host()),
        None => authority.host().to_owned(),
    };
    if understood != host {
        return None;
    }
    // Borrowed from the input rather than from the parsed authority, which is
    // dropped here; they are byte-equal by the check above.
    Some(&host[..authority.host().len()])
}

/// The credential presented in `Authorization: Bearer …`, if any.
#[must_use]
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_ordered_by_policy_and_not_by_declaration() {
        assert!(CallerCapability::Admin.at_least(CallerCapability::Operator));
        assert!(CallerCapability::Operator.at_least(CallerCapability::Observer));
        assert!(!CallerCapability::Observer.at_least(CallerCapability::Operator));
    }

    #[test]
    fn only_loopback_spellings_are_admitted() {
        for host in [
            "127.0.0.1",
            "127.0.0.1:7717",
            "localhost",
            "localhost:7717",
            // `Host` is case-insensitive, and refusing this spelling would be a
            // bug rather than caution.
            "LocalHost:7717",
            "[::1]",
            "[::1]:7717",
            "127.0.0.2:7717",
        ] {
            assert!(is_loopback_host(host), "{host} is a loopback authority");
        }
        for host in [
            "kontor.example.com",
            "10.0.0.4:7717",
            // Not loopback: the wildcards are *every* interface.
            "0.0.0.0:7717",
            "[::]:7717",
            "",
        ] {
            assert!(!is_loopback_host(host), "{host} is not loopback");
        }
    }

    #[test]
    fn a_malformed_authority_is_refused_however_loopback_it_looks() {
        // Every one of these parses as an authority whose `host()` is loopback,
        // and every one of them was accepted before the reassembly check existed.
        // They are the whole reason reading `host()` alone is not a validator.
        for host in [
            "localhost:bad",
            "localhost:",
            "127.0.0.1:",
            "127.0.0.1:x",
            // Out of range, so the port is not understood — and silently dropped.
            "localhost:99999",
            "127.0.0.1:65536",
            "[::1]junk",
            // Userinfo has no place in a `Host`, and the parser hands back only
            // what follows the `@`.
            "evil@127.0.0.1",
            "evil@localhost:7717",
        ] {
            assert!(
                !is_loopback_host(host),
                "{host} is malformed and must be refused whatever its host part parses to"
            );
        }
    }

    #[test]
    fn a_hostile_authority_is_refused_however_it_is_spelled() {
        for host in [
            // The rebinding shapes: a name that resolves wherever its owner likes.
            "127.0.0.1.evil.com",
            "localhost.evil.com",
            "evil.com:7717",
            // A second address grammar is not a second chance.
            "2130706433",
            "0x7f.0.0.1",
            "017700000001",
            // Structurally not an authority at all.
            "[::1",
            "::1",
            "127.0.0.1:7717/path",
            "localhost:7717?x=1",
            "localhost:7717#f",
            " 127.0.0.1",
            "127.0.0.1 ",
            "127.0.0.1:7717 ",
            ":7717",
            ":",
        ] {
            assert!(!is_loopback_host(host), "{host} must not reach a handler");
        }
    }

    #[test]
    fn a_malformed_host_is_refused_by_the_ingress_itself() {
        // The same rule, through the check the middleware actually calls, so a
        // future refactor cannot leave `is_loopback_host` correct and the ingress
        // reading something else.
        let policy = IngressPolicy::default();
        for host in ["localhost:bad", "evil@127.0.0.1", "[::1]junk", "evil.com"] {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, host.parse().expect("a header value"));
            assert_eq!(
                policy.admit(&headers),
                Err(IngressRefusal::Host),
                "{host} must be refused at the ingress"
            );
        }
        // And a missing `Host` is refused rather than defaulted.
        assert_eq!(
            policy.admit(&HeaderMap::new()),
            Err(IngressRefusal::Host),
            "a request with no Host claims to come from nowhere"
        );
    }

    #[test]
    fn a_secret_grants_exactly_its_own_tier() {
        let credentials = RealmCredentials::new(
            SecretString::from("observer-secret"),
            SecretString::from("operator-secret"),
            SecretString::from("admin-secret"),
        );
        assert_eq!(
            credentials.authority("observer-secret"),
            Some(CallerCapability::Observer)
        );
        assert_eq!(
            credentials.authority("admin-secret"),
            Some(CallerCapability::Admin)
        );
        assert_eq!(credentials.authority("observer-secre"), None);
        assert_eq!(credentials.authority(""), None);
    }

    #[test]
    fn a_presented_origin_must_be_allowed_and_a_missing_one_is_fine() {
        let policy = IngressPolicy::default();
        let mut headers = HeaderMap::new();
        headers.insert(HOST, "127.0.0.1:7717".parse().expect("a header value"));
        policy
            .admit(&headers)
            .expect("a loopback host with no origin");

        headers.insert(ORIGIN, "https://evil.example".parse().expect("a value"));
        assert_eq!(policy.admit(&headers), Err(IngressRefusal::Origin));

        headers.insert(ORIGIN, "tauri://localhost".parse().expect("a value"));
        policy
            .admit(&headers)
            .expect("the configured desktop origin");

        headers.insert(HOST, "evil.example".parse().expect("a value"));
        assert_eq!(policy.admit(&headers), Err(IngressRefusal::Host));
    }
}
