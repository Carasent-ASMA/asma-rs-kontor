//! Reading one Codex account's quota headroom before it refuses.
//!
//! # Why this is a probe and not a parsed refusal
//!
//! Kontor already reads exhaustion out of a provider's own error text
//! ([`kontor_accounts::classify`]). That is *reactive*: it learns an account is
//! out at the moment a seat has already stopped on it. On 2026-08-21 both Codex
//! accounts hit their weekly allowance within a day of each other and every
//! Codex-pinned seat stopped, because nothing had asked either account how much
//! was left while there was still time to route elsewhere.
//!
//! This module asks. It reports headroom as a set of
//! [`kontor_core::quota::QuotaWindow`] values, which the scheduler's admission
//! pass judges against its declared thresholds — so an account at 85% of its
//! weekly window stops taking *new* seats while the ones already on it finish.
//!
//! # The one place that reads `auth.json`
//!
//! [`crate::adapter`] states, and keeps, a strict rule: it opens exactly one
//! file inside a config home — the operator's non-secret marker — and never a
//! credential. That rule is about the **launch** path, where the coding client
//! must stay the sole reader of its own credentials because nothing on that path
//! needs the token.
//!
//! A usage probe does need it, so this module is the deliberate, single
//! exception, and it is fenced the same way [`kontor_accounts`] fences a resolved
//! credential:
//!
//! * the token lands in a [`SecretString`], which is zeroized on drop;
//! * [`CodexUsageToken`] has no `Serialize`, and its `Debug` is redacted;
//! * the only exit is [`CodexUsageToken::authorization`], which builds one
//!   header value for one request;
//! * every failure is mapped to a closed reason before it is returned, so no
//!   `std::io::Error` or `reqwest::Error` — whose `Display` names a path, a host
//!   or a query — is ever wrapped.
//!
//! # Per-account, because that is the whole point
//!
//! The token is read from *the account's own* `CODEX_HOME`, never from an
//! ambient one. Two Codex logins are two directories, so this is also the probe
//! that finally answers whether the second account reports its own rate limits
//! or shares the first's: point it at each home in turn and compare. Until this
//! existed the question was unanswerable, and account-before-rung resolution
//! depends on the answer.

use std::fmt;

use kontor_core::DomainError;
use kontor_core::id::Timestamp;
use kontor_core::quota::{QuotaWindow, QuotaWindowKind};
use kontor_core::spec::ProviderQuotaKind;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

/// The endpoint that reports a ChatGPT-plan account's rate-limit windows.
///
/// Hard-coded rather than configured: it is not a deployment's choice, it is
/// where this vendor publishes the fact. A deployment that needs a different
/// host is talking to a different provider and needs a different probe.
pub const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

/// The credential file a Codex home keeps its OAuth tokens in.
pub const AUTH_FILE_NAME: &str = "auth.json";

// ---------------------------------------------------------------------------
// The token
// ---------------------------------------------------------------------------

/// One account's usage-probe credential, read from that account's own home.
pub struct CodexUsageToken {
    access_token: SecretString,
}

impl fmt::Debug for CodexUsageToken {
    /// Redacted, and deliberately says nothing about length either: a length is
    /// a fact about a secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexUsageToken")
            .field("access_token", &"<redacted>")
            .finish()
    }
}

/// The shape this module reads out of `auth.json`, and nothing more.
///
/// Every other key in that file — the refresh token, the id token, the account
/// id, the last-refresh instant — is deliberately not modelled. Deserializing
/// what is not needed is how a refresh token ends up in a struct that later
/// grows a `Serialize`.
#[derive(Deserialize)]
struct AuthFile {
    tokens: AuthTokens,
}

#[derive(Deserialize)]
struct AuthTokens {
    access_token: String,
}

impl CodexUsageToken {
    /// Read the access token out of one approved Codex home.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] with a closed reason when the home holds
    /// no readable `auth.json`, or when the file carries no access token. The
    /// underlying `std::io::Error` is dropped unread: its text names the path it
    /// failed on, and that path is the config home a refusal must not carry.
    pub fn read_from_home(home: &str) -> RuntimeResult<Self> {
        if home.is_empty() || !std::path::Path::new(home).is_absolute() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexUsageToken",
                "a usage probe needs an absolute config home to read the account's token from",
            )));
        }
        let path = std::path::Path::new(home).join(AUTH_FILE_NAME);
        let raw = std::fs::read_to_string(&path).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                "CodexUsageToken",
                "the approved config home holds no readable credential file",
            ))
        })?;
        // The parse error is dropped for the same reason: serde's message quotes
        // the input it choked on, which here is credential material.
        let parsed: AuthFile = serde_json::from_str(&raw).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                "CodexUsageToken",
                "the credential file in this config home is not in the expected shape",
            ))
        })?;
        if parsed.tokens.access_token.trim().is_empty() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "CodexUsageToken",
                "the credential file in this config home carries no access token",
            )));
        }
        Ok(Self {
            access_token: SecretString::from(parsed.tokens.access_token),
        })
    }

    /// The one `Authorization` header value this token produces.
    ///
    /// The only exit from the secret. It is handed straight to a request builder
    /// and is never logged, stored, returned to a caller or put in an error.
    #[must_use]
    pub fn authorization(&self) -> SecretString {
        SecretString::from(format!("Bearer {}", self.access_token.expose_secret()))
    }
}

// ---------------------------------------------------------------------------
// The payload
// ---------------------------------------------------------------------------

/// The rate-limit half of the usage payload — the only half this reads.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CodexUsage {
    /// The windows the account is subject to.
    pub rate_limits: CodexRateLimits,
}

/// The concurrent windows one account holds.
///
/// Two optional slots because that is what the vendor publishes. The *names* are
/// the vendor's layout and carry no meaning here — which window each slot holds
/// is decided by its `window_minutes`, never by whether it arrived as `primary`.
/// The active Pro profile was verified on 2026-08-06 reporting
/// `window_minutes: 10080` in `primary` and `secondary: null`; a reader that had
/// taken "primary" to mean anything would have recorded a weekly allowance as
/// whatever that slot happened to mean next quarter.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CodexRateLimits {
    /// The first slot, when the vendor populates it.
    #[serde(default)]
    pub primary: Option<CodexWindow>,
    /// The second slot, when the vendor populates it.
    #[serde(default)]
    pub secondary: Option<CodexWindow>,
}

/// One window, exactly as the vendor states it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CodexWindow {
    /// How much of the window is spent, as a percentage.
    pub used_percent: f64,
    /// How long the window is. This is what classifies it.
    pub window_minutes: u32,
    /// When it refills, as epoch seconds.
    ///
    /// Read from the **structured** field and never from prose. Verified
    /// 2026-08-05: the same refusal whose sentence read *"try again at Aug 30th,
    /// 2026 11:28 PM"* carried `resets_at: 1788121720`, which is
    /// `2026-08-30T20:28:40Z` — exactly one timezone offset apart. The prose is
    /// the vendor's local rendering; the number is the fact.
    ///
    /// Both attested spellings are accepted. A payload carrying only a *relative*
    /// span is deliberately not converted here: no such payload has been
    /// observed, and inventing `now + span` would put a guessed instant into a
    /// routing decision.
    #[serde(alias = "resets_at")]
    pub reset_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// What one probe concluded about an account's headroom on one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedHeadroom {
    /// The state to record on the `(account, provider)` row.
    pub state: ProviderQuotaKind,
    /// Every window the probe could classify, ordered by kind.
    pub windows: Vec<QuotaWindow>,
}

/// Turn one usage payload into recordable headroom.
///
/// Two outcomes, and the second is the one that matters:
///
/// * any window with a structured reset instant yields
///   [`ProviderQuotaKind::Available`] plus that window. "Available" here is a
///   statement about the *row*, not about the account having room — how much
///   room is left is the `used_percent` on each window, and whether that is
///   enough is the scheduler's threshold decision, not this module's.
/// * a payload with no classifiable window at all yields
///   [`ProviderQuotaKind::Unknown`], which fails closed. Deliberately **not**
///   [`ProviderQuotaKind::CannotReport`]: this endpoint exists precisely to
///   report headroom, so an answer with nothing in it means *this reading
///   failed*, not *this provider has no such number*. `CannotReport` is for a
///   provider that structurally cannot answer — OpenRouter's `:free` routes —
///   and using it here would mark a Codex account permanently usable on the
///   strength of a probe that told us nothing.
#[must_use]
pub fn classify_usage(usage: &CodexUsage) -> ObservedHeadroom {
    let mut windows: Vec<QuotaWindow> = [
        usage.rate_limits.primary.as_ref(),
        usage.rate_limits.secondary.as_ref(),
    ]
    .into_iter()
    .flatten()
    .filter_map(window_of)
    .collect();
    // Ordered by kind, and deduplicated on it: two readings of one kind is not a
    // richer observation, it is two readings one of which is stale. The store's
    // primary key refuses the second, so agreeing here keeps the probe's answer
    // and the stored row identical.
    windows.sort_by_key(|window| window.kind);
    windows.dedup_by_key(|window| window.kind);
    let state = if windows.is_empty() {
        ProviderQuotaKind::Unknown
    } else {
        ProviderQuotaKind::Available
    };
    ObservedHeadroom { state, windows }
}

/// One vendor window as a typed one, or nothing.
///
/// A window with no structured reset instant is dropped rather than given a
/// guessed one. A window is a span with an end; an allowance that cannot say
/// when it returns is not a window, and the caller sees that as the `Unknown`
/// state above.
fn window_of(window: &CodexWindow) -> Option<QuotaWindow> {
    let resets_at = Timestamp::from_second(window.reset_at?).ok()?;
    Some(QuotaWindow {
        kind: QuotaWindowKind::from_minutes(window.window_minutes),
        resets_at,
        // Rounded up, and clamped. Up because 70.1% spent must not read as 70%
        // when 70 is the threshold — the safe direction is to over-report
        // consumption, which stops admitting slightly early rather than slightly
        // late. Clamped because a percentage above 100 is a vendor bug, and
        // saturating it keeps a `u8` honest instead of wrapping to nearly zero.
        used_percent: window.used_percent.ceil().clamp(0.0, 100.0) as u8,
    })
}

// ---------------------------------------------------------------------------
// The transport seam
// ---------------------------------------------------------------------------

/// How a usage payload is fetched.
///
/// A seam for the same reason [`crate::CodexTransport`] is one: a test must be
/// able to prove the classification without a network, and the live
/// implementation must be the only thing that ever holds a token.
#[async_trait::async_trait]
pub trait CodexUsageProbe: Send + Sync + fmt::Debug {
    /// Read one account's usage, using the token from its own home.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the endpoint could not be
    /// reached or answered unusably. That is a fact about the channel and never
    /// about the account: an implementation must not turn a failed request into
    /// an empty payload, because an empty payload classifies as `Unknown` and
    /// `Unknown` blocks — a transport hiccup would silently park an account that
    /// had plenty of room.
    async fn usage(&self, home: &str) -> RuntimeResult<CodexUsage>;
}

/// The live probe.
#[derive(Debug)]
pub struct CodexLiveUsageProbe {
    client: reqwest::Client,
    endpoint: String,
}

impl CodexLiveUsageProbe {
    /// Build a probe against the vendor endpoint.
    #[must_use]
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            endpoint: USAGE_ENDPOINT.to_owned(),
        }
    }

    /// Build a probe against a stated endpoint, for a recorded-server test.
    ///
    /// Not a deployment knob: nothing reads this from configuration. It exists so
    /// the live request path itself — headers, status handling, body decoding —
    /// can be exercised against a local recording instead of the vendor.
    #[must_use]
    pub fn against(client: reqwest::Client, endpoint: String) -> Self {
        Self { client, endpoint }
    }
}

#[async_trait::async_trait]
impl CodexUsageProbe for CodexLiveUsageProbe {
    async fn usage(&self, home: &str) -> RuntimeResult<CodexUsage> {
        let token = CodexUsageToken::read_from_home(home)?;
        let response = self
            .client
            .get(&self.endpoint)
            // The secret's one exit, straight into this request's header block.
            .header(
                reqwest::header::AUTHORIZATION,
                token.authorization().expose_secret(),
            )
            .send()
            .await
            // Mapped to a closed reason: a `reqwest::Error`'s `Display` names the
            // host and can name the URL, and a refusal must carry neither.
            .map_err(|_| RuntimeError::Transport {
                rule: "the usage endpoint could not be reached",
            })?;
        if !response.status().is_success() {
            // The status is deliberately not reported either. A 401 says the
            // token this home holds is stale, which is an operator's problem, but
            // the refusal that carries it must not become a place to log which
            // account's token failed.
            return Err(RuntimeError::Transport {
                rule: "the usage endpoint refused the probe",
            });
        }
        response
            .json::<CodexUsage>()
            .await
            .map_err(|_| RuntimeError::Transport {
                rule: "the usage endpoint answered in an unreadable shape",
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(body: serde_json::Value) -> CodexUsage {
        serde_json::from_value(body).expect("a readable usage payload")
    }

    #[test]
    fn a_window_is_classified_by_its_span_and_not_by_the_slot_it_arrived_in() {
        // The verified Pro shape: one weekly window in `primary`, no secondary.
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": {
                    "used_percent": 28.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                },
                "secondary": null
            }
        })));
        assert_eq!(observed.state, ProviderQuotaKind::Available);
        assert_eq!(observed.windows.len(), 1);
        assert_eq!(observed.windows[0].kind, QuotaWindowKind::Weekly);
        assert_eq!(observed.windows[0].used_percent, 28);
        assert_eq!(
            observed.windows[0].resets_at,
            Timestamp::from_second(1_788_121_720).expect("a representable instant")
        );
    }

    #[test]
    fn a_weekly_window_in_the_secondary_slot_is_still_a_weekly_window() {
        // The same fact with the slots swapped must classify identically. This is
        // the mutant: a reader that maps `primary` to one kind and `secondary` to
        // another records the vendor's layout instead of the vendor's numbers.
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": null,
                "secondary": {
                    "used_percent": 28.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                }
            }
        })));
        assert_eq!(observed.windows.len(), 1);
        assert_eq!(observed.windows[0].kind, QuotaWindowKind::Weekly);
    }

    #[test]
    fn two_concurrent_windows_are_both_reported() {
        // The Claude-shaped case the multi-window state exists for: a five-hour
        // session window beside a weekly one.
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": {
                    "used_percent": 11.0,
                    "window_minutes": 300,
                    "reset_at": 1_788_000_000_i64
                },
                "secondary": {
                    "used_percent": 62.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                }
            }
        })));
        assert_eq!(
            observed
                .windows
                .iter()
                .map(|window| window.kind)
                .collect::<Vec<_>>(),
            vec![QuotaWindowKind::Session, QuotaWindowKind::Weekly],
            "an account holding two windows must report both, or the latest-reset \
             derivation has nothing to derive from"
        );
    }

    #[test]
    fn the_structured_instant_is_read_and_the_prose_spelling_is_also_accepted() {
        // Both attested spellings of the same structured field.
        for key in ["reset_at", "resets_at"] {
            let observed = classify_usage(&usage(serde_json::json!({
                "rate_limits": {
                    "primary": {
                        "used_percent": 1.0,
                        "window_minutes": 10080,
                        key: 1_788_121_720_i64
                    }
                }
            })));
            assert_eq!(
                observed.windows.first().map(|window| window.resets_at),
                Some(Timestamp::from_second(1_788_121_720).expect("an instant")),
                "{key} must be read as the structured reset"
            );
        }
    }

    #[test]
    fn a_window_without_a_structured_reset_is_dropped_and_fails_closed() {
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": { "used_percent": 99.0, "window_minutes": 10080 }
            }
        })));
        assert!(observed.windows.is_empty());
        assert_eq!(
            observed.state,
            ProviderQuotaKind::Unknown,
            "a probe that learned nothing must fail closed, never guess an instant"
        );
    }

    #[test]
    fn an_empty_payload_is_unknown_and_never_cannot_report() {
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": { "primary": null, "secondary": null }
        })));
        assert_eq!(
            observed.state,
            ProviderQuotaKind::Unknown,
            "this endpoint exists to report headroom, so an empty answer is a failed \
             reading and not a provider that structurally cannot answer"
        );
        assert_ne!(observed.state, ProviderQuotaKind::CannotReport);
    }

    #[test]
    fn consumption_is_rounded_up_so_a_threshold_is_never_crossed_unnoticed() {
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": {
                    "used_percent": 70.1,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                }
            }
        })));
        assert_eq!(
            observed.windows[0].used_percent, 71,
            "over-reporting consumption stops admitting slightly early; \
             under-reporting walks into the limit"
        );
    }

    #[test]
    fn a_vendor_percentage_out_of_range_is_clamped_rather_than_wrapped() {
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": {
                    "used_percent": 250.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                },
                "secondary": {
                    "used_percent": -5.0,
                    "window_minutes": 300,
                    "reset_at": 1_788_000_000_i64
                }
            }
        })));
        let by_kind = |kind| {
            observed
                .windows
                .iter()
                .find(|window| window.kind == kind)
                .expect("the window is present")
                .used_percent
        };
        assert_eq!(by_kind(QuotaWindowKind::Weekly), 100);
        assert_eq!(by_kind(QuotaWindowKind::Session), 0);
    }

    #[test]
    fn two_windows_of_one_kind_keep_only_one_reading() {
        // The store's primary key refuses the second row, so the probe must not
        // hand it two in the first place.
        let observed = classify_usage(&usage(serde_json::json!({
            "rate_limits": {
                "primary": {
                    "used_percent": 10.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_720_i64
                },
                "secondary": {
                    "used_percent": 90.0,
                    "window_minutes": 10080,
                    "reset_at": 1_788_121_999_i64
                }
            }
        })));
        assert_eq!(observed.windows.len(), 1);
    }

    // -----------------------------------------------------------------------
    // The token
    // -----------------------------------------------------------------------

    #[test]
    fn a_token_is_read_from_the_accounts_own_home() {
        let home = tempfile::tempdir().expect("a temporary home");
        std::fs::write(
            home.path().join(AUTH_FILE_NAME),
            r#"{"tokens":{"access_token":"tok-abc","refresh_token":"refresh-should-be-ignored"}}"#,
        )
        .expect("the fixture credential file is written");
        let token = CodexUsageToken::read_from_home(&home.path().to_string_lossy())
            .expect("the token reads back");
        assert_eq!(token.authorization().expose_secret(), "Bearer tok-abc");
    }

    #[test]
    fn a_token_never_appears_in_debug_output() {
        let home = tempfile::tempdir().expect("a temporary home");
        std::fs::write(
            home.path().join(AUTH_FILE_NAME),
            r#"{"tokens":{"access_token":"tok-secret-value"}}"#,
        )
        .expect("the fixture credential file is written");
        let token = CodexUsageToken::read_from_home(&home.path().to_string_lossy())
            .expect("the token reads back");
        let rendered = format!("{token:?}");
        assert!(
            !rendered.contains("tok-secret-value"),
            "a token must not travel in Debug output: {rendered}"
        );
    }

    #[test]
    fn a_refusal_never_names_the_config_home_it_failed_on() {
        let home = tempfile::tempdir().expect("a temporary home");
        let path = home.path().to_string_lossy().into_owned();
        // No `auth.json` at all.
        let error = CodexUsageToken::read_from_home(&path).expect_err("a refusal");
        let rendered = error.to_string();
        assert!(
            !rendered.contains(&path),
            "a refusal must carry a reason, never the path: {rendered}"
        );
    }

    #[test]
    fn a_relative_home_and_a_malformed_credential_file_are_both_refused() {
        assert!(CodexUsageToken::read_from_home("relative/home").is_err());
        assert!(CodexUsageToken::read_from_home("").is_err());

        let home = tempfile::tempdir().expect("a temporary home");
        std::fs::write(home.path().join(AUTH_FILE_NAME), "{\"tokens\":{}}")
            .expect("the fixture is written");
        assert!(CodexUsageToken::read_from_home(&home.path().to_string_lossy()).is_err());

        std::fs::write(
            home.path().join(AUTH_FILE_NAME),
            r#"{"tokens":{"access_token":"   "}}"#,
        )
        .expect("the fixture is written");
        assert!(
            CodexUsageToken::read_from_home(&home.path().to_string_lossy()).is_err(),
            "a blank token is not a token"
        );
    }
}
